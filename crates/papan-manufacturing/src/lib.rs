//! Manufacturing pipeline: Gerber, Excellon, BOM and pick-and-place.
//!
//! All generators are deterministic and consume only the PCB IR. External
//! tools are never required to produce output; the formats are deliberately
//! conservative subsets of RS-274X and Excellon so they can be opened by
//! independent CAM software.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use papan_electrical::unrouted_nets;
use papan_ir::{
    ComponentInstance, Layer, Length, PadDefinition, PadShape, PcbIr, Point, PropValue,
};
use papan_types::{Diagnostic, Diagnostics, ValidationReport};

#[derive(Clone, Debug)]
pub struct ManufacturingOutput {
    /// Gerber files keyed by output filename.
    pub gerber: BTreeMap<String, String>,
    pub drill: String,
    pub bom: String,
    pub pick_and_place: String,
    pub diagnostics: Diagnostics,
}

impl ManufacturingOutput {
    pub fn files(&self) -> Vec<(String, String)> {
        let mut files: Vec<(String, String)> = self
            .gerber
            .iter()
            .map(|(name, content)| (name.clone(), content.clone()))
            .collect();
        files.push(("drill.drl".to_string(), self.drill.clone()));
        files.push(("bom.csv".to_string(), self.bom.clone()));
        files.push((
            "pick_and_place.csv".to_string(),
            self.pick_and_place.clone(),
        ));
        files
    }

    pub fn write_to_dir(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        for (name, content) in self.files() {
            std::fs::write(dir.join(name), content)?;
        }
        Ok(())
    }

    pub fn production_ready(&self) -> bool {
        !self.diagnostics.has_errors()
    }
}

/// Generate every manufacturing artifact for a design.
pub fn export_all(ir: &PcbIr) -> ManufacturingOutput {
    let mut gerber = BTreeMap::new();
    for layer in ir.copper_layers() {
        gerber.insert(gerber_filename(layer), generate_gerber(ir, layer));
    }
    if let Some(edge) = ir.layer("Edge.Cuts") {
        gerber.insert(gerber_filename(edge), generate_edge_gerber(ir));
    }
    ManufacturingOutput {
        gerber,
        drill: generate_excellon(ir),
        bom: generate_bom(ir),
        pick_and_place: generate_pick_and_place(ir),
        diagnostics: validate_manufacturing(ir),
    }
}

/// Validate that the IR is suitable for manufacturing.
pub fn validate_manufacturing(ir: &PcbIr) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    if ir.board.outline.is_empty() || ir.board.outline.area_mm2() <= 0.0 {
        diagnostics.push(Diagnostic::error(
            "MFG_001",
            "board outline is missing or has zero area",
        ));
    }
    if ir.copper_layers().is_empty() {
        diagnostics.push(Diagnostic::error(
            "MFG_002",
            "the stackup contains no copper layers",
        ));
    }
    for component in &ir.components {
        if component.position.is_none() {
            diagnostics.push(
                Diagnostic::error(
                    "MFG_010",
                    format!(
                        "component '{}' has no placement and cannot be assembled",
                        component.designator
                    ),
                )
                .with_object(&component.id.0),
            );
        }
    }
    for trace in &ir.traces {
        if trace.width < ir.constraints.min_trace_width {
            diagnostics.push(
                Diagnostic::error(
                    "MFG_020",
                    format!(
                        "trace '{}' is narrower than the manufacturing minimum",
                        trace.id.0
                    ),
                )
                .with_object(&trace.id.0),
            );
        }
    }
    for via in &ir.vias {
        if via.drill < ir.constraints.min_via_drill {
            diagnostics.push(
                Diagnostic::error(
                    "MFG_021",
                    format!("via '{}' drill is below the manufacturing minimum", via.id.0),
                )
                .with_object(&via.id.0)
                .with_location(via.at),
            );
        }
        if via.diameter <= via.drill {
            diagnostics.push(
                Diagnostic::error(
                    "MFG_022",
                    format!("via '{}' diameter must exceed its drill", via.id.0),
                )
                .with_object(&via.id.0)
                .with_location(via.at),
            );
        }
    }
    for net in unrouted_nets(ir) {
        diagnostics.push(
            Diagnostic::error(
                "MFG_030",
                format!(
                    "net '{}' is not fully routed and cannot be manufactured",
                    net.net_name
                ),
            )
            .with_object(&net.net.0),
        );
    }
    if ir.components.is_empty() {
        diagnostics.push(Diagnostic::info("MFG_040", "design has no components"));
    }
    diagnostics.sort();
    diagnostics
}

