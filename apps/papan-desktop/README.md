# PAPAN Desktop (Tauri MVP)

Aplikasi desktop Tauri untuk coding dan pemeriksaan proyek PAPAN, menggunakan engine PAPAN yang ada (`papan-api` / `papan-document`) tanpa mengubah perilaku inti.

## Menjalankan (development)

Dari root workspace:

```powershell
cd C:\Users\PC\Documents\papan
cargo tauri dev --manifest-path apps/papan-desktop/src-tauri/Cargo.toml
```

Atau dari folder `src-tauri`:

```powershell
cd C:\Users\PC\Documents\papan\apps\papan-desktop\src-tauri
cargo tauri dev
```

Prasyarat: Rust toolchain, Node/npm (untuk tooling Tauri bila diperlukan), WebView2 di Windows.

## Fitur MVP saat ini

- Buka/check project/file PAPAN lewat path (`PapanProject::open`).
- Check source `.pn` dari editor teks (`PapanDocument::from_source`).
- Tampilkan diagnostik, jumlah komponen/net/traces/vias.

## Arah pengembangan berikutnya

- Dialog pilih file/folder (Tauri dialog plugin).
- Kanvas PCB 2D (render geometri/trace/via/pad dari IR/engine).
- Editor node graph (`papan-graph`) + sinkronisasi transaksi.
- Autoroute/DRC/ERC/export manufaktur via command Tauri.
