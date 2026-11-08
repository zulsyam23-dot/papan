# PCB Engine

The PCB engine operates on PIR and is deterministic.

## Geometry and placement

- fixed-point geometry primitives (`Point`, `Segment`, `Rect`, polygon utilities)
- pad/component world positions derived from definitions, placement, side, and rotation
- deterministic auto-placement for components without explicit coordinates
- collision/outline checks used by DRC and manufacturing validation

## Electrical engine

`papan-electrical` computes connectivity from net membership and copper geometry, detects unrouted nets/unconnected pins, and runs ERC. Common codes include `ERC_010` (unconnected pin), `ERC_030` (unrouted/disconnected groups in connectivity checks), and pin-type/net-type conflict checks.

## Routing

`papan-routing` provides a deterministic autorouter:

- Manhattan/L-shaped path candidates
- width from constraints/directives (clamped below minimum with `ROUTE_012`)
- simple obstacle awareness around foreign pads/geometry where implemented
- layer assignment and via insertion when required
- clears/rebuilds routing data as requested

## DRC

`papan-drc` reports stable `DRC_*` diagnostics for trace width/layer/geometry, via geometry, clearances, edge clearance, drill spacing, unrouted nets (`DRC_150`), and placement issues. Reports can distinguish errors/warnings and production readiness.

## Limitations

- routing is intentionally conservative; not a full push-and-shove or global optimizer
- advanced multilayer strategies, differential pairs, and length matching are limited
- validation quality depends on available footprint/library geometry
