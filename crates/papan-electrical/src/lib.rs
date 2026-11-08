//! Electrical engine: connectivity graph, unrouted detection and ERC.
//!
//! Connectivity is computed from net membership and copper geometry. Pins and
//! trace endpoints are grouped with a deterministic union-find; a net with more
//! than one group has unrouted connections.

use std::collections::{BTreeMap, BTreeSet};

use papan_ir::{Net, ObjectId, PcbIr, PinRef, PinType, Trace};
use papan_placement::pin_ref_position;
use papan_types::{Diagnostic, Diagnostics, Point, Severity, ValidationReport};

/// Connectivity of one net, split into electrically connected groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetConnectivity {
    pub net: ObjectId,
    pub net_name: String,
    /// Each group is a set of pins that are geometrically connected.
    pub groups: Vec<Vec<PinRef>>,
}

impl NetConnectivity {
    pub fn is_connected(&self) -> bool {
        self.groups.len() <= 1
    }
}

/// Analyze every net and group its pins by copper connectivity.
pub fn connectivity(ir: &PcbIr) -> Vec<NetConnectivity> {
    ir.nets
        .iter()
        .map(|net| NetConnectivity {
            net: net.id.clone(),
            net_name: net.name.clone(),
            groups: net_groups(ir, net),
        })
        .collect()
}

fn net_groups(ir: &PcbIr, net: &Net) -> Vec<Vec<PinRef>> {
    let traces: Vec<&Trace> = ir
        .traces
        .iter()
        .filter(|trace| trace.net == net.id)
        .collect();
    let vias: Vec<&papan_ir::Via> = ir.vias.iter().filter(|via| via.net == net.id).collect();

    let mut points: BTreeMap<Point, usize> = BTreeMap::new();
    let mut parents: Vec<usize> = Vec::new();
    let find = |point: Point, points: &mut BTreeMap<Point, usize>, parents: &mut Vec<usize>| {
        let next = parents.len();
        let index = *points.entry(point).or_insert(next);
        if index == next {
            parents.push(next);
        }
        index
    };

    for trace in &traces {
        for pair in trace.points.windows(2) {
            let a = find(pair[0], &mut points, &mut parents);
            let b = find(pair[1], &mut points, &mut parents);
            union(&mut parents, a, b);
        }
    }
    for via in &vias {
        // A via joins its own endpoint on both layers; creating the point is
        // enough because all traces ending at the via share the same key.
        let _ = find(via.at, &mut points, &mut parents);
    }

    let mut pin_nodes: Vec<PinRef> = Vec::new();
    let mut pin_roots: Vec<usize> = Vec::new();
    for pin in &net.members {
        let Some(position) = pin_ref_position(ir, pin) else {
            continue;
        };
        let index = find(position, &mut points, &mut parents);
        pin_nodes.push(pin.clone());
        pin_roots.push(root(&mut parents, index));
    }

    let mut buckets: BTreeMap<usize, Vec<PinRef>> = BTreeMap::new();
    for (pin, root_index) in pin_nodes.into_iter().zip(pin_roots) {
        let root_index = root(&mut parents, root_index);
        buckets.entry(root_index).or_default().push(pin);
    }
    let mut groups: Vec<Vec<PinRef>> = buckets.into_values().collect();
    for group in &mut groups {
        group.sort_by(|a, b| {
            a.component
                .0
                .cmp(&b.component.0)
                .then_with(|| a.pin.cmp(&b.pin))
        });
    }
    groups.sort_by(|a, b| {
        a.first()
            .map(|pin| (&pin.component.0, &pin.pin))
            .cmp(&b.first().map(|pin| (&pin.component.0, &pin.pin)))
    });
    groups
}

fn root(parents: &mut Vec<usize>, index: usize) -> usize {
    if parents[index] == index {
        index
    } else {
        let next = parents[index];
        let root_index = root(parents, next);
        parents[index] = root_index;
        root_index
    }
}

fn union(parents: &mut Vec<usize>, a: usize, b: usize) {
    let ra = root(parents, a);
    let rb = root(parents, b);
    if ra != rb {
        let (min, max) = if ra < rb { (ra, rb) } else { (rb, ra) };
        parents[max] = min;
    }
}

/// Nets whose pins form more than one connected group.
pub fn unrouted_nets(ir: &PcbIr) -> Vec<NetConnectivity> {
    connectivity(ir)
        .into_iter()
        .filter(|net| !net.is_connected())
        .collect()
}

