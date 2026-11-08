//! PAPAN PCB Intermediate Representation (PIR).
//!
//! The IR stores semantic information rather than only graphical coordinates:
//! component definitions, pin and pad mappings, net membership, geometry,
//! stackup, constraints and routing. Every object has a stable identifier and a
//! schema version. External formats are never allowed to become the source of
//! truth for this model.

use std::collections::BTreeMap;

use papan_geometry::{Polygon, Size};
pub use papan_types::{Angle, Length, ObjectId, Point, Quantity, Span};
use serde::{Deserialize, Serialize};

pub const PIR_SCHEMA_VERSION: u32 = 1;
pub const GENERATOR: &str = "papan 0.1.0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    pub title: String,
    pub schema_version: u32,
    pub generator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            title: "Untitled".to_string(),
            schema_version: PIR_SCHEMA_VERSION,
            generator: GENERATOR.to_string(),
            source_hash: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    pub id: ObjectId,
    pub name: String,
    pub width: Length,
    pub height: Length,
    pub thickness: Length,
    pub layer_count: u32,
    pub outline: Polygon,
}

impl Board {
    pub fn new(name: impl Into<String>, width: Length, height: Length, layers: u32, thickness: Length) -> Self {
        let name = name.into();
        Self {
            id: ObjectId::stable("board", &name),
            name,
            width,
            height,
            thickness,
            layer_count: layers,
            outline: Polygon::rect(width, height),
        }
    }

