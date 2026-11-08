use std::collections::{BTreeMap, BTreeSet};

use papan_backend::{BackendError, BackendImport, BackendInput, BackendResult};
use papan_geometry::{Polygon, Size};
use papan_ir::{
    Angle, ComponentCategory, ComponentDefinition, ComponentInstance, FootprintDefinition, Length,
    Net, NetKind, ObjectId, PadDefinition, PadKind, PadLayers, PadShape, PcbIr, PinDefinition,
    PinRef, PinType, Point, PropValue, Quantity, Side, Stackup, Trace, Via,
};
use papan_types::{Diagnostic, Diagnostics};

use crate::sexpr::{parse_sexpr, SExpr};

// Import
// ---------------------------------------------------------------------------

/// Import a KiCad PCB subset into the PIR.
pub fn import_kicad(input: &BackendInput) -> BackendResult<BackendImport> {
    let expressions = parse_sexpr(&input.content)?;
    let root = expressions
        .iter()
        .find(|expr| expr.head().map(|head| head.eq_ignore_ascii_case("kicad_pcb")).unwrap_or(false))
        .ok_or_else(|| {
            BackendError::new(
                "KICAD_005",
                "input is not a KiCad PCB file (missing kicad_pcb root)",
            )
        })?;

    let mut diagnostics = Diagnostics::new();
    let name = input
        .path
        .as_ref()
        .and_then(|path| path.file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or(if input.name.is_empty() {
            "KiCad Import"
        } else {
            &input.name
        })
        .to_string();
    let mut ir = PcbIr::new(name.clone());
    ir.metadata.title = name.clone();

    // Title block.
    if let Some(title) = root.child("title_block").and_then(|block| block.child("title")) {
        if let Some(value) = title.list().and_then(|values| values.get(1)).and_then(SExpr::atom) {
            if !value.is_empty() {
                ir.metadata.title = value.to_string();
                ir.board.name = value.to_string();
            }
        }
    }

    // Board thickness.
    if let Some(thickness) = root
        .child("general")
        .and_then(|general| general.child("thickness"))
        .and_then(expr_number)
    {
        if thickness > 0.0 {
            ir.board.thickness = Length::from_mm(thickness);
        }
    }

    // Copper layer count.
    let mut copper_names: Vec<String> = Vec::new();
    if let Some(layers) = root.child("layers") {
        if let Some(values) = layers.list() {
            for entry in values.iter().skip(1) {
                let Some(entry_values) = entry.list() else {
                    continue;
                };
                let Some(layer_name) = entry_values.get(1).and_then(SExpr::atom) else {
                    continue;
                };
                let kind = entry_values.get(2).and_then(SExpr::atom).unwrap_or("");
                if kind.eq_ignore_ascii_case("signal")
                    || layer_name.ends_with(".Cu")
                {
                    copper_names.push(layer_name.to_string());
                }
            }
        }
    }
    let layer_count = copper_names.len().max(1) as u32;
    ir.board.layer_count = layer_count;
    ir.stackup = Stackup::for_copper_count(layer_count);

    // Nets.
    let mut nets_by_number: BTreeMap<u32, ObjectId> = BTreeMap::new();
    let mut net_by_name: BTreeMap<String, ObjectId> = BTreeMap::new();
    for net_expr in root.children("net") {
        let values = net_expr.list().unwrap_or(&[]);
        let Some(number_atom) = values.get(1).and_then(SExpr::atom) else {
            continue;
        };
        let Ok(number) = number_atom.parse::<u32>() else {
            continue;
        };
        if number == 0 {
            continue;
        }
        let name = values
            .get(2)
            .and_then(SExpr::atom)
            .map(str::to_string)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("Net-{number}"));
        let id = net_by_name
            .get(&name)
            .cloned()
            .unwrap_or_else(|| ObjectId::stable("net", &name));
        net_by_name.insert(name.clone(), id.clone());
        nets_by_number.insert(number, id.clone());
        if ir.find_net_by_id(&id).is_none() {
            ir.add_net(Net {
                id,
                name,
                kind: NetKind::Other,
                voltage: None,
                members: Vec::new(),
                properties: BTreeMap::new(),
                source_span: None,
            });
        }
    }

    let origin = board_origin(root);
    let (width, height, outline) = board_outline(root, &mut diagnostics);
    ir.board.width = width;
    ir.board.height = height;
    ir.board.outline = outline;
    if ir.board.outline.points.is_empty() {
        ir.board.width = Length::from_mm(100.0);
        ir.board.height = Length::from_mm(80.0);
        ir.board.outline = Polygon::rect(ir.board.width, ir.board.height);
        diagnostics.push(Diagnostic::warning(
            "KICAD_010",
            "no Edge.Cuts outline found; using a 100x80mm board",
        ));
    }

    let mut definitions: BTreeMap<String, ComponentDefinition> = BTreeMap::new();
    let mut designators: BTreeSet<String> = BTreeSet::new();
    let mut index = 0usize;
    for footprint in root.children("footprint") {
        index += 1;
        let values = footprint.list().unwrap_or(&[]);
        let footprint_name = values
            .get(1)
            .and_then(SExpr::atom)
            .unwrap_or("generic")
            .to_string();
        let lib_name = footprint_name
            .split(':')
            .next_back()
            .unwrap_or(&footprint_name)
            .to_string();
        let at = footprint.child("at").map(expr_numbers).unwrap_or_default();
        let raw_position = Point::new(
            Length::from_mm(at.first().copied().unwrap_or(0.0)),
            Length::from_mm(at.get(1).copied().unwrap_or(0.0)),
        );
        let position = Point::new(
            raw_position.x - origin.x,
            raw_position.y - origin.y,
        );
        let rotation = Angle::from_deg(at.get(2).copied().unwrap_or(0.0));
        let layer = footprint
            .child("layer")
            .and_then(|layer| layer.list())
            .and_then(|values| values.get(1))
            .and_then(SExpr::atom)
            .unwrap_or("F.Cu");
        let side = if layer.eq_ignore_ascii_case("B.Cu") {
            Side::Bottom
        } else {
            Side::Top
        };
        let designator = footprint
            .children("property")
            .into_iter()
            .find_map(property_pair("Reference"))
            .or_else(|| {
                footprint
                    .child("fp_text")
                    .and_then(|text| text.list())
                    .and_then(|values| {
                        let head = values.get(1).and_then(SExpr::atom)?;
                        if head.eq_ignore_ascii_case("reference") {
                            values.get(2).and_then(SExpr::atom).map(str::to_string)
                        } else {
                            None
                        }
                    })
            })
            .unwrap_or_else(|| format!("U{index}"));
        let designator = unique_designator(&designator, &mut designators);
        let value_text = footprint
            .children("property")
            .into_iter()
            .find_map(property_pair("Value"))
            .or_else(|| {
                footprint
                    .child("fp_text")
                    .and_then(|text| text.list())
                    .and_then(|values| {
                        let head = values.get(1).and_then(SExpr::atom)?;
                        if head.eq_ignore_ascii_case("value") {
                            values.get(2).and_then(SExpr::atom).map(str::to_string)
                        } else {
                            None
                        }
                    })
            });

        let definition_id = ObjectId::stable("footprint", &lib_name);
        let definition = definitions
            .entry(lib_name.clone())
            .or_insert_with(|| ComponentDefinition {
                id: definition_id.clone(),
                name: lib_name.clone(),
                category: ComponentCategory::Other,
                pins: Vec::new(),
                footprint: FootprintDefinition {
                    name: lib_name.clone(),
                    pads: Vec::new(),
                    description: String::new(),
                },
                manufacturer: None,
                datasheet: None,
                description: String::new(),
            });

        let is_new_definition = definition.footprint.pads.is_empty();
        let mut pad_index = 0usize;
        for pad in footprint.children("pad") {
            pad_index += 1;
            let pad_values = pad.list().unwrap_or(&[]);
            let number = pad_values
                .get(1)
                .and_then(SExpr::atom)
                .map(str::to_string)
                .filter(|number| !number.is_empty())
                .unwrap_or_else(|| pad_index.to_string());
            let kind_text = pad_values.get(2).and_then(SExpr::atom).unwrap_or("smd");
            let shape_text = pad_values.get(3).and_then(SExpr::atom).unwrap_or("rect");
            let pad_at = pad.child("at").map(expr_numbers).unwrap_or_default();
            let pad_position = Point::from_mm(
                pad_at.first().copied().unwrap_or(0.0),
                pad_at.get(1).copied().unwrap_or(0.0),
            );
            let size = pad.child("size").map(expr_numbers).unwrap_or_default();
            let pad_size = Size::from_mm(
                size.first().copied().unwrap_or(0.5),
                size.get(1).copied().unwrap_or(size.first().copied().unwrap_or(0.5)),
            );
            let drill = pad.child("drill").and_then(expr_number).map(Length::from_mm);
            let is_through_hole =
                kind_text.eq_ignore_ascii_case("thru_hole") || kind_text.eq_ignore_ascii_case("np_thru_hole");
            let pad_kind = if is_through_hole {
                PadKind::ThroughHole
            } else {
                PadKind::Smd
            };
            let pad_shape = if shape_text.eq_ignore_ascii_case("circle")
                || shape_text.eq_ignore_ascii_case("oval")
            {
                PadShape::Round
            } else {
                PadShape::Rect
            };
            let pad_layers = {
                let texts: Vec<&str> = pad
                    .child("layers")
                    .map(SExpr::atoms)
                    .unwrap_or_default();
                if texts.iter().any(|text| text.contains("*")) {
                    PadLayers::Both
                } else if texts.iter().any(|text| text.eq_ignore_ascii_case("B.Cu")) {
                    PadLayers::Bottom
                } else {
                    PadLayers::Top
                }
            };
            if is_new_definition {
                if !definition.pins.iter().any(|pin| pin.number == number) {
                    definition.pins.push(PinDefinition {
                        name: number.clone(),
                        number: number.clone(),
                        electrical: PinType::Passive,
                        description: String::new(),
                    });
                }
                definition.footprint.pads.push(PadDefinition {
                    number: number.clone(),
                    pin: number.clone(),
                    kind: pad_kind,
                    shape: pad_shape,
                    position: pad_position,
                    size: pad_size,
                    drill,
                    layers: pad_layers,
                });
            }

            // Pad-local net association.
            if let Some(net_number) = pad.child("net").and_then(expr_number_u32) {
                if net_number != 0 {
                    let net_id = nets_by_number.get(&net_number).cloned();
                    if let Some(net_id) = net_id {
                        let pin = PinRef::new(
                            ObjectId::stable("component", &designator),
                            number.clone(),
                        );
                        ir.connect(pin, net_id, None);
                    }
                }
            }
        }
        ir.add_definition(definition.clone());

        let value = value_text.as_ref().and_then(|value| {
            Quantity::parse_literal(value)
                .ok()
                .filter(|quantity| quantity.kind != papan_types::UnitKind::Dimensionless)
        });
        let mut properties = BTreeMap::new();
        if value.is_none() {
            if let Some(value) = value_text {
                if !value.is_empty() {
                    properties.insert("value".to_string(), PropValue::String(value));
                }
            }
        }
        ir.components.push(ComponentInstance {
            id: ObjectId::stable("component", &designator),
            designator,
            definition_id: definition_id.clone(),
            value,
            properties,
            position: Some(position),
            rotation,
            side,
            locked: false,
            source_span: None,
        });
    }

    // Track segments.
    for segment in root.children("segment") {
        let Some(start) = segment.child("start") else {
            continue;
        };
        let Some(end) = segment.child("end") else {
            continue;
        };
        let start_values = expr_numbers(start);
        let end_values = expr_numbers(end);
        let width = segment
            .child("width")
            .and_then(expr_number)
            .unwrap_or(0.25);
        let layer_name = segment
            .child("layer")
            .and_then(expr_atom_at_1)
            .unwrap_or_else(|| "F.Cu".to_string());
        let (Some(net_number), Some(layer)) = (
            segment.child("net").and_then(expr_number_u32),
            layer_id_for(&ir, &layer_name, &mut diagnostics),
        ) else {
            diagnostics.push(Diagnostic::warning(
                "KICAD_011",
                "track segment without a resolvable net was skipped",
            ));
            continue;
        };
        let Some(net_id) = nets_by_number.get(&net_number).cloned() else {
            diagnostics.push(Diagnostic::warning(
                "KICAD_012",
                format!("track segment references unknown net number {net_number}"),
            ));
            continue;
        };
        let a = Point::new(
            Length::from_mm(start_values.first().copied().unwrap_or(0.0)) - origin.x,
            Length::from_mm(start_values.get(1).copied().unwrap_or(0.0)) - origin.y,
        );
        let b = Point::new(
            Length::from_mm(end_values.first().copied().unwrap_or(0.0)) - origin.x,
            Length::from_mm(end_values.get(1).copied().unwrap_or(0.0)) - origin.y,
        );
        let id = ObjectId::new(format!(
            "trace:import:{}",
            ir.traces.len() + 1
        ));
        ir.traces.push(Trace {
            id,
            net: net_id,
            layer,
            width: Length::from_mm(width),
            points: vec![a, b],
            source_span: None,
        });
    }

    // Track arcs (KiCad 7/8: start/mid/end).
    for arc in root.children("arc") {
        let (Some(start), Some(mid), Some(end)) = (arc.child("start"), arc.child("mid"), arc.child("end")) else {
            diagnostics.push(Diagnostic::warning(
                "KICAD_011",
                "track arc missing start/mid/end was skipped",
            ));
            continue;
        };
        let start_v = expr_numbers(start);
        let mid_v = expr_numbers(mid);
        let end_v = expr_numbers(end);
        let width = arc.child("width").and_then(expr_number).unwrap_or(0.25);
        let layer_name = arc
            .child("layer")
            .and_then(expr_atom_at_1)
            .unwrap_or_else(|| "F.Cu".to_string());
        let (Some(net_number), Some(layer)) = (
            arc.child("net").and_then(expr_number_u32),
            layer_id_for(&ir, &layer_name, &mut diagnostics),
        ) else {
            diagnostics.push(Diagnostic::warning(
                "KICAD_011",
                "track arc without a resolvable net was skipped",
            ));
            continue;
        };
        let Some(net_id) = nets_by_number.get(&net_number).cloned() else {
            diagnostics.push(Diagnostic::warning(
                "KICAD_012",
                format!("track arc references unknown net number {net_number}"),
            ));
            continue;
        };
        let s = to_board_point(&start_v, origin);
        let m = to_board_point(&mid_v, origin);
        let e = to_board_point(&end_v, origin);
        let points = tessellate_arc(s, m, e, 16);
        let id = ObjectId::new(format!("trace:import:{}", ir.traces.len() + 1));
        ir.traces.push(Trace {
            id,
            net: net_id,
            layer,
            width: Length::from_mm(width),
            points,
            source_span: None,
        });
    }

    // Vias.
    for via in root.children("via") {
        let Some(at) = via.child("at") else {
            continue;
        };
        let values = expr_numbers(at);
        let position = Point::new(
            Length::from_mm(values.first().copied().unwrap_or(0.0)) - origin.x,
            Length::from_mm(values.get(1).copied().unwrap_or(0.0)) - origin.y,
        );
        let diameter = via.child("size").and_then(expr_number).unwrap_or(0.6);
        let drill = via.child("drill").and_then(expr_number).unwrap_or(0.3);
        let net_number = via.child("net").and_then(expr_number_u32).unwrap_or(0);
        let Some(net_id) = nets_by_number.get(&net_number).cloned() else {
            diagnostics.push(Diagnostic::warning(
                "KICAD_013",
                "via without a resolvable net was skipped",
            ));
            continue;
        };
        let copper = ir.copper_layers();
        let from_layer = copper
            .first()
            .map(|layer| layer.id.clone())
            .unwrap_or_else(|| ObjectId::stable("layer", "F.Cu"));
        let to_layer = copper
            .last()
            .map(|layer| layer.id.clone())
            .unwrap_or_else(|| ObjectId::stable("layer", "B.Cu"));
        let id = ObjectId::new(format!("via:import:{}", ir.vias.len() + 1));
        ir.vias.push(Via {
            id,
            net: net_id,
            at: position,
            drill: Length::from_mm(drill),
            diameter: Length::from_mm(diameter),
            from_layer,
            to_layer,
            source_span: None,
        });
    }

    // Targeted diagnostics for graphical shapes (Edge.Cuts outline shapes are handled).
    let mut gr_arc_graphical = 0usize;
    for gr_arc in root.children("gr_arc") {
        if !is_edge_cuts(gr_arc) {
            gr_arc_graphical += 1;
        }
    }
    let mut gr_circle_graphical = 0usize;
    for gr_circle in root.children("gr_circle") {
        if !is_edge_cuts(gr_circle) {
            gr_circle_graphical += 1;
        }
    }
    let gr_graphical = gr_arc_graphical + gr_circle_graphical;
    if gr_graphical > 0 {
        diagnostics.push(
            Diagnostic::warning(
                "KICAD_013",
                format!(
                    "{gr_graphical} graphical element(s) (gr_arc/gr_circle, non-Edge.Cuts) not imported; Edge.Cuts arcs/circles are tessellated into the board outline"
                ),
            )
            .with_suggestion("verify graphical/cosmetic layers if needed"),
        );
    }
    let mut fp_shape_count = 0usize;
    for footprint in root.children("footprint") {
        fp_shape_count += footprint.children("fp_arc").len();
        fp_shape_count += footprint.children("fp_circle").len();
    }
    if fp_shape_count > 0 {
        diagnostics.push(
            Diagnostic::warning(
                "KICAD_013",
                format!("{fp_shape_count} footprint graphic element(s) (fp_arc/fp_circle) not imported (footprint graphics not modeled)"),
            )
            .with_suggestion("verify footprint geometry/silkscreen in KiCad if needed"),
        );
    }

    // Conservative subset: warn when major KiCad features may be dropped.
    let src = &input.content;
    let mut unsupported = Vec::new();
    if src.contains("(zone") { unsupported.push("zone (copper pour/keepout)"); }
    if src.contains("(dimension") { unsupported.push("dimension"); }
    if src.contains("(group") { unsupported.push("group"); }
    if src.contains("(rule_area") || src.contains("(keepout") { unsupported.push("rule_area/keepout"); }
    if !unsupported.is_empty() {
        diagnostics.push(
            Diagnostic::warning(
                "KICAD_012",
                format!(
                    "KiCad import is a conservative subset; elements may not be fully imported: {}",
                    unsupported.join(", ")
                ),
            )
            .with_suggestion("verify the imported board; use KiCad-native workflow for full fidelity"),
        );
    }

    // Keep definitions deterministic.
    ir.definitions.sort_by(|a, b| a.id.cmp(&b.id));
    diagnostics.sort();
    Ok(BackendImport { ir, diagnostics })
}

