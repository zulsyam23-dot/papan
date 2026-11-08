//! Parser and lexer integration tests.

use papan_integration_tests::{two_pin_design, TWO_PIN_BOARD};
use papan_ir::{Length, NetKind, PinType};
use papan_parser::parse;

#[test]
fn parses_two_pin_board_without_errors() {
    let parsed = parse(TWO_PIN_BOARD);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    assert_eq!(parsed.program.board.as_ref().map(|b| b.name.as_str()), Some("Integration Demo"));
    assert!(!parsed.program.items.is_empty());
}

#[test]
fn parser_reports_structured_errors_with_recovery() {
    let parsed = parse(
        r#"
board "Broken"
component R1: resistor
connect R1.1 VCC
"#,
    );
    assert!(parsed.diagnostics.has_errors());
    assert!(
        parsed.diagnostics.iter().any(|d| d.code.starts_with("PARSE_")),
        "{:?}",
        parsed.diagnostics
    );
    assert!(parsed.program.board.is_some(), "parser should recover board declaration");
}

#[test]
fn parser_accepts_route_and_constraint_directives() {
    let source = r#"
board "Routing"
size 50mm x 30mm
component R1: resistor { value: 1kΩ }
component R2: resistor { value: 1kΩ }
net N1
connect R1.1 -> N1
connect R2.1 -> N1
place R1 at (10mm, 10mm)
place R2 at (30mm, 10mm)
route N1 { width: 0.25mm layer: F.Cu }
constraints { min_trace_width: 0.15mm clearance: 0.20mm }
"#;
    let parsed = parse(source);
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
}

#[test]
fn parsed_program_lowers_to_valid_ir() {
    let ir = two_pin_design();
    assert_eq!(ir.board.name, "Integration Demo");
    assert_eq!(ir.board.width, Length::from_mm(40.0));
    assert_eq!(ir.components.len(), 2);
    assert_eq!(ir.nets.len(), 1);
    assert_eq!(ir.nets[0].kind, NetKind::Other);
    assert!(ir.definition_for(&ir.components[0]).is_some());
    assert_eq!(ir.pin_definition(&papan_ir::PinRef::new(ir.components[0].id.clone(), "1")).unwrap().electrical, PinType::Passive);
}
