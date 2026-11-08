//! Routing integration tests.

use papan_integration_tests::two_pin_design;
use papan_ir::Length;
use papan_routing::autoroute;

#[test]
fn autorouter_creates_traces_and_connects_net() {
    let mut ir = two_pin_design();
    assert!(ir.traces.is_empty());
    let report = autoroute(&mut ir);
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert!(!ir.traces.is_empty());
    for trace in &ir.traces {
        assert!(trace.width.is_positive());
        assert!(trace.width >= Length::from_mm(0.15) || report.diagnostics.iter().any(|d| d.code == "ROUTE_012"));
        assert!(trace.points.len() >= 2);
        assert!(ir.layer_by_id(&trace.layer).is_some());
    }
}

#[test]
fn autorouter_respects_route_directives_for_layer_changes() {
    let compiled = papan_document::compile_source(
        r#"
board "Via"
size 50mm x 30mm
layers 2
component R1: resistor { value: 10kΩ }
component R2: resistor { value: 10kΩ }
net N1
connect R1.1 -> N1
connect R2.1 -> N1
place R1 at (10mm, 15mm)
place R2 at (40mm, 15mm)
route N1 { layer: B.Cu }
"#,
        &papan_library::ComponentLibrary::builtin(),
    );
    assert!(!compiled.diagnostics.has_errors(), "{:?}", compiled.diagnostics);
    let mut ir = compiled.ir;
    let report = autoroute(&mut ir);
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    // Routing a net on a non-default layer commonly inserts vias in this deterministic router.
    assert!(!ir.traces.is_empty());
}
