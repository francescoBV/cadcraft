//! Gap F7: the dimension style settings DIMALTTD/DIMALTTZ (alternate-unit tolerances), DIMUPT,
//! DIMTXTDIRECTION, the jog height factor and the break size, set through DIMSTYLE, SETVAR and
//! DIMOVERRIDE, and what they change: the alternate tolerances' precision, the text's reading
//! direction, the height of a DIMJOGLINE zigzag and the gap DIMBREAK Auto cuts.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Dimension, EntityKind, Handle};
use serde_json::{Value, json};

fn dim(s: &Session, h: Handle) -> Dimension {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(d) => d.clone(),
        k => panic!("not a dimension: {k:?}"),
    }
}

fn add(s: &mut Session, cmd: &str, p: Value) -> Handle {
    let r = s.execute(cmd, &p).unwrap();
    match r.get("handle").and_then(Value::as_str) {
        Some(h) => Handle::parse_hex(h).unwrap(),
        None => s.doc().unwrap().model.last().unwrap().handle,
    }
}

#[test]
fn new_style_settings_through_dimstyle_setvar_and_dimoverride() {
    let mut s = Session::new();
    let r = s
        .execute(
            "dimstyle",
            &json!({ "name": "F7", "DIMALTTD": 3, "DIMALTTZ": 8, "DIMUPT": 1, "DIMTXTDIRECTION": 1, "jogHeight": 2, "breakSize": 0.25 }),
        )
        .unwrap();
    assert!(r.get("ignored").is_none_or(|i| i.as_array().is_some_and(Vec::is_empty)), "{r}");
    let st = s.doc().unwrap().dim_style("F7").cloned().unwrap();
    assert_eq!((st.alt_tol_decimals, st.alt_tol_zero_suppression), (3, 8));
    assert!(st.user_positioned_text && st.text_direction == 1);
    assert_eq!((st.jog_height, st.break_size), (2.0, 0.25));
    // The DIM* system variables read the current style.
    assert_eq!(s.execute("getvar", &json!({ "name": "DIMALTTD" })).unwrap()["value"], json!(3));
    assert_eq!(s.execute("getvar", &json!({ "name": "DIMTXTDIRECTION" })).unwrap()["value"], json!(1));
    s.execute("setvar", &json!({ "name": "DIMALTTZ", "value": 4 })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "DIMALTTZ" })).unwrap()["value"], json!(4));
    // Out of range: refused.
    for bad in [json!({ "name": "F7", "jogHeight": 0 }), json!({ "name": "F7", "breakSize": -1 }), json!({ "name": "F7", "DIMTXTDIRECTION": 4 })] {
        let r = s.execute("dimstyle", &bad).unwrap();
        assert_eq!(r["ignored"].as_array().unwrap().len(), 1, "{bad}");
    }
    assert!(s.execute("setvar", &json!({ "name": "DIMTXTDIRECTION", "value": 9 })).is_err());
    // Per dimension: the jog height factor sets the zigzag's height (4 × the text height).
    s.execute("dimstyle.current", &json!({ "name": "Standard" })).unwrap();
    let h = add(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 2] }));
    s.execute("dimoverride", &json!({ "handles": [h.hex()], "jogHeight": 4 })).unwrap();
    assert_eq!(dim(&s, h).overrides.get("jogHeight"), Some(&json!(4)));
    s.execute("dimjogline", &json!({ "handle": h.hex(), "at": [5, 2] })).unwrap();
    let g = cadcraft_engine::render::dimension_in(s.doc().unwrap(), &dim(&s, h));
    let zig = g.lines.iter().find(|l| l.len() == 4).expect("the jog");
    let height = zig.iter().map(|p| p.y).fold(f64::MIN, f64::max) - zig.iter().map(|p| p.y).fold(f64::MAX, f64::min);
    let th = s.doc().unwrap().dim_style("Standard").unwrap().text_height;
    assert!((height - 4.0 * th).abs() < 1e-9, "{height} vs {}", 4.0 * th);
}

#[test]
fn alternate_tolerances_use_their_own_precision_and_text_can_read_right_to_left() {
    let mut s = Session::new();
    s.execute(
        "dimstyle",
        &json!({ "name": "T", "DIMTOL": 1, "DIMTP": 0.01, "DIMTM": 0.02, "DIMALT": 1, "DIMALTF": 25.4, "DIMALTD": 1, "DIMALTTD": 3 }),
    )
    .unwrap();
    let h = add(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [2, 0], "at": [1, 1] }));
    let g = cadcraft_engine::render::dimension_in(s.doc().unwrap(), &dim(&s, h));
    // [50.8 +0.254 -0.508]: the alternate tolerances with DIMALTTD's three places.
    assert!(g.mtext.contains("[50.8") && g.mtext.contains("+0.254") && g.mtext.contains("-0.508"), "{}", g.mtext);
    // Equal tolerances: ± with DIMALTTD's places.
    s.execute("dimstyle", &json!({ "name": "T", "DIMTM": 0.01 })).unwrap();
    let h = add(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [2, 0], "at": [1, 1] }));
    let g = cadcraft_engine::render::dimension_in(s.doc().unwrap(), &dim(&s, h));
    assert!(g.mtext.contains("[50.8%%p0.254]"), "{}", g.mtext);
    // DIMTXTDIRECTION 1: right to left.
    s.execute("dimstyle", &json!({ "name": "R", "DIMTXTDIRECTION": 1, "DIMDEC": 2, "DIMTOL": 0, "DIMALT": 0 })).unwrap();
    let h = add(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [2.5, 0], "at": [1, 1] }));
    let g = cadcraft_engine::render::dimension_in(s.doc().unwrap(), &dim(&s, h));
    assert_eq!(g.mtext, "05.2");
}

#[test]
fn dimbreak_auto_cuts_the_style_break_size_times_dimscale() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({ "name": "Standard", "breakSize": 0.5 })).unwrap();
    let d = add(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 2] }));
    add(&mut s, "line", json!({ "points": [[4, -1], [4, 5]] }));
    s.execute("dimbreak", &json!({ "handles": [d.hex()] })).unwrap();
    let breaks = dim(&s, d).marks.breaks;
    assert!(!breaks.is_empty() && breaks.iter().all(|b| b.auto && (b.size - 0.5).abs() < 1e-12), "{breaks:?}");
    // The style's overall scale multiplies it; a per-dimension override wins; `size` wins over both.
    s.execute("dimstyle", &json!({ "name": "Standard", "DIMSCALE": 2 })).unwrap();
    s.execute("dimbreak", &json!({ "handles": [d.hex()] })).unwrap();
    assert!(dim(&s, d).marks.breaks.iter().all(|b| (b.size - 1.0).abs() < 1e-12));
    s.execute("dimoverride", &json!({ "handles": [d.hex()], "breakSize": 0.25 })).unwrap();
    s.execute("dimbreak", &json!({ "handles": [d.hex()] })).unwrap();
    assert!(dim(&s, d).marks.breaks.iter().all(|b| (b.size - 0.5).abs() < 1e-12));
    s.execute("dimbreak", &json!({ "handles": [d.hex()], "size": 0.1 })).unwrap();
    assert!(dim(&s, d).marks.breaks.iter().all(|b| (b.size - 0.1).abs() < 1e-12));
}
