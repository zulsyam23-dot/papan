use serde::{Deserialize, Serialize};

use crate::units::{format_display, trim_number, Angle, Length};

/// Domain type carried by a quantity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Dimensionless,
    Length,
    Area,
    Angle,
    Voltage,
    Current,
    Resistance,
    Capacitance,
    Inductance,
    Frequency,
    Impedance,
    Temperature,
    Power,
    Percent,
}

impl UnitKind {
    pub fn label(self) -> &'static str {
        match self {
            UnitKind::Dimensionless => "number",
            UnitKind::Length => "length",
            UnitKind::Area => "area",
            UnitKind::Angle => "angle",
            UnitKind::Voltage => "voltage",
            UnitKind::Current => "current",
            UnitKind::Resistance => "resistance",
            UnitKind::Capacitance => "capacitance",
            UnitKind::Inductance => "inductance",
            UnitKind::Frequency => "frequency",
            UnitKind::Impedance => "impedance",
            UnitKind::Temperature => "temperature",
            UnitKind::Power => "power",
            UnitKind::Percent => "percent",
        }
    }
}

/// A unit-aware value. The stored integer is in the unit kind's base scale:
///
/// * length: nanometres
/// * area: square nanometres
/// * angle: millidegrees
/// * voltage: microvolts
/// * current: nanoamperes
/// * resistance / impedance: milliohms
/// * capacitance: picofarads
/// * inductance: nanohenries
/// * frequency: hertz
/// * temperature: millidegrees Celsius
/// * power: microwatts
/// * percent: basis points (1% = 100)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Quantity {
    pub kind: UnitKind,
    pub value: i64,
}

impl Quantity {
    pub const fn new(kind: UnitKind, value: i64) -> Self {
        Self { kind, value }
    }

    pub const fn dimensionless(value: i64) -> Self {
        Self::new(UnitKind::Dimensionless, value)
    }

    pub fn from_length(length: Length) -> Self {
        Self::new(UnitKind::Length, length.0)
    }

    pub fn from_angle(angle: Angle) -> Self {
        Self::new(UnitKind::Angle, angle.0)
    }

    pub fn as_length(self) -> Option<Length> {
        (self.kind == UnitKind::Length).then_some(Length(self.value))
    }

    pub fn as_angle(self) -> Option<Angle> {
        (self.kind == UnitKind::Angle).then_some(Angle(self.value))
    }

    pub fn as_mm(self) -> Option<f64> {
        self.as_length().map(Length::as_mm)
    }

    pub fn is_kind(self, kind: UnitKind) -> bool {
        self.kind == kind
    }

    /// Parse a lexical quantity such as `0.25mm`, `10kΩ`, `16MHz`, `500mA`
    /// or `1%`.
    pub fn parse_literal(literal: &str) -> Result<Self, UnitError> {
        let text = literal.trim();
        if text.is_empty() {
            return Err(UnitError::new(literal, "empty quantity"));
        }

        let mut sign = 1.0f64;
        let mut rest = text;
        if let Some(stripped) = rest.strip_prefix('-') {
            sign = -1.0;
            rest = stripped;
        } else if let Some(stripped) = rest.strip_prefix('+') {
            rest = stripped;
        }

        let mut split = 0usize;
        let mut dot_seen = false;
        let mut digit_seen = false;
        for (idx, ch) in rest.char_indices() {
            if ch.is_ascii_digit() {
                digit_seen = true;
                split = idx + ch.len_utf8();
            } else if ch == '.' && !dot_seen {
                dot_seen = true;
                split = idx + ch.len_utf8();
            } else {
                break;
            }
        }
        if !digit_seen {
            return Err(UnitError::new(literal, "expected a number"));
        }

        let number_text = &rest[..split];
        let unit_text = rest[split..].trim();
        let number: f64 = number_text
            .parse()
            .map_err(|_| UnitError::new(literal, "invalid number"))?;
        let (kind, scale) = unit_info(unit_text)
            .ok_or_else(|| UnitError::new(literal, format!("unknown unit '{unit_text}'")))?;
        let value = (sign * number * scale).round();
        if !value.is_finite() || value > i64::MAX as f64 || value < i64::MIN as f64 {
            return Err(UnitError::new(literal, "quantity is out of range"));
        }
        Ok(Self {
            kind,
            value: value as i64,
        })
    }

