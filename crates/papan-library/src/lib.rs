//! Component library.
//!
//! The library is independent from the compiler and from any vendor. Built-in
//! definitions provide a useful starting point; external libraries can be added
//! through the public API. Footprints here are intentionally simplified but
//! geometrically valid, so the placement, routing and DRC engines can operate on
//! designs without external dependencies.

use std::collections::BTreeMap;

use papan_geometry::Size;
use papan_ir::{
    ComponentCategory, ComponentDefinition, FootprintDefinition, ObjectId, PadDefinition, PadKind,
    PadLayers, PadShape, PinDefinition, PinType,
};
use papan_types::{Length, Point};

#[derive(Clone, Debug, Default)]
pub struct ComponentLibrary {
    entries: BTreeMap<String, ComponentDefinition>,
}

impl ComponentLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builtin() -> Self {
        let mut library = Self::new();
        library.insert(two_pin("resistor", ComponentCategory::Resistor, "0603"));
        library.insert(two_pin("capacitor", ComponentCategory::Capacitor, "0603"));
        library.insert(two_pin("inductor", ComponentCategory::Inductor, "0603"));
        library.insert(polarized_two_pin("diode", ComponentCategory::Diode, "SOD-323"));
        library.insert(polarized_two_pin("led", ComponentCategory::Led, "0603"));
        library.insert(two_pin("switch", ComponentCategory::Switch, "SW_SPST"));
        library.insert(two_pin("crystal", ComponentCategory::Crystal, "HC-49"));
        library.insert(three_pin("transistor", ComponentCategory::Transistor, "SOT-23"));
        library.insert(generic_named("connector", ComponentCategory::Connector, 4, "PinHeader_1x04"));
        library.insert(generic_named("ic", ComponentCategory::Ic, 8, "DIP-8"));

        library.insert(mcu_atmega32u4());
        library.insert(usb_c());
        library
    }

    pub fn insert(&mut self, definition: ComponentDefinition) {
        self.entries
            .insert(definition.name.to_ascii_lowercase(), definition);
    }

    pub fn definitions(&self) -> impl Iterator<Item = &ComponentDefinition> {
        self.entries.values()
    }

    pub fn resolve_kind(&self, kind: &str) -> Option<&ComponentDefinition> {
        let lowered = kind.to_ascii_lowercase();
        let canonical = match lowered.as_str() {
            "res" | "r" => "resistor",
            "cap" | "c" => "capacitor",
            "ind" | "l" => "inductor",
            "d" => "diode",
            "sw" => "switch",
            "xtal" => "crystal",
            "conn" => "connector",
            "chip" | "mcu" => "ic",
            "q" => "transistor",
            other => other,
        };
        self.entries.get(canonical)
    }

    pub fn resolve_part(&self, part: &str) -> Option<&ComponentDefinition> {
        self.entries.get(&part.to_ascii_lowercase())
    }

    /// Deterministic placeholder for an unknown quoted part. The semantic
    /// analyzer emits a warning so users know they are relying on a stub.
    pub fn generic_part(&self, part: &str, pin_count: usize) -> ComponentDefinition {
        generic_named(part, ComponentCategory::Other, pin_count.max(2), "GENERIC")
    }
}

fn pin(name: &str, number: &str, electrical: PinType) -> PinDefinition {
    PinDefinition {
        name: name.to_string(),
        number: number.to_string(),
        electrical,
        description: String::new(),
    }
}

fn two_pin(name: &str, category: ComponentCategory, footprint: &str) -> ComponentDefinition {
    let pins = vec![
        pin("1", "1", PinType::Passive),
        pin("2", "2", PinType::Passive),
    ];
    let pads = vec![
        smd_pad("1", "1", -0.8, 0.0),
        smd_pad("2", "2", 0.8, 0.0),
    ];
    make_definition(name, category, pins, footprint, pads)
}

fn polarized_two_pin(
    name: &str,
    category: ComponentCategory,
    footprint: &str,
) -> ComponentDefinition {
    let pins = vec![
        pin("A", "1", PinType::Passive),
        pin("K", "2", PinType::Passive),
    ];
    let pads = vec![
        smd_pad("1", "A", -0.9, 0.0),
        smd_pad("2", "K", 0.9, 0.0),
    ];
    make_definition(name, category, pins, footprint, pads)
}

