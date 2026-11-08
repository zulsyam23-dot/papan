//! Structural verifier for the PAPAN PCB IR.
//!
//! Every IR change must keep this verifier passing. The verifier checks object
//! identity, references, pin/pad mappings, stackup coherence, routing geometry
//! and manufacturing constraints. It does not perform electrical or design rule
//! checks; those live in the electrical and DRC engines.

use std::collections::{BTreeMap, BTreeSet};

use papan_ir::{
    PcbIr, PIR_SCHEMA_VERSION,
};
use papan_types::{Diagnostic, Diagnostics, ObjectId};

pub fn verify(ir: &PcbIr) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    if ir.schema_version != PIR_SCHEMA_VERSION {
        diagnostics.push(
            Diagnostic::error(
                "IR_001",
                format!(
                    "unsupported PIR schema version {} (expected {})",
                    ir.schema_version, PIR_SCHEMA_VERSION
                ),
            )
            .with_object("ir"),
        );
    }

    verify_board(ir, &mut diagnostics);
    verify_identity(ir, &mut diagnostics);
    verify_definitions(ir, &mut diagnostics);
    verify_components(ir, &mut diagnostics);
    verify_nets(ir, &mut diagnostics);
    verify_connections(ir, &mut diagnostics);
    verify_traces(ir, &mut diagnostics);
    verify_vias(ir, &mut diagnostics);
    verify_constraints(ir, &mut diagnostics);
    verify_route_directives(ir, &mut diagnostics);

    diagnostics.sort();
    diagnostics
}

fn verify_board(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let board = &ir.board;
    if board.id.is_empty() {
        diagnostics.push(Diagnostic::error("IR_002", "board has an empty identifier"));
    }
    if board.name.trim().is_empty() {
        diagnostics.push(Diagnostic::error("IR_003", "board name is empty").with_object(&board.id.0));
    }
    if !board.width.is_positive() || !board.height.is_positive() {
        diagnostics.push(
            Diagnostic::error("IR_004", "board width and height must be positive")
                .with_object(&board.id.0),
        );
    }
    if !board.thickness.is_positive() {
        diagnostics.push(
            Diagnostic::error("IR_005", "board thickness must be positive")
                .with_object(&board.id.0),
        );
    }
    if board.layer_count == 0 || board.layer_count > 64 {
        diagnostics.push(
            Diagnostic::error(
                "IR_006",
                format!("invalid layer count {}", board.layer_count),
            )
            .with_object(&board.id.0),
        );
    }
    if board.outline.is_empty() {
        diagnostics.push(
            Diagnostic::error("IR_007", "board outline must contain at least three points")
                .with_object(&board.id.0),
        );
    } else if board.outline.area_mm2() <= 0.0 {
        diagnostics.push(
            Diagnostic::error("IR_008", "board outline has zero area").with_object(&board.id.0),
        );
    }

    let copper = ir.stackup.copper_layers();
    if copper.len() as u32 != board.layer_count {
        diagnostics.push(
            Diagnostic::error(
                "IR_009",
                format!(
                    "board declares {} layers but stackup has {} copper layers",
                    board.layer_count,
                    copper.len()
                ),
            )
            .with_object(&board.id.0),
        );
    }
}

