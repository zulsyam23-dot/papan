//! Geometry kernel integration tests.

use papan_geometry::{Rect, Segment, Size};
use papan_ir::{Length, Point};

#[test]
fn segment_intersection_and_distance_behave_as_expected() {
    let a = Segment::new(Point::from_mm(0.0, 0.0), Point::from_mm(10.0, 0.0));
    let b = Segment::new(Point::from_mm(5.0, -5.0), Point::from_mm(5.0, 5.0));
    let c = Segment::new(Point::from_mm(0.0, 1.0), Point::from_mm(10.0, 1.0));
    assert!(a.intersects(b));
    assert!(!a.intersects(c));
    assert!(a.distance_to_segment(c) <= Length::from_mm(1.0001));
    assert!(a.distance_to_segment(c) >= Length::from_mm(0.9999));
}

#[test]
fn rect_operations_are_deterministic() {
    let rect = Rect::new(Point::from_mm(1.0, 2.0), Size::from_mm(4.0, 3.0));
    let expanded = rect.expand(Length::from_mm(0.5));
    assert!(expanded.contains_point(Point::from_mm(0.75, 2.0)));
    assert!(rect.intersects(Rect::centered(Point::from_mm(3.0, 3.0), Size::from_mm(1.0, 1.0))));
    let clipped = rect.clip_segment(Segment::new(
        Point::from_mm(-1.0, 3.0),
        Point::from_mm(10.0, 3.0),
    ));
    let clipped = clipped.expect("segment should intersect rect");
    assert!(rect.contains_point(clipped.a) || rect.contains_point(clipped.b));
}