pub fn validate(ir: &PcbIr) -> ValidationReport {
    ValidationReport::from_diagnostics(validate_manufacturing(ir))
}

fn gerber_filename(layer: &Layer) -> String {
    format!("{}.gbr", layer.name.replace('.', "_"))
}

/// Generate an RS-274X Gerber file for one copper layer.
pub fn generate_gerber(ir: &PcbIr, layer: &Layer) -> String {
    let traces: Vec<&papan_ir::Trace> = ir
        .traces
        .iter()
        .filter(|trace| trace.layer == layer.id)
        .collect();

    // Collect apertures deterministically before drawing anything.
    let mut aperture_keys: BTreeSet<String> = BTreeSet::new();
    for trace in &traces {
        aperture_keys.insert(format!("C:{}", trace.width.0));
    }
    for (_, pad) in pads_on_layer(ir, layer) {
        aperture_keys.insert(pad_aperture(&pad));
    }
    let apertures: BTreeMap<String, u32> = aperture_keys
        .into_iter()
        .enumerate()
        .map(|(index, key)| (key, 10 + index as u32))
        .collect();

    let mut output = String::new();
    output.push_str("G04 PAPAN Gerber*\n");
    output.push_str(&format!("G04 Layer {}\n", layer.name));
    output.push_str("%FSLAX46Y46*%\n%MOMM*%\nG01*\n%LPD*%\n");
    for (key, code) in &apertures {
        let definition = key
            .strip_prefix("C:")
            .map(|width| {
                format!(
                    "%ADD{code}C,{}*%\n",
                    mm_fixed(width.parse::<i64>().unwrap_or(0), 6)
                )
            })
            .or_else(|| {
                key.strip_prefix("R:").map(|dimensions| {
                    let mut parts = dimensions.split('x');
                    let width = parts.next().and_then(|part| part.parse::<i64>().ok()).unwrap_or(0);
                    let height = parts.next().and_then(|part| part.parse::<i64>().ok()).unwrap_or(0);
                    format!(
                        "%ADD{code}R,{}X{}*%\n",
                        mm_fixed(width, 6),
                        mm_fixed(height, 6)
                    )
                })
            })
            .or_else(|| {
                key.strip_prefix("O:").map(|diameter| {
                    format!(
                        "%ADD{code}C,{}*%\n",
                        mm_fixed(diameter.parse::<i64>().unwrap_or(0), 6)
                    )
                })
            })
            .unwrap_or_default();
        output.push_str(&definition);
    }
    output.push_str("G04 traces*\n");
    for trace in &traces {
        let key = format!("C:{}", trace.width.0);
        if let Some(code) = apertures.get(&key) {
            output.push_str(&format!("D{code}*\n"));
        }
        if let Some(first) = trace.points.first() {
            output.push_str(&format!("X{}Y{}D02*\n", first.x.0, first.y.0));
            for point in trace.points.iter().skip(1) {
                output.push_str(&format!("X{}Y{}D01*\n", point.x.0, point.y.0));
            }
        }
    }
    output.push_str("G04 pads*\n");
    for (component, pad) in pads_on_layer(ir, layer) {
        let Some(position) = papan_placement::pad_position(&component, &pad) else {
            continue;
        };
        let key = pad_aperture(&pad);
        if let Some(code) = apertures.get(&key) {
            output.push_str(&format!("D{code}*\n"));
            output.push_str(&format!(
                "X{}Y{}D03*\n",
                position.x.0, position.y.0
            ));
        }
    }
    output.push_str("M02*\n");
    output
}

fn generate_edge_gerber(ir: &PcbIr) -> String {
    let mut output = String::new();
    output.push_str("G04 PAPAN Gerber*\nG04 Layer Edge.Cuts\n");
    output.push_str("%FSLAX46Y46*%\n%MOMM*%\nG01*\n%LPD*%\n");
    output.push_str(&format!("%ADD10C,{}*%\n", mm_fixed(Length::from_mm(0.10).0, 6)));
    output.push_str("D10*\n");
    let points = &ir.board.outline.points;
    if let Some(first) = points.first() {
        output.push_str(&format!("X{}Y{}D02*\n", first.x.0, first.y.0));
    }
    for point in points.iter().skip(1) {
        output.push_str(&format!("X{}Y{}D01*\n", point.x.0, point.y.0));
    }
    if let Some(first) = points.first() {
        output.push_str(&format!("X{}Y{}D01*\n", first.x.0, first.y.0));
    }
    output.push_str("M02*\n");
    output
}

