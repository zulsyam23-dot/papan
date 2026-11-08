# Public API

The public facade lives in `papan-api`.

## Main types

- `PapanProject`: owns `PapanDocument` and `ComponentLibrary`; supports create/open/compile/import/export/validate/save/undo-redo.
- `Board<'_>`: project-scoped board operations.
- `ComponentRef`, `PinHandle`, `NetRef`, `LayerRef`: typed handles.
- `ExportFormat`, `ExportBundle`, `ExportFile`: export interface.
- `ApiError` / `ApiResult<T>`: structured API errors.

## Typical usage

```rust
use papan_api::{ExportFormat, PapanProject};
use papan_ir::Length;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut project = PapanProject::compile(r#"
board "Demo"
size 50mm x 30mm
component R1: resistor { value: 10kΩ }
component R2: resistor { value: 10kΩ }
net N1
net N2
connect R1.1 -> N1
connect R2.1 -> N1
connect R1.2 -> N2
connect R2.2 -> N2
place R1 at (15mm, 15mm)
place R2 at (35mm, 15mm)
"#)?;

    project.board().autoroute()?;
    let report = project.validate();
    if !report.production_ready {
        eprintln!("{:?}", report.diagnostics);
    }

    let bundle = project.export(ExportFormat::Manufacturing)?;
    bundle.write_to_dir("output")?;
    Ok(())
}
```

## Imperative edits

Use `commit_ir` for IR-level mutations. The API regenerates canonical source/graph projections from the committed IR:

```rust
use papan_api::PapanProject;
use papan_types::Quantity;

let mut project = PapanProject::with_title("Demo");
let mut board = project.board();
let r1 = board.add_component("resistor")?;
board.set_value(&r1, Quantity::parse_literal("4.7kΩ").unwrap())?;
```

`Board` methods internally commit transactional changes. Graph edits can be applied with `edit_graph` / `commit_graph`.

## Formats

`ExportFormat` supports source, graph, document JSON, IR JSON, Gerber, drill, BOM, pick-and-place, manufacturing bundle, and KiCad.

## API behavior

- operations are deterministic for identical inputs
- diagnostics/errors are structured
- no GUI state is mixed into the core API
- exports can be blocked or warned by validation depending on caller/CLI options; manufacturing validation remains visible in diagnostics
