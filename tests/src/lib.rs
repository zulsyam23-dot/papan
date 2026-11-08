//! Shared helpers for PAPAN integration tests.
//!
//! The actual tests live next to this crate's manifest so the repository layout
//! mirrors the PRD `tests/` structure.

use papan_document::compile_source;
use papan_library::ComponentLibrary;

/// A small but complete two-resistor board used by several test suites.
pub const TWO_PIN_BOARD: &str = r#"
board "Integration Demo"

size 40mm x 30mm
thickness 1.6mm
layers 2

component R1: resistor {
    value: 10kΩ
}
component R2: resistor {
    value: 10kΩ
}
net N1
connect R1.1 -> N1
connect R2.1 -> N1
place R1 at (10mm, 10mm)
place R2 at (10mm, 20mm)
"#;

/// Compile [`TWO_PIN_BOARD`] with the built-in library.
pub fn two_pin_design() -> papan_ir::PcbIr {
    let compiled = compile_source(TWO_PIN_BOARD, &ComponentLibrary::builtin());
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.ir
}

/// Return a process-unique temporary directory without extra dependencies.
pub fn temp_dir(name: &str) -> std::path::PathBuf {
    let unique = format!(
        "papan-test-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    );
    std::env::temp_dir().join(unique)
}
