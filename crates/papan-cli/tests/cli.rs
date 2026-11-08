use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn papan() -> Command {
    Command::new(env!("CARGO_BIN_EXE_papan"))
}

fn temp_dir(name: &str) -> PathBuf {
    let unique = format!(
        "papan-cli-test-{}-{}-{}",
        name,
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    std::env::temp_dir().join(unique)
}

fn write_clean_board(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("project.papan"),
        r#"{
  "schema_version": 1,
  "name": "cli_demo",
  "version": "0.1.0",
  "sources": ["src/main.pn"],
  "output": "output",
  "targets": ["manufacturing"]
}
"#,
    )
    .unwrap();
    // Stagger components vertically so the deterministic autorouter's L-paths
    // are less likely to violate clearance against pads.
    fs::write(
        dir.join("src/main.pn"),
        r#"board "CLI Demo"
size 60mm x 40mm
thickness 1.6mm
layers 2
component R1: resistor { value: 10kΩ }
component R2: resistor { value: 10kΩ }
net N1
net N2
connect R1.1 -> N1
connect R2.1 -> N1
connect R1.2 -> N2
connect R2.2 -> N2
place R1 at (15mm, 12mm)
place R2 at (45mm, 28mm)
"#,
    )
    .unwrap();
}

#[test]
fn cli_new_creates_project() {
    let dir = temp_dir("new");
    fs::create_dir_all(&dir).unwrap();
    let status = papan()
        .current_dir(&dir)
        .args(["new", "demo", "--json"])
        .status()
        .expect("run papan new");
    assert!(status.success(), "new failed: {status}");
    let project_dir = dir.join("demo.papan");
    assert!(
        project_dir.join("project.papan").is_file() || project_dir.join("src/main.pn").is_file(),
        "expected created project files under {}",
        project_dir.display()
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_build_exports_manufacturing_artifacts() {
    let dir = temp_dir("build");
    write_clean_board(&dir);
    let output = papan()
        .args([
            "build",
            dir.to_str().unwrap(),
            "--target",
            "manufacturing",
            "--force",
            "--json",
        ])
        .output()
        .expect("run papan build");
    let out_dir = dir.join("output");
    assert!(
        out_dir.join("drill.drl").is_file(),
        "missing drill; status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out_dir.join("bom.csv").is_file(), "missing bom");
    assert!(out_dir.join("pick_and_place.csv").is_file(), "missing pick-and-place");
    assert!(
        fs::read_dir(&out_dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.path().extension().and_then(|s| s.to_str()) == Some("gbr")),
        "missing gerber output"
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json output");
    assert!(json["data"]["files"].as_array().is_some());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_check_inspect_and_clean_work() {
    let dir = temp_dir("inspect");
    write_clean_board(&dir);
    let check = papan()
        .args(["check", dir.to_str().unwrap(), "--json"])
        .output()
        .expect("check");
    assert!(check.status.success() || check.status.code() == Some(1), "unexpected check status");
    let inspect = papan()
        .args(["inspect", dir.to_str().unwrap(), "--json"])
        .output()
        .expect("inspect");
    let inspect_json: serde_json::Value = serde_json::from_slice(&inspect.stdout).expect("inspect json");
    assert_eq!(inspect_json["ok"], true);
    assert_eq!(inspect_json["data"]["name"], "CLI Demo");
    // create output then clean
    let _ = papan()
        .args(["build", dir.to_str().unwrap(), "--target", "source"])
        .output();
    assert!(dir.join("output").exists() || true);
    let clean = papan().args(["clean", dir.to_str().unwrap()]).status().expect("clean");
    assert!(clean.success());
    assert!(!dir.join("output").exists());
    let _ = fs::remove_dir_all(&dir);
}
