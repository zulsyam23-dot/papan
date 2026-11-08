//! Design rule checker.
//!
//! DRC operates purely on the PCB IR and reports stable, object-linked
//! diagnostics. It checks copper clearance, width, via geometry, board edge
//! clearance, drill spacing, unrouted connections and placement collisions.

use std::collections::{BTreeMap, BTreeSet};

use papan_electrical::unrouted_nets;
use papan_geometry::Rect;
use papan_ir::{ComponentInstance, Layer, Length, ObjectId, PcbIr, PadDefinition, Point, Trace, Via};
use papan_placement::{collisions, component_rect, pad_rect};
use papan_types::{Diagnostic, Diagnostics, ValidationReport};

/// Run all DRC checks.
pub fn check_drc(ir: &PcbIr) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    check_traces(ir, &mut diagnostics);
    check_vias(ir, &mut diagnostics);
    check_trace_clearance(ir, &mut diagnostics);
    check_trace_pad_clearance(ir, &mut diagnostics);
    check_via_clearance(ir, &mut diagnostics);
    check_edge_clearance(ir, &mut diagnostics);
    check_drill_spacing(ir, &mut diagnostics);
    check_unrouted(ir, &mut diagnostics);
    check_placement(ir, &mut diagnostics);

    diagnostics.sort();
    diagnostics
}

/// Validate a design for production readiness.
pub fn validate(ir: &PcbIr) -> ValidationReport {
    ValidationReport::from_diagnostics(check_drc(ir))
}

fn check_traces(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for trace in &ir.traces {
        if trace.width < ir.constraints.min_trace_width {
            diagnostics.push(
                Diagnostic::error(
                    "DRC_101",
                    format!(
                        "trace width {} is below the minimum {}",
                        trace.width.to_source(),
                        ir.constraints.min_trace_width.to_source()
                    ),
                )
                .with_object(&trace.id.0)
                .with_suggestion("increase the route width or lower min_trace_width"),
            );
        }
        if !trace.width.is_positive() {
            diagnostics.push(
                Diagnostic::error("DRC_102", "trace width must be positive")
                    .with_object(&trace.id.0),
            );
        }
        match ir.layer_by_id(&trace.layer) {
            Some(layer) if layer.is_copper() => {}
            Some(layer) => diagnostics.push(
                Diagnostic::error(
                    "DRC_103",
                    format!("trace '{}' uses non-copper layer '{}'", trace.id.0, layer.name),
                )
                .with_object(&trace.id.0),
            ),
            None => diagnostics.push(
                Diagnostic::error(
                    "DRC_104",
                    format!("trace '{}' references a missing layer", trace.id.0),
                )
                .with_object(&trace.id.0),
            ),
        }
        if trace.points.len() < 2 {
            diagnostics.push(
                Diagnostic::error("DRC_105", "trace must contain at least two points")
                    .with_object(&trace.id.0),
            );
        }
        for point in &trace.points {
            if !ir.board.outline.contains_point(*point) {
                diagnostics.push(
                    Diagnostic::error("DRC_106", "trace point lies outside the board outline")
                        .with_object(&trace.id.0)
                        .with_location(*point),
                );
                break;
            }
        }
    }
}

fn check_vias(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for via in &ir.vias {
        if via.drill < ir.constraints.min_via_drill {
            diagnostics.push(
                Diagnostic::error(
                    "DRC_110",
                    format!(
                        "via drill {} is below the minimum {}",
                        via.drill.to_source(),
                        ir.constraints.min_via_drill.to_source()
                    ),
                )
                .with_object(&via.id.0)
                .with_location(via.at),
            );
        }
        if via.diameter < ir.constraints.min_via_diameter {
            diagnostics.push(
                Diagnostic::error(
                    "DRC_111",
                    format!(
                        "via diameter {} is below the minimum {}",
                        via.diameter.to_source(),
                        ir.constraints.min_via_diameter.to_source()
                    ),
                )
                .with_object(&via.id.0)
                .with_location(via.at),
            );
        }
        let annular = Length((via.diameter.0 - via.drill.0) / 2);
        if annular < ir.constraints.min_annular_ring {
            diagnostics.push(
                Diagnostic::error(
                    "DRC_112",
                    format!(
                        "via annular ring {} is below the minimum {}",
                        annular.to_source(),
                        ir.constraints.min_annular_ring.to_source()
                    ),
                )
                .with_object(&via.id.0)
                .with_location(via.at),
            );
        }
        if !ir.board.outline.contains_point(via.at) {
            diagnostics.push(
                Diagnostic::error("DRC_113", "via lies outside the board outline")
                    .with_object(&via.id.0)
                    .with_location(via.at),
            );
        }
    }
}

