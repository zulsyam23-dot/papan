//! KiCad interoperability adapter.
//!
//! This adapter reads and writes a conservative KiCad `.kicad_pcb` subset using
//! an internal S-expression parser. KiCad is *not* required at build or run
//! time, and the adapter never becomes the design source of truth: imports are
//! converted into PIR objects and exports are generated from PIR objects.
//!
//! The exporter keeps board outline, footprints/pads, nets, connections, track
//! segments and vias. Round-tripping preserves geometry and connectivity, while
//! hierarchy and KiCad-specific metadata are intentionally dropped.

use std::collections::{BTreeMap, BTreeSet};

use papan_backend::{
    BackendCapabilities, BackendError, BackendFile, BackendImport, BackendInput, BackendOutput,
    BackendResult, PcbBackend,
};
use papan_geometry::{Polygon, Size};
use papan_ir::{
    Angle, ComponentCategory, ComponentDefinition, ComponentInstance, FootprintDefinition, Layer,
    Length, Net, NetKind, ObjectId, PadDefinition, PadKind, PadLayers, PadShape, PcbIr, PinDefinition,
    PinRef, PinType, Point, PropValue, Quantity, Side, Stackup, Trace, Via,
};
use papan_types::{Diagnostic, Diagnostics, ValidationReport};

pub const ADAPTER_ID: &str = "kicad";

#[derive(Clone, Debug, Default)]
pub struct KiCadBackend;

impl KiCadBackend {
    pub fn new() -> Self {
        Self
    }
}

impl PcbBackend for KiCadBackend {
    fn id(&self) -> &str {
        ADAPTER_ID
    }

    fn display_name(&self) -> &str {
        "KiCad PCB"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            can_import: true,
            can_export: true,
            can_validate: true,
            preserves_hierarchy: false,
        }
    }

    fn import(&self, input: &BackendInput) -> BackendResult<BackendImport> {
        import_kicad(input)
    }

    fn export(&self, ir: &PcbIr) -> BackendResult<BackendOutput> {
        let text = export_kicad(ir);
        let mut diagnostics = papan_ir_verifier::verify(ir);
        if ir.board.outline.points.is_empty() {
            diagnostics.push(Diagnostic::warning(
                "KICAD_020",
                "board outline is empty; KiCad export will have no Edge.Cuts geometry",
            ));
        }
        Ok(BackendOutput {
            files: vec![BackendFile {
                name: "board.kicad_pcb".to_string(),
                content: text,
                mime_type: Some("text/x-kicad-pcb".to_string()),
            }],
            diagnostics,
        })
    }

    fn validate(&self, ir: &PcbIr) -> BackendResult<ValidationReport> {
        let mut diagnostics = papan_ir_verifier::verify(ir);
        if ir.board.outline.points.is_empty() {
            diagnostics.push(Diagnostic::error(
                "KICAD_021",
                "a board outline is required for KiCad interoperability",
            ));
        }
        Ok(ValidationReport::from_diagnostics(diagnostics))
    }
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Generate a deterministic KiCad PCB file from the PIR.
pub fn export_kicad(ir: &PcbIr) -> String {
    let nets: Vec<&Net> = {
        let mut nets: Vec<&Net> = ir.nets.iter().collect();
        nets.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.0.cmp(&b.id.0)));
        nets
    };
    let mut net_numbers: BTreeMap<ObjectId, u32> = BTreeMap::new();
    for (index, net) in nets.iter().enumerate() {
        net_numbers.insert(net.id.clone(), index as u32 + 1);
    }

    let mut output = String::new();
    output.push_str("(kicad_pcb\n");
    output.push_str("  (version 20240108)\n");
    output.push_str("  (generator \"papan\")\n");
    output.push_str(&format!(
        "  (general (thickness {}))\n",
        number(ir.board.thickness)
    ));
    output.push_str("  (paper \"A4\")\n");
    output.push_str("  (title_block (title \"");
    output.push_str(&escape(&ir.metadata.title));
    output.push_str("\"))\n");
    output.push_str("  (layers\n");
    for layer in &ir.stackup.layers {
        output.push_str(&format!(
            "    ({} \"{}\" {})\n",
            kicad_layer_number(layer),
            layer.name,
            match layer.kind {
                papan_ir::LayerKind::Copper => "signal",
                papan_ir::LayerKind::Mechanical => "user",
                _ => "user",
            }
        ));
    }
    output.push_str("  )\n");
    output.push_str("  (setup (pad_to_mask_clearance 0))\n");
    output.push_str("  (net 0 \"\")\n");
    for net in &nets {
        output.push_str(&format!(
            "  (net {} \"{}\")\n",
            net_numbers.get(&net.id).copied().unwrap_or(0),
            escape(&net.name)
        ));
    }

    // Component pin -> net mapping for pad annotations.
    let mut pin_nets: BTreeMap<(String, String), u32> = BTreeMap::new();
    for connection in &ir.connections {
        let Some(component) = ir.find_component_by_id(&connection.pin.component) else {
            continue;
        };
        if let Some(number) = net_numbers.get(&connection.net).copied() {
            pin_nets.insert(
                (component.designator.clone(), connection.pin.pin.clone()),
                number,
            );
        }
    }

    let mut components: Vec<&ComponentInstance> = ir.components.iter().collect();
    components.sort_by(|a, b| a.designator.cmp(&b.designator));
    for component in components {
        let Some(definition) = ir.definition_for(component) else {
            continue;
        };
        let layer = match component.side {
            Side::Top => "F.Cu",
            Side::Bottom => "B.Cu",
        };
        output.push_str(&format!(
            "  (footprint \"{}\" (layer \"{}\") (at {} {}{})\n",
            escape(&definition.footprint.name),
            layer,
            number(component.position.unwrap_or(Point::ORIGIN).x),
            number(component.position.unwrap_or(Point::ORIGIN).y),
            angle_suffix(component.rotation)
        ));
        output.push_str(&format!(
            "    (property \"Reference\" \"{}\")\n",
            escape(&component.designator)
        ));
        output.push_str(&format!(
            "    (property \"Value\" \"{}\")\n",
            escape(&component_value(component))
        ));
        for pad in &definition.footprint.pads {
            output.push_str(&pad_sexpr(component, pad, &pin_nets));
        }
        output.push_str("  )\n");
    }

    for trace in &ir.traces {
        let layer = ir
            .layer_by_id(&trace.layer)
            .map(|layer| layer.name.as_str())
            .unwrap_or("F.Cu");
        let net = net_numbers.get(&trace.net).copied().unwrap_or(0);
        for pair in trace.points.windows(2) {
            output.push_str(&format!(
                "  (segment (start {} {}) (end {} {}) (width {}) (layer \"{}\") (net {}))\n",
                number(pair[0].x),
                number(pair[0].y),
                number(pair[1].x),
                number(pair[1].y),
                number(trace.width),
                escape(layer),
                net
            ));
        }
    }

    for via in &ir.vias {
        let net = net_numbers.get(&via.net).copied().unwrap_or(0);
        let from = ir
            .layer_by_id(&via.from_layer)
            .map(|layer| layer.name.as_str())
            .unwrap_or("F.Cu");
        let to = ir
            .layer_by_id(&via.to_layer)
            .map(|layer| layer.name.as_str())
            .unwrap_or("B.Cu");
        output.push_str(&format!(
            "  (via (at {} {}) (size {}) (drill {}) (layers \"{}\" \"{}\") (net {}))\n",
            number(via.at.x),
            number(via.at.y),
            number(via.diameter),
            number(via.drill),
            escape(from),
            escape(to),
            net
        ));
    }

    // Outline as explicit Edge.Cuts lines; this preserves polygons exactly.
    let points = &ir.board.outline.points;
    for index in 0..points.len() {
        let a = points[index];
        let b = points[(index + 1) % points.len()];
        output.push_str(&format!(
            "  (gr_line (start {} {}) (end {} {}) (stroke (width 0.1) (type default)) (layer \"Edge.Cuts\"))\n",
            number(a.x),
            number(a.y),
            number(b.x),
            number(b.y)
        ));
    }
    if points.is_empty() {
        output.push_str(&format!(
            "  (gr_rect (start 0 0) (end {} {}) (stroke (width 0.1) (type default)) (fill none) (layer \"Edge.Cuts\"))\n",
            number(ir.board.width),
            number(ir.board.height)
        ));
    }
    output.push_str(")\n");
    output
}

