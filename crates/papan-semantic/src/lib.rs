//! Semantic analysis, type checking and lowering from the AST to the PCB IR.
//!
//! The semantic analyzer is the only component allowed to turn source text into
//! IR objects. It resolves symbols, validates references and units, lowers
//! declarations in a deterministic order and records `check`/`export`
//! directives as structured commands so the CLI and API do not need to re-parse
//! the source.

use std::collections::BTreeMap;

use papan_ast::{
    Block, ComponentDecl, ComponentKind, Item, ModuleDecl, NetDecl, PlaceTarget, Program, QuantityLit,
    RouteDecl, Value, ViaDecl,
};
use papan_geometry::Polygon;
use papan_ir::{
    Angle, ComponentInstance, Length, Net, NetKind, ObjectId, PcbIr, PinRef, Point, PropValue,
    Quantity, RouteDirective, Side, Stackup,
};
use papan_library::ComponentLibrary;
use papan_placement::{auto_place, board_anchor, pin_ref_position};
use papan_types::{Diagnostic, Diagnostics, Span, UnitKind};

/// A structured action requested by the source program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// `check DRC` / `check ERC`
    Check(String),
    /// `export gerber` / `export drill` / ...
    Export(String),
}

impl Command {
    pub fn name(&self) -> &str {
        match self {
            Command::Check(name) | Command::Export(name) => name,
        }
    }
}

/// Result of semantic analysis.
#[derive(Clone, Debug)]
pub struct CompileOutput {
    pub ir: PcbIr,
    pub diagnostics: Diagnostics,
    pub commands: Vec<Command>,
}

impl CompileOutput {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

/// Compile a parsed program into the PCB IR.
pub fn compile(program: &Program, library: &ComponentLibrary) -> CompileOutput {
    let mut lowering = Lowering {
        ir: PcbIr::new(match &program.board {
            Some(board) => board.name.clone(),
            None => "Untitled".to_string(),
        }),
        diagnostics: Diagnostics::new(),
        commands: Vec::new(),
        library,
        scopes: Vec::new(),
    };

    if program.board.is_none() {
        lowering.diagnostics.push(
            Diagnostic::warning("SEM_001", "source has no board declaration; using 'Untitled'")
                .with_suggestion("add `board \"Name\"`"),
        );
    }

    lowering.lower_top_level(&program.items);
    let placement_diagnostics = auto_place(&mut lowering.ir);
    lowering.diagnostics.extend(placement_diagnostics);
    lowering.diagnostics.sort();

    CompileOutput {
        ir: lowering.ir,
        diagnostics: lowering.diagnostics,
        commands: lowering.commands,
    }
}

struct Scope {
    prefix: String,
    components: BTreeMap<String, String>,
    nets: BTreeMap<String, String>,
}

impl Scope {
    fn root() -> Self {
        Self {
            prefix: String::new(),
            components: BTreeMap::new(),
            nets: BTreeMap::new(),
        }
    }

    fn child(&self, module: &ModuleDecl) -> Self {
        Self {
            prefix: format!("{}{}_", self.prefix, module.name),
            components: BTreeMap::new(),
            nets: BTreeMap::new(),
        }
    }
}

struct Lowering<'a> {
    ir: PcbIr,
    diagnostics: Diagnostics,
    commands: Vec<Command>,
    library: &'a ComponentLibrary,
    scopes: Vec<Scope>,
}

impl<'a> Lowering<'a> {
    fn lower_top_level(&mut self, items: &[Item]) {
        let scope = Scope::root();
        self.scopes.push(scope);
        self.register_scope(items);
        self.lower_items(items);
        self.scopes.pop();
    }

    fn lower_module(&mut self, module: &ModuleDecl) {
        let scope = self
            .scopes
            .last()
            .map(|parent| parent.child(module))
            .unwrap_or_else(|| {
                let mut scope = Scope::root();
                scope.prefix = format!("{}_", module.name);
                scope
            });
        self.scopes.push(scope);
        self.register_scope(&module.items);
        self.lower_items(&module.items);
        self.scopes.pop();
    }