fn property_pair<'a>(key: &'a str) -> impl Fn(&SExpr) -> Option<String> + 'a {
    move |expr: &SExpr| {
        let values = expr.list()?;
        let head = values.first()?.atom()?;
        if !head.eq_ignore_ascii_case("property") {
            return None;
        }
        let name = values.get(1)?.atom()?;
        if name.eq_ignore_ascii_case(key) {
            values.get(2)?.atom().map(str::to_string)
        } else {
            None
        }
    }
}

fn expr_atom_at_1(expr: &SExpr) -> Option<String> {
    expr.list()?.get(1)?.atom().map(str::to_string)
}

fn is_edge_cuts(expr: &SExpr) -> bool {
    expr.child("layer")
        .and_then(expr_atom_at_1)
        .map(|layer| layer.eq_ignore_ascii_case("Edge.Cuts"))
        .unwrap_or(false)
}

fn expr_number(expr: &SExpr) -> Option<f64> {
    expr.list()?
        .iter()
        .skip(1)
        .find_map(|value| value.atom().and_then(|atom| parse_number(atom)))
}

fn expr_numbers(expr: &SExpr) -> Vec<f64> {
    expr.atoms()
        .iter()
        .filter_map(|atom| parse_number(atom))
        .collect()
}

fn expr_number_u32(expr: &SExpr) -> Option<u32> {
    expr.list()?
        .iter()
        .skip(1)
        .find_map(|value| value.atom().and_then(|atom| atom.parse::<u32>().ok()))
}