/// Pins that have no connection record.
pub fn unconnected_pins(ir: &PcbIr) -> Vec<PinRef> {
    let mut connected: BTreeSet<(ObjectId, String)> = BTreeSet::new();
    for connection in &ir.connections {
        connected.insert((connection.pin.component.clone(), connection.pin.pin.clone()));
    }
    let mut result = Vec::new();
    for component in &ir.components {
        let Some(definition) = ir.definition_for(component) else {
            continue;
        };
        for pin in &definition.pins {
            if pin.electrical == PinType::NoConnect {
                continue;
            }
            if !connected.contains(&(component.id.clone(), pin.name.clone())) {
                result.push(PinRef::new(component.id.clone(), pin.name.clone()));
            }
        }
    }
    result
}

/// Run electrical rule checks. Errors block production; warnings do not.
pub fn check_erc(ir: &PcbIr) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    check_pin_maps(ir, &mut diagnostics);
    check_unconnected(ir, &mut diagnostics);
    check_nets(ir, &mut diagnostics);
    check_unrouted(ir, &mut diagnostics);
    check_duplicate_membership(ir, &mut diagnostics);

    diagnostics.sort();
    diagnostics
}

/// Validate the electrical model, returning a report suitable for the CLI.
pub fn validate(ir: &PcbIr) -> ValidationReport {
    ValidationReport::from_diagnostics(check_erc(ir))
}

fn check_pin_maps(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for component in &ir.components {
        let Some(definition) = ir.definition_for(component) else {
            continue;
        };
        for pin in &definition.pins {
            if definition.pad_for_pin(&pin.name).is_none() {
                diagnostics.push(
                    Diagnostic::error(
                        "ERC_001",
                        format!(
                            "pin '{}' on component '{}' has no pad mapping",
                            pin.name, component.designator
                        ),
                    )
                    .with_object(&component.id.0),
                );
            }
        }
    }
}

fn check_unconnected(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for pin in unconnected_pins(ir) {
        let location = pin_ref_position(ir, &pin);
        let designator = ir
            .find_component_by_id(&pin.component)
            .map(|component| component.designator.clone())
            .unwrap_or_else(|| pin.component.0.clone());
        let mut diagnostic = Diagnostic::warning(
            "ERC_010",
            format!("pin '{designator}.{}' is not connected", pin.pin),
        )
        .with_object(&pin.component.0);
        if let Some(location) = location {
            diagnostic = diagnostic.with_location(location);
        }
        diagnostics.push(diagnostic);
    }
}

fn check_nets(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for net in &ir.nets {
        if net.members.is_empty() {
            diagnostics.push(
                Diagnostic::warning("ERC_020", format!("net '{}' has no members", net.name))
                    .with_object(&net.id.0),
            );
            continue;
        }
        if net.members.len() == 1 {
            diagnostics.push(
                Diagnostic::warning(
                    "ERC_021",
                    format!("net '{}' has only one member", net.name),
                )
                .with_object(&net.id.0),
            );
        }

        let mut outputs = Vec::new();
        let mut power_pins = 0usize;
        let mut ground_pins = 0usize;
        for member in &net.members {
            let Some(pin) = ir.pin_definition(member) else {
                continue;
            };
            match pin.electrical {
                PinType::Output => outputs.push(format!("{}.{}", member.component.0, member.pin)),
                PinType::Power => power_pins += 1,
                PinType::Ground => ground_pins += 1,
                _ => {}
            }
            if net.kind == papan_ir::NetKind::Power && pin.electrical == PinType::Ground {
                diagnostics.push(
                    Diagnostic::error(
                        "ERC_022",
                        format!(
                            "ground pin '{}.{}' is connected to power net '{}'",
                            member.component.0, member.pin, net.name
                        ),
                    )
                    .with_object(&net.id.0)
                    .with_suggestion("connect the pin to a ground net"),
                );
            }
            if net.kind == papan_ir::NetKind::Ground && pin.electrical == PinType::Power {
                diagnostics.push(
                    Diagnostic::error(
                        "ERC_023",
                        format!(
                            "power pin '{}.{}' is connected to ground net '{}'",
                            member.component.0, member.pin, net.name
                        ),
                    )
                    .with_object(&net.id.0)
                    .with_suggestion("connect the pin to a power net"),
                );
            }
        }

        if outputs.len() > 1 {
            diagnostics.push(
                Diagnostic::error(
                    "ERC_024",
                    format!(
                        "net '{}' has multiple output drivers: {}",
                        net.name,
                        outputs.join(", ")
                    ),
                )
                .with_object(&net.id.0)
                .with_suggestion("use a single driver or mark the pins bidirectional"),
            );
        }
        if net.kind == papan_ir::NetKind::Power && power_pins == 0 {
            diagnostics.push(
                Diagnostic::warning(
                    "ERC_025",
                    format!("power net '{}' has no power source pin", net.name),
                )
                .with_object(&net.id.0),
            );
        }
        if net.kind == papan_ir::NetKind::Ground && ground_pins == 0 {
            diagnostics.push(
                Diagnostic::warning(
                    "ERC_026",
                    format!("ground net '{}' has no ground pin", net.name),
                )
                .with_object(&net.id.0),
            );
        }
    }
}

