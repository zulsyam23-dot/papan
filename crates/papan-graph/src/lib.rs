//! Typed node graph system.
//!
//! The node graph is an alternative *view* of the same semantic design that the
//! textual language describes. It never owns a second source of truth: graph
//! nodes carry typed ports, parameters and stable IDs, and the graph projects
//! deterministically to canonical PAPAN source (which is then compiled into the
//! PIR by the semantic analyzer). Graphs can also be projected from an existing
//! PIR so text edits show up in the node editor.
//!
//! Validation is fully deterministic and reports stable `GRAPH_*` codes.

use std::collections::{BTreeMap, BTreeSet};

use papan_ir::{Angle, Length, ObjectId, PcbIr, Quantity, Span};
use papan_types::{Diagnostic, Diagnostics, UnitKind};
use serde::{Deserialize, Serialize};

pub const GRAPH_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Typed ports
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortType {
    Board,
    Component,
    Pin,
    Pad,
    Net,
    Layer,
    Geometry,
    Constraint,
    Signal,
    Quantity,
    Route,
    Via,
    Footprint,
    ManufacturingArtifact,
}

impl PortType {
    pub fn label(self) -> &'static str {
        match self {
            PortType::Board => "Board",
            PortType::Component => "Component",
            PortType::Pin => "Pin",
            PortType::Pad => "Pad",
            PortType::Net => "Net",
            PortType::Layer => "Layer",
            PortType::Geometry => "Geometry",
            PortType::Constraint => "Constraint",
            PortType::Signal => "Signal",
            PortType::Quantity => "Quantity",
            PortType::Route => "Route",
            PortType::Via => "Via",
            PortType::Footprint => "Footprint",
            PortType::ManufacturingArtifact => "ManufacturingArtifact",
        }
    }

    /// Whether a connection from `self` to `other` is semantically valid.
    ///
    /// The table is intentionally explicit: graph validation, not the UI, is the
    /// authority on which typed connections exist.
    pub fn connects_to(self, other: PortType) -> bool {
        use PortType::*;
        if self == other {
            return matches!(
                self,
                Board
                    | Geometry
                    | Constraint
                    | Quantity
                    | Route
                    | Via
                    | Net
                    | Pin
                    | Pad
                    | Footprint
                    | ManufacturingArtifact
            );
        }
        matches!(
            (self, other),
            (Pin, Net)
                | (Net, Pin)
                | (Pad, Pin)
                | (Pin, Pad)
                | (Net, Route)
                | (Route, Net)
                | (Layer, Route)
                | (Route, Layer)
                | (Net, Via)
                | (Via, Net)
                | (Layer, Via)
                | (Via, Layer)
                | (Pad, Route)
                | (Route, Pad)
        ) || component_port_pair(self, other)
    }
}

fn component_port_pair(a: PortType, b: PortType) -> bool {
    use PortType::*;
    matches!(
        (a, b),
        (Component, Pin)
            | (Pin, Component)
            | (Component, Geometry)
            | (Geometry, Component)
            | (Pin, Route)
            | (Route, Pin)
            | (Constraint, Route)
            | (Route, Constraint)
            | (Constraint, Board)
            | (Board, Constraint)
            | (Signal, Net)
            | (Net, Signal)
            | (Board, Geometry)
            | (Geometry, Board)
            | (Component, ManufacturingArtifact)
            | (Geometry, ManufacturingArtifact)
            | (Board, ManufacturingArtifact)
            | (ManufacturingArtifact, Board)
    )
}

impl std::fmt::Display for PortType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortDirection {
    Input,
    Output,
    #[default]
    InOut,
}

impl PortDirection {
    pub fn label(self) -> &'static str {
        match self {
            PortDirection::Input => "input",
            PortDirection::Output => "output",
            PortDirection::InOut => "inout",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    pub port_type: PortType,
    pub direction: PortDirection,
    pub required: bool,
}

impl Port {
    pub fn new(
        name: impl Into<String>,
        port_type: PortType,
        direction: PortDirection,
    ) -> Self {
        Self {
            name: name.into(),
            port_type,
            direction,
            required: false,
        }
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PortRef {
    pub node: ObjectId,
    pub port: String,
}

impl PortRef {
    pub fn new(node: ObjectId, port: impl Into<String>) -> Self {
        Self {
            node,
            port: port.into(),
        }
    }

    pub fn display(&self) -> String {
        format!("{}.{}", self.node, self.port)
    }
}

// ---------------------------------------------------------------------------
// Node schema
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeCategory {
    Board,
    Component,
    PinPad,
    Electrical,
    Placement,
    Routing,
    Layer,
    Copper,
    Mechanical,
    Rule,
    Simulation,
    Manufacturing,
}

impl NodeCategory {
    pub fn label(self) -> &'static str {
        match self {
            NodeCategory::Board => "board",
            NodeCategory::Component => "component",
            NodeCategory::PinPad => "pin_pad",
            NodeCategory::Electrical => "electrical",
            NodeCategory::Placement => "placement",
            NodeCategory::Routing => "routing",
            NodeCategory::Layer => "layer",
            NodeCategory::Copper => "copper",
            NodeCategory::Mechanical => "mechanical",
            NodeCategory::Rule => "rule",
            NodeCategory::Simulation => "simulation",
            NodeCategory::Manufacturing => "manufacturing",
        }
    }

    /// Stable projection rank: dependencies before dependants.
    pub fn rank(self) -> u8 {
        match self {
            NodeCategory::Board => 0,
            NodeCategory::Layer => 1,
            NodeCategory::Component => 2,
            NodeCategory::PinPad => 3,
            NodeCategory::Electrical => 4,
            NodeCategory::Placement => 5,
            NodeCategory::Routing => 6,
            NodeCategory::Copper => 7,
            NodeCategory::Mechanical => 8,
            NodeCategory::Rule => 9,
            NodeCategory::Simulation => 10,
            NodeCategory::Manufacturing => 11,
        }
    }
}

/// All node kinds from the PRD, in declaration order.
pub const NODE_KINDS: &[(&str, NodeCategory)] = &[
    // Board nodes
    ("Board", NodeCategory::Board),
    ("BoardSize", NodeCategory::Board),
    ("BoardOutline", NodeCategory::Board),
    ("BoardOrigin", NodeCategory::Board),
    ("BoardThickness", NodeCategory::Board),
    ("BoardCutout", NodeCategory::Mechanical),
    ("MountingHole", NodeCategory::Mechanical),
    ("Stackup", NodeCategory::Layer),
    // Component nodes
    ("Component", NodeCategory::Component),
    ("Resistor", NodeCategory::Component),
    ("Capacitor", NodeCategory::Component),
    ("Diode", NodeCategory::Component),
    ("Transistor", NodeCategory::Component),
    ("IC", NodeCategory::Component),
    ("Connector", NodeCategory::Component),
    ("Crystal", NodeCategory::Component),
    ("LED", NodeCategory::Component),
    ("Switch", NodeCategory::Component),
    // Pin and pad nodes
    ("Pin", NodeCategory::PinPad),
    ("Pad", NodeCategory::PinPad),
    ("SMDPad", NodeCategory::PinPad),
    ("ThroughHolePad", NodeCategory::PinPad),
    ("PadArray", NodeCategory::PinPad),
    ("PinMapping", NodeCategory::PinPad),
    // Electrical nodes
    ("Net", NodeCategory::Electrical),
    ("NetClass", NodeCategory::Electrical),
    ("PowerNet", NodeCategory::Electrical),
    ("GroundNet", NodeCategory::Electrical),
    ("DifferentialPair", NodeCategory::Electrical),
    ("NetTie", NodeCategory::Electrical),
    ("Connect", NodeCategory::Electrical),
    // Placement nodes
    ("Place", NodeCategory::Placement),
    ("Move", NodeCategory::Placement),
    ("Rotate", NodeCategory::Placement),
    ("Align", NodeCategory::Placement),
    ("Group", NodeCategory::Placement),
    ("Lock", NodeCategory::Placement),
    ("Keepout", NodeCategory::Placement),
    // Routing nodes
    ("Trace", NodeCategory::Routing),
    ("Route", NodeCategory::Routing),
    ("Via", NodeCategory::Routing),
    ("TrackWidth", NodeCategory::Routing),
    ("Clearance", NodeCategory::Routing),
    ("AutoRoute", NodeCategory::Routing),
    ("DifferentialRoute", NodeCategory::Routing),
    ("LengthMatch", NodeCategory::Routing),
    // Layer nodes
    ("CopperLayer", NodeCategory::Layer),
    ("Dielectric", NodeCategory::Layer),
    ("SolderMask", NodeCategory::Layer),
    ("Silkscreen", NodeCategory::Layer),
    ("PasteMask", NodeCategory::Layer),
    ("LayerStack", NodeCategory::Layer),
    // Copper nodes
    ("CopperZone", NodeCategory::Copper),
    ("GroundPlane", NodeCategory::Copper),
    ("PowerPlane", NodeCategory::Copper),
    ("ThermalRelief", NodeCategory::Copper),
    ("ZoneFill", NodeCategory::Copper),
    // Mechanical nodes
    ("Dimension", NodeCategory::Mechanical),
    ("Hole", NodeCategory::Mechanical),
    ("Slot", NodeCategory::Mechanical),
    ("Edge", NodeCategory::Mechanical),
    ("Cutout", NodeCategory::Mechanical),
    ("Enclosure", NodeCategory::Mechanical),
    ("ThreeDModel", NodeCategory::Mechanical),
    // Rule nodes
    ("DRC", NodeCategory::Rule),
    ("ERC", NodeCategory::Rule),
    ("ClearanceRule", NodeCategory::Rule),
    ("WidthRule", NodeCategory::Rule),
    ("ViaRule", NodeCategory::Rule),
    ("ImpedanceRule", NodeCategory::Rule),
    ("ThermalRule", NodeCategory::Rule),
    // Simulation nodes
    ("VoltageAnalysis", NodeCategory::Simulation),
    ("CurrentAnalysis", NodeCategory::Simulation),
    ("SPICE", NodeCategory::Simulation),
    ("SignalIntegrity", NodeCategory::Simulation),
    ("PowerIntegrity", NodeCategory::Simulation),
    ("ThermalSimulation", NodeCategory::Simulation),
    ("ImpedanceAnalysis", NodeCategory::Simulation),
    // Manufacturing nodes
    ("Gerber", NodeCategory::Manufacturing),
    ("Drill", NodeCategory::Manufacturing),
    ("BOM", NodeCategory::Manufacturing),
    ("PickPlace", NodeCategory::Manufacturing),
    ("AssemblyDrawing", NodeCategory::Manufacturing),
    ("ManufacturingCheck", NodeCategory::Manufacturing),
    ("Export", NodeCategory::Manufacturing),
];

/// A canonical, case-insensitive node kind name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeKind(pub String);

impl NodeKind {
    pub fn new(name: impl AsRef<str>) -> Option<Self> {
        canonical_kind(name.as_ref()).map(|canonical| NodeKind(canonical.to_string()))
    }

