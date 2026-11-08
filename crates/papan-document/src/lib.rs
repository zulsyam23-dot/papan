//! PAPAN document model, transactions and text/graph projections.
//!
//! A document has exactly one semantic source of truth: the PIR, which is
//! produced by compiling the canonical source text. The node graph is a typed
//! projection of the same PIR and can be edited independently; graph edits are
//! validated, projected back to canonical source and only then committed.
//!
//! All mutations go through [`PapanDocument::commit_source`],
//! [`PapanDocument::commit_graph`] or [`PapanDocument::commit_ir`]. A candidate
//! state is compiled and verified first. If it contains errors the previous
//! valid document is left untouched, which makes edits transactional and gives
//! undo/redo a reliable history.

use std::path::{Path, PathBuf};

use papan_graph::{NodeGraph, SourceProjection};
use papan_ir::PcbIr;
use papan_ir_verifier::verify;
use papan_library::ComponentLibrary;
use papan_parser::parse;
use papan_semantic::{compile, CompileOutput};
use papan_types::{Diagnostics, ValidationReport};
use serde::{Deserialize, Serialize};

pub const DOCUMENT_SCHEMA_VERSION: u32 = 1;
pub const PROJECT_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

/// Result of compiling source text into the PIR and its node projection.
#[derive(Clone, Debug)]
pub struct CompiledDesign {
    pub ir: PcbIr,
    pub graph: NodeGraph,
    pub diagnostics: Diagnostics,
    /// Structured `check`/`export` commands requested by the source.
    pub commands: Vec<String>,
}

