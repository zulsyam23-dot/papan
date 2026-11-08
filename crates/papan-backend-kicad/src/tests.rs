use super::*;
use std::collections::BTreeMap;

use papan_backend::{BackendInput, PcbBackend};
use papan_geometry::{Polygon, Size};
use papan_ir::{
    Angle, ComponentCategory, ComponentDefinition, ComponentInstance, Connection,
    FootprintDefinition, Length, Net, NetKind, ObjectId, PadDefinition, PadKind, PadLayers,
    PadShape, PcbIr, PinDefinition, PinRef, PinType, Point, PropValue, Quantity, Side, Stackup,
    Trace, Via,
};

    fn resistor_definition() -> ComponentDefinition {
        ComponentDefinition {
            id: ObjectId::stable("part", "resistor"),
            name: "resistor".to_string(),
            category: ComponentCategory::Resistor,
            pins: vec![
                PinDefinition {
                    name: "1".into(),
                    number: "1".into(),
                    electrical: PinType::Passive,
                    description: String::new(),
                },
                PinDefinition {
                    name: "2".into(),
                    number: "2".into(),
                    electrical: PinType::Passive,
                    description: String::new(),
                },
            ],
            footprint: FootprintDefinition {
                name: "0603".into(),
                pads: vec![
                    PadDefinition {
                        number: "1".into(),
                        pin: "1".into(),
                        kind: PadKind::Smd,
                        shape: PadShape::Rect,
                        position: Point::from_mm(-0.8, 0.0),
                        size: Size::from_mm(0.9, 0.95),
                        drill: None,
                        layers: PadLayers::Top,
                    },
                    PadDefinition {
                        number: "2".into(),
                        pin: "2".into(),
                        kind: PadKind::Smd,
                        shape: PadShape::Rect,
                        position: Point::from_mm(0.8, 0.0),
                        size: Size::from_mm(0.9, 0.95),
                        drill: None,
                        layers: PadLayers::Top,
                    },
                ],
                description: String::new(),
            },
            manufacturer: None,
            datasheet: None,
            description: String::new(),
        }
    }

    fn fixture() -> PcbIr {
        let mut ir = PcbIr::new("KiCad Demo");
        ir.board.width = Length::from_mm(50.0);
        ir.board.height = Length::from_mm(30.0);
        ir.board.outline = Polygon::rect(ir.board.width, ir.board.height);
        ir.definitions.clear();
        ir.components.clear();
        ir.nets.clear();
        ir.connections.clear();
        ir.add_definition(resistor_definition());
        for (designator, x) in [("R1", 10.0), ("R2", 30.0)] {
            ir.components.push(ComponentInstance {
                id: ObjectId::stable("component", designator),
                designator: designator.to_string(),
                definition_id: ObjectId::stable("part", "resistor"),
                value: Some(Quantity::parse_literal("10kΩ").unwrap()),
                properties: BTreeMap::new(),
                position: Some(Point::from_mm(x, 10.0)),
                rotation: Angle::ZERO,
                side: Side::Top,
                locked: false,
                source_span: None,
            });
        }
        let net_id = ObjectId::stable("net", "N1");
        let p1 = PinRef::new(ObjectId::stable("component", "R1"), "1");
        let p2 = PinRef::new(ObjectId::stable("component", "R2"), "1");
        ir.add_net(Net {
            id: net_id.clone(),
            name: "N1".into(),
            kind: NetKind::Other,
            voltage: None,
            members: vec![p1.clone(), p2.clone()],
            properties: BTreeMap::new(),
            source_span: None,
        });
        ir.connect(p1, net_id.clone(), None);
        ir.connect(p2, net_id.clone(), None);
        ir.traces.push(Trace {
            id: ObjectId::stable("trace", "t1"),
            net: net_id.clone(),
            layer: ObjectId::stable("layer", "F.Cu"),
            width: Length::from_mm(0.25),
            points: vec![Point::from_mm(9.2, 10.0), Point::from_mm(29.2, 10.0)],
            source_span: None,
        });
        ir.vias.push(Via {
            id: ObjectId::stable("via", "v1"),
            net: net_id,
            at: Point::from_mm(20.0, 20.0),
            drill: Length::from_mm(0.3),
            diameter: Length::from_mm(0.6),
            from_layer: ObjectId::stable("layer", "F.Cu"),
            to_layer: ObjectId::stable("layer", "B.Cu"),
            source_span: None,
        });
        ir
    }

    #[test]
    fn exports_expected_kicad_objects() {
        let ir = fixture();
        let text = export_kicad(&ir);
        assert!(text.starts_with("(kicad_pcb\n"));
        assert!(text.contains("(footprint \"0603\""));
        assert!(text.contains("(property \"Reference\" \"R1\")"));
        assert!(text.contains("(segment (start 9.2 10)"));
        assert!(text.contains("(via (at 20 20) (size 0.6) (drill 0.3)"));
        assert!(text.contains("(net 1 \"N1\")"));
        assert!(text.contains("(gr_line"));
        assert!(text.trim_end().ends_with(')'));
    }

    #[test]
    fn roundtrip_preserves_geometry_and_connectivity() {
        let ir = fixture();
        let text = export_kicad(&ir);
        let imported = import_kicad(&BackendInput::new("board.kicad_pcb", text)).unwrap();
        assert!(!imported.diagnostics.has_errors(), "{:?}", imported.diagnostics);
        assert_eq!(imported.ir.board.width, ir.board.width);
        assert_eq!(imported.ir.board.height, ir.board.height);
        assert_eq!(imported.ir.components.len(), 2);
        assert!(imported.ir.find_component("R1").is_some());
        assert!(imported.ir.find_net("N1").is_some());
        assert!(!imported.ir.traces.is_empty());
        assert_eq!(imported.ir.vias.len(), 1);
        let net = imported.ir.find_net("N1").unwrap();
        assert_eq!(net.members.len(), 2);
    }

    #[test]
    fn imports_minimal_file_with_rect_outline() {
        let text = r#"
(kicad_pcb
  (version 20240108)
  (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "VCC")
  (footprint "R_0603"
    (layer "F.Cu")
    (at 10 10 90)
    (property "Reference" "R1")
    (property "Value" "10k")
    (pad "1" smd rect (at -0.8 0) (size 0.9 0.95) (layers "F.Cu") (net 1 "VCC"))
    (pad "2" smd rect (at 0.8 0) (size 0.9 0.95) (layers "F.Cu"))
  )
  (gr_rect (start 0 0) (end 50 30) (layer "Edge.Cuts"))
)
"#;
        let imported = import_kicad(&BackendInput::new("test.kicad_pcb", text)).unwrap();
        assert_eq!(imported.ir.board.width, Length::from_mm(50.0));
        assert_eq!(imported.ir.board.height, Length::from_mm(30.0));
        let component = imported.ir.find_component("R1").unwrap();
        assert_eq!(component.position, Some(Point::from_mm(10.0, 10.0)));
        assert_eq!(component.rotation, Angle::from_deg(90.0));
        assert_eq!(component.properties.get("value"), Some(&PropValue::String("10k".into())));
        assert!(imported.ir.find_net("VCC").is_some());
        assert_eq!(imported.ir.find_net("VCC").unwrap().members.len(), 1);
    }

    #[test]
    fn rejects_non_kicad_input() {
        let error = import_kicad(&BackendInput::new("x", "not a board")).unwrap_err();
        assert_eq!(error.code, "KICAD_005");
    }

    #[test]
    fn rejects_unbalanced_input() {
        let error = import_kicad(&BackendInput::new("x", "(kicad_pcb")).unwrap_err();
        assert_eq!(error.code, "KICAD_002");
    }

    #[test]
    fn validate_flags_missing_outline() {
        let mut ir = PcbIr::new("Empty");
        ir.board.outline = Polygon::new(Vec::new());
        let report = KiCadBackend::new().validate(&ir).unwrap();
        assert!(!report.production_ready);
        assert!(report.diagnostics.iter().any(|d| d.code == "KICAD_021"));
    }

    #[test]
    fn backend_trait_export_writes_one_file() {
        let output = KiCadBackend::new().export(&fixture()).unwrap();
        assert_eq!(output.files.len(), 1);
        assert_eq!(output.files[0].name, "board.kicad_pcb");
        assert!(output.diagnostics.iter().all(|d| d.severity != papan_types::Severity::Error));
    }

    #[test]
    fn imports_arc_track_as_polyline() {
        // Semicircle arc: start (10,10), mid (20,20), end (30,10) => bulges to y=20.
        let text = r#"
(kicad_pcb
  (version 20240108)
  (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "N1")
  (gr_rect (start 0 0) (end 100 80) (layer "Edge.Cuts") (width 0.1))
  (arc (start 10 10) (mid 20 20) (end 30 10) (width 0.25) (layer "F.Cu") (net 1))
)
"#;
        let imported = import_kicad(&BackendInput::new("board.kicad_pcb", text.to_string())).unwrap();
        assert!(!imported.diagnostics.has_errors(), "{:?}", imported.diagnostics);
        assert!(
            !imported.diagnostics.iter().any(|d| d.code == "KICAD_012" && d.message.contains("arc")),
            "track arc should be imported, not warned as unsupported: {:?}",
            imported.diagnostics
        );
        assert_eq!(imported.ir.traces.len(), 1, "{:?}", imported.ir.traces);
        let trace = &imported.ir.traces[0];
        assert!(trace.points.len() >= 3, "arc should tessellate: {:?}", trace.points);
        assert_eq!(trace.points.first().unwrap().x, Length::from_mm(10.0));
        assert_eq!(trace.points.last().unwrap().x, Length::from_mm(30.0));
        let max_y = trace.points.iter().map(|p| p.y.0).max().unwrap();
        assert!(max_y > Length::from_mm(15.0).0, "arc not bulging: {max_y}");
    }

    #[test]
    fn warns_for_graphical_gr_arc_but_not_edgecuts() {
        let text = r#"
(kicad_pcb
  (version 20240108)
  (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (net 0 "")
  (gr_rect (start 0 0) (end 50 30) (layer "Edge.Cuts") (width 0.1))
  (gr_arc (start 5 5) (mid 10 10) (end 15 5) (layer "F.SilkS") (width 0.12))
)
"#;
        let imported = import_kicad(&BackendInput::new("board.kicad_pcb", text.to_string())).unwrap();
        assert!(
            imported.diagnostics.iter().any(|d| d.code == "KICAD_013" && d.message.contains("non-Edge.Cuts")),
            "expected KICAD_013 for non-Edge.Cuts gr_arc: {:?}",
            imported.diagnostics
        );
    }

    #[test]
    fn edgecuts_gr_poly_defines_outline() {
        let text = r#"
(kicad_pcb
  (version 20240108)
  (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (gr_poly (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)) (layer "Edge.Cuts") (width 0.1))
)
"#;
        let imported = import_kicad(&BackendInput::new("board.kicad_pcb", text.to_string())).unwrap();
        assert!(!imported.diagnostics.has_errors(), "{:?}", imported.diagnostics);
        assert_eq!(imported.ir.board.width, Length::from_mm(40.0));
        assert_eq!(imported.ir.board.height, Length::from_mm(20.0));
        assert!(!imported.ir.board.outline.points.is_empty());
    }

    #[test]
    fn edgecuts_gr_circle_defines_outline() {
        let text = r#"
(kicad_pcb
  (version 20240108)
  (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (gr_circle (center 25 15) (end 35 15) (layer "Edge.Cuts") (width 0.1))
)
"#;
        let imported = import_kicad(&BackendInput::new("board.kicad_pcb", text.to_string())).unwrap();
        assert!(!imported.diagnostics.has_errors(), "{:?}", imported.diagnostics);
        // radius 10mm => diameter 20mm
        assert_eq!(imported.ir.board.width, Length::from_mm(20.0));
        assert_eq!(imported.ir.board.height, Length::from_mm(20.0));
        assert!(imported.ir.board.outline.points.len() >= 8);
    }

    #[test]
    fn warns_for_graphical_gr_circle() {
        let text = r#"
(kicad_pcb
  (version 20240108)
  (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (net 0 "")
  (gr_rect (start 0 0) (end 50 30) (layer "Edge.Cuts") (width 0.1))
  (gr_circle (center 10 10) (end 12 10) (layer "F.SilkS") (width 0.12))
)
"#;
        let imported = import_kicad(&BackendInput::new("board.kicad_pcb", text.to_string())).unwrap();
        assert!(
            imported.diagnostics.iter().any(|d| d.code == "KICAD_013" && d.message.contains("non-Edge.Cuts")),
            "expected KICAD_013 for non-Edge.Cuts gr_circle: {:?}",
            imported.diagnostics
        );
    }

    #[test]
    fn edgecuts_gr_arc_contributes_to_outline_without_graphical_warning() {
        let text = r#"
(kicad_pcb
  (version 20240108)
  (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (gr_line (start 0 0) (end 50 0) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 50 0) (end 50 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 50 30) (end 0 30) (layer "Edge.Cuts") (width 0.1))
  (gr_arc (start 0 30) (mid -5 15) (end 0 0) (layer "Edge.Cuts") (width 0.1))
)
"#;
        let imported = import_kicad(&BackendInput::new("board.kicad_pcb", text.to_string())).unwrap();
        assert!(!imported.diagnostics.has_errors(), "{:?}", imported.diagnostics);
        assert!(
            !imported.diagnostics.iter().any(|d| d.code == "KICAD_013" && d.message.contains("graphical gr_arc")),
            "Edge.Cuts gr_arc should not be warned as graphical: {:?}",
            imported.diagnostics
        );
        assert!(!imported.ir.board.outline.points.is_empty());
        assert!(
            imported.ir.board.width > Length::from_mm(54.0),
            "outline should include arc bulge: width={:?}",
            imported.ir.board.width
        );
    }

    #[test]
    fn warns_for_fp_arc_in_footprint() {
        let text = r#"
(kicad_pcb
  (version 20240108)
  (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (gr_rect (start 0 0) (end 50 30) (layer "Edge.Cuts") (width 0.1))
  (footprint "R_0603"
    (layer "F.Cu")
    (at 10 10)
    (property "Reference" "R1")
    (property "Value" "10k")
    (fp_arc (start -1 0) (mid 0 1) (end 1 0) (layer "F.SilkS") (width 0.12))
    (pad "1" smd rect (at -0.8 0) (size 0.9 0.95) (layers "F.Cu"))
  )
)
"#;
        let imported = import_kicad(&BackendInput::new("board.kicad_pcb", text.to_string())).unwrap();
        assert!(
            imported.diagnostics.iter().any(|d| d.code == "KICAD_013" && d.message.contains("fp_arc")),
            "expected KICAD_013 for fp_arc: {:?}",
            imported.diagnostics
        );
    }