    pub fn center(&self) -> Point {
        Point::new(
            Length(self.width.0 / 2),
            Length(self.height.0 / 2),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    Copper,
    Dielectric,
    Silkscreen,
    SolderMask,
    PasteMask,
    Mechanical,
}

impl LayerKind {
    pub fn is_copper(self) -> bool {
        self == LayerKind::Copper
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layer {
    pub id: ObjectId,
    pub name: String,
    pub kind: LayerKind,
    pub index: u32,
    pub thickness: Length,
}

impl Layer {
    pub fn is_copper(&self) -> bool {
        self.kind.is_copper()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stackup {
    pub layers: Vec<Layer>,
}

impl Stackup {
    pub fn for_copper_count(count: u32) -> Self {
        let count = count.max(1);
        let mut layers = Vec::new();
        let mut index = 0u32;
        layers.push(Layer {
            id: ObjectId::stable("layer", "F.Cu"),
            name: "F.Cu".to_string(),
            kind: LayerKind::Copper,
            index,
            thickness: Length::from_um(35.0),
        });
        index += 1;
        for inner in 1..count.saturating_sub(1) {
            let name = format!("In{inner}.Cu");
            layers.push(Layer {
                id: ObjectId::stable("layer", &name),
                name,
                kind: LayerKind::Copper,
                index,
                thickness: Length::from_um(35.0),
            });
            index += 1;
        }
        if count > 1 {
            layers.push(Layer {
                id: ObjectId::stable("layer", "B.Cu"),
                name: "B.Cu".to_string(),
                kind: LayerKind::Copper,
                index,
                thickness: Length::from_um(35.0),
            });
            index += 1;
        }
        for (name, kind) in [
            ("F.SilkS", LayerKind::Silkscreen),
            ("B.SilkS", LayerKind::Silkscreen),
            ("F.Mask", LayerKind::SolderMask),
            ("B.Mask", LayerKind::SolderMask),
            ("Edge.Cuts", LayerKind::Mechanical),
        ] {
            layers.push(Layer {
                id: ObjectId::stable("layer", name),
                name: name.to_string(),
                kind,
                index,
                thickness: Length::from_um(10.0),
            });
            index += 1;
        }
        Self { layers }
    }

    pub fn copper_layers(&self) -> Vec<&Layer> {
        let mut layers: Vec<&Layer> = self.layers.iter().filter(|layer| layer.is_copper()).collect();
        layers.sort_by_key(|layer| layer.index);
        layers
    }

    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers
            .iter()
            .find(|layer| layer.name.eq_ignore_ascii_case(name))
    }

    pub fn layer_by_id(&self, id: &ObjectId) -> Option<&Layer> {
        self.layers.iter().find(|layer| &layer.id == id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinType {
    Power,
    Ground,
    Input,
    Output,
    Bidirectional,
    Passive,
    NoConnect,
}

impl PinType {
    pub fn can_drive(self) -> bool {
        matches!(
            self,
            PinType::Output | PinType::Bidirectional | PinType::Power
        )
    }

    pub fn is_power_source(self) -> bool {
        self == PinType::Power
    }

    pub fn is_ground(self) -> bool {
        self == PinType::Ground
    }

    pub fn label(self) -> &'static str {
        match self {
            PinType::Power => "power",
            PinType::Ground => "ground",
            PinType::Input => "input",
            PinType::Output => "output",
            PinType::Bidirectional => "bidirectional",
            PinType::Passive => "passive",
            PinType::NoConnect => "no_connect",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinDefinition {
    pub name: String,
    pub number: String,
    pub electrical: PinType,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadKind {
    Smd,
    ThroughHole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadShape {
    Rect,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadLayers {
    Top,
    Bottom,
    Both,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadDefinition {
    pub number: String,
    pub pin: String,
    pub kind: PadKind,
    pub shape: PadShape,
    /// Position relative to the component origin.
    pub position: Point,
    pub size: Size,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drill: Option<Length>,
    pub layers: PadLayers,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FootprintDefinition {
    pub name: String,
    pub pads: Vec<PadDefinition>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentCategory {
    Resistor,
    Capacitor,
    Inductor,
    Diode,
    Led,
    Switch,
    Crystal,
    Connector,
    Ic,
    Transistor,
    Other,
}

impl ComponentCategory {
    pub fn keyword(self) -> Option<&'static str> {
        match self {
            ComponentCategory::Resistor => Some("resistor"),
            ComponentCategory::Capacitor => Some("capacitor"),
            ComponentCategory::Inductor => Some("inductor"),
            ComponentCategory::Diode => Some("diode"),
            ComponentCategory::Led => Some("led"),
            ComponentCategory::Switch => Some("switch"),
            ComponentCategory::Crystal => Some("crystal"),
            ComponentCategory::Connector => Some("connector"),
            ComponentCategory::Ic => Some("ic"),
            ComponentCategory::Transistor => Some("transistor"),
            ComponentCategory::Other => None,
        }
    }

    pub fn expected_value_kind(self) -> Option<papan_types::UnitKind> {
        use papan_types::UnitKind;
        match self {
            ComponentCategory::Resistor => Some(UnitKind::Resistance),
            ComponentCategory::Capacitor => Some(UnitKind::Capacitance),
            ComponentCategory::Inductor => Some(UnitKind::Inductance),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentDefinition {
    pub id: ObjectId,
    pub name: String,
    pub category: ComponentCategory,
    pub pins: Vec<PinDefinition>,
    pub footprint: FootprintDefinition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datasheet: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl ComponentDefinition {
    pub fn pin(&self, name: &str) -> Option<&PinDefinition> {
        self.pins
            .iter()
            .find(|pin| pin.name.eq_ignore_ascii_case(name) || pin.number == name)
    }

    pub fn pad_for_pin(&self, pin: &str) -> Option<&PadDefinition> {
        self.footprint
            .pads
            .iter()
            .find(|pad| pad.pin.eq_ignore_ascii_case(pin) || pad.number == pin)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PropValue {
    Quantity(Quantity),
    String(String),
    Bool(bool),
    Integer(i64),
    Array(Vec<PropValue>),
}

impl PropValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropValue::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_quantity(&self) -> Option<Quantity> {
        match self {
            PropValue::Quantity(value) => Some(*value),
            _ => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            PropValue::Quantity(value) => value.to_source(),
            PropValue::String(value) => value.clone(),
            PropValue::Bool(value) => value.to_string(),
            PropValue::Integer(value) => value.to_string(),
            PropValue::Array(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(PropValue::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Top,
    Bottom,
}

impl Default for Side {
    fn default() -> Self {
        Side::Top
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentInstance {
    pub id: ObjectId,
    pub designator: String,
    pub definition_id: ObjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Quantity>,
    #[serde(default)]
    pub properties: BTreeMap<String, PropValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Point>,
    pub rotation: Angle,
    pub side: Side,
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<Span>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PinRef {
    pub component: ObjectId,
    pub pin: String,
}

impl PinRef {
    pub fn new(component: ObjectId, pin: impl Into<String>) -> Self {
        Self {
            component,
            pin: pin.into(),
        }
    }

    pub fn display(&self) -> String {
        format!("{}.{}", self.component, self.pin)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetKind {
    Power,
    Ground,
    Digital,
    Analog,
    #[default]
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Net {
    pub id: ObjectId,
    pub name: String,
    pub kind: NetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage: Option<Quantity>,
    #[serde(default)]
    pub members: Vec<PinRef>,
    #[serde(default)]
    pub properties: BTreeMap<String, PropValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<Span>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub id: ObjectId,
    pub pin: PinRef,
    pub net: ObjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<Span>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    pub id: ObjectId,
    pub net: ObjectId,
    pub layer: ObjectId,
    pub width: Length,
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<Span>,
}

impl Trace {
    pub fn segments(&self) -> Vec<papan_geometry::Segment> {
        self.points
            .windows(2)
            .map(|pair| papan_geometry::Segment::new(pair[0], pair[1]))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Via {
    pub id: ObjectId,
    pub net: ObjectId,
    pub at: Point,
    pub drill: Length,
    pub diameter: Length,
    pub from_layer: ObjectId,
    pub to_layer: ObjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<Span>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteDirective {
    pub net: ObjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<ObjectId>,
    pub differential_pair: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impedance: Option<Quantity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<Span>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraints {
    pub min_trace_width: Length,
    pub clearance: Length,
    pub min_via_drill: Length,
    pub min_via_diameter: Length,
    pub copper_edge_clearance: Length,
    pub min_annular_ring: Length,
    pub solder_mask_clearance: Length,
    pub drill_to_drill: Length,
    pub default_via_drill: Length,
    pub default_via_diameter: Length,
}

impl Default for Constraints {
    fn default() -> Self {
        Self {
            min_trace_width: Length::from_mm(0.15),
            clearance: Length::from_mm(0.20),
            min_via_drill: Length::from_mm(0.30),
            min_via_diameter: Length::from_mm(0.60),
            copper_edge_clearance: Length::from_mm(0.25),
            min_annular_ring: Length::from_mm(0.13),
            solder_mask_clearance: Length::from_mm(0.10),
            drill_to_drill: Length::from_mm(0.30),
            default_via_drill: Length::from_mm(0.30),
            default_via_diameter: Length::from_mm(0.60),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PcbIr {
    pub schema_version: u32,
    pub metadata: Metadata,
    pub board: Board,
    pub stackup: Stackup,
    pub definitions: Vec<ComponentDefinition>,
    pub components: Vec<ComponentInstance>,
    pub nets: Vec<Net>,
    pub connections: Vec<Connection>,
    pub traces: Vec<Trace>,
    pub vias: Vec<Via>,
    pub route_directives: Vec<RouteDirective>,
    pub constraints: Constraints,
}

impl PcbIr {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let board = Board::new(
            name.clone(),
            Length::from_mm(100.0),
            Length::from_mm(80.0),
            2,
            Length::from_mm(1.6),
        );
        let stackup = Stackup::for_copper_count(board.layer_count);
        Self {
            schema_version: PIR_SCHEMA_VERSION,
            metadata: Metadata {
                title: name,
                ..Metadata::default()
            },
            board,
            stackup,
            definitions: Vec::new(),
            components: Vec::new(),
            nets: Vec::new(),
            connections: Vec::new(),
            traces: Vec::new(),
            vias: Vec::new(),
            route_directives: Vec::new(),
            constraints: Constraints::default(),
        }
    }

    pub fn find_definition(&self, id: &ObjectId) -> Option<&ComponentDefinition> {
        self.definitions.iter().find(|definition| &definition.id == id)
    }

    pub fn definition_for(&self, component: &ComponentInstance) -> Option<&ComponentDefinition> {
        self.find_definition(&component.definition_id)
    }

    pub fn find_component(&self, designator: &str) -> Option<&ComponentInstance> {
        self.components
            .iter()
            .find(|component| component.designator.eq_ignore_ascii_case(designator))
    }

    pub fn find_component_by_id(&self, id: &ObjectId) -> Option<&ComponentInstance> {
        self.components.iter().find(|component| &component.id == id)
    }

    pub fn find_component_mut(&mut self, designator: &str) -> Option<&mut ComponentInstance> {
        self.components
            .iter_mut()
            .find(|component| component.designator.eq_ignore_ascii_case(designator))
    }

    pub fn find_component_by_id_mut(&mut self, id: &ObjectId) -> Option<&mut ComponentInstance> {
        self.components.iter_mut().find(|component| &component.id == id)
    }

    pub fn find_net(&self, name: &str) -> Option<&Net> {
        self.nets.iter().find(|net| net.name.eq_ignore_ascii_case(name))
    }

    pub fn find_net_by_id(&self, id: &ObjectId) -> Option<&Net> {
        self.nets.iter().find(|net| &net.id == id)
    }

    pub fn find_net_mut(&mut self, name: &str) -> Option<&mut Net> {
        self.nets.iter_mut().find(|net| net.name.eq_ignore_ascii_case(name))
    }

    pub fn find_net_by_id_mut(&mut self, id: &ObjectId) -> Option<&mut Net> {
        self.nets.iter_mut().find(|net| &net.id == id)
    }

    pub fn copper_layers(&self) -> Vec<&Layer> {
        self.stackup.copper_layers()
    }

    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.stackup.layer(name)
    }

    pub fn layer_by_id(&self, id: &ObjectId) -> Option<&Layer> {
        self.stackup.layer_by_id(id)
    }

    pub fn pin_definition(&self, pin: &PinRef) -> Option<&PinDefinition> {
        let component = self.find_component_by_id(&pin.component)?;
        let definition = self.definition_for(component)?;
        definition.pin(&pin.pin)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn add_definition(&mut self, definition: ComponentDefinition) {
        if let Some(existing) = self
            .definitions
            .iter_mut()
            .find(|existing| existing.id == definition.id)
        {
            *existing = definition;
        } else {
            self.definitions.push(definition);
        }
    }

    pub fn add_component(&mut self, component: ComponentInstance) {
        self.components.push(component);
    }

    pub fn add_net(&mut self, net: Net) {
        self.nets.push(net);
    }

    pub fn connect(&mut self, pin: PinRef, net: ObjectId, span: Option<Span>) -> Option<ObjectId> {
        let net_mut = self.find_net_by_id_mut(&net)?;
        if !net_mut.members.contains(&pin) {
            net_mut.members.push(pin.clone());
        }
        let id = ObjectId::stable(
            "connection",
            &format!("{}-{}-{}", pin.component.as_str(), pin.pin, net.as_str()),
        );
        if !self.connections.iter().any(|connection| connection.id == id) {
            self.connections.push(Connection {
                id: id.clone(),
                pin,
                net,
                source_span: span,
            });
        }
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_stackup_has_expected_layers() {
        let stackup = Stackup::for_copper_count(4);
        let copper: Vec<_> = stackup.copper_layers().iter().map(|l| l.name.clone()).collect();
        assert_eq!(copper, vec!["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
        assert!(stackup.layer("Edge.Cuts").is_some());
    }

    #[test]
    fn json_roundtrip_preserves_ir() {
        let ir = PcbIr::new("Demo");
        let json = ir.to_json().unwrap();
        let restored = PcbIr::from_json(&json).unwrap();
        assert_eq!(ir, restored);
    }

    #[test]
    fn connect_adds_member_once() {
        let mut ir = PcbIr::new("Demo");
        ir.add_net(Net {
            id: ObjectId::stable("net", "VCC"),
            name: "VCC".into(),
            kind: NetKind::Power,
            voltage: Some(Quantity::new(papan_types::UnitKind::Voltage, 5_000_000)),
            members: Vec::new(),
            properties: BTreeMap::new(),
            source_span: None,
        });
        let pin = PinRef::new(ObjectId::stable("component", "MCU"), "VCC");
        ir.connect(pin.clone(), ObjectId::stable("net", "VCC"), None);
        ir.connect(pin, ObjectId::stable("net", "VCC"), None);
        assert_eq!(ir.nets[0].members.len(), 1);
        assert_eq!(ir.connections.len(), 1);
    }
}