fn three_pin(name: &str, category: ComponentCategory, footprint: &str) -> ComponentDefinition {
    let pins = vec![
        pin("1", "1", PinType::Bidirectional),
        pin("2", "2", PinType::Bidirectional),
        pin("3", "3", PinType::Bidirectional),
    ];
    let pads = vec![
        thru_pad("1", "1", -1.27, 0.0),
        thru_pad("2", "2", 0.0, 1.1),
        thru_pad("3", "3", 1.27, 0.0),
    ];
    make_definition(name, category, pins, footprint, pads)
}

fn generic_named(
    name: &str,
    category: ComponentCategory,
    pin_count: usize,
    footprint: &str,
) -> ComponentDefinition {
    let mut pins = Vec::new();
    for index in 1..=pin_count {
        pins.push(pin(
            &index.to_string(),
            &index.to_string(),
            PinType::Bidirectional,
        ));
    }
    let pads = generic_pads(&pins);
    make_definition(name, category, pins, footprint, pads)
}

fn mcu_atmega32u4() -> ComponentDefinition {
    let names: &[(&str, PinType)] = &[
        ("VCC", PinType::Power),
        ("GND", PinType::Ground),
        ("AVCC", PinType::Power),
        ("UGND", PinType::Ground),
        ("UVCC", PinType::Power),
        ("VBUS", PinType::Power),
        ("D+", PinType::Bidirectional),
        ("D-", PinType::Bidirectional),
        ("XTAL1", PinType::Input),
        ("XTAL2", PinType::Output),
        ("RESET", PinType::Input),
        ("AREF", PinType::Input),
        ("PE2", PinType::Bidirectional),
        ("PE6", PinType::Bidirectional),
        ("PB0", PinType::Bidirectional),
        ("PB1", PinType::Bidirectional),
        ("PB2", PinType::Bidirectional),
        ("PB3", PinType::Bidirectional),
        ("PB4", PinType::Bidirectional),
        ("PB5", PinType::Bidirectional),
        ("PB6", PinType::Bidirectional),
        ("PB7", PinType::Bidirectional),
        ("PC6", PinType::Bidirectional),
        ("PC7", PinType::Bidirectional),
        ("PD0", PinType::Bidirectional),
        ("PD1", PinType::Bidirectional),
        ("PD2", PinType::Bidirectional),
        ("PD3", PinType::Bidirectional),
        ("PD4", PinType::Bidirectional),
        ("PD5", PinType::Bidirectional),
        ("PD6", PinType::Bidirectional),
        ("PD7", PinType::Bidirectional),
        ("PF0", PinType::Bidirectional),
        ("PF1", PinType::Bidirectional),
        ("PF4", PinType::Bidirectional),
        ("PF5", PinType::Bidirectional),
        ("PF6", PinType::Bidirectional),
        ("PF7", PinType::Bidirectional),
    ];
    let pins: Vec<PinDefinition> = names
        .iter()
        .enumerate()
        .map(|(index, (name, electrical))| pin(name, &(index + 1).to_string(), *electrical))
        .collect();
    let pads = generic_pads(&pins);
    let mut definition = make_definition(
        "ATmega32U4",
        ComponentCategory::Ic,
        pins,
        "TQFP-44",
        pads,
    );
    definition.manufacturer = Some("Microchip".to_string());
    definition.description = "AVR microcontroller with native USB".to_string();
    definition
}

fn usb_c() -> ComponentDefinition {
    let names: &[(&str, PinType)] = &[
        ("VBUS", PinType::Power),
        ("GND", PinType::Ground),
        ("CC1", PinType::Bidirectional),
        ("CC2", PinType::Bidirectional),
        ("D+", PinType::Bidirectional),
        ("D-", PinType::Bidirectional),
        ("SBU1", PinType::Bidirectional),
        ("SBU2", PinType::Bidirectional),
        ("SHIELD", PinType::Passive),
    ];
    let pins: Vec<PinDefinition> = names
        .iter()
        .enumerate()
        .map(|(index, (name, electrical))| pin(name, &(index + 1).to_string(), *electrical))
        .collect();
    let pads = generic_pads(&pins);
    let mut definition = make_definition(
        "USB-C",
        ComponentCategory::Connector,
        pins,
        "USB_C_Receptacle",
        pads,
    );
    definition.description = "USB Type-C receptacle (simplified footprint)".to_string();
    definition
}