fn verify_identity(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let mut seen: BTreeMap<ObjectId, &'static str> = BTreeMap::new();
    let mut check = |id: &ObjectId, kind: &'static str, diagnostics: &mut Diagnostics| {
        if id.is_empty() {
            diagnostics.push(Diagnostic::error(
                "IR_010",
                format!("{kind} has an empty stable ID"),
            ));
            return;
        }
        if let Some(previous) = seen.insert(id.clone(), kind) {
            diagnostics.push(
                Diagnostic::error(
                    "IR_011",
                    format!("duplicate object ID '{}' used by {previous} and {kind}", id.0),
                )
                .with_object(&id.0),
            );
        }
    };

    check(&ir.board.id, "board", diagnostics);
    for definition in &ir.definitions {
        check(&definition.id, "component definition", diagnostics);
    }
    for component in &ir.components {
        check(&component.id, "component", diagnostics);
    }
    for net in &ir.nets {
        check(&net.id, "net", diagnostics);
    }
    for connection in &ir.connections {
        check(&connection.id, "connection", diagnostics);
    }
    for trace in &ir.traces {
        check(&trace.id, "trace", diagnostics);
    }
    for via in &ir.vias {
        check(&via.id, "via", diagnostics);
    }
    for layer in &ir.stackup.layers {
        check(&layer.id, "layer", diagnostics);
    }

    let mut designators = BTreeSet::new();
    for component in &ir.components {
        if !designators.insert(component.designator.to_ascii_lowercase()) {
            diagnostics.push(
                Diagnostic::error(
                    "IR_012",
                    format!("duplicate component designator '{}'", component.designator),
                )
                .with_object(&component.id.0),
            );
        }
    }

    let mut net_names = BTreeSet::new();
    for net in &ir.nets {
        if !net_names.insert(net.name.to_ascii_lowercase()) {
            diagnostics.push(
                Diagnostic::error("IR_013", format!("duplicate net name '{}'", net.name))
                    .with_object(&net.id.0),
            );
        }
    }

    let mut layer_names = BTreeSet::new();
    for layer in &ir.stackup.layers {
        if !layer_names.insert(layer.name.to_ascii_lowercase()) {
            diagnostics.push(
                Diagnostic::error("IR_014", format!("duplicate layer name '{}'", layer.name))
                    .with_object(&layer.id.0),
            );
        }
    }
}

fn verify_definitions(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let mut definition_ids = BTreeSet::new();
    for definition in &ir.definitions {
        if !definition_ids.insert(definition.id.clone()) {
            diagnostics.push(
                Diagnostic::error(
                    "IR_015",
                    format!("duplicate component definition '{}'", definition.name),
                )
                .with_object(&definition.id.0),
            );
        }
        if definition.pins.is_empty() {
            diagnostics.push(
                Diagnostic::warning(
                    "IR_016",
                    format!("component definition '{}' has no pins", definition.name),
                )
                .with_object(&definition.id.0),
            );
        }
        let mut pin_names = BTreeSet::new();
        let mut pin_numbers = BTreeSet::new();
        for pin in &definition.pins {
            if !pin_names.insert(pin.name.to_ascii_lowercase()) {
                diagnostics.push(
                    Diagnostic::error(
                        "IR_017",
                        format!(
                            "duplicate pin name '{}' in component definition '{}'",
                            pin.name, definition.name
                        ),
                    )
                    .with_object(&definition.id.0),
                );
            }
            if !pin_numbers.insert(pin.number.clone()) {
                diagnostics.push(
                    Diagnostic::error(
                        "IR_018",
                        format!(
                            "duplicate pin number '{}' in component definition '{}'",
                            pin.number, definition.name
                        ),
                    )
                    .with_object(&definition.id.0),
                );
            }
        }
        let mut pad_numbers = BTreeSet::new();
        for pad in &definition.footprint.pads {
            if !pad_numbers.insert(pad.number.clone()) {
                diagnostics.push(
                    Diagnostic::error(
                        "IR_019",
                        format!(
                            "duplicate pad number '{}' in footprint '{}'",
                            pad.number, definition.footprint.name
                        ),
                    )
                    .with_object(&definition.id.0),
                );
            }
            if definition.pin(&pad.pin).is_none() {
                diagnostics.push(
                    Diagnostic::error(
                        "IR_020",
                        format!(
                            "pad '{}' references missing pin '{}' in component definition '{}'",
                            pad.number, pad.pin, definition.name
                        ),
                    )
                    .with_object(&definition.id.0),
                );
            }
            if !pad.size.width.is_positive() || !pad.size.height.is_positive() {
                diagnostics.push(
                    Diagnostic::error(
                        "IR_021",
                        format!("pad '{}' has non-positive size", pad.number),
                    )
                    .with_object(&definition.id.0),
                );
            }
        }
        for pin in &definition.pins {
            if definition.pad_for_pin(&pin.name).is_none() {
                diagnostics.push(
                    Diagnostic::error(
                        "IR_022",
                        format!(
                            "pin '{}' has no pad mapping in component definition '{}'",
                            pin.name, definition.name
                        ),
                    )
                    .with_object(&definition.id.0),
                );
            }
        }
    }
}

