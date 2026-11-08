# PAPAN — Product Requirements Document

**Product:** PAPAN Programming Language  
**Category:** Electronic Design Automation (EDA)  
**Implementation Language:** Rust  
**Paradigm:** Hybrid Declarative + Node-Based Programming  
**Architecture:** Custom Core Engine + External Backend Adapters  
**Target:** All PCB Design Categories  
**Document Status:** Architecture Baseline  
**Version:** 0.1.0

---

# 1. Product Vision

PAPAN adalah bahasa pemrograman khusus Electronic Design Automation (EDA) untuk mendesain, menganalisis, memvalidasi, dan menghasilkan Printed Circuit Board (PCB).

PAPAN bukan sekadar scripting language untuk menggambar jalur PCB. PAPAN harus menjadi bahasa desain elektronik yang memiliki compiler, type system, intermediate representation, node graph, constraint solver, geometry engine, electrical engine, routing engine, dan manufacturing pipeline.

PAPAN harus dapat digunakan oleh:

- Manusia melalui textual programming.
- Desainer elektronik melalui visual node editor.
- AI melalui structured programming API.
- Software eksternal melalui interoperable interfaces.

Tujuan akhirnya adalah memungkinkan pembuatan PCB sederhana hingga kompleks, termasuk keyboard mekanikal, embedded systems, motherboard, multilayer PCB, high-speed PCB, dan perangkat elektronik profesional.

## 1.1 Core Principles

1. Text dan Node harus memiliki kemampuan yang setara.
2. Semua representasi harus mengacu pada satu semantic model.
3. Setiap koneksi node harus memiliki tipe yang jelas.
4. Desain harus dapat divalidasi sebelum produksi.
5. Semua perubahan harus dapat dilacak.
6. Compiler harus deterministik untuk input dan konfigurasi yang sama.
7. Tidak boleh ada perubahan IR tanpa pembaruan verifier.
8. Setiap perbaikan compiler wajib memiliki regression test.
9. Engine harus modular dan dapat dikembangkan secara independen.
10. Integrasi eksternal tidak boleh mengunci arsitektur internal PAPAN.

---

# 2. Architecture Decisions

## 2.1 Hybrid Programming Model

PAPAN menggunakan dua antarmuka pemrograman:

### Textual Programming

Pengguna menulis desain menggunakan sintaks PAPAN.

Contoh:

```text
board "Mechanical Keyboard"

size 300mm x 120mm
thickness 1.6mm
layers 4

component MCU: "ATmega32U4"
component USB: "USB-C"

net VCC
net GND

connect MCU.VCC -> VCC
connect MCU.GND -> GND

place MCU at (150mm, 60mm)
place USB at (150mm, 10mm)

check DRC
check ERC

export gerber
```

### Visual Node Programming

Semua deklarasi, komponen, koneksi, aturan, dan operasi desain dapat direpresentasikan melalui node.

Node editor harus mendukung:

- Node creation.
- Node deletion.
- Node connections.
- Typed ports.
- Node grouping.
- Node parameters.
- Node properties.
- Node validation.
- Node search.
- Node templates.
- Node duplication.
- Node comments.
- Graph navigation.
- Undo/redo.
- Multi-selection.
- Copy/paste.
- Graph serialization.

### Synchronization Rules

Text dan Node tidak boleh mempunyai dua sumber kebenaran yang berbeda.

Keduanya harus mengubah semantic document model yang sama.

Perubahan melalui Text:

Text → Parser → Semantic Model → PCB IR → Node Projection.

Perubahan melalui Node:

Node Graph → Graph Validation → Semantic Model → PCB IR → Text Projection.

Perubahan harus menggunakan transaction system sehingga edit yang belum valid tidak merusak desain yang sebelumnya valid.

---

# 3. Core Data Architecture

## 3.1 PAPAN Document Model

PAPAN harus mempunyai struktur dokumen yang stabil.

Setiap objek memiliki:

- Stable identifier.
- Object type.
- Properties.
- Relationships.
- Source location.
- Version information.
- Validation state.

Contoh:

```text
Project
 ├── Board
 │    ├── Outline
 │    ├── Stackup
 │    ├── Components
 │    ├── Nets
 │    ├── Footprints
 │    ├── Traces
 │    ├── Vias
 │    ├── Zones
 │    ├── Constraints
 │    └── Mechanical
 │
 ├── Libraries
 ├── Rules
 ├── Parameters
 ├── Variants
 └── Manufacturing
```

## 3.2 Intermediate Representation (PIR)

PIR adalah representasi internal utama PAPAN.

PIR harus menyimpan semantic information, bukan hanya graphical coordinates.

PIR mencakup:

- Board entities.
- Electrical connectivity.
- Component definitions.
- Pin and pad mappings.
- Net relationships.
- Geometry.
- Layer stackup.
- Design constraints.
- Routing.
- Manufacturing metadata.
- Node relationships.
- Object identities.

PIR harus mempunyai schema version dan verifier.

Perubahan schema harus melalui migration mechanism.

## 3.3 Identity System

Semua objek harus memiliki stable ID.

Contoh:

```text
component MCU {
    id: "component-001"
}
```

Memindahkan komponen tidak boleh menghasilkan ID baru.

ID diperlukan untuk:

- Incremental compilation.
- Undo/redo.
- Change tracking.
- AI modifications.
- Node synchronization.
- Project versioning.

---

# 4. Language Specification

## 4.1 Syntax Philosophy

Sintaks PAPAN menggunakan pendekatan declarative dan constraint-based.

Bahasa harus mudah dibaca dan tidak mengharuskan pengguna menulis algoritma geometris secara manual.

## 4.2 Basic Syntax

```text
board "My PCB"

size 100mm x 80mm
thickness 1.6mm
layers 2
```

## 4.3 Data Types

Primitive types:

- Integer
- Float
- Boolean
- String
- Identifier
- Array
- Object
- Enum
- Quantity
- Reference

Domain types:

- Length
- Area
- Angle
- Voltage
- Current
- Resistance
- Capacitance
- Inductance
- Frequency
- Impedance
- Temperature
- Power
- Layer
- Net
- Component
- Pin
- Pad
- Footprint
- Geometry
- Constraint

Semua quantity harus mempunyai unit yang jelas.

Contoh:

```text
width: 0.25mm
voltage: 5V
current: 500mA
frequency: 16MHz
impedance: 90Ω
temperature: 60degC
```

Unit conversion harus dilakukan secara eksplisit dan konsisten.

## 4.4 Component Declaration

```text
component MCU: "ATmega32U4"

component R1: resistor {
    value: 10kΩ
    tolerance: 1%
    package: "0603"
}

component C1: capacitor {
    value: 100nF
    voltage: 16V
    package: "0603"
}
```

Komponen harus mendukung:

- Symbol.
- Footprint.
- Pin mapping.
- Datasheet reference.
- Electrical properties.
- 3D model.
- Manufacturer metadata.
- Component variants.

## 4.5 Net Declaration

```text
net VCC {
    voltage: 5V
    type: power
}

net GND {
    type: ground
}
```

## 4.6 Connections

```text
connect MCU.VCC -> VCC
connect MCU.GND -> GND
```

Compiler harus memeriksa pin compatibility, electrical rules, dan connectivity.

## 4.7 Placement

```text
place MCU at (50mm, 40mm)

place USB at board.top.center

place R1 near MCU.VCC distance 3mm

rotate MCU 90deg
```

Placement harus mendukung absolute coordinates, relative positioning, alignment, grouping, dan constraints.

## 4.8 Routing

```text
route USB_DP {
    width: 0.15mm
    layer: F.Cu
}

via {
    drill: 0.30mm
    diameter: 0.60mm
}
```

Routing harus mempertimbangkan:

- Clearance.
- Trace width.
- Layer availability.
- Via rules.
- Impedance.
- Differential pairs.
- Length matching.
- Return paths.
- Routing obstacles.

## 4.9 Constraints

```text
constraints {
    min_trace_width: 0.15mm
    clearance: 0.20mm
    min_via_drill: 0.30mm
    copper_edge_clearance: 0.25mm
}
```

Constraints harus dapat didefinisikan secara global, per-net, per-net-class, dan per-design-region.

## 4.10 Modules

PAPAN harus mendukung reusable design modules.

