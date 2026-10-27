//! Gap F7: the dimension style settings DIMALTTD / DIMALTTZ (alternate-unit tolerances), DIMUPT
//! (text placed with the dimension line), DIMTXTDIRECTION (reading direction), the jog height
//! factor and the DIMBREAK size survive DXF in every version CADCraft writes and the DWG bridge:
//! as DIMSTYLE groups where the version has them, else in the style's CADCRAFT "DIMVARS" xdata;
//! the jog height and break size in the `ACAD_DSTYLE_DIMJAG` / `ACAD_DSTYLE_DIMBREAK` style
//! xdata. Per-dimension overrides of them survive too.

use cadcraft_doc::{Common, DimKind, DimMarks, DimStyle, Dimension, Drawing, EntityKind, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read, write_with};

fn styled() -> DimStyle {
    DimStyle {
        name: "Styled".into(),
        alt: true,
        alt_tol_decimals: 3,
        alt_tol_zero_suppression: 8,
        user_positioned_text: true,
        text_direction: 1,
        jog_height: 2.25,
        break_size: 0.3,
        tolerance: true,
        tol_plus: 0.01,
        tol_minus: 0.02,
        ..DimStyle::default()
    }
}

fn dimension(style: &str, overrides: serde_json::Value) -> EntityKind {
    EntityKind::Dimension(Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 3.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: style.into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: overrides.as_object().cloned().unwrap_or_default(),
        assoc: Vec::new(),
        marks: DimMarks::default(),
    })
}

/// A drawing with the style, a dimension using it, and one overriding every new setting.
fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.dim_styles.push(styled());
    let mut iso = DimStyle::iso25();
    iso.name = "Metric".into();
    d.dim_styles.push(iso);
    d.add(&Space::Model, Common::default(), dimension("Styled", serde_json::json!({}))).unwrap();
    let ov = serde_json::json!({
        "DIMALTTD": 1, "DIMALTTZ": 4, "DIMUPT": 1, "DIMTXTDIRECTION": 1, "jogHeight": 3.5, "breakSize": 0.75,
    });
    d.add(&Space::Model, Common::default(), dimension("Standard", ov)).unwrap();
    d
}

fn save(d: &Drawing, name: &str, version: Version) -> Vec<u8> {
    write_with(d, name, &SaveOptions { version: Some(version), ..Default::default() }).unwrap()
}

fn dims(d: &Drawing) -> Vec<Dimension> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm.clone()),
            _ => None,
        })
        .collect()
}

fn check(back: &Drawing, what: &str) {
    assert_eq!(back.dim_style("Styled"), Some(&styled()), "{what}");
    let metric = back.dim_style("Metric").unwrap();
    assert_eq!((metric.break_size, metric.jog_height), (3.75, 1.5), "{what}");
    let ds = dims(back);
    assert_eq!(ds.len(), 2, "{what}");
    let st = DimStyle::default().with_overrides(&ds[1].overrides);
    assert_eq!(
        (st.alt_tol_decimals, st.alt_tol_zero_suppression, st.user_positioned_text, st.text_direction),
        (1, 4, true, 1),
        "{what}: {:?}",
        ds[1].overrides
    );
    assert_eq!((st.jog_height, st.break_size), (3.5, 0.75), "{what}");
}

#[test]
fn new_style_settings_roundtrip_in_every_dxf_version() {
    let d = drawing();
    let first = read(&save(&d, "a.dxf", Version::R2018), "a.dxf").unwrap();
    check(&first, "R2018");
    for v in [Version::R2000, Version::R2004, Version::R2007, Version::R2010, Version::R2013, Version::R2018] {
        let back = read(&save(&first, "a.dxf", v), "a.dxf").unwrap();
        check(&back, v.year());
        // Read again and saved, nothing more comes in (the style xdata isn't also kept raw).
        let again = read(&save(&back, "a.dxf", v), "a.dxf").unwrap();
        assert_eq!(again.dim_styles, back.dim_styles, "{}", v.year());
        assert_eq!(dims(&again), dims(&back), "{}", v.year());
    }
}

