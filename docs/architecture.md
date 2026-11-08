# Architecture

PAPAN is organized as a Rust workspace with crate boundaries that match the PRD.

## Data flow

```text
.pn / .graph / .papan
        │
        ▼
   lexer + parser + AST
        │
        ▼
  semantic analysis
        │
        ▼
      PIR (papan-ir)
        │
        ├─ IR verifier
        ├─ graph projection (papan-graph)
        ├─ placement / electrical / routing / DRC
        └─ manufacturing + adapters
                │
                ▼
           papan-api / papan-cli
```

## Core principles

- **Single semantic model**: text and nodes project to the same PIR.
- **Stable IDs**: objects use deterministic identifiers (`ObjectId::stable(kind, name)` and related stable forms).
- **Determinism**: fixed-point geometry, stable sorting, deterministic routing/projection.
- **Verification**: structural IR verification is separate from ERC/DRC/manufacturing validation.
- **Transactional document model**: edits commit through `PapanDocument`/API transactions and can be undone/redone.

## Major crates

- `papan-types`: IDs, spans, diagnostics, units, fixed-point length/angle/point.
- `papan-lexer`, `papan-ast`, `papan-parser`: text frontend with recovery.
- `papan-semantic`: AST lowering to PIR, symbol resolution, modules, placement directives.
- `papan-ir`, `papan-ir-verifier`: PIR schema and structural checks.
- `papan-geometry`, `papan-placement`: fixed-point geometry and footprint/component placement.
- `papan-library`: built-in component definitions and generic stubs.
- `papan-electrical`, `papan-routing`, `papan-drc`: connectivity/ERC, autorouting, design rules.
- `papan-manufacturing`: Gerber/Excellon/BOM/pick-and-place generation and validation.
- `papan-graph`: typed node schema, validation, diff, layout, source/IR projection.
- `papan-document`: compilation, projections, transactions, project loading/saving.
- `papan-backend`, `papan-backend-kicad`: adapter abstraction and KiCad subset.
- `papan-api`: public facade over documents/IR/exports.
- `papan-cli`: command-line interface.
