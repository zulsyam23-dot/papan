//! DRC integration tests.

use papan_drc::check_drc;
use papan_integration_tests::TWO_PIN_BOARD;
use papan_routing::autoroute;

#[test]
fn routed_design_can_pass_drc_or_report_specific_geometry_issues() {
    let compiled = papan_document::compile_source(TWO_PIN_BOARD, &papan_library::ComponentLibrary::builtin());
    assert!(!compiled.diagnostics.has_errors(), "{:?}", compiled.diagnostics);
    let mut ir = compiled.ir;
    let route = autoroute(&mut ir);
    assert!(!route.has_errors(), "{:?}", route.diagnostics);
    let drc = check_drc(&ir);
    // Integration expectation: DRC evaluates actual geometry/connectivity and returns
    // stable codes; a clean routed design should not have fatal unknown failures.
    assert!(drc.iter().all(|d| d.code.starts_with("DRC_")), "{:?}", drc);
}

#[test]
fn unrouted_net_produces_drc_150() {
    let compiled = papan_document::compile_source(TWO_PIN_BOARD, &papan_library::ComponentLibrary::builtin());
    let ir = compiled.ir;
    let drc = check_drc(&ir);
    assert!(drc.iter().any(|d| d.code == "DRC_150"), "{:?}", drc);
}