```text
module USB_Power {
    input voltage: 5V
    output power: VCC

    component C1: capacitor 10uF
    component C2: capacitor 100nF
}
```

Modules harus mendukung parameters, inputs, outputs, dependencies, dan versioning.

---

# 5. Node System

Node system merupakan salah satu fondasi utama PAPAN.

## 5.1 Node Categories

### Board Nodes
- Board
- BoardSize
- BoardOutline
- BoardOrigin
- BoardThickness
- BoardCutout
- MountingHole
- Stackup

### Component Nodes
- Component
- Resistor
- Capacitor
- Diode
- Transistor
- IC
- Connector
- Crystal
- LED
- Switch

### Pin and Pad Nodes
- Pin
- Pad
- SMDPad
- ThroughHolePad
- PadArray
- PinMapping

### Electrical Nodes
- Net
- NetClass
- PowerNet
- GroundNet
- DifferentialPair
- NetTie
- Connect

### Placement Nodes
- Place
- Move
- Rotate
- Align
- Group
- Lock
- Keepout

### Routing Nodes
- Trace
- Route
- Via
- TrackWidth
- Clearance
- AutoRoute
- DifferentialRoute
- LengthMatch

### Layer Nodes
- CopperLayer
- Dielectric
- SolderMask
- Silkscreen
- PasteMask
- LayerStack

### Copper Nodes
- CopperZone
- GroundPlane
- PowerPlane
- ThermalRelief
- ZoneFill

### Mechanical Nodes
- Dimension
- Hole
- Slot
- Edge
- Cutout
- Enclosure
- ThreeDModel

### Rule Nodes
- DRC
- ERC
- ClearanceRule
- WidthRule
- ViaRule
- ImpedanceRule
- ThermalRule

### Simulation Nodes
- VoltageAnalysis
- CurrentAnalysis
- SPICE
- SignalIntegrity
- PowerIntegrity
- ThermalSimulation
- ImpedanceAnalysis

### Manufacturing Nodes
- Gerber
- Drill
- BOM
- PickPlace
- AssemblyDrawing
- ManufacturingCheck
- Export

## 5.2 Typed Ports

Node connections harus menggunakan typed ports.

Contoh:

| Port Type | Description |
|---|---|
| Component | Component reference |
| Pin | Electrical pin |
| Pad | Physical pad |
| Net | Electrical network |
| Layer | PCB layer |
| Geometry | Geometric object |
| Constraint | Design constraint |
| Signal | Signal definition |
| Quantity | Unit-aware value |
| ManufacturingArtifact | Manufacturing output |

Koneksi tidak valid harus ditolak.

Contoh:

Pin → Net: valid.

Net → Route: valid melalui routing input yang sesuai.

Layer → Route: valid sebagai layer parameter.

Component → Layer: invalid.

Compiler harus mempunyai typed connection verifier.

## 5.3 Node Graph Features

- Automatic node layout.
- Graph minimap.
- Node search.
- Collapsible groups.
- Graph bookmarks.
- Typed connection visualization.
- Error highlighting.
- Node dependency tracking.
- Incremental graph updates.
- Graph serialization.
- Subgraph reuse.
- Graph diff.

---

# 6. Compiler Architecture

Compiler harus modular dan mempunyai tahapan kompilasi yang eksplisit.

```text
PAPAN SOURCE
     |
     v
Lexer
     |
     v
Parser
     |
     v
AST
     |
     v
Semantic Analysis
     |
     v
Type Checker
     |
     v
Document Model
     |
     v
PCB IR
     |
     +------------------+
     |                  |
     v                  v
Electrical Analysis   Geometry Analysis
     |                  |
     +--------+---------+
              |
              v
       Constraint Solver
              |
              v
       Placement Engine
              |
              v
        Routing Engine
              |
              v
       PCB Validation
              |
              v
     Manufacturing Pipeline
```

## 6.1 Lexer

Responsibilities:

- Tokenization.
- Identifiers.
- Numeric literals.
- Unit literals.
- Strings.
- Operators.
- Comments.
- Source spans.

## 6.2 Parser

Responsibilities:

- Grammar validation.
- AST generation.
- Syntax diagnostics.
- Error recovery.

## 6.3 Semantic Analyzer

Responsibilities:

