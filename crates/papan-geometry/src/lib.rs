//! Fixed-point geometry kernel used by placement, routing, DRC and
//! manufacturing. Coordinates are integer nanometres from [`papan_types`], so
//! comparisons are exact and deterministic.

use papan_types::{Length, Point};
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Size {
    pub width: Length,
    pub height: Length,
}

impl Size {
    pub const fn new(width: Length, height: Length) -> Self {
        Self { width, height }
    }

    pub fn from_mm(width: f64, height: f64) -> Self {
        Self {
            width: Length::from_mm(width),
            height: Length::from_mm(height),
        }
    }

    pub fn half(self) -> Size {
        Size {
            width: Length(self.width.0 / 2),
            height: Length(self.height.0 / 2),
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Segment {
    pub a: Point,
    pub b: Point,
}

impl Segment {
    pub const fn new(a: Point, b: Point) -> Self {
        Self { a, b }
    }

    pub fn length_sq(self) -> i128 {
        self.a.distance_sq(self.b)
    }

    pub fn length(self) -> Length {
        self.a.distance(self.b)
    }

    pub fn midpoint(self) -> Point {
        Point::new(
            Length((self.a.x.0 as i128 + self.b.x.0 as i128).div_euclid(2) as i64),
            Length((self.a.y.0 as i128 + self.b.y.0 as i128).div_euclid(2) as i64),
        )
    }

    pub fn is_degenerate(self) -> bool {
        self.a == self.b
    }

    pub fn intersects(self, other: Segment) -> bool {
        let o1 = orientation(self.a, self.b, other.a);
        let o2 = orientation(self.a, self.b, other.b);
        let o3 = orientation(other.a, other.b, self.a);
        let o4 = orientation(other.a, other.b, self.b);

        if o1 != 0 && o2 != 0 && o1.signum() != o2.signum()
            && o3 != 0
            && o4 != 0
            && o3.signum() != o4.signum()
        {
            return true;
        }
        (o1 == 0 && on_segment(self.a, self.b, other.a))
            || (o2 == 0 && on_segment(self.a, self.b, other.b))
            || (o3 == 0 && on_segment(other.a, other.b, self.a))
            || (o4 == 0 && on_segment(other.a, other.b, self.b))
    }

    pub fn distance_sq_to_point(self, point: Point) -> i128 {
        point_segment_distance_sq(point, self.a, self.b)
    }

    pub fn distance_to_point(self, point: Point) -> Length {
        Length(sqrt_round(self.distance_sq_to_point(point)))
    }

    pub fn distance_sq_to_segment(self, other: Segment) -> i128 {
        if self.intersects(other) {
            return 0;
        }
        self.distance_sq_to_point(other.a)
            .min(self.distance_sq_to_point(other.b))
            .min(other.distance_sq_to_point(self.a))
            .min(other.distance_sq_to_point(self.b))
    }

    pub fn distance_to_segment(self, other: Segment) -> Length {
        Length(sqrt_round(self.distance_sq_to_segment(other)))
    }

    pub fn distance_to_rect(self, rect: Rect) -> Length {
        Length(sqrt_round(self.distance_sq_to_rect(rect)))
    }

    pub fn distance_sq_to_rect(self, rect: Rect) -> i128 {
        if rect.contains_point(self.a) || rect.contains_point(self.b) {
            return 0;
        }
        let edges = rect.edges();
        let mut best = i128::MAX;
        for edge in edges {
            best = best.min(self.distance_sq_to_segment(edge));
        }
        best
    }

    pub fn bbox(self) -> Rect {
        Rect::from_points(self.a, self.b)
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub fn new(origin: Point, size: Size) -> Self {
        Self { origin, size }
    }

    pub fn centered(center: Point, size: Size) -> Self {
        Self {
            origin: Point::new(
                Length(center.x.0 - size.width.0 / 2),
                Length(center.y.0 - size.height.0 / 2),
            ),
            size,
        }
    }

    pub fn from_points(a: Point, b: Point) -> Self {
        let min_x = a.x.min(b.x);
        let min_y = a.y.min(b.y);
        let max_x = a.x.max(b.x);
        let max_y = a.y.max(b.y);
        Self {
            origin: Point::new(min_x, min_y),
            size: Size::new(max_x - min_x, max_y - min_y),
        }
    }

    pub fn min(self) -> Point {
        self.origin
    }

    pub fn max(self) -> Point {
        Point::new(
            self.origin.x + self.size.width,
            self.origin.y + self.size.height,
        )
    }

    pub fn center(self) -> Point {
        Point::new(
            self.origin.x + Length(self.size.width.0 / 2),
            self.origin.y + Length(self.size.height.0 / 2),
        )
    }

    pub fn corners(self) -> [Point; 4] {
        [
            self.origin,
            Point::new(self.origin.x + self.size.width, self.origin.y),
            self.max(),
            Point::new(self.origin.x, self.origin.y + self.size.height),
        ]
    }

    pub fn edges(self) -> [Segment; 4] {
        let [a, b, c, d] = self.corners();
        [
            Segment::new(a, b),
            Segment::new(b, c),
            Segment::new(c, d),
            Segment::new(d, a),
        ]
    }

    pub fn expand(self, margin: Length) -> Self {
        Self {
            origin: Point::new(self.origin.x - margin, self.origin.y - margin),
            size: Size::new(
                self.size.width + margin * 2,
                self.size.height + margin * 2,
            ),
        }
    }

    pub fn contains_point(self, point: Point) -> bool {
        point.x >= self.origin.x
            && point.y >= self.origin.y
            && point.x <= self.origin.x + self.size.width
            && point.y <= self.origin.y + self.size.height
    }

    pub fn intersects(self, other: Rect) -> bool {
        self.origin.x <= other.max().x
            && other.origin.x <= self.max().x
            && self.origin.y <= other.max().y
            && other.origin.y <= self.max().y
    }

    pub fn intersection(self, other: Rect) -> Option<Rect> {
        let min_x = self.origin.x.max(other.origin.x);
        let min_y = self.origin.y.max(other.origin.y);
        let max_x = self.max().x.min(other.max().x);
        let max_y = self.max().y.min(other.max().y);
        if max_x < min_x || max_y < min_y {
            None
        } else {
            Some(Rect::new(
                Point::new(min_x, min_y),
                Size::new(max_x - min_x, max_y - min_y),
            ))
        }
    }

    pub fn distance_sq_to_point(self, point: Point) -> i128 {
        let min = self.min();
        let max = self.max();
        let dx = if point.x.0 < min.x.0 {
            (min.x.0 - point.x.0) as i128
        } else if point.x.0 > max.x.0 {
            (point.x.0 - max.x.0) as i128
        } else {
            0
        };
        let dy = if point.y.0 < min.y.0 {
            (min.y.0 - point.y.0) as i128
        } else if point.y.0 > max.y.0 {
            (point.y.0 - max.y.0) as i128
        } else {
            0
        };
        dx * dx + dy * dy
    }

    /// Liang-Barsky clipping of a segment to this rectangle.
    pub fn clip_segment(self, segment: Segment) -> Option<Segment> {
        let min = self.min();
        let max = self.max();
        let x0 = segment.a.x.0 as f64;
        let y0 = segment.a.y.0 as f64;
        let x1 = segment.b.x.0 as f64;
        let y1 = segment.b.y.0 as f64;
        let dx = x1 - x0;
        let dy = y1 - y0;
        let mut t0 = 0.0f64;
        let mut t1 = 1.0f64;
        let p = [-dx, dx, -dy, dy];
        let q = [x0 - min.x.0 as f64, max.x.0 as f64 - x0, y0 - min.y.0 as f64, max.y.0 as f64 - y0];
        for index in 0..4 {
            if p[index].abs() < f64::EPSILON {
                if q[index] < 0.0 {
                    return None;
                }
            } else {
                let r = q[index] / p[index];
                if p[index] < 0.0 {
                    if r > t1 {
                        return None;
                    }
                    if r > t0 {
                        t0 = r;
                    }
                } else {
                    if r < t0 {
                        return None;
                    }
                    if r < t1 {
                        t1 = r;
                    }
                }
            }
        }
        Some(Segment::new(
            Point::new(
                Length((x0 + t0 * dx).round() as i64),
                Length((y0 + t0 * dy).round() as i64),
            ),
            Point::new(
                Length((x0 + t1 * dx).round() as i64),
                Length((y0 + t1 * dy).round() as i64),
            ),
        ))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Polygon {
    pub points: Vec<Point>,
}

impl Polygon {
    pub fn new(points: Vec<Point>) -> Self {
        Self { points }
    }

    pub fn rect(width: Length, height: Length) -> Self {
        Self::new(vec![
            Point::ORIGIN,
            Point::new(width, Length::ZERO),
            Point::new(width, height),
            Point::new(Length::ZERO, height),
        ])
    }

    pub fn rect_at(origin: Point, width: Length, height: Length) -> Self {
        Self::new(vec![
            origin,
            Point::new(origin.x + width, origin.y),
            Point::new(origin.x + width, origin.y + height),
            Point::new(origin.x, origin.y + height),
        ])
    }

    pub fn is_empty(&self) -> bool {
        self.points.len() < 3
    }

    pub fn signed_area_sq_nm(&self) -> i128 {
        if self.points.len() < 3 {
            return 0;
        }
        let mut sum = 0i128;
        for index in 0..self.points.len() {
            let a = self.points[index];
            let b = self.points[(index + 1) % self.points.len()];
            sum += (a.x.0 as i128) * (b.y.0 as i128) - (b.x.0 as i128) * (a.y.0 as i128);
        }
        sum
    }

    /// Area in square millimetres, rounded to six decimals.
    pub fn area_mm2(&self) -> f64 {
        self.signed_area_sq_nm().abs() as f64 / 2.0 / 1e12
    }

    pub fn edges(&self) -> Vec<Segment> {
        if self.points.len() < 2 {
            return Vec::new();
        }
        let mut edges = Vec::with_capacity(self.points.len());
        for index in 0..self.points.len() {
            edges.push(Segment::new(
                self.points[index],
                self.points[(index + 1) % self.points.len()],
            ));
        }
        edges
    }

    pub fn perimeter(&self) -> Length {
        let mut total = Length::ZERO;
        for edge in self.edges() {
            total = total + edge.length();
        }
        total
    }

    pub fn bounding_box(&self) -> Rect {
        if self.points.is_empty() {
            return Rect::default();
        }
        let mut min_x = self.points[0].x;
        let mut min_y = self.points[0].y;
        let mut max_x = self.points[0].x;
        let mut max_y = self.points[0].y;
        for point in &self.points[1..] {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }
        Rect::new(Point::new(min_x, min_y), Size::new(max_x - min_x, max_y - min_y))
    }

    pub fn contains_point(&self, point: Point) -> bool {
        if self.is_empty() {
            return false;
        }
        // Boundary counts as inside.
        for edge in self.edges() {
            if edge.distance_sq_to_point(point) == 0 {
                return true;
            }
        }
        let mut inside = false;
        let mut j = self.points.len() - 1;
        for i in 0..self.points.len() {
            let pi = self.points[i];
            let pj = self.points[j];
            let intersects = (pi.y.0 > point.y.0) != (pj.y.0 > point.y.0);
            if intersects {
                let denominator = (pj.y.0 - pi.y.0) as f64;
                let x_intersection = pi.x.0 as f64
                    + (point.y.0 - pi.y.0) as f64 * (pj.x.0 - pi.x.0) as f64 / denominator;
                if (point.x.0 as f64) < x_intersection {
                    inside = !inside;
                }
            }
            j = i;
        }
        inside
    }

    pub fn distance_to_point(&self, point: Point) -> Length {
        let mut best = i128::MAX;
        for edge in self.edges() {
            best = best.min(edge.distance_sq_to_point(point));
        }
        if best == i128::MAX {
            Length::ZERO
        } else {
            Length(sqrt_round(best))
        }
    }

    pub fn distance_to_segment(&self, segment: Segment) -> Length {
        let mut best = i128::MAX;
        for edge in self.edges() {
            best = best.min(segment.distance_sq_to_segment(edge));
        }
        if best == i128::MAX {
            Length::ZERO
        } else {
            Length(sqrt_round(best))
        }
    }
}

fn orientation(a: Point, b: Point, c: Point) -> i128 {
    (b.x.0 as i128 - a.x.0 as i128) * (c.y.0 as i128 - a.y.0 as i128)
        - (b.y.0 as i128 - a.y.0 as i128) * (c.x.0 as i128 - a.x.0 as i128)
}

fn on_segment(a: Point, b: Point, p: Point) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

pub fn point_segment_distance_sq(point: Point, a: Point, b: Point) -> i128 {
    let abx = b.x.0 as i128 - a.x.0 as i128;
    let aby = b.y.0 as i128 - a.y.0 as i128;
    let len_sq = abx * abx + aby * aby;
    if len_sq == 0 {
        return point.distance_sq(a);
    }
    let apx = point.x.0 as i128 - a.x.0 as i128;
    let apy = point.y.0 as i128 - a.y.0 as i128;
    let dot = apx * abx + apy * aby;
    let t = dot.clamp(0, len_sq);
    let closest_x = a.x.0 as i128 + abx * t / len_sq;
    let closest_y = a.y.0 as i128 + aby * t / len_sq;
    let dx = point.x.0 as i128 - closest_x;
    let dy = point.y.0 as i128 - closest_y;
    dx * dx + dy * dy
}

fn sqrt_round(value: i128) -> i64 {
    if value <= 0 {
        return 0;
    }
    (value as f64).sqrt().round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(value: f64) -> Length {
        Length::from_mm(value)
    }

    #[test]
    fn polygon_area_and_contains() {
        let polygon = Polygon::rect(mm(10.0), mm(20.0));
        assert!((polygon.area_mm2() - 200.0).abs() < 1e-9);
        assert!(polygon.contains_point(Point::from_mm(5.0, 10.0)));
        assert!(!polygon.contains_point(Point::from_mm(15.0, 10.0)));
        assert!(polygon.contains_point(Point::from_mm(0.0, 10.0)));
    }

    #[test]
    fn segment_distance_and_intersection() {
        let a = Segment::new(Point::from_mm(0.0, 0.0), Point::from_mm(10.0, 0.0));
        let b = Segment::new(Point::from_mm(5.0, -5.0), Point::from_mm(5.0, 5.0));
        assert!(a.intersects(b));
        assert_eq!(a.distance_to_segment(b), mm(0.0));

        let c = Segment::new(Point::from_mm(0.0, 1.0), Point::from_mm(10.0, 1.0));
        assert_eq!(a.distance_to_segment(c), mm(1.0));
    }

    #[test]
    fn rect_clipping() {
        let rect = Rect::new(Point::ORIGIN, Size::from_mm(10.0, 10.0));
        let segment = Segment::new(Point::from_mm(-5.0, 5.0), Point::from_mm(15.0, 5.0));
        let clipped = rect.clip_segment(segment).unwrap();
        assert_eq!(clipped.a, Point::from_mm(0.0, 5.0));
        assert_eq!(clipped.b, Point::from_mm(10.0, 5.0));
    }

    #[test]
    fn point_segment_distance() {
        let distance = point_segment_distance_sq(
            Point::from_mm(5.0, 3.0),
            Point::from_mm(0.0, 0.0),
            Point::from_mm(10.0, 0.0),
        );
        assert_eq!(distance, (mm(3.0).0 as i128).pow(2));
    }
}
