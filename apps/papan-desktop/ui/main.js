const invoke = window.__TAURI__?.core?.invoke;
let currentView = null;
let pan = { x: 40, y: 40 }, zoom = 4, drag = null;

const layerColors = { 'f.cu': '#c84b4b', 'b.cu': '#4b7fc8', 'in1.cu': '#c8a84b', 'in2.cu': '#4bc88a' };
function layerColor(name, copper) {
  if (layerColors[name.toLowerCase()]) return layerColors[name.toLowerCase()];
  return copper ? '#c87a4b' : '#8a8f98';
}
function esc(s) { return String(s).replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c])); }

function setDiagnostics(diags, hasErrors) {
  const el = document.getElementById('diagnostics');
  const sum = document.getElementById('diag-summary');
  sum.textContent = diags?.length ? `(${diags.length})` : '(clean)';
  sum.className = hasErrors ? 'err' : 'ok';
  if (!diags?.length) { el.textContent = 'No diagnostics.'; el.className = 'ok'; return; }
  el.className = hasErrors ? 'err' : 'ok';
  el.textContent = diags.map(d => `[${d.severity}] ${d.code}: ${d.message}`).join('\n');
}

function renderSidebar(view) {
  document.getElementById('board-info').innerHTML =
    `<div><strong>${esc(view.name)}</strong></div>` +
    `<div>${view.board.width_mm} × ${view.board.height_mm} mm</div>` +
    `<div>Components: ${view.components.length} | Nets: ${view.nets.length}</div>` +
    `<div>Traces: ${view.traces.length} | Vias: ${view.vias.length}</div>`;
  document.getElementById('layers').innerHTML = view.layers.map(l =>
    `<div class="item"><span class="swatch" style="background:${layerColor(l.name,l.copper)}"></span>${esc(l.name)} <span class="muted">(${l.kind})</span></div>`
  ).join('') || '<div class="muted">No layers</div>';
  document.getElementById('nets').innerHTML = view.nets.map(n =>
    `<div class="item">${esc(n.name)} <span class="muted">${n.kind} · ${n.member_count}</span></div>`
  ).join('') || '<div class="muted">No nets</div>';
  document.getElementById('components').innerHTML = view.components.map(c =>
    `<div class="item">${esc(c.designator)} <span class="muted">${esc(c.part)}${c.placed ? '' : ' · unplaced'}</span></div>`
  ).join('') || '<div class="muted">No components</div>';
}

function renderBoard(view) {
  const svg = document.getElementById('board-canvas');
  const hint = document.getElementById('canvas-hint');
  hint.style.display = 'none';
  const b = view.board;
  const layerById = Object.fromEntries(view.layers.map(l => [l.id, l]));
  const g = [];
  g.push(`<g transform="translate(${pan.x},${pan.y}) scale(${zoom},${-zoom}) translate(0,${-b.height_mm})">`);
  g.push(`<rect x="0" y="0" width="${b.width_mm}" height="${b.height_mm}" fill="#1a2a1a" stroke="var(--edge)" stroke-width="${1/zoom}" />`);
  // traces
  for (const t of view.traces) {
    if (!t.points || t.points.length < 2) continue;
    const l = layerById[t.layer];
    const color = l ? layerColor(l.name, l.copper) : '#59a869';
    const pts = t.points.map(p => `${p.x},${p.y}`).join(' ');
    g.push(`<polyline points="${pts}" fill="none" stroke="${color}" stroke-width="${Math.max(t.width_mm, 0.15)}" stroke-linecap="round" stroke-linejoin="round" opacity="0.9" />`);
  }
  // vias
  for (const v of view.vias) {
    const r = Math.max(v.diameter_mm / 2, 0.2);
    g.push(`<circle cx="${v.x}" cy="${v.y}" r="${r}" fill="#888" stroke="#ddd" stroke-width="${0.05}" />`);
    g.push(`<circle cx="${v.x}" cy="${v.y}" r="${Math.max(v.drill_mm/2,0.1)}" fill="#141517" />`);
  }
  // components (simple markers; pad rendering can be added)
  for (const c of view.components) {
    if (!c.placed) continue;
    g.push(`<g transform="translate(${c.x},${c.y}) rotate(${c.rotation_deg})">`);
    // pads (local footprint coords, simplified)
    for (const p of c.pads || []) {
      if (p.shape === 'circle') g.push(`<circle cx="${p.x}" cy="${p.y}" r="${Math.max(p.w,p.h)/2}" fill="#b87333" opacity="0.7" />`);
      else g.push(`<rect x="${p.x-p.w/2}" y="${p.y-p.h/2}" width="${p.w}" height="${p.h}" fill="#b87333" opacity="0.7" />`);
    }
    g.push(`<circle cx="0" cy="0" r="0.4" fill="#fff" opacity="0.8" />`);
    g.push(`<text x="0.6" y="0.4" font-size="1.2" fill="#fff" font-family="Consolas, monospace" transform="scale(1,-1)">${esc(c.designator)}</text>`);
    g.push(`</g>`);
  }
  g.push(`</g>`);
  svg.innerHTML = g.join('');
}