fn make_definition(
    name: &str,
    category: ComponentCategory,
    pins: Vec<PinDefinition>,
    footprint_name: &str,
    pads: Vec<PadDefinition>,
) -> ComponentDefinition {
    ComponentDefinition {
        id: ObjectId::stable("part", name),
        name: name.to_string(),
        category,
        pins,
        footprint: FootprintDefinition {
            name: footprint_name.to_string(),
            pads,
            description: "simplified deterministic footprint".to_string(),
        },
        manufacturer: None,
        datasheet: None,
        description: match category {
            ComponentCategory::Other => String::new(),
            _ => format!("{} ({footprint_name})", name),
        },
    }
}

fn smd_pad(number: &str, pin: &str, x_mm: f64, y_mm: f64) -> PadDefinition {
    PadDefinition {
        number: number.to_string(),
        pin: pin.to_string(),
        kind: PadKind::Smd,
        shape: PadShape::Rect,
        position: Point::from_mm(x_mm, y_mm),
        size: Size::from_mm(0.9, 0.95),
        drill: None,
        layers: PadLayers::Top,
    }
}

fn thru_pad(number: &str, pin: &str, x_mm: f64, y_mm: f64) -> PadDefinition {
    PadDefinition {
        number: number.to_string(),
        pin: pin.to_string(),
        kind: PadKind::ThroughHole,
        shape: PadShape::Round,
        position: Point::from_mm(x_mm, y_mm),
        size: Size::from_mm(1.6, 1.6),
        drill: Some(Length::from_mm(0.8)),
        layers: PadLayers::Both,
    }
}

/// Lay out pads on a deterministic grid centred on the component origin.
fn generic_pads(pins: &[PinDefinition]) -> Vec<PadDefinition> {
    let count = pins.len();
    if count == 0 {
        return Vec::new();
    }
    if count == 2 {
        return vec![
            smd_pad("1", &pins[0].name, -0.8, 0.0),
            smd_pad("2", &pins[1].name, 0.8, 0.0),
        ];
    }
    let columns = (count as f64).sqrt().ceil() as usize;
    let rows = count.div_ceil(columns);
    let mut pads = Vec::with_capacity(count);
    for (index, pin_def) in pins.iter().enumerate() {
        let row = index / columns;
        let column = index % columns;
        let centered_columns = columns as i64 - 1;
        let centered_rows = rows as i64 - 1;
        let x = (column as i64 * 2 - centered_columns) as f64 * 1.27;
        let y = (row as i64 * 2 - centered_rows) as f64 * 1.27;
        pads.push(thru_pad(&pin_def.number, &pin_def.name, x, y));
    }
    pads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_library_resolves_parts() {
        let library = ComponentLibrary::builtin();
        assert!(library.resolve_kind("resistor").is_some());
        assert!(library.resolve_kind("cap").is_some());
        let mcu = library.resolve_part("ATmega32U4").unwrap();
        assert!(mcu.pin("VCC").is_some());
        assert!(mcu.pin("D+").is_some());
        assert_eq!(mcu.footprint.pads.len(), mcu.pins.len());
    }

    #[test]
    fn usb_c_has_power_and_ground() {
        let library = ComponentLibrary::builtin();
        let usb = library.resolve_part("USB-C").unwrap();
        assert_eq!(usb.pin("VBUS").unwrap().electrical, PinType::Power);
        assert_eq!(usb.pin("GND").unwrap().electrical, PinType::Ground);
    }

    #[test]
    fn generic_part_is_deterministic() {
        let library = ComponentLibrary::builtin();
        let a = library.generic_part("XYZ", 8);
        let b = library.generic_part("XYZ", 8);
        assert_eq!(a, b);
        assert_eq!(a.footprint.pads.len(), 8);
    }
}
