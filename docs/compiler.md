# Compiler

The PAPAN compiler converts source text into PIR and records structured commands.

## Pipeline

1. **Lexing** (`papan-lexer`): tokenizes source and emits `LEX_*` diagnostics.
2. **Parsing** (`papan-parser`): builds an AST with recovery and `PARSE_*` diagnostics.
3. **Semantic analysis** (`papan-semantic`): resolves symbols, validates units/types/references, lowers declarations, applies placement directives, records `check`/`export` commands, emits `SEM_*` diagnostics.
4. **IR verification** (`papan-ir-verifier`): checks schema, identities, references, pin/pad mappings, stackup, geometry constraints, and routing structure (`IR_*`).
5. **Derived validation/engines**: ERC/DRC/manufacturing run on PIR as requested by API/CLI.

## PIR

The PIR (`papan-ir`) stores semantic and physical PCB data:

- board, stackup, layers
- component definitions and instances
- nets, connections, pin refs
- traces, vias, route directives
- constraints and metadata

All lengths are integer nanometres; angles are millidegrees.

## Document integration

`papan-document` compiles source with a `ComponentLibrary`, refreshes graph projections, manages undo/redo snapshots, and exposes project loading/saving. Imperative API mutations use `commit_ir`, which mutates IR then regenerates canonical source/graph projections.