fn verify_components(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for component in &ir.components {
        if ir.find_definition(&component.definition_id).is_none() {
            diagnostics.push(
                Diagnostic::error(
                    "IR_030",
                    format!(
                        "component '{}' references missing definition '{}'",
                        component.designator, component.definition_id.0
                    ),
                )
                .with_object(&component.id.0),
            );
        }
        if let Some(position) = component.position {
            if !ir.board.outline.contains_point(position) {
                diagnostics.push(
                    Diagnostic::error(
                        "IR_031",
                        format!(
                            "component '{}' is placed outside the board outline",
                            component.designator
                        ),
                    )
                    .with_object(&component.id.0)
                    .with_location(position),
                );
            }
        }
    }
}

fn verify_nets(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let mut pin_net: BTreeMap<(ObjectId, String), Vec<ObjectId>> = BTreeMap::new();
    for net in &ir.nets {
        let mut members = BTreeSet::new();
        for member in &net.members {
            if !members.insert(member.clone()) {
                diagnostics.push(
                    Diagnostic::error(
                        "IR_040",
                        format!(
                            "pin {}.{} is listed twice on net '{}'",
                            member.component.0, member.pin, net.name
                        ),
                    )
                    .with_object(&net.id.0),
                );
            }
            match ir.find_component_by_id(&member.component) {
                None => diagnostics.push(
                    Diagnostic::error(
                        "IR_041",
                        format!(
                            "net '{}' references missing component '{}'",
                            net.name, member.component.0
                        ),
                    )
                    .with_object(&net.id.0),
                ),
                Some(component) => match ir.definition_for(component) {
                    None => {}
                    Some(definition) => {
                        if definition.pin(&member.pin).is_none() {
                            diagnostics.push(
                                Diagnostic::error(
                                    "IR_042",
                                    format!(
                                        "net '{}' references missing pin '{}' on component '{}'",
                                        net.name, member.pin, component.designator
                                    ),
                                )
                                .with_object(&net.id.0),
                            );
                        }
                    }
                },
            }
            pin_net
                .entry((member.component.clone(), member.pin.clone()))
                .or_default()
                .push(net.id.clone());
        }
    }

    for (pin, nets) in pin_net {
        if nets.len() > 1 {
            diagnostics.push(
                Diagnostic::error(
                    "IR_043",
                    format!(
                        "pin {}.{} is a member of multiple nets",
                        pin.0, pin.1
                    ),
                )
                .with_object(&pin.0.0),
            );
        }
    }
}

fn verify_connections(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let mut seen = BTreeSet::new();
    for connection in &ir.connections {
        if !seen.insert((connection.pin.clone(), connection.net.clone())) {
            diagnostics.push(
                Diagnostic::error(
                    "IR_050",
                    format!(
                        "duplicate connection for pin {}.{}",
                        connection.pin.component.0, connection.pin.pin
                    ),
                )
                .with_object(&connection.id.0),
            );
        }
        let net = ir.find_net_by_id(&connection.net);
        match net {
            None => diagnostics.push(
                Diagnostic::error(
                    "IR_051",
                    format!("connection references missing net '{}'", connection.net.0),
                )
                .with_object(&connection.id.0),
            ),
            Some(net) => {
                if !net.members.contains(&connection.pin) {
                    diagnostics.push(
                        Diagnostic::error(
                            "IR_052",
                            format!(
                                "connection for pin {}.{} is not a member of net '{}'",
                                connection.pin.component.0, connection.pin.pin, net.name
                            ),
                        )
                        .with_object(&connection.id.0),
                    );
                }
            }
        }
        match ir.find_component_by_id(&connection.pin.component) {
            None => diagnostics.push(
                Diagnostic::error(
                    "IR_053",
                    format!(
                        "connection references missing component '{}'",
                        connection.pin.component.0
                    ),
                )
                .with_object(&connection.id.0),
            ),
            Some(component) => {
                if let Some(definition) = ir.definition_for(component) {
                    if definition.pin(&connection.pin.pin).is_none() {
                        diagnostics.push(
                            Diagnostic::error(
                                "IR_054",
                                format!(
                                    "connection references missing pin '{}' on '{}'",
                                    connection.pin.pin, component.designator
                                ),
                            )
                            .with_object(&connection.id.0),
                        );
                    }
                }
            }
        }
    }

    // Every net member must have a connection record.
    let connection_pins: BTreeSet<(ObjectId, String)> = ir
        .connections
        .iter()
        .map(|connection| (connection.pin.component.clone(), connection.pin.pin.clone()))
        .collect();
    for net in &ir.nets {
        for member in &net.members {
            if !connection_pins.contains(&(member.component.clone(), member.pin.clone())) {
                diagnostics.push(
                    Diagnostic::error(
                        "IR_055",
                        format!(
                            "net '{}' member {}.{} has no connection record",
                            net.name, member.component.0, member.pin
                        ),
                    )
                    .with_object(&net.id.0),
                );
            }
        }
    }
}