    /// Register local component and net names before lowering so definitions
    /// may be referenced from statements that appear earlier in the source.
    fn register_scope(&mut self, items: &[Item]) {
        let Some(scope) = self.scopes.last_mut() else {
            return;
        };
        for item in items {
            match item {
                Item::Component(decl) => {
                    let full = format!("{}{}", scope.prefix, decl.name);
                    scope
                        .components
                        .insert(decl.name.to_ascii_lowercase(), full);
                }
                Item::Net(decl) => {
                    let full = format!("{}{}", scope.prefix, decl.name);
                    scope.nets.insert(decl.name.to_ascii_lowercase(), full);
                }
                _ => {}
            }
        }
    }

    fn lower_items(&mut self, items: &[Item]) {
        for item in items {
            match item {
                Item::Component(decl) => {
                    let full = self.resolve_component(&decl.name);
                    self.lower_component(decl, &full);
                }
                Item::Net(decl) => {
                    let full = self.resolve_net(&decl.name);
                    self.lower_net(decl, &full);
                }
                _ => {}
            }
        }
        for item in items {
            match item {
                Item::Size {
                    width,
                    height,
                    span,
                } => self.lower_size(width, height, *span),
                Item::Thickness { value, span } => self.lower_thickness(value, *span),
                Item::Layers { count, span } => self.lower_layers(*count, *span),
                Item::Connect { from, to, span } => self.lower_connect(from, to, *span),
                Item::Place {
                    component,
                    target,
                    span,
                } => self.lower_place(component, target, *span),
                Item::Rotate {
                    component,
                    angle,
                    span,
                } => self.lower_rotate(component, angle, *span),
                Item::Route(decl) => self.lower_route(decl),
                Item::Via(decl) => self.lower_via(decl),
                Item::Constraints(block) => self.lower_constraints(block),
                Item::Check { rule, span } => {
                    self.commands.push(Command::Check(rule.clone()));
                    let _ = span;
                }
                Item::Export { target, .. } => {
                    self.commands.push(Command::Export(target.clone()));
                }
                Item::Module(module) => self.lower_module(module),
                Item::Component(_) | Item::Net(_) | Item::ModulePort { .. } => {}
            }
        }
    }

    // ------------------------------------------------------------------
    // Name resolution
    // ------------------------------------------------------------------

    fn resolve_component(&self, name: &str) -> String {
        let lowered = name.to_ascii_lowercase();
        for scope in self.scopes.iter().rev() {
            if let Some(full) = scope.components.get(&lowered) {
                return full.clone();
            }
        }
        name.to_string()
    }

    fn resolve_net(&self, name: &str) -> String {
        let lowered = name.to_ascii_lowercase();
        for scope in self.scopes.iter().rev() {
            if let Some(full) = scope.nets.get(&lowered) {
                return full.clone();
            }
        }
        name.to_string()
    }

    // ------------------------------------------------------------------
    // Board settings
    // ------------------------------------------------------------------

    fn lower_size(&mut self, width: &QuantityLit, height: &QuantityLit, span: Span) {
        let Some(width_value) = length_of(width) else {
            self.diagnostics.push(
                Diagnostic::error("SEM_002", "board width must be a length")
                    .with_span(width.span)
                    .with_suggestion("use e.g. `size 100mm x 80mm`"),
            );
            return;
        };
        let Some(height_value) = length_of(height) else {
            self.diagnostics.push(
                Diagnostic::error("SEM_002", "board height must be a length")
                    .with_span(height.span)
                    .with_suggestion("use e.g. `size 100mm x 80mm`"),
            );
            return;
        };
        if !width_value.is_positive() || !height_value.is_positive() {
            self.diagnostics.push(
                Diagnostic::error("SEM_003", "board size must be positive").with_span(span),
            );
            return;
        }
        self.ir.board.width = width_value;
        self.ir.board.height = height_value;
        self.ir.board.outline = Polygon::rect(width_value, height_value);
    }

    fn lower_thickness(&mut self, value: &QuantityLit, span: Span) {
        let Some(thickness) = length_of(value) else {
            self.diagnostics.push(
                Diagnostic::error("SEM_004", "board thickness must be a length")
                    .with_span(value.span)
                    .with_suggestion("use e.g. `thickness 1.6mm`"),
            );
            return;
        };
        if !thickness.is_positive() {
            self.diagnostics
                .push(Diagnostic::error("SEM_005", "board thickness must be positive").with_span(span));
            return;
        }
        self.ir.board.thickness = thickness;
    }