fn parse_number(atom: &str) -> Option<f64> {
    let trimmed = atom.trim_end_matches(|ch: char| ch.is_ascii_alphabetic());
    trimmed.parse::<f64>().ok()
}

fn unique_designator(base: &str, seen: &mut BTreeSet<String>) -> String {
    let mut candidate = base.to_string();
    let mut counter = 2usize;
    while !seen.insert(candidate.clone()) {
        candidate = format!("{base}_{counter}");
        counter += 1;
    }
    candidate
}

fn board_origin(root: &SExpr) -> Point {
    let bounds = outline_bounds(root);
    match bounds {
        Some((min_x, min_y, _, _)) => Point::from_mm(min_x, min_y),
        None => Point::ORIGIN,
    }
}

fn outline_bounds(root: &SExpr) -> Option<(f64, f64, f64, f64)> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    let mut add = |x: f64, y: f64| {
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((min_x, min_y, max_x, max_y)) => (
                min_x.min(x),
                min_y.min(y),
                max_x.max(x),
                max_y.max(y),
            ),
        });
    };
    for rect in root.children("gr_rect").into_iter().chain(root.children("gr_bbox")) {
        if !is_edge_cuts(rect) {
            continue;
        }
        let Some(start) = rect.child("start") else { continue };
        let Some(end) = rect.child("end") else { continue };
        let start_values = expr_numbers(start);
        let end_values = expr_numbers(end);
        if let (Some(sx), Some(sy), Some(ex), Some(ey)) = (
            start_values.first(),
            start_values.get(1),
            end_values.first(),
            end_values.get(1),
        ) {
            add(*sx, *sy);
            add(*ex, *ey);
        }
    }
    for line in root.children("gr_line") {
        if !is_edge_cuts(line) {
            continue;
        }
        for node in ["start", "end"] {
            if let Some(child) = line.child(node) {
                let values = expr_numbers(child);
                if let (Some(&x), Some(&y)) = (values.first(), values.get(1)) {
                    add(x, y);
                }
            }
        }
    }
    for poly in root.children("gr_poly") {
        if !is_edge_cuts(poly) {
            continue;
        }
        for (x, y) in gr_poly_points_mm(poly) {
            add(x, y);
        }
    }
    for circle in root.children("gr_circle") {
        if !is_edge_cuts(circle) {
            continue;
        }
        for (x, y) in gr_circle_points_mm(circle, 32) {
            add(x, y);
        }
    }
    for arc in root.children("gr_arc") {
        if !is_edge_cuts(arc) {
            continue;
        }
        for (x, y) in arc_points_mm(arc) {
            add(x, y);
        }
    }
    bounds
}

