//! Deterministic routing engine and basic autorouter.
//!
//! The router connects net members with Manhattan (L-shaped) traces, respects
//! route directives and design constraints, avoids foreign pads where possible
//! and inserts vias when it must change layer. It is intentionally conservative
//! and fully deterministic: no timing, randomness or global optimisation is
//! used, so identical inputs always produce identical copper.

use std::collections::BTreeSet;

use papan_electrical::connectivity;
use papan_geometry::{Rect, Segment};
use papan_ir::{Layer, Length, ObjectId, PcbIr, PinRef, Point, Trace, Via};
use papan_placement::{component_rect, pin_ref_position};
use papan_types::{Diagnostic, Diagnostics};

#[derive(Clone, Debug)]
pub struct RouteReport {
    pub diagnostics: Diagnostics,
    /// Number of traces created.
    pub routed: usize,
    /// Nets that could not be fully routed.
    pub failed: Vec<ObjectId>,
}

impl RouteReport {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

/// Remove all routing so a design can be rebuilt from source.
pub fn clear_routing(ir: &mut PcbIr) {
    ir.traces.clear();
    ir.vias.clear();
}

/// Route all nets deterministically.
pub fn autoroute(ir: &mut PcbIr) -> RouteReport {
    let mut diagnostics = Diagnostics::new();
    let mut routed = 0usize;
    let mut failed = Vec::new();

    let net_ids: Vec<ObjectId> = ir.nets.iter().map(|net| net.id.clone()).collect();
    for net_id in net_ids {
        let before = count_traces(ir, &net_id);
        route_net(ir, &net_id, &mut diagnostics);
        routed += count_traces(ir, &net_id).saturating_sub(before);
        if !net_connected(ir, &net_id) {
            failed.push(net_id.clone());
            if let Some(net) = ir.find_net_by_id(&net_id) {
                diagnostics.push(
                    Diagnostic::warning(
                        "ROUTE_001",
                        format!("net '{}' could not be fully routed", net.name),
                    )
                    .with_object(&net_id.0)
                    .with_suggestion("check pad placement, clearance and route directives"),
                );
            }
        }
    }

    if ir.nets.is_empty() {
        diagnostics.push(Diagnostic::info("ROUTE_002", "no nets to route"));
    }

    diagnostics.sort();
    RouteReport {
        diagnostics,
        routed,
        failed,
    }
}

fn count_traces(ir: &PcbIr, net: &ObjectId) -> usize {
    ir.traces.iter().filter(|trace| &trace.net == net).count()
}

fn net_connected(ir: &PcbIr, net: &ObjectId) -> bool {
    connectivity(ir)
        .into_iter()
        .find(|entry| &entry.net == net)
        .map(|entry| entry.is_connected())
        .unwrap_or(true)
}

fn route_net(ir: &mut PcbIr, net_id: &ObjectId, diagnostics: &mut Diagnostics) {
    let Some(net) = ir.find_net_by_id(net_id) else {
        return;
    };
    if net.members.len() < 2 {
        return;
    }
    let net_name = net.name.clone();
    let members = net.members.clone();

    let directive = ir
        .route_directives
        .iter()
        .rev()
        .find(|directive| &directive.net == net_id)
        .cloned();
    if let Some(directive) = &directive {
        if directive.differential_pair {
            diagnostics.push(
                Diagnostic::info(
                    "ROUTE_010",
                    format!(
                        "differential pair routing for '{net_name}' uses the deterministic single-ended fallback"
                    ),
                )
                .with_object(&net_id.0)
                .with_suggestion("advanced differential/length-matched routing is not implemented yet"),
            );
        }
        if directive.length.is_some() {
            diagnostics.push(
                Diagnostic::info(
                    "ROUTE_011",
                    format!("length matching for '{net_name}' is recorded but not enforced"),
                )
                .with_object(&net_id.0),
            );
        }
        if directive.impedance.is_some() {
            diagnostics.push(
                Diagnostic::info(
                    "ROUTE_013",
                    format!("impedance control for '{net_name}' is recorded but not enforced"),
                )
                .with_object(&net_id.0)
                .with_suggestion("controlled-impedance routing is not implemented yet"),
            );
        }
    }

    let min_width = ir.constraints.min_trace_width;
    let mut width = directive
        .as_ref()
        .and_then(|directive| directive.width)
        .unwrap_or_else(|| min_width.max(Length::from_mm(0.25)));
    if width < min_width {
        diagnostics.push(
            Diagnostic::warning(
                "ROUTE_012",
                format!(
                    "trace width {} on '{net_name}' is below the minimum {}; clamping",
                    width.to_source(),
                    min_width.to_source()
                ),
            )
            .with_object(&net_id.0),
        );
        width = min_width;
    }
    if !width.is_positive() {
        width = Length::from_mm(0.25);
    }

    let copper_layers: Vec<Layer> = ir.copper_layers().into_iter().cloned().collect();
    if copper_layers.is_empty() {
        diagnostics.push(
            Diagnostic::error("ROUTE_013", "no copper layer is available for routing")
                .with_object(&net_id.0),
        );
        return;
    }
    let mut layer = directive
        .as_ref()
        .and_then(|directive| directive.layer.clone())
        .and_then(|id| ir.layer_by_id(&id).cloned())
        .filter(|layer| layer.is_copper())
        .unwrap_or_else(|| copper_layers[0].clone());

    let mut placed: Vec<(PinRef, Point)> = Vec::new();
    for member in &members {
        match pin_ref_position(ir, member) {
            Some(position) => placed.push((member.clone(), position)),
            None => diagnostics.push(
                Diagnostic::warning(
                    "ROUTE_014",
                    format!(
                        "pin '{}.{}' has no resolved position and was not routed",
                        member.component.0, member.pin
                    ),
                )
                .with_object(&member.component.0)
                .with_suggestion("place the component and check its footprint pad mapping"),
            ),
        }
    }
    if placed.len() < 2 {
        diagnostics.push(
            Diagnostic::warning(
                "ROUTE_015",
                format!("net '{net_name}' has fewer than two routable pins"),
            )
            .with_object(&net_id.0),
        );
        return;
    }
    placed.sort_by(|a, b| {
        a.1.x
            .cmp(&b.1.x)
            .then_with(|| a.1.y.cmp(&b.1.y))
            .then_with(|| a.0.component.0.cmp(&b.0.component.0))
            .then_with(|| a.0.pin.cmp(&b.0.pin))
    });

    let obstacles = obstacle_rects(ir, &placed);
    let anchor = placed[0].1;
    let anchor_pin = placed[0].0.clone();
    let clearance = ir.constraints.clearance + Length(width.0 / 2);

    for (pin, position) in placed.iter().skip(1) {
        if already_connected(ir, net_id, &anchor_pin, pin) {
            continue;
        }
        let candidates = candidate_paths(anchor, *position);
        let mut chosen = None;
        for candidate in &candidates {
            if !path_blocked(candidate, &obstacles, clearance) {
                chosen = Some(candidate.clone());
                break;
            }
        }
        let chosen = match chosen {
            Some(path) => path,
            None => {
                diagnostics.push(
                    Diagnostic::warning(
                        "ROUTE_020",
                        format!(
                            "direct path for '{net_name}' is obstructed; routing with a fallback path"
                        ),
                    )
                    .with_object(&net_id.0)
                    .with_location(*position),
                );
                candidates[0].clone()
            }
        };

        // If the preferred layer is obstructed, move this connection to the
        // next copper layer and add vias at both endpoints.
        let mut trace_layer = layer.clone();
        let mut add_vias = false;
        if path_blocked(&chosen, &obstacles, clearance) && copper_layers.len() > 1 {
            if let Some(next) = copper_layers
                .iter()
                .find(|candidate| candidate.index != layer.index)
            {
                let blocked_on_next = path_blocked(&chosen, &obstacles, clearance);
                trace_layer = next.clone();
                add_vias = blocked_on_next || next.index != copper_layers[0].index;
            }
        }
        if trace_layer.index != copper_layers[0].index {
            add_vias = true;
        }

        let index = ir.traces.len();
        let trace_id = ObjectId::stable("trace", &format!("{net_name}-{index}"));
        ir.traces.push(Trace {
            id: trace_id,
            net: net_id.clone(),
            layer: trace_layer.id.clone(),
            width,
            points: chosen.clone(),
            source_span: None,
        });

        if add_vias {
            let (drill, diameter) = via_geometry(ir);
            let via_count = ir.vias.len();
            for (offset, point) in [chosen[0], *chosen.last().unwrap()].into_iter().enumerate() {
                ir.vias.push(Via {
                    id: ObjectId::stable(
                        "via",
                        &format!("{net_name}-{}-{}", via_count + offset, point.to_source()),
                    ),
                    net: net_id.clone(),
                    at: point,
                    drill,
                    diameter,
                    from_layer: copper_layers[0].id.clone(),
                    to_layer: trace_layer.id.clone(),
                    source_span: None,
                });
            }
        }
        layer = trace_layer;
    }
}

fn via_geometry(ir: &PcbIr) -> (Length, Length) {
    let constraints = &ir.constraints;
    let drill = constraints
        .default_via_drill
        .max(constraints.min_via_drill);
    let mut diameter = constraints
        .default_via_diameter
        .max(constraints.min_via_diameter)
        .max(Length(drill.0 + constraints.min_annular_ring.0 * 2));
    if diameter <= drill {
        diameter = Length(drill.0 + constraints.min_annular_ring.0 * 2 + 1);
    }
    (drill, diameter)
}

fn already_connected(ir: &PcbIr, net: &ObjectId, a: &PinRef, b: &PinRef) -> bool {
    connectivity(ir)
        .into_iter()
        .find(|entry| &entry.net == net)
        .map(|entry| {
            entry
                .groups
                .iter()
                .any(|group| group.contains(a) && group.contains(b))
        })
        .unwrap_or(false)
}

/// Candidate Manhattan paths, horizontal-first then vertical-first. Collinear
/// points are removed so a straight connection produces a two-point trace.
fn candidate_paths(a: Point, b: Point) -> Vec<Vec<Point>> {
    let horizontal = simplify(vec![a, Point::new(b.x, a.y), b]);
    let vertical = simplify(vec![a, Point::new(a.x, b.y), b]);
    if horizontal == vertical {
        vec![horizontal]
    } else {
        vec![horizontal, vertical]
    }
}

fn simplify(points: Vec<Point>) -> Vec<Point> {
    let mut result: Vec<Point> = Vec::new();
    for point in points {
        if result.last() == Some(&point) {
            continue;
        }
        result.push(point);
    }
    if result.len() >= 3 {
        let mut deduped = Vec::with_capacity(result.len());
        for index in 0..result.len() {
            if index == 0 || index + 1 == result.len() {
                deduped.push(result[index]);
                continue;
            }
            let a = result[index - 1];
            let b = result[index];
            let c = result[index + 1];
            let collinear = (a.x == b.x && b.x == c.x) || (a.y == b.y && b.y == c.y);
            if !collinear {
                deduped.push(b);
            }
        }
        result = deduped;
    }
    result
}

fn obstacle_rects(ir: &PcbIr, members: &[(PinRef, Point)]) -> Vec<Rect> {
    let excluded: BTreeSet<ObjectId> = members
        .iter()
        .map(|(pin, _)| pin.component.clone())
        .collect();
    let mut rects = Vec::new();
    for component in &ir.components {
        if excluded.contains(&component.id) {
            continue;
        }
        if let Some(rect) = component_rect(ir, component) {
            rects.push(rect.expand(Length::from_mm(0.05)));
        }
    }
    rects
}

fn path_blocked(path: &[Point], obstacles: &[Rect], clearance: Length) -> bool {
    if path.len() < 2 {
        return false;
    }
    for pair in path.windows(2) {
        let segment = Segment::new(pair[0], pair[1]);
        for rect in obstacles {
            if segment.distance_to_rect(*rect) < clearance {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use papan_geometry::Size;
    use papan_ir::{
        Angle, ComponentCategory, ComponentDefinition, ComponentInstance, Connection,
        FootprintDefinition, Net, NetKind, PadDefinition, PadKind, PadLayers, PadShape,
        PinDefinition, PinType, Side,
    };

    fn definition(name: &str, pin_names: &[&str]) -> ComponentDefinition {
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
                        position: Point::from_mm(index as f64, 0.0),
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
        let mut ir = PcbIr::new("Route");
        ir.board.width = Length::from_mm(50.0);
        ir.board.height = Length::from_mm(40.0);
        ir.board.outline = papan_geometry::Polygon::rect(ir.board.width, ir.board.height);
        ir.definitions.clear();
        ir.components.clear();
        ir.nets.clear();
        ir.connections.clear();
        ir.add_definition(definition("chip", &["1"]));
        ir.add_component(component("U1", "chip", 10.0, 10.0));
        ir.add_component(component("U2", "chip", 30.0, 20.0));
        let p1 = PinRef::new(ObjectId::stable("component", "U1"), "1");
        let p2 = PinRef::new(ObjectId::stable("component", "U2"), "1");
        ir.add_net(Net {
            id: ObjectId::stable("net", "SIG"),
            name: "SIG".into(),
            kind: NetKind::Digital,
            voltage: None,
            members: vec![p1.clone(), p2.clone()],
            properties: BTreeMap::new(),
            source_span: None,
        });
        let net_id = ObjectId::stable("net", "SIG");
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

    #[test]
    fn routes_two_pins_deterministically() {
        let mut ir = fixture();
        let report = autoroute(&mut ir);
        assert!(report.failed.is_empty(), "{:?}", report.diagnostics);
        assert_eq!(report.routed, 1);
        assert_eq!(ir.traces.len(), 1);
        let trace = &ir.traces[0];
        assert_eq!(trace.points.first(), Some(&Point::from_mm(10.0, 10.0)));
        assert_eq!(trace.points.last(), Some(&Point::from_mm(30.0, 20.0)));
        assert!(net_connected(&ir, &ObjectId::stable("net", "SIG")));
    }

    #[test]
    fn autoroute_is_idempotent() {
        let mut ir = fixture();
        autoroute(&mut ir);
        let trace_count = ir.traces.len();
        autoroute(&mut ir);
        assert_eq!(ir.traces.len(), trace_count);
    }

    #[test]
    fn clamps_width_to_constraint() {
        let mut ir = fixture();
        ir.route_directives.push(papan_ir::RouteDirective {
            net: ObjectId::stable("net", "SIG"),
            width: Some(Length::from_mm(0.05)),
            layer: None,
            differential_pair: false,
            length: None,
            impedance: None,
            source_span: None,
        });
        let report = autoroute(&mut ir);
        assert!(report.diagnostics.iter().any(|d| d.code == "ROUTE_012"));
        assert_eq!(ir.traces[0].width, ir.constraints.min_trace_width);
    }

    #[test]
    fn layer_directive_inserts_vias() {
        let mut ir = fixture();
        ir.route_directives.push(papan_ir::RouteDirective {
            net: ObjectId::stable("net", "SIG"),
            width: Some(Length::from_mm(0.25)),
            layer: Some(ObjectId::stable("layer", "B.Cu")),
            differential_pair: false,
            length: None,
            impedance: None,
            source_span: None,
        });
        autoroute(&mut ir);
        assert_eq!(ir.traces[0].layer, ObjectId::stable("layer", "B.Cu"));
        assert_eq!(ir.vias.len(), 2);
        for via in &ir.vias {
            assert!(via.diameter > via.drill);
        }
    }

    #[test]
    fn missing_position_is_reported() {
        let mut ir = fixture();
        ir.find_component_mut("U2").unwrap().position = None;
        let report = autoroute(&mut ir);
        assert!(report.diagnostics.iter().any(|d| d.code == "ROUTE_014"));
    }
}
