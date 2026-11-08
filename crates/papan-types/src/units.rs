use serde::{Deserialize, Serialize};

use crate::quantity::{Quantity, UnitError, UnitKind};

/// Fixed-point length in nanometres.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Length(pub i64);

impl Length {
    pub const ZERO: Length = Length(0);

    pub const fn from_nm(nm: i64) -> Self {
        Self(nm)
    }

    pub fn from_um(um: f64) -> Self {
        Self((um * 1_000.0).round() as i64)
    }

    pub fn from_mm(mm: f64) -> Self {
        Self((mm * 1_000_000.0).round() as i64)
    }

    pub fn from_mil(mil: f64) -> Self {
        Self((mil * 25_400.0).round() as i64)
    }

    pub fn from_inch(inch: f64) -> Self {
        Self((inch * 25_400_000.0).round() as i64)
    }

    pub fn as_nm(self) -> i64 {
        self.0
    }

    pub fn as_mm(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    pub fn is_positive(self) -> bool {
        self.0 > 0
    }

    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    pub fn parse_literal(literal: &str) -> Result<Self, UnitError> {
        let quantity = Quantity::parse_literal(literal)?;
        quantity
            .as_length()
            .ok_or_else(|| UnitError::new(literal, "expected a length such as 0.25mm"))
    }

    pub fn to_source(self) -> String {
        format_display(self.0, 1_000_000, "mm")
    }
}

impl std::ops::Add for Length {
    type Output = Length;
    fn add(self, rhs: Length) -> Length {
        Length(self.0.saturating_add(rhs.0))
    }
}

impl std::ops::Sub for Length {
    type Output = Length;
    fn sub(self, rhs: Length) -> Length {
        Length(self.0.saturating_sub(rhs.0))
    }
}

impl std::ops::Neg for Length {
    type Output = Length;
    fn neg(self) -> Length {
        Length(-self.0)
    }
}

impl std::ops::Mul<i64> for Length {
    type Output = Length;
    fn mul(self, rhs: i64) -> Length {
        Length(self.0.saturating_mul(rhs))
    }
}

impl std::ops::Mul<f64> for Length {
    type Output = Length;
    fn mul(self, rhs: f64) -> Length {
        Length((self.0 as f64 * rhs).round() as i64)
    }
}

impl std::fmt::Display for Length {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_source())
    }
}

/// Fixed-point angle in millidegrees.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Angle(pub i64);

impl Angle {
    pub const ZERO: Angle = Angle(0);

    pub fn from_deg(deg: f64) -> Self {
        Self((deg * 1_000.0).round() as i64)
    }

    pub fn from_mdeg(mdeg: i64) -> Self {
        Self(mdeg)
    }

    pub fn as_deg(self) -> f64 {
        self.0 as f64 / 1_000.0
    }

    pub fn normalized(self) -> Self {
        let mut value = self.0 % 360_000;
        if value < 0 {
            value += 360_000;
        }
        Self(value)
    }

    pub fn parse_literal(literal: &str) -> Result<Self, UnitError> {
        let quantity = Quantity::parse_literal(literal)?;
        if quantity.kind == UnitKind::Angle {
            return Ok(Self(quantity.value));
        }
        // Allow plain numbers for angles, e.g. `rotate MCU 90`.
        if quantity.kind == UnitKind::Dimensionless {
            return Ok(Self(quantity.value.saturating_mul(1_000)));
        }
        Err(UnitError::new(literal, "expected an angle such as 90deg"))
    }

    pub fn to_source(self) -> String {
        format!("{}deg", trim_number(self.as_deg()))
    }
}

/// A point in board space, in nanometres.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Point {
    pub x: Length,
    pub y: Length,
}

impl Point {
    pub const ORIGIN: Point = Point {
        x: Length::ZERO,
        y: Length::ZERO,
    };

    pub const fn new(x: Length, y: Length) -> Self {
        Self { x, y }
    }

    pub fn from_mm(x: f64, y: f64) -> Self {
        Self {
            x: Length::from_mm(x),
            y: Length::from_mm(y),
        }
    }

    pub fn translate(self, dx: Length, dy: Length) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }

    pub fn distance_sq(self, other: Point) -> i128 {
        let dx = self.x.0 as i128 - other.x.0 as i128;
        let dy = self.y.0 as i128 - other.y.0 as i128;
        dx * dx + dy * dy
    }

    pub fn distance(self, other: Point) -> Length {
        let squared = self.distance_sq(other);
        Length((squared as f64).sqrt().round() as i64)
    }

    pub fn to_source(self) -> String {
        format!("({}, {})", self.x.to_source(), self.y.to_source())
    }
}

pub(crate) fn format_display(value: i64, divisor: i64, suffix: &str) -> String {
    if divisor <= 1 {
        return format!("{value}{suffix}");
    }
    let negative = value < 0;
    let abs = value.unsigned_abs();
    let div = divisor as u64;
    let whole = abs / div;
    let rem = abs % div;
    let digits = count_digits(div) - 1;
    let mut text = if rem == 0 {
        format!("{whole}")
    } else {
        let mut frac = format!("{rem:0width$}", width = digits);
        while frac.ends_with('0') {
            frac.pop();
        }
        format!("{whole}.{frac}")
    };
    if negative {
        text.insert(0, '-');
    }
    text.push_str(suffix);
    text
}

fn count_digits(mut value: u64) -> usize {
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}

pub(crate) fn trim_number(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let mut text = format!("{value:.6}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_roundtrip() {
        let value = Length::parse_literal("1.6mm").unwrap();
        assert_eq!(value, Length::from_mm(1.6));
        assert_eq!(value.to_source(), "1.6mm");
    }

    #[test]
    fn mil_and_inch() {
        assert_eq!(Length::parse_literal("40mil").unwrap(), Length::from_mil(40.0));
        assert_eq!(Length::parse_literal("1in").unwrap(), Length::from_inch(1.0));
    }

    #[test]
    fn point_distance() {
        let a = Point::from_mm(0.0, 0.0);
        let b = Point::from_mm(3.0, 4.0);
        assert_eq!(a.distance(b), Length::from_mm(5.0));
    }

    #[test]
    fn angle_normalization() {
        assert_eq!(Angle::from_deg(-90.0).normalized(), Angle::from_deg(270.0));
        assert_eq!(Angle::parse_literal("90deg").unwrap(), Angle::from_deg(90.0));
        assert_eq!(Angle::parse_literal("90").unwrap(), Angle::from_deg(90.0));
    }
}