fn board_outline(root: &SExpr, diagnostics: &mut Diagnostics) -> (Length, Length, Polygon) {
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut rect: Option<(f64, f64, f64, f64)> = None;
    for gr_rect in root.children("gr_rect") {
        if !is_edge_cuts(gr_rect) {
            continue;
        }
        let (Some(start), Some(end)) = (gr_rect.child("start"), gr_rect.child("end")) else {
            continue;
        };
        let start_values = expr_numbers(start);
        let end_values = expr_numbers(end);
        if let (Some(sx), Some(sy), Some(ex), Some(ey)) = (
            start_values.first(),
            start_values.get(1),
            end_values.first(),
            end_values.get(1),
        ) {
            rect = Some((*sx, *sy, *ex, *ey));
            points.push((*sx, *sy));
            points.push((*ex, *sy));
            points.push((*ex, *ey));
            points.push((*sx, *ey));
        }
    }
    let mut edges: Vec<(Point, Point)> = Vec::new();
    for gr_line in root.children("gr_line") {
        if !is_edge_cuts(gr_line) {
            continue;
        }
        let (Some(start), Some(end)) = (gr_line.child("start"), gr_line.child("end")) else {
            continue;
        };
        let start_values = expr_numbers(start);
        let end_values = expr_numbers(end);
        if let (Some(sx), Some(sy), Some(ex), Some(ey)) = (
            start_values.first(),
            start_values.get(1),
            end_values.first(),
            end_values.get(1),
        ) {
            let a = Point::from_mm(*sx, *sy);
            let b = Point::from_mm(*ex, *ey);
            points.push((*sx, *sy));
            points.push((*ex, *ey));
            edges.push((a, b));
        }
    }
    for gr_arc in root.children("gr_arc") {
        if !is_edge_cuts(gr_arc) {
            continue;
        }
        let pts = arc_points_mm(gr_arc);
        for (x, y) in &pts {
            points.push((*x, *y));
        }
        for pair in pts.windows(2) {
            edges.push((Point::from_mm(pair[0].0, pair[0].1), Point::from_mm(pair[1].0, pair[1].1)));
        }
    }
    for gr_poly in root.children("gr_poly") {
        if !is_edge_cuts(gr_poly) {
            continue;
        }
        let pts = gr_poly_points_mm(gr_poly);
        if pts.len() < 2 {
            continue;
        }
        for (x, y) in &pts {
            points.push((*x, *y));
        }
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            edges.push((Point::from_mm(a.0, a.1), Point::from_mm(b.0, b.1)));
        }
    }
    for gr_circle in root.children("gr_circle") {
        if !is_edge_cuts(gr_circle) {
            continue;
        }
        let pts = gr_circle_points_mm(gr_circle, 32);
        if pts.len() < 2 {
            continue;
        }
        for (x, y) in &pts {
            points.push((*x, *y));
        }
        for pair in pts.windows(2) {
            edges.push((Point::from_mm(pair[0].0, pair[0].1), Point::from_mm(pair[1].0, pair[1].1)));
        }
    }
    let Some((min_x, min_y, max_x, max_y)) = points.iter().fold(
        None::<(f64, f64, f64, f64)>,
        |bounds, (x, y)| match bounds {
            None => Some((*x, *y, *x, *y)),
            Some((min_x, min_y, max_x, max_y)) => Some((
                min_x.min(*x),
                min_y.min(*y),
                max_x.max(*x),
                max_y.max(*y),
            )),
        },
    ) else {
        return (Length::ZERO, Length::ZERO, Polygon::new(Vec::new()));
    };
    let width = Length::from_mm(max_x - min_x);
    let height = Length::from_mm(max_y - min_y);
    let outline = if edges.len() >= 3 {
        let mut ordered = order_edges(edges);
        if ordered.len() < 3 {
            if let Some((sx, sy, ex, ey)) = rect {
                let origin = Point::from_mm(sx.min(ex) - min_x, sy.min(ey) - min_y);
                Polygon::rect_at(origin, Length::from_mm((ex - sx).abs()), Length::from_mm((ey - sy).abs()))
            } else {
                diagnostics.push(Diagnostic::warning(
                    "KICAD_014",
                    "Edge.Cuts geometry is not a closed loop; using its bounding box",
                ));
                Polygon::rect(width, height)
            }
        } else {
            for point in &mut ordered {
                *point = Point::new(
                    point.x - Length::from_mm(min_x),
                    point.y - Length::from_mm(min_y),
                );
            }
            Polygon::new(ordered)
        }
    } else if let Some((sx, sy, ex, ey)) = rect {
        let origin = Point::from_mm(sx.min(ex) - min_x, sy.min(ey) - min_y);
        Polygon::rect_at(origin, Length::from_mm((ex - sx).abs()), Length::from_mm((ey - sy).abs()))
    } else {
        Polygon::rect(width, height)
    };
    (width, height, outline)
}

