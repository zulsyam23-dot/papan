const invoke = window.__TAURI__?.core?.invoke;
let currentView = null;
let pan = { x: 30, y: 30 }, zoom = 5;
let panning = null, moving = null;

const $ = id => document.getElementById(id);
const layerColors = { 'f.cu': '#e06b6b', 'b.cu': '#5b8def', 'in1.cu': '#d4b85a', 'in2.cu': '#5ad49a' };
function layerColor(name, copper) { return layerColors[(name||'').toLowerCase()] || (copper ? '#c98a5a' : '#8a949e'); }
function esc(s){ return String(s??'').replace(/[&<>"']/g, c=>({ '&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;' }[c])); }
function setStatus(msg, cls=''){ const s=$('status'); s.textContent=msg; s.className='status '+cls; }

function boardPoint(clientX, clientY) {
  const svg = $('board-canvas'), r = svg.getBoundingClientRect();
  const b = currentView?.board; if (!b) return null;
  const sx = clientX - r.left, sy = clientY - r.top;
  return { x: (sx - pan.x) / zoom, y: b.height_mm - (sy - pan.y) / zoom };
}

function fitView() {
  const svg = $('board-canvas'), r = svg.getBoundingClientRect(), b = currentView?.board;
  if (!r.width || !r.height || !b?.width_mm || !b?.height_mm) return;
  const pad = 40;
  zoom = Math.max(0.2, Math.min(60, Math.min((r.width-pad*2)/b.width_mm, (r.height-pad*2)/b.height_mm)));
  pan.x = (r.width - b.width_mm*zoom)/2;
  pan.y = (r.height + b.height_mm*zoom)/2;
}

function render() {
  if (!currentView) return;
  const svg = $('board-canvas'), b = currentView.board;
  const layerById = Object.fromEntries((currentView.layers||[]).map(l=>[l.id,l]));
  let out = `<rect width="100%" height="100%" fill="transparent"/>`;
  out += `<g transform="translate(${pan.x},${pan.y}) scale(${zoom},${-zoom}) translate(0,${-b.height_mm})">`;
  out += `<rect x="0" y="0" width="${b.width_mm}" height="${b.height_mm}" rx="0.5" fill="var(--board)" stroke="var(--edge)" stroke-width="${1.2/zoom}"/>`;
  for (const t of (currentView.traces||[])) {
    if (!t.points || t.points.length < 2) continue;
    const l = layerById[t.layer], color = l ? layerColor(l.name,l.copper) : '#59a869';
    out += `<polyline points="${t.points.map(p=>`${p.x},${p.y}`).join(' ')}" fill="none" stroke="${color}" stroke-width="${Math.max(t.width_mm,0.18)}" stroke-linecap="round" stroke-linejoin="round" opacity="0.95"/>`;
  }
  for (const v of (currentView.vias||[])) {
    out += `<circle cx="${v.x}" cy="${v.y}" r="${Math.max(v.diameter_mm/2,0.25)}" fill="#b8c0cc" stroke="#eef" stroke-width="${0.06/zoom}"/>`;
    out += `<circle cx="${v.x}" cy="${v.y}" r="${Math.max(v.drill_mm/2,0.12)}" fill="#0b1219"/>`;
  }
  for (const c of (currentView.components||[])) {
    if (!c.placed) continue;
    const hi = moving && moving.designator === c.designator;
    out += `<g class="comp" data-designator="${esc(c.designator)}" transform="translate(${c.x},${c.y}) rotate(${c.rotation_deg||0})" style="cursor:${currentView.can_edit?'move':'default'}">`;
    for (const p of (c.pads||[])) {
      if (p.shape === 'circle') out += `<circle cx="${p.x}" cy="${p.y}" r="${Math.max(p.w,p.h)/2}" fill="${hi?'#ffd166':'#cd7f32'}" opacity="0.85"/>`;
      else out += `<rect x="${p.x-p.w/2}" y="${p.y-p.h/2}" width="${p.w}" height="${p.h}" rx="0.08" fill="${hi?'#ffd166':'#cd7f32'}" opacity="0.85"/>`;
    }
    out += `<circle cx="0" cy="0" r="0.35" fill="#fff"/>`;
    out += `<text x="0.7" y="0.45" font-size="1.4" fill="#fff" font-weight="600" font-family="Segoe UI, sans-serif" transform="scale(1,-1)">${esc(c.designator)}</text>`;
    out += `<text x="0.7" y="-0.9" font-size="0.9" fill="#c9d6e2" font-family="Segoe UI, sans-serif" transform="scale(1,-1)">${esc(c.part)}</text>`;
    out += `</g>`;
  }
  out += `</g>`;
  svg.innerHTML = out;
}

function renderInfo() {
  if (!currentView) return;
  $('welcome').classList.add('hidden'); $('workspace').classList.remove('hidden');
  const b = currentView.board;
  $('summary').innerHTML = `<div><strong>${esc(currentView.name)}</strong></div><div>Ukuran: ${b.width_mm} × ${b.height_mm} mm</div><div>Komponen: ${currentView.components.length}</div><div>Net: ${currentView.nets.length}</div><div>Jalur: ${currentView.traces.length} · Via: ${currentView.vias.length}</div>`;
  $('source').value = currentView.source || '';
  const errs = currentView.diagnostics||[];
  $('diag-text').textContent = errs.length ? `${errs.length} diagnostik (${currentView.has_errors?'ada error':'peringatan/info'})` : 'Tidak ada masalah terdeteksi.';
  $('diag-text').className = currentView.has_errors ? 'err' : (errs.length ? 'muted' : 'ok');
  $('diagnostics').textContent = errs.length ? errs.map(d=>`[${d.severity}] ${d.code}: ${d.message}`).join('\n') : 'Bersih.';
  $('diagnostics').className = currentView.has_errors ? 'err' : 'ok';
  $('lists').innerHTML = `<strong>Nets</strong><div class="muted">${(currentView.nets||[]).map(n=>esc(n.name)).join(', ')||'-'}</div><br/><strong>Komponen</strong><div class="muted">${(currentView.components||[]).map(c=>`${esc(c.designator)} (${esc(c.part)})${c.placed?'':' [belum ditempatkan]'}`).join('<br/>')||'-'}</div>`;
}

async function applyView(view) { currentView = view; renderInfo(); fitView(); render(); setStatus('Siap', 'ok'); }
async function call(cmd, args={}) {
  try { setStatus('Memproses...'); return await invoke(cmd, args); }
  catch(e){ setStatus(String(e), 'err'); $('diag-text').textContent = String(e); $('diag-text').className='err'; throw e; }
}

function findComponentAt(pt) {
  if (!pt || !currentView) return null;
  let best=null, bestD=3.0; // mm hit radius
  for (const c of (currentView.components||[])) {
    if (!c.placed) continue;
    const d = Math.hypot(c.x-pt.x, c.y-pt.y);
    if (d < bestD) { bestD=d; best=c; }
  }
  return best;
}

function setupDragDrop() {
  document.querySelectorAll('.part[draggable]').forEach(el => {
    el.addEventListener('dragstart', e => { e.dataTransfer.setData('text/papan-part', el.dataset.part); e.dataTransfer.effectAllowed='copy'; });
  });
  const zone = $('drop-zone'), hint = $('drop-hint');
  zone.addEventListener('dragover', e => { if (e.dataTransfer.types.includes('text/papan-part')) { e.preventDefault(); hint.classList.remove('hidden'); } });
  zone.addEventListener('dragleave', e => { if (e.target===zone) hint.classList.add('hidden'); });
  zone.addEventListener('drop', async e => {
    const part = e.dataTransfer.getData('text/papan-part');
    hint.classList.add('hidden');
    if (!part || !currentView) return;
    e.preventDefault();
    if (!currentView.can_edit) { setStatus('Buka proyek (bukan mode check sementara) untuk menambah komponen.', 'err'); return; }
    const bp = boardPoint(e.clientX, e.clientY);
    const x = Math.max(0, Math.min(currentView.board.width_mm, bp?.x ?? 10));
    const y = Math.max(0, Math.min(currentView.board.height_mm, bp?.y ?? 10));
    try { applyView(await call('add_component_at', { part, xMm: x, yMm: y })); setStatus(`Komponen ${part} ditambahkan.`, 'ok'); }
    catch(_) {}
  });
}

function setupCanvas() {
  const svg = $('board-canvas');
  svg.addEventListener('wheel', e => {
    if (!currentView) return; e.preventDefault();
    const r=svg.getBoundingClientRect(), mx=e.clientX-r.left, my=e.clientY-r.top, f=e.deltaY<0?1.15:1/1.15;
    pan.x = mx-(mx-pan.x)*f; pan.y = my-(my-pan.y)*f; zoom *= f; render();
  }, {passive:false});
  svg.addEventListener('mousedown', e => {
    if (!currentView || e.button!==0) return;
    const bp = boardPoint(e.clientX, e.clientY);
    const c = currentView.can_edit ? findComponentAt(bp) : null;
    if (c) { moving = { designator: c.designator }; render(); setStatus(`Geser ${c.designator} lalu lepas...`); }
    else { panning = { x: e.clientX-pan.x, y: e.clientY-pan.y }; }
  });
  window.addEventListener('mousemove', e => {
    if (moving && currentView) { /* visual follows cursor on commit; keep simple */ return; }
    if (panning) { pan.x = e.clientX-panning.x; pan.y = e.clientY-panning.y; render(); }
  });
  window.addEventListener('mouseup', async e => {
    if (moving && currentView) {
      const d = moving.designator; moving = null;
      const bp = boardPoint(e.clientX, e.clientY);
      if (bp) {
        const x = Math.max(0, Math.min(currentView.board.width_mm, bp.x));
        const y = Math.max(0, Math.min(currentView.board.height_mm, bp.y));
        try { applyView(await call('move_component_to', { designator: d, xMm: x, yMm: y })); setStatus(`${d} dipindahkan.`, 'ok'); }
        catch(_){ render(); }
      } else render();
    }
    panning = null;
  });
}

function setupModal() {
  const modal = $('modal');
  $('new-btn').onclick = $('welcome-new').onclick = () => modal.classList.remove('hidden');
  $('new-cancel').onclick = () => modal.classList.add('hidden');
  $('new-ok').onclick = async () => {
    modal.classList.add('hidden');
    try { applyView(await call('new_project', { name: $('new-name').value||'Papan Baru', widthMm: +$('new-w').value||80, heightMm: +$('new-h').value||50, layers: Math.max(1, +$('new-l').value||2) })); }
    catch(_) {}
  };
}

async function init() {
  if (!invoke) { setStatus('Tauri API tidak tersedia.', 'err'); return; }
  try { await invoke('app_info'); } catch {}
  setupDragDrop(); setupCanvas(); setupModal();
  $('open-btn').onclick = async () => { const p=$('project-path').value.trim(); if(p) try{ applyView(await call('open_project',{path:p})); }catch(_){} };
  $('welcome-open').onclick = async () => { const p=$('welcome-path').value.trim(); if(p){ $('project-path').value=p; try{ applyView(await call('open_project',{path:p})); }catch(_){} } };
  $('check-btn').onclick = async () => { try{ applyView(await call('check_source',{source:$('source').value})); }catch(_){} };
  $('fit-btn').onclick = () => { if(currentView){ fitView(); render(); } };
  $('welcome-path').value = String.raw`C:\Users\PC\Documents\papan\examples\ujicoba`;
  setStatus('Siap. Buat proyek baru atau buka proyek.');
}
init();
