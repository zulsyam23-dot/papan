# Manufacturing

PAPAN generates manufacturing artifacts from PIR without external EDA runtime dependencies.

## Outputs

`papan-manufacturing` exports:

- **Gerber** (`*.gbr`): RS-274X-style copper/mechanical layers; filenames normalize layer names (for example `F.Cu` → `F_Cu.gbr`, `Edge.Cuts` → `Edge_Cuts.gbr`).
- **Excellon drill** (`drill.drl`): vias and through-hole drills.
- **BOM** (`bom.csv`): header `Designator,Value,Part,Category,Footprint,Quantity,Manufacturer`.
- **Pick-and-place** (`pick_and_place.csv`): header `Designator,Mid X (mm),Mid Y (mm),Layer,Rotation,Value,Package`.

## Validation

Manufacturing validation reports `MFG_*` diagnostics for missing outline/layers, missing placement, trace/via manufacturing constraint violations, and unrouted nets (`MFG_030`) among other checks.

## CLI/API export

```powershell
papan build myboard.papan --target manufacturing
papan export myboard.papan --format gerber
papan export myboard.papan --format drill
papan export myboard.papan --format bom
papan export myboard.papan --format pick-and-place
```

API:

```rust
use papan_api::{ExportFormat, PapanProject};
# fn main() -> papan_api::ApiResult<()> {
let project = PapanProject::open("myboard.papan")?;
let bundle = project.export(ExportFormat::Manufacturing)?;
bundle.write_to_dir("output")?;
# Ok(()) }
```

## Production readiness

The CLI blocks physical builds with validation errors unless `--force` is supplied. It may auto-route physical targets when traces are absent. Diagnostics should be reviewed before fabrication; exported files are deterministic representations of the current PIR.