    fn lower_layers(&mut self, count: u32, span: Span) {
        if count == 0 || count > 64 {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_006",
                    format!("layer count must be between 1 and 64, found {count}"),
                )
                .with_span(span),
            );
            return;
        }
        self.ir.board.layer_count = count;
        self.ir.stackup = Stackup::for_copper_count(count);
    }

    // ------------------------------------------------------------------
    // Components and nets
    // ------------------------------------------------------------------

    fn lower_component(&mut self, decl: &ComponentDecl, full_name: &str) {
        if self.ir.find_component(full_name).is_some() {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_010",
                    format!("duplicate component designator '{full_name}'"),
                )
                .with_span(decl.span),
            );
            return;
        }

        let mut definition = match &decl.kind {
            ComponentKind::Named(kind) => match self.library.resolve_kind(kind) {
                Some(definition) => definition.clone(),
                None => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "SEM_011",
                            format!("unknown component kind '{kind}'"),
                        )
                        .with_span(decl.span)
                        .with_suggestion("use a quoted library part, e.g. component U1: \"ATmega32U4\""),
                    );
                    return;
                }
            },
            ComponentKind::Part(part) => match self.library.resolve_part(part) {
                Some(definition) => definition.clone(),
                None => {
                    self.diagnostics.push(
                        Diagnostic::warning(
                            "SEM_012",
                            format!(
                                "unknown library part '{part}'; using a deterministic generic stub"
                            ),
                        )
                        .with_span(decl.span)
                        .with_suggestion("add the part to the component library for a real footprint"),
                    );
                    self.library.generic_part(part, 2)
                }
            },
        };
        definition.id = ObjectId::stable("part", &definition.name);
        self.ir.add_definition(definition.clone());

        let mut properties = BTreeMap::new();
        for property in &decl.properties {
            properties.insert(property.key.to_ascii_lowercase(), property_to_prop(&property.value));
        }

        let mut value = decl.value.as_ref().and_then(|literal| literal.value);
        if let Some(PropValue::Quantity(property_value)) = properties.get("value") {
            value = Some(*property_value);
        }
        if let (Some(value), Some(expected)) = (value, definition.category.expected_value_kind()) {
            if value.kind != expected {
                self.diagnostics.push(
                    Diagnostic::error(
                        "SEM_013",
                        format!(
                            "component '{full_name}' expects a {} value, found {}",
                            expected.label(),
                            value.kind.label()
                        ),
                    )
                    .with_span(decl.span)
                    .with_object(full_name),
                );
            }
        }

        let side = match properties
            .get("side")
            .and_then(PropValue::as_str)
            .map(|value| value.to_ascii_lowercase())
        {
            Some(value) if value == "bottom" || value == "back" => Side::Bottom,
            _ => Side::Top,
        };
        let locked = matches!(properties.get("locked"), Some(PropValue::Bool(true)));

        self.ir.components.push(ComponentInstance {
            id: ObjectId::stable("component", full_name),
            designator: full_name.to_string(),
            definition_id: definition.id.clone(),
            value,
            properties,
            position: None,
            rotation: Angle::ZERO,
            side,
            locked,
            source_span: Some(decl.span),
        });
    }

    fn lower_net(&mut self, decl: &NetDecl, full_name: &str) {
        if self.ir.find_net(full_name).is_some() {
            self.diagnostics.push(
                Diagnostic::error("SEM_020", format!("duplicate net name '{full_name}'"))
                    .with_span(decl.span),
            );
            return;
        }
        let mut kind = NetKind::Other;
        let mut voltage = None;
        let mut properties = BTreeMap::new();
        for property in &decl.properties {
            let lowered = property.key.to_ascii_lowercase();
            match lowered.as_str() {
                "type" => {
                    let text = match &property.value {
                        Value::Ident(value) | Value::String(value) => value.to_ascii_lowercase(),
                        _ => String::new(),
                    };
                    kind = match text.as_str() {
                        "power" => NetKind::Power,
                        "ground" | "gnd" => NetKind::Ground,
                        "digital" => NetKind::Digital,
                        "analog" => NetKind::Analog,
                        _ => {
                            self.diagnostics.push(
                                Diagnostic::warning(
                                    "SEM_021",
                                    format!("unknown net type '{}' on net '{full_name}'", text),
                                )
                                .with_span(property.span)
                                .with_suggestion("use power, ground, digital or analog"),
                            );
                            NetKind::Other
                        }
                    };
                }
                "voltage" => {
                    let quantity = quantity_of(&property.value);
                    match quantity {
                        Some(quantity) if quantity.kind == UnitKind::Voltage => voltage = Some(quantity),
                        Some(quantity) => self.diagnostics.push(
                            Diagnostic::error(
                                "SEM_022",
                                format!(
                                    "net '{full_name}' voltage must be a voltage, found {}",
                                    quantity.kind.label()
                                ),
                            )
                            .with_span(property.span),
                        ),
                        None => {}
                    }
                }
                _ => {}
            }
            properties.insert(lowered, property_to_prop(&property.value));
        }
        self.ir.add_net(Net {
            id: ObjectId::stable("net", full_name),
            name: full_name.to_string(),
            kind,
            voltage,
            members: Vec::new(),
            properties,
            source_span: Some(decl.span),
        });
    }

    // ------------------------------------------------------------------
    // Connectivity
    // ------------------------------------------------------------------

    fn lower_connect(&mut self, from: &[String], to: &[String], span: Span) {
        if from.len() < 2 {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_030",
                    "connection source must be `Component.Pin`",
                )
                .with_span(span),
            );
            return;
        }
        if to.is_empty() {
            self.diagnostics.push(
                Diagnostic::error("SEM_031", "connection target must be a net name").with_span(span),
            );
            return;
        }
        let component_name = self.resolve_component(&from[0]);
        let pin_name = from[1..].join(".");
        let net_name = self.resolve_net(&to.join("."));

        let Some(component) = self.ir.find_component(&component_name) else {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_032",
                    format!("connection references unknown component '{component_name}'"),
                )
                .with_span(span),
            );
            return;
        };
        if self.ir.find_definition(&component.definition_id).is_none() {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_033",
                    format!("component '{component_name}' has no component definition"),
                )
                .with_span(span),
            );
            return;
        }
        let pin_definition = self
            .ir
            .definition_for(component)
            .and_then(|definition| definition.pin(&pin_name))
            .cloned();
        let Some(pin_definition) = pin_definition else {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_034",
                    format!("component '{component_name}' has no pin '{pin_name}'"),
                )
                .with_span(span)
                .with_suggestion("check the pin map of the component definition"),
            );
            return;
        };
        let Some(net) = self.ir.find_net(&net_name) else {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_035",
                    format!("connection references unknown net '{net_name}'"),
                )
                .with_span(span)
                .with_suggestion("declare the net before connecting to it"),
            );
            return;
        };
        let net_id = net.id.clone();

        if pin_definition.electrical == papan_ir::PinType::NoConnect {
            self.diagnostics.push(
                Diagnostic::warning(
                    "SEM_036",
                    format!("pin {component_name}.{pin_name} is marked no-connect but is connected"),
                )
                .with_span(span)
                .with_object(&component.id.0),
            );
        }

        let component_id = component.id.clone();
        let pin = PinRef::new(component_id.clone(), pin_name);
        if self.ir.connect(pin, net_id, Some(span)).is_none() {
            self.diagnostics.push(
                Diagnostic::error("SEM_037", "failed to record connection")
                    .with_span(span)
                    .with_object(&component_id.0),
            );
        }
    }

    // ------------------------------------------------------------------
    // Placement
    // ------------------------------------------------------------------

    fn lower_place(&mut self, component_name: &str, target: &PlaceTarget, span: Span) {
        let full_name = self.resolve_component(component_name);
        let Some(component) = self.ir.find_component(&full_name) else {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_040",
                    format!("placement references unknown component '{full_name}'"),
                )
                .with_span(span),
            );
            return;
        };
        let position = match target {
            PlaceTarget::At { x, y } => {
                let Some(x) = length_of(x) else {
                    self.diagnostics.push(
                        Diagnostic::error("SEM_041", "placement x must be a length")
                            .with_span(x.span),
                    );
                    return;
                };
                let Some(y) = length_of(y) else {
                    self.diagnostics.push(
                        Diagnostic::error("SEM_042", "placement y must be a length")
                            .with_span(y.span),
                    );
                    return;
                };
                Some(Point::new(x, y))
            }
            PlaceTarget::Anchor(path) => match board_anchor(path, self.ir.board.width, self.ir.board.height)
            {
                Some(position) => Some(position),
                None => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "SEM_043",
                            format!("unknown board anchor '{}'", path.join(".")),
                        )
                        .with_span(span)
                        .with_suggestion("use e.g. board.top.center or board.bottom.left"),
                    );
                    None
                }
            },
            PlaceTarget::Near {
                reference,
                distance,
            } => {
                let Some(distance) = length_of(distance) else {
                    self.diagnostics.push(
                        Diagnostic::error("SEM_044", "relative placement distance must be a length")
                            .with_span(span),
                    );
                    return;
                };
                let reference_point = if reference.is_empty() {
                    None
                } else {
                    let reference_component = self.resolve_component(&reference[0]);
                    if reference.len() == 1 {
                        self.ir
                            .find_component(&reference_component)
                            .and_then(|component| component.position)
                    } else {
                        let pin = PinRef::new(
                            ObjectId::stable("component", &reference_component),
                            reference[1..].join("."),
                        );
                        pin_ref_position(&self.ir, &pin)
                    }
                };
                match reference_point {
                    Some(point) => Some(Point::new(point.x + distance, point.y)),
                    None => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "SEM_045",
                                format!(
                                    "relative placement reference '{}' could not be resolved",
                                    reference.join(".")
                                ),
                            )
                            .with_span(span),
                        );
                        None
                    }
                }
            }
        };
        if let Some(position) = position {
            let clamped = clamp_to_board(position, self.ir.board.width, self.ir.board.height);
            if clamped != position {
                self.diagnostics.push(
                    Diagnostic::warning(
                        "SEM_046",
                        format!(
                            "placement of '{full_name}' was clamped into the board outline"
                        ),
                    )
                    .with_span(span)
                    .with_object(&component.id.0)
                    .with_location(clamped),
                );
            }
            if let Some(component) = self.ir.find_component_mut(&full_name) {
                component.position = Some(clamped);
            }
        }
    }

    fn lower_rotate(&mut self, component_name: &str, angle: &QuantityLit, span: Span) {
        let full_name = self.resolve_component(component_name);
        let Some(component) = self.ir.find_component(&full_name) else {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_047",
                    format!("rotation references unknown component '{full_name}'"),
                )
                .with_span(span),
            );
            return;
        };
        let Some(value) = angle_of(angle) else {
            self.diagnostics.push(
                Diagnostic::error("SEM_048", "rotation angle must be an angle")
                    .with_span(span)
                    .with_suggestion("use e.g. `rotate MCU 90deg`"),
            );
            return;
        };
        let id = component.id.clone();
        if let Some(component) = self.ir.find_component_by_id_mut(&id) {
            component.rotation = value.normalized();
        }
    }

    // ------------------------------------------------------------------
    // Routing directives
    // ------------------------------------------------------------------

    fn lower_route(&mut self, decl: &RouteDecl) {
        let net_name = self.resolve_net(&decl.net);
        let Some(net) = self.ir.find_net(&net_name) else {
            self.diagnostics.push(
                Diagnostic::warning(
                    "SEM_050",
                    format!("route directive references unknown net '{net_name}'"),
                )
                .with_span(decl.span)
                .with_suggestion("declare the net before adding routing rules"),
            );
            return;
        };
        let net_id = net.id.clone();
        let mut directive = RouteDirective {
            net: net_id,
            width: None,
            layer: None,
            differential_pair: false,
            length: None,
            impedance: None,
            source_span: Some(decl.span),
        };
        for property in &decl.properties {
            match property.key.to_ascii_lowercase().as_str() {
                "width" => match length_of_lit(&property.value) {
                    Some(width) if width.is_positive() => directive.width = Some(width),
                    _ => self.diagnostics.push(
                        Diagnostic::error("SEM_051", "route width must be a positive length")
                            .with_span(property.span),
                    ),
                },
                "length" => match length_of_lit(&property.value) {
                    Some(length) if length.is_positive() => directive.length = Some(length),
                    _ => self.diagnostics.push(
                        Diagnostic::error("SEM_052", "route length must be a positive length")
                            .with_span(property.span),
                    ),
                },
                "layer" => {
                    let name = match &property.value {
                        Value::Ident(value) | Value::String(value) => value.clone(),
                        _ => String::new(),
                    };
                    match self.ir.layer(&name) {
                        Some(layer) if layer.is_copper() => directive.layer = Some(layer.id.clone()),
                        Some(layer) => self.diagnostics.push(
                            Diagnostic::error(
                                "SEM_053",
                                format!("route layer '{}' is not a copper layer", layer.name),
                            )
                            .with_span(property.span),
                        ),
                        None => self.diagnostics.push(
                            Diagnostic::error(
                                "SEM_054",
                                format!("route references unknown layer '{name}'"),
                            )
                            .with_span(property.span),
                        ),
                    }
                }
                "differential_pair" | "differential" => {
                    directive.differential_pair = matches!(&property.value, Value::Bool(true));
                }
                "impedance" => match quantity_of(&property.value) {
                    Some(quantity)
                        if quantity.kind == UnitKind::Impedance
                            || quantity.kind == UnitKind::Resistance =>
                    {
                        directive.impedance = Some(quantity)
                    }
                    Some(quantity) => self.diagnostics.push(
                        Diagnostic::error(
                            "SEM_055",
                            format!(
                                "route impedance must be an impedance, found {}",
                                quantity.kind.label()
                            ),
                        )
                        .with_span(property.span),
                    ),
                    None => {}
                },
                _ => {}
            }
        }
        self.ir.route_directives.push(directive);
    }

    fn lower_via(&mut self, decl: &ViaDecl) {
        let mut drill = None;
        let mut diameter = None;
        for property in &decl.properties {
            match property.key.to_ascii_lowercase().as_str() {
                "drill" => match length_of_lit(&property.value) {
                    Some(value) if value.is_positive() => drill = Some(value),
                    _ => self.diagnostics.push(
                        Diagnostic::error("SEM_056", "via drill must be a positive length")
                            .with_span(property.span),
                    ),
                },
                "diameter" => match length_of_lit(&property.value) {
                    Some(value) if value.is_positive() => diameter = Some(value),
                    _ => self.diagnostics.push(
                        Diagnostic::error("SEM_057", "via diameter must be a positive length")
                            .with_span(property.span),
                    ),
                },
                _ => {}
            }
        }
        if let Some(drill) = drill {
            self.ir.constraints.default_via_drill = drill;
        }
        if let Some(diameter) = diameter {
            self.ir.constraints.default_via_diameter = diameter;
        }
        let constraints = &self.ir.constraints;
        if constraints.default_via_diameter <= constraints.default_via_drill {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_058",
                    "via diameter must be larger than via drill",
                )
                .with_span(decl.span),
            );
        }
    }

    fn lower_constraints(&mut self, block: &Block) {
        for property in &block.properties {
            let lowered = property.key.to_ascii_lowercase();
            let known = matches!(
                lowered.as_str(),
                "min_trace_width"
                    | "clearance"
                    | "min_via_drill"
                    | "min_via_diameter"
                    | "copper_edge_clearance"
                    | "min_annular_ring"
                    | "solder_mask_clearance"
                    | "drill_to_drill"
                    | "default_via_drill"
                    | "default_via_diameter"
            );
            if !known {
                self.diagnostics.push(
                    Diagnostic::warning(
                        "SEM_060",
                        format!("unknown constraint '{}'", property.key),
                    )
                    .with_span(property.span),
                );
                continue;
            }
            let Some(value) = length_of_lit(&property.value) else {
                self.diagnostics.push(
                    Diagnostic::error(
                        "SEM_061",
                        format!("constraint '{}' must be a length", property.key),
                    )
                    .with_span(property.span),
                );
                continue;
            };
            if value.0 < 0 {
                self.diagnostics.push(
                    Diagnostic::error(
                        "SEM_062",
                        format!("constraint '{}' must not be negative", property.key),
                    )
                    .with_span(property.span),
                );
                continue;
            }
            self.apply_constraint(&lowered, value);
        }
        let constraints = self.ir.constraints.clone();
        if constraints.min_via_diameter <= constraints.min_via_drill {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_063",
                    "min_via_diameter must be larger than min_via_drill",
                )
                .with_span(block.span),
            );
        }
        if constraints.default_via_diameter <= constraints.default_via_drill {
            self.diagnostics.push(
                Diagnostic::error(
                    "SEM_064",
                    "default_via_diameter must be larger than default_via_drill",
                )
                .with_span(block.span),
            );
        }
    }

    fn apply_constraint(&mut self, name: &str, value: Length) {
        let constraints = &mut self.ir.constraints;
        match name {
            "min_trace_width" => constraints.min_trace_width = value,
            "clearance" => constraints.clearance = value,
            "min_via_drill" => constraints.min_via_drill = value,
            "min_via_diameter" => constraints.min_via_diameter = value,
            "copper_edge_clearance" => constraints.copper_edge_clearance = value,
            "min_annular_ring" => constraints.min_annular_ring = value,
            "solder_mask_clearance" => constraints.solder_mask_clearance = value,
            "drill_to_drill" => constraints.drill_to_drill = value,
            "default_via_drill" => constraints.default_via_drill = value,
            "default_via_diameter" => constraints.default_via_diameter = value,
            _ => {}
        }
    }
}