    pub fn to_source(self) -> String {
        match self.kind {
            UnitKind::Dimensionless => self.value.to_string(),
            UnitKind::Length => Length(self.value).to_source(),
            UnitKind::Area => {
                if self.value.unsigned_abs() >= 1_000_000_000_000 {
                    format_display(self.value, 1_000_000_000_000, "mm2")
                } else {
                    format_display(self.value, 1_000_000, "um2")
                }
            }
            UnitKind::Angle => Angle(self.value).to_source(),
            UnitKind::Voltage => {
                if self.value.unsigned_abs() >= 1_000_000 {
                    format_display(self.value, 1_000_000, "V")
                } else {
                    format_display(self.value, 1_000, "mV")
                }
            }
            UnitKind::Current => {
                let abs = self.value.unsigned_abs();
                if abs >= 1_000_000_000 {
                    format_display(self.value, 1_000_000_000, "A")
                } else if abs >= 1_000_000 {
                    format_display(self.value, 1_000_000, "mA")
                } else {
                    format_display(self.value, 1_000, "uA")
                }
            }
            UnitKind::Resistance | UnitKind::Impedance => {
                let abs = self.value.unsigned_abs();
                if abs >= 1_000_000_000 {
                    format_display(self.value, 1_000_000_000, "MΩ")
                } else if abs >= 1_000_000 {
                    format_display(self.value, 1_000_000, "kΩ")
                } else {
                    format_display(self.value, 1_000, "Ω")
                }
            }
            UnitKind::Capacitance => {
                let abs = self.value.unsigned_abs();
                if abs >= 1_000_000_000_000 {
                    format_display(self.value, 1_000_000_000_000, "F")
                } else if abs >= 1_000_000 {
                    format_display(self.value, 1_000_000, "uF")
                } else if abs >= 1_000 {
                    format_display(self.value, 1_000, "nF")
                } else {
                    format_display(self.value, 1, "pF")
                }
            }
            UnitKind::Inductance => {
                let abs = self.value.unsigned_abs();
                if abs >= 1_000_000_000 {
                    format_display(self.value, 1_000_000_000, "H")
                } else if abs >= 1_000_000 {
                    format_display(self.value, 1_000_000, "mH")
                } else if abs >= 1_000 {
                    format_display(self.value, 1_000, "uH")
                } else {
                    format_display(self.value, 1, "nH")
                }
            }
            UnitKind::Frequency => {
                let abs = self.value.unsigned_abs();
                if abs >= 1_000_000_000 {
                    format_display(self.value, 1_000_000_000, "GHz")
                } else if abs >= 1_000_000 {
                    format_display(self.value, 1_000_000, "MHz")
                } else if abs >= 1_000 {
                    format_display(self.value, 1_000, "kHz")
                } else {
                    format_display(self.value, 1, "Hz")
                }
            }
            UnitKind::Temperature => format_display(self.value, 1_000, "degC"),
            UnitKind::Power => {
                let abs = self.value.unsigned_abs();
                if abs >= 1_000_000 {
                    format_display(self.value, 1_000_000, "W")
                } else if abs >= 1_000 {
                    format_display(self.value, 1_000, "mW")
                } else {
                    format_display(self.value, 1, "uW")
                }
            }
            UnitKind::Percent => format_display(self.value, 100, "%"),
        }
    }
}

impl std::fmt::Display for Quantity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_source())
    }
}

