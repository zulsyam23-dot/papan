# PAPAN Language Specification (Implemented Subset)

PAPAN source files use `.pn`. The compiler also supports `.graph` (JSON node graph) and `.papan` project manifests/directories.

## Design goals

- deterministic parsing and compilation
- explicit semantic declarations
- physical units where appropriate
- stable diagnostics with source spans

## Core declarations

```papan
board "Name"
size 100mm x 80mm
thickness 1.6mm
layers 2

component R1: resistor { value: 10kΩ }
component U1: "ATmega32U4"

net VCC { type: power voltage: 5V }
net GND { type: ground }

connect R1.1 -> VCC
place R1 at (20mm, 20mm)
place U1 at board.top.center
place R2 near R1 distance 5mm
rotate R1 90deg
```

## Supported constructs

- board metadata: `board`, `size`, `thickness`, `layers`
- components: named kinds (`resistor`, `capacitor`, `ic`, etc.) and quoted library parts (`"ATmega32U4"`, `"USB-C"`)
- nets with properties (`type`, `voltage`)
- connectivity: `connect Component.Pin -> Net`
- placement: absolute coordinates, board anchors, relative `near ... distance`, `rotate`
- routing directives and constraints:
  - `route <net> { width: 0.25mm layer: F.Cu differential_pair: false length: 20mm impedance: 50Ω }`
  - `via { drill: 0.30mm diameter: 0.60mm }` (updates default via constraints)
  - `constraints { min_trace_width: 0.15mm clearance: 0.20mm ... }`
- modules (namespaced lowering)
- `check DRC`, `check ERC`, `export gerber`/etc. are recorded as semantic commands

## Units

Lengths use `mm`, `mil`, or `inch` (parsed into fixed-point nanometres internally). Angles use `deg`. Electrical quantities use units such as `Ω`, `kΩ`, `uF`, `nH`, `V`, `A`.

## Diagnostics

Diagnostics are structured and stable. Prefix families include `LEX_`, `PARSE_`, `SEM_`, `IR_`, `ERC_`, `DRC_`, `ROUTE_`, `MFG_`, `GRAPH_`, `KICAD_`, `DOC_`, `API_`, `CLI_`.

## Limitations

The language subset is implemented to support deterministic compilation and manufacturing workflows. Advanced features from the PRD (simulation nodes/directives, full hierarchical module port semantics, complete constraint taxonomy) may be parsed/projected conservatively or reported as unsupported/limited.
