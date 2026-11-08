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

Current limitations (intentional/out of scope for this implementation phase):

- no GUI
- no SPICE/signal-integrity/thermal simulation
- autorouter is deterministic and basic; advanced multilayer optimization, differential-pair routing, and length matching are not fully implemented
- KiCad adapter supports a conservative subset, not the full KiCad format
- importing arbitrary vendor libraries/footprints is limited to the built-in library plus generic stubs

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
