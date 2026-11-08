//! External backend adapter interface.
//!
//! Adapters are interoperability boundaries, never the source of truth. Every
//! adapter imports into and exports from the PIR only. The interface is
//! deliberately synchronous and deterministic; adapters may use external tools
//! internally but the PAPAN core must build and compile without them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use papan_ir::PcbIr;
use papan_types::{Diagnostics, ValidationReport};

/// Stable error type returned by backend adapters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendError {
    pub code: String,
    pub message: String,
    pub diagnostics: Diagnostics,
}

impl BackendError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            diagnostics: Diagnostics::new(),
        }
    }

    pub fn with_diagnostics(mut self, diagnostics: Diagnostics) -> Self {
        self.diagnostics = diagnostics;
        self
    }
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BackendError {}

pub type BackendResult<T> = Result<T, BackendError>;

/// One imported or exported file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendFile {
    pub name: String,
    pub content: String,
    pub mime_type: Option<String>,
}

impl BackendFile {
    pub fn text(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
            mime_type: Some("text/plain".to_string()),
        }
    }
}

/// Input passed to an adapter import.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackendInput {
    /// Logical file name, e.g. `external.kicad_pcb`.
    pub name: String,
    /// File contents. Adapters must not require a filesystem.
    pub content: String,
    /// Optional source path, used only for diagnostics and relative assets.
    pub path: Option<PathBuf>,
}

impl BackendInput {
    pub fn new(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
            path: None,
        }
    }

    pub fn from_path(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let content = std::fs::read_to_string(&path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("input")
            .to_string();
        Ok(Self {
            name,
            content,
            path: Some(path),
        })
    }
}

#[derive(Clone, Debug)]
pub struct BackendImport {
    pub ir: PcbIr,
    pub diagnostics: Diagnostics,
}

impl BackendImport {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

#[derive(Clone, Debug, Default)]
pub struct BackendOutput {
    pub files: Vec<BackendFile>,
    pub diagnostics: Diagnostics,
}

impl BackendOutput {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    pub fn file_map(&self) -> BTreeMap<String, String> {
        self.files
            .iter()
            .map(|file| (file.name.clone(), file.content.clone()))
            .collect()
    }

    pub fn write_to_dir(&self, dir: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        for file in &self.files {
            let path = dir.join(&file.name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, &file.content)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub can_import: bool,
    pub can_export: bool,
    pub can_validate: bool,
    pub preserves_hierarchy: bool,
}

impl Default for BackendCapabilities {
    fn default() -> Self {
        Self {
            can_import: true,
            can_export: true,
            can_validate: true,
            preserves_hierarchy: false,
        }
    }
}

/// Interface implemented by every external format adapter.
///
/// The PRD sketches `import(BackendInput)`, `export(&PcbIr)` and
/// `validate(&PcbIr)`. The concrete trait keeps those shapes and adds an
/// identifier, capabilities and output format selection so multiple adapters can
/// coexist in one registry.
pub trait PcbBackend {
    /// Stable adapter identifier, e.g. `kicad`.
    fn id(&self) -> &str;

    fn display_name(&self) -> &str {
        self.id()
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::default()
    }

    /// Import an external design into the PIR.
    fn import(&self, input: &BackendInput) -> BackendResult<BackendImport>;

    /// Export the PIR into external files.
    fn export(&self, ir: &PcbIr) -> BackendResult<BackendOutput>;

    /// Validate that the IR can be represented by this adapter.
    fn validate(&self, ir: &PcbIr) -> BackendResult<ValidationReport> {
        let diagnostics = papan_ir_verifier::verify(ir);
        Ok(ValidationReport::from_diagnostics(diagnostics))
    }
}

/// Registry of available adapters.
#[derive(Default)]
pub struct BackendRegistry {
    backends: BTreeMap<String, Box<dyn PcbBackend>>,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<B: PcbBackend + 'static>(&mut self, backend: B) {
        self.backends.insert(backend.id().to_ascii_lowercase(), Box::new(backend));
    }

    pub fn get(&self, id: &str) -> Option<&dyn PcbBackend> {
        self.backends
            .get(&id.to_ascii_lowercase())
            .map(|backend| backend.as_ref())
    }

    pub fn ids(&self) -> Vec<&str> {
        self.backends.keys().map(String::as_str).collect()
    }

    pub fn import(&self, id: &str, input: &BackendInput) -> BackendResult<BackendImport> {
        let backend = self
            .get(id)
            .ok_or_else(|| BackendError::new("BACKEND_001", format!("unknown backend '{id}'")))?;
        backend.import(input)
    }

    pub fn export(&self, id: &str, ir: &PcbIr) -> BackendResult<BackendOutput> {
        let backend = self
            .get(id)
            .ok_or_else(|| BackendError::new("BACKEND_001", format!("unknown backend '{id}'")))?;
        backend.export(ir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy;

    impl PcbBackend for Dummy {
        fn id(&self) -> &str {
            "dummy"
        }

        fn import(&self, _input: &BackendInput) -> BackendResult<BackendImport> {
            Ok(BackendImport {
                ir: PcbIr::new("Dummy"),
                diagnostics: Diagnostics::new(),
            })
        }

        fn export(&self, _ir: &PcbIr) -> BackendResult<BackendOutput> {
            Ok(BackendOutput {
                files: vec![BackendFile::text("dummy.txt", "dummy")],
                diagnostics: Diagnostics::new(),
            })
        }
    }

    #[test]
    fn registry_dispatches_adapters() {
        let mut registry = BackendRegistry::new();
        registry.register(Dummy);
        assert_eq!(registry.ids(), vec!["dummy"]);
        let output = registry.export("DUMMY", &PcbIr::new("Demo")).unwrap();
        assert_eq!(output.files[0].name, "dummy.txt");
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn unknown_backend_is_a_structured_error() {
        let registry = BackendRegistry::new();
        let error = registry
            .import("nope", &BackendInput::new("x", ""))
            .unwrap_err();
        assert_eq!(error.code, "BACKEND_001");
    }
}