    pub fn canonical(&self) -> &str {
        &self.0
    }

    pub fn category(&self) -> Option<NodeCategory> {
        NODE_KINDS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&self.0))
            .map(|(_, category)| *category)
    }

    /// Node kinds that have a deterministic source projection.
    pub fn supported(&self) -> bool {
        matches!(
            self.0.as_str(),
            "Board"
                | "BoardSize"
                | "BoardThickness"
                | "Stackup"
                | "LayerStack"
                | "Component"
                | "Resistor"
                | "Capacitor"
                | "Diode"
                | "Transistor"
                | "IC"
                | "Connector"
                | "Crystal"
                | "LED"
                | "Switch"
                | "Pin"
                | "Net"
                | "PowerNet"
                | "GroundNet"
                | "Connect"
                | "Place"
                | "Move"
                | "Rotate"
                | "Route"
                | "Trace"
                | "Via"
                | "DRC"
                | "ERC"
                | "ClearanceRule"
                | "WidthRule"
                | "ViaRule"
                | "ImpedanceRule"
                | "Export"
        )
    }

    pub fn default_ports(&self) -> Vec<Port> {
        use PortDirection::*;
        use PortType::*;
        let kind = self.0.as_str();
        match kind {
            "Board" => vec![Port::new("board", Board, Output).required(true)],
            "BoardSize" => vec![
                Port::new("board", Board, Input),
                Port::new("geometry", Geometry, Output),
            ],
            "BoardOutline" | "BoardOrigin" | "BoardCutout" => vec![
                Port::new("board", Board, Input),
                Port::new("geometry", Geometry, Output),
            ],
            "BoardThickness" => vec![
                Port::new("board", Board, Input),
                Port::new("thickness", Quantity, Output),
            ],
            "Stackup" | "LayerStack" | "CopperLayer" | "Dielectric" | "SolderMask"
            | "Silkscreen" | "PasteMask" => vec![
                Port::new("board", Board, Input),
                Port::new("layer", Layer, Output),
            ],
            "Component"
            | "Resistor"
            | "Capacitor"
            | "Diode"
            | "Transistor"
            | "IC"
            | "Connector"
            | "Crystal"
            | "LED"
            | "Switch" => vec![
                Port::new("component", Component, Output).required(true),
                Port::new("place", Component, Input),
                Port::new("pin", Pin, Output),
            ],
            "Pin" | "PinMapping" => vec![
                Port::new("component", Component, Input),
                Port::new("pin", Pin, Output),
            ],
            "Pad" | "SMDPad" | "ThroughHolePad" | "PadArray" => vec![
                Port::new("pin", Pin, Input),
                Port::new("pad", Pad, Output),
            ],
            "Net" | "NetClass" | "PowerNet" | "GroundNet" | "DifferentialPair" | "NetTie" => {
                vec![
                    Port::new("net", Net, Output).required(true),
                    Port::new("pins", Pin, Input),
                ]
            }
            "Connect" => vec![
                Port::new("pin", Pin, Input),
                Port::new("net", Net, Input),
            ],
            "Place" | "Move" | "Align" => vec![
                Port::new("component", Component, Input),
                Port::new("constraint", Constraint, Input),
                Port::new("placed", Component, Output),
            ],
            "Rotate" | "Lock" | "Group" | "Keepout" => vec![
                Port::new("component", Component, Input),
                Port::new("placed", Component, Output),
            ],
            "Route" | "Trace" | "AutoRoute" | "DifferentialRoute" | "LengthMatch"
            | "TrackWidth" | "Clearance" => vec![
                Port::new("net", Net, Input),
                Port::new("layer", Layer, Input),
                Port::new("constraint", Constraint, Input),
                Port::new("geometry", Geometry, Output),
            ],
            "Via" => vec![
                Port::new("net", Net, Input),
                Port::new("layer", Layer, Input),
                Port::new("geometry", Geometry, Output),
            ],
            "CopperZone" | "GroundPlane" | "PowerPlane" | "ThermalRelief" | "ZoneFill" => vec![
                Port::new("net", Net, Input),
                Port::new("layer", Layer, Input),
                Port::new("geometry", Geometry, Output),
            ],
            "Dimension" | "Hole" | "Slot" | "Edge" | "Cutout" | "Enclosure" | "ThreeDModel" => {
                vec![
                    Port::new("board", Board, Input),
                    Port::new("geometry", Geometry, Output),
                ]
            }
            "DRC" | "ERC" | "ClearanceRule" | "WidthRule" | "ViaRule" | "ImpedanceRule"
            | "ThermalRule" => vec![
                Port::new("board", Board, Input),
                Port::new("constraint", Constraint, Output),
            ],
            "VoltageAnalysis" | "CurrentAnalysis" | "SPICE" | "SignalIntegrity"
            | "PowerIntegrity" | "ThermalSimulation" | "ImpedanceAnalysis" => vec![
                Port::new("board", Board, Input),
                Port::new("signal", Signal, Input),
                Port::new("result", Quantity, Output),
            ],
            "Gerber" | "Drill" | "BOM" | "PickPlace" | "AssemblyDrawing"
            | "ManufacturingCheck" | "Export" => vec![
                Port::new("board", Board, Input),
                Port::new("geometry", Geometry, Input),
                Port::new("component", Component, Input),
                Port::new("artifact", ManufacturingArtifact, Output),
            ],
            _ => vec![
                Port::new("input", Quantity, Input),
                Port::new("output", Quantity, Output),
            ],
        }
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn canonical_kind(name: &str) -> Option<&'static str> {
    let normalized = name.replace([' ', '-', '_'], "");
    NODE_KINDS
        .iter()
        .find(|(canonical, _)| {
            canonical.eq_ignore_ascii_case(name) || canonical.replace('_', "").eq_ignore_ascii_case(&normalized)
        })
        .map(|(canonical, _)| *canonical)
}

/// Every node kind in canonical order.
pub fn all_node_kinds() -> Vec<NodeKind> {
    NODE_KINDS
        .iter()
        .map(|(name, _)| NodeKind((*name).to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ParamValue {
    Quantity(Quantity),
    Length(Length),
    Angle(Angle),
    String(String),
    Identifier(String),
    Bool(bool),
    Integer(i64),
    Array(Vec<ParamValue>),
}

impl ParamValue {
    pub fn display(&self) -> String {
        match self {
            ParamValue::Quantity(value) => value.to_source(),
            ParamValue::Length(value) => value.to_source(),
            ParamValue::Angle(value) => value.to_source(),
            ParamValue::String(value) => format!("\"{}\"", escape(value)),
            ParamValue::Identifier(value) => value.clone(),
            ParamValue::Bool(value) => value.to_string(),
            ParamValue::Integer(value) => value.to_string(),
            ParamValue::Array(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(ParamValue::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            ParamValue::String(value) | ParamValue::Identifier(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_quantity(&self) -> Option<Quantity> {
        match self {
            ParamValue::Quantity(value) => Some(*value),
            ParamValue::Length(value) => Some(Quantity::from_length(*value)),
            ParamValue::Angle(value) => Some(Quantity::from_angle(*value)),
            _ => None,
        }
    }

    pub fn as_length(&self) -> Option<Length> {
        match self {
            ParamValue::Length(value) => Some(*value),
            ParamValue::Quantity(value) => value.as_length(),
            _ => None,
        }
    }

    pub fn as_angle(&self) -> Option<Angle> {
        match self {
            ParamValue::Angle(value) => Some(*value),
            ParamValue::Quantity(value) => value.as_angle(),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        match self {
            ParamValue::Integer(value) => Some(*value),
            ParamValue::Quantity(value) if value.kind == UnitKind::Dimensionless => {
                Some(value.value)
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Nodes and connections
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodePosition {
    /// Layout only; never affects the generated design.
    pub x: i64,
    pub y: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: ObjectId,
    pub kind: NodeKind,
    pub name: String,
    #[serde(default)]
    pub position: NodePosition,
    #[serde(default)]
    pub parameters: BTreeMap<String, ParamValue>,
    #[serde(default)]
    pub ports: Vec<Port>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<Span>,
}

impl Node {
    pub fn new(kind: NodeKind, name: impl Into<String>) -> Self {
        let name = name.into();
        let id = ObjectId::stable("node", &format!("{}:{}", kind.canonical(), name));
        let ports = kind.default_ports();
        Self {
            id,
            kind,
            name,
            position: NodePosition::default(),
            parameters: BTreeMap::new(),
            ports,
            comment: None,
            group: None,
            source_span: None,
        }
    }

    pub fn with_id(mut self, id: ObjectId) -> Self {
        self.id = id;
        self
    }

    pub fn with_parameter(mut self, key: impl Into<String>, value: ParamValue) -> Self {
        self.parameters.insert(key.into(), value);
        self
    }

    pub fn with_position(mut self, x: i64, y: i64) -> Self {
        self.position = NodePosition { x, y };
        self
    }

    pub fn port(&self, name: &str) -> Option<&Port> {
        self.ports
            .iter()
            .find(|port| port.name.eq_ignore_ascii_case(name))
    }

    pub fn parameter(&self, key: &str) -> Option<&ParamValue> {
        self.parameters
            .get(key)
            .or_else(|| {
                self.parameters
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(key))
                    .map(|(_, value)| value)
            })
    }

    pub fn string(&self, key: &str) -> Option<&str> {
        self.parameter(key).and_then(ParamValue::as_str)
    }

    pub fn length(&self, key: &str) -> Option<Length> {
        self.parameter(key).and_then(ParamValue::as_length)
    }

    pub fn angle(&self, key: &str) -> Option<Angle> {
        self.parameter(key).and_then(ParamValue::as_angle)
    }

    pub fn integer(&self, key: &str) -> Option<i64> {
        self.parameter(key).and_then(ParamValue::as_integer)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphConnection {
    pub id: ObjectId,
    pub from: PortRef,
    pub to: PortRef,
}

impl GraphConnection {
    pub fn new(from: PortRef, to: PortRef) -> Self {
        let id = ObjectId::stable(
            "connection",
            &format!("{}-{}-{}-{}", from.node, from.port, to.node, to.port),
        );
        Self { id, from, to }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeGroup {
    pub id: ObjectId,
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub members: Vec<ObjectId>,
}

impl NodeGroup {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: ObjectId::stable("group", &name),
            name,
            collapsed: false,
            members: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphMetadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl Default for GraphMetadata {
    fn default() -> Self {
        Self {
            name: "Untitled".to_string(),
            description: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeGraph {
    pub schema_version: u32,
    pub metadata: GraphMetadata,
    pub nodes: Vec<Node>,
    pub connections: Vec<GraphConnection>,
    #[serde(default)]
    pub groups: Vec<NodeGroup>,
}

impl Default for NodeGraph {
    fn default() -> Self {
        Self::new("Untitled")
    }
}

impl NodeGraph {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema_version: GRAPH_SCHEMA_VERSION,
            metadata: GraphMetadata {
                name: name.into(),
                description: String::new(),
            },
            nodes: Vec::new(),
            connections: Vec::new(),
            groups: Vec::new(),
        }
    }

    pub fn add_node(&mut self, kind: NodeKind, name: impl Into<String>) -> ObjectId {
        let mut node = Node::new(kind, name);
        let base = node.id.clone();
        let mut counter = 2usize;
        while self.node(&node.id).is_some() {
            node.id = ObjectId::new(format!("{}#{}", base.as_str(), counter));
            counter += 1;
        }
        let id = node.id.clone();
        self.nodes.push(node);
        id
    }

    pub fn add(&mut self, node: Node) -> ObjectId {
        let id = node.id.clone();
        if self.node(&id).is_some() {
            return self.add_node(node.kind.clone(), node.name.clone());
        }
        self.nodes.push(node);
        id
    }

    pub fn node(&self, id: &ObjectId) -> Option<&Node> {
        self.nodes.iter().find(|node| &node.id == id)
    }

    pub fn node_mut(&mut self, id: &ObjectId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|node| &node.id == id)
    }

    pub fn node_named(&self, name: &str) -> Option<&Node> {
        self.nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case(name))
            .or_else(|| {
                self.nodes
                    .iter()
                    .find(|node| node.id.as_str().eq_ignore_ascii_case(name))
            })
    }

    pub fn nodes_of_kind(&self, kind: &str) -> Vec<&Node> {
        self.nodes
            .iter()
            .filter(|node| node.kind.canonical().eq_ignore_ascii_case(kind))
            .collect()
    }

    pub fn remove_node(&mut self, id: &ObjectId) -> Option<Node> {
        let index = self.nodes.iter().position(|node| &node.id == id)?;
        self.connections
            .retain(|connection| &connection.from.node != id && &connection.to.node != id);
        Some(self.nodes.remove(index))
    }

    pub fn connect(&mut self, from: PortRef, to: PortRef) -> Result<ObjectId, Diagnostic> {
        self.validate_connection(&from, &to)?;
        let connection = GraphConnection::new(from, to);
        if self
            .connections
            .iter()
            .any(|existing| existing.from == connection.from && existing.to == connection.to)
        {
            return Ok(connection.id);
        }
        let id = connection.id.clone();
        self.connections.push(connection);
        Ok(id)
    }

    pub fn disconnect(&mut self, from: &PortRef, to: &PortRef) -> bool {
        let before = self.connections.len();
        self.connections
            .retain(|connection| &connection.from != from || &connection.to != to);
        before != self.connections.len()
    }

    pub fn connections_from<'a>(
        &'a self,
        node: &'a ObjectId,
        port: &'a str,
    ) -> impl Iterator<Item = &'a GraphConnection> + 'a {
        self.connections.iter().filter(move |connection| {
            &connection.from.node == node && connection.from.port.eq_ignore_ascii_case(port)
        })
    }

    pub fn connections_to<'a>(
        &'a self,
        node: &'a ObjectId,
        port: &'a str,
    ) -> impl Iterator<Item = &'a GraphConnection> + 'a {
        self.connections.iter().filter(move |connection| {
            &connection.to.node == node && connection.to.port.eq_ignore_ascii_case(port)
        })
    }

    fn validate_connection(&self, from: &PortRef, to: &PortRef) -> Result<(), Diagnostic> {
        if from.node == to.node && from.port.eq_ignore_ascii_case(&to.port) {
            return Err(Diagnostic::error(
                "GRAPH_020",
                "a node port cannot be connected to itself",
            ));
        }
        let from_node = self.node(&from.node).ok_or_else(|| {
            Diagnostic::error(
                "GRAPH_021",
                format!("connection source node '{}' does not exist", from.node),
            )
        })?;
        let to_node = self.node(&to.node).ok_or_else(|| {
            Diagnostic::error(
                "GRAPH_022",
                format!("connection target node '{}' does not exist", to.node),
            )
        })?;
        let from_port = from_node.port(&from.port).ok_or_else(|| {
            Diagnostic::error(
                "GRAPH_023",
                format!(
                    "node '{}' has no output port '{}'",
                    from_node.name, from.port
                ),
            )
        })?;
        let to_port = to_node.port(&to.port).ok_or_else(|| {
            Diagnostic::error(
                "GRAPH_024",
                format!("node '{}' has no input port '{}'", to_node.name, to.port),
            )
        })?;
        if from_port.direction == PortDirection::Input
            && to_port.direction == PortDirection::Input
        {
            return Err(Diagnostic::error(
                "GRAPH_025",
                format!(
                    "cannot connect input '{}' to input '{}'",
                    from_port.name, to_port.name
                ),
            ));
        }
        if from_port.direction == PortDirection::Output
            && to_port.direction == PortDirection::Output
        {
            return Err(Diagnostic::error(
                "GRAPH_026",
                format!(
                    "cannot connect output '{}' to output '{}'",
                    from_port.name, to_port.name
                ),
            ));
        }
        if !from_port.port_type.connects_to(to_port.port_type) {
            return Err(Diagnostic::error(
                "GRAPH_027",
                format!(
                    "typed connection {} -> {} is invalid",
                    from_port.port_type, to_port.port_type
                ),
            )
            .with_object(&from_node.id.0)
            .with_suggestion(format!(
                "connect compatible ports; {} cannot feed {}",
                from_port.port_type, to_port.port_type
            )));
        }
        Ok(())
    }

    /// Validate graph structure, ports, connections and node parameters.
    pub fn validate(&self) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        if self.schema_version != GRAPH_SCHEMA_VERSION {
            diagnostics.push(Diagnostic::error(
                "GRAPH_001",
                format!(
                    "unsupported graph schema version {} (expected {})",
                    self.schema_version, GRAPH_SCHEMA_VERSION
                ),
            ));
        }
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            if node.id.is_empty() {
                diagnostics.push(Diagnostic::error("GRAPH_002", "node has an empty identifier"));
            }
            if !ids.insert(node.id.clone()) {
                diagnostics.push(
                    Diagnostic::error(
                        "GRAPH_003",
                        format!("duplicate node identifier '{}'", node.id),
                    )
                    .with_object(&node.id.0),
                );
            }
            let mut port_names = BTreeSet::new();
            for port in &node.ports {
                if !port_names.insert(port.name.to_ascii_lowercase()) {
                    diagnostics.push(
                        Diagnostic::error(
                            "GRAPH_004",
                            format!(
                                "node '{}' declares duplicate port '{}'",
                                node.name, port.name
                            ),
                        )
                        .with_object(&node.id.0),
                    );
                }
            }
            if node.ports.is_empty() {
                diagnostics.push(
                    Diagnostic::warning(
                        "GRAPH_005",
                        format!("node '{}' has no typed ports", node.name),
                    )
                    .with_object(&node.id.0),
                );
            }
        }
        let mut connection_ids = BTreeSet::new();
        for connection in &self.connections {
            if !connection_ids.insert(connection.id.clone()) {
                diagnostics.push(
                    Diagnostic::error(
                        "GRAPH_006",
                        format!("duplicate connection identifier '{}'", connection.id),
                    )
                    .with_object(&connection.id.0),
                );
            }
            if let Err(diagnostic) = self.validate_connection(&connection.from, &connection.to) {
                diagnostics.push(diagnostic);
            }
        }
        for group in &self.groups {
            for member in &group.members {
                if self.node(member).is_none() {
                    diagnostics.push(
                        Diagnostic::warning(
                            "GRAPH_007",
                            format!(
                                "group '{}' references missing node '{}'",
                                group.name, member
                            ),
                        )
                        .with_object(&group.id.0),
                    );
                }
            }
        }
        diagnostics.sort();
        diagnostics
    }

    /// Deterministic automatic layout grouped by category.
    pub fn auto_layout(&mut self) {
        let mut nodes: Vec<usize> = (0..self.nodes.len()).collect();
        nodes.sort_by(|a, b| {
            let left = &self.nodes[*a];
            let right = &self.nodes[*b];
            let left_rank = left.kind.category().map(NodeCategory::rank).unwrap_or(99);
            let right_rank = right.kind.category().map(NodeCategory::rank).unwrap_or(99);
            left_rank
                .cmp(&right_rank)
                .then_with(|| left.kind.canonical().cmp(right.kind.canonical()))
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut columns: BTreeMap<u8, i64> = BTreeMap::new();
        for index in nodes {
            let rank = self.nodes[index]
                .kind
                .category()
                .map(NodeCategory::rank)
                .unwrap_or(99);
            let row = columns.entry(rank).or_insert(0);
            self.nodes[index].position = NodePosition {
                x: rank as i64 * 260,
                y: *row * 140,
            };
            *row += 1;
        }
    }

    /// Structural diff between two graphs.
    pub fn diff(&self, other: &NodeGraph) -> GraphDiff {
        let self_nodes: BTreeSet<&ObjectId> = self.nodes.iter().map(|node| &node.id).collect();
        let other_nodes: BTreeSet<&ObjectId> = other.nodes.iter().map(|node| &node.id).collect();
        let self_connections: BTreeSet<&ObjectId> =
            self.connections.iter().map(|connection| &connection.id).collect();
        let other_connections: BTreeSet<&ObjectId> = other
            .connections
            .iter()
            .map(|connection| &connection.id)
            .collect();
        GraphDiff {
            added_nodes: other_nodes
                .difference(&self_nodes)
                .map(|id| (*id).clone())
                .collect(),
            removed_nodes: self_nodes
                .difference(&other_nodes)
                .map(|id| (*id).clone())
                .collect(),
            added_connections: other_connections
                .difference(&self_connections)
                .map(|id| (*id).clone())
                .collect(),
            removed_connections: self_connections
                .difference(&other_connections)
                .map(|id| (*id).clone())
                .collect(),
        }
    }

    // -----------------------------------------------------------------------
    // Text projection
    // -----------------------------------------------------------------------

    /// Project the graph to canonical PAPAN source.
    ///
    /// Nodes are ordered by category and name, so identical graphs always
    /// produce byte-identical source. Unsupported node kinds are skipped with an
    /// informational diagnostic instead of failing the whole projection.
    pub fn to_source(&self) -> SourceProjection {
        let mut diagnostics = Diagnostics::new();
        let mut lines: Vec<(u8, String, String, String)> = Vec::new();

        let board_nodes = self.nodes_of_kind("Board");
        let board_name = board_nodes
            .first()
            .and_then(|node| node.string("name").or(Some(node.name.as_str())))
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(self.metadata.name.as_str());
        lines.push((
            0,
            "00".to_string(),
            "board".to_string(),
            format!("board \"{}\"", escape(board_name)),
        ));
        if board_nodes.len() > 1 {
            diagnostics.push(Diagnostic::warning(
                "GRAPH_030",
                "multiple Board nodes found; using the first deterministically",
            ));
        }

        for node in &self.nodes {
            let kind = node.kind.canonical();
            match kind {
                "Board" => {}
                "BoardSize" => match (node.length("width"), node.length("height")) {
                    (Some(width), Some(height)) => lines.push((
                        0,
                        "01".to_string(),
                        node.id.0.clone(),
                        format!(
                            "size {} x {}",
                            width.to_source(),
                            height.to_source()
                        ),
                    )),
                    _ => diagnostics.push(missing_params(node, "width and height")),
                },
                "BoardThickness" => match node.length("thickness") {
                    Some(thickness) => lines.push((
                        0,
                        "02".to_string(),
                        node.id.0.clone(),
                        format!("thickness {}", thickness.to_source()),
                    )),
                    None => diagnostics.push(missing_params(node, "thickness")),
                },
                "Stackup" | "LayerStack" => match node.integer("layers") {
                    Some(layers) if layers > 0 => lines.push((
                        0,
                        "03".to_string(),
                        node.id.0.clone(),
                        format!("layers {}", layers),
                    )),
                    Some(_) => diagnostics.push(Diagnostic::error(
                        "GRAPH_031",
                        format!("node '{}' layer count must be positive", node.name),
                    )),
                    None => diagnostics.push(missing_params(node, "layers")),
                },
                "Component" | "Resistor" | "Capacitor" | "Diode" | "Transistor" | "IC"
                | "Connector" | "Crystal" | "LED" | "Switch" => {
                    match component_statement(node, kind) {
                        Ok(statement) => lines.push((
                            1,
                            "10".to_string(),
                            format!("{}:{}", node.kind, node.name),
                            statement,
                        )),
                        Err(diagnostic) => diagnostics.push(diagnostic),
                    }
                }
                "Net" | "PowerNet" | "GroundNet" => match net_statement(node, kind) {
                    Ok(statement) => lines.push((
                        2,
                        "20".to_string(),
                        format!("{}:{}", node.kind, node.name),
                        statement,
                    )),
                    Err(diagnostic) => diagnostics.push(diagnostic),
                },
                "Connect" => match (node.string("from"), node.string("to")) {
                    (Some(from), Some(to)) => lines.push((
                        3,
                        "30".to_string(),
                        node.id.0.clone(),
                        format!("connect {} -> {}", from, to),
                    )),
                    _ => diagnostics.push(missing_params(node, "from and to")),
                },
                "Place" | "Move" => match place_statement(node) {
                    Ok(statement) => lines.push((
                        4,
                        "40".to_string(),
                        format!("place:{}", node.name),
                        statement,
                    )),
                    Err(diagnostic) => diagnostics.push(diagnostic),
                },
                "Rotate" => {
                    let Some(component) = component_name(node) else {
                        diagnostics.push(missing_params(node, "component"));
                        continue;
                    };
                    let Some(angle) = node.angle("angle").or_else(|| node.angle("rotation")) else {
                        diagnostics.push(missing_params(node, "angle"));
                        continue;
                    };
                    lines.push((
                        4,
                        "41".to_string(),
                        format!("rotate:{}", node.name),
                        format!("rotate {} {}", component, angle.to_source()),
                    ));
                }
                "Route" | "Trace" => match route_statement(node) {
                    Ok(statement) => lines.push((
                        5,
                        "50".to_string(),
                        format!("route:{}", node.name),
                        statement,
                    )),
                    Err(diagnostic) => diagnostics.push(diagnostic),
                },
                "Via" => match via_statement(node) {
                    Ok(statement) => lines.push((
                        5,
                        "51".to_string(),
                        format!("via:{}", node.id),
                        statement,
                    )),
                    Err(diagnostic) => diagnostics.push(diagnostic),
                },
                "DRC" | "ERC" | "ClearanceRule" | "WidthRule" | "ViaRule" | "ImpedanceRule"
                | "ThermalRule" => {
                    if let Some(statement) = constraints_statement(node) {
                        lines.push((
                            6,
                            "60".to_string(),
                            format!("constraints:{}", node.id),
                            statement,
                        ));
                    }
                }
                "Export" => {
                    if let Some(target) = node.string("target") {
                        lines.push((
                            7,
                            "70".to_string(),
                            format!("export:{}", node.id),
                            format!("export {}", target),
                        ));
                    }
                }
                _ if !node.kind.supported() => {
                    // Nodes for simulation, copper zones and mechanical
                    // features are preserved in the graph but not in the
                    // current source language subset.
                }
                _ => {
                    diagnostics.push(Diagnostic::info(
                        "GRAPH_050",
                        format!(
                            "node '{}' of kind '{}' has no source projection and was skipped",
                            node.name, kind
                        ),
                    ));
                }
            }
        }

        // Explicit Connect nodes and Pin -> Net graph edges both become
        // `connect` statements. Edges are deduplicated and emitted in a stable
        // order.
        let mut edge_connects: Vec<(String, String)> = Vec::new();
        for connection in &self.connections {
            let Some(from_node) = self.node(&connection.from.node) else {
                continue;
            };
            let Some(to_node) = self.node(&connection.to.node) else {
                continue;
            };
            if from_node.kind.canonical() == "Pin"
                && matches!(
                    to_node.kind.canonical(),
                    "Net" | "PowerNet" | "GroundNet" | "NetClass"
                )
                && connection.from.port.eq_ignore_ascii_case("pin")
            {
                let Some(component) = component_name(from_node) else {
                    continue;
                };
                let Some(pin) = from_node.string("pin").map(str::to_string) else {
                    continue;
                };
                let net = to_node
                    .string("name")
                    .unwrap_or(to_node.name.as_str())
                    .to_string();
                edge_connects.push((format!("{}.{}", component, pin), net));
            }
        }
        edge_connects.sort();
        edge_connects.dedup();
        for (pin, net) in edge_connects {
            lines.push((
                3,
                "31".to_string(),
                format!("edge:{}:{}", pin, net),
                format!("connect {} -> {}", pin, net),
            ));
        }

        lines.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| a.3.cmp(&b.3))
        });
        let mut source = String::new();
        for (_, _, _, line) in lines {
            source.push_str(&line);
            source.push('\n');
        }
        diagnostics.sort();
        SourceProjection { source, diagnostics }
    }

    // -----------------------------------------------------------------------
    // IR projection
    // -----------------------------------------------------------------------

    /// Build a node graph view from the PCB IR.
    ///
    /// This is the text -> IR -> node projection direction required by the
    /// synchronization rules. Purely physical results (traces and vias) are
    /// derived data and are not projected as editable source nodes.
    pub fn from_ir(ir: &PcbIr) -> NodeGraph {
        let mut graph = NodeGraph::new(&ir.board.name);
        let mut board = Node::new(NodeKind::new("Board").unwrap(), &ir.board.name)
            .with_parameter("name", ParamValue::String(ir.board.name.clone()));
        board.position = NodePosition { x: 0, y: 0 };
        graph.add(board);

        graph.add(
            Node::new(NodeKind::new("BoardSize").unwrap(), "Board Size")
                .with_parameter("width", ParamValue::Length(ir.board.width))
                .with_parameter("height", ParamValue::Length(ir.board.height))
                .with_position(260, 0),
        );
        graph.add(
            Node::new(NodeKind::new("BoardThickness").unwrap(), "Board Thickness")
                .with_parameter("thickness", ParamValue::Length(ir.board.thickness))
                .with_position(260, 140),
        );
        graph.add(
            Node::new(NodeKind::new("Stackup").unwrap(), "Stackup")
                .with_parameter("layers", ParamValue::Integer(ir.board.layer_count as i64))
                .with_position(260, 280),
        );

        for (index, component) in ir.components.iter().enumerate() {
            let definition = ir.definition_for(component);
            let part = definition
                .map(|definition| definition.name.clone())
                .unwrap_or_else(|| "generic".to_string());
            let mut node = Node::new(NodeKind::new("Component").unwrap(), &component.designator)
                .with_parameter("designator", ParamValue::String(component.designator.clone()))
                .with_parameter("part", ParamValue::String(part));
            if let Some(value) = component.value {
                node.parameters
                    .insert("value".to_string(), ParamValue::Quantity(value));
            }
            node.position = NodePosition {
                x: 520,
                y: index as i64 * 140,
            };
            graph.add(node);

            if let Some(position) = component.position {
                let mut place = Node::new(
                    NodeKind::new("Place").unwrap(),
                    format!("Place {}", component.designator),
                )
                .with_parameter(
                    "component",
                    ParamValue::Identifier(component.designator.clone()),
                )
                .with_parameter("x", ParamValue::Length(position.x))
                .with_parameter("y", ParamValue::Length(position.y));
                if component.rotation.0 != 0 {
                    place.parameters.insert(
                        "rotation".to_string(),
                        ParamValue::Angle(component.rotation),
                    );
                }
                place.position = NodePosition {
                    x: 780,
                    y: index as i64 * 140,
                };
                graph.add(place);
            }
        }

        for (index, net) in ir.nets.iter().enumerate() {
            let kind = match net.kind {
                papan_ir::NetKind::Power => "PowerNet",
                papan_ir::NetKind::Ground => "GroundNet",
                _ => "Net",
            };
            let mut node = Node::new(NodeKind::new(kind).unwrap(), &net.name)
                .with_parameter("name", ParamValue::String(net.name.clone()))
                .with_parameter(
                    "type",
                    ParamValue::Identifier(
                        match net.kind {
                            papan_ir::NetKind::Power => "power",
                            papan_ir::NetKind::Ground => "ground",
                            papan_ir::NetKind::Digital => "digital",
                            papan_ir::NetKind::Analog => "analog",
                            papan_ir::NetKind::Other => "other",
                        }
                        .to_string(),
                    ),
                );
            if let Some(voltage) = net.voltage {
                node.parameters
                    .insert("voltage".to_string(), ParamValue::Quantity(voltage));
            }
            node.position = NodePosition {
                x: 1040,
                y: index as i64 * 140,
            };
            graph.add(node);
        }

        // Pin nodes and typed Pin -> Net edges for every connected pin.
        let mut pins: BTreeSet<(String, String)> = BTreeSet::new();
        for connection in &ir.connections {
            if let Some(component) = ir.find_component_by_id(&connection.pin.component) {
                pins.insert((component.designator.clone(), connection.pin.pin.clone()));
            }
        }
        for net in &ir.nets {
            for pin in &net.members {
                if let Some(component) = ir.find_component_by_id(&pin.component) {
                    pins.insert((component.designator.clone(), pin.pin.clone()));
                }
            }
        }
        for (index, (designator, pin_name)) in pins.into_iter().enumerate() {
            let mut pin = Node::new(
                NodeKind::new("Pin").unwrap(),
                format!("{}.{}", designator, pin_name),
            )
            .with_parameter("component", ParamValue::Identifier(designator.clone()))
            .with_parameter("pin", ParamValue::String(pin_name.clone()));
            pin.position = NodePosition {
                x: 780,
                y: 520 + index as i64 * 140,
            };
            let pin_id = graph.add(pin);
            let connected_nets: Vec<&papan_ir::Net> = ir
                .nets
                .iter()
                .filter(|net| {
                    net.members.iter().any(|member| {
                        member.pin == pin_name
                            && ir
                                .find_component_by_id(&member.component)
                                .map(|component| component.designator == designator)
                                .unwrap_or(false)
                    })
                })
                .collect();
            for net in connected_nets {
                if let Some(net_node) = graph.node_named(&net.name) {
                    let _ = graph.connect(
                        PortRef::new(pin_id.clone(), "pin"),
                        PortRef::new(net_node.id.clone(), "pins"),
                    );
                }
            }
        }

        for directive in &ir.route_directives {
            let Some(net) = ir.find_net_by_id(&directive.net) else {
                continue;
            };
            let mut node = Node::new(NodeKind::new("Route").unwrap(), format!("Route {}", net.name))
                .with_parameter("net", ParamValue::Identifier(net.name.clone()));
            if let Some(width) = directive.width {
                node.parameters
                    .insert("width".to_string(), ParamValue::Length(width));
            }
            if let Some(layer_id) = &directive.layer {
                if let Some(layer) = ir.layer_by_id(layer_id) {
                    node.parameters.insert(
                        "layer".to_string(),
                        ParamValue::Identifier(layer.name.clone()),
                    );
                }
            }
            if let Some(length) = directive.length {
                node.parameters
                    .insert("length".to_string(), ParamValue::Length(length));
            }
            if directive.differential_pair {
                node.parameters.insert(
                    "differential_pair".to_string(),
                    ParamValue::Bool(true),
                );
            }
            if let Some(impedance) = directive.impedance {
                node.parameters
                    .insert("impedance".to_string(), ParamValue::Quantity(impedance));
            }
            node.position = NodePosition { x: 1300, y: 0 };
            graph.add(node);
        }

        let constraints = &ir.constraints;
        graph.add(
            Node::new(NodeKind::new("DRC").unwrap(), "Constraints")
                .with_parameter(
                    "min_trace_width",
                    ParamValue::Length(constraints.min_trace_width),
                )
                .with_parameter("clearance", ParamValue::Length(constraints.clearance))
                .with_parameter("min_via_drill", ParamValue::Length(constraints.min_via_drill))
                .with_parameter(
                    "min_via_diameter",
                    ParamValue::Length(constraints.min_via_diameter),
                )
                .with_parameter(
                    "copper_edge_clearance",
                    ParamValue::Length(constraints.copper_edge_clearance),
                )
                .with_parameter(
                    "min_annular_ring",
                    ParamValue::Length(constraints.min_annular_ring),
                )
                .with_parameter(
                    "solder_mask_clearance",
                    ParamValue::Length(constraints.solder_mask_clearance),
                )
                .with_parameter("drill_to_drill", ParamValue::Length(constraints.drill_to_drill))
                .with_parameter(
                    "default_via_drill",
                    ParamValue::Length(constraints.default_via_drill),
                )
                .with_parameter(
                    "default_via_diameter",
                    ParamValue::Length(constraints.default_via_diameter),
                )
                .with_position(0, 520),
        );

        graph
    }

    /// Convenience wrapper around [`NodeGraph::from_ir`] and
    /// [`NodeGraph::to_source`].
    pub fn project_ir_to_source(ir: &PcbIr) -> SourceProjection {
        NodeGraph::from_ir(ir).to_source()
    }

    // -----------------------------------------------------------------------
    // Serialization
    // -----------------------------------------------------------------------

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_json(&text).map_err(std::io::Error::other)
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let json = self.to_json().map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphDiff {
    pub added_nodes: Vec<ObjectId>,
    pub removed_nodes: Vec<ObjectId>,
    pub added_connections: Vec<ObjectId>,
    pub removed_connections: Vec<ObjectId>,
}

impl GraphDiff {
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.added_connections.is_empty()
            && self.removed_connections.is_empty()
    }
}

/// Result of projecting a graph to source text.
#[derive(Clone, Debug)]
pub struct SourceProjection {
    pub source: String,
    pub diagnostics: Diagnostics,
}

impl SourceProjection {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

// ---------------------------------------------------------------------------
// Projection helpers
// ---------------------------------------------------------------------------

fn missing_params(node: &Node, params: &str) -> Diagnostic {
    Diagnostic::error(
        "GRAPH_040",
        format!(
            "node '{}' of kind '{}' is missing required parameter(s): {}",
            node.name, node.kind, params
        ),
    )
    .with_object(&node.id.0)
}

fn component_statement(node: &Node, kind: &str) -> Result<String, Diagnostic> {
    let designator = node
        .string("designator")
        .unwrap_or(node.name.as_str())
        .to_string();
    if !is_path_segment(&designator) {
        return Err(Diagnostic::error(
            "GRAPH_041",
            format!("component designator '{}' is not a valid identifier", designator),
        )
        .with_object(&node.id.0));
    }
    let component_type = if let Some(part) = node.string("part") {
        format!("\"{}\"", escape(part))
    } else if let Some(kind_param) = node.string("kind") {
        kind_param.to_string()
    } else {
        // A component-specific node kind (Resistor, IC, ...) supplies the type.
        let keyword = match kind {
            "Component" => {
                return Err(missing_params(node, "part or kind"));
            }
            other => other.to_ascii_lowercase(),
        };
        keyword
    };
    let mut statement = format!("component {}: {}", designator, component_type);
    if let Some(value) = node.parameter("value").and_then(ParamValue::as_quantity) {
        statement.push(' ');
        statement.push_str(&value.to_source());
    }
    let known = [
        "designator",
        "part",
        "kind",
        "value",
    ];
    let mut properties: Vec<String> = Vec::new();
    for (key, value) in &node.parameters {
        if known.iter().any(|known| known.eq_ignore_ascii_case(key)) {
            continue;
        }
        if !is_property_key(key) {
            continue;
        }
        properties.push(format!("    {}: {}", key, value.display()));
    }
    if !properties.is_empty() {
        statement.push_str(" {\n");
        statement.push_str(&properties.join("\n"));
        statement.push_str("\n}");
    }
    Ok(statement)
}

fn net_statement(node: &Node, kind: &str) -> Result<String, Diagnostic> {
    let name = node
        .string("name")
        .unwrap_or(node.name.as_str())
        .to_string();
    if !is_path_segment(&name) {
        return Err(Diagnostic::error(
            "GRAPH_042",
            format!("net name '{}' is not a valid identifier", name),
        )
        .with_object(&node.id.0));
    }
    let mut properties: Vec<String> = Vec::new();
    if let Some(voltage) = node.parameter("voltage").and_then(ParamValue::as_quantity) {
        properties.push(format!("    voltage: {}", voltage.to_source()));
    }
    let net_type = node
        .string("type")
        .map(str::to_string)
        .or_else(|| match kind {
            "PowerNet" => Some("power".to_string()),
            "GroundNet" => Some("ground".to_string()),
            _ => None,
        });
    if let Some(net_type) = net_type {
        properties.push(format!("    type: {}", net_type));
    }
    let mut statement = format!("net {}", name);
    if !properties.is_empty() {
        statement.push_str(" {\n");
        statement.push_str(&properties.join("\n"));
        statement.push_str("\n}");
    }
    Ok(statement)
}

fn component_name(node: &Node) -> Option<String> {
    node.string("component")
        .or_else(|| node.string("designator"))
        .map(str::to_string)
        .or_else(|| {
            if node.kind.canonical() == "Component" {
                Some(node.name.clone())
            } else {
                None
            }
        })
}

fn place_statement(node: &Node) -> Result<String, Diagnostic> {
    let Some(component) = component_name(node) else {
        return Err(missing_params(node, "component"));
    };
    let statement = if let (Some(x), Some(y)) = (node.length("x"), node.length("y")) {
        format!(
            "place {} at ({}, {})",
            component,
            x.to_source(),
            y.to_source()
        )
    } else if let Some(anchor) = node.string("anchor") {
        format!("place {} at {}", component, anchor)
    } else if let (Some(reference), Some(distance)) =
        (node.string("near"), node.length("distance"))
    {
        format!(
            "place {} near {} distance {}",
            component,
            reference,
            distance.to_source()
        )
    } else {
        return Err(missing_params(node, "x and y, anchor, or near and distance"));
    };
    if let Some(rotation) = node.angle("rotation").or_else(|| node.angle("angle")) {
        Ok(format!(
            "{}\nrotate {} {}",
            statement,
            component,
            rotation.to_source()
        ))
    } else {
        Ok(statement)
    }
}

fn route_statement(node: &Node) -> Result<String, Diagnostic> {
    let net = node
        .string("net")
        .unwrap_or(node.name.as_str())
        .to_string();
    if !is_path_segment(&net) {
        return Err(Diagnostic::error(
            "GRAPH_043",
            format!("route references invalid net name '{}'", net),
        )
        .with_object(&node.id.0));
    }
    let mut properties: Vec<String> = Vec::new();
    if let Some(width) = node.length("width") {
        properties.push(format!("    width: {}", width.to_source()));
    }
    if let Some(layer) = node.string("layer") {
        properties.push(format!("    layer: {}", layer));
    }
    if let Some(length) = node.length("length") {
        properties.push(format!("    length: {}", length.to_source()));
    }
    if node
        .parameter("differential_pair")
        .and_then(ParamValue::as_bool)
        .unwrap_or(false)
    {
        properties.push("    differential_pair: true".to_string());
    }
    if let Some(impedance) = node.parameter("impedance").and_then(ParamValue::as_quantity) {
        properties.push(format!("    impedance: {}", impedance.to_source()));
    }
    let mut statement = format!("route {}", net);
    if !properties.is_empty() {
        statement.push_str(" {\n");
        statement.push_str(&properties.join("\n"));
        statement.push_str("\n}");
    }
    Ok(statement)
}

fn via_statement(node: &Node) -> Result<String, Diagnostic> {
    let mut properties: Vec<String> = Vec::new();
    if let Some(drill) = node.length("drill") {
        properties.push(format!("    drill: {}", drill.to_source()));
    }
    if let Some(diameter) = node.length("diameter") {
        properties.push(format!("    diameter: {}", diameter.to_source()));
    }
    if properties.is_empty() {
        return Err(missing_params(node, "drill or diameter"));
    }
    Ok(format!(
        "via {{\n{}\n}}",
        properties.join("\n")
    ))
}

fn constraints_statement(node: &Node) -> Option<String> {
    let known = [
        "min_trace_width",
        "clearance",
        "min_via_drill",
        "min_via_diameter",
        "copper_edge_clearance",
        "min_annular_ring",
        "solder_mask_clearance",
        "drill_to_drill",
        "default_via_drill",
        "default_via_diameter",
    ];
    let mut properties: Vec<String> = Vec::new();
    for key in known {
        if let Some(value) = node.length(key) {
            properties.push(format!("    {}: {}", key, value.to_source()));
        }
    }
    if properties.is_empty() {
        return None;
    }
    Some(format!(
        "constraints {{\n{}\n}}",
        properties.join("\n")
    ))
}

fn is_path_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn is_property_key(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use papan_ir::{ComponentInstance, Net, NetKind, PcbIr, PinRef, Side};

    fn component_graph() -> NodeGraph {
        let mut graph = NodeGraph::new("Demo");
        graph.add(
            Node::new(NodeKind::new("Board").unwrap(), "Demo")
                .with_parameter("name", ParamValue::String("Demo".into())),
        );
        graph.add(
            Node::new(NodeKind::new("BoardSize").unwrap(), "Size")
                .with_parameter("width", ParamValue::Length(Length::from_mm(50.0)))
                .with_parameter("height", ParamValue::Length(Length::from_mm(30.0))),
        );
        let r1 = graph.add(
            Node::new(NodeKind::new("Component").unwrap(), "R1")
                .with_parameter("part", ParamValue::String("resistor".into()))
                .with_parameter(
                    "value",
                    ParamValue::Quantity(Quantity::parse_literal("10kΩ").unwrap()),
                ),
        );
        let net = graph.add(
            Node::new(NodeKind::new("Net").unwrap(), "VCC")
                .with_parameter("name", ParamValue::String("VCC".into()))
                .with_parameter("voltage", ParamValue::Quantity(Quantity::parse_literal("5V").unwrap())),
        );
        let pin = graph.add(
            Node::new(NodeKind::new("Pin").unwrap(), "R1.1")
                .with_parameter("component", ParamValue::Identifier("R1".into()))
                .with_parameter("pin", ParamValue::String("1".into())),
        );
        graph
            .connect(
                PortRef::new(pin, "pin"),
                PortRef::new(net, "pins"),
            )
            .unwrap();
        graph.add(
            Node::new(NodeKind::new("Place").unwrap(), "Place R1")
                .with_parameter("component", ParamValue::Identifier("R1".into()))
                .with_parameter("x", ParamValue::Length(Length::from_mm(10.0)))
                .with_parameter("y", ParamValue::Length(Length::from_mm(10.0))),
        );
        let _ = r1;
        graph
    }

    #[test]
    fn categories_and_default_ports_are_typed() {
        let kind = NodeKind::new("net").unwrap();
        assert_eq!(kind.canonical(), "Net");
        let ports = kind.default_ports();
        assert!(ports.iter().any(|port| port.port_type == PortType::Net));
        assert_eq!(
            NodeKind::new("IC").unwrap().category(),
            Some(NodeCategory::Component)
        );
    }

    #[test]
    fn invalid_typed_connection_is_rejected() {
        let mut graph = NodeGraph::new("Demo");
        let component = graph.add(Node::new(
            NodeKind::new("Component").unwrap(),
            "R1",
        ));
        let via = graph.add(Node::new(NodeKind::new("Via").unwrap(), "V1"));
        let error = graph
            .connect(
                PortRef::new(component, "component"),
                PortRef::new(via, "net"),
            )
            .unwrap_err();
        assert_eq!(error.code, "GRAPH_027");
    }

    #[test]
    fn valid_pin_to_net_connection_is_accepted() {
        let graph = component_graph();
        assert_eq!(graph.connections.len(), 1);
        assert!(graph.validate().is_empty());
    }

    #[test]
    fn projection_is_deterministic_and_compiles() {
        let graph = component_graph();
        let first = graph.to_source();
        let second = graph.to_source();
        assert_eq!(first.source, second.source);
        assert!(first.source.contains("board \"Demo\""));
        assert!(first.source.contains("size 50mm x 30mm"));
        assert!(first.source.contains("component R1: \"resistor\" 10kΩ"));
        assert!(first.source.contains("connect R1.1 -> VCC"));
        assert!(first.source.contains("place R1 at (10mm, 10mm)"));
        assert!(!first.has_errors());
    }

    #[test]
    fn graph_json_roundtrip() {
        let graph = component_graph();
        let json = graph.to_json().unwrap();
        let restored = NodeGraph::from_json(&json).unwrap();
        assert_eq!(graph, restored);
    }

    #[test]
    fn ir_projection_contains_design_objects() {
        let mut ir = PcbIr::new("Board");
        ir.add_net(Net {
            id: ObjectId::stable("net", "VCC"),
            name: "VCC".into(),
            kind: NetKind::Power,
            voltage: None,
            members: vec![PinRef::new(ObjectId::stable("component", "R1"), "1")],
            properties: BTreeMap::new(),
            source_span: None,
        });
        ir.components.push(ComponentInstance {
            id: ObjectId::stable("component", "R1"),
            designator: "R1".into(),
            definition_id: ObjectId::stable("part", "resistor"),
            value: None,
            properties: BTreeMap::new(),
            position: Some(papan_ir::Point::from_mm(5.0, 5.0)),
            rotation: Angle::ZERO,
            side: Side::Top,
            locked: false,
            source_span: None,
        });
        let graph = NodeGraph::from_ir(&ir);
        assert!(graph.node_named("R1").is_some());
        assert!(graph.node_named("VCC").is_some());
        assert_eq!(graph.nodes_of_kind("Pin").len(), 1);
        let projection = graph.to_source();
        assert!(projection.source.contains("connect R1.1 -> VCC"));
    }

    #[test]
    fn auto_layout_is_stable() {
        let mut graph = component_graph();
        graph.auto_layout();
        let positions: Vec<_> = graph.nodes.iter().map(|node| node.position).collect();
        graph.auto_layout();
        let again: Vec<_> = graph.nodes.iter().map(|node| node.position).collect();
        assert_eq!(positions, again);
    }

    #[test]
    fn graph_diff_reports_additions() {
        let graph = component_graph();
        let mut changed = graph.clone();
        changed.add(Node::new(NodeKind::new("Net").unwrap(), "GND"));
        let diff = graph.diff(&changed);
        assert_eq!(diff.added_nodes.len(), 1);
        assert!(diff.removed_nodes.is_empty());
    }
}