fn check_trace_clearance(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let traces: Vec<&Trace> = ir.traces.iter().collect();
    for i in 0..traces.len() {
        for j in (i + 1)..traces.len() {
            let a = traces[i];
            let b = traces[j];
            if a.net == b.net || a.layer != b.layer {
                continue;
            }
            let copper_allowance = Length((a.width.0 + b.width.0) / 2);
            for segment_a in a.segments() {
                for segment_b in b.segments() {
                    let distance = segment_a.distance_to_segment(segment_b);
                    if distance < copper_allowance + ir.constraints.clearance {
                        diagnostics.push(
                            Diagnostic::error(
                                "DRC_120",
                                format!(
                                    "trace clearance violation between '{}' and '{}'",
                                    a.id.0, b.id.0
                                ),
                            )
                            .with_object(&a.id.0)
                            .with_location(segment_a.midpoint())
                            .with_suggestion("increase spacing or reduce trace width"),
                        );
                        break;
                    }
                }
                if diagnostics.iter().any(|d| {
                    d.code == "DRC_120" && d.object.as_deref() == Some(a.id.0.as_str())
                }) {
                    break;
                }
            }
        }
    }
}

fn check_trace_pad_clearance(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let pin_nets = pin_net_map(ir);
    for trace in &ir.traces {
        let Some(layer) = ir.layer_by_id(&trace.layer) else {
            continue;
        };
        for component in &ir.components {
            let Some(definition) = ir.definition_for(component) else {
                continue;
            };
            for pad in &definition.footprint.pads {
                if !pad_on_layer(pad, layer) {
                    continue;
                }
                if pin_nets
                    .get(&(component.id.clone(), pad.pin.clone()))
                    .map(|net| *net == trace.net)
                    .unwrap_or(false)
                {
                    continue;
                }
                let Some(rect) = pad_rect(component, pad) else {
                    continue;
                };
                let allowance = ir.constraints.clearance + Length(trace.width.0 / 2);
                if trace
                    .segments()
                    .iter()
                    .any(|segment| segment.distance_to_rect(rect) < allowance)
                {
                    diagnostics.push(
                        Diagnostic::error(
                            "DRC_121",
                            format!(
                                "trace '{}' is too close to pad '{}' on component '{}'",
                                trace.id.0, pad.number, component.designator
                            ),
                        )
                        .with_object(&trace.id.0)
                        .with_location(rect.center())
                        .with_suggestion("move the trace or the component"),
                    );
                }
            }
        }
    }
}

fn check_via_clearance(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for via in &ir.vias {
        for trace in &ir.traces {
            if via.net == trace.net {
                continue;
            }
            if !via_covers_layer(via, &trace.layer, ir) {
                continue;
            }
            let allowance = Length((via.diameter.0 + trace.width.0) / 2);
            if trace
                .segments()
                .iter()
                .any(|segment| segment.distance_to_point(via.at) < allowance + ir.constraints.clearance)
            {
                diagnostics.push(
                    Diagnostic::error(
                        "DRC_122",
                        format!(
                            "via '{}' is too close to trace '{}'",
                            via.id.0, trace.id.0
                        ),
                    )
                    .with_object(&via.id.0)
                    .with_location(via.at),
                );
            }
        }
    }
    for i in 0..ir.vias.len() {
        for j in (i + 1)..ir.vias.len() {
            let a = &ir.vias[i];
            let b = &ir.vias[j];
            if a.net == b.net {
                continue;
            }
            let allowance = Length((a.diameter.0 + b.diameter.0) / 2);
            if a.at.distance(b.at) < allowance + ir.constraints.clearance {
                diagnostics.push(
                    Diagnostic::error(
                        "DRC_123",
                        format!("via clearance violation between '{}' and '{}'", a.id.0, b.id.0),
                    )
                    .with_object(&a.id.0)
                    .with_location(a.at),
                );
            }
        }
    }
}

