//! PAPAN public structured API.
//!
//! The API is a stable, documented facade over the document model and every
//! engine. It is the recommended integration point for external applications,
//! command line tools and AI agents. All edits are transactional: a mutation is
//! compiled, verified and rejected as a whole if it contains errors.
//!
//! ```
//! use papan_api::{Length, PapanProject};
//!
//! let mut project = PapanProject::new();
//! let mut board = project.create_board("Demo");
//! let r1 = board.add_component("resistor").unwrap();
//! let r2 = board.add_component("resistor").unwrap();
//! let net = board.add_net("N1").unwrap();
//! board.connect(r1.pin("1"), &net).unwrap();
//! board.connect(r2.pin("1"), &net).unwrap();
//! board.place(&r1, Length::from_mm(10.0), Length::from_mm(10.0), 0.0).unwrap();
//! board.place(&r2, Length::from_mm(25.0), Length::from_mm(10.0), 0.0).unwrap();
//! assert!(project.document().ir.components.len() == 2);
//! ```
//!
//! The API version is exposed as [`API_VERSION`]. Additive changes do not bump
//! the version; breaking changes do.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use papan_backend::{BackendInput, BackendRegistry};
use papan_backend_kicad::KiCadBackend;
use papan_document::{
    compile_source, CommitResult, DocumentError, PapanDocument, ProjectFiles, ProjectManifest,
};
use papan_drc::check_drc;
use papan_electrical::check_erc;
use papan_graph::{GraphDiff, NodeGraph};
use papan_ir::{
    ComponentInstance, Layer, Net, PadKind, PadLayers, PadShape, PinRef,
};
use papan_library::ComponentLibrary;
use papan_manufacturing::{export_all, ManufacturingOutput};
use papan_routing::{autoroute, clear_routing, RouteReport};
use papan_types::{Diagnostic, Diagnostics, ValidationReport};
use serde::{Deserialize, Serialize};

pub use papan_graph::{Node, NodeKind, ParamValue, PortType};
pub use papan_ir::{
    Angle, ComponentCategory, ComponentDefinition, Constraints, Length, NetKind, ObjectId, PcbIr,
    PinType, Point, PropValue, Quantity, Side,
};
pub use papan_types::{Severity, Span};

/// Version of the public API contract.
pub const API_VERSION: u32 = 1;

/// Stable error code used by the API layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Diagnostics::is_empty")]
    pub diagnostics: Diagnostics,
}

impl ApiError {
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

    pub fn with_diagnostic(mut self, diagnostic: Diagnostic) -> Self {
        self.diagnostics.push(diagnostic);
        self
    }

    fn from_commit(result: CommitResult) -> Self {
        Self::new(
            "API_TRANSACTION",
            format!("transaction '{}' was rejected", result.description),
        )
        .with_diagnostics(result.diagnostics)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<DocumentError> for ApiError {
    fn from(error: DocumentError) -> Self {
        Self::new(error.code, error.message).with_diagnostics(error.diagnostics)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::new("API_IO", error.to_string())
    }
}

impl From<papan_backend::BackendError> for ApiError {
    fn from(error: papan_backend::BackendError) -> Self {
        Self::new(error.code, error.message).with_diagnostics(error.diagnostics)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

// ---------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------

/// A stable reference to a component instance.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ComponentRef {
    pub id: ObjectId,
    pub designator: String,
    pub part: String,
}

impl ComponentRef {
    pub fn pin(&self, pin: impl Into<String>) -> PinHandle {
        PinHandle {
            component: self.id.clone(),
            designator: self.designator.clone(),
            pin: pin.into(),
        }
    }
}

/// A stable reference to a pin on a component instance.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PinHandle {
    pub component: ObjectId,
    pub designator: String,
    pub pin: String,
}

impl PinHandle {
    pub fn to_pin_ref(&self) -> PinRef {
        PinRef::new(self.component.clone(), self.pin.clone())
    }

    pub fn display(&self) -> String {
        format!("{}.{}", self.designator, self.pin)
    }
}

/// A stable reference to a net.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NetRef {
    pub id: ObjectId,
    pub name: String,
}

/// A stable reference to a layer.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LayerRef {
    pub id: ObjectId,
    pub name: String,
}

