use papan_api::PapanProject;
use papan_document::PapanDocument;
use papan_ir::{NetKind, PadShape, PcbIr, Side};
use papan_types::{Diagnostics, Severity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use tauri::State;

#[derive(Default)]
pub struct AppState {
    project: Mutex<Option<PapanProject>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiagnosticDto {
    pub code: String,
    pub severity: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayerDto { pub id: String, pub name: String, pub kind: String, pub copper: bool }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetDto { pub id: String, pub name: String, pub kind: String, pub member_count: usize }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PointDto { pub x: f64, pub y: f64 }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PadDto { pub name: String, pub x: f64, pub y: f64, pub w: f64, pub h: f64, pub shape: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentDto {
    pub id: String, pub designator: String, pub part: String, pub value: String,
    pub x: f64, pub y: f64, pub rotation_deg: f64, pub side: String, pub placed: bool,
    pub pads: Vec<PadDto>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceDto { pub id: String, pub net: String, pub layer: String, pub width_mm: f64, pub points: Vec<PointDto> }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViaDto { pub id: String, pub net: String, pub x: f64, pub y: f64, pub diameter_mm: f64, pub drill_mm: f64 }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardDto { pub name: String, pub width_mm: f64, pub height_mm: f64, pub thickness_mm: f64 }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PinWorldDto { pub net: String, pub component: String, pub pin: String, pub x: f64, pub y: f64 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardView {
    pub name: String, pub source: String, pub diagnostics: Vec<DiagnosticDto>, pub has_errors: bool,
    pub board: BoardDto, pub layers: Vec<LayerDto>, pub nets: Vec<NetDto>,
    pub components: Vec<ComponentDto>, pub traces: Vec<TraceDto>, pub vias: Vec<ViaDto>,
    pub can_edit: bool,
    #[serde(default)] pub can_undo: bool,
    #[serde(default)] pub can_redo: bool,
    #[serde(default)] pub pin_world: Vec<PinWorldDto>,
}

fn mm(nm: i64) -> f64 { nm as f64 / 1_000_000.0 }
fn mm_len(l: papan_ir::Length) -> f64 { mm(l.0) }
fn pt(p: papan_ir::Point) -> PointDto { PointDto { x: mm_len(p.x), y: mm_len(p.y) } }
fn severity_str(sev: &Severity) -> String { format!("{sev:?}").to_lowercase() }
fn map_diagnostics(diagnostics: &Diagnostics) -> Vec<DiagnosticDto> {
    diagnostics.iter().map(|d| DiagnosticDto { code: d.code.clone(), severity: severity_str(&d.severity), message: d.message.clone() }).collect()
}
fn net_kind(kind: &NetKind) -> String { format!("{kind:?}").to_lowercase() }
fn pad_shape(shape: &PadShape) -> String { match shape { PadShape::Round => "circle".to_string(), PadShape::Rect => "rect".to_string() } }

fn rotate_pt(x: f64, y: f64, deg: f64) -> (f64, f64) {
    let r = deg.to_radians();
    (x * r.cos() - y * r.sin(), x * r.sin() + y * r.cos())
}

fn build_board_view(name: String, source: String, ir: &PcbIr, diagnostics: &Diagnostics, can_edit: bool, can_undo: bool, can_redo: bool) -> BoardView {
    let defs: BTreeMap<_, _> = ir.definitions.iter().map(|d| (d.id.clone(), d)).collect();
    let comp_index: BTreeMap<_, _> = ir.components.iter().map(|c| (c.id.clone(), c)).collect();
    let layers = ir.stackup.layers.iter().map(|l| LayerDto { id: l.id.to_string(), name: l.name.clone(), kind: format!("{:?}", l.kind).to_lowercase(), copper: l.kind.is_copper() }).collect();
    let nets = ir.nets.iter().map(|n| NetDto { id: n.id.to_string(), name: n.name.clone(), kind: net_kind(&n.kind), member_count: n.members.len() }).collect();
    let components = ir.components.iter().map(|c| {
        let (x, y, placed) = match c.position { Some(p) => (mm_len(p.x), mm_len(p.y), true), None => (0.0, 0.0, false) };
        let def = defs.get(&c.definition_id).copied();
        let pads = def.map(|d| d.footprint.pads.iter().map(|pad| PadDto {
            name: if !pad.number.is_empty() { pad.number.clone() } else { pad.pin.clone() },
            x: mm_len(pad.position.x), y: mm_len(pad.position.y), w: mm_len(pad.size.width), h: mm_len(pad.size.height), shape: pad_shape(&pad.shape),
        }).collect()).unwrap_or_default();
        let value = c.properties.get("value").map(|v| format!("{v:?}")).or_else(|| c.value.as_ref().map(|v| format!("{v:?}"))).unwrap_or_default();
        ComponentDto {
            id: c.id.to_string(), designator: c.designator.clone(), part: def.map(|d| d.name.clone()).unwrap_or_else(|| c.definition_id.to_string()),
            value, x, y, rotation_deg: c.rotation.0 as f64 / 1000.0, side: format!("{:?}", c.side).to_lowercase(), placed, pads,
        }
    }).collect();
    // resolve net member pin positions for ratsnest (world coords)
    let mut pin_world = Vec::new();
    for net in &ir.nets {
        for member in &net.members {
            if let Some(c) = comp_index.get(&member.component) {
                if let (Some(pos), Some(def)) = (c.position, defs.get(&c.definition_id)) {
                    let rot = c.rotation.0 as f64 / 1000.0;
                    let (cx, cy) = (mm_len(pos.x), mm_len(pos.y));
                    if let Some(pad) = def.footprint.pads.iter().find(|p| p.number == member.pin || p.pin == member.pin) {
                        let (px, py) = rotate_pt(mm_len(pad.position.x), mm_len(pad.position.y), rot);
                        pin_world.push(PinWorldDto { net: net.name.clone(), component: c.designator.clone(), pin: member.pin.clone(), x: cx + px, y: cy + py });
                    } else {
                        pin_world.push(PinWorldDto { net: net.name.clone(), component: c.designator.clone(), pin: member.pin.clone(), x: cx, y: cy });
                    }
                }
            }
        }
    }
    let traces = ir.traces.iter().map(|t| TraceDto { id: t.id.to_string(), net: t.net.to_string(), layer: t.layer.to_string(), width_mm: mm_len(t.width), points: t.points.iter().copied().map(pt).collect() }).collect();
    let vias = ir.vias.iter().map(|v| ViaDto { id: v.id.to_string(), net: v.net.to_string(), x: mm_len(v.at.x), y: mm_len(v.at.y), diameter_mm: mm_len(v.diameter), drill_mm: mm_len(v.drill) }).collect();
    BoardView {
        name, source, has_errors: diagnostics.has_errors(), diagnostics: map_diagnostics(diagnostics),
        board: BoardDto { name: ir.board.name.clone(), width_mm: mm_len(ir.board.width), height_mm: mm_len(ir.board.height), thickness_mm: mm_len(ir.board.thickness) },
        layers, nets, components, traces, vias, can_edit, can_undo, can_redo, pin_world,
    }
}

fn view_from_project(project: &PapanProject) -> BoardView {
    let doc = project.document();
    let source = project.source().to_string();
    build_board_view(doc.title.clone(), source, &doc.ir, &doc.diagnostics, true, project.can_undo(), project.can_redo())
}

#[tauri::command]
fn app_info() -> String { format!("PAPAN Desktop (papan-api v{})", papan_api::API_VERSION) }

#[tauri::command]
fn open_project(path: String, state: State<'_, AppState>) -> Result<BoardView, String> {
    let mut project = PapanProject::open(&path).map_err(|e| e.to_string())?;
    // Auto-route when nets exist but no traces yet, so projects open with visible routing.
    if !project.ir().nets.is_empty() && project.ir().traces.is_empty() {
        let _ = project.board().autoroute();
    }
    let view = view_from_project(&project);
    *state.project.lock().map_err(|_| "state lock poisoned")? = Some(project);
    Ok(view)
}

#[tauri::command]
fn new_project(name: String, width_mm: f64, height_mm: f64, layers: u32, state: State<'_, AppState>) -> Result<BoardView, String> {
    let layers = layers.clamp(1, 8);
    let source = format!(
        "board \"{}\"\n\nsize {}mm x {}mm\nthickness 1.6mm\nlayers {}\n",
        name.replace('"', "'"),
        width_mm,
        height_mm,
        layers
    );
    let project = PapanProject::compile(source).map_err(|e| e.to_string())?;
    let view = view_from_project(&project);
    *state.project.lock().map_err(|_| "state lock poisoned")? = Some(project);
    Ok(view)
}

#[tauri::command]
fn check_source(source: String) -> Result<BoardView, String> {
    let doc = PapanDocument::from_source(&source);
    Ok(build_board_view(doc.title.clone(), source, &doc.ir, &doc.diagnostics, false, false, false))
}

#[tauri::command]
fn undo(state: State<'_, AppState>) -> Result<BoardView, String> {
    let mut guard = state.project.lock().map_err(|_| "state lock poisoned")?;
    let project = guard.as_mut().ok_or("Belum ada proyek.")?;
    if !project.undo() { return Err("Tidak ada yang bisa di-undo.".to_string()); }
    Ok(view_from_project(project))
}

#[tauri::command]
fn redo(state: State<'_, AppState>) -> Result<BoardView, String> {
    let mut guard = state.project.lock().map_err(|_| "state lock poisoned")?;
    let project = guard.as_mut().ok_or("Belum ada proyek.")?;
    if !project.redo() { return Err("Tidak ada yang bisa di-redo.".to_string()); }
    Ok(view_from_project(project))
}

#[tauri::command]
fn save_source(path: String, state: State<'_, AppState>) -> Result<String, String> {
    let guard = state.project.lock().map_err(|_| "state lock poisoned")?;
    let project = guard.as_ref().ok_or("Belum ada proyek.")?;
    project.save_source(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

#[tauri::command]
fn autoroute(state: State<'_, AppState>) -> Result<BoardView, String> {
    let mut guard = state.project.lock().map_err(|_| "state lock poisoned")?;
    let project = guard.as_mut().ok_or("Belum ada proyek.")?;
    project.board().autoroute().map_err(|e| e.to_string())?;
    Ok(view_from_project(project))
}

#[tauri::command]
fn get_view(state: State<'_, AppState>) -> Result<BoardView, String> {
    let guard = state.project.lock().map_err(|_| "state lock poisoned")?;
    let project = guard.as_ref().ok_or("Belum ada proyek. Buka atau buat proyek dulu.")?;
    Ok(view_from_project(project))
}

#[tauri::command]
fn add_component_at(part: String, x_mm: f64, y_mm: f64, state: State<'_, AppState>) -> Result<BoardView, String> {
    let mut guard = state.project.lock().map_err(|_| "state lock poisoned")?;
    let project = guard.as_mut().ok_or("Belum ada proyek. Buka atau buat proyek dulu.")?;
    let result = (|| -> Result<(), String> {
        let mut board = project.board();
        let cref = board.add_component(&part).map_err(|e| e.to_string())?;
        board.place_mm(&cref, x_mm, y_mm, 0.0).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = result { return Err(e); }
    Ok(view_from_project(project))
}

fn component_info(project: &PapanProject, designator: &str) -> Option<(f64, bool)> {
    project.document().ir.components.iter()
        .find(|c| c.designator.eq_ignore_ascii_case(designator))
        .map(|c| (c.rotation.0 as f64 / 1000.0, matches!(c.side, Side::Top)))
}

#[tauri::command]
fn move_component_to(designator: String, x_mm: f64, y_mm: f64, rotation_deg: Option<f64>, state: State<'_, AppState>) -> Result<BoardView, String> {
    let mut guard = state.project.lock().map_err(|_| "state lock poisoned")?;
    let project = guard.as_mut().ok_or("Belum ada proyek. Buka atau buat proyek dulu.")?;
    let (cur_rot, _) = component_info(project, &designator).unwrap_or((0.0, true));
    let rot = rotation_deg.unwrap_or(cur_rot);
    let result = (|| -> Result<(), String> {
        let mut board = project.board();
        let cref = board.component(&designator).ok_or_else(|| format!("Komponen '{designator}' tidak ditemukan"))?;
        board.place_mm(&cref, x_mm, y_mm, rot).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = result { return Err(e); }
    Ok(view_from_project(project))
}

#[tauri::command]
fn rotate_component(designator: String, rotation_deg: f64, state: State<'_, AppState>) -> Result<BoardView, String> {
    let mut guard = state.project.lock().map_err(|_| "state lock poisoned")?;
    let project = guard.as_mut().ok_or("Belum ada proyek. Buka atau buat proyek dulu.")?;
    let result = (|| -> Result<(), String> {
        let mut board = project.board();
        let cref = board.component(&designator).ok_or_else(|| format!("Komponen '{designator}' tidak ditemukan"))?;
        board.rotate_component(&cref, rotation_deg).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = result { return Err(e); }
    Ok(view_from_project(project))
}

#[tauri::command]
fn delete_component(designator: String, state: State<'_, AppState>) -> Result<BoardView, String> {
    let mut guard = state.project.lock().map_err(|_| "state lock poisoned")?;
    let project = guard.as_mut().ok_or("Belum ada proyek. Buka atau buat proyek dulu.")?;
    let result = (|| -> Result<(), String> {
        let mut board = project.board();
        let cref = board.component(&designator).ok_or_else(|| format!("Komponen '{designator}' tidak ditemukan"))?;
        board.remove_component(&cref).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = result { return Err(e); }
    Ok(view_from_project(project))
}

#[tauri::command]
fn flip_component(designator: String, state: State<'_, AppState>) -> Result<BoardView, String> {
    let mut guard = state.project.lock().map_err(|_| "state lock poisoned")?;
    let project = guard.as_mut().ok_or("Belum ada proyek. Buka atau buat proyek dulu.")?;
    let (_, is_top) = component_info(project, &designator).ok_or_else(|| format!("Komponen '{designator}' tidak ditemukan"))?;
    let side = if is_top { Side::Bottom } else { Side::Top };
    let result = (|| -> Result<(), String> {
        let mut board = project.board();
        let cref = board.component(&designator).ok_or_else(|| format!("Komponen '{designator}' tidak ditemukan"))?;
        board.set_side(&cref, side).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = result { return Err(e); }
    Ok(view_from_project(project))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            app_info, open_project, new_project, check_source, get_view,
            add_component_at, move_component_to, rotate_component, delete_component, flip_component,
            undo, redo, save_source, autoroute
        ])
        .run(tauri::generate_context!())
        .expect("error while running PAPAN desktop");
}
