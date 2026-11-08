//! Placement engine helpers.
//!
//! The semantic layer stores absolute component positions in the IR. This crate
//! turns footprints into absolute pad geometry, performs transformations,
//! resolves board anchors, detects bounding-box collisions and provides a
//! deterministic auto-placement fallback for components without an explicit
//! `place` statement.

use papan_geometry::{Rect, Size};
use papan_ir::{
    ComponentDefinition, ComponentInstance, PcbIr, PadDefinition, PinRef, Side,
};
use papan_types::{Angle, Diagnostic, Diagnostics, Length, Point};

/// Rotate a vector around the origin by a fixed-point angle.
pub fn rotate_point(point: Point, angle: Angle) -> Point {
    let normalized = angle.normalized();
    if normalized.0 % 360_000 == 0 {
        return point;
    }
    let radians = normalized.as_deg().to_radians();
    let (sin, cos) = radians.sin_cos();
    let x = point.x.0 as f64 * cos - point.y.0 as f64 * sin;
    let y = point.x.0 as f64 * sin + point.y.0 as f64 * cos;
    Point::new(Length(x.round() as i64), Length(y.round() as i64))
}

/// Absolute position of a pad on a placed component.
pub fn pad_position(component: &ComponentInstance, pad: &PadDefinition) -> Option<Point> {
    let origin = component.position?;
    let mut local = pad.position;
    if component.side == Side::Bottom {
        local = Point::new(local.x, -local.y);
    }
    let rotated = rotate_point(local, component.rotation);
    Some(Point::new(origin.x + rotated.x, origin.y + rotated.y))
}

/// Absolute position of a pin, resolved through the component footprint.
pub fn pin_position(
    ir: &PcbIr,
    component: &ComponentInstance,
    pin: &str,
) -> Option<Point> {
    let definition = ir.definition_for(component)?;
    let pad = definition.pad_for_pin(pin)?;
    pad_position(component, pad)
}

pub fn pin_ref_position(ir: &PcbIr, pin: &PinRef) -> Option<Point> {
    let component = ir.find_component_by_id(&pin.component)?;
    pin_position(ir, component, &pin.pin)
}

/// Absolute bounding rectangle of a pad.
pub fn pad_rect(component: &ComponentInstance, pad: &PadDefinition) -> Option<Rect> {
    let center = pad_position(component, pad)?;
    let mut size = pad.size;
    let rotation = component.rotation.normalized().0 % 180_000;
    if rotation >= 45_000 && rotation < 135_000 {
        size = Size::new(size.height, size.width);
    }
    Some(Rect::centered(center, size))
}

pub fn pad_center(component: &ComponentInstance, pad: &PadDefinition) -> Option<Point> {
    pad_position(component, pad)
}

/// Bounding rectangle of a placed component, derived from its pads.
pub fn component_rect(ir: &PcbIr, component: &ComponentInstance) -> Option<Rect> {
    let origin = component.position?;
    let definition = ir.definition_for(component)?;
    let mut bounds: Option<Rect> = None;
    for pad in &definition.footprint.pads {
        if let Some(rect) = pad_rect(component, pad) {
            bounds = Some(match bounds {
                None => rect,
                Some(existing) => Rect::from_points(
                    Point::new(existing.min().x.min(rect.min().x), existing.min().y.min(rect.min().y)),
                    Point::new(existing.max().x.max(rect.max().x), existing.max().y.max(rect.max().y)),
                ),
            });
        }
    }
    Some(bounds.unwrap_or_else(|| Rect::centered(origin, Size::from_mm(10.0, 10.0))))
}

pub fn component_bounds_for_definition(definition: &ComponentDefinition) -> Size {
    let mut min_x = Length::ZERO;
    let mut min_y = Length::ZERO;
    let mut max_x = Length::ZERO;
    let mut max_y = Length::ZERO;
    if definition.footprint.pads.is_empty() {
        return Size::from_mm(10.0, 10.0);
    }
    for pad in &definition.footprint.pads {
        min_x = min_x.min(pad.position.x - Length(pad.size.width.0 / 2));
        min_y = min_y.min(pad.position.y - Length(pad.size.height.0 / 2));
        max_x = max_x.max(pad.position.x + Length(pad.size.width.0 / 2));
        max_y = max_y.max(pad.position.y + Length(pad.size.height.0 / 2));
    }
    Size::new(max_x - min_x, max_y - min_y)
}