- Symbol resolution.
- Reference resolution.
- Component lookup.
- Net resolution.
- Dependency analysis.
- Scope validation.

## 6.4 Type Checker

Memastikan setiap operasi dan koneksi memiliki tipe yang sesuai.

Contoh:

- Voltage tidak dapat digunakan sebagai Length.
- Pin harus mempunyai referensi komponen.
- Net harus mempunyai valid electrical members.
- Layer harus termasuk stackup yang aktif.

## 6.5 PCB IR

PCB IR harus menjadi input bagi engine internal dan backend adapters.

IR tidak boleh bergantung pada format internal KiCad atau aplikasi eksternal lainnya.

## 6.6 Incremental Compilation

Compiler harus mampu mengompilasi hanya bagian desain yang berubah.

Perubahan resistor tidak boleh memaksa seluruh proyek dihitung ulang jika tidak memiliki dependency yang relevan.

Dependency graph harus mendukung invalidation dan cache.

---

# 7. PCB Engine Architecture

PAPAN menggunakan custom core engine dengan external backend adapters.

## 7.1 Custom Core

### Geometry Engine

Responsibilities:

- Board geometry.
- Polygon operations.
- Clipping.
- Intersection.
- Distance calculations.
- Offset operations.
- Spatial indexing.
- Collision detection.

Gunakan fixed-point atau representasi numerik yang terkontrol untuk operasi geometri PCB agar tidak bergantung pada floating-point rounding yang tidak konsisten.

### Electrical Engine

Responsibilities:

- Net connectivity.
- Pin and pad validation.
- Electrical graph.
- Connectivity analysis.
- Unconnected pins.
- Short circuit detection.
- Electrical rule evaluation.

### Placement Engine

Responsibilities:

- Component placement.
- Relative constraints.
- Collision detection.
- Alignment.
- Keepout enforcement.
- Placement optimization.

### Routing Engine

Responsibilities:

- Track routing.
- Via placement.
- Layer transitions.
- Obstacle avoidance.
- Clearance enforcement.
- Differential pair routing.
- Length matching.
- Interactive routing.
- Autorouting.

Autorouter harus mempunyai deterministic mode dan dapat menghasilkan laporan kegagalan.

### Constraint Solver

Responsibilities:

- Position constraints.
- Distance constraints.
- Alignment constraints.
- Relative placement.
- Geometric constraints.
- Design rule constraints.

### Manufacturing Engine

Responsibilities:

- Gerber generation.
- Excellon drill generation.
- BOM.
- Pick-and-place.
- Assembly drawing.
- Manufacturing validation.

## 7.2 External Backend Adapters

Adapter bukan sumber kebenaran desain.

Adapter harus mempunyai interface tersendiri.

Contoh:

```rust
trait PcbBackend {
    fn import(&self, input: BackendInput) -> Result<PcbIr>;
    fn export(&self, ir: &PcbIr) -> Result<BackendOutput>;
    fn validate(&self, ir: &PcbIr) -> Result<ValidationReport>;
}
```

Implementasi awal:

- KiCad adapter.
- Gerber interoperability adapter.
- IPC-2581 adapter pada tahap lanjutan.
- ODB++ adapter pada tahap lanjutan.

Adapter hanya boleh mengubah PIR melalui proses import/export yang terdefinisi dan tervalidasi.

Tidak boleh ada ketergantungan wajib terhadap KiCad agar PAPAN dapat membuat dan mengompilasi desain native.

---

# 8. DRC and ERC

## 8.1 Design Rule Check

DRC harus memeriksa:

- Copper clearance.
- Trace width.
- Via diameter.
- Via drill.
- Board edge clearance.
- Copper-to-edge distance.
- Solder mask clearance.
- Drill-to-drill distance.
- Unrouted connections.
- Copper overlap.
- Layer constraints.
- Manufacturing constraints.

## 8.2 Electrical Rule Check

ERC harus memeriksa:

- Unconnected pins.
- Conflicting pin types.
- Invalid power connections.
- Multiple output conflicts.
- Missing power source.
- Invalid net references.
- Component pin mapping errors.

## 8.3 Diagnostics

Setiap error harus memiliki:

- Stable error code.
- Severity.
- Description.
- Source reference.
- Object reference.
- Coordinate when applicable.
- Suggested correction when available.

Contoh:

```text
error DRC_102 {
    severity: error
    message: "Trace clearance violation"
    object: "trace-001"
    location: (50mm, 40mm)
}
```

Validation harus membedakan error, warning, informational message, dan waived rule.

Tidak boleh menandai desain sebagai production-ready jika terdapat pelanggaran yang belum diselesaikan sesuai manufacturing profile.

---

# 9. Simulation Architecture

Simulation merupakan sistem modular.

## 9.1 SPICE Integration

Mendukung:

- DC analysis.
- AC analysis.
- Transient analysis.
- Operating point.
- Component models.

## 9.2 Signal Integrity

Mendukung pengembangan:

- Transmission line models.
- Differential pair analysis.
- Impedance calculations.
- Crosstalk analysis.
- Reflection analysis.

## 9.3 Power Integrity

Mendukung:

- Voltage drop.
- Current distribution.
- Power plane analysis.
- Decoupling analysis.

## 9.4 Thermal Analysis

Mendukung:

- Component heat.
- Copper heat distribution.
- Thermal vias.
- Thermal relief.
- Temperature estimation.

Semua simulasi harus menyertakan model, asumsi, solver version, dan batas akurasi.

Tidak boleh mengklaim simulasi sebagai pengganti pengukuran fisik atau validasi laboratorium.

---

# 10. Component Library Architecture

Component Library harus menjadi sistem independen.

Setiap component definition mendukung:

```text
ComponentDefinition
 ├── Metadata
 ├── Symbol
 ├── Footprint
 ├── PinMap
 ├── ElectricalProperties
 ├── Package
 ├── 3DModel
 ├── Datasheet
 └── Manufacturer
```

Fitur:

- Library search.
- Component creation.
- Footprint editor.
- Symbol editor.
- Pin mapping.
- Component validation.
- Versioning.
- Dependency management.
- Local library.
- Shared library.
- Library import/export.

Library tidak boleh bergantung pada satu vendor komponen.

---

# 11. AI Integration

AI merupakan salah satu consumer utama PAPAN API.

AI tidak boleh mengedit file internal secara sembarangan.

AI harus menggunakan structured APIs.

Contoh:

```text
project.add_component(...)
project.connect(...)
project.place(...)
project.modify(...)
project.validate(...)
project.export(...)
```

## 11.1 AI Capabilities

- Generate PCB from specifications.
- Generate schematic.
- Add components.
- Create connections.
- Modify placement.
- Suggest routing.
- Explain DRC errors.
- Optimize board layout.
- Compare design versions.
- Generate manufacturing documentation.

## 11.2 AI Safety and Validation

Semua perubahan AI harus:

1. Menggunakan stable object IDs.
2. Dijalankan dalam transaction.
3. Melewati semantic validation.
4. Menampilkan perubahan sebelum commit jika berdampak besar.
5. Melewati DRC/ERC yang relevan.
6. Dapat dibatalkan.

AI tidak boleh menganggap desain valid hanya karena source code berhasil diparse.

---

# 12. Public API

PAPAN harus menyediakan API untuk aplikasi eksternal.

Contoh konseptual:

```rust
let project = PapanProject::new();

let board = project.create_board("Keyboard");

let mcu = board.add_component("ATmega32U4");

let usb = board.add_component("USB-C");

board.connect(mcu.pin("VCC"), board.net("VCC"));

let report = project.validate();

project.export(ExportFormat::Gerber);
```

Public API harus mendukung:

- Project creation.
- Board creation.
- Component management.
- Net management.
- Connections.
- Placement.
- Routing.
- Constraints.
- Validation.
- Export.
- Graph operations.
- Document serialization.
- Undo/redo transactions.

API harus mempunyai versioning dan dokumentasi.

---

# 13. Project and File Format

Struktur project:

```text
keyboard.papan/
│
├── project.papan
│
├── src/
│   ├── main.pn
│   ├── components.pn
│   ├── connections.pn
│   ├── placement.pn
│   ├── routing.pn
│   └── rules.pn
│
├── nodes/
│   └── main.graph
│
├── libraries/
│
├── assets/
│   └── models/
│
├── tests/
│
├── cache/
│
└── output/
```