fn check_edge_clearance(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let edges = ir.board.outline.edges();
    for trace in &ir.traces {
        let radius = Length(trace.width.0 / 2);
        for segment in trace.segments() {
            let mut distance = Length(i64::MAX);
            for edge in &edges {
                distance = distance.min(segment.distance_to_segment(*edge));
            }
            if distance != Length(i64::MAX)
                && distance < radius + ir.constraints.copper_edge_clearance
            {
                diagnostics.push(
                    Diagnostic::error(
                        "DRC_130",
                        format!(
                            "trace '{}' violates copper-to-edge clearance",
                            trace.id.0
                        ),
                    )
                    .with_object(&trace.id.0)
                    .with_location(segment.midpoint())
                    .with_suggestion("move the trace away from the board edge"),
                );
                break;
            }
        }
    }
    for via in &ir.vias {
        let mut distance = Length(i64::MAX);
        for edge in &edges {
            distance = distance.min(edge.distance_to_point(via.at));
        }
        if distance != Length(i64::MAX)
            && distance < Length(via.diameter.0 / 2) + ir.constraints.copper_edge_clearance
        {
            diagnostics.push(
                Diagnostic::error(
                    "DRC_131",
                    format!("via '{}' violates copper-to-edge clearance", via.id.0),
                )
                .with_object(&via.id.0)
                .with_location(via.at),
            );
        }
    }
}

fn check_drill_spacing(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let mut holes: Vec<(ObjectId, Point, Length)> = Vec::new();
    for via in &ir.vias {
        holes.push((via.id.clone(), via.at, via.drill));
    }
    for component in &ir.components {
        let Some(definition) = ir.definition_for(component) else {
            continue;
        };
        for pad in &definition.footprint.pads {
            if let Some(drill) = pad.drill {
                if let Some(position) = pad_center(component, pad) {
                    holes.push((
                        ObjectId::stable(
                            "pad",
                            &format!("{}-{}", component.designator, pad.number),
                        ),
                        position,
                        drill,
                    ));
                }
            }
        }
    }
    for i in 0..holes.len() {
        for j in (i + 1)..holes.len() {
            let (id_a, at_a, drill_a) = &holes[i];
            let (id_b, at_b, drill_b) = &holes[j];
            let edge_gap = at_a.distance(*at_b) - Length((drill_a.0 + drill_b.0) / 2);
            if edge_gap < ir.constraints.drill_to_drill {
                diagnostics.push(
                    Diagnostic::error(
                        "DRC_140",
                        format!(
                            "drill-to-drill clearance violation between '{}' and '{}'",
                            id_a.0, id_b.0
                        ),
                    )
                    .with_object(&id_a.0)
                    .with_location(*at_a)
                    .with_suggestion("move the drills apart or reduce their diameter"),
                );
            }
        }
    }
}

fn check_unrouted(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for net in unrouted_nets(ir) {
        diagnostics.push(
            Diagnostic::error(
                "DRC_150",
                format!(
                    "net '{}' is not fully routed ({} disconnected groups)",
                    net.net_name,
                    net.groups.len()
                ),
            )
            .with_object(&net.net.0)
            .with_suggestion("route the net or add a waiver"),
        );
    }
}

fn check_placement(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for collision in collisions(ir) {
        diagnostics.push(
            Diagnostic::warning(
                "DRC_160",
                format!(
                    "components '{}' and '{}' overlap each other",
                    collision.a.0, collision.b.0
                ),
            )
            .with_object(&collision.a.0)
            .with_location(collision.overlap.center())
            .with_suggestion("move one of the components"),
        );
    }
    for component in &ir.components {
        if let Some(rect) = component_rect(ir, component) {
            if !rect_inside_outline(rect, ir) {
                diagnostics.push(
                    Diagnostic::warning(
                        "DRC_161",
                        format!(
                            "component '{}' extends beyond the board outline",
                            component.designator
                        ),
                    )
                    .with_object(&component.id.0)
                    .with_location(rect.center()),
                );
            }
        }
    }
}

fn pin_net_map(ir: &PcbIr) -> BTreeMap<(ObjectId, String), ObjectId> {
    let mut result = BTreeMap::new();
    for net in &ir.nets {
        for pin in &net.members {
            result.insert((pin.component.clone(), pin.pin.clone()), net.id.clone());
        }
    }
    result
}

fn pad_on_layer(pad: &PadDefinition, layer: &Layer) -> bool {
    if pad.kind == papan_ir::PadKind::ThroughHole {
        return true;
    }
    match pad.layers {
        papan_ir::PadLayers::Both => true,
        papan_ir::PadLayers::Top => layer.name == "F.Cu",
        papan_ir::PadLayers::Bottom => layer.name == "B.Cu",
    }
}

