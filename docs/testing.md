# Testing

PAPAN uses crate-local tests and cross-crate integration tests.

## Test layout

- crate tests: `crates/<name>/src/...` `#[cfg(test)]` modules and crate test targets
- integration tests package: `tests/` (workspace member `papan-integration-tests`)
- CLI integration tests: `crates/papan-cli/tests/`

## Integration test suites (`tests/`)

Declared in `tests/Cargo.toml`:

- `parser.rs` — lexer/parser recovery and AST-to-IR lowering smoke checks
- `semantic.rs` — symbol/unit validation, modules, commands
- `ir.rs` — PIR schema/verifier behavior and JSON roundtrip
- `nodes.rs` — typed graph validation, source projection, graph/IR consistency
- `geometry.rs` — segment/rectangle fixed-point geometry
- `electrical.rs` — connectivity, unrouted nets, unconnected pins, ERC
- `routing.rs` — autorouter traces, widths, layer/via behavior
- `drc.rs` — design-rule diagnostics over actual geometry/connectivity
- `erc.rs` — ERC report behavior
- `manufacturing.rs` — Gerber/Excellon/BOM/pick-and-place content and bundle validation
- `end_to_end.rs` — source to routing/validation/export and document roundtrip

## Running tests

```powershell
cargo test --workspace
cargo test -p papan-api
cargo test -p papan-cli --test cli
cargo test -p papan-integration-tests --test end_to_end
```

On PowerShell, `cargo test` can occasionally surface a `NativeCommandError` wrapper even when the test summary shows success; rely on the Cargo test summary and process behavior.

## Writing good tests

- verify geometry/connectivity/manufacturing data, not only “compiled successfully”
- assert stable diagnostic codes/prefixes where behavior is contractual
- prefer deterministic fixtures (`tests/src/lib.rs` helpers)
- place components to avoid accidental autorouter/DRC artifacts unrelated to the behavior under test
- add regression tests for every compiler bug fix (per PRD)