impl CompiledDesign {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

/// Compile source text, verify the resulting IR and project the node graph.
pub fn compile_source(source: &str, library: &ComponentLibrary) -> CompiledDesign {
    let parsed = parse(source);
    let CompileOutput {
        ir,
        diagnostics: semantic_diagnostics,
        commands,
    } = compile(&parsed.program, library);

    let mut diagnostics = parsed.diagnostics;
    diagnostics.extend(semantic_diagnostics);
    diagnostics.extend(verify(&ir));
    diagnostics.sort();

    let graph = NodeGraph::from_ir(&ir);
    CompiledDesign {
        ir,
        graph,
        diagnostics,
        commands: commands
            .iter()
            .map(|command| match command {
                papan_semantic::Command::Check(name) => format!("check:{name}"),
                papan_semantic::Command::Export(name) => format!("export:{name}"),
            })
            .collect(),
    }
}

/// Project a PIR back to canonical PAPAN source through the node graph.
pub fn project_source(ir: &PcbIr) -> SourceProjection {
    NodeGraph::project_ir_to_source(ir)
}

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct DocumentState {
    source: String,
    ir: PcbIr,
    graph: NodeGraph,
    diagnostics: Diagnostics,
    commands: Vec<String>,
    version: u64,
    dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub description: String,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PapanDocument {
    pub schema_version: u32,
    pub title: String,
    /// Canonical textual representation.
    pub source: String,
    /// Semantic truth used by every engine.
    pub ir: PcbIr,
    /// Typed node view of the same design.
    pub graph: NodeGraph,
    /// Diagnostics of the last committed compilation.
    pub diagnostics: Diagnostics,
    #[serde(default)]
    pub commands: Vec<String>,
    pub version: u64,
    pub dirty: bool,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    #[serde(skip)]
    undo_stack: Vec<DocumentState>,
    #[serde(skip)]
    redo_stack: Vec<DocumentState>,
}

impl PapanDocument {
    /// Create a new empty document with a board declaration.
    pub fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        let source = default_source(&title);
        let library = ComponentLibrary::builtin();
        let compiled = compile_source(&source, &library);
        Self::from_compiled(title, source, compiled)
    }

    /// Build a document from source text. Invalid source is preserved in the
    /// document with diagnostics so `check`/`inspect` can report it.
    pub fn from_source(source: impl Into<String>) -> Self {
        let library = ComponentLibrary::builtin();
        Self::from_source_with_library(source, &library)
    }

    pub fn from_source_with_library(
        source: impl Into<String>,
        library: &ComponentLibrary,
    ) -> Self {
        let source = source.into();
        let title = extract_title(&source);
        let compiled = compile_source(&source, library);
        Self::from_compiled(title, source, compiled)
    }

    fn from_compiled(title: String, source: String, compiled: CompiledDesign) -> Self {
        Self {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            title,
            source,
            ir: compiled.ir,
            graph: compiled.graph,
            diagnostics: compiled.diagnostics,
            commands: compiled.commands,
            version: 1,
            dirty: false,
            history: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Build a document from a node graph, projecting it to source first.
    pub fn from_graph(graph: NodeGraph, library: &ComponentLibrary) -> Result<Self, DocumentError> {
        let projection = graph.to_source();
        if projection.has_errors() {
            return Err(DocumentError::new(
                "DOC_GRAPH",
                "node graph cannot be projected to source",
            )
            .with_diagnostics(projection.diagnostics.clone()));
        }
        let compiled = compile_source(&projection.source, library);
        let mut diagnostics = projection.diagnostics;
        diagnostics.extend(compiled.diagnostics.clone());
        diagnostics.sort();
        if diagnostics.has_errors() {
            return Err(DocumentError::new(
                "DOC_GRAPH",
                "node graph projection does not compile",
            )
            .with_diagnostics(diagnostics));
        }
        let mut document = Self::from_compiled(
            extract_title(&projection.source),
            projection.source,
            compiled,
        );
        document.graph = graph;
        document.diagnostics = diagnostics;
        Ok(document)
    }

    /// Load a `.pn` file, a `.papan` manifest or a project directory.
    pub fn load(path: &Path) -> Result<Self, DocumentError> {
        let project = ProjectFiles::load(path)?;
        if project.source.trim().is_empty() {
            if let Some(graph) = project.graph {
                let library = ComponentLibrary::builtin();
                return Self::from_graph(graph, &library);
            }
            return Err(DocumentError::new(
                "DOC_IO",
                format!("project '{}' has no source files", path.display()),
            ));
        }
        Ok(Self::from_source(project.source))
    }

    pub fn save_source(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &self.source)
    }

    pub fn save_json(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    pub fn structural_validation(&self) -> ValidationReport {
        let mut diagnostics = self.diagnostics.clone();
        diagnostics.extend(self.graph.validate());
        diagnostics.sort();
        ValidationReport::from_diagnostics(diagnostics)
    }

    /// Project the current IR to canonical source without mutating the document.
    pub fn projected_source(&self) -> SourceProjection {
        project_source(&self.ir)
    }

    // -----------------------------------------------------------------------
    // Transactions
    // -----------------------------------------------------------------------

    /// Replace the source text transactionally.
    pub fn commit_source(
        &mut self,
        source: impl Into<String>,
        description: impl Into<String>,
    ) -> CommitResult {
        let source = source.into();
        let library = ComponentLibrary::builtin();
        let compiled = compile_source(&source, &library);
        if compiled.diagnostics.has_errors() {
            return CommitResult::rejected(description, compiled.diagnostics);
        }
        let candidate = Self {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            title: extract_title(&source),
            source,
            ir: compiled.ir,
            graph: compiled.graph,
            diagnostics: compiled.diagnostics,
            commands: compiled.commands,
            version: self.version + 1,
            dirty: true,
            history: self.history.clone(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        };
        self.commit_candidate(candidate, description.into())
    }

    /// Apply a node graph edit transactionally. The graph is validated, then
    /// projected to source, compiled and verified before it is committed.
    pub fn commit_graph(
        &mut self,
        graph: NodeGraph,
        description: impl Into<String>,
    ) -> CommitResult {
        let description = description.into();
        let mut diagnostics = graph.validate();
        if diagnostics.has_errors() {
            return CommitResult::rejected(description, diagnostics);
        }
        let projection = graph.to_source();
        diagnostics.extend(projection.diagnostics.clone());
        diagnostics.sort();
        if diagnostics.has_errors() {
            return CommitResult::rejected(description, diagnostics);
        }
        let library = ComponentLibrary::builtin();
        let compiled = compile_source(&projection.source, &library);
        diagnostics.extend(compiled.diagnostics.clone());
        diagnostics.sort();
        if diagnostics.has_errors() {
            return CommitResult::rejected(description, diagnostics);
        }
        let candidate = Self {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            title: extract_title(&projection.source),
            source: projection.source,
            ir: compiled.ir,
            graph,
            diagnostics,
            commands: compiled.commands,
            version: self.version + 1,
            dirty: true,
            history: self.history.clone(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        };
        self.commit_candidate(candidate, description)
    }

    /// Mutate the PIR transactionally. The verifier must pass; projections are
    /// regenerated from the new IR before commit.
    pub fn commit_ir<F>(
        &mut self,
        description: impl Into<String>,
        mutation: F,
    ) -> CommitResult
    where
        F: FnOnce(&mut PcbIr) -> Diagnostics,
    {
        let description = description.into();
        let mut candidate = self.clone();
        let mut diagnostics = mutation(&mut candidate.ir);
        diagnostics.extend(verify(&candidate.ir));
        diagnostics.sort();
        if diagnostics.has_errors() {
            return CommitResult::rejected(description, diagnostics);
        }
        let projection = project_source(&candidate.ir);
        diagnostics.extend(projection.diagnostics.clone());
        diagnostics.sort();
        if diagnostics.has_errors() {
            // A routed PIR can contain data the current language cannot spell
            // (for example physical traces). If the projection is not compilable
            // we still commit the verified IR, because the IR remains valid and
            // is the semantic truth; the textual projection is best effort.
        }
        candidate.source = projection.source;
        candidate.graph = NodeGraph::from_ir(&candidate.ir);
        candidate.diagnostics = diagnostics;
        candidate.version = self.version + 1;
        candidate.dirty = true;
        candidate.history = self.history.clone();
        candidate.undo_stack = Vec::new();
        candidate.redo_stack = Vec::new();
        self.commit_candidate(candidate, description)
    }

    /// Apply a source-level edit that must produce a valid document.
    pub fn try_set_source(&mut self, source: impl Into<String>) -> CommitResult {
        self.commit_source(source, "Edit source")
    }

    fn commit_candidate(
        &mut self,
        mut candidate: PapanDocument,
        description: String,
    ) -> CommitResult {
        let snapshot = self.snapshot();
        let version = candidate.version;
        let diagnostics = candidate.diagnostics.clone();
        candidate.history = self.history.clone();
        candidate.history.push(HistoryEntry {
            description: description.clone(),
            version,
        });
        candidate.undo_stack = self.undo_stack.clone();
        candidate.undo_stack.push(snapshot);
        candidate.redo_stack.clear();
        *self = candidate;
        CommitResult {
            committed: true,
            description,
            diagnostics,
            version,
        }
    }

    fn snapshot(&self) -> DocumentState {
        DocumentState {
            source: self.source.clone(),
            ir: self.ir.clone(),
            graph: self.graph.clone(),
            diagnostics: self.diagnostics.clone(),
            commands: self.commands.clone(),
            version: self.version,
            dirty: self.dirty,
        }
    }

    fn restore(&mut self, state: DocumentState) {
        self.source = state.source;
        self.ir = state.ir;
        self.graph = state.graph;
        self.diagnostics = state.diagnostics;
        self.commands = state.commands;
        self.version = state.version;
        self.dirty = state.dirty;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Undo the last committed edit.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.redo_stack.push(current);
        if let Some(entry) = self.history.pop() {
            self.history.push(HistoryEntry {
                description: format!("undo {}", entry.description),
                version: previous.version,
            });
        }
        self.restore(previous);
        self.dirty = true;
        true
    }

    /// Redo the last undone edit.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.undo_stack.push(current);
        self.restore(next);
        self.dirty = true;
        true
    }

    /// Regenerate both projections from the IR.
    pub fn refresh_projections(&mut self) {
        self.graph = NodeGraph::from_ir(&self.ir);
        self.source = project_source(&self.ir).source;
    }

    /// Verify the current PIR in place (used after loading serialized IR).
    pub fn revalidate(&mut self) -> ValidationReport {
        let mut diagnostics = self.diagnostics.clone();
        diagnostics.extend(verify(&self.ir));
        diagnostics.extend(self.graph.validate());
        diagnostics.sort();
        self.diagnostics = diagnostics.clone();
        ValidationReport::from_diagnostics(diagnostics)
    }
}

/// Outcome of a transactional edit.
#[derive(Clone, Debug)]
pub struct CommitResult {
    pub committed: bool,
    pub description: String,
    pub diagnostics: Diagnostics,
    pub version: u64,
}

impl CommitResult {
    fn rejected(description: impl Into<String>, diagnostics: Diagnostics) -> Self {
        let mut diagnostics = diagnostics;
        diagnostics.sort();
        Self {
            committed: false,
            description: description.into(),
            diagnostics,
            version: 0,
        }
    }

    pub fn is_ok(&self) -> bool {
        self.committed
    }
}

fn default_source(title: &str) -> String {
    format!(
        "board \"{}\"\n\nsize 100mm x 80mm\nthickness 1.6mm\nlayers 2\n",
        title.replace('"', "")
    )
}

fn extract_title(source: &str) -> String {
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("board") {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix('"') {
                if let Some(end) = rest.find('"') {
                    return rest[..end].to_string();
                }
            }
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    "Untitled".to_string()
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct DocumentError {
    pub code: String,
    pub message: String,
    pub diagnostics: Diagnostics,
}

impl DocumentError {
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

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for DocumentError {}

// ---------------------------------------------------------------------------
// Project layout
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub schema_version: u32,
    pub name: String,
    pub version: String,
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nodes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
}

impl ProjectManifest {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            name: name.into(),
            version: "0.1.0".to_string(),
            sources: vec!["src/main.pn".to_string()],
            nodes: Some("nodes/main.graph".to_string()),
            output: Some("output".to_string()),
            targets: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, DocumentError> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            DocumentError::new(
                "DOC_IO",
                format!("cannot read '{}': {}", path.display(), error),
            )
        })?;
        serde_json::from_str(&text).map_err(|error| {
            DocumentError::new(
                "DOC_MANIFEST",
                format!("invalid project manifest '{}': {}", path.display(), error),
            )
        })
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)
    }
}

#[derive(Clone, Debug)]
pub struct ProjectFiles {
    pub root: PathBuf,
    pub manifest: ProjectManifest,
    pub source: String,
    pub graph: Option<NodeGraph>,
    pub source_files: Vec<PathBuf>,
}

impl ProjectFiles {
    /// Resolve a project directory, manifest or source file.
    pub fn load(path: &Path) -> Result<Self, DocumentError> {
        let path = path.to_path_buf();
        if path.is_dir() {
            return Self::from_directory(&path);
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match extension.as_str() {
            "papan" => Self::from_manifest_file(&path),
            "pn" | "graph" => Self::from_single_file(&path),
            _ => {
                if path.exists() {
                    Self::from_single_file(&path)
                } else {
                    Err(DocumentError::new(
                        "DOC_IO",
                        format!("project path '{}' does not exist", path.display()),
                    ))
                }
            }
        }
    }

    fn from_directory(root: &Path) -> Result<Self, DocumentError> {
        let manifest_path = root.join("project.papan");
        if manifest_path.is_file() {
            return Self::from_manifest_file(&manifest_path);
        }
        let mut sources: Vec<PathBuf> = Vec::new();
        let src = root.join("src");
        if src.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(&src)
                .map_err(|error| {
                    DocumentError::new(
                        "DOC_IO",
                        format!("cannot read '{}': {}", src.display(), error),
                    )
                })?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .and_then(|extension| extension.to_str())
                        .map(|extension| extension.eq_ignore_ascii_case("pn"))
                        .unwrap_or(false)
                })
                .collect();
            entries.sort();
            sources.extend(entries);
        }
        if sources.is_empty() {
            return Err(DocumentError::new(
                "DOC_IO",
                format!(
                    "'{}' is not a PAPAN project (missing project.papan or src/*.pn)",
                    root.display()
                ),
            ));
        }
        let mut manifest = ProjectManifest::new(
            root.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("project"),
        );
        manifest.sources = sources
            .iter()
            .filter_map(|path| {
                path.strip_prefix(root)
                    .ok()
                    .map(|path| path.to_string_lossy().replace('\\', "/"))
            })
            .collect();
        let source = read_sources(root, &sources)?;
        let graph = read_graph(root.join("nodes/main.graph"));
        Ok(Self {
            root: root.to_path_buf(),
            manifest,
            source,
            graph,
            source_files: sources,
        })
    }