fn via_covers_layer(via: &Via, layer: &ObjectId, ir: &PcbIr) -> bool {
    let from = ir.layer_by_id(&via.from_layer).map(|layer| layer.index);
    let to = ir.layer_by_id(&via.to_layer).map(|layer| layer.index);
    let wanted = ir.layer_by_id(layer).map(|layer| layer.index);
    match (from, to, wanted) {
        (Some(from), Some(to), Some(wanted)) => {
            let (low, high) = if from <= to { (from, to) } else { (to, from) };
            wanted >= low && wanted <= high
        }
        _ => false,
    }
}

fn pad_center(component: &ComponentInstance, pad: &PadDefinition) -> Option<Point> {
    papan_placement::pad_position(component, pad)
}

fn rect_inside_outline(rect: Rect, ir: &PcbIr) -> bool {
    rect.corners()
        .iter()
        .all(|point| ir.board.outline.contains_point(*point))
}

/// Deduplicated diagnostic codes used by this crate.
pub fn codes() -> BTreeSet<&'static str> {
    [
        "DRC_101", "DRC_102", "DRC_103", "DRC_104", "DRC_105", "DRC_106", "DRC_110", "DRC_111",
        "DRC_112", "DRC_113", "DRC_120", "DRC_121", "DRC_122", "DRC_123", "DRC_130", "DRC_131",
        "DRC_140", "DRC_150", "DRC_160", "DRC_161",
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use papan_geometry::Size;
    use papan_ir::{
        Angle, ComponentCategory, ComponentDefinition, ComponentInstance, Connection,
        FootprintDefinition, Net, NetKind, PadKind, PadLayers, PadShape, PinDefinition, PinType, Side,
    };

    fn definition(name: &str, pin_names: &[&str], pad_at: f64) -> ComponentDefinition {
        ComponentDefinition {
            id: ObjectId::stable("part", name),
            name: name.into(),
            category: ComponentCategory::Ic,
            pins: pin_names
                .iter()
                .map(|pin| PinDefinition {
                    name: (*pin).into(),
                    number: (*pin).into(),
                    electrical: PinType::Passive,
                    description: String::new(),
                })
                .collect(),
            footprint: FootprintDefinition {
                name: format!("{name}-fp"),
                pads: pin_names
                    .iter()
                    .enumerate()
                    .map(|(index, pin)| PadDefinition {
                        number: (*pin).into(),
                        pin: (*pin).into(),
                        kind: PadKind::Smd,
                        shape: PadShape::Rect,
                        position: Point::from_mm(pad_at + index as f64, 0.0),
                        size: Size::from_mm(0.8, 0.8),
                        drill: None,
                        layers: PadLayers::Top,
                    })
                    .collect(),
                description: String::new(),
            },
            manufacturer: None,
            datasheet: None,
            description: String::new(),
        }
    }

    fn component(name: &str, part: &str, x: f64, y: f64) -> ComponentInstance {
        ComponentInstance {
            id: ObjectId::stable("component", name),
            designator: name.into(),
            definition_id: ObjectId::stable("part", part),
            value: None,
            properties: BTreeMap::new(),
            position: Some(Point::from_mm(x, y)),
            rotation: Angle::ZERO,
            side: Side::Top,
            locked: false,
            source_span: None,
        }
    }

    fn fixture() -> PcbIr {
        let mut ir = PcbIr::new("DRC");
        ir.board.width = Length::from_mm(60.0);
        ir.board.height = Length::from_mm(40.0);
        ir.board.outline = papan_geometry::Polygon::rect(ir.board.width, ir.board.height);
        ir.definitions.clear();
        ir.components.clear();
        ir.nets.clear();
        ir.connections.clear();
        ir.add_definition(definition("chip", &["1"], 0.0));
        // Two chips with pads at their origins.
        let mut left = component("U1", "chip", 10.0, 10.0);
        left.definition_id = ObjectId::stable("part", "chip");
        let mut right = component("U2", "chip", 30.0, 10.0);
        right.definition_id = ObjectId::stable("part", "chip");
        ir.add_component(left);
        ir.add_component(right);
        let p1 = papan_ir::PinRef::new(ObjectId::stable("component", "U1"), "1");
        let p2 = papan_ir::PinRef::new(ObjectId::stable("component", "U2"), "1");
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
        for pin in [p1, p2] {
            ir.connections.push(Connection {
                id: ObjectId::stable(
                    "connection",
                    &format!("{}-{}-{}", pin.component.0, pin.pin, net_id.0),
                ),
                pin,
                net: net_id.clone(),
                source_span: None,
            });
        }
        ir
    }

    fn add_trace(ir: &mut PcbIr, a: Point, b: Point, net: &str, width: f64) -> ObjectId {
        let id = ObjectId::stable("trace", &format!("{net}-{}", ir.traces.len()));
        ir.traces.push(Trace {
            id: id.clone(),
            net: ObjectId::stable("net", net),
            layer: ObjectId::stable("layer", "F.Cu"),
            width: Length::from_mm(width),
            points: vec![a, b],
            source_span: None,
        });
        id
    }

    #[test]
    fn clean_routed_trace_passes_drc() {
        let mut ir = fixture();
        add_trace(
            &mut ir,
            Point::from_mm(10.0, 10.0),
            Point::from_mm(30.0, 10.0),
            "N1",
            0.25,
        );
        let diagnostics = check_drc(&ir);
        assert!(!diagnostics.has_errors(), "{diagnostics:?}");
        assert!(validate(&ir).production_ready);
    }

    #[test]
    fn detects_trace_clearance_violation() {
        let mut ir = fixture();
        add_trace(
            &mut ir,
            Point::from_mm(10.0, 20.0),
            Point::from_mm(30.0, 20.0),
            "N1",
            0.25,
        );
        // A foreign trace just 0.1mm away with 0.2mm clearance required.
        let mut foreign = Trace {
            id: ObjectId::stable("trace", "foreign"),
            net: ObjectId::stable("net", "N2"),
            layer: ObjectId::stable("layer", "F.Cu"),
            width: Length::from_mm(0.25),
            points: vec![Point::from_mm(10.0, 20.3), Point::from_mm(30.0, 20.3)],
            source_span: None,
        };
        foreign.width = Length::from_mm(0.25);
        ir.nets.push(Net {
            id: ObjectId::stable("net", "N2"),
            name: "N2".into(),
            kind: NetKind::Other,
            voltage: None,
            members: Vec::new(),
            properties: BTreeMap::new(),
            source_span: None,
        });
        ir.traces.push(foreign);
        assert!(check_drc(&ir).iter().any(|d| d.code == "DRC_120"));
    }

    #[test]
    fn detects_width_and_edge_violations() {
        let mut ir = fixture();
        add_trace(
            &mut ir,
            Point::from_mm(0.05, 10.0),
            Point::from_mm(30.0, 10.0),
            "N1",
            0.05,
        );
        let codes: Vec<_> = check_drc(&ir).iter().map(|d| d.code.clone()).collect();
        assert!(codes.contains(&"DRC_101".to_string()), "{codes:?}");
        assert!(codes.contains(&"DRC_130".to_string()), "{codes:?}");
    }

    #[test]
    fn detects_bad_via_geometry() {
        let mut ir = fixture();
        ir.vias.push(Via {
            id: ObjectId::stable("via", "v1"),
            net: ObjectId::stable("net", "N1"),
            at: Point::from_mm(20.0, 20.0),
            drill: Length::from_mm(0.2),
            diameter: Length::from_mm(0.4),
            from_layer: ObjectId::stable("layer", "F.Cu"),
            to_layer: ObjectId::stable("layer", "B.Cu"),
            source_span: None,
        });
        let codes: Vec<_> = check_drc(&ir).iter().map(|d| d.code.clone()).collect();
        assert!(codes.contains(&"DRC_110".to_string()), "{codes:?}");
        assert!(codes.contains(&"DRC_111".to_string()), "{codes:?}");
        assert!(codes.contains(&"DRC_112".to_string()), "{codes:?}");
    }

    #[test]
    fn unrouted_net_blocks_production() {
        let ir = fixture();
        let diagnostics = check_drc(&ir);
        assert!(diagnostics.iter().any(|d| d.code == "DRC_150"));
        assert!(!validate(&ir).production_ready);
    }

    #[test]
    fn reports_overlapping_components() {
        let mut ir = fixture();
        ir.find_component_mut("U2").unwrap().position = Some(Point::from_mm(10.2, 10.0));
        assert!(check_drc(&ir).iter().any(|d| d.code == "DRC_160"));
    }
}