#[test]
fn groups_by_version_and_style_xdata() {
    let d = drawing();
    let text = |v: Version| String::from_utf8(save(&d, "a.dxf", v)).unwrap();
    let record = |dxf: &str| -> Vec<String> {
        let lines: Vec<String> = dxf.lines().map(|l| l.trim().to_string()).collect();
        let at = lines.windows(2).position(|w| w[0] == "2" && w[1] == "Styled").unwrap();
        // The record ends at the next group 0 (codes sit at even offsets from the name's code).
        let end = (at..lines.len()).step_by(2).find(|i| lines[*i] == "0").unwrap_or(lines.len());
        lines[at..end].to_vec()
    };
    let has = |rec: &[String], code: &str, value: &str| rec.windows(2).any(|w| w[0] == code && w[1] == value);
    // R2010: DIMTXTDIRECTION 294 is a group; R2007 and older keep it in xdata.
    let r2010 = record(&text(Version::R2010));
    assert!(has(&r2010, "294", "1"), "{r2010:?}");
    assert!(has(&r2010, "274", "3") && has(&r2010, "286", "8") && has(&r2010, "288", "1"));
    let r2007 = record(&text(Version::R2007));
    assert!(!has(&r2007, "294", "1") && has(&r2007, "1070", "294"), "DIMTXTDIRECTION is a 2010 group: {r2007:?}");
    let r2000 = record(&text(Version::R2000));
    assert!(has(&r2000, "1070", "294"), "{r2000:?}");
    // The jog height factor and break size: style xdata, applications registered.
    for rec in [&r2000, &r2010] {
        let jag = rec.iter().position(|l| l == "ACAD_DSTYLE_DIMJAG").unwrap();
        assert_eq!(rec[jag + 1..jag + 5], ["1070", "388", "1040", "2.25"]);
        let brk = rec.iter().position(|l| l == "ACAD_DSTYLE_DIMBREAK").unwrap();
        assert_eq!(rec[brk + 1..brk + 5], ["1070", "391", "1040", "0.3"]);
    }
    let all = text(Version::R2018);
    for app in ["ACAD_DSTYLE_DIMJAG", "ACAD_DSTYLE_DIMBREAK"] {
        assert!(all.lines().map(str::trim).collect::<Vec<_>>().windows(4).any(|w| w[0] == "AcDbRegAppTableRecord" && w[2] == app), "APPID {app}");
    }
    // Styles with the default jog height and break size write no xdata, and the applications
    // are registered only when used.
    let plain = String::from_utf8(save(&Drawing::new_imperial(), "a.dxf", Version::R2018)).unwrap();
    assert!(!plain.contains("ACAD_DSTYLE_DIMJAG") && !plain.contains("ACAD_DSTYLE_DIMBREAK"));
    // The header carries the current style's values (DIMSTYLE is Standard here: the defaults).
    assert!(all.contains("$DIMALTTD") && all.contains("$DIMTXTDIRECTION") && all.contains("$DIMUPT"));
}

#[test]
fn a_file_with_bad_style_xdata_keeps_the_defaults() {
    let d = drawing();
    let text = String::from_utf8(save(&d, "a.dxf", Version::R2018)).unwrap();
    let bad = text.replacen("388\r\n1040\r\n2.25", "388\r\n1040\r\nnan", 1).replacen("391\r\n1040\r\n0.3", "391\r\n1040\r\n-4", 1);
    assert_ne!(bad, text);
    let back = read(bad.as_bytes(), "a.dxf").unwrap();
    let st = back.dim_style("Styled").unwrap();
    assert_eq!((st.jog_height, st.break_size), (1.5, 0.125));
    // Group 294 is a DXF boolean (290–299): any other value reads as on, never out of range.
    let bad = text.replacen("\r\n294\r\n1\r\n", "\r\n294\r\n77\r\n", 1);
    assert_ne!(bad, text);
    let st = read(bad.as_bytes(), "a.dxf").unwrap().dim_style("Styled").cloned().unwrap();
    assert_eq!(st.text_direction, 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn new_style_settings_survive_the_dwg_bridge() {
    let first = read(&save(&drawing(), "a.dxf", Version::R2018), "a.dxf").unwrap();
    let dwg = cadcraft_io::write(&first, "a.dwg").unwrap();
    let back = read(&dwg, "a.dwg").unwrap();
    check(&back, "DWG");
}