fn unit_info(unit: &str) -> Option<(UnitKind, f64)> {
    use UnitKind::*;
    if unit.is_empty() {
        return Some((Dimensionless, 1.0));
    }
    let normalized = unit.replace('µ', "u").replace('μ', "u");
    let unit = normalized.as_str();
    let result = match unit {
        // length
        "nm" => (Length, 1.0),
        "um" => (Length, 1_000.0),
        "mm" => (Length, 1_000_000.0),
        "cm" => (Length, 10_000_000.0),
        "m" => (Length, 1_000_000_000.0),
        "mil" | "mils" => (Length, 25_400.0),
        "in" | "inch" => (Length, 25_400_000.0),
        // area
        "mm2" | "mm^2" => (Area, 1_000_000_000_000.0),
        "cm2" | "cm^2" => (Area, 100_000_000_000_000.0),
        "um2" | "um^2" => (Area, 1_000_000.0),
        // angle
        "deg" | "degree" | "degrees" => (Angle, 1_000.0),
        "rad" => (Angle, 57_295.779_513_082_3),
        // voltage
        "uV" => (Voltage, 1.0),
        "mV" => (Voltage, 1_000.0),
        "V" | "v" => (Voltage, 1_000_000.0),
        "kV" => (Voltage, 1_000_000_000.0),
        // current
        "nA" => (Current, 1.0),
        "uA" => (Current, 1_000.0),
        "mA" => (Current, 1_000_000.0),
        "A" | "a" => (Current, 1_000_000_000.0),
        // resistance
        "Ω" | "ohm" | "ohms" => (Resistance, 1_000.0),
        "kΩ" | "kohm" => (Resistance, 1_000_000.0),
        "MΩ" | "Mohm" => (Resistance, 1_000_000_000.0),
        "mΩ" => (Resistance, 1.0),
        // capacitance
        "pF" => (Capacitance, 1.0),
        "nF" => (Capacitance, 1_000.0),
        "uF" => (Capacitance, 1_000_000.0),
        "mF" => (Capacitance, 1_000_000_000.0),
        "F" | "f" => (Capacitance, 1_000_000_000_000.0),
        // inductance
        "nH" => (Inductance, 1.0),
        "uH" => (Inductance, 1_000.0),
        "mH" => (Inductance, 1_000_000.0),
        "H" => (Inductance, 1_000_000_000.0),
        // frequency
        "Hz" => (Frequency, 1.0),
        "kHz" => (Frequency, 1_000.0),
        "MHz" => (Frequency, 1_000_000.0),
        "GHz" => (Frequency, 1_000_000_000.0),
        // impedance
        "ZΩ" => (Impedance, 1_000.0),
        // temperature
        "degC" | "°C" => (Temperature, 1_000.0),
        // power
        "uW" => (Power, 1.0),
        "mW" => (Power, 1_000.0),
        "W" | "w" => (Power, 1_000_000.0),
        // percent
        "%" => (Percent, 100.0),
        _ => return None,
    };
    Some(result)
}

/// Error returned when a lexical quantity cannot be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitError {
    pub literal: String,
    pub message: String,
}

impl UnitError {
    pub fn new(literal: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            literal: literal.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for UnitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid quantity '{}': {}", self.literal, self.message)
    }
}

impl std::error::Error for UnitError {}

/// Helper used by text projections to emit a clean decimal.
pub fn quantity_number(value: i64, divisor: i64) -> String {
    trim_number(value as f64 / divisor as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lengths() {
        assert_eq!(
            Quantity::parse_literal("0.25mm").unwrap(),
            Quantity::new(UnitKind::Length, 250_000)
        );
        assert_eq!(
            Quantity::parse_literal("40mil").unwrap(),
            Quantity::new(UnitKind::Length, 1_016_000)
        );
    }

    #[test]
    fn parses_electrical_quantities() {
        assert_eq!(
            Quantity::parse_literal("10kΩ").unwrap(),
            Quantity::new(UnitKind::Resistance, 10_000_000)
        );
        assert_eq!(
            Quantity::parse_literal("16MHz").unwrap(),
            Quantity::new(UnitKind::Frequency, 16_000_000)
        );
        assert_eq!(
            Quantity::parse_literal("500mA").unwrap(),
            Quantity::new(UnitKind::Current, 500_000_000)
        );
        assert_eq!(
            Quantity::parse_literal("1%").unwrap(),
            Quantity::new(UnitKind::Percent, 100)
        );
    }

    #[test]
    fn quantity_roundtrip() {
        for literal in [
            "0.25mm",
            "1.6mm",
            "5V",
            "500mA",
            "16MHz",
            "10kΩ",
            "1%",
            "100nF",
            "60degC",
        ] {
            let quantity = Quantity::parse_literal(literal).unwrap();
            let rendered = quantity.to_source();
            let reparsed = Quantity::parse_literal(&rendered).unwrap();
            assert_eq!(quantity, reparsed, "{literal} -> {rendered}");
        }
    }

    #[test]
    fn rejects_unknown_units() {
        assert!(Quantity::parse_literal("10florps").is_err());
        assert!(Quantity::parse_literal("mm").is_err());
    }
}
