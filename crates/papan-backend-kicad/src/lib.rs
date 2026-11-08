//! KiCad interoperability adapter.
//!
//! This adapter reads and writes a conservative KiCad .kicad_pcb subset using
//! an internal S-expression parser. KiCad is *not* required at build or run
//! time, and the adapter never becomes the design source of truth: imports are
//! converted into PIR objects and exports are generated from PIR objects.
//!
//! The exporter keeps board outline, footprints/pads, nets, connections, track
//! segments/arcs and vias. Round-tripping preserves geometry and connectivity,
//! while hierarchy and KiCad-specific metadata are intentionally dropped.

mod export;
mod import;
mod sexpr;
#[cfg(test)]
mod tests;

pub use export::export_kicad;
pub use import::import_kicad;

use papan_backend::{
    BackendCapabilities, BackendFile, BackendImport, BackendInput, BackendOutput, BackendResult,
    PcbBackend,
};
use papan_ir::PcbIr;
use papan_types::{Diagnostic, ValidationReport};

pub const ADAPTER_ID: &str = "kicad";

#[derive(Clone, Debug, Default)]
pub struct KiCadBackend;

impl KiCadBackend {
    pub fn new() -> Self {
        Self
    }
}

impl PcbBackend for KiCadBackend {
    fn id(&self) -> &str {
        ADAPTER_ID
    }

    fn display_name(&self) -> &str {
        "KiCad PCB"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            can_import: true,
            can_export: true,
            can_validate: true,
            preserves_hierarchy: false,
        }
    }

    fn import(&self, input: &BackendInput) -> BackendResult<BackendImport> {
        import_kicad(input)
    }

    fn export(&self, ir: &PcbIr) -> BackendResult<BackendOutput> {
        let text = export_kicad(ir);
        let mut diagnostics = papan_ir_verifier::verify(ir);
        if ir.board.outline.points.is_empty() {
            diagnostics.push(Diagnostic::warning(
                "KICAD_020",
                "board outline is empty; KiCad export will have no Edge.Cuts geometry",
            ));
        }
        Ok(BackendOutput {
            files: vec![BackendFile {
                name: "board.kicad_pcb".to_string(),
                content: text,
                mime_type: Some("text/x-kicad-pcb".to_string()),
            }],
            diagnostics,
        })
    }

    fn validate(&self, ir: &PcbIr) -> BackendResult<ValidationReport> {
        let mut diagnostics = papan_ir_verifier::verify(ir);
        if ir.board.outline.points.is_empty() {
            diagnostics.push(Diagnostic::error(
                "KICAD_021",
                "a board outline is required for KiCad interoperability",
            ));
        }
        Ok(ValidationReport::from_diagnostics(diagnostics))
    }
}