use papan_api::PapanProject;
use papan_document::PapanDocument;
use papan_types::{Diagnostics, Severity};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiagnosticDto {
    pub code: String,
    pub severity: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub source: String,
    pub diagnostics: Vec<DiagnosticDto>,
    pub has_errors: bool,
    pub component_count: usize,
    pub net_count: usize,
    pub trace_count: usize,
    pub via_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub diagnostics: Vec<DiagnosticDto>,
    pub has_errors: bool,
    pub component_count: usize,
    pub net_count: usize,
}

fn severity_str(sev: &Severity) -> String {
    format!("{sev:?}").to_lowercase()
}

fn map_diagnostics(diagnostics: &Diagnostics) -> Vec<DiagnosticDto> {
    diagnostics
        .iter()
        .map(|d| DiagnosticDto {
            code: d.code.clone(),
            severity: severity_str(&d.severity),
            message: d.message.clone(),
        })
        .collect()
}

fn document_info(name: String, source: String, doc: &PapanDocument) -> ProjectInfo {
    let diagnostics = map_diagnostics(&doc.diagnostics);
    ProjectInfo {
        name,
        source,
        has_errors: doc.diagnostics.has_errors(),
        diagnostics,
        component_count: doc.ir.components.len(),
        net_count: doc.ir.nets.len(),
        trace_count: doc.ir.traces.len(),
        via_count: doc.ir.vias.len(),
    }
}

#[tauri::command]
fn open_project(path: String) -> Result<ProjectInfo, String> {
    let project = PapanProject::open(&path).map_err(|e| e.to_string())?;
    let doc = project.document();
    Ok(document_info(doc.title.clone(), doc.source.clone(), doc))
}

#[tauri::command]
fn check_source(source: String) -> Result<CheckResult, String> {
    let doc = PapanDocument::from_source(&source);
    Ok(CheckResult {
        has_errors: doc.diagnostics.has_errors(),
        diagnostics: map_diagnostics(&doc.diagnostics),
        component_count: doc.ir.components.len(),
        net_count: doc.ir.nets.len(),
    })
}

#[tauri::command]
fn app_info() -> String {
    format!("PAPAN Desktop (papan-api v{})", papan_api::API_VERSION)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![open_project, check_source, app_info])
        .run(tauri::generate_context!())
        .expect("error while running PAPAN desktop");
}