fn check_unrouted(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    for net in unrouted_nets(ir) {
        diagnostics.push(
            Diagnostic::warning(
                "ERC_030",
                format!(
                    "net '{}' is not fully routed ({} disconnected groups)",
                    net.net_name,
                    net.groups.len()
                ),
            )
            .with_object(&net.net.0)
            .with_suggestion("run the router or add traces/vias for this net"),
        );
    }
}

fn check_duplicate_membership(ir: &PcbIr, diagnostics: &mut Diagnostics) {
    let mut membership: BTreeMap<(ObjectId, String), Vec<String>> = BTreeMap::new();
    for net in &ir.nets {
        for pin in &net.members {
            membership
                .entry((pin.component.clone(), pin.pin.clone()))
                .or_default()
                .push(net.name.clone());
        }
    }
    for ((component, pin), nets) in membership {
        if nets.len() > 1 {
            diagnostics.push(
                Diagnostic::error(
                    "ERC_040",
                    format!(
                        "pin '{}.{pin}' belongs to multiple nets: {}",
                        component.0,
                        nets.join(", ")
                    ),
                )
                .with_object(&component.0)
                .with_suggestion("leave the pin on a single net"),
            );
        }
    }
}

/// True when a diagnostic should prevent production.
pub fn blocks_production(diagnostic: &Diagnostic) -> bool {
    diagnostic.severity == Severity::Error
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use papan_geometry::Size;
    use papan_ir::{
        Angle, ComponentCategory, ComponentDefinition, ComponentInstance, FootprintDefinition,
        Length, Net, NetKind, PadDefinition, PadKind, PadLayers, PadShape, PinDefinition, Side,
    };
    use papan_types::Point;

    fn definition(name: &str, pins: &[(&str, PinType)]) -> ComponentDefinition {
        ComponentDefinition {
            id: ObjectId::stable("part", name),
            name: name.into(),
            category: ComponentCategory::Ic,
            pins: pins
                .iter()
                .map(|(pin, electrical)| PinDefinition {
                    name: (*pin).into(),
                    number: (*pin).into(),
                    electrical: *electrical,
                    description: String::new(),
                })
                .collect(),
            footprint: FootprintDefinition {
                name: format!("{name}-fp"),
                pads: pins
                    .iter()
                    .enumerate()
                    .map(|(index, (pin, _))| PadDefinition {
                        number: (*pin).into(),
                        pin: (*pin).into(),
                        kind: PadKind::Smd,
                        shape: PadShape::Rect,
                        position: Point::from_mm(index as f64 * 2.0, 0.0),
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

    fn component(name: &str, part: &str, x: f64) -> ComponentInstance {
        ComponentInstance {
            id: ObjectId::stable("component", name),
            designator: name.into(),
            definition_id: ObjectId::stable("part", part),
            value: None,
            properties: BTreeMap::new(),
            position: Some(Point::from_mm(x, 10.0)),
            rotation: Angle::ZERO,
            side: Side::Top,
            locked: false,
            source_span: None,
        }
    }

    fn net(name: &str, kind: NetKind, members: Vec<PinRef>) -> Net {
        Net {
            id: ObjectId::stable("net", name),
            name: name.into(),
            kind,
            voltage: None,
            members,
            properties: BTreeMap::new(),
            source_span: None,
        }
    }

    fn base_ir() -> PcbIr {
        let mut ir = PcbIr::new("Electrical");
        ir.board.width = Length::from_mm(60.0);
        ir.board.height = Length::from_mm(40.0);
        ir.board.outline = papan_geometry::Polygon::rect(ir.board.width, ir.board.height);
        ir.definitions.clear();
        ir.components.clear();
        ir.nets.clear();
        ir.connections.clear();
        ir
    }

    #[test]
    fn trace_connects_pins() {
        let mut ir = base_ir();
        ir.add_definition(definition("chip", &[("1", PinType::Passive), ("2", PinType::Passive)]));
        ir.add_component(component("U1", "chip", 10.0));
        ir.add_component(component("U2", "chip", 20.0));
        let p1 = PinRef::new(ObjectId::stable("component", "U1"), "1");
        let p2 = PinRef::new(ObjectId::stable("component", "U2"), "1");
        ir.add_net(net("N1", NetKind::Other, vec![p1.clone(), p2.clone()]));
        let net_id = ObjectId::stable("net", "N1");
        ir.connect(p1, net_id.clone(), None);
        ir.connect(p2.clone(), net_id.clone(), None);
        // Pads are at component origin, so trace from (10,10) to (20,10).
        ir.traces.push(Trace {
            id: ObjectId::stable("trace", "t1"),
            net: net_id,
            layer: ObjectId::stable("layer", "F.Cu"),
            width: Length::from_mm(0.25),
            points: vec![Point::from_mm(10.0, 10.0), Point::from_mm(20.0, 10.0)],
            source_span: None,
        });
        let connectivity = connectivity(&ir);
        assert_eq!(connectivity.len(), 1);
        assert!(connectivity[0].is_connected(), "{:?}", connectivity[0]);
        assert!(unrouted_nets(&ir).is_empty());
    }

    #[test]
    fn reports_unrouted_net() {
        let mut ir = base_ir();
        ir.add_definition(definition("chip", &[("1", PinType::Passive), ("2", PinType::Passive)]));
        ir.add_component(component("U1", "chip", 10.0));
        ir.add_component(component("U2", "chip", 20.0));
        let p1 = PinRef::new(ObjectId::stable("component", "U1"), "1");
        let p2 = PinRef::new(ObjectId::stable("component", "U2"), "1");
        ir.add_net(net("N1", NetKind::Other, vec![p1.clone(), p2.clone()]));
        let net_id = ObjectId::stable("net", "N1");
        ir.connect(p1, net_id.clone(), None);
        ir.connect(p2, net_id, None);
        assert_eq!(unrouted_nets(&ir).len(), 1);
        assert!(check_erc(&ir).iter().any(|d| d.code == "ERC_030"));
    }

    #[test]
    fn reports_multiple_outputs() {
        let mut ir = base_ir();
        ir.add_definition(definition("driver", &[("Y", PinType::Output)]));
        ir.add_component(component("U1", "driver", 10.0));
        ir.add_component(component("U2", "driver", 20.0));
        let p1 = PinRef::new(ObjectId::stable("component", "U1"), "Y");
        let p2 = PinRef::new(ObjectId::stable("component", "U2"), "Y");
        ir.add_net(net("SIG", NetKind::Digital, vec![p1.clone(), p2.clone()]));
        let net_id = ObjectId::stable("net", "SIG");
        ir.connect(p1, net_id.clone(), None);
        ir.connect(p2, net_id, None);
        assert!(check_erc(&ir).iter().any(|d| d.code == "ERC_024"));
    }

    #[test]
    fn reports_unconnected_pin() {
        let mut ir = base_ir();
        ir.add_definition(definition("chip", &[("1", PinType::Passive)]));
        ir.add_component(component("U1", "chip", 10.0));
        let pins = unconnected_pins(&ir);
        assert_eq!(pins.len(), 1);
        assert!(check_erc(&ir).iter().any(|d| d.code == "ERC_010"));
    }

    #[test]
    fn power_and_ground_mismatch_is_an_error() {
        let mut ir = base_ir();
        ir.add_definition(definition(
            "chip",
            &[("VCC", PinType::Power), ("GND", PinType::Ground)],
        ));
        ir.add_component(component("U1", "chip", 10.0));
        let vcc = PinRef::new(ObjectId::stable("component", "U1"), "VCC");
        ir.add_net(net("GND", NetKind::Ground, vec![vcc.clone()]));
        ir.connect(vcc, ObjectId::stable("net", "GND"), None);
        let erc = check_erc(&ir);
        assert!(erc.iter().any(|d| d.code == "ERC_023"), "{erc:?}");
        assert!(!validate(&ir).production_ready);
    }
}