fn order_edges(edges: Vec<(Point, Point)>) -> Vec<Point> {
    if edges.is_empty() {
        return Vec::new();
    }
    let mut remaining = edges;
    let first = remaining.remove(0);
    let mut points = vec![first.0, first.1];
    let mut cursor = first.1;
    while !remaining.is_empty() {
        let Some(index) = remaining.iter().position(|(a, b)| {
            length_close(*a, cursor) || length_close(*b, cursor)
        }) else {
            break;
        };
        let (a, b) = remaining.remove(index);
        let next = if length_close(a, cursor) { b } else { a };
        if length_close(next, points[0]) {
            break;
        }
        points.push(next);
        cursor = next;
    }
    points
}

fn length_close(a: Point, b: Point) -> bool {
    a.distance(b) < Length::from_um(1.0)
}

fn to_board_point(values: &[f64], origin: Point) -> Point {
    Point::new(
        Length::from_mm(values.first().copied().unwrap_or(0.0)) - origin.x,
        Length::from_mm(values.get(1).copied().unwrap_or(0.0)) - origin.y,
    )
}

fn tessellate_arc(start: Point, mid: Point, end: Point, segments: usize) -> Vec<Point> {
    let s = (start.x.0 as f64, start.y.0 as f64);
    let m = (mid.x.0 as f64, mid.y.0 as f64);
    let e = (end.x.0 as f64, end.y.0 as f64);
    let Some((cx, cy, radius, a0, a1)) = arc_params(s, m, e) else {
        return vec![start, end];
    };
    let segments = segments.max(2);
    let mut points = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let t = i as f64 / segments as f64;
        let a = a0 + (a1 - a0) * t;
        points.push(Point::new(
            Length((cx + radius * a.cos()).round() as i64),
            Length((cy + radius * a.sin()).round() as i64),
        ));
    }
    // Ensure exact endpoints.
    if let Some(first) = points.first_mut() {
        *first = start;
    }
    if let Some(last) = points.last_mut() {
        *last = end;
    }
    points
}