fn pad_sexpr(
    component: &ComponentInstance,
    pad: &PadDefinition,
    pin_nets: &BTreeMap<(String, String), u32>,
) -> String {
    let kind = match pad.kind {
        PadKind::Smd => "smd",
        PadKind::ThroughHole => "thru_hole",
    };
    let shape = match pad.shape {
        PadShape::Rect => "rect",
        PadShape::Round => "circle",
    };
    let net = pin_nets
        .get(&(component.designator.clone(), pad.pin.clone()))
        .copied()
        .unwrap_or(0);
    let mut output = format!(
        "    (pad \"{}\" {} {} (at {} {}) (size {} {})",
        escape(&pad.number),
        kind,
        shape,
        number(pad.position.x),
        number(pad.position.y),
        number(pad.size.width),
        number(pad.size.height),
    );
    if let Some(drill) = pad.drill {
        output.push_str(&format!(" (drill {})", number(drill)));
    }
    let layers = match (pad.kind, pad.layers, component.side) {
        (PadKind::ThroughHole, _, _) => "\"*.Cu\" \"*.Mask\"".to_string(),
        (PadKind::Smd, PadLayers::Both, _) => "\"*.Cu\"".to_string(),
        (PadKind::Smd, PadLayers::Top, Side::Bottom) => "\"B.Cu\"".to_string(),
        (PadKind::Smd, PadLayers::Bottom, Side::Bottom) => "\"B.Cu\"".to_string(),
        (PadKind::Smd, PadLayers::Bottom, Side::Top) => "\"B.Cu\"".to_string(),
        (PadKind::Smd, PadLayers::Top, Side::Top) => "\"F.Cu\"".to_string(),
    };
    output.push_str(&format!(" (layers {})", layers));
    if net != 0 {
        output.push_str(&format!(" (net {})", net));
    }
    output.push_str(")\n");
    output
}

fn kicad_layer_number(layer: &Layer) -> u32 {
    match layer.name.as_str() {
        "F.Cu" => 0,
        "B.Cu" => 31,
        "F.SilkS" => 37,
        "B.SilkS" => 38,
        "F.Mask" => 39,
        "B.Mask" => 40,
        "Edge.Cuts" => 44,
        _ => layer.index + 1,
    }
}

fn component_value(component: &ComponentInstance) -> String {
    component
        .value
        .map(|value| value.to_source())
        .or_else(|| match component.properties.get("value") {
            Some(PropValue::Quantity(value)) => Some(value.to_source()),
            Some(value) => Some(value.display()),
            None => None,
        })
        .unwrap_or_default()
}

fn angle_suffix(angle: Angle) -> String {
    if angle.0 == 0 {
        String::new()
    } else {
        format!(" {}", angle_degrees(angle))
    }
}

fn angle_degrees(angle: Angle) -> String {
    trim_float(angle.as_deg(), 3)
}

fn number(length: Length) -> String {
    trim_float(length.as_mm(), 6)
}