    fn from_manifest_file(manifest_path: &Path) -> Result<Self, DocumentError> {
        let root = manifest_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let manifest = match ProjectManifest::load(manifest_path) {
            Ok(manifest) => manifest,
            Err(error) => {
                // A `.papan` file may also be a standalone source program.
                let text = std::fs::read_to_string(manifest_path).map_err(|read_error| {
                    DocumentError::new(
                        "DOC_IO",
                        format!(
                            "cannot read '{}': {} (manifest error: {})",
                            manifest_path.display(),
                            read_error,
                            error.message
                        ),
                    )
                })?;
                let mut manifest = ProjectManifest::new(
                    manifest_path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("project"),
                );
                manifest.sources = vec![manifest_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("main.pn")
                    .to_string()];
                manifest.nodes = None;
                return Ok(Self {
                    root,
                    manifest,
                    source: text,
                    graph: None,
                    source_files: vec![manifest_path.to_path_buf()],
                });
            }
        };
        let mut sources = Vec::new();
        for entry in &manifest.sources {
            let source_path = root.join(entry);
            if source_path.is_file() {
                sources.push(source_path);
            }
        }
        if sources.is_empty() {
            let fallback = root.join("src/main.pn");
            if fallback.is_file() {
                sources.push(fallback);
            }
        }
        if sources.is_empty() {
            return Err(DocumentError::new(
                "DOC_IO",
                format!(
                    "project '{}' lists no readable source files",
                    manifest_path.display()
                ),
            ));
        }
        let source = read_sources(&root, &sources)?;
        let graph_path = manifest
            .nodes
            .as_ref()
            .map(|nodes| root.join(nodes))
            .or_else(|| Some(root.join("nodes/main.graph")));
        let graph = graph_path.and_then(read_graph);
        Ok(Self {
            root,
            manifest,
            source,
            graph,
            source_files: sources,
        })
    }

