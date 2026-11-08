use std::collections::BTreeMap;

use papan_ir::{
    Angle, ComponentInstance, Layer, Length, Net, ObjectId, PadDefinition, PadKind, PadLayers,
    PadShape, PcbIr, Point, PropValue, Side,
};

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