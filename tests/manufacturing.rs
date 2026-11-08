//! Manufacturing integration tests.

use papan_integration_tests::two_pin_design;
use papan_manufacturing::{export_all, generate_bom, generate_excellon, generate_gerber, generate_pick_and_place, validate};
use papan_routing::autoroute;

#[test]
fn manufacturing_generators_emit_expected_formats() {
    let mut ir = two_pin_design();
    autoroute(&mut ir);
    let layer = ir.layer("F.Cu").expect("F.Cu");
    let gerber = generate_gerber(&ir, layer);
    assert!(gerber.contains("%MOMM*%"));
    assert!(gerber.ends_with("M02*\n") || gerber.contains("M02*"));
    let drill = generate_excellon(&ir);
    assert!(drill.starts_with("M48\n"));
    assert!(drill.contains("M30"));
    let bom = generate_bom(&ir);
    assert!(bom.starts_with("Designator,Value,Part,Category,Footprint,Quantity,Manufacturer"));
    assert!(bom.contains("R1"));
    let pnp = generate_pick_and_place(&ir);
    assert!(pnp.starts_with("Designator,Mid X (mm),Mid Y (mm),Layer,Rotation,Value,Package"));
    assert!(pnp.contains("R1"));
}

#[test]
fn export_all_writes_complete_manufacturing_bundle() {
    let mut ir = two_pin_design();
    autoroute(&mut ir);
    let output = export_all(&ir);
    assert!(output.gerber.contains_key("F_Cu.gbr") || output.gerber.keys().any(|k| k.contains("Cu")));
    assert!(!output.drill.is_empty());
    assert!(!output.bom.is_empty());
    assert!(!output.pick_and_place.is_empty());
    let report = validate(&ir);
    // After routing, manufacturing validation should not fail for missing routing in this board.
    assert!(!report.diagnostics.iter().any(|d| d.code == "MFG_030"), "{:?}", report.diagnostics);
}