function fitView(view) {
  const svg = document.getElementById('board-canvas');
  const r = svg.getBoundingClientRect();
  const b = view.board;
  if (!r.width || !r.height || !b.width_mm || !b.height_mm) return;
  const pad = 30;
  zoom = Math.min((r.width - pad*2) / b.width_mm, (r.height - pad*2) / b.height_mm);
  zoom = Math.max(0.1, Math.min(zoom, 50));
  pan.x = (r.width - b.width_mm * zoom) / 2;
  pan.y = (r.height + b.height_mm * zoom) / 2; // accounts for Y flip in group
}

function applyView(view) {
  currentView = view;
  document.getElementById('source').value = view.source || '';
  setDiagnostics(view.diagnostics, view.has_errors);
  renderSidebar(view);
  fitView(view);
  renderBoard(view);
}

function setupCanvas() {
  const svg = document.getElementById('board-canvas');
  svg.addEventListener('wheel', e => {
    if (!currentView) return;
    e.preventDefault();
    const r = svg.getBoundingClientRect();
    const mx = e.clientX - r.left, my = e.clientY - r.top;
    const f = e.deltaY < 0 ? 1.15 : 1/1.15;
    pan.x = mx - (mx - pan.x) * f;
    pan.y = my - (my - pan.y) * f;
    zoom *= f;
    renderBoard(currentView);
  }, { passive: false });
  svg.addEventListener('mousedown', e => { drag = { x: e.clientX - pan.x, y: e.clientY - pan.y }; });
  window.addEventListener('mousemove', e => { if (!drag || !currentView) return; pan.x = e.clientX - drag.x; pan.y = e.clientY - drag.y; renderBoard(currentView); });
  window.addEventListener('mouseup', () => { drag = null; });
}

async function init() {
  const info = document.getElementById('app-info');
  if (!invoke) { info.textContent = 'Tauri API unavailable.'; return; }
  try { info.textContent = await invoke('app_info'); } catch { info.textContent = 'PAPAN Desktop'; }
  setupCanvas();
  document.getElementById('open-btn').addEventListener('click', async () => {
    const path = document.getElementById('project-path').value.trim(); if (!path) return;
    try { applyView(await invoke('open_project', { path })); } catch (e) { setDiagnostics([{severity:'error',code:'UI',message:String(e)}], true); }
  });
  document.getElementById('check-btn').addEventListener('click', async () => {
    try { applyView(await invoke('check_source', { source: document.getElementById('source').value })); } catch (e) { setDiagnostics([{severity:'error',code:'UI',message:String(e)}], true); }
  });
  document.getElementById('fit-btn').addEventListener('click', () => { if (currentView) { fitView(currentView); renderBoard(currentView); } });
}
init();