fn verify_traces(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for trace in &ir.traces {
        if ir.find_net_by_id(&trace.net).is_none() {
            diagnostics.push(
                Diagnostic::error(
                    "IR_060",
                    format!("trace references missing net '{}'", trace.net.0),
                )
                .with_object(&trace.id.0),
            );
        }
        match ir.layer_by_id(&trace.layer) {
            None => diagnostics.push(
                Diagnostic::error(
                    "IR_061",
                    format!("trace references missing layer '{}'", trace.layer.0),
                )
                .with_object(&trace.id.0),
            ),
            Some(layer) if !layer.is_copper() => diagnostics.push(
                Diagnostic::error(
                    "IR_062",
                    format!("trace is assigned to non-copper layer '{}'", layer.name),
                )
                .with_object(&trace.id.0),
            ),
            Some(_) => {}
        }
        if !trace.width.is_positive() {
            diagnostics.push(
                Diagnostic::error("IR_063", "trace width must be positive")
                    .with_object(&trace.id.0),
            );
        }
        if trace.points.len() < 2 {
            diagnostics.push(
                Diagnostic::error("IR_064", "trace must contain at least two points")
                    .with_object(&trace.id.0),
            );
        }
        for point in &trace.points {
            if !ir.board.outline.contains_point(*point) {
                diagnostics.push(
                    Diagnostic::error("IR_065", "trace point lies outside the board outline")
                        .with_object(&trace.id.0)
                        .with_location(*point),
                );
                break;
            }
        }
    }
}

fn verify_vias(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for via in &ir.vias {
        if ir.find_net_by_id(&via.net).is_none() {
            diagnostics.push(
                Diagnostic::error(
                    "IR_070",
                    format!("via references missing net '{}'", via.net.0),
                )
                .with_object(&via.id.0),
            );
        }
        if !via.drill.is_positive() {
            diagnostics.push(
                Diagnostic::error("IR_071", "via drill must be positive").with_object(&via.id.0),
            );
        }
        if via.diameter <= via.drill {
            diagnostics.push(
                Diagnostic::error(
                    "IR_072",
                    "via diameter must be larger than its drill",
                )
                .with_object(&via.id.0),
            );
        }
        for layer_id in [&via.from_layer, &via.to_layer] {
            match ir.layer_by_id(layer_id) {
                None => diagnostics.push(
                    Diagnostic::error(
                        "IR_073",
                        format!("via references missing layer '{}'", layer_id.0),
                    )
                    .with_object(&via.id.0),
                ),
                Some(layer) if !layer.is_copper() => diagnostics.push(
                    Diagnostic::error(
                        "IR_074",
                        format!("via references non-copper layer '{}'", layer.name),
                    )
                    .with_object(&via.id.0),
                ),
                Some(_) => {}
            }
        }
        if !ir.board.outline.contains_point(via.at) {
            diagnostics.push(
                Diagnostic::error("IR_075", "via lies outside the board outline")
                    .with_object(&via.id.0)
                    .with_location(via.at),
            );
        }
    }
}