fn clamp_to_board(point: Point, width: Length, height: Length) -> Point {
    Point::new(
        Length(point.x.0.clamp(0, width.0.max(0))),
        Length(point.y.0.clamp(0, height.0.max(0))),
    )
}

fn quantity_of(value: &Value) -> Option<Quantity> {
    match value {
        Value::Quantity(literal) => literal.value,
        _ => None,
    }
}

fn length_of(literal: &QuantityLit) -> Option<Length> {
    literal.value.and_then(|quantity| quantity.as_length())
}

fn length_of_lit(value: &Value) -> Option<Length> {
    quantity_of(value).and_then(|quantity| quantity.as_length())
}

fn angle_of(literal: &QuantityLit) -> Option<Angle> {
    literal.value.and_then(|quantity| match quantity.kind {
        UnitKind::Angle => Some(Angle(quantity.value)),
        UnitKind::Dimensionless => Some(Angle(quantity.value.saturating_mul(1_000))),
        _ => None,
    })
}

fn property_to_prop(value: &Value) -> PropValue {
    match value {
        Value::Quantity(literal) => match literal.value {
            Some(quantity) => PropValue::Quantity(quantity),
            None => PropValue::String(literal.raw.clone()),
        },
        Value::String(value) => PropValue::String(value.clone()),
        Value::Ident(value) => match value.to_ascii_lowercase().as_str() {
            "true" => PropValue::Bool(true),
            "false" => PropValue::Bool(false),
            _ => PropValue::String(value.clone()),
        },
        Value::Bool(value) => PropValue::Bool(*value),
        Value::Array(values) => PropValue::Array(values.iter().map(property_to_prop).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use papan_parser::parse;

    fn compile_source(source: &str) -> CompileOutput {
        let parsed = parse(source);
        assert!(
            !parsed.diagnostics.has_errors(),
            "unexpected parse errors: {:?}",
            parsed.diagnostics
        );
        compile(&parsed.program, &ComponentLibrary::builtin())
    }

    const SAMPLE: &str = r#"
board "Mechanical Keyboard"
size 300mm x 120mm
thickness 1.6mm
layers 4

component MCU: "ATmega32U4"
component USB: "USB-C"
component R1: resistor {
    value: 10kΩ
    tolerance: 1%
    package: "0603"
}
component C1: capacitor 10uF

net VCC { voltage: 5V type: power }
net GND { type: ground }

connect MCU.VCC -> VCC
connect MCU.GND -> GND
connect R1.1 -> VCC
connect R1.2 -> GND

place MCU at (150mm, 60mm)
place USB at board.top.center
place R1 near MCU.VCC distance 3mm
rotate MCU 90deg

route VCC { width: 0.15mm layer: F.Cu }
via { drill: 0.30mm diameter: 0.60mm }
constraints { min_trace_width: 0.15mm clearance: 0.20mm }

check DRC
check ERC
export gerber
"#;

    #[test]
    fn lowers_full_program_to_ir() {
        let output = compile_source(SAMPLE);
        assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
        assert_eq!(output.ir.board.name, "Mechanical Keyboard");
        assert_eq!(output.ir.board.width, Length::from_mm(300.0));
        assert_eq!(output.ir.board.layer_count, 4);
        assert_eq!(output.ir.components.len(), 4);
        assert_eq!(output.ir.nets.len(), 2);
        assert_eq!(output.ir.connections.len(), 4);
        assert_eq!(output.ir.route_directives.len(), 1);
        assert_eq!(
            output.commands,
            vec![
                Command::Check("DRC".into()),
                Command::Check("ERC".into()),
                Command::Export("gerber".into())
            ]
        );
        let mcu = output.ir.find_component("MCU").unwrap();
        assert_eq!(mcu.rotation, Angle::from_deg(90.0));
        assert_eq!(mcu.position, Some(Point::from_mm(150.0, 60.0)));
        let usb = output.ir.find_component("USB").unwrap();
        assert_eq!(usb.position, Some(Point::from_mm(150.0, 120.0)));
        let r1 = output.ir.find_component("R1").unwrap();
        assert_eq!(r1.value, Some(Quantity::new(UnitKind::Resistance, 10_000_000)));
        assert!(r1.position.is_some());
        let vcc = output.ir.find_net("VCC").unwrap();
        assert_eq!(vcc.kind, NetKind::Power);
        assert_eq!(vcc.voltage, Some(Quantity::new(UnitKind::Voltage, 5_000_000)));
        assert!(output.ir.constraints.min_trace_width == Length::from_mm(0.15));
        assert!(output.ir.constraints.clearance == Length::from_mm(0.20));
    }

    #[test]
    fn reports_unknown_component_and_pin() {
        let output = compile_source(
            r#"
            board "Bad"
            component R1: resistor 1kΩ
            net VCC
            connect NOPE.1 -> VCC
            connect R1.1 -> MISSING
            "#,
        );
        let codes: Vec<_> = output.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&"SEM_032"), "{codes:?}");
        assert!(codes.contains(&"SEM_035"), "{codes:?}");
    }

    #[test]
    fn reports_wrong_value_unit() {
        let output = compile_source(
            r#"
            board "Units"
            component R1: resistor 100nF
            "#,
        );
        assert!(
            output.diagnostics.iter().any(|d| d.code == "SEM_013"),
            "{:?}",
            output.diagnostics
        );
    }

    #[test]
    fn reports_duplicate_designator() {
        let output = compile_source(
            r#"
            board "Dup"
            component R1: resistor 1kΩ
            component R1: resistor 2kΩ
            "#,
        );
        assert!(
            output.diagnostics.iter().any(|d| d.code == "SEM_010"),
            "{:?}",
            output.diagnostics
        );
    }

    #[test]
    fn modules_are_namespaced() {
        let output = compile_source(
            r#"
            board "Modules"
            module Power {
                component C1: capacitor 10uF
                net VCC { type: power }
                connect C1.1 -> VCC
            }
            component R1: resistor 1kΩ
            "#,
        );
        assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
        assert!(output.ir.find_component("Power_C1").is_some());
        assert!(output.ir.find_net("Power_VCC").is_some());
        assert_eq!(output.ir.connections.len(), 1);
    }

    #[test]
    fn unknown_part_gets_generic_stub_warning() {
        let output = compile_source(
            r#"
            board "Unknown"
            component U1: "MysteryPart"
            "#,
        );
        assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
        assert!(output.diagnostics.iter().any(|d| d.code == "SEM_012"));
        assert!(output.ir.find_component("U1").is_some());
    }

    #[test]
    fn components_without_placement_are_auto_placed() {
        let output = compile_source(
            r#"
            board "Auto"
            size 50mm x 40mm
            component R1: resistor 1kΩ
            component R2: resistor 2kΩ
            "#,
        );
        assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
        let r1 = output.ir.find_component("R1").unwrap();
        let r2 = output.ir.find_component("R2").unwrap();
        assert!(r1.position.is_some());
        assert!(r2.position.is_some());
        assert_ne!(r1.position, r2.position);
        assert!(output.diagnostics.iter().any(|d| d.code == "SEM_017"));
    }
}
