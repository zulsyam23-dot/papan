//! Semantic analysis integration tests.

use papan_library::ComponentLibrary;
use papan_semantic::{compile, Command};
use papan_types::UnitKind;

#[test]
fn semantic_compilation_builds_components_nets_and_commands() {
    let parsed = papan_parser::parse(
        r#"
board "Semantic"
size 80mm x 50mm
component R1: resistor { value: 10kΩ }
component C1: capacitor 100nF
net VCC { type: power voltage: 5V }
net GND { type: ground }
connect R1.1 -> VCC
connect R1.2 -> GND
connect C1.1 -> VCC
connect C1.2 -> GND
place R1 at (20mm, 20mm)
place C1 at (30mm, 20mm)
check DRC
export gerber
"#,
    );
    assert!(!parsed.diagnostics.has_errors(), "{:?}", parsed.diagnostics);
    let output = compile(&parsed.program, &ComponentLibrary::builtin());
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert!(output.ir.find_component("R1").is_some());
    assert!(output.ir.find_net("VCC").is_some());
    assert!(output.commands.iter().any(|c| matches!(c, Command::Check(rule) if rule == "DRC")));
    assert!(output.commands.iter().any(|c| matches!(c, Command::Export(target) if target == "gerber")));
}

#[test]
fn semantic_reports_invalid_units_and_unknown_references() {
    let parsed = papan_parser::parse(
        r#"
board "Bad"
component R1: resistor 100nF
net VCC
connect NOPE.1 -> VCC
connect R1.1 -> MISSING
"#,
    );
    let output = compile(&parsed.program, &ComponentLibrary::builtin());
    let codes: Vec<_> = output.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"SEM_013"), "{codes:?}");
    assert!(codes.contains(&"SEM_032"), "{codes:?}");
    assert!(codes.contains(&"SEM_035"), "{codes:?}");
}

#[test]
fn semantic_accepts_quoted_parts_and_modules() {
    let parsed = papan_parser::parse(
        r#"
board "MCU"
module Power {
  component C1: capacitor 10uF
  net VCC { type: power voltage: 3.3V }
  connect C1.1 -> VCC
}
component U1: "ATmega32U4"
connect U1.VCC -> Power_VCC
"#,
    );
    let output = compile(&parsed.program, &ComponentLibrary::builtin());
    assert!(!output.diagnostics.has_errors(), "{:?}", output.diagnostics);
    assert!(output.ir.find_component("Power_C1").is_some());
    assert!(output.ir.find_component("U1").is_some());
    assert_eq!(
        output.ir.find_net("Power_VCC").and_then(|n| n.voltage).map(|q| q.kind),
        Some(UnitKind::Voltage)
    );
}