fn arc_points_mm(arc: &SExpr) -> Vec<(f64, f64)> {
    let (Some(start), Some(end)) = (arc.child("start"), arc.child("end")) else {
        return Vec::new();
    };
    let sv = expr_numbers(start);
    let ev = expr_numbers(end);
    let (Some(&sx), Some(&sy), Some(&ex), Some(&ey)) =
        (sv.first(), sv.get(1), ev.first(), ev.get(1))
    else {
        return Vec::new();
    };
    let Some(mid) = arc.child("mid") else {
        return vec![(sx, sy), (ex, ey)];
    };
    let mv = expr_numbers(mid);
    let (Some(&mx), Some(&my)) = (mv.first(), mv.get(1)) else {
        return vec![(sx, sy), (ex, ey)];
    };
    tessellate_arc_mm((sx, sy), (mx, my), (ex, ey), 16)
}

fn gr_poly_points_mm(poly: &SExpr) -> Vec<(f64, f64)> {
    let Some(pts) = poly.child("pts") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for xy in pts.children("xy") {
        let values = expr_numbers(xy);
        if let (Some(&x), Some(&y)) = (values.first(), values.get(1)) {
            out.push((x, y));
        }
    }
    if out.is_empty() {
        // Fallback: flat pairs (older/simple forms)
        let values = expr_numbers(pts);
        for pair in values.chunks(2) {
            if pair.len() == 2 {
                out.push((pair[0], pair[1]));
            }
        }
    }
    out
}

