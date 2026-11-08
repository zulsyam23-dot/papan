//! End-to-end PAPAN integration tests.

use papan_api::{ExportFormat, PapanProject};
use papan_integration_tests::{temp_dir, TWO_PIN_BOARD};

#[test]
fn source_compiles_routes_validates_and_exports_manufacturing_data() {
    let mut project = PapanProject::compile(TWO_PIN_BOARD).expect("compile");
    assert!(!project.has_errors(), "{:?}", project.diagnostics());
    let route = project.board().autoroute().expect("autoroute");
    assert!(!route.has_errors(), "{:?}", route.diagnostics);
    assert!(!project.ir().traces.is_empty(), "autorouter must create traces");

    let bundle = project.export(ExportFormat::Manufacturing).expect("export");
    let files = bundle.file_map();
    let gerber = files.iter().find(|(n, _)| n.ends_with("Cu.gbr")).expect("copper gerber");
    assert!(gerber.1.contains("%MOMM*%"));
    assert!(files.get("drill.drl").expect("drill").contains("M30"));
    assert!(files.get("bom.csv").expect("bom").contains("R1"));
    assert!(files.get("pick_and_place.csv").expect("pnp").contains("R1"));
}

#[test]
fn document_project_roundtrip_preserves_design() {
    let dir = temp_dir("end-to-end");
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("board.pn");
    std::fs::write(&source, TWO_PIN_BOARD).unwrap();

    let project = PapanProject::open(&source).expect("open");
    assert_eq!(project.ir().components.len(), 2);
    assert_eq!(project.ir().nets.len(), 1);

    let json_path = dir.join("board.document.json");
    project.save_document_json(&json_path).expect("save json");
    let json = std::fs::read_to_string(&json_path).unwrap();
    let reopened = PapanProject::from_json(&json).expect("from json");
    assert_eq!(reopened.ir().board.name, "Integration Demo");
    assert_eq!(reopened.ir().components.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