    fn from_single_file(path: &Path) -> Result<Self, DocumentError> {
        let root = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let text = std::fs::read_to_string(path).map_err(|error| {
            DocumentError::new(
                "DOC_IO",
                format!("cannot read '{}': {}", path.display(), error),
            )
        })?;
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension == "graph" {
            let graph = NodeGraph::from_json(&text).map_err(|error| {
                DocumentError::new(
                    "DOC_GRAPH",
                    format!("invalid node graph '{}': {}", path.display(), error),
                )
            })?;
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("graph");
            let mut manifest = ProjectManifest::new(name);
            manifest.sources.clear();
            manifest.nodes = Some(
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("main.graph")
                    .to_string(),
            );
            return Ok(Self {
                root,
                manifest,
                source: String::new(),
                graph: Some(graph),
                source_files: Vec::new(),
            });
        }
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("project");
        let mut manifest = ProjectManifest::new(name);
        manifest.sources = vec![path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("main.pn")
            .to_string()];
        manifest.nodes = None;
        Ok(Self {
            root,
            manifest,
            source: text,
            graph: None,
            source_files: vec![path.to_path_buf()],
        })
    }

    /// Write the canonical source projection back to `src/main.pn` and save the
    /// node projection. The project manifest is created if missing.
    pub fn save_document(&self, document: &PapanDocument) -> std::io::Result<()> {
        let manifest_path = self.root.join("project.papan");
        let mut manifest = self.manifest.clone();
        if manifest.sources.is_empty() {
            manifest.sources = vec!["src/main.pn".to_string()];
        }
        if let Some(first) = manifest.sources.first() {
            document.save_source(&self.root.join(first))?;
        } else {
            document.save_source(&self.root.join("src/main.pn"))?;
        }
        if let Some(nodes) = &manifest.nodes {
            document.graph.save(&self.root.join(nodes))?;
        }
        manifest.save(&manifest_path)
    }
}