fn gr_circle_points_mm(circle: &SExpr, segments: usize) -> Vec<(f64, f64)> {
    let (Some(center), Some(end)) = (circle.child("center"), circle.child("end")) else {
        return Vec::new();
    };
    let cv = expr_numbers(center);
    let ev = expr_numbers(end);
    let (Some(&cx), Some(&cy), Some(&ex), Some(&ey)) =
        (cv.first(), cv.get(1), ev.first(), ev.get(1))
    else {
        return Vec::new();
    };
    let radius = ((ex - cx).powi(2) + (ey - cy).powi(2)).sqrt();
    if !radius.is_finite() || radius < 1e-9 {
        return Vec::new();
    }
    let segments = segments.max(8);
    let mut out = Vec::with_capacity(segments + 1);
    let two_pi = std::f64::consts::TAU;
    for i in 0..=segments {
        let a = two_pi * (i as f64) / (segments as f64);
        out.push((cx + radius * a.cos(), cy + radius * a.sin()));
    }
    out
}

fn tessellate_arc_mm(
    s: (f64, f64),
    m: (f64, f64),
    e: (f64, f64),
    segments: usize,
) -> Vec<(f64, f64)> {
    let Some((cx, cy, radius, a0, a1)) = arc_params(s, m, e) else {
        return vec![s, e];
    };
    let segments = segments.max(2);
    let mut points = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let t = i as f64 / segments as f64;
        let a = a0 + (a1 - a0) * t;
        points.push((cx + radius * a.cos(), cy + radius * a.sin()));
    }
    if let Some(first) = points.first_mut() {
        *first = s;
    }
    if let Some(last) = points.last_mut() {
        *last = e;
    }
    points
}

