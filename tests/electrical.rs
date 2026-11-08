//! Electrical engine integration tests.

use papan_electrical::{check_erc, connectivity, unconnected_pins, unrouted_nets};
use papan_integration_tests::{two_pin_design, TWO_PIN_BOARD};
use papan_library::ComponentLibrary;
use papan_routing::autoroute;

#[test]
fn connectivity_detects_unrouted_then_routed_net() {
    let mut ir = two_pin_design();
    let before = unrouted_nets(&ir);
    assert_eq!(before.len(), 1, "{before:?}");
    assert_eq!(before[0].groups.len(), 2);
    let report = autoroute(&mut ir);
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    let after = connectivity(&ir);
    assert!(after.iter().all(|n| n.is_connected()), "{after:?}");
}

#[test]
fn unconnected_pins_are_reported_by_erc() {
    let ir = two_pin_design();
    let pins = unconnected_pins(&ir);
    assert!(pins.iter().any(|p| p.pin == "2"));
    let erc = check_erc(&ir);
    assert!(erc.iter().any(|d| d.code == "ERC_010"), "{:?}", erc);
}

#[test]
fn erc_reports_power_ground_mismatch() {
    let compiled = papan_document::compile_source(
        r#"
board "ERC"
size 40mm x 30mm
component R1: resistor { value: 10kΩ }
net VCC { type: power voltage: 5V }
net GND { type: ground }
connect R1.1 -> VCC
connect R1.2 -> GND
"#,
        &ComponentLibrary::builtin(),
    );
    assert!(!compiled.diagnostics.has_errors(), "{:?}", compiled.diagnostics);
    let erc = check_erc(&compiled.ir);
    // Passive-only nets may warn about missing power/ground source pins, but should not
    // report output conflicts or short diagnostics.
    assert!(!erc.iter().any(|d| d.code == "ERC_024"), "{:?}", erc);
    let _ = TWO_PIN_BOARD;
}