File extensions:

- `.pn` — PAPAN source.
- `.graph` — Node graph serialization.
- `.papan` — Project manifest or project package.

Project serialization harus mendukung schema version, stable IDs, references, and migrations.

Build artifacts harus dapat dihasilkan ulang dari source dan project dependencies.

Cache bukan sumber kebenaran.

---

# 14. CLI Design

CLI executable: `papan`

Contoh commands:

```bash
papan new keyboard
papan check keyboard.papan
papan build keyboard.papan
papan build keyboard.papan --target gerber
papan build keyboard.papan --target drill
papan export keyboard.papan --format kicad
papan import external.kicad_pcb
papan validate keyboard.papan
papan inspect keyboard.papan
papan clean keyboard.papan
```

CLI harus mempunyai:

- Structured errors.
- Human-readable diagnostics.
- JSON output mode.
- Exit codes.
- Verbose mode.
- Build profiles.
- Reproducible builds.

---

# 15. GUI Requirements

GUI bukan bagian dari compiler core.

GUI menggunakan public engine APIs.

Interface utama:

1. Project Explorer.
2. Text Editor.
3. Node Graph Editor.
4. PCB 2D Canvas.
5. PCB 3D Viewer.
6. Component Library Browser.
7. Properties Inspector.
8. Layer Manager.
9. Routing Tools.
10. Design Rules Panel.
11. DRC/ERC Panel.
12. Simulation Panel.
13. Manufacturing Panel.
14. Console and Diagnostics.
15. Version History.

Semua panel harus menggunakan document model yang sama.

GUI harus mendukung responsive layout, viewport virtualization, rendering cache, incremental invalidation, dan efficient geometry batching.

---

# 16. Performance Requirements

Performance harus menjadi bagian dari desain sejak awal.

Target engineering:

- Incremental parsing.
- Incremental semantic analysis.
- Incremental IR updates.
- Cached geometry.
- Spatial indexing.
- Parallel independent calculations.
- Efficient graph traversal.
- Memory-aware object storage.
- Background validation.
- Cancellation support.
- Deterministic builds.

Benchmark harus menguji:

- Small PCB.
- Medium PCB.
- Large PCB.
- High-component-count PCB.
- Dense multilayer PCB.
- Large node graphs.

Tidak boleh menetapkan klaim performa tanpa benchmark yang dapat direproduksi.

---

# 17. Rust Workspace Architecture

Proposed workspace:

```text
papan/
│
├── Cargo.toml
│
├── crates/
│   │
│   ├── papan-core/
│   ├── papan-types/
│   ├── papan-units/
│   ├── papan-lexer/
│   ├── papan-parser/
│   ├── papan-ast/
│   ├── papan-semantic/
│   ├── papan-ir/
│   ├── papan-ir-verifier/
│   │
│   ├── papan-node/
│   ├── papan-graph/
│   ├── papan-document/
│   ├── papan-transaction/
│   ├── papan-diff/
│   │
│   ├── papan-geometry/
│   ├── papan-electrical/
│   ├── papan-constraints/
│   ├── papan-placement/
│   ├── papan-routing/
│   ├── papan-stackup/
│   │
│   ├── papan-drc/
│   ├── papan-erc/
│   ├── papan-simulation/
│   │
│   ├── papan-library/
│   ├── papan-manufacturing/
│   ├── papan-export/
│   ├── papan-import/
│   │
│   ├── papan-backend/
│   ├── papan-backend-kicad/
│   ├── papan-backend-gerber/
│   │
│   ├── papan-api/
│   ├── papan-cli/
│   └── papan-gui/
│
├── tests/
│   ├── parser/
│   ├── semantic/
│   ├── ir/
│   ├── nodes/
│   ├── geometry/
│   ├── electrical/
│   ├── routing/
│   ├── drc/
│   ├── erc/
│   ├── manufacturing/
│   └── end_to_end/
│
├── examples/
│   ├── simple_board/
│   ├── keyboard/
│   ├── embedded/
│   └── multilayer/
│
├── docs/
│   ├── language-spec.md
│   ├── architecture.md
│   ├── node-system.md
│   ├── compiler.md
│   ├── pcb-engine.md
│   ├── api.md
│   ├── testing.md
│   └── manufacturing.md
│
└── PRD.md
```