impl From<&Layer> for LayerRef {
    fn from(layer: &Layer) -> Self {
        Self {
            id: layer.id.clone(),
            name: layer.name.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Canonical PAPAN source (`.pn`).
    Source,
    /// Serialized node graph (`.graph`).
    NodeGraph,
    /// Serialized document, including the IR (JSON).
    DocumentJson,
    /// Serialized PIR only (JSON).
    IrJson,
    /// All copper Gerber files.
    Gerber,
    /// Excellon drill file.
    Drill,
    /// Bill of materials CSV.
    Bom,
    /// Pick-and-place CSV.
    PickAndPlace,
    /// Complete manufacturing package (Gerber + drill + BOM + pick-and-place).
    Manufacturing,
    /// KiCad board file.
    KiCad,
}

impl ExportFormat {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "source" | "pn" => Self::Source,
            "graph" | "nodes" => Self::NodeGraph,
            "document" | "json" | "document-json" => Self::DocumentJson,
            "ir" | "pir" => Self::IrJson,
            "gerber" | "gbr" => Self::Gerber,
            "drill" | "excellon" => Self::Drill,
            "bom" => Self::Bom,
            "pick-and-place" | "pick_and_place" | "pnp" | "pos" => Self::PickAndPlace,
            "manufacturing" | "mfg" | "fabrication" | "fab" => Self::Manufacturing,
            "kicad" => Self::KiCad,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::NodeGraph => "graph",
            Self::DocumentJson => "document-json",
            Self::IrJson => "ir-json",
            Self::Gerber => "gerber",
            Self::Drill => "drill",
            Self::Bom => "bom",
            Self::PickAndPlace => "pick-and-place",
            Self::Manufacturing => "manufacturing",
            Self::KiCad => "kicad",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFile {
    pub name: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportBundle {
    pub format: ExportFormat,
    pub files: Vec<ExportFile>,
    #[serde(default)]
    pub diagnostics: Diagnostics,
}

impl ExportBundle {
    pub fn new(format: ExportFormat) -> Self {
        Self {
            format,
            files: Vec::new(),
            diagnostics: Diagnostics::new(),
        }
    }

    pub fn push(&mut self, name: impl Into<String>, content: impl Into<String>) {
        self.files.push(ExportFile {
            name: name.into(),
            content: content.into(),
        });
    }

    pub fn file_map(&self) -> BTreeMap<String, String> {
        self.files
            .iter()
            .map(|file| (file.name.clone(), file.content.clone()))
            .collect()
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    pub fn write_to_dir(&self, dir: &Path) -> std::io::Result<()> {
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

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

/// A PAPAN project: one board, its canonical source, PIR and node graph, plus
/// the component library used to resolve parts.
#[derive(Clone, Debug)]
pub struct PapanProject {
    name: String,
    document: PapanDocument,
    library: ComponentLibrary,
    source_path: Option<PathBuf>,
    project_root: Option<PathBuf>,
}

impl Default for PapanProject {
    fn default() -> Self {
        Self::new()
    }
}

impl PapanProject {
    /// Create an empty project with a default 100 mm x 80 mm board.
    pub fn new() -> Self {
        Self::with_title("Untitled")
    }

    /// Create a project with a named board.
    pub fn with_title(title: impl Into<String>) -> Self {
        let name = title.into();
        let library = ComponentLibrary::builtin();
        let document = PapanDocument::new(&name);
        Self {
            name,
            document,
            library,
            source_path: None,
            project_root: None,
        }
    }

    /// Build a project from existing source text. Invalid source is preserved
    /// with diagnostics so callers can inspect it via [`Self::diagnostics`].
    pub fn from_source(source: impl Into<String>) -> Self {
        Self::from_source_with_library(source, ComponentLibrary::builtin())
    }

    pub fn from_source_with_library(source: impl Into<String>, library: ComponentLibrary) -> Self {
        let source = source.into();
        let document = PapanDocument::from_source_with_library(source, &library);
        let name = document.title.clone();
        Self {
            name,
            document,
            library,
            source_path: None,
            project_root: None,
        }
    }

    /// Parse and verify source, returning an error when compilation fails.
    pub fn compile(source: impl Into<String>) -> ApiResult<Self> {
        let project = Self::from_source(source);
        if project.has_errors() {
            return Err(ApiError::new("API_COMPILE", "source contains errors")
                .with_diagnostics(project.diagnostics().clone()));
        }
        Ok(project)
    }

    /// Open a `.pn` file, a `.graph` file, a `.papan` manifest or a project
    /// directory.
    pub fn open(path: impl AsRef<Path>) -> ApiResult<Self> {
        let path = path.as_ref();
        let files = ProjectFiles::load(path)?;
        let document = if files.source.trim().is_empty() {
            match files.graph {
                Some(graph) => {
                    PapanDocument::from_graph(graph, &ComponentLibrary::builtin())?
                }
                None => {
                    return Err(ApiError::new(
                        "API_IO",
                        format!("project '{}' has no source files", path.display()),
                    ))
                }
            }
        } else {
            PapanDocument::from_source(&files.source)
        };
        let is_project = path.is_dir() || path.extension().and_then(|e| e.to_str()) == Some("papan");
        let (project_root, source_path) = if is_project {
            let root = if path.is_dir() {
                path.to_path_buf()
            } else {
                path.parent().unwrap_or(Path::new(".")).to_path_buf()
            };
            (
                Some(root),
                files.source_files.first().cloned(),
            )
        } else {
            (None, Some(path.to_path_buf()))
        };
        let name = document.title.clone();
        Ok(Self {
            name,
            document,
            library: ComponentLibrary::builtin(),
            source_path,
            project_root,
        })
    }

    /// Create a new project directory using the standard PAPAN layout.
    pub fn create(root: impl AsRef<Path>, name: &str) -> ApiResult<Self> {
        let root = root.as_ref();
        let manifest = papan_document::create_project(root, name)?;
        let project_root = if root.extension().and_then(|e| e.to_str()) == Some("papan") {
            root.to_path_buf()
        } else {
            root.join(format!("{name}.papan"))
        };
        let source_path = project_root.join(
            manifest
                .sources
                .first()
                .cloned()
                .unwrap_or_else(|| "src/main.pn".to_string()),
        );
        let mut project = Self::with_title(name);
        project.project_root = Some(project_root);
        project.source_path = Some(source_path);
        Ok(project)
    }

    pub fn api_version(&self) -> u32 {
        API_VERSION
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn document(&self) -> &PapanDocument {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut PapanDocument {
        &mut self.document
    }

    pub fn library(&self) -> &ComponentLibrary {
        &self.library
    }

    pub fn library_mut(&mut self) -> &mut ComponentLibrary {
        &mut self.library
    }

    pub fn source(&self) -> &str {
        &self.document.source
    }

    pub fn ir(&self) -> &PcbIr {
        &self.document.ir
    }

    pub fn graph(&self) -> &NodeGraph {
        &self.document.graph
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.document.diagnostics
    }

    pub fn version(&self) -> u64 {
        self.document.version
    }

    pub fn has_errors(&self) -> bool {
        self.document.has_errors()
    }

    pub fn commands(&self) -> &[String] {
        &self.document.commands
    }

    /// Get mutable access to the board facade.
    pub fn board(&mut self) -> Board<'_> {
        Board { project: self }
    }

    /// Replace the current document with a fresh named board.
    ///
    /// This is the imperative counterpart of the conceptual
    /// `project.create_board("Keyboard")` API. It returns a board handle that
    /// borrows the project.
    pub fn create_board(&mut self, title: impl Into<String>) -> Board<'_> {
        let title = title.into();
        self.name = title.clone();
        self.document = PapanDocument::new(&title);
        self.source_path = None;
        self.project_root = None;
        Board { project: self }
    }

    // -----------------------------------------------------------------------
    // Transactions and projections
    // -----------------------------------------------------------------------

    /// Replace the canonical source transactionally.
    pub fn commit_source(
        &mut self,
        source: impl Into<String>,
        description: impl Into<String>,
    ) -> ApiResult<CommitResult> {
        let result = self.document.commit_source(source, description);
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        self.name = self.document.title.clone();
        Ok(result)
    }

    /// Apply a node graph edit transactionally.
    pub fn commit_graph(
        &mut self,
        graph: NodeGraph,
        description: impl Into<String>,
    ) -> ApiResult<CommitResult> {
        let result = self.document.commit_graph(graph, description);
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        self.name = self.document.title.clone();
        Ok(result)
    }

    /// Mutate and commit the node graph in one transaction. The closure receives
    /// a clone of the current graph; returning an error rejects the edit and
    /// leaves the document untouched.
    pub fn edit_graph<F>(&mut self, description: impl Into<String>, edit: F) -> ApiResult<CommitResult>
    where
        F: FnOnce(&mut NodeGraph) -> Result<(), ApiError>,
    {
        let mut graph = self.document.graph.clone();
        edit(&mut graph)?;
        self.commit_graph(graph, description)
    }

    /// Mutate the PIR transactionally. The verifier must accept the result.
    pub fn commit_ir<F>(&mut self, description: impl Into<String>, mutation: F) -> ApiResult<CommitResult>
    where
        F: FnOnce(&mut PcbIr) -> Diagnostics,
    {
        let result = self.document.commit_ir(description, mutation);
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(result)
    }

    pub fn can_undo(&self) -> bool {
        self.document.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.document.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        self.document.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.document.redo()
    }

    /// Canonical source projection from the current PIR.
    pub fn project_source(&self) -> String {
        self.document.projected_source().source
    }

    /// Compare two node graphs structurally.
    pub fn graph_diff(&self, other: &NodeGraph) -> GraphDiff {
        self.document.graph.diff(other)
    }

    // -----------------------------------------------------------------------
    // Validation
    // -----------------------------------------------------------------------

    /// Run structural verification, ERC, DRC and manufacturing validation.
    pub fn validate(&self) -> ValidationReport {
        let mut report = self.document.structural_validation();
        report.merge(ValidationReport::from_diagnostics(check_erc(&self.document.ir)));
        report.merge(ValidationReport::from_diagnostics(check_drc(&self.document.ir)));
        report.merge(ValidationReport::from_diagnostics(
            papan_manufacturing::validate_manufacturing(&self.document.ir),
        ));
        let mut report = report;
        report.diagnostics.sort();
        report.production_ready = !report.diagnostics.has_errors();
        report
    }

    pub fn validate_erc(&self) -> ValidationReport {
        ValidationReport::from_diagnostics(check_erc(&self.document.ir))
    }

    pub fn validate_drc(&self) -> ValidationReport {
        ValidationReport::from_diagnostics(check_drc(&self.document.ir))
    }

    pub fn validate_manufacturing(&self) -> ValidationReport {
        ValidationReport::from_diagnostics(papan_manufacturing::validate_manufacturing(
            &self.document.ir,
        ))
    }

    // -----------------------------------------------------------------------
    // Manufacturing and export
    // -----------------------------------------------------------------------

    pub fn manufacturing_output(&self) -> ManufacturingOutput {
        export_all(&self.document.ir)
    }

    /// Produce a deterministic export bundle for the requested format.
    pub fn export(&self, format: ExportFormat) -> ApiResult<ExportBundle> {
        let mut bundle = ExportBundle::new(format);
        match format {
            ExportFormat::Source => bundle.push("main.pn", self.document.source.clone()),
            ExportFormat::NodeGraph => {
                let json = self
                    .document
                    .graph
                    .to_json()
                    .map_err(|error| ApiError::new("API_EXPORT", error.to_string()))?;
                bundle.push("main.graph", json);
            }
            ExportFormat::DocumentJson => {
                let json = self
                    .document
                    .to_json()
                    .map_err(|error| ApiError::new("API_EXPORT", error.to_string()))?;
                bundle.push("document.json", json);
            }
            ExportFormat::IrJson => {
                let json = self
                    .document
                    .ir
                    .to_json()
                    .map_err(|error| ApiError::new("API_EXPORT", error.to_string()))?;
                bundle.push("design.pir.json", json);
            }
            ExportFormat::Gerber => {
                let output = export_all(&self.document.ir);
                bundle.diagnostics.extend(output.diagnostics.clone());
                for (name, content) in output.gerber {
                    bundle.push(name, content);
                }
            }
            ExportFormat::Drill => {
                let output = export_all(&self.document.ir);
                bundle.diagnostics.extend(output.diagnostics.clone());
                bundle.push("drill.drl", output.drill);
            }
            ExportFormat::Bom => {
                let output = export_all(&self.document.ir);
                bundle.diagnostics.extend(output.diagnostics.clone());
                bundle.push("bom.csv", output.bom);
            }
            ExportFormat::PickAndPlace => {
                let output = export_all(&self.document.ir);
                bundle.diagnostics.extend(output.diagnostics.clone());
                bundle.push("pick_and_place.csv", output.pick_and_place);
            }
            ExportFormat::Manufacturing => {
                let output = export_all(&self.document.ir);
                bundle.diagnostics.extend(output.diagnostics.clone());
                for (name, content) in output.files() {
                    bundle.push(name, content);
                }
            }
            ExportFormat::KiCad => {
                let registry = builtin_backends();
                let output = registry.export("kicad", &self.document.ir)?;
                bundle.diagnostics.extend(output.diagnostics);
                for file in output.files {
                    bundle.push(file.name, file.content);
                }
            }
        }
        Ok(bundle)
    }

    /// Import an external design, returning a validated node graph projection.
    pub fn import(&mut self, format: &str, input: BackendInput) -> ApiResult<&mut Self> {
        let registry = builtin_backends();
        let imported = registry.import(format, &input)?;
        if imported.has_errors() {
            return Err(ApiError::new(
                "API_IMPORT",
                format!("import of '{}' contains errors", input.name),
            )
            .with_diagnostics(imported.diagnostics));
        }
        let result = self
            .document
            .commit_ir(format!("Import {}", input.name), |ir| {
                *ir = imported.ir.clone();
                Diagnostics::new()
            });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        self.name = self.document.title.clone();
        Ok(self)
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    pub fn save_source(&self, path: impl AsRef<Path>) -> ApiResult<()> {
        self.document
            .save_source(path.as_ref())
            .map_err(ApiError::from)
    }

    pub fn save_document_json(&self, path: impl AsRef<Path>) -> ApiResult<()> {
        self.document
            .save_json(path.as_ref())
            .map_err(ApiError::from)
    }

    pub fn save_graph(&self, path: impl AsRef<Path>) -> ApiResult<()> {
        self.document.graph.save(path.as_ref()).map_err(ApiError::from)
    }

    pub fn to_json(&self) -> ApiResult<String> {
        self.document
            .to_json()
            .map_err(|error| ApiError::new("API_EXPORT", error.to_string()))
    }

    pub fn from_json(text: &str) -> ApiResult<Self> {
        let document = PapanDocument::from_json(text)
            .map_err(|error| ApiError::new("API_IMPORT", error.to_string()))?;
        let name = document.title.clone();
        Ok(Self {
            name,
            document,
            library: ComponentLibrary::builtin(),
            source_path: None,
            project_root: None,
        })
    }

    /// Save using the path this project was opened from, falling back to
    /// `fallback` when the project has no associated path.
    pub fn save(&self, fallback: impl AsRef<Path>) -> ApiResult<PathBuf> {
        let project_root = self.project_root.clone();
        let source_path = self.source_path.clone();
        if let Some(root) = project_root {
            let files = ProjectFiles {
                root: root.clone(),
                manifest: ProjectManifest::new(&self.name),
                source: self.document.source.clone(),
                graph: Some(self.document.graph.clone()),
                source_files: source_path.iter().cloned().collect(),
            };
            files.save_document(&self.document).map_err(ApiError::from)?;
            Ok(root)
        } else {
            let path = source_path.unwrap_or_else(|| fallback.as_ref().to_path_buf());
            self.document.save_source(&path).map_err(ApiError::from)?;
            Ok(path)
        }
    }
}

// ---------------------------------------------------------------------------
// Board facade
// ---------------------------------------------------------------------------

/// Mutable board operations. Every method commits through the document model.
pub struct Board<'a> {
    project: &'a mut PapanProject,
}

impl<'a> Board<'a> {
    pub fn project(&self) -> &PapanProject {
        self.project
    }

    pub fn project_mut(&mut self) -> &mut PapanProject {
        self.project
    }

    pub fn name(&self) -> &str {
        &self.project.document.ir.board.name
    }

    pub fn ir(&self) -> &PcbIr {
        &self.project.document.ir
    }

    pub fn document(&self) -> &PapanDocument {
        &self.project.document
    }

    pub fn graph(&self) -> &NodeGraph {
        &self.project.document.graph
    }

    pub fn size(&self) -> (Length, Length) {
        (
            self.project.document.ir.board.width,
            self.project.document.ir.board.height,
        )
    }

    pub fn layers(&self) -> Vec<LayerRef> {
        self.project
            .document
            .ir
            .stackup
            .layers
            .iter()
            .map(LayerRef::from)
            .collect()
    }

    pub fn copper_layers(&self) -> Vec<LayerRef> {
        self.project
            .document
            .ir
            .copper_layers()
            .into_iter()
            .map(LayerRef::from)
            .collect()
    }

    // -----------------------------------------------------------------------
    // Components
    // -----------------------------------------------------------------------

    /// Add a component resolved from the built-in library.
    ///
    /// The designator is allocated deterministically from the part category
    /// (R1, C1, U1, ...), the definition is copied into the PIR and the part is
    /// placed on the deterministic auto-placement grid.
    pub fn add_component(&mut self, part: &str) -> ApiResult<ComponentRef> {
        let definition = self
            .project
            .library
            .resolve_part(part)
            .or_else(|| self.project.library.resolve_kind(part))
            .cloned()
            .ok_or_else(|| {
                ApiError::new(
                    "API_PART",
                    format!("unknown component part or kind '{part}'"),
                )
                .with_diagnostic(
                    Diagnostic::error(
                        "API_003",
                        format!("part '{part}' is not in the component library"),
                    )
                    .with_suggestion("register a custom definition or use a built-in kind"),
                )
            })?;
        self.add_definition_instance(definition, None)
    }

    /// Add a component with an explicit designator.
    pub fn add_component_as(
        &mut self,
        designator: impl Into<String>,
        part: &str,
    ) -> ApiResult<ComponentRef> {
        let designator = designator.into();
        let definition = self
            .project
            .library
            .resolve_part(part)
            .or_else(|| self.project.library.resolve_kind(part))
            .cloned()
            .ok_or_else(|| {
                ApiError::new(
                    "API_PART",
                    format!("unknown component part or kind '{part}'"),
                )
            })?;
        self.add_definition_instance(definition, Some(designator))
    }

    /// Add a component using a caller supplied definition.
    pub fn add_custom_component(
        &mut self,
        definition: ComponentDefinition,
        designator: Option<String>,
    ) -> ApiResult<ComponentRef> {
        self.project.library.insert(definition.clone());
        self.add_definition_instance(definition, designator)
    }

    fn add_definition_instance(
        &mut self,
        definition: ComponentDefinition,
        designator: Option<String>,
    ) -> ApiResult<ComponentRef> {
        let designator = designator.unwrap_or_else(|| {
            let prefix = designator_prefix(definition.category);
            allocate_designator(&self.project.document.ir, prefix)
        });
        if self
            .project
            .document
            .ir
            .find_component(&designator)
            .is_some()
        {
            return Err(ApiError::new(
                "API_DESIGNATOR",
                format!("component designator '{designator}' already exists"),
            ));
        }
        let reference = ComponentRef {
            id: ObjectId::stable("component", &designator),
            designator: designator.clone(),
            part: definition.name.clone(),
        };
        let definition_clone = definition.clone();
        let component_id = reference.id.clone();
        let mut auto_diagnostics = Diagnostics::new();
        let result = self.project.document.commit_ir("Add component", |ir| {
            ir.add_definition(definition_clone.clone());
            ir.add_component(ComponentInstance {
                id: component_id.clone(),
                designator: designator.clone(),
                definition_id: definition_clone.id.clone(),
                value: None,
                properties: BTreeMap::new(),
                position: None,
                rotation: Angle::ZERO,
                side: Side::Top,
                locked: false,
                source_span: None,
            });
            auto_diagnostics = papan_placement::auto_place(ir);
            auto_diagnostics.clone()
        });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(reference)
    }

    /// Set the component value, for example a 10 kOhm resistor.
    pub fn set_value(&mut self, component: &ComponentRef, value: Quantity) -> ApiResult<()> {
        self.mutate_component(component, "Set component value", |instance| {
            instance.value = Some(value);
            Ok(())
        })
    }

    /// Set a string, boolean or numeric property.
    pub fn set_property(
        &mut self,
        component: &ComponentRef,
        key: impl Into<String>,
        value: PropValue,
    ) -> ApiResult<()> {
        let key = key.into();
        self.mutate_component(component, "Set component property", |instance| {
            instance.properties.insert(key.clone(), value.clone());
            Ok(())
        })
    }

    pub fn component(&self, designator: &str) -> Option<ComponentRef> {
        let ir = &self.project.document.ir;
        let component = ir.find_component(designator)?;
        let part = ir
            .definition_for(component)
            .map(|definition| definition.name.clone())
            .unwrap_or_else(|| "generic".to_string());
        Some(ComponentRef {
            id: component.id.clone(),
            designator: component.designator.clone(),
            part,
        })
    }

    pub fn components(&self) -> Vec<ComponentRef> {
        let ir = &self.project.document.ir;
        ir.components
            .iter()
            .map(|component| ComponentRef {
                id: component.id.clone(),
                designator: component.designator.clone(),
                part: ir
                    .definition_for(component)
                    .map(|definition| definition.name.clone())
                    .unwrap_or_else(|| "generic".to_string()),
            })
            .collect()
    }

    /// Validate a pin name against the component definition.
    pub fn has_pin(&self, component: &ComponentRef, pin: &str) -> bool {
        self.project
            .document
            .ir
            .find_component_by_id(&component.id)
            .and_then(|instance| {
                self.project.document.ir.definition_for(instance)
            })
            .map(|definition| definition.pin(pin).is_some())
            .unwrap_or(false)
    }

    /// Remove a component and all of its connections.
    pub fn remove_component(&mut self, component: &ComponentRef) -> ApiResult<()> {
        let id = component.id.clone();
        let result = self.project.document.commit_ir("Remove component", |ir| {
            if ir.find_component_by_id(&id).is_none() {
                return Diagnostics::from(vec![Diagnostic::error(
                    "API_004",
                    format!("component '{}' does not exist", component.designator),
                )]);
            }
            ir.components.retain(|instance| instance.id != id);
            ir.connections.retain(|connection| connection.pin.component != id);
            for net in &mut ir.nets {
                net.members.retain(|member| member.component != id);
            }
            Diagnostics::new()
        });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(())
    }

    fn mutate_component<F>(
        &mut self,
        component: &ComponentRef,
        description: &str,
        mutation: F,
    ) -> ApiResult<()>
    where
        F: FnOnce(&mut ComponentInstance) -> Result<(), Diagnostic>,
    {
        let id = component.id.clone();
        let mut failure: Option<Diagnostic> = None;
        let result = self.project.document.commit_ir(description, |ir| {
            let Some(instance) = ir
                .components
                .iter_mut()
                .find(|instance| instance.id == id)
            else {
                return Diagnostics::from(vec![Diagnostic::error(
                    "API_004",
                    format!("component '{}' does not exist", component.designator),
                )]);
            };
            match mutation(instance) {
                Ok(()) => Diagnostics::new(),
                Err(diagnostic) => {
                    failure = Some(diagnostic.clone());
                    Diagnostics::from(vec![diagnostic])
                }
            }
        });
        if let Some(diagnostic) = failure {
            let message = diagnostic.message.clone();
            return Err(ApiError::new("API_COMPONENT", message).with_diagnostic(diagnostic));
        }
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Placement
    // -----------------------------------------------------------------------

    /// Place a component at an absolute board position.
    pub fn place(
        &mut self,
        component: &ComponentRef,
        x: Length,
        y: Length,
        rotation_deg: f64,
    ) -> ApiResult<()> {
        let angle = Angle::from_deg(rotation_deg);
        self.mutate_component(component, "Place component", |instance| {
            instance.position = Some(Point::new(x, y));
            instance.rotation = angle.normalized();
            Ok(())
        })
    }

    /// Place a component using a millimetre convenience overload.
    pub fn place_mm(
        &mut self,
        component: &ComponentRef,
        x_mm: f64,
        y_mm: f64,
        rotation_deg: f64,
    ) -> ApiResult<()> {
        self.place(
            component,
            Length::from_mm(x_mm),
            Length::from_mm(y_mm),
            rotation_deg,
        )
    }

    pub fn move_component(
        &mut self,
        component: &ComponentRef,
        x: Length,
        y: Length,
    ) -> ApiResult<()> {
        self.mutate_component(component, "Move component", |instance| {
            instance.position = Some(Point::new(x, y));
            Ok(())
        })
    }

    pub fn rotate_component(&mut self, component: &ComponentRef, rotation_deg: f64) -> ApiResult<()> {
        let angle = Angle::from_deg(rotation_deg);
        self.mutate_component(component, "Rotate component", |instance| {
            instance.rotation = angle.normalized();
            Ok(())
        })
    }

    /// Move a component to the bottom side, mirroring its effective placement.
    pub fn set_side(&mut self, component: &ComponentRef, side: Side) -> ApiResult<()> {
        self.mutate_component(component, "Set component side", |instance| {
            instance.side = side;
            Ok(())
        })
    }

    pub fn lock_component(&mut self, component: &ComponentRef, locked: bool) -> ApiResult<()> {
        self.mutate_component(component, "Lock component", |instance| {
            instance.locked = locked;
            Ok(())
        })
    }

    /// Place every component without an explicit position.
    pub fn auto_place(&mut self) -> ApiResult<Diagnostics> {
        let mut diagnostics = Diagnostics::new();
        let result = self.project.document.commit_ir("Auto place", |ir| {
            diagnostics = papan_placement::auto_place(ir);
            diagnostics.clone()
        });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(diagnostics)
    }

    // -----------------------------------------------------------------------
    // Nets and connections
    // -----------------------------------------------------------------------

    pub fn add_net(&mut self, name: &str) -> ApiResult<NetRef> {
        self.add_typed_net(name, NetKind::Other)
    }

    pub fn add_typed_net(&mut self, name: &str, kind: NetKind) -> ApiResult<NetRef> {
        if name.trim().is_empty() {
            return Err(ApiError::new("API_NET", "net name cannot be empty"));
        }
        if self.project.document.ir.find_net(name).is_some() {
            return Err(ApiError::new(
                "API_NET",
                format!("net '{name}' already exists"),
            ));
        }
        let reference = NetRef {
            id: ObjectId::stable("net", name),
            name: name.to_string(),
        };
        let net_id = reference.id.clone();
        let net_name = reference.name.clone();
        let result = self.project.document.commit_ir("Add net", |ir| {
            ir.add_net(Net {
                id: net_id.clone(),
                name: net_name.clone(),
                kind,
                voltage: None,
                members: Vec::new(),
                properties: BTreeMap::new(),
                source_span: None,
            });
            Diagnostics::new()
        });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(reference)
    }

    /// Add or return an existing net by name.
    pub fn net(&mut self, name: &str) -> ApiResult<NetRef> {
        if let Some(net) = self.project.document.ir.find_net(name) {
            return Ok(NetRef {
                id: net.id.clone(),
                name: net.name.clone(),
            });
        }
        self.add_net(name)
    }

    pub fn find_net(&self, name: &str) -> Option<NetRef> {
        self.project
            .document
            .ir
            .find_net(name)
            .map(|net| NetRef {
                id: net.id.clone(),
                name: net.name.clone(),
            })
    }

    pub fn nets(&self) -> Vec<NetRef> {
        self.project
            .document
            .ir
            .nets
            .iter()
            .map(|net| NetRef {
                id: net.id.clone(),
                name: net.name.clone(),
            })
            .collect()
    }

    /// Connect a pin to a net. Both the pin and the net must exist.
    pub fn connect(&mut self, pin: PinHandle, net: &NetRef) -> ApiResult<()> {
        if let Some(component) = self
            .project
            .document
            .ir
            .find_component_by_id(&pin.component)
        {
            let valid = self
                .project
                .document
                .ir
                .definition_for(component)
                .map(|definition| definition.pin(&pin.pin).is_some())
                .unwrap_or(false);
            if !valid {
                return Err(ApiError::new(
                    "API_PIN",
                    format!("component '{}' has no pin '{}'", pin.designator, pin.pin),
                ));
            }
        } else {
            return Err(ApiError::new(
                "API_PIN",
                format!("component '{}' does not exist", pin.designator),
            ));
        }
        let net_id = net.id.clone();
        if self.project.document.ir.find_net_by_id(&net_id).is_none() {
            return Err(ApiError::new(
                "API_NET",
                format!("net '{}' does not exist", net.name),
            ));
        }
        let pin_ref = pin.to_pin_ref();
        let result = self.project.document.commit_ir("Connect pin", |ir| {
            if ir.connect(pin_ref.clone(), net_id.clone(), None).is_none() {
                return Diagnostics::from(vec![Diagnostic::error(
                    "API_005",
                    format!("cannot connect '{}'", pin.display()),
                )]);
            }
            Diagnostics::new()
        });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(())
    }

    pub fn disconnect(&mut self, pin: &PinHandle, net: &NetRef) -> ApiResult<()> {
        let pin_ref = pin.to_pin_ref();
        let net_id = net.id.clone();
        let result = self.project.document.commit_ir("Disconnect pin", |ir| {
            if let Some(net) = ir.find_net_by_id_mut(&net_id) {
                net.members.retain(|member| member != &pin_ref);
            }
            ir.connections
                .retain(|connection| !(connection.pin == pin_ref && connection.net == net_id));
            Diagnostics::new()
        });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(())
    }

    pub fn set_net_kind(&mut self, net: &NetRef, kind: NetKind) -> ApiResult<()> {
        let net_id = net.id.clone();
        let result = self.project.document.commit_ir("Set net kind", |ir| {
            let Some(net) = ir.find_net_by_id_mut(&net_id) else {
                return Diagnostics::from(vec![Diagnostic::error(
                    "API_006",
                    "net does not exist",
                )]);
            };
            net.kind = kind;
            Diagnostics::new()
        });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Routing and constraints
    // -----------------------------------------------------------------------

    /// Route every net deterministically.
    pub fn autoroute(&mut self) -> ApiResult<RouteReport> {
        let mut report: Option<RouteReport> = None;
        let result = self.project.document.commit_ir("Autoroute", |ir| {
            let route = autoroute(ir);
            let diagnostics = route.diagnostics.clone();
            report = Some(route);
            diagnostics
        });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(report.expect("autoroute report is always produced"))
    }

    pub fn clear_routing(&mut self) -> ApiResult<()> {
        let result = self
            .project
            .document
            .commit_ir("Clear routing", |ir| {
                clear_routing(ir);
                Diagnostics::new()
            });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(())
    }

    pub fn constraints(&self) -> &Constraints {
        &self.project.document.ir.constraints
    }

    pub fn set_clearance(&mut self, clearance: Length) -> ApiResult<()> {
        self.mutate_constraints("Set clearance", move |constraints| {
            constraints.clearance = clearance;
        })
    }

    pub fn set_min_trace_width(&mut self, width: Length) -> ApiResult<()> {
        self.mutate_constraints("Set minimum trace width", move |constraints| {
            constraints.min_trace_width = width;
        })
    }

    pub fn set_constraints(&mut self, constraints: Constraints) -> ApiResult<()> {
        self.mutate_constraints("Set constraints", move |current| {
            *current = constraints;
        })
    }

    fn mutate_constraints<F>(&mut self, description: &str, mutation: F) -> ApiResult<()>
    where
        F: FnOnce(&mut Constraints),
    {
        let result = self.project.document.commit_ir(description, |ir| {
            mutation(&mut ir.constraints);
            Diagnostics::new()
        });
        if !result.committed {
            return Err(ApiError::from_commit(result));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Validation and export
    // -----------------------------------------------------------------------

    pub fn validate(&self) -> ValidationReport {
        let mut report = self.project.document.structural_validation();
        report.merge(ValidationReport::from_diagnostics(check_erc(
            &self.project.document.ir,
        )));
        report.merge(ValidationReport::from_diagnostics(check_drc(
            &self.project.document.ir,
        )));
        report.merge(ValidationReport::from_diagnostics(
            papan_manufacturing::validate_manufacturing(&self.project.document.ir),
        ));
        report.diagnostics.sort();
        report.production_ready = !report.diagnostics.has_errors();
        report
    }

    pub fn export(&self, format: ExportFormat) -> ApiResult<ExportBundle> {
        self.project.export(format)
    }

    pub fn manufacturing_output(&self) -> ManufacturingOutput {
        export_all(&self.project.document.ir)
    }
}

fn designator_prefix(category: ComponentCategory) -> &'static str {
    match category {
        ComponentCategory::Resistor => "R",
        ComponentCategory::Capacitor => "C",
        ComponentCategory::Inductor => "L",
        ComponentCategory::Diode | ComponentCategory::Led => "D",
        ComponentCategory::Switch => "SW",
        ComponentCategory::Crystal => "Y",
        ComponentCategory::Connector => "J",
        ComponentCategory::Ic => "U",
        ComponentCategory::Transistor => "Q",
        ComponentCategory::Other => "X",
    }
}

fn allocate_designator(ir: &PcbIr, prefix: &str) -> String {
    let existing: BTreeSet<String> = ir
        .components
        .iter()
        .map(|component| component.designator.to_ascii_uppercase())
        .collect();
    for index in 1..=1_000_000u32 {
        let candidate = format!("{prefix}{index}");
        if !existing.contains(&candidate.to_ascii_uppercase()) {
            return candidate;
        }
    }
    format!("{prefix}{}", ir.components.len() + 1)
}

fn builtin_backends() -> BackendRegistry {
    let mut registry = BackendRegistry::new();
    registry.register(KiCadBackend::new());
    registry
}

/// Create a deterministic built-in component definition programmatically.
///
/// This is useful for AI agents and tests that need a component which is not in
/// the built-in library. Pads are placed on a deterministic grid.
pub fn custom_definition(
    name: &str,
    category: ComponentCategory,
    pins: &[&str],
) -> ComponentDefinition {
    use papan_geometry::Size;
    use papan_ir::{FootprintDefinition, PadDefinition, PinDefinition};

    let pin_definitions: Vec<PinDefinition> = pins
        .iter()
        .enumerate()
        .map(|(index, pin)| PinDefinition {
            name: (*pin).to_string(),
            number: (index + 1).to_string(),
            electrical: PinType::Bidirectional,
            description: String::new(),
        })
        .collect();
    let count = pin_definitions.len().max(1);
    let columns = (count as f64).sqrt().ceil() as usize;
    let rows = count.div_ceil(columns);
    let pads: Vec<PadDefinition> = pin_definitions
        .iter()
        .enumerate()
        .map(|(index, pin)| {
            let row = index / columns;
            let column = index % columns;
            let x = (column as i64 * 2 - (columns as i64 - 1)) as f64 * 1.27;
            let y = (row as i64 * 2 - (rows as i64 - 1)) as f64 * 1.27;
            PadDefinition {
                number: pin.number.clone(),
                pin: pin.name.clone(),
                kind: PadKind::ThroughHole,
                shape: PadShape::Round,
                position: Point::from_mm(x, y),
                size: Size::from_mm(1.6, 1.6),
                drill: Some(Length::from_mm(0.8)),
                layers: PadLayers::Both,
            }
        })
        .collect();
    ComponentDefinition {
        id: ObjectId::stable("part", name),
        name: name.to_string(),
        category,
        pins: pin_definitions,
        footprint: FootprintDefinition {
            name: format!("{name}_THT"),
            pads,
            description: "generated deterministic footprint".to_string(),
        },
        manufacturer: None,
        datasheet: None,
        description: String::new(),
    }
}

/// Compile source text without constructing a document.
pub fn compile(text: &str) -> papan_document::CompiledDesign {
    compile_source(text, &ComponentLibrary::builtin())
}

pub use papan_document::CompiledDesign;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_source() -> String {
        r#"
board "Api Demo"

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
"#
        .to_string()
    }

    #[test]
    fn public_api_builds_board_from_scratch() {
        let mut project = PapanProject::new();
        {
            let mut board = project.create_board("Demo");
            let r1 = board.add_component("resistor").unwrap();
            let r2 = board.add_component("resistor").unwrap();
            assert_eq!(r1.designator, "R1");
            assert_eq!(r2.designator, "R2");
            let net = board.add_net("N1").unwrap();
            board.connect(r1.pin("1"), &net).unwrap();
            board.connect(r2.pin("1"), &net).unwrap();
            board.place_mm(&r1, 10.0, 10.0, 0.0).unwrap();
            board.place_mm(&r2, 10.0, 20.0, 0.0).unwrap();
            board.autoroute().unwrap();
            let report = board.validate();
            assert!(!report.has_errors(), "{:?}", report.diagnostics);
            let export = board.export(ExportFormat::Manufacturing).unwrap();
            assert!(export.file_map().contains_key("drill.drl"));
            assert!(export.file_map().contains_key("bom.csv"));
        }
        assert_eq!(project.ir().components.len(), 2);
        assert_eq!(project.ir().nets.len(), 1);
        assert!(project.source().contains("component R1"));
        assert!(project.source().contains("connect R1.1 -> N1"));
    }

    #[test]
    fn source_roundtrip_and_validation() {
        let mut project = PapanProject::compile(sample_source()).unwrap();
        assert_eq!(project.ir().components.len(), 2);
        project.board().autoroute().unwrap();
        let report = project.validate();
        assert!(!report.has_errors(), "{:?}", report.diagnostics);
        let export = project.export(ExportFormat::Source).unwrap();
        assert!(export.files[0].content.contains("board \"Api Demo\""));
    }

    #[test]
    fn invalid_edit_is_rejected_without_state_change() {
        let mut project = PapanProject::compile(sample_source()).unwrap();
        let before = project.source().to_string();
        let error = project
            .commit_source("board \"Broken\"\ncomponent R1: no_such_kind\n", "break")
            .unwrap_err();
        assert_eq!(error.code, "API_TRANSACTION");
        assert!(error.diagnostics.has_errors());
        assert_eq!(project.source(), before);
        assert_eq!(project.ir().components.len(), 2);
    }

    #[test]
    fn transactions_support_undo_and_redo() {
        let mut project = PapanProject::compile(sample_source()).unwrap();
        let board = project.board();
        let r1 = board.component("R1").unwrap();
        drop(board);
        project.board().move_component(&r1, Length::from_mm(12.0), Length::from_mm(12.0)).unwrap();
        assert_eq!(
            project.ir().find_component("R1").unwrap().position,
            Some(Point::from_mm(12.0, 12.0))
        );
        assert!(project.undo());
        assert_eq!(
            project.ir().find_component("R1").unwrap().position,
            Some(Point::from_mm(10.0, 10.0))
        );
        assert!(project.redo());
    }

    #[test]
    fn graph_edit_is_transactional() {
        let mut project = PapanProject::compile(sample_source()).unwrap();
        project
            .edit_graph("add N2", |graph| {
                let kind = NodeKind::new("Net").unwrap();
                graph.add(
                    Node::new(kind, "N2")
                        .with_parameter("name", ParamValue::String("N2".into())),
                );
                Ok(())
            })
            .unwrap();
        assert!(project.ir().find_net("N2").is_some());

        // Adding a malformed component node is rejected by graph validation.
        let before = project.source().to_string();
        let error = project
            .edit_graph("bad node", |graph| {
                let kind = NodeKind::new("Component").unwrap();
                graph.add(Node::new(kind, "bad designator"));
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error.code, "API_TRANSACTION");
        assert_eq!(project.source(), before);
    }

    #[test]
    fn custom_components_are_available_to_the_api() {
        let mut project = PapanProject::new();
        let mut board = project.create_board("Custom");
        let definition = custom_definition("SENSOR-X", ComponentCategory::Ic, &["VCC", "GND", "OUT"]);
        let sensor = board
            .add_custom_component(definition, Some("U1".to_string()))
            .unwrap();
        assert!(board.has_pin(&sensor, "OUT"));
        let net = board.add_net("OUT").unwrap();
        board.connect(sensor.pin("OUT"), &net).unwrap();
        assert_eq!(project.ir().nets[0].members.len(), 1);
    }

    #[test]
    fn export_formats_are_deterministic() {
        let project = PapanProject::compile(sample_source()).unwrap();
        let first = project.export(ExportFormat::Manufacturing).unwrap();
        let second = project.export(ExportFormat::Manufacturing).unwrap();
        assert_eq!(first, second);
        assert!(first.file_map().keys().any(|name| name.ends_with(".gbr")));
        assert_eq!(
            project.export(ExportFormat::KiCad).unwrap().files[0]
                .name
                .as_str(),
            "board.kicad_pcb"
        );
    }

    #[test]
    fn errors_are_structured_and_serializable() {
        let mut project = PapanProject::new();
        let mut board = project.create_board("Errors");
        let error = board.add_component("definitely-not-a-part").unwrap_err();
        assert_eq!(error.code, "API_PART");
        assert!(!error.diagnostics.is_empty());
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("API_PART"));
    }
}
