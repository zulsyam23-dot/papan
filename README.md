# PAPAN

PAPAN is a deterministic, Rust-based PCB design toolchain. Text (`.pn`) and node graph (`.graph`) representations share one semantic model, compile to a stable PCB Intermediate Representation (PIR), and can be validated/exported to manufacturing outputs.

## Status

This repository implements the core PRD architecture:

- multi-crate workspace with stable shared types
- lexer/parser/AST/semantic lowering
- PIR and IR verifier
- fixed-point geometry and placement
- electrical connectivity/ERC primitives
- deterministic Manhattan/L-shaped autorouter
- DRC and manufacturing validation
- Gerber, Excellon drill, BOM, and pick-and-place export
- typed node graph projection/import and JSON serialization
- document model with transactions and undo/redo
- public Rust API and CLI
- KiCad `.kicad_pcb` adapter (conservative subset, internal parser)

## Kekurangan & Keterbatasan (Current Limitations)

Berikut kekurangan/keterbatasan PAPAN saat ini (sebagian disengaja / di luar cakupan fase implementasi ini).

### Bahasa & Semantik (`.pn` / `.graph` / `.papan`)
- **Subset bahasa (bukan cakupan PRD penuh)**: beberapa fitur PRD masih konservatif/terbatas atau belum didukung penuh (mis. simulasi, semantik port modul hierarkis penuh, taksonomi constraint lengkap).
- **Tidak ada model skematik/simbol penuh**: belum ada lembar/simbol skematik, bus/label skematik, atau alur Eeschema↔PCB seperti EDA mapan.
- **Konektivitas source terbatas**: `connect` berbentuk `Component.Pin -> Net` (harus ke net yang dideklarasikan), bukan koneksi pin-ke-pin bebas atau inferensi net implisit; penanganan pin tak terpakai/lanjutan terbatas.
- **Hierarki modul masih dasar**: `module` diturunkan dengan namespacing, tetapi semantik port/hierarki modul belum penuh.
- **Minim abstraksi/parametrisasi**: belum ada variabel/parameter, perulangan/generator, macro/templating, atau komputasi tata letak di bahasa (instansiasi/layout ditulis eksplisit).
- **Constraint tercatat, penegakan belum menyeluruh**: direktif `route`/`constraints` (mis. `differential_pair`, `length`, `impedance`) dapat diparse/diturunkan, tetapi tidak semuanya ditegakkan penuh oleh engine.