fn trim_float(value: f64, decimals: usize) -> String {
    if !value.is_finite() || value == 0.0 {
        return "0".to_string();
    }
    let mut text = format!("{value:.decimals$}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// S-expression reader
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum SExpr {
    Atom(String),
    List(Vec<SExpr>),
}

impl SExpr {
    fn atom(&self) -> Option<&str> {
        match self {
            SExpr::Atom(value) => Some(value),
            SExpr::List(_) => None,
        }
    }

    fn list(&self) -> Option<&[SExpr]> {
        match self {
            SExpr::List(values) => Some(values),
            SExpr::Atom(_) => None,
        }
    }

    fn head(&self) -> Option<&str> {
        self.list().and_then(|values| values.first()).and_then(SExpr::atom)
    }

    fn children(&self, head: &str) -> Vec<&SExpr> {
        self.list()
            .map(|values| {
                values
                    .iter()
                    .filter(|value| value.head().map(|value| value.eq_ignore_ascii_case(head)).unwrap_or(false))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn child(&self, head: &str) -> Option<&SExpr> {
        self.list()?.iter().find(|value| {
            value
                .head()
                .map(|value| value.eq_ignore_ascii_case(head))
                .unwrap_or(false)
        })
    }

    fn atoms(&self) -> Vec<&str> {
        match self {
            SExpr::Atom(value) => vec![value.as_str()],
            SExpr::List(values) => values.iter().filter_map(SExpr::atom).collect(),
        }
    }

}

fn parse_sexpr(text: &str) -> BackendResult<Vec<SExpr>> {
    let mut parser = SexprParser {
        chars: text.chars().collect(),
        index: 0,
    };
    parser.parse_all()
}

struct SexprParser {
    chars: Vec<char>,
    index: usize,
}

impl SexprParser {
    fn parse_all(&mut self) -> BackendResult<Vec<SExpr>> {
        let mut expressions = Vec::new();
        loop {
            self.skip_whitespace();
            if self.index >= self.chars.len() {
                break;
            }
            expressions.push(self.parse_expr()?);
        }
        Ok(expressions)
    }

    fn parse_expr(&mut self) -> BackendResult<SExpr> {
        self.skip_whitespace();
        let Some(ch) = self.chars.get(self.index).copied() else {
            return Err(BackendError::new(
                "KICAD_001",
                "unexpected end of KiCad file",
            ));
        };
        match ch {
            '(' => {
                self.index += 1;
                let mut values = Vec::new();
                loop {
                    self.skip_whitespace();
                    match self.chars.get(self.index).copied() {
                        Some(')') => {
                            self.index += 1;
                            break;
                        }
                        None => {
                            return Err(BackendError::new(
                                "KICAD_002",
                                "unbalanced '(' in KiCad file",
                            ))
                        }
                        _ => values.push(self.parse_expr()?),
                    }
                }
                Ok(SExpr::List(values))
            }
            ')' => Err(BackendError::new(
                "KICAD_003",
                "unexpected ')' in KiCad file",
            )),
            '"' => {
                self.index += 1;
                let mut value = String::new();
                let mut closed = false;
                while let Some(ch) = self.chars.get(self.index).copied() {
                    self.index += 1;
                    match ch {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => {
                            if let Some(next) = self.chars.get(self.index).copied() {
                                self.index += 1;
                                value.push(match next {
                                    'n' => '\n',
                                    't' => '\t',
                                    other => other,
                                });
                            }
                        }
                        other => value.push(other),
                    }
                }
                if !closed {
                    return Err(BackendError::new(
                        "KICAD_004",
                        "unterminated string in KiCad file",
                    ));
                }
                Ok(SExpr::Atom(value))
            }
            _ => {
                let mut value = String::new();
                while let Some(ch) = self.chars.get(self.index).copied() {
                    if ch.is_whitespace() || ch == '(' || ch == ')' || ch == '"' {
                        break;
                    }
                    value.push(ch);
                    self.index += 1;
                }
                Ok(SExpr::Atom(value))
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.chars.get(self.index).copied() {
            if ch.is_whitespace() {
                self.index += 1;
            } else {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Import a KiCad PCB subset into the PIR.
pub fn import_kicad(input: &BackendInput) -> BackendResult<BackendImport> {
    let expressions = parse_sexpr(&input.content)?;
    let root = expressions
        .iter()
        .find(|expr| expr.head().map(|head| head.eq_ignore_ascii_case("kicad_pcb")).unwrap_or(false))
        .ok_or_else(|| {
            BackendError::new(
                "KICAD_005",
                "input is not a KiCad PCB file (missing kicad_pcb root)",
            )
        })?;

    let mut diagnostics = Diagnostics::new();
    let name = input
        .path
        .as_ref()
        .and_then(|path| path.file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or(if input.name.is_empty() {
            "KiCad Import"
        } else {
            &input.name
        })
        .to_string();
    let mut ir = PcbIr::new(name.clone());
    ir.metadata.title = name.clone();

    // Title block.
    if let Some(title) = root.child("title_block").and_then(|block| block.child("title")) {
        if let Some(value) = title.list().and_then(|values| values.get(1)).and_then(SExpr::atom) {
            if !value.is_empty() {
                ir.metadata.title = value.to_string();
                ir.board.name = value.to_string();
            }
        }
    }

    // Board thickness.
    if let Some(thickness) = root
        .child("general")
        .and_then(|general| general.child("thickness"))
        .and_then(expr_number)
    {
        if thickness > 0.0 {
            ir.board.thickness = Length::from_mm(thickness);
        }
    }

    // Copper layer count.
    let mut copper_names: Vec<String> = Vec::new();
    if let Some(layers) = root.child("layers") {
        if let Some(values) = layers.list() {
            for entry in values.iter().skip(1) {
                let Some(entry_values) = entry.list() else {
                    continue;
                };
                let Some(layer_name) = entry_values.get(1).and_then(SExpr::atom) else {
                    continue;
                };
                let kind = entry_values.get(2).and_then(SExpr::atom).unwrap_or("");
                if kind.eq_ignore_ascii_case("signal")
                    || layer_name.ends_with(".Cu")
                {
                    copper_names.push(layer_name.to_string());
                }
            }
        }
    }
    let layer_count = copper_names.len().max(1) as u32;
    ir.board.layer_count = layer_count;
    ir.stackup = Stackup::for_copper_count(layer_count);

    // Nets.
    let mut nets_by_number: BTreeMap<u32, ObjectId> = BTreeMap::new();
    let mut net_by_name: BTreeMap<String, ObjectId> = BTreeMap::new();
    for net_expr in root.children("net") {
        let values = net_expr.list().unwrap_or(&[]);
        let Some(number_atom) = values.get(1).and_then(SExpr::atom) else {
            continue;
        };
        let Ok(number) = number_atom.parse::<u32>() else {
            continue;
        };
        if number == 0 {
            continue;
        }
        let name = values
            .get(2)
            .and_then(SExpr::atom)
            .map(str::to_string)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("Net-{number}"));
        let id = net_by_name
            .get(&name)
            .cloned()
            .unwrap_or_else(|| ObjectId::stable("net", &name));
        net_by_name.insert(name.clone(), id.clone());
        nets_by_number.insert(number, id.clone());
        if ir.find_net_by_id(&id).is_none() {
            ir.add_net(Net {
                id,
                name,
                kind: NetKind::Other,
                voltage: None,
                members: Vec::new(),
                properties: BTreeMap::new(),
                source_span: None,
            });
        }
    }

    let origin = board_origin(root);
    let (width, height, outline) = board_outline(root, &mut diagnostics);
    ir.board.width = width;
    ir.board.height = height;
    ir.board.outline = outline;
    if ir.board.outline.points.is_empty() {
        ir.board.width = Length::from_mm(100.0);
        ir.board.height = Length::from_mm(80.0);
        ir.board.outline = Polygon::rect(ir.board.width, ir.board.height);
        diagnostics.push(Diagnostic::warning(
            "KICAD_010",
            "no Edge.Cuts outline found; using a 100x80mm board",
        ));
    }

    let mut definitions: BTreeMap<String, ComponentDefinition> = BTreeMap::new();
    let mut designators: BTreeSet<String> = BTreeSet::new();
    let mut index = 0usize;
    for footprint in root.children("footprint") {
        index += 1;
        let values = footprint.list().unwrap_or(&[]);
        let footprint_name = values
            .get(1)
            .and_then(SExpr::atom)
            .unwrap_or("generic")
            .to_string();
        let lib_name = footprint_name
            .split(':')
            .next_back()
            .unwrap_or(&footprint_name)
            .to_string();
        let at = footprint.child("at").map(expr_numbers).unwrap_or_default();
        let raw_position = Point::new(
            Length::from_mm(at.first().copied().unwrap_or(0.0)),
            Length::from_mm(at.get(1).copied().unwrap_or(0.0)),
        );
        let position = Point::new(
            raw_position.x - origin.x,
            raw_position.y - origin.y,
        );
        let rotation = Angle::from_deg(at.get(2).copied().unwrap_or(0.0));
        let layer = footprint
            .child("layer")
            .and_then(|layer| layer.list())
            .and_then(|values| values.get(1))
            .and_then(SExpr::atom)
            .unwrap_or("F.Cu");
        let side = if layer.eq_ignore_ascii_case("B.Cu") {
            Side::Bottom
        } else {
            Side::Top
        };
        let designator = footprint
            .children("property")
            .into_iter()
            .find_map(property_pair("Reference"))
            .or_else(|| {
                footprint
                    .child("fp_text")
                    .and_then(|text| text.list())
                    .and_then(|values| {
                        let head = values.get(1).and_then(SExpr::atom)?;
                        if head.eq_ignore_ascii_case("reference") {
                            values.get(2).and_then(SExpr::atom).map(str::to_string)
                        } else {
                            None
                        }
                    })
            })
            .unwrap_or_else(|| format!("U{index}"));
        let designator = unique_designator(&designator, &mut designators);
        let value_text = footprint
            .children("property")
            .into_iter()
            .find_map(property_pair("Value"))
            .or_else(|| {
                footprint
                    .child("fp_text")
                    .and_then(|text| text.list())
                    .and_then(|values| {
                        let head = values.get(1).and_then(SExpr::atom)?;
                        if head.eq_ignore_ascii_case("value") {
                            values.get(2).and_then(SExpr::atom).map(str::to_string)
                        } else {
                            None
                        }
                    })
            });

        let definition_id = ObjectId::stable("footprint", &lib_name);
        let definition = definitions
            .entry(lib_name.clone())
            .or_insert_with(|| ComponentDefinition {
                id: definition_id.clone(),
                name: lib_name.clone(),
                category: ComponentCategory::Other,
                pins: Vec::new(),
                footprint: FootprintDefinition {
                    name: lib_name.clone(),
                    pads: Vec::new(),
                    description: String::new(),
                },
                manufacturer: None,
                datasheet: None,
                description: String::new(),
            });

        let is_new_definition = definition.footprint.pads.is_empty();
        let mut pad_index = 0usize;
        for pad in footprint.children("pad") {
            pad_index += 1;
            let pad_values = pad.list().unwrap_or(&[]);
            let number = pad_values
                .get(1)
                .and_then(SExpr::atom)
                .map(str::to_string)
                .filter(|number| !number.is_empty())
                .unwrap_or_else(|| pad_index.to_string());
            let kind_text = pad_values.get(2).and_then(SExpr::atom).unwrap_or("smd");
            let shape_text = pad_values.get(3).and_then(SExpr::atom).unwrap_or("rect");
            let pad_at = pad.child("at").map(expr_numbers).unwrap_or_default();
            let pad_position = Point::from_mm(
                pad_at.first().copied().unwrap_or(0.0),
                pad_at.get(1).copied().unwrap_or(0.0),
            );
            let size = pad.child("size").map(expr_numbers).unwrap_or_default();
            let pad_size = Size::from_mm(
                size.first().copied().unwrap_or(0.5),
                size.get(1).copied().unwrap_or(size.first().copied().unwrap_or(0.5)),
            );
            let drill = pad.child("drill").and_then(expr_number).map(Length::from_mm);
            let is_through_hole =
                kind_text.eq_ignore_ascii_case("thru_hole") || kind_text.eq_ignore_ascii_case("np_thru_hole");
            let pad_kind = if is_through_hole {
                PadKind::ThroughHole
            } else {
                PadKind::Smd
            };
            let pad_shape = if shape_text.eq_ignore_ascii_case("circle")
                || shape_text.eq_ignore_ascii_case("oval")
            {
                PadShape::Round
            } else {
                PadShape::Rect
            };
            let pad_layers = {
                let texts: Vec<&str> = pad
                    .child("layers")
                    .map(SExpr::atoms)
                    .unwrap_or_default();
                if texts.iter().any(|text| text.contains("*")) {
                    PadLayers::Both
                } else if texts.iter().any(|text| text.eq_ignore_ascii_case("B.Cu")) {
                    PadLayers::Bottom
                } else {
                    PadLayers::Top
                }
            };
            if is_new_definition {
                if !definition.pins.iter().any(|pin| pin.number == number) {
                    definition.pins.push(PinDefinition {
                        name: number.clone(),
                        number: number.clone(),
                        electrical: PinType::Passive,
                        description: String::new(),
                    });
                }
                definition.footprint.pads.push(PadDefinition {
                    number: number.clone(),
                    pin: number.clone(),
                    kind: pad_kind,
                    shape: pad_shape,
                    position: pad_position,
                    size: pad_size,
                    drill,
                    layers: pad_layers,
                });
            }

            // Pad-local net association.
            if let Some(net_number) = pad.child("net").and_then(expr_number_u32) {
                if net_number != 0 {
                    let net_id = nets_by_number.get(&net_number).cloned();
                    if let Some(net_id) = net_id {
                        let pin = PinRef::new(
                            ObjectId::stable("component", &designator),
                            number.clone(),
                        );
                        ir.connect(pin, net_id, None);
                    }
                }
            }
        }
        ir.add_definition(definition.clone());

        let value = value_text.as_ref().and_then(|value| {
            Quantity::parse_literal(value)
                .ok()
                .filter(|quantity| quantity.kind != papan_types::UnitKind::Dimensionless)
        });
        let mut properties = BTreeMap::new();
        if value.is_none() {
            if let Some(value) = value_text {
                if !value.is_empty() {
                    properties.insert("value".to_string(), PropValue::String(value));
                }
            }
        }
        ir.components.push(ComponentInstance {
            id: ObjectId::stable("component", &designator),
            designator,
            definition_id: definition_id.clone(),
            value,
            properties,
            position: Some(position),
            rotation,
            side,
            locked: false,
            source_span: None,
        });
    }

    // Track segments.
    for segment in root.children("segment") {
        let Some(start) = segment.child("start") else {
            continue;
        };
        let Some(end) = segment.child("end") else {
            continue;
        };
        let start_values = expr_numbers(start);
        let end_values = expr_numbers(end);
        let width = segment
            .child("width")
            .and_then(expr_number)
            .unwrap_or(0.25);
        let layer_name = segment
            .child("layer")
            .and_then(expr_atom_at_1)
            .unwrap_or_else(|| "F.Cu".to_string());
        let (Some(net_number), Some(layer)) = (
            segment.child("net").and_then(expr_number_u32),
            layer_id_for(&ir, &layer_name, &mut diagnostics),
        ) else {
            diagnostics.push(Diagnostic::warning(
                "KICAD_011",
                "track segment without a resolvable net was skipped",
            ));
            continue;
        };
        let Some(net_id) = nets_by_number.get(&net_number).cloned() else {
            diagnostics.push(Diagnostic::warning(
                "KICAD_012",
                format!("track segment references unknown net number {net_number}"),
            ));
            continue;
        };
        let a = Point::new(
            Length::from_mm(start_values.first().copied().unwrap_or(0.0)) - origin.x,
            Length::from_mm(start_values.get(1).copied().unwrap_or(0.0)) - origin.y,
        );
        let b = Point::new(
            Length::from_mm(end_values.first().copied().unwrap_or(0.0)) - origin.x,
            Length::from_mm(end_values.get(1).copied().unwrap_or(0.0)) - origin.y,
        );
        let id = ObjectId::new(format!(
            "trace:import:{}",
            ir.traces.len() + 1
        ));
        ir.traces.push(Trace {
            id,
            net: net_id,
            layer,
            width: Length::from_mm(width),
            points: vec![a, b],
            source_span: None,
        });
    }

    // Vias.
    for via in root.children("via") {
        let Some(at) = via.child("at") else {
            continue;
        };
        let values = expr_numbers(at);
        let position = Point::new(
            Length::from_mm(values.first().copied().unwrap_or(0.0)) - origin.x,
            Length::from_mm(values.get(1).copied().unwrap_or(0.0)) - origin.y,
        );
        let diameter = via.child("size").and_then(expr_number).unwrap_or(0.6);
        let drill = via.child("drill").and_then(expr_number).unwrap_or(0.3);
        let net_number = via.child("net").and_then(expr_number_u32).unwrap_or(0);
        let Some(net_id) = nets_by_number.get(&net_number).cloned() else {
            diagnostics.push(Diagnostic::warning(
                "KICAD_013",
                "via without a resolvable net was skipped",
            ));
            continue;
        };
        let copper = ir.copper_layers();
        let from_layer = copper
            .first()
            .map(|layer| layer.id.clone())
            .unwrap_or_else(|| ObjectId::stable("layer", "F.Cu"));
        let to_layer = copper
            .last()
            .map(|layer| layer.id.clone())
            .unwrap_or_else(|| ObjectId::stable("layer", "B.Cu"));
        let id = ObjectId::new(format!("via:import:{}", ir.vias.len() + 1));
        ir.vias.push(Via {
            id,
            net: net_id,
            at: position,
            drill: Length::from_mm(drill),
            diameter: Length::from_mm(diameter),
            from_layer,
            to_layer,
            source_span: None,
        });
    }

    // Keep definitions deterministic.
    ir.definitions.sort_by(|a, b| a.id.cmp(&b.id));
    diagnostics.sort();
    Ok(BackendImport { ir, diagnostics })
}

fn property_pair<'a>(key: &'a str) -> impl Fn(&SExpr) -> Option<String> + 'a {
    move |expr: &SExpr| {
        let values = expr.list()?;
        let head = values.first()?.atom()?;
        if !head.eq_ignore_ascii_case("property") {
            return None;
        }
        let name = values.get(1)?.atom()?;
        if name.eq_ignore_ascii_case(key) {
            values.get(2)?.atom().map(str::to_string)
        } else {
            None
        }
    }
}

fn expr_atom_at_1(expr: &SExpr) -> Option<String> {
    expr.list()?.get(1)?.atom().map(str::to_string)
}

fn is_edge_cuts(expr: &SExpr) -> bool {
    expr.child("layer")
        .and_then(expr_atom_at_1)
        .map(|layer| layer.eq_ignore_ascii_case("Edge.Cuts"))
        .unwrap_or(false)
}

fn expr_number(expr: &SExpr) -> Option<f64> {
    expr.list()?
        .iter()
        .skip(1)
        .find_map(|value| value.atom().and_then(|atom| parse_number(atom)))
}

fn expr_numbers(expr: &SExpr) -> Vec<f64> {
    expr.atoms()
        .iter()
        .filter_map(|atom| parse_number(atom))
        .collect()
}

fn expr_number_u32(expr: &SExpr) -> Option<u32> {
    expr.list()?
        .iter()
        .skip(1)
        .find_map(|value| value.atom().and_then(|atom| atom.parse::<u32>().ok()))
}

fn parse_number(atom: &str) -> Option<f64> {
    let trimmed = atom.trim_end_matches(|ch: char| ch.is_ascii_alphabetic());
    trimmed.parse::<f64>().ok()
}

fn unique_designator(base: &str, seen: &mut BTreeSet<String>) -> String {
    let mut candidate = base.to_string();
    let mut counter = 2usize;
    while !seen.insert(candidate.clone()) {
        candidate = format!("{base}_{counter}");
        counter += 1;
    }
    candidate
}

fn board_origin(root: &SExpr) -> Point {
    let bounds = outline_bounds(root);
    match bounds {
        Some((min_x, min_y, _, _)) => Point::from_mm(min_x, min_y),
        None => Point::ORIGIN,
    }
}

fn outline_bounds(root: &SExpr) -> Option<(f64, f64, f64, f64)> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    let mut add = |x: f64, y: f64| {
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((min_x, min_y, max_x, max_y)) => (
                min_x.min(x),
                min_y.min(y),
                max_x.max(x),
                max_y.max(y),
            ),
        });
    };
    for rect in root.children("gr_rect").into_iter().chain(root.children("gr_bbox")) {
        if !is_edge_cuts(rect) {
            continue;
        }
        let Some(start) = rect.child("start") else { continue };
        let Some(end) = rect.child("end") else { continue };
        let start_values = expr_numbers(start);
        let end_values = expr_numbers(end);
        if let (Some(sx), Some(sy), Some(ex), Some(ey)) = (
            start_values.first(),
            start_values.get(1),
            end_values.first(),
            end_values.get(1),
        ) {
            add(*sx, *sy);
            add(*ex, *ey);
        }
    }
    for line in root.children("gr_line").into_iter().chain(root.children("gr_poly")) {
        if !is_edge_cuts(line) {
            continue;
        }
        for node in ["start", "end", "pts"] {
            if let Some(child) = line.child(node) {
                let values = expr_numbers(child);
                for pair in values.chunks(2) {
                    if pair.len() == 2 {
                        add(pair[0], pair[1]);
                    }
                }
            }
        }
    }
    bounds
}

fn board_outline(root: &SExpr, diagnostics: &mut Diagnostics) -> (Length, Length, Polygon) {
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut rect: Option<(f64, f64, f64, f64)> = None;
    for gr_rect in root.children("gr_rect") {
        if !is_edge_cuts(gr_rect) {
            continue;
        }
        let (Some(start), Some(end)) = (gr_rect.child("start"), gr_rect.child("end")) else {
            continue;
        };
        let start_values = expr_numbers(start);
        let end_values = expr_numbers(end);
        if let (Some(sx), Some(sy), Some(ex), Some(ey)) = (
            start_values.first(),
            start_values.get(1),
            end_values.first(),
            end_values.get(1),
        ) {
            rect = Some((*sx, *sy, *ex, *ey));
            points.push((*sx, *sy));
            points.push((*ex, *sy));
            points.push((*ex, *ey));
            points.push((*sx, *ey));
        }
    }
    let mut edges: Vec<(Point, Point)> = Vec::new();
    for gr_line in root.children("gr_line") {
        if !is_edge_cuts(gr_line) {
            continue;
        }
        let (Some(start), Some(end)) = (gr_line.child("start"), gr_line.child("end")) else {
            continue;
        };
        let start_values = expr_numbers(start);
        let end_values = expr_numbers(end);
        if let (Some(sx), Some(sy), Some(ex), Some(ey)) = (
            start_values.first(),
            start_values.get(1),
            end_values.first(),
            end_values.get(1),
        ) {
            let a = Point::from_mm(*sx, *sy);
            let b = Point::from_mm(*ex, *ey);
            points.push((*sx, *sy));
            points.push((*ex, *ey));
            edges.push((a, b));
        }
    }
    let Some((min_x, min_y, max_x, max_y)) = points.iter().fold(
        None::<(f64, f64, f64, f64)>,
        |bounds, (x, y)| match bounds {
            None => Some((*x, *y, *x, *y)),
            Some((min_x, min_y, max_x, max_y)) => Some((
                min_x.min(*x),
                min_y.min(*y),
                max_x.max(*x),
                max_y.max(*y),
            )),
        },
    ) else {
        return (Length::ZERO, Length::ZERO, Polygon::new(Vec::new()));
    };
    let width = Length::from_mm(max_x - min_x);
    let height = Length::from_mm(max_y - min_y);
    let outline = if let Some((sx, sy, ex, ey)) = rect {
        let origin = Point::from_mm(sx.min(ex) - min_x, sy.min(ey) - min_y);
        Polygon::rect_at(origin, Length::from_mm((ex - sx).abs()), Length::from_mm((ey - sy).abs()))
    } else if edges.len() >= 3 {
        let mut ordered = order_edges(edges);
        if ordered.len() < 3 {
            diagnostics.push(Diagnostic::warning(
                "KICAD_014",
                "Edge.Cuts geometry is not a closed loop; using its bounding box",
            ));
            Polygon::rect(width, height)
        } else {
            for point in &mut ordered {
                *point = Point::new(
                    point.x - Length::from_mm(min_x),
                    point.y - Length::from_mm(min_y),
                );
            }
            Polygon::new(ordered)
        }
    } else {
        Polygon::rect(width, height)
    };
    (width, height, outline)
}

fn order_edges(edges: Vec<(Point, Point)>) -> Vec<Point> {
    if edges.is_empty() {
        return Vec::new();
    }
    let mut remaining = edges;
    let first = remaining.remove(0);
    let mut points = vec![first.0, first.1];
    let mut cursor = first.1;
    while !remaining.is_empty() {
        let Some(index) = remaining.iter().position(|(a, b)| {
            length_close(*a, cursor) || length_close(*b, cursor)
        }) else {
            break;
        };
        let (a, b) = remaining.remove(index);
        let next = if length_close(a, cursor) { b } else { a };
        if length_close(next, points[0]) {
            break;
        }
        points.push(next);
        cursor = next;
    }
    points
}

fn length_close(a: Point, b: Point) -> bool {
    a.distance(b) < Length::from_um(1.0)
}

fn layer_id_for(ir: &PcbIr, name: &str, diagnostics: &mut Diagnostics) -> Option<ObjectId> {
    if let Some(layer) = ir.layer(name) {
        return Some(layer.id.clone());
    }
    let fallback = ir.copper_layers().first()?.id.clone();
    diagnostics.push(Diagnostic::warning(
        "KICAD_015",
        format!("unknown copper layer '{name}'; using the first copper layer"),
    ));
    Some(fallback)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use papan_backend::BackendInput;

    fn resistor_definition() -> ComponentDefinition {
        ComponentDefinition {
            id: ObjectId::stable("part", "resistor"),
            name: "resistor".to_string(),
            category: ComponentCategory::Resistor,
            pins: vec![
                PinDefinition {
                    name: "1".into(),
                    number: "1".into(),
                    electrical: PinType::Passive,
                    description: String::new(),
                },
                PinDefinition {
                    name: "2".into(),
                    number: "2".into(),
                    electrical: PinType::Passive,
                    description: String::new(),
                },
            ],
            footprint: FootprintDefinition {
                name: "0603".into(),
                pads: vec![
                    PadDefinition {
                        number: "1".into(),
                        pin: "1".into(),
                        kind: PadKind::Smd,
                        shape: PadShape::Rect,
                        position: Point::from_mm(-0.8, 0.0),
                        size: Size::from_mm(0.9, 0.95),
                        drill: None,
                        layers: PadLayers::Top,
                    },
                    PadDefinition {
                        number: "2".into(),
                        pin: "2".into(),
                        kind: PadKind::Smd,
                        shape: PadShape::Rect,
                        position: Point::from_mm(0.8, 0.0),
                        size: Size::from_mm(0.9, 0.95),
                        drill: None,
                        layers: PadLayers::Top,
                    },
                ],
                description: String::new(),
            },
            manufacturer: None,
            datasheet: None,
            description: String::new(),
        }
    }

    fn fixture() -> PcbIr {
        let mut ir = PcbIr::new("KiCad Demo");
        ir.board.width = Length::from_mm(50.0);
        ir.board.height = Length::from_mm(30.0);
        ir.board.outline = Polygon::rect(ir.board.width, ir.board.height);
        ir.definitions.clear();
        ir.components.clear();
        ir.nets.clear();
        ir.connections.clear();
        ir.add_definition(resistor_definition());
        for (designator, x) in [("R1", 10.0), ("R2", 30.0)] {
            ir.components.push(ComponentInstance {
                id: ObjectId::stable("component", designator),
                designator: designator.to_string(),
                definition_id: ObjectId::stable("part", "resistor"),
                value: Some(Quantity::parse_literal("10kΩ").unwrap()),
                properties: BTreeMap::new(),
                position: Some(Point::from_mm(x, 10.0)),
                rotation: Angle::ZERO,
                side: Side::Top,
                locked: false,
                source_span: None,
            });
        }
        let net_id = ObjectId::stable("net", "N1");
        let p1 = PinRef::new(ObjectId::stable("component", "R1"), "1");
        let p2 = PinRef::new(ObjectId::stable("component", "R2"), "1");
        ir.add_net(Net {
            id: net_id.clone(),
            name: "N1".into(),
            kind: NetKind::Other,
            voltage: None,
            members: vec![p1.clone(), p2.clone()],
            properties: BTreeMap::new(),
            source_span: None,
        });
        ir.connect(p1, net_id.clone(), None);
        ir.connect(p2, net_id.clone(), None);
        ir.traces.push(Trace {
            id: ObjectId::stable("trace", "t1"),
            net: net_id.clone(),
            layer: ObjectId::stable("layer", "F.Cu"),
            width: Length::from_mm(0.25),
            points: vec![Point::from_mm(9.2, 10.0), Point::from_mm(29.2, 10.0)],
            source_span: None,
        });
        ir.vias.push(Via {
            id: ObjectId::stable("via", "v1"),
            net: net_id,
            at: Point::from_mm(20.0, 20.0),
            drill: Length::from_mm(0.3),
            diameter: Length::from_mm(0.6),
            from_layer: ObjectId::stable("layer", "F.Cu"),
            to_layer: ObjectId::stable("layer", "B.Cu"),
            source_span: None,
        });
        ir
    }

    #[test]
    fn exports_expected_kicad_objects() {
        let ir = fixture();
        let text = export_kicad(&ir);
        assert!(text.starts_with("(kicad_pcb\n"));
        assert!(text.contains("(footprint \"0603\""));
        assert!(text.contains("(property \"Reference\" \"R1\")"));
        assert!(text.contains("(segment (start 9.2 10)"));
        assert!(text.contains("(via (at 20 20) (size 0.6) (drill 0.3)"));
        assert!(text.contains("(net 1 \"N1\")"));
        assert!(text.contains("(gr_line"));
        assert!(text.trim_end().ends_with(')'));
    }

    #[test]
    fn roundtrip_preserves_geometry_and_connectivity() {
        let ir = fixture();
        let text = export_kicad(&ir);
        let imported = import_kicad(&BackendInput::new("board.kicad_pcb", text)).unwrap();
        assert!(!imported.diagnostics.has_errors(), "{:?}", imported.diagnostics);
        assert_eq!(imported.ir.board.width, ir.board.width);
        assert_eq!(imported.ir.board.height, ir.board.height);
        assert_eq!(imported.ir.components.len(), 2);
        assert!(imported.ir.find_component("R1").is_some());
        assert!(imported.ir.find_net("N1").is_some());
        assert!(!imported.ir.traces.is_empty());
        assert_eq!(imported.ir.vias.len(), 1);
        let net = imported.ir.find_net("N1").unwrap();
        assert_eq!(net.members.len(), 2);
    }

    #[test]
    fn imports_minimal_file_with_rect_outline() {
        let text = r#"
(kicad_pcb
  (version 20240108)
  (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "VCC")
  (footprint "R_0603"
    (layer "F.Cu")
    (at 10 10 90)
    (property "Reference" "R1")
    (property "Value" "10k")
    (pad "1" smd rect (at -0.8 0) (size 0.9 0.95) (layers "F.Cu") (net 1 "VCC"))
    (pad "2" smd rect (at 0.8 0) (size 0.9 0.95) (layers "F.Cu"))
  )
  (gr_rect (start 0 0) (end 50 30) (layer "Edge.Cuts"))
)
"#;
        let imported = import_kicad(&BackendInput::new("test.kicad_pcb", text)).unwrap();
        assert_eq!(imported.ir.board.width, Length::from_mm(50.0));
        assert_eq!(imported.ir.board.height, Length::from_mm(30.0));
        let component = imported.ir.find_component("R1").unwrap();
        assert_eq!(component.position, Some(Point::from_mm(10.0, 10.0)));
        assert_eq!(component.rotation, Angle::from_deg(90.0));
        assert_eq!(component.properties.get("value"), Some(&PropValue::String("10k".into())));
        assert!(imported.ir.find_net("VCC").is_some());
        assert_eq!(imported.ir.find_net("VCC").unwrap().members.len(), 1);
    }

    #[test]
    fn rejects_non_kicad_input() {
        let error = import_kicad(&BackendInput::new("x", "not a board")).unwrap_err();
        assert_eq!(error.code, "KICAD_005");
    }

    #[test]
    fn rejects_unbalanced_input() {
        let error = import_kicad(&BackendInput::new("x", "(kicad_pcb")).unwrap_err();
        assert_eq!(error.code, "KICAD_002");
    }

    #[test]
    fn validate_flags_missing_outline() {
        let mut ir = PcbIr::new("Empty");
        ir.board.outline = Polygon::new(Vec::new());
        let report = KiCadBackend::new().validate(&ir).unwrap();
        assert!(!report.production_ready);
        assert!(report.diagnostics.iter().any(|d| d.code == "KICAD_021"));
    }

    #[test]
    fn backend_trait_export_writes_one_file() {
        let output = KiCadBackend::new().export(&fixture()).unwrap();
        assert_eq!(output.files.len(), 1);
        assert_eq!(output.files[0].name, "board.kicad_pcb");
        assert!(output.diagnostics.iter().all(|d| d.severity != papan_types::Severity::Error));
    }
}