fn read_sources(root: &Path, sources: &[PathBuf]) -> Result<String, DocumentError> {
    let mut combined = String::new();
    for path in sources {
        let resolved = if path.is_absolute() || path.starts_with(root) {
            path.clone()
        } else {
            root.join(path)
        };
        let text = std::fs::read_to_string(&resolved).map_err(|error| {
            DocumentError::new(
                "DOC_IO",
                format!("cannot read '{}': {}", resolved.display(), error),
            )
        })?;
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&text);
        if !combined.ends_with('\n') {
            combined.push('\n');
        }
    }
    Ok(combined)
}

fn read_graph(path: PathBuf) -> Option<NodeGraph> {
    let text = std::fs::read_to_string(&path).ok()?;
    NodeGraph::from_json(&text).ok()
}

/// Create the standard PAPAN project layout.
pub fn create_project(root: &Path, name: &str) -> std::io::Result<ProjectManifest> {
    let root = if root.extension().and_then(|extension| extension.to_str()) == Some("papan") {
        root.to_path_buf()
    } else {
        root.join(format!("{name}.papan"))
    };
    std::fs::create_dir_all(root.join("src"))?;
    std::fs::create_dir_all(root.join("nodes"))?;
    std::fs::create_dir_all(root.join("output"))?;
    std::fs::create_dir_all(root.join("libraries"))?;
    std::fs::create_dir_all(root.join("assets/models"))?;
    std::fs::create_dir_all(root.join("tests"))?;
    std::fs::create_dir_all(root.join("cache"))?;

    let source = default_source(name);
    std::fs::write(root.join("src/main.pn"), &source)?;
    let document = PapanDocument::from_source(&source);
    document.graph.save(&root.join("nodes/main.graph"))?;
    let manifest = ProjectManifest::new(name);
    manifest.save(&root.join("project.papan"))?;
    Ok(manifest)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
board "Demo"

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
place R2 at (25mm, 10mm)
route N1 {
    width: 0.25mm
    layer: F.Cu
}
"#;

    #[test]
    fn compiles_sample_and_builds_graph() {
        let document = PapanDocument::from_source(SAMPLE);
        assert!(!document.has_errors(), "{:?}", document.diagnostics);
        assert_eq!(document.ir.components.len(), 2);
        assert_eq!(document.ir.nets.len(), 1);
        assert!(document.graph.node_named("R1").is_some());
        assert_eq!(document.graph.nodes_of_kind("Pin").len(), 2);
    }

    #[test]
    fn invalid_source_does_not_replace_valid_document() {
        let mut document = PapanDocument::from_source(SAMPLE);
        let before = document.source.clone();
        let result = document.commit_source(
            "board \"Broken\"\ncomponent R1: not_a_kind\n",
            "break it",
        );
        assert!(!result.committed);
        assert!(result.diagnostics.has_errors());
        assert_eq!(document.source, before);
        assert!(document.ir.components.len() == 2);
    }

    #[test]
    fn transactions_support_undo_and_redo() {
        let mut document = PapanDocument::from_source(SAMPLE);
        let original = document.ir.board.width;
        let edited = SAMPLE.replace("size 40mm x 30mm", "size 60mm x 30mm");
        let result = document.commit_source(edited, "resize board");
        assert!(result.committed);
        assert_eq!(document.ir.board.width, papan_ir::Length::from_mm(60.0));
        assert!(document.can_undo());
        assert!(document.undo());
        assert_eq!(document.ir.board.width, original);
        assert!(document.can_redo());
        assert!(document.redo());
        assert_eq!(document.ir.board.width, papan_ir::Length::from_mm(60.0));
    }

    #[test]
    fn text_projection_roundtrips_semantics() {
        let document = PapanDocument::from_source(SAMPLE);
        let projection = document.projected_source();
        assert!(!projection.has_errors(), "{:?}", projection.diagnostics);
        let recompiled = PapanDocument::from_source(&projection.source);
        assert!(!recompiled.has_errors(), "{:?}", recompiled.diagnostics);
        assert_eq!(document.ir.board.width, recompiled.ir.board.width);
        assert_eq!(document.ir.components.len(), recompiled.ir.components.len());
        assert_eq!(document.ir.nets.len(), recompiled.ir.nets.len());
        assert_eq!(
            document.ir.connections.len(),
            recompiled.ir.connections.len()
        );
        for (left, right) in document.ir.components.iter().zip(&recompiled.ir.components) {
            assert_eq!(left.designator, right.designator);
            assert_eq!(left.position, right.position);
            assert_eq!(left.value, right.value);
        }
    }

    #[test]
    fn graph_edit_is_transactional() {
        let mut document = PapanDocument::from_source(SAMPLE);
        let mut graph = document.graph.clone();
        let id = graph.add_node(
            papan_graph::NodeKind::new("Net").unwrap(),
            "GND",
        );
        if let Some(node) = graph.node_mut(&id) {
            node.parameters.insert(
                "name".to_string(),
                papan_graph::ParamValue::String("GND".to_string()),
            );
        }
        let result = document.commit_graph(graph, "add ground net");
        assert!(result.committed, "{:?}", result.diagnostics);
        assert!(document.ir.find_net("GND").is_some());
        assert!(document.undo());
        assert!(document.ir.find_net("GND").is_none());
    }

    #[test]
    fn ir_transaction_regenerates_projection() {
        let mut document = PapanDocument::from_source(SAMPLE);
        let result = document.commit_ir("move R1", |ir| {
            if let Some(component) = ir.find_component_mut("R1") {
                component.position = Some(papan_ir::Point::from_mm(5.0, 5.0));
            }
            Diagnostics::new()
        });
        assert!(result.committed);
        assert!(document.source.contains("place R1 at (5mm, 5mm)"));
        assert_eq!(
            document.ir.find_component("R1").unwrap().position,
            Some(papan_ir::Point::from_mm(5.0, 5.0))
        );
    }

    #[test]
    fn json_roundtrip_preserves_document() {
        let document = PapanDocument::from_source(SAMPLE);
        let json = document.to_json().unwrap();
        let restored = PapanDocument::from_json(&json).unwrap();
        assert_eq!(restored.source, document.source);
        assert_eq!(restored.ir, document.ir);
        assert_eq!(restored.graph, document.graph);
    }

    #[test]
    fn creates_and_loads_project_layout() {
        let root = std::env::temp_dir().join(format!("papan-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = create_project(&root, "demo").unwrap();
        assert_eq!(manifest.name, "demo");
        let project = ProjectFiles::load(&root.join("demo.papan")).unwrap();
        assert!(project.source.contains("board \"demo\""));
        assert!(project.graph.is_some());
        let document = PapanDocument::load(&root.join("demo.papan")).unwrap();
        assert!(!document.has_errors());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn malformed_json_manifest_falls_back_to_source() {
        let path = std::env::temp_dir().join(format!(
            "papan-standalone-{}-{}.papan",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "board \"Solo\"\nsize 10mm x 10mm\n").unwrap();
        let project = ProjectFiles::load(&path).unwrap();
        assert!(project.source.contains("Solo"));
        let _ = std::fs::remove_file(&path);
    }
}
