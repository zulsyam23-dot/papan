//! ERC integration tests.

use papan_electrical::validate;
use papan_integration_tests::TWO_PIN_BOARD;

#[test]
fn erc_reports_unconnected_and_unrouted_conditions() {
    let compiled = papan_document::compile_source(TWO_PIN_BOARD, &papan_library::ComponentLibrary::builtin());
    let report = validate(&compiled.ir);
    let codes: Vec<_> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.iter().any(|c| c.starts_with("ERC_")), "{codes:?}");
    // Two-pin board leaves R1.2/R2.2 unconnected and N1 unrouted before routing.
    assert!(codes.contains(&"ERC_010") || codes.contains(&"ERC_030"), "{codes:?}");
}
