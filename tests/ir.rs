//! IR and IR verifier integration tests.

use papan_integration_tests::two_pin_design;
use papan_ir::{Length, ObjectId, PIR_SCHEMA_VERSION};
use papan_ir_verifier::verify;

#[test]
fn compiled_ir_is_structurally_valid() {
    let ir = two_pin_design();
    assert_eq!(ir.schema_version, PIR_SCHEMA_VERSION);
    let diagnostics = verify(&ir);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics);
}

#[test]
fn verifier_reports_missing_component_definition() {
    let mut ir = two_pin_design();
    ir.components[0].definition_id = ObjectId::stable("part", "missing");
    let diagnostics = verify(&ir);
    assert!(
        diagnostics.iter().any(|d| d.code.starts_with("IR_")),
        "{:?}",
        diagnostics
    );
}

#[test]
fn ir_serializes_deterministically_enough_for_roundtrip() {
    let ir = two_pin_design();
    let json = ir.to_json().expect("serialize IR");
    let roundtrip = papan_ir::PcbIr::from_json(&json).expect("deserialize IR");
    assert_eq!(roundtrip.board.name, ir.board.name);
    assert_eq!(roundtrip.components.len(), ir.components.len());
    assert_eq!(roundtrip.nets.len(), ir.nets.len());
    assert_eq!(roundtrip.board.width, Length::from_mm(40.0));
}
