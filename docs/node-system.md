# Node System

The node graph is a typed projection of the PIR, not a second authoritative model.

## Concepts

- **Node kinds** are canonicalized (board, component, net, connect, place, route, rule, export, etc.).
- **Ports** are typed and directional (`PortType`, `PortDirection`). Connections are validated by graph rules.
- **Parameters** carry typed values (`ParamValue`: quantities, lengths, angles, strings, identifiers, booleans, integers, arrays).
- **Stable IDs** identify nodes, connections, and groups.

## Validation

`NodeGraph::validate()` returns stable `GRAPH_*` diagnostics for schema issues, duplicate IDs/ports, missing references, invalid typed connections, and unsupported/ill-formed projections.

## Synchronization

- `NodeGraph::from_ir(&PcbIr)` projects semantic design data into nodes.
- `NodeGraph::to_source()` emits deterministic canonical `.pn` source.
- `NodeGraph::project_ir_to_source(&PcbIr)` is used by the document layer for IR-to-source projection.
- `PapanProject::edit_graph` / `PapanDocument::commit_graph` apply graph edits transactionally: validate, project to source, compile, commit.

## Graph I/O

Graphs serialize as JSON (`.graph`) and preserve schema version, metadata, nodes, connections, and groups. `auto_layout()` and `diff()` provide deterministic layout/diff utilities.

## Limitations

The graph can represent more node kinds than the current source subset can project. Unsupported nodes are preserved/validated as appropriate but may be skipped from source projection with informational diagnostics rather than becoming new language syntax.