/// Resolve board anchors such as `board.top.center` or `board.bottom.left`.
pub fn board_anchor(path: &[String], board_width: Length, board_height: Length) -> Option<Point> {
    let mut vertical: Option<&str> = None;
    let mut horizontal: Option<&str> = None;
    for part in path {
        match part.to_ascii_lowercase().as_str() {
            "board" => {}
            "top" => vertical = Some("top"),
            "bottom" => vertical = Some("bottom"),
            "left" => horizontal = Some("left"),
            "right" => horizontal = Some("right"),
            "center" => {
                if vertical.is_some() && horizontal.is_none() {
                    horizontal = Some("center");
                } else if horizontal.is_some() && vertical.is_none() {
                    vertical = Some("center");
                } else if vertical.is_none() && horizontal.is_none() {
                    vertical = Some("center");
                    horizontal = Some("center");
                }
            }
            _ => return None,
        }
    }
    let x = match horizontal? {
        "left" => Length::ZERO,
        "right" => board_width,
        "center" => Length(board_width.0 / 2),
        _ => return None,
    };
    let y = match vertical? {
        "top" => board_height,
        "bottom" => Length::ZERO,
        "center" => Length(board_height.0 / 2),
        _ => return None,
    };
    Some(Point::new(x, y))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacementCollision {
    pub a: papan_types::ObjectId,
    pub b: papan_types::ObjectId,
    pub overlap: Rect,
}

/// Report component bounding box overlaps for components on the same side.
pub fn collisions(ir: &PcbIr) -> Vec<PlacementCollision> {
    let mut collisions = Vec::new();
    let mut placed: Vec<(&ComponentInstance, Rect)> = Vec::new();
    for component in &ir.components {
        if let Some(rect) = component_rect(ir, component) {
            placed.push((component, rect));
        }
    }
    for i in 0..placed.len() {
        for j in (i + 1)..placed.len() {
            let (a, rect_a) = placed[i];
            let (b, rect_b) = placed[j];
            if a.side != b.side {
                continue;
            }
            if let Some(overlap) = rect_a.intersection(rect_b) {
                if overlap.size.width.0 > 0 && overlap.size.height.0 > 0 {
                    collisions.push(PlacementCollision {
                        a: a.id.clone(),
                        b: b.id.clone(),
                        overlap,
                    });
                }
            }
        }
    }
    collisions
}

/// Place components without an explicit position on a deterministic grid.
pub fn auto_place(ir: &mut PcbIr) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    let margin = Length::from_mm(10.0);
    let spacing = Length::from_mm(5.0);
    let mut cursor_x = margin;
    let mut cursor_y = margin;
    let mut row_height = Length::ZERO;
    let definitions = ir.definitions.clone();

    for component in &mut ir.components {
        if component.position.is_some() {
            continue;
        }
        let Some(definition) = definitions
            .iter()
            .find(|definition| definition.id == component.definition_id)
        else {
            continue;
        };
        let size = component_bounds_for_definition(&definition);
        if cursor_x + size.width > ir.board.width - margin {
            cursor_x = margin;
            cursor_y = cursor_y + row_height + spacing;
            row_height = Length::ZERO;
        }
        let position = Point::new(
            cursor_x + Length(size.width.0 / 2),
            cursor_y + Length(size.height.0 / 2),
        );
        component.position = Some(position);
        cursor_x = cursor_x + size.width + spacing;
        row_height = row_height.max(size.height);
        diagnostics.push(
            Diagnostic::info(
                "SEM_017",
                format!(
                    "component '{}' had no placement; assigned deterministic position {}",
                    component.designator,
                    position.to_source()
                ),
            )
            .with_object(&component.id.0)
            .with_location(position),
        );
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use papan_geometry::Size;
    use papan_ir::{
        ComponentCategory, FootprintDefinition, ObjectId, PadKind, PadLayers, PadShape,
        PinDefinition, PinType,
    };

    fn definition() -> ComponentDefinition {
        ComponentDefinition {
            id: ObjectId::stable("part", "U1"),
            name: "U1".into(),
            category: ComponentCategory::Ic,
            pins: vec![PinDefinition {
                name: "1".into(),
                number: "1".into(),
                electrical: PinType::Passive,
                description: String::new(),
            }],
            footprint: FootprintDefinition {
                name: "test".into(),
                pads: vec![PadDefinition {
                    number: "1".into(),
                    pin: "1".into(),
                    kind: PadKind::Smd,
                    shape: PadShape::Rect,
                    position: Point::from_mm(2.0, 0.0),
                    size: Size::from_mm(1.0, 1.0),
                    drill: None,
                    layers: PadLayers::Top,
                }],
                description: String::new(),
            },
            manufacturer: None,
            datasheet: None,
            description: String::new(),
        }
    }

    fn instance(position: Point, rotation: Angle) -> ComponentInstance {
        ComponentInstance {
            id: ObjectId::stable("component", "U1"),
            designator: "U1".into(),
            definition_id: ObjectId::stable("part", "U1"),
            value: None,
            properties: Default::default(),
            position: Some(position),
            rotation,
            side: Side::Top,
            locked: false,
            source_span: None,
        }
    }

    #[test]
    fn pad_position_applies_rotation_and_translation() {
        let definition = definition();
        let component = instance(Point::from_mm(10.0, 10.0), Angle::from_deg(90.0));
        let position = pad_position(&component, &definition.footprint.pads[0]).unwrap();
        assert_eq!(position, Point::from_mm(10.0, 12.0));
    }

    #[test]
    fn anchors_resolve() {
        let width = Length::from_mm(100.0);
        let height = Length::from_mm(50.0);
        let path = vec!["board".to_string(), "top".to_string(), "center".to_string()];
        assert_eq!(
            board_anchor(&path, width, height),
            Some(Point::from_mm(50.0, 50.0))
        );
        let path = vec!["board".to_string(), "bottom".to_string(), "left".to_string()];
        assert_eq!(
            board_anchor(&path, width, height),
            Some(Point::from_mm(0.0, 0.0))
        );
    }

    #[test]
    fn detects_collision() {
        let mut ir = PcbIr::new("Test");
        ir.add_definition(definition());
        let mut a = instance(Point::from_mm(10.0, 10.0), Angle::ZERO);
        a.id = ObjectId::stable("component", "U1");
        a.designator = "U1".into();
        let mut b = a.clone();
        b.id = ObjectId::stable("component", "U2");
        b.designator = "U2".into();
        b.position = Some(Point::from_mm(10.5, 10.0));
        ir.add_component(a);
        ir.add_component(b);
        assert_eq!(collisions(&ir).len(), 1);
    }
}
