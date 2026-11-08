const invoke = window.__TAURI__?.core?.invoke;

function showDiagnostics(target, diagnostics, hasErrors) {
  if (!diagnostics || diagnostics.length === 0) {
    target.textContent = hasErrors ? "Errors reported (no details)." : "No diagnostics.";
    target.className = hasErrors ? "error" : "ok";
    return;
  }
  target.className = hasErrors ? "error" : "ok";
  target.textContent = diagnostics
    .map((d) => `[${d.severity}] ${d.code}: ${d.message}`)
    .join("\n");
}

async function init() {
  const info = document.getElementById("app-info");
  const pathInput = document.getElementById("project-path");
  const source = document.getElementById("source");
  const diagnostics = document.getElementById("diagnostics");
  const summary = document.getElementById("project-summary");
  if (!invoke) {
    info.textContent = "Tauri API not available (run with cargo tauri dev/build).";
    return;
  }
  info.textContent = await invoke("app_info");

  document.getElementById("open-btn").addEventListener("click", async () => {
    const path = pathInput.value.trim();
    if (!path) return;
    summary.textContent = "Loading...";
    try {
      const result = await invoke("open_project", { path });
      source.value = result.source || "";
      summary.textContent = `${result.name} — components: ${result.component_count}, nets: ${result.net_count}, traces: ${result.trace_count}, vias: ${result.via_count}`;
      showDiagnostics(diagnostics, result.diagnostics, result.has_errors);
    } catch (e) {
      summary.textContent = "";
      diagnostics.className = "error";
      diagnostics.textContent = String(e);
    }
  });

  document.getElementById("check-btn").addEventListener("click", async () => {
    try {
      const result = await invoke("check_source", { source: source.value });
      summary.textContent = `Check — components: ${result.component_count}, nets: ${result.net_count}`;
      showDiagnostics(diagnostics, result.diagnostics, result.has_errors);
    } catch (e) {
      diagnostics.className = "error";
      diagnostics.textContent = String(e);
    }
  });
}

init();