fn pad_aperture(pad: &PadDefinition) -> String {
    match pad.shape {
        PadShape::Rect => format!("R:{}x{}", pad.size.width.0, pad.size.height.0),
        PadShape::Round => format!("O:{}", pad.size.width.0.min(pad.size.height.0)),
    }
}

fn pads_on_layer<'a>(ir: &'a PcbIr, layer: &Layer) -> Vec<(ComponentInstance, PadDefinition)> {
    let mut pads = Vec::new();
    for component in &ir.components {
        let Some(definition) = ir.definition_for(component) else {
            continue;
        };
        for pad in &definition.footprint.pads {
            let present = match pad.kind {
                papan_ir::PadKind::ThroughHole => true,
                papan_ir::PadKind::Smd => match pad.layers {
                    papan_ir::PadLayers::Both => true,
                    papan_ir::PadLayers::Top => layer.name == "F.Cu",
                    papan_ir::PadLayers::Bottom => layer.name == "B.Cu",
                },
            };
            if present {
                pads.push((component.clone(), pad.clone()));
            }
        }
    }
    pads
}

/// Generate an Excellon drill file for vias and through-hole pads.
pub fn generate_excellon(ir: &PcbIr) -> String {
    let mut holes: Vec<(Length, Point)> = Vec::new();
    for via in &ir.vias {
        holes.push((via.drill, via.at));
    }
    for (component, pad) in all_pads(ir) {
        if let Some(drill) = pad.drill {
            if let Some(position) = papan_placement::pad_position(&component, &pad) {
                holes.push((drill, position));
            }
        }
    }
    holes.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.x.cmp(&b.1.x))
            .then_with(|| a.1.y.cmp(&b.1.y))
    });

    let diameters: Vec<Length> = {
        let mut set = BTreeSet::new();
        for (drill, _) in &holes {
            set.insert(*drill);
        }
        set.into_iter().collect()
    };
    let mut tools: BTreeMap<i64, u32> = BTreeMap::new();
    for (index, diameter) in diameters.iter().enumerate() {
        tools.insert(diameter.0, index as u32 + 1);
    }

    let mut output = String::new();
    output.push_str("M48\n; PAPAN Excellon drill file\nFMAT,2\nMETRIC,TZ\n");
    for diameter in &diameters {
        let code = tools.get(&diameter.0).copied().unwrap_or(1);
        output.push_str(&format!("T{code}C{}\n", mm_fixed(diameter.0, 4)));
    }
    output.push_str("%\nG90\nG05\n");
    let mut current_tool = None;
    for (drill, position) in &holes {
        let tool = tools.get(&drill.0).copied().unwrap_or(1);
        if current_tool != Some(tool) {
            output.push_str(&format!("T{tool}\n"));
            current_tool = Some(tool);
        }
        output.push_str(&format!(
            "X{}Y{}\n",
            mm_fixed(position.x.0, 4),
            mm_fixed(position.y.0, 4)
        ));
    }
    output.push_str("M30\n");
    output
}

fn all_pads(ir: &PcbIr) -> Vec<(ComponentInstance, PadDefinition)> {
    let mut pads = Vec::new();
    for component in &ir.components {
        let Some(definition) = ir.definition_for(component) else {
            continue;
        };
        for pad in &definition.footprint.pads {
            pads.push((component.clone(), pad.clone()));
        }
    }
    pads
}

/// Generate a deterministic BOM as CSV.
pub fn generate_bom(ir: &PcbIr) -> String {
    let mut output = String::from(
        "Designator,Value,Part,Category,Footprint,Quantity,Manufacturer\n",
    );
    let mut components: Vec<&ComponentInstance> = ir.components.iter().collect();
    components.sort_by(|a, b| a.designator.cmp(&b.designator));
    for component in components {
        let definition = ir.definition_for(component);
        let value = component_value(component).unwrap_or_default();
        let part = definition
            .map(|definition| definition.name.clone())
            .unwrap_or_default();
        let category = definition
            .map(|definition| {
                definition
                    .category
                    .keyword()
                    .unwrap_or("other")
                    .to_string()
            })
            .unwrap_or_default();
        let footprint = definition
            .map(|definition| definition.footprint.name.clone())
            .unwrap_or_default();
        let manufacturer = definition
            .and_then(|definition| definition.manufacturer.clone())
            .unwrap_or_default();
        output.push_str(&format!(
            "{},{},{},{},{},1,{}\n",
            csv(&component.designator),
            csv(&value),
            csv(&part),
            csv(&category),
            csv(&footprint),
            csv(&manufacturer),
        ));
    }
    output
}

