const invoke = window.__TAURI__?.core?.invoke;
let currentView = null;
let pan = { x: 30, y: 30 }, zoom = 5;
let panning = null, moving = null, activePart = null, raf = 0, pendingPt = null;
let selectedDesignator = null, showHelp = false;
let mouseBoardPt = null, currentPath = null;

const $ = id => document.getElementById(id);
const layerColors = { 'f.cu': '#d94f4f', 'b.cu': '#4f7fd9', 'in1.cu': '#d9b84f', 'in2.cu': '#4fd99a' };
function layerColor(name, copper) { return layerColors[(name||'').toLowerCase()] || (copper ? '#c98a5a' : '#8a949e'); }
function esc(s){ return String(s??'').replace(/[&<>"']/g, c=>({ '&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;' }[c])); }
function setStatus(msg, cls=''){ const s=$('status'); s.textContent=msg; s.className='status '+cls; }
function snapOn(){ return $('snap')?.checked; }
function snap(v){ return snapOn() ? Math.round(v) : v; }

function boardPoint(clientX, clientY) {
  const svg = $('board-canvas'), r = svg.getBoundingClientRect();
  const b = currentView?.board; if (!b) return null;
  const sx = clientX - r.left, sy = clientY - r.top;
  return { x: (sx - pan.x) / zoom, y: b.height_mm - (sy - pan.y) / zoom };
}
function clampToBoard(pt) {
  if (!pt || !currentView) return pt;
  const b = currentView.board;
  return { x: Math.max(0, Math.min(b.width_mm, snap(pt.x))), y: Math.max(0, Math.min(b.height_mm, snap(pt.y))) };
}
function setCoords(pt) {
  $('coords').textContent = pt ? `X: ${pt.x.toFixed(2)} mm  Y: ${pt.y.toFixed(2)} mm` : 'X: -- mm  Y: -- mm';
}
function rotPt(x,y,deg){ const r=deg*Math.PI/180; return [x*Math.cos(r)-y*Math.sin(r), x*Math.sin(r)+y*Math.cos(r)]; }
function ratsnestOn(){ return $('ratsnest')?.checked; }
function ghostPads(part){
  if (!currentView) return [];
  const c = (currentView.components||[]).find(x => (x.part||'').toLowerCase()===String(part).toLowerCase() && x.pads && x.pads.length);
  return c ? c.pads : [];
}
function fitView() {
  const svg = $('board-canvas'), r = svg.getBoundingClientRect(), b = currentView?.board;
  if (!r.width || !r.height || !b?.width_mm || !b?.height_mm) return;
  const pad = 36;
  zoom = Math.max(0.2, Math.min(80, Math.min((r.width-pad*2)/b.width_mm, (r.height-pad*2)/b.height_mm)));
  pan.x = (r.width - b.width_mm*zoom)/2;
  pan.y = (r.height + b.height_mm*zoom)/2;
}

function render() {
  if (!currentView) return;
  const svg = $('board-canvas'), b = currentView.board;
  const layerById = Object.fromEntries((currentView.layers||[]).map(l=>[l.id,l]));
  let out = `<rect width="100%" height="100%" fill="transparent"/>`;
  out += `<g transform="translate(${pan.x},${pan.y}) scale(${zoom},${-zoom}) translate(0,${-b.height_mm})">`;
  // grid
  if (zoom > 2.5) {
    const step = zoom > 10 ? 1 : 5;
    let dots = '';
    for (let x=0; x<=b.width_mm; x+=step) for (let y=0; y<=b.height_mm; y+=step) dots += `<circle cx="${x}" cy="${y}" r="${0.12/Math.max(1,zoom/8)}" fill="#2a3a4d"/>`;
    out += dots;
  }
  out += `<rect x="0" y="0" width="${b.width_mm}" height="${b.height_mm}" rx="0.4" fill="#132713" fill-opacity="0.95" stroke="#d4a24c" stroke-width="${1.4/zoom}"/>`;
  out += `<line x1="0" y1="0" x2="0" y2="${Math.min(5,b.height_mm)}" stroke="#d4a24c" stroke-width="${1/zoom}" opacity="0.6"/>`;
  out += `<line x1="0" y1="0" x2="${Math.min(5,b.width_mm)}" y2="0" stroke="#d4a24c" stroke-width="${1/zoom}" opacity="0.6"/>`;
  // ratsnest (airwires) - guidance, under traces
  if (ratsnestOn() && currentView.pin_world && currentView.pin_world.length) {
    const groups = {};
    for (const p of currentView.pin_world) (groups[p.net]||(groups[p.net]=[])).push(p);
    let rn = `<g opacity="0.55" stroke="#7fc8ff" fill="none" stroke-width="${Math.max(0.18, 0.9/zoom)}" stroke-dasharray="${1.2/zoom} ${0.8/zoom}">`;
    for (const pins of Object.values(groups)) {
      if (pins.length < 2) continue;
      const a = pins[0];
      for (let i=1;i<pins.length;i++) rn += `<line x1="${a.x}" y1="${a.y}" x2="${pins[i].x}" y2="${pins[i].y}"/>`;
    }
    out += rn + `</g>`;
  }
  for (const t of (currentView.traces||[])) {
    if (!t.points || t.points.length < 2) continue;
    const l = layerById[t.layer], color = l ? layerColor(l.name,l.copper) : '#59a869';
    out += `<polyline points="${t.points.map(p=>`${p.x},${p.y}`).join(' ')}" fill="none" stroke="${color}" stroke-width="${Math.max(t.width_mm,0.18)}" stroke-linecap="round" stroke-linejoin="round"/>`;
  }
  for (const v of (currentView.vias||[])) {
    out += `<circle cx="${v.x}" cy="${v.y}" r="${Math.max(v.diameter_mm/2,0.25)}" fill="#c8d0da" stroke="#fff" stroke-width="${0.06/zoom}"/>`;
    out += `<circle cx="${v.x}" cy="${v.y}" r="${Math.max(v.drill_mm/2,0.12)}" fill="#0b1219"/>`;
  }
  // ghost preview while placing
  if (activePart && mouseBoardPt) {
    const gp = mouseBoardPt, pads = ghostPads(activePart);
    out += `<g transform="translate(${gp.x},${gp.y})" opacity="0.65">`;
    out += `<line x1="-3" y1="0" x2="3" y2="0" stroke="#6cb6ff" stroke-width="${1/zoom}"/>`;
    out += `<line x1="0" y1="-3" x2="0" y2="3" stroke="#6cb6ff" stroke-width="${1/zoom}"/>`;
    for (const p of pads) {
      if (p.shape === 'circle') out += `<circle cx="${p.x}" cy="${p.y}" r="${Math.max(p.w,p.h)/2}" fill="#6cb6ff" fill-opacity="0.25" stroke="#6cb6ff" stroke-width="${0.08/zoom}"/>`;
      else out += `<rect x="${p.x-p.w/2}" y="${p.y-p.h/2}" width="${p.w}" height="${p.h}" rx="${Math.min(p.w,p.h)*0.12}" fill="#6cb6ff" fill-opacity="0.25" stroke="#6cb6ff" stroke-width="${0.08/zoom}"/>`;
    }
    out += `<text x="1.2" y="1.8" font-size="1.4" fill="#9fd0ff" font-family="Segoe UI, sans-serif" transform="scale(1,-1)">${esc(activePart)}</text>`;
    out += `</g>`;
  }
  for (const c of (currentView.components||[])) {
    if (!c.placed) continue;
    const dragging = moving && moving.designator === c.designator;
    const selected = !dragging && selectedDesignator === c.designator;
    out += `<g transform="translate(${c.x},${c.y}) rotate(${c.rotation_deg||0})">`;
    if (selected) out += `<rect x="-2.2" y="-2.2" width="4.4" height="4.4" rx="0.2" fill="none" stroke="#6cb6ff" stroke-width="${1.2/zoom}" stroke-dasharray="${0.6/zoom} ${0.4/zoom}"/>`;
    for (const p of (c.pads||[])) {
      const fill = dragging ? '#ffd166' : (selected ? '#e6a34a' : '#c87933');
      if (p.shape === 'circle') out += `<circle cx="${p.x}" cy="${p.y}" r="${Math.max(p.w,p.h)/2}" fill="${fill}" stroke="#3a220f" stroke-width="${0.06/zoom}"/>`;
      else out += `<rect x="${p.x-p.w/2}" y="${p.y-p.h/2}" width="${p.w}" height="${p.h}" rx="${Math.min(p.w,p.h)*0.12}" fill="${fill}" stroke="#3a220f" stroke-width="${0.06/zoom}"/>`;
    }
    out += `<circle cx="0" cy="0" r="${0.3/Math.max(1,zoom/12)}" fill="${selected?'#6cb6ff':'#fff'}" opacity="0.95"/>`;
    out += `<text x="0.8" y="0.5" font-size="1.5" fill="${dragging?'#ffd166':selected?'#9fd0ff':'#fff'}" font-weight="700" font-family="Segoe UI, sans-serif" transform="scale(1,-1)" paint-order="stroke" stroke="#0b1219" stroke-width="0.12">${esc(c.designator)}</text>`;
    out += `<text x="0.8" y="-1.0" font-size="0.95" fill="#cfe3ff" font-family="Segoe UI, sans-serif" transform="scale(1,-1)" paint-order="stroke" stroke="#0b1219" stroke-width="0.08">${esc(c.part)} ${c.rotation_deg?Math.round(c.rotation_deg)+'°':''}</text>`;
    out += `</g>`;
  }
  out += `</g>`;
  svg.innerHTML = out;
}

function renderInfo() {
  if (!currentView) return;
  $('welcome').classList.add('hidden'); $('workspace').classList.remove('hidden');
  const b = currentView.board;
  $('summary').innerHTML = `<div><strong>${esc(currentView.name)}</strong></div><div>Board: ${b.width_mm} × ${b.height_mm} mm</div><div>Komponen: ${currentView.components.length}</div><div>Net: ${currentView.nets.length}</div><div>Jalur: ${currentView.traces.length} · Via: ${currentView.vias.length}</div><div class="muted" style="margin-top:6px">${currentView.can_edit?'Mode edit: klik-taruh, drag-geser, tengah=pan, kanan/Esc=batal.':'Mode lihat/check.'}</div>`;
  $('source').value = currentView.source || '';
  const errs = currentView.diagnostics||[];
  $('diag-text').textContent = errs.length ? `${errs.length} diagnostik` : 'Bersih';
  $('diag-text').className = currentView.has_errors ? 'err' : (errs.length ? '' : 'ok');
  $('diagnostics').textContent = errs.length ? errs.map(d=>`[${d.severity}] ${d.code}: ${d.message}`).join('\n') : 'Bersih.';
  $('diagnostics').className = currentView.has_errors ? 'err' : 'ok';
  $('lists').innerHTML = `<strong>Nets</strong><div class="muted">${(currentView.nets||[]).map(n=>esc(n.name)).join(', ')||'-'}</div><br/><strong>Komponen</strong><div class="muted">${(currentView.components||[]).map(c=>`${esc(c.designator)} (${esc(c.part)})${c.placed?'':' [belum ditempatkan]'}`).join('<br/>')||'-'}</div>`;
}

function selectedComponent() {
  if (!selectedDesignator || !currentView) return null;
  return (currentView.components||[]).find(c => c.designator === selectedDesignator && c.placed) || null;
}
function setSelected(designator) {
  selectedDesignator = designator || null;
  render();
  const c = selectedComponent();
  if (c) {
    setStatus(`Dipilih: ${c.designator} (${c.part}) @ ${c.x.toFixed(2)},${c.y.toFixed(2)} mm ${Math.round(c.rotation_deg||0)}° (M geser, R putar, F flip, Del hapus)`);
  }
  if (!activePart) setActivePart(null); else syncToolButtons();
}
function zoomBy(factor, cx, cy) {
  if (!currentView) return;
  const svg = $('board-canvas'), r = svg.getBoundingClientRect();
  const mx = cx ?? r.width/2, my = cy ?? r.height/2;
  pan.x = mx-(mx-pan.x)*factor; pan.y = my-(my-pan.y)*factor;
  zoom = Math.max(0.15, Math.min(120, zoom*factor));
  render();
}
function syncHistoryButtons() {
  if ($('undo-btn')) $('undo-btn').disabled = !currentView?.can_undo;
  if ($('redo-btn')) $('redo-btn').disabled = !currentView?.can_redo;
}
async function applyView(view, keepViewport = true) {
  const had = !!currentView;
  currentView = view;
  if (selectedDesignator && !(currentView.components||[]).some(c => c.designator === selectedDesignator)) selectedDesignator = null;
  renderInfo(); syncHistoryButtons();
  if (!had || !keepViewport) fitView();
  render();
}
async function call(cmd, args={}) {
  try { setStatus('Memproses...'); const r = await invoke(cmd, args); setStatus('Siap','ok'); return r; }
  catch(e){ setStatus(String(e), 'err'); $('diag-text').textContent = String(e); $('diag-text').className='err'; throw e; }
}

function syncToolButtons() {
  const setA = (id, on) => { const el=$(id); if(el) el.classList.toggle('active', !!on); };
  setA('tool-select', !activePart && !(moving && moving.keyboard));
  setA('tool-add', !!activePart);
  setA('tool-move', !!(moving && moving.keyboard));
}
function setActivePart(part) {
  activePart = part;
  document.querySelectorAll('.part').forEach(el => el.classList.toggle('active', !!part && el.dataset.part===part));
  $('tool-info').innerHTML = part
    ? `Alat: <strong>Taruh ${esc(part)}</strong> — klik di papan. (Kanan/Esc batal)`
    : (selectedDesignator ? `Alat: <strong>Pilih</strong> — ${esc(selectedDesignator)} dipilih. Gunakan tombol Geser/Putar/Flip/Hapus atau pintasan.` : 'Alat: <strong>Pilih</strong> — klik komponen di papan untuk memilih.');
  $('drop-hint').classList.toggle('hidden', !part);
  $('board-canvas').style.cursor = part ? 'copy' : 'crosshair';
  syncToolButtons();
}

function findComponentAt(pt) {
  if (!pt || !currentView) return null;
  let best=null, bestD=Math.max(2.5, 8/zoom);
  for (const c of (currentView.components||[])) {
    if (!c.placed) continue;
    const d = Math.hypot(c.x-pt.x, c.y-pt.y);
    if (d < bestD) { bestD=d; best=c; }
  }
  return best;
}

async function placePartAt(part, pt) {
  if (!currentView?.can_edit) { setStatus('Buka/Buat proyek dulu (mode edit) untuk menaruh komponen.', 'err'); return; }
  const p = clampToBoard(pt);
  try { applyView(await call('add_component_at', { part, xMm: p.x, yMm: p.y })); setStatus(`${part} ditambahkan.`, 'ok'); }
  catch(_) {}
}

function scheduleRender() { if (raf) return; raf = requestAnimationFrame(()=>{ raf=0; render(); }); }

function isTypingTarget(el) {
  if (!el) return false;
  const tag = (el.tagName||'').toLowerCase();
  return tag === 'input' || tag === 'textarea' || el.isContentEditable;
}
function toggleHelp(force) {
  showHelp = typeof force === 'boolean' ? force : !showHelp;
  $('help-overlay').classList.toggle('hidden', !showHelp);
}
function activateAddMode() {
  setActivePart(null);
  setStatus('Mode Tambah (A): pilih jenis komponen di bawah, lalu klik papan untuk menaruh. (Kanan/Esc batal)');
  $('palette')?.classList?.add('pulse'); setTimeout(()=>$('palette')?.classList?.remove('pulse'), 700);
  syncToolButtons();
}
function activateSelectMode() {
  if (moving && currentView) { moving.component.x=moving.ox; moving.component.y=moving.oy; moving=null; pendingPt=null; render(); }
  setActivePart(null);
  setStatus('Mode Pilih: klik komponen di papan untuk memilih, lalu gunakan alat (Geser/Putar/Flip/Hapus).');
  syncToolButtons();
}
async function doRotate(deltaDeg) {
  const c = selectedComponent();
  if (!c || !currentView?.can_edit) { setStatus('Pilih komponen dulu (klik komponen), lalu tekan R.', 'err'); return; }
  const rot = ((Math.round(c.rotation_deg||0) + deltaDeg) % 360 + 360) % 360;
  try { applyView(await call('rotate_component', { designator: c.designator, rotationDeg: rot })); setSelected(c.designator); setStatus(`${c.designator} diputar ke ${rot}°`, 'ok'); } catch(_) {}
}
async function doDelete() {
  const c = selectedComponent();
  if (!c || !currentView?.can_edit) { setStatus('Pilih komponen dulu, lalu tekan Del.', 'err'); return; }
  const d = c.designator;
  try { selectedDesignator = null; applyView(await call('delete_component', { designator: d })); setStatus(`${d} dihapus.`, 'ok'); } catch(_) {}
}
async function doFlip() {
  const c = selectedComponent();
  if (!c || !currentView?.can_edit) { setStatus('Pilih komponen dulu, lalu tekan F (flip sisi).', 'err'); return; }
  try { applyView(await call('flip_component', { designator: c.designator })); setSelected(c.designator); setStatus(`${c.designator} di-flip sisi.`, 'ok'); } catch(_) {}
}
function startMoveSelected() {
  const c = selectedComponent();
  if (!c || !currentView?.can_edit) { setStatus('Pilih komponen dulu (klik di papan), lalu klik Geser atau tekan M.', 'err'); return; }
  moving = { designator: c.designator, component: c, ox: c.x, oy: c.y, orot: c.rotation_deg||0, sx: 0, sy: 0, dragging: true, keyboard: true };
  $('board-canvas').style.cursor='grabbing';
  setStatus(`Geser ${c.designator}: gerak mouse, klik kiri tempat, R putar, kanan/Esc batal.`);
  syncToolButtons(); render();
}
async function nudgeSelected(dx, dy) {
  const c = selectedComponent();
  if (!c || !currentView?.can_edit) return;
  const step = snapOn() ? 1 : 0.1;
  const x = Math.max(0, Math.min(currentView.board.width_mm, snap(c.x + dx*step)));
  const y = Math.max(0, Math.min(currentView.board.height_mm, snap(c.y + dy*step)));
  try { applyView(await call('move_component_to', { designator: c.designator, xMm: x, yMm: y, rotationDeg: c.rotation_deg||0 })); setSelected(c.designator); } catch(_) {}
}
async function doUndo(){ if(!currentView?.can_edit) return; try{ applyView(await call('undo',{})); setStatus('Undo.','ok'); }catch(e){} }
async function doRedo(){ if(!currentView?.can_edit) return; try{ applyView(await call('redo',{})); setStatus('Redo.','ok'); }catch(e){} }
async function doSave(){
  if(!currentView?.can_edit){ setStatus('Tidak ada proyek edit yang bisa disimpan.','err'); return; }
  let p = currentPath || '';
  if (p && !p.toLowerCase().endsWith('.pn') && !p.toLowerCase().endsWith('.papan') && !p.toLowerCase().endsWith('.graph')) p = p.replace(/[\\\/]+$/,'') + '\\board.pn';
  if (!p || !p.toLowerCase().endsWith('.pn')) p = 'C:\\Users\\PC\\Documents\\papan\\board.pn';
  const chosen = window.prompt('Simpan source PAPAN ke path (.pn):', p);
  if (!chosen) return;
  try { await call('save_source', { path: chosen }); currentPath = chosen; $('project-path').value = chosen; setStatus(`Tersimpan: ${chosen}`,'ok'); } catch(e){}
}

function setupCanvas() {
  const svg = $('board-canvas'), zone = $('drop-zone');
  zone.addEventListener('contextmenu', e => e.preventDefault());
  svg.addEventListener('contextmenu', e => e.preventDefault());
  svg.addEventListener('auxclick', e => { if (e.button === 1) e.preventDefault(); });

  svg.addEventListener('wheel', e => {
    if (!currentView) return; e.preventDefault();
    const r=svg.getBoundingClientRect(), mx=e.clientX-r.left, my=e.clientY-r.top;
    const f = e.deltaY < 0 ? 1.12 : 1/1.12;
    pan.x = mx-(mx-pan.x)*f; pan.y = my-(my-pan.y)*f; zoom = Math.max(0.15, Math.min(120, zoom*f));
    render(); setCoords(clampToBoard(boardPoint(e.clientX,e.clientY)));
  }, {passive:false});

  // KiCad-like: Left=select/place/move, Middle=pan, Right=cancel
  svg.addEventListener('mousedown', e => {
    if (!currentView) return;
    const bp = boardPoint(e.clientX, e.clientY);
    if (e.button === 1) { e.preventDefault(); panning = { x: e.clientX-pan.x, y: e.clientY-pan.y, btn:1 }; svg.style.cursor='grabbing'; return; }
    if (e.button === 2) {
      e.preventDefault();
      if (activePart) { setActivePart(null); setStatus('Batal taruh komponen.'); }
      else if (moving) { moving.component.x=moving.ox; moving.component.y=moving.oy; moving=null; pendingPt=null; render(); setStatus('Batal geser.'); svg.style.cursor=activePart?'copy':'crosshair'; }
      return;
    }
    if (e.button !== 0) return;
    // keyboard move mode (M/G): left click commits placement
    if (moving && moving.keyboard) { return; }
    if (activePart) { placePartAt(activePart, bp); return; }
    const c = currentView.can_edit ? findComponentAt(bp) : null;
    if (c) {
      setSelected(c.designator);
      moving = { designator: c.designator, component: c, ox: c.x, oy: c.y, orot: c.rotation_deg||0, sx: e.clientX, sy: e.clientY, dragging: false, keyboard: false };
      svg.style.cursor='grab';
    } else {
      setSelected(null);
    }
  });

  window.addEventListener('mousemove', e => {
    const bp = boardPoint(e.clientX, e.clientY);
    const cp = bp ? clampToBoard(bp) : null;
    setCoords(cp);
    const prev = mouseBoardPt; mouseBoardPt = cp;
    if (moving) {
      if (!moving.dragging && !moving.keyboard) {
        if (Math.hypot(e.clientX - moving.sx, e.clientY - moving.sy) > 4) { moving.dragging = true; svg.style.cursor='grabbing'; }
        else return;
      }
      if (cp) { moving.component.x = cp.x; moving.component.y = cp.y; pendingPt = cp; scheduleRender(); }
      return;
    }
    if (panning) { pan.x = e.clientX-panning.x; pan.y = e.clientY-panning.y; render(); return; }
    if (activePart && cp && (!prev || prev.x!==cp.x || prev.y!==cp.y)) scheduleRender();
  });

  window.addEventListener('mouseup', async e => {
    if (panning && e.button === panning.btn) { panning=null; svg.style.cursor = activePart?'copy':'crosshair'; return; }
    if (moving && e.button === 0) {
      const d = moving.designator, comp = moving.component, ox=moving.ox, oy=moving.oy, orot=moving.orot;
      const didDrag = moving.dragging || moving.keyboard;
      const p = pendingPt || clampToBoard(boardPoint(e.clientX,e.clientY));
      const rot = (comp.rotation_deg ?? orot ?? 0);
      moving=null; pendingPt=null; svg.style.cursor = activePart?'copy':'crosshair'; syncToolButtons();
      if (!didDrag) { setSelected(d); render(); setStatus(`${d} dipilih. (M/G=geser, R=rotasi, F=flip, Del=hapus, panah=nudge)`); return; }
      if (p) {
        try { applyView(await call('move_component_to', { designator: d, xMm: p.x, yMm: p.y, rotationDeg: rot })); setSelected(d); setStatus(`${d} → (${p.x.toFixed(1)}, ${p.y.toFixed(1)}) mm ${Math.round(rot)}°`, 'ok'); }
        catch(_) { comp.x=ox; comp.y=oy; comp.rotation_deg=orot; render(); }
      } else { comp.x=ox; comp.y=oy; comp.rotation_deg=orot; setSelected(d); render(); }
    }
  });

  window.addEventListener('keydown', async e => {
    if (showHelp && (e.key==='Escape' || e.key==='?' || e.key==='F1')) { e.preventDefault(); toggleHelp(false); return; }
    // Ctrl combos (project-level)
    if (e.ctrlKey && !e.shiftKey && (e.key==='z' || e.key==='Z')) { e.preventDefault(); if (!isTypingTarget(e.target)) await doUndo(); return; }
    if ((e.ctrlKey && (e.key==='y' || e.key==='Y')) || (e.ctrlKey && e.shiftKey && (e.key==='z' || e.key==='Z'))) { e.preventDefault(); if (!isTypingTarget(e.target)) await doRedo(); return; }
    if (e.ctrlKey && (e.key==='s' || e.key==='S')) { e.preventDefault(); await doSave(); return; }
    if (isTypingTarget(e.target)) return;
    const k = e.key;
    if (k==='?' || k==='F1') { e.preventDefault(); toggleHelp(); return; }
    if (k==='Escape') {
      if (moving && currentView) { moving.component.x=moving.ox; moving.component.y=moving.oy; moving.component.rotation_deg=moving.orot; moving=null; pendingPt=null; render(); }
      setActivePart(null); syncToolButtons(); setStatus('Batal.'); return;
    }
    if (!currentView) return;
    if (k==='Home') { e.preventDefault(); fitView(); render(); return; }
    if (k==='+' || k==='=') { e.preventDefault(); zoomBy(1.15); return; }
    if (k==='-' || k==='_') { e.preventDefault(); zoomBy(1/1.15); return; }
    if (k==='a' || k==='A') { e.preventDefault(); activateAddMode(); return; }
    if (k==='m' || k==='M' || k==='g' || k==='G') { e.preventDefault(); startMoveSelected(); return; }
    if (k==='r' || k==='R') {
      e.preventDefault();
      if (moving) { moving.component.rotation_deg = ((Math.round(moving.component.rotation_deg||0) + (e.shiftKey?-90:90))%360+360)%360; scheduleRender(); setStatus(`Putar ${moving.designator} → ${moving.component.rotation_deg}° (klik untuk tempat)`); return; }
      await doRotate(e.shiftKey ? -90 : 90); return;
    }
    if (k==='f' || k==='F') { e.preventDefault(); await doFlip(); return; }
    if (k==='Delete' || k==='Backspace') { e.preventDefault(); await doDelete(); return; }
    if (k==='ArrowLeft') { e.preventDefault(); await nudgeSelected(e.shiftKey?-0.1:-1, 0); return; }
    if (k==='ArrowRight') { e.preventDefault(); await nudgeSelected(e.shiftKey?0.1:1, 0); return; }
    if (k==='ArrowUp') { e.preventDefault(); await nudgeSelected(0, e.shiftKey?0.1:1); return; }
    if (k==='ArrowDown') { e.preventDefault(); await nudgeSelected(0, e.shiftKey?-0.1:-1); return; }
  });

  // fallback HTML5 drag from palette (optional)
  zone.addEventListener('dragover', e => { if (e.dataTransfer.types.includes('text/papan-part')) { e.preventDefault(); $('drop-hint').classList.remove('hidden'); } });
  zone.addEventListener('dragleave', e => { if (e.target===zone) $('drop-hint').classList.toggle('hidden', !activePart); });
  zone.addEventListener('drop', async e => { const part = e.dataTransfer.getData('text/papan-part'); $('drop-hint').classList.toggle('hidden', !activePart); if (part) { e.preventDefault(); placePartAt(part, boardPoint(e.clientX,e.clientY)); } });
}

function setupPalette() {
  document.querySelectorAll('.part[data-part]').forEach(el => {
    el.addEventListener('click', () => setActivePart(activePart===el.dataset.part ? null : el.dataset.part));
    el.setAttribute('draggable','true');
    el.addEventListener('dragstart', e => { e.dataTransfer.setData('text/papan-part', el.dataset.part); e.dataTransfer.effectAllowed='copy'; });
  });
}

function setupModal() {
  const modal = $('modal');
  $('new-btn').onclick = $('welcome-new').onclick = () => modal.classList.remove('hidden');
  $('new-cancel').onclick = () => modal.classList.add('hidden');
  $('new-ok').onclick = async () => { modal.classList.add('hidden'); try { applyView(await call('new_project', { name: $('new-name').value||'Papan Baru', widthMm: +$('new-w').value||80, heightMm: +$('new-h').value||50, layers: Math.max(1, +$('new-l').value||2) })); currentPath=null; setActivePart(null);} catch(_) {} };
}

async function init() {
  if (!invoke) { setStatus('Tauri API tidak tersedia.', 'err'); return; }
  setupPalette(); setupCanvas(); setupModal();
  $('open-btn').onclick = async () => { const p=$('project-path').value.trim(); if(p) try{ applyView(await call('open_project',{path:p})); currentPath=p; setActivePart(null);}catch(_){} };
  $('welcome-open').onclick = async () => { const p=$('welcome-path').value.trim(); if(p){ $('project-path').value=p; try{ applyView(await call('open_project',{path:p})); currentPath=p; setActivePart(null);}catch(_){} } };
  $('save-btn') && ($('save-btn').onclick = () => doSave());
  $('undo-btn') && ($('undo-btn').onclick = () => doUndo());
  $('redo-btn') && ($('redo-btn').onclick = () => doRedo());
  $('ratsnest') && ($('ratsnest').onchange = () => render());
  $('check-btn').onclick = async () => { try{ applyView(await call('check_source',{source:$('source').value})); }catch(_){} };
  $('fit-btn').onclick = () => { if(currentView){ fitView(); render(); } };
  $('help-btn').onclick = () => toggleHelp();
  $('help-close').onclick = () => toggleHelp(false);
  $('tool-select') && ($('tool-select').onclick = () => activateSelectMode());
  $('tool-add') && ($('tool-add').onclick = () => activateAddMode());
  $('tool-move') && ($('tool-move').onclick = () => startMoveSelected());
  $('tool-rotate') && ($('tool-rotate').onclick = () => doRotate(90));
  $('tool-flip') && ($('tool-flip').onclick = () => doFlip());
  $('tool-delete') && ($('tool-delete').onclick = () => doDelete());
  $('tool-fit') && ($('tool-fit').onclick = () => { if(currentView){ fitView(); render(); } });
  $('tool-help') && ($('tool-help').onclick = () => toggleHelp());
  $('welcome-path').value = String.raw`C:\Users\PC\Documents\papan\examples\ujicoba`;
  setActivePart(null); syncHistoryButtons();
  setStatus('Siap. Klik tombol Alat atau pintasan (?). Ghost+ratusnest aktif, Ctrl+Z/Y undo/redo.', 'ok');
}
init();
