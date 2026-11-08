//! Node graph integration tests.

use papan_document::compile_source;
use papan_graph::{Node, NodeGraph, NodeKind, ParamValue, PortRef};
use papan_integration_tests::{two_pin_design, TWO_PIN_BOARD};
use papan_library::ComponentLibrary;
use papan_types::UnitKind;

#[test]
fn ir_projects_to_typed_graph_and_canonical_source() {
    let ir = two_pin_design();
    let graph = NodeGraph::from_ir(&ir);
    assert!(!graph.nodes.is_empty());
    assert!(graph.nodes.iter().any(|n| n.kind.canonical() == "Board"));
    assert!(graph.nodes.iter().any(|n| n.kind.canonical() == "Component"));
    let projected = graph.to_source();
    assert!(!projected.has_errors(), "{:?}", projected.diagnostics);
    assert!(projected.source.contains("board \"Integration Demo\""));
}

#[test]
fn pin_to_net_edge_projects_to_connect_statement() {
    let mut graph = NodeGraph::new("Nodes");
    let board = graph.add(Node::new(NodeKind::new("Board").unwrap(), "Nodes"));
    let _ = board;
    let _r1 = graph.add(
        Node::new(NodeKind::new("Component").unwrap(), "R1")
            .with_parameter("designator", ParamValue::String("R1".into()))
            .with_parameter("part", ParamValue::String("resistor".into()))
            .with_parameter(
                "value",
                ParamValue::Quantity(papan_types::Quantity::new(UnitKind::Resistance, 10_000_000)),
            ),
    );
    let net = graph.add(Node::new(NodeKind::new("Net").unwrap(), "N1").with_parameter("name", ParamValue::String("N1".into())));
    let pin = graph.add(
        Node::new(NodeKind::new("Pin").unwrap(), "R1.1")
            .with_parameter("component", ParamValue::Identifier("R1".into()))
            .with_parameter("pin", ParamValue::Identifier("1".into())),
    );
    graph
        .connect(PortRef::new(pin, "pin"), PortRef::new(net, "pins"))
        .expect("typed pin->net connection");
    let source = graph.to_source();
    assert!(!source.has_errors(), "{:?}", source.diagnostics);
    assert!(source.source.contains("connect R1.1 -> N1"));
    // Compiling the graph-derived source should succeed with the builtin library.
    let compiled = compile_source(&source.source, &ComponentLibrary::builtin());
    assert!(!compiled.diagnostics.has_errors(), "{:?}", compiled.diagnostics);
}

#[test]
fn graph_validation_rejects_invalid_typed_connection() {
    let mut graph = NodeGraph::new("Bad");
    let board = graph.add(Node::new(NodeKind::new("Board").unwrap(), "Bad"));
    let net = graph.add(Node::new(NodeKind::new("Net").unwrap(), "N1"));
    let result = graph.connect(PortRef::new(board, "board"), PortRef::new(net, "net"));
    assert!(result.is_err());
    if let Err(d) = result {
        assert!(d.code.starts_with("GRAPH_"));
    }
}

#[test]
fn source_and_graph_compile_to_consistent_semantics() {
    let compiled = compile_source(TWO_PIN_BOARD, &ComponentLibrary::builtin());
    assert!(!compiled.diagnostics.has_errors(), "{:?}", compiled.diagnostics);
    let graph_ir = compile_source(&compiled.graph.to_source().source, &ComponentLibrary::builtin());
    assert!(!graph_ir.diagnostics.has_errors(), "{:?}", graph_ir.diagnostics);
    assert_eq!(compiled.ir.components.len(), graph_ir.ir.components.len());
    assert_eq!(compiled.ir.nets.len(), graph_ir.ir.nets.len());
}