Crate boundaries harus ditentukan berdasarkan tanggung jawab dan dependency direction, bukan hanya jumlah modul.

---

# 18. Testing and Verification

Testing merupakan persyaratan wajib.

## 18.1 Compiler Tests

- Lexer tests.
- Parser tests.
- AST tests.
- Semantic tests.
- Type checker tests.
- Diagnostic tests.
- IR verifier tests.

## 18.2 Node Tests

- Port type compatibility.
- Graph connectivity.
- Invalid connection rejection.
- Graph serialization.
- Text-node synchronization.
- Stable ID preservation.

## 18.3 Geometry Tests

- Polygon operations.
- Collision detection.
- Distance calculations.
- Clipping.
- Board outline.
- Copper clearance.

## 18.4 Electrical Tests

- Connectivity.
- Short detection.
- Open connection detection.
- Pin compatibility.
- Net validation.

## 18.5 Routing Tests

- Trace generation.
- Obstacle avoidance.
- Clearance.
- Via placement.
- Layer transitions.
- Differential routing.
- Length matching.

## 18.6 End-to-End Tests

Wajib menguji:

```text
PAPAN SOURCE
     |
     v
PARSE
     |
     v
COMPILE
     |
     v
GENERATE PCB
     |
     v
RUN DRC/ERC
     |
     v
EXPORT GERBER
     |
     v
VERIFY MANUFACTURING OUTPUT
```

Build yang sukses saja tidak cukup.

Output harus diperiksa terhadap expected geometry, connectivity, manufacturing data, dan design rules.

Setiap compiler fix wajib memiliki regression test.

Perubahan IR wajib disertai verifier tests dan migration tests.

Text → IR → Text harus mempertahankan semantic equivalence.

Node → IR → Node harus mempertahankan semantic equivalence.

---

# 19. Development Roadmap

## Phase 1 — Language Foundation

Deliverables:

- Grammar specification.
- Lexer.
- Parser.
- AST.
- Semantic analyzer.
- Unit system.
- Diagnostic system.

Acceptance criteria:

- Contoh program dapat diparse.
- Invalid syntax menghasilkan diagnostic yang tepat.
- Unit checking berjalan.
- Parser memiliki regression tests.

## Phase 2 — Core Data Model

Deliverables:

- Document model.
- PCB IR.
- IR verifier.
- Stable object IDs.
- Serialization.
- Transaction system.

Acceptance criteria:

- Board dan component dapat direpresentasikan.
- IR dapat disimpan dan dibaca kembali.
- Verifier menolak invalid references.
- Stable IDs tetap konsisten.

## Phase 3 — Electrical Foundation

Deliverables:

- Component definitions.
- Pin mapping.
- Footprints.
- Net system.
- Connectivity graph.
- Electrical validation.

Acceptance criteria:

- Komponen dapat ditambahkan.
- Pin dapat dipetakan.
- Net dapat dibuat.
- Koneksi dapat diperiksa.

## Phase 4 — Node Graph

Deliverables:

- Node schema.
- Typed ports.
- Graph editor model.
- Graph serializer.
- Text-node synchronization.

Acceptance criteria:

- Semua objek inti dapat direpresentasikan sebagai node.
- Invalid connections ditolak.
- Text dan node mempertahankan semantic equivalence.

## Phase 5 — Geometry and Placement

Deliverables:

- Board outline.
- Geometry kernel.
- Footprint placement.
- Collision detection.
- Constraint solver.

Acceptance criteria:

- Komponen dapat ditempatkan.
- Board geometry dapat dihitung.
- Placement constraints dapat divalidasi.

## Phase 6 — Routing

Deliverables:

- Copper traces.
- Via system.
- Layer transitions.
- Routing engine.
- Basic autorouter.

Acceptance criteria:

- Routing menghasilkan konektivitas yang benar.
- Clearance diperiksa.
- Unrouted connections terdeteksi.

## Phase 7 — Design Validation

Deliverables:

- DRC.
- ERC.
- Rule engine.
- Diagnostics.
- Validation reports.

Acceptance criteria:

- Kesalahan geometris dapat ditemukan.
- Kesalahan elektris dapat ditemukan.
- Error dapat ditautkan ke objek dan lokasi.

## Phase 8 — Manufacturing

Deliverables:

- Gerber.
- Excellon.
- BOM.
- Pick-and-place.
- Manufacturing validation.

Acceptance criteria:

- Output dapat dibuka pada software independen.
- Layer mapping benar.
- Drill data benar.
- Manufacturing output lolos pemeriksaan format.

## Phase 9 — Advanced PCB

Deliverables:

- Multilayer stackup.
- Differential routing.
- Impedance constraints.
- Length matching.
- Advanced placement.
- Advanced copper zones.

Acceptance criteria:

- Layer transitions tervalidasi.
- Differential pair constraints diperiksa.
- Stackup dapat digunakan oleh solver yang sesuai.

## Phase 10 — Simulation and AI

Deliverables:

- SPICE integration.
- Signal integrity integration.
- Power integrity.
- Thermal analysis.
- AI structured API.
- Design optimization interfaces.

Acceptance criteria:

- Simulation results mempunyai provenance dan assumptions.
- AI modifications menggunakan transactions.
- Perubahan AI dapat diverifikasi dan dibatalkan.

---

# 20. Non-Goals

Hal berikut bukan tujuan tahap awal:

- Mengimplementasikan seluruh algoritma EDA sekaligus.
- Menggantikan semua software EDA pada versi pertama.
- Menjanjikan autorouting sempurna.
- Menganggap setiap desain yang lolos parser siap diproduksi.
- Menjadikan KiCad sebagai mandatory runtime dependency.
- Membuat node editor tanpa semantic validation.
- Menambahkan AI sebelum document model dan API stabil.

---

# 21. Engineering Rules

Aturan berikut wajib dipatuhi sepanjang pengembangan:

1. Jangan mengubah IR tanpa memperbarui verifier.
2. Setiap compiler fix harus memiliki regression test.
3. Jangan menghapus fitur yang sudah ada tanpa proposal dan persetujuan.
4. Jangan melakukan architectural rewrite tanpa design proposal.
5. Cargo test saja tidak cukup untuk membuktikan PCB engine benar.
6. Selalu jalankan source-to-manufacturing end-to-end tests.
7. Bandingkan output native dengan referensi independen.
8. Jangan menyembunyikan error DRC/ERC.
9. Jangan menggunakan cache sebagai sumber kebenaran.
10. Semua format eksternal harus melalui adapter.
11. Jangan mencampurkan GUI state dengan PCB semantic state.
12. Semua public API harus mempunyai kontrak dan dokumentasi.
13. Perubahan schema harus memiliki migration strategy.
14. Optimasi tidak boleh mengubah semantic output.
15. Setiap fase harus memiliki acceptance criteria sebelum dilanjutkan.

---

# 22. Definition of Done

Sebuah fitur dinyatakan selesai apabila:

- Implementasi selesai.
- API terdokumentasi.
- Unit tests lulus.
- Integration tests lulus.
- Regression tests lulus.
- IR verification lulus jika relevan.
- Performance tidak mengalami regresi yang tidak dapat diterima.
- Error handling tersedia.
- Contoh penggunaan tersedia.
- End-to-end test tersedia jika fitur memengaruhi pipeline desain.

---

# 23. Final Product Objective

PAPAN harus berkembang menjadi bahasa pemrograman PCB yang mempunyai:

- Compiler sendiri.
- Syntax sendiri.
- Type system sendiri.
- PCB Intermediate Representation sendiri.
- Node graph system sendiri.
- Geometry engine sendiri.
- Electrical engine sendiri.
- Placement engine sendiri.
- Routing engine sendiri.
- Design rule engine sendiri.
- Manufacturing pipeline sendiri.
- Public API sendiri.
- External backend interoperability.

Integrasi software lain harus memperluas kemampuan PAPAN, bukan menggantikan identitas dan fondasi teknisnya.

**Final architectural principle:**

PAPAN is the source of truth. The language defines the design, the IR defines its semantics, the engine computes its physical realization, and backend adapters provide interoperability.

The same design must remain semantically consistent whether it is authored through text, nodes, or an AI agent.