### Routing, Placement, DRC & ERC
- **Autorouter deterministik & dasar** (Manhattan/L-shaped): optimasi multilayer lanjut, **differential-pair routing, length matching/tuning**, dan kontrol impedansi lanjutan belum diimplementasikan penuh. Constraint high-speed pada `route` (`differential_pair`/`length`/`impedance`) **tidak ditegakkan penuh**, tetapi kini dilaporkan eksplisit (warning/diagnostik: `ROUTE_010/011/013`, `DRC_170/171/172`) agar tidak menyesatkan.
- **Tidak ada routing interaktif** (push-shove/walk-around/drag trace/via interaktif) di core engine.
- **DRC/ERC cakupannya terus diperkuat namun belum setara** rule set manufaktur/EDA penuh (aturan lanjutan/edge-case/DFM pabrikan spesifik bisa belum lengkap). Pengecekan mencakup antara lain lebar/clearance tembaga (trace/pad/via), geometri via/annular, jarak drill-to-drill, clearance **hole/drill-ke-tembaga lain** (`DRC_126`, layer/span-aware: via & THT pad drill vs trace/pad net lain), edge clearance tembaga (trace/via/**pad**, `DRC_130/131/132`), koneksi tak ter-route, overlap komponen, serta clearance **via-pad** (`DRC_124`) dan **pad-pad beda net** (`DRC_125`). **Gap terkonfirmasi (audit):** geometri DRC memakai **AABB axis-aligned** dari bentuk pad yang dirotasi (`pad_rect`, benar untuk rotasi arbitrer) sehingga *false negative* rotasi non-ortogonal berkurang; **pad `Round` melingkar (sumbu sama)** kini memakai jarak **disk** (mengurangi *false positive* sudut AABB), sedangkan **pad `Round` obround/oval (sumbu beda)** dan bentuk non-rect presisi penuh tetap konservatif/AABB. Aturan solder mask/paste/courtyard, copper pour/zone/keepout/cutout, NPTH/drill khusus (bila dimodelkan), dan DFM pabrikan spesifik belum dicakup.
- **Placement/transformasi disederhanakan**: representasi pad/footprint deterministik sederhana; akurasi mekanik/produksi untuk footprint kompleks bergantung kelengkapan definisi. Perhitungan geometri pad/DRC kini memakai **AABB rotasi yang benar** (mengurangi risiko *false negative* clearance pada rotasi non-ortogonal; sebelumnya swap 90° saja). Untuk clearance DRC, **pad `Round` melingkar (sumbu sama)** memakai jarak **disk** (mengurangi *false positive* sudut); **obround/oval** dan bentuk presisi lain masih konservatif (AABB) untuk menghindari *false negative*.

### Library & Footprint
- **Library terbatas**: terutama komponen/footprint **bawaan + stub generik**; metadata pin (`PinType`)/footprint bisa tidak spesifik.
- **ERC bergantung metadata library**: deteksi driver/sumber daya/NC (termasuk `ERC_011` NC terhubung, `ERC_024` multi-output, `ERC_025/026` sumber power/ground; `Output` dapat dianggap sumber pada net `Power`) bergantung akurasi `PinType`/definisi part — gunakan library/part tervalidasi untuk hasil produksi.
- **Impor library/footprint vendor arbitrer terbatas** (belum setara ekosistem KiCad/vendor); footprint mekanik kompleks/3D umumnya belum tersedia.
- Untuk produksi nyata, footprint dan aturan manufaktur perlu divalidasi/disesuaikan.

### Interoperabilitas (KiCad) & Node Graph
- **Adapter KiCad `.kicad_pcb` subset konservatif**: tidak mencakup seluruh format/fitur KiCad; *round-trip* penuh terbatas (parser/adapter internal, bukan dependensi penuh KiCad). Import mendukung `segment`, `via`, outline Edge.Cuts (`gr_line`/`gr_rect`/`gr_poly`, serta **`gr_arc`/`gr_circle`** yang ditesselasi ke outline/poligon), footprint/pad/net, dan **track `arc`** (KiCad 7/8 start/mid/end → polyline). Elemen grafis yang belum dimodelkan penuh dilaporkan terarah: `gr_arc`/`gr_circle` non-Edge.Cuts dan `fp_arc`/`fp_circle` → `KICAD_013`; elemen besar lain yang mungkin belum terimpor penuh → `KICAD_012` (mis. zone/pour, dimension, group, keepout/rule_area). Kode backend KiCad dipisah menjadi modul (`lib`/`export`/`import`/`sexpr`/`tests`) untuk pemeliharaan. Verifikasi manual tetap diperlukan.
- **Proyeksi/import node graph parsial**: beberapa *node kind* bisa tidak didukung dan dilewati; fidelitas dua-arah source↔graph tidak selalu identik byte-per-byte untuk semua kasus.

### Manufaktur & Output
- Pipeline dasar tersedia (Gerber, Excellon drill, BOM, pick-and-place) dan tervalidasi, namun **opsi ekspor, DFM, stackup/aturan pabrikan spesifik, dan verifikasi produksi mendalam** belum setara tool EDA matang.

### Simulasi & Analisis
- **Tidak ada** SPICE, signal-integrity, power-integrity, thermal, atau analisis elektromagnetik/simulasi sejenis.

### Tooling, GUI & Ekosistem
- **Tidak ada GUI resmi** di workspace inti saat ini (viewer/editor interaktif, routing UI, manajemen layer/zone/inspector/3D setara KiCad belum tersedia).
- **Ekosistem tooling bantu terbatas**: LSP/editor integration kaya, formatter/linter khusus, wizard/library manager/3D viewer, dan integrasi pihak ketiga masih belum matang.
- Dokumentasi contoh ada, namun cakupan contoh/validasi hardware produksi masih berkembang.

## Workspace

Crates live under `crates/` (for example `papan-types`, `papan-parser`, `papan-ir`, `papan-routing`, `papan-manufacturing`, `papan-api`, `papan-cli`). Integration tests live under `tests/`.

## Build and test

```powershell
cargo test --workspace
cargo build --workspace
```

## CLI

The CLI binary is `papan`.

```powershell
cargo run -p papan-cli -- --help
cargo run -p papan-cli -- new demo.papan
cargo run -p papan-cli -- check demo.papan
cargo run -p papan-cli -- build demo.papan --target manufacturing
cargo run -p papan-cli -- export demo.papan --format kicad
cargo run -p papan-cli -- import board.kicad_pcb --format kicad
cargo run -p papan-cli -- validate demo.papan
cargo run -p papan-cli -- inspect demo.papan --json
cargo run -p papan-cli -- clean demo.papan
```

CLI exit codes: `0` success, `1` design/validation errors, `2` usage errors, `3` I/O/import/internal errors.

## Examples

```powershell
cargo run -p papan-cli -- build examples/simple_board
cargo run -p papan-cli -- build examples/keyboard --force
cargo run -p papan-cli -- build examples/embedded --force
cargo run -p papan-cli -- build examples/multilayer --force
```

Some examples are intentionally richer than the current router/DRC can fully optimize; `--force` can be used to emit outputs while preserving diagnostics.

## Documentation

See [`docs/`](docs/):

- `language-spec.md`
- `architecture.md`
- `node-system.md`
- `compiler.md`
- `pcb-engine.md`
- `api.md`
- `testing.md`
- `manufacturing.md`