fn arc_params(
    s: (f64, f64),
    m: (f64, f64),
    e: (f64, f64),
) -> Option<(f64, f64, f64, f64, f64)> {
    let (sx, sy) = s;
    let (mx, my) = m;
    let (ex, ey) = e;
    let d = 2.0 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    if d.abs() < 1e-9 {
        return None; // collinear/degenerate
    }
    let s2 = sx * sx + sy * sy;
    let m2 = mx * mx + my * my;
    let e2 = ex * ex + ey * ey;
    let cx = (s2 * (my - ey) + m2 * (ey - sy) + e2 * (sy - my)) / d;
    let cy = (s2 * (ex - mx) + m2 * (sx - ex) + e2 * (mx - sx)) / d;
    let radius = ((sx - cx).powi(2) + (sy - cy).powi(2)).sqrt();
    if !radius.is_finite() || radius < 1e-9 {
        return None;
    }
    let a0 = (sy - cy).atan2(sx - cx);
    let am = (my - cy).atan2(mx - cx);
    let a1_raw = (ey - cy).atan2(ex - cx);
    let two_pi = std::f64::consts::TAU;
    let norm = |a: f64| (a % two_pi + two_pi) % two_pi;
    let mut d01 = norm(a1_raw - a0);
    let dm = norm(am - a0);
    if dm > d01 {
        d01 -= two_pi;
    }
    let a1 = a0 + d01;
    Some((cx, cy, radius, a0, a1))
}

fn layer_id_for(ir: &PcbIr, name: &str, diagnostics: &mut Diagnostics) -> Option<ObjectId> {
    if let Some(layer) = ir.layer(name) {
        return Some(layer.id.clone());
    }
    let fallback = ir.copper_layers().first()?.id.clone();
    diagnostics.push(Diagnostic::warning(
        "KICAD_015",
        format!("unknown copper layer '{name}'; using the first copper layer"),
    ));
    Some(fallback)
}

// ---------------------------------------------------------------------------