/// Generate a deterministic pick-and-place CSV.
pub fn generate_pick_and_place(ir: &PcbIr) -> String {
    let mut output = String::from(
        "Designator,Mid X (mm),Mid Y (mm),Layer,Rotation,Value,Package\n",
    );
    let mut components: Vec<&ComponentInstance> = ir.components.iter().collect();
    components.sort_by(|a, b| a.designator.cmp(&b.designator));
    for component in components {
        let position = component
            .position
            .map(|position| {
                (
                    mm_fixed(position.x.0, 4),
                    mm_fixed(position.y.0, 4),
                )
            })
            .unwrap_or_else(|| (String::new(), String::new()));
        let layer = match component.side {
            papan_ir::Side::Top => "top",
            papan_ir::Side::Bottom => "bottom",
        };
        let value = component_value(component).unwrap_or_default();
        let package = ir
            .definition_for(component)
            .map(|definition| definition.footprint.name.clone())
            .unwrap_or_default();
        output.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            csv(&component.designator),
            position.0,
            position.1,
            layer,
            trim_angle(component.rotation.as_deg()),
            csv(&value),
            csv(&package),
        ));
    }
    output
}

fn component_value(component: &ComponentInstance) -> Option<String> {
    component
        .value
        .map(|value| value.to_source())
        .or_else(|| match component.properties.get("value") {
            Some(PropValue::Quantity(value)) => Some(value.to_source()),
            Some(value) => Some(value.display()),
            None => None,
        })
}

fn csv(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn trim_angle(value: f64) -> String {
    let mut text = format!("{value:.3}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// Format nanometres as millimetres with a fixed number of decimals.
fn mm_fixed(nm: i64, decimals: usize) -> String {
    let decimals = decimals.min(9);
    let divisor = 10i64.pow((6 - decimals.min(6)) as u32);
    let rounded = if divisor > 1 {
        let half = divisor / 2;
        if nm >= 0 {
            (nm + half) / divisor
        } else {
            (nm - half) / divisor
        }
    } else {
        nm * 10i64.pow((decimals - 6) as u32)
    };
    let negative = rounded < 0;
    let abs = rounded.unsigned_abs();
    let scale = 10i64.pow(decimals as u32) as u64;
    let whole = abs / scale;
    let fraction = abs % scale;
    let mut text = if decimals == 0 {
        format!("{whole}")
    } else {
        format!("{whole}.{fraction:0width$}", width = decimals)
    };
    if negative && (whole != 0 || fraction != 0) {
        text.insert(0, '-');
    }
    text
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use papan_geometry::{Polygon, Size};
    use papan_ir::{
        Angle, ComponentCategory, ComponentDefinition, FootprintDefinition, Net, NetKind,
        ObjectId, PadKind, PadLayers, PinDefinition, PinType, Side, Trace, Via,
    };

    fn definition() -> ComponentDefinition {
        ComponentDefinition {
            id: ObjectId::stable("part", "resistor"),
            name: "resistor".into(),
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
        let mut ir = PcbIr::new("Manufacturing");
        ir.board.width = Length::from_mm(50.0);
        ir.board.height = Length::from_mm(30.0);
        ir.board.outline = Polygon::rect(ir.board.width, ir.board.height);
        ir.definitions.clear();
        ir.components.clear();
        ir.nets.clear();
        ir.connections.clear();
        ir.add_definition(definition());
        ir.components.push(ComponentInstance {
            id: ObjectId::stable("component", "R1"),
            designator: "R1".into(),
            definition_id: ObjectId::stable("part", "resistor"),
            value: Some(papan_types::Quantity::parse_literal("10kΩ").unwrap()),
            properties: BTreeMap::new(),
            position: Some(Point::from_mm(10.0, 10.0)),
            rotation: Angle::ZERO,
            side: Side::Top,
            locked: false,
            source_span: None,
        });
        ir
    }

    fn routed_fixture() -> PcbIr {
        let mut ir = fixture();
        ir.components.push(ComponentInstance {
            id: ObjectId::stable("component", "R2"),
            designator: "R2".into(),
            definition_id: ObjectId::stable("part", "resistor"),
            value: Some(papan_types::Quantity::parse_literal("1kΩ").unwrap()),
            properties: BTreeMap::new(),
            position: Some(Point::from_mm(30.0, 10.0)),
            rotation: Angle::ZERO,
            side: Side::Top,
            locked: false,
            source_span: None,
        });
        let p1 = papan_ir::PinRef::new(ObjectId::stable("component", "R1"), "1");
        let p2 = papan_ir::PinRef::new(ObjectId::stable("component", "R2"), "1");
        ir.add_net(Net {
            id: ObjectId::stable("net", "N1"),
            name: "N1".into(),
            kind: NetKind::Other,
            voltage: None,
            members: vec![p1.clone(), p2.clone()],
            properties: BTreeMap::new(),
            source_span: None,
        });
        let net_id = ObjectId::stable("net", "N1");
        ir.connect(p1, net_id.clone(), None);
        ir.connect(p2, net_id.clone(), None);
        ir.traces.push(Trace {
            id: ObjectId::stable("trace", "t1"),
            net: net_id.clone(),
            layer: ObjectId::stable("layer", "F.Cu"),
            width: Length::from_mm(0.25),
            points: vec![
                Point::from_mm(9.2, 10.0),
                Point::from_mm(29.2, 10.0),
            ],
            source_span: None,
        });
        ir
    }

    #[test]
    fn gerber_has_header_trace_and_pad() {
        let ir = routed_fixture();
        let layer = ir.layer("F.Cu").unwrap().clone();
        let gerber = generate_gerber(&ir, &layer);
        assert!(gerber.contains("%FSLAX46Y46*%"));
        assert!(gerber.contains("%MOMM*%"));
        assert!(gerber.contains("D01*"));
        assert!(gerber.contains("D03*"));
        assert!(gerber.contains("M02*"));
        assert!(gerber.contains("R,0.900000X0.950000"));
        assert!(gerber.contains("C,0.250000"));
    }

    #[test]
    fn edge_gerber_closes_outline() {
        let ir = fixture();
        let gerber = generate_edge_gerber(&ir);
        assert!(gerber.contains("X0Y0D02*"));
        assert!(gerber.ends_with("M02*\n"));
        assert!(gerber.contains("G04 Layer Edge.Cuts"));
    }

    #[test]
    fn excellon_groups_tools() {
        let mut ir = fixture();
        ir.vias.push(Via {
            id: ObjectId::stable("via", "v1"),
            net: ObjectId::stable("net", "N1"),
            at: Point::from_mm(20.0, 20.0),
            drill: Length::from_mm(0.3),
            diameter: Length::from_mm(0.6),
            from_layer: ObjectId::stable("layer", "F.Cu"),
            to_layer: ObjectId::stable("layer", "B.Cu"),
            source_span: None,
        });
        let drill = generate_excellon(&ir);
        assert!(drill.starts_with("M48\n"));
        assert!(drill.contains("METRIC"));
        assert!(drill.contains("T1C0.3000"));
        assert!(drill.contains("X20.0000Y20.0000"));
        assert!(drill.contains("M30"));
    }

    #[test]
    fn bom_and_pick_place_include_components() {
        let ir = fixture();
        let bom = generate_bom(&ir);
        assert!(bom.starts_with("Designator,Value,Part"));
        assert!(bom.contains("R1,10kΩ,resistor,resistor,0603,1,"));
        let pick = generate_pick_and_place(&ir);
        assert!(pick.contains("R1,10.0000,10.0000,top,0,10kΩ,0603"));
    }

    #[test]
    fn validation_flags_missing_placement() {
        let mut ir = fixture();
        ir.components[0].position = None;
        let diagnostics = validate_manufacturing(&ir);
        assert!(diagnostics.iter().any(|d| d.code == "MFG_010"));
        assert!(!validate(&ir).production_ready);
    }

    #[test]
    fn validation_flags_unrouted_net() {
        let mut ir = routed_fixture();
        ir.traces.clear();
        assert!(validate_manufacturing(&ir).iter().any(|d| d.code == "MFG_030"));
    }

    #[test]
    fn writes_all_artifacts() {
        let ir = routed_fixture();
        let output = export_all(&ir);
        let dir = std::env::temp_dir().join(format!(
            "papan-mfg-{}-{}",
            std::process::id(),
            ir.board.width.0
        ));
        let _ = std::fs::remove_dir_all(&dir);
        output.write_to_dir(&dir).unwrap();
        assert!(dir.join("F_Cu.gbr").is_file());
        assert!(dir.join("Edge_Cuts.gbr").is_file());
        assert!(dir.join("drill.drl").is_file());
        assert!(dir.join("bom.csv").is_file());
        assert!(dir.join("pick_and_place.csv").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fixed_format_is_stable() {
        assert_eq!(mm_fixed(Length::from_mm(1.6).0, 3), "1.600");
        assert_eq!(mm_fixed(Length::from_mm(-1.6).0, 3), "-1.600");
        assert_eq!(mm_fixed(250_000, 6), "0.250000");
        assert_eq!(mm_fixed(250_000, 0), "0");
        assert_eq!(mm_fixed(1_234_567, 4), "1.2346");
    }
}