fn verify_constraints(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let constraints = &ir.constraints;
    let checks = [
        ("min_trace_width", constraints.min_trace_width),
        ("clearance", constraints.clearance),
        ("min_via_drill", constraints.min_via_drill),
        ("min_via_diameter", constraints.min_via_diameter),
        ("copper_edge_clearance", constraints.copper_edge_clearance),
        ("min_annular_ring", constraints.min_annular_ring),
        ("solder_mask_clearance", constraints.solder_mask_clearance),
        ("drill_to_drill", constraints.drill_to_drill),
        ("default_via_drill", constraints.default_via_drill),
        ("default_via_diameter", constraints.default_via_diameter),
    ];
    for (name, value) in checks {
        if value.0 < 0 {
            diagnostics.push(Diagnostic::error(
                "IR_080",
                format!("constraint '{name}' must not be negative"),
            ));
        }
    }
    if constraints.min_via_diameter <= constraints.min_via_drill {
        diagnostics.push(Diagnostic::error(
            "IR_081",
            "min_via_diameter must be larger than min_via_drill",
        ));
    }
    if constraints.default_via_diameter <= constraints.default_via_drill {
        diagnostics.push(Diagnostic::error(
            "IR_082",
            "default_via_diameter must be larger than default_via_drill",
        ));
    }
}

fn verify_route_directives(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for directive in &ir.route_directives {
        if ir.find_net_by_id(&directive.net).is_none() {
            diagnostics.push(Diagnostic::error(
                "IR_090",
                format!("route directive references missing net '{}'", directive.net.0),
            ));
        }
        if let Some(layer) = &directive.layer {
            match ir.layer_by_id(layer) {
                None => diagnostics.push(Diagnostic::error(
                    "IR_091",
                    format!("route directive references missing layer '{}'", layer.0),
                )),
                Some(layer) if !layer.is_copper() => diagnostics.push(Diagnostic::error(
                    "IR_092",
                    format!("route directive targets non-copper layer '{}'", layer.name),
                )),
                Some(_) => {}
            }
        }
        if let Some(width) = directive.width {
            if !width.is_positive() {
                diagnostics.push(Diagnostic::error(
                    "IR_093",
                    "route directive width must be positive",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use papan_ir::{
        Board, ComponentInstance, Metadata, Net, NetKind, PinRef, Side, Stackup,
    };
    use papan_types::{Angle, Length, Point};

    fn base_ir() -> PcbIr {
        let mut ir = PcbIr::new("Test");
        ir.board = Board::new(
            "Test",
            Length::from_mm(100.0),
            Length::from_mm(80.0),
            2,
            Length::from_mm(1.6),
        );
        ir.stackup = Stackup::for_copper_count(2);
        ir.definitions.clear();
        ir.components.clear();
        ir.nets.clear();
        ir.connections.clear();
        ir
    }

    #[test]
    fn accepts_empty_valid_board() {
        let diagnostics = verify(&base_ir());
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_missing_component_definition() {
        let mut ir = base_ir();
        ir.components.push(ComponentInstance {
            id: ObjectId::stable("component", "U1"),
            designator: "U1".into(),
            definition_id: ObjectId::stable("part", "Missing"),
            value: None,
            properties: Default::default(),
            position: Some(Point::from_mm(10.0, 10.0)),
            rotation: Angle::ZERO,
            side: Side::Top,
            locked: false,
            source_span: None,
        });
        let diagnostics = verify(&ir);
        assert!(diagnostics.iter().any(|d| d.code == "IR_030"));
    }

    #[test]
    fn rejects_bad_net_member() {
        let mut ir = base_ir();
        ir.nets.push(Net {
            id: ObjectId::stable("net", "VCC"),
            name: "VCC".into(),
            kind: NetKind::Power,
            voltage: None,
            members: vec![PinRef::new(ObjectId::stable("component", "U1"), "VCC")],
            properties: Default::default(),
            source_span: None,
        });
        let diagnostics = verify(&ir);
        assert!(diagnostics.iter().any(|d| d.code == "IR_041"));
    }

    #[test]
    fn rejects_duplicate_ids() {
        let mut ir = base_ir();
        ir.nets.push(Net {
            id: ir.board.id.clone(),
            name: "VCC".into(),
            kind: NetKind::Power,
            voltage: None,
            members: Vec::new(),
            properties: Default::default(),
            source_span: None,
        });
        let diagnostics = verify(&ir);
        assert!(diagnostics.iter().any(|d| d.code == "IR_011"));
    }

    #[test]
    fn rejects_bad_layer_count() {
        let mut ir = base_ir();
        ir.board.layer_count = 4;
        let diagnostics = verify(&ir);
        assert!(diagnostics.iter().any(|d| d.code == "IR_009"));
        let _ = Metadata::default();
    }
}
