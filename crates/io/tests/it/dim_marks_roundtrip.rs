//! Dimension edits survive DXF: the oblique angle as group 52 (relative to the dimension line,
//! DXF Reference), jogs, breaks and inspection frames as CADCraft xdata. Damaged xdata is
//! skipped.

use cadcraft_doc::{Common, DimBreak, DimKind, DimMarks, Dimension, Drawing, EntityKind, Inspection, JoggedRadius, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, write_dxf};

fn dim(kind: DimKind, marks: DimMarks) -> EntityKind {
    EntityKind::Dimension(Dimension {
        kind,
        defpt: Vec3::new(0.0, 3.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::new(3.0, 4.0, 0.0),
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks,
    })
}

fn marks_of(d: &Drawing) -> Vec<DimMarks> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm.marks.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn dimension_marks_roundtrip() {
    let mut d = Drawing::new_imperial();
    let linear = DimMarks {
        oblique: Some(60f64.to_radians()),
        jog_line: Some(Vec3::new(2.0, 3.0, 0.0)),
        breaks: vec![
            DimBreak { at: Vec3::new(4.0, 3.0, 0.0), size: 0.125, auto: true, object: None },
            DimBreak { at: Vec3::new(7.0, 3.0, 0.0), size: 0.5, auto: false, object: None },
        ],
        inspect: Some(Inspection { shape: 1, label: "QC".into(), rate: "25%".into() }),
        ..Default::default()
    };
    let aligned = DimMarks { oblique: Some(100f64.to_radians()), ..Default::default() };
    let jogged = DimMarks { jogged: Some(JoggedRadius { center: Vec3::new(1.0, -2.0, 0.0), jog: Vec3::new(2.0, 1.0, 0.0) }), ..Default::default() };
    for (k, m) in [(DimKind::Linear { rotation: 0.0 }, &linear), (DimKind::Aligned, &aligned), (DimKind::Radius, &jogged)] {
        d.add(&Space::Model, Common::default(), dim(k, m.clone())).unwrap();
    }
    let text = write_dxf(&d);
    // The oblique angle is the standard group 52, not xdata.
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert!(lines.windows(2).any(|w| w[0] == "52" && (w[1].parse::<f64>().unwrap() - 60.0).abs() < 1e-9));
    let back = read_dxf(text.as_bytes()).unwrap();
    let got = marks_of(&back);
    assert_eq!(got.len(), 3);
    let close = |a: Option<f64>, b: Option<f64>| a.zip(b).is_some_and(|(a, b)| (a - b).abs() < 1e-9);
    assert!(close(got[0].oblique, linear.oblique) && close(got[1].oblique, aligned.oblique));
    assert_eq!(DimMarks { oblique: None, ..got[0].clone() }, DimMarks { oblique: None, ..linear.clone() });
    assert_eq!(got[2], jogged);
    // And again: nothing piles up.
    assert_eq!(marks_of(&read_dxf(write_dxf(&back).as_bytes()).unwrap()).len(), 3);
}

#[test]
fn damaged_marks_are_skipped() {
    let mut d = Drawing::new_imperial();
    let m = DimMarks {
        jog_line: Some(Vec3::new(2.0, 3.0, 0.0)),
        breaks: vec![DimBreak { at: Vec3::new(4.0, 3.0, 0.0), size: 0.125, auto: false, object: None }],
        inspect: Some(Inspection { shape: 0, label: "A".into(), rate: "B".into() }),
        ..Default::default()
    };
    d.add(&Space::Model, Common::default(), dim(DimKind::Linear { rotation: 0.0 }, m)).unwrap();
    let text = write_dxf(&d);
    // A break size of NaN, an inspection shape out of range, a jog point cut short.
    let bad = text.replacen("1040\r\n0.125\r\n", "1040\r\nnan\r\n", 1).replacen("INSPECT\r\n1070\r\n0\r\n", "INSPECT\r\n1070\r\n77\r\n", 1).replacen(
        "JOGLINE\r\n1010\r\n",
        "JOGLINE\r\n1000\r\n",
        1,
    );
    assert_ne!(bad, text);
    let back = read_dxf(bad.as_bytes()).unwrap();
    let got = marks_of(&back);
    assert_eq!(got.first(), Some(&DimMarks::default()), "{got:?}");
}

/// An automatic DIMBREAK break names the object that makes it (`1005` in the BREAK item), in DXF
/// and through the DWG bridge, so it keeps following that object after the file is reopened.
#[test]
fn auto_breaks_keep_the_object_that_makes_them() {
    let mut d = Drawing::new_imperial();
    let line = d
        .add(&Space::Model, Common::default(), EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(4.0, -1.0, 0.0), b: Vec3::new(4.0, 5.0, 0.0) }))
        .unwrap();
    let marks = DimMarks {
        breaks: vec![
            DimBreak { at: Vec3::new(4.0, 3.0, 0.0), size: 0.3, auto: true, object: Some(line) },
            DimBreak { at: Vec3::new(7.0, 3.0, 0.0), size: 0.5, auto: false, object: None },
        ],
        ..Default::default()
    };
    d.add(&Space::Model, Common::default(), dim(DimKind::Linear { rotation: 0.0 }, marks)).unwrap();
    let check = |back: &Drawing, what: &str| {
        let line = back.model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).map(|e| e.handle);
        let br = marks_of(back).remove(0).breaks;
        assert_eq!(br.len(), 2, "{what}");
        assert_eq!((br[0].auto, br[0].object), (true, line), "{what}: the auto break names the line");
        assert_eq!((br[1].auto, br[1].object), (false, None), "{what}");
    };
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    check(&back, "DXF");
    check(&read_dxf(write_dxf(&back).as_bytes()).unwrap(), "DXF again");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dwg = cadcraft_io::write(&back, "a.dwg").unwrap();
        check(&cadcraft_io::read(&dwg, "a.dwg").unwrap(), "DWG");
    }
    // A damaged or null handle is no object.
    let text = write_dxf(&d);
    let bad = text.replacen(&format!("1005\r\n{}\r\n", line.hex()), "1005\r\n0\r\n", 1);
    assert_ne!(bad, text);
    let br = marks_of(&read_dxf(bad.as_bytes()).unwrap()).remove(0).breaks;
    assert_eq!(br.first().map(|b| b.object), Some(None));
}
