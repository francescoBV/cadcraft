//! Gap F7: the smart DIM command (JSON and prompts, hover preview, options) and DIMLAYER, the
//! layer new dimensions go on.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{DimKind, Dimension, EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::{Value, json};

fn hex(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

fn dims(s: &Session) -> Vec<(Handle, Dimension, String)> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Dimension(d) => Some((e.handle, d.clone(), e.common.layer.clone())),
            _ => None,
        })
        .collect()
}

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
fn dim_json_makes_the_dimension_that_suits_the_object() {
    let mut s = Session::new();
    let slanted = add(&mut s, "line", json!({ "points": [[0, 0], [8, 6]] }));
    let flat = add(&mut s, "line", json!({ "points": [[0, -5], [10, -5]] }));
    let circle = add(&mut s, "circle", json!({ "center": [20, 0], "radius": 3 }));
    let arc = add(&mut s, "arc", json!({ "center": [30, 0], "radius": 2, "start": 0, "end": 90 }));
    let kind = |s: &mut Session, p: Value| {
        let h = hex(&s.execute("dim", &p).unwrap());
        dim(s, h).kind
    };
    // A slanted line: aligned within its span, horizontal or vertical beyond its ends.
    assert_eq!(kind(&mut s, json!({ "object": slanted.hex(), "at": [2, 5] })), DimKind::Aligned);
    assert!(matches!(kind(&mut s, json!({ "object": slanted.hex(), "at": [4, 12] })), DimKind::Linear { rotation } if rotation == 0.0));
    assert!(matches!(kind(&mut s, json!({ "object": flat.hex(), "at": [5, -7] })), DimKind::Linear { rotation } if rotation == 0.0));
    assert_eq!(kind(&mut s, json!({ "object": circle.hex(), "at": [25, 2] })), DimKind::Diameter);
    assert_eq!(kind(&mut s, json!({ "object": circle.hex(), "at": [25, 2], "type": "radius" })), DimKind::Radius);
    assert_eq!(kind(&mut s, json!({ "object": arc.hex(), "at": [33, 2] })), DimKind::Radius);
    assert_eq!(kind(&mut s, json!({ "object": arc.hex(), "at": [33, 2], "type": "arc" })), DimKind::ArcLength);
    assert_eq!(kind(&mut s, json!({ "object": arc.hex(), "at": [33, 2], "type": "angular" })), DimKind::Angular3P);
    // Two lines: the angle between them (a two-line angular dimension, as DIMANGULAR makes);
    // two parallel lines: the distance between them.
    let other = add(&mut s, "line", json!({ "points": [[0, 0], [10, 0]] }));
    assert_eq!(kind(&mut s, json!({ "object": slanted.hex(), "second": other.hex(), "at": [5, 1] })), DimKind::Angular);
    let h = hex(&s.execute("dim", &json!({ "object": flat.hex(), "second": other.hex(), "at": [12, -2] })).unwrap());
    let d = dim(&s, h);
    assert_eq!(d.kind, DimKind::Aligned);
    assert!((d.p13.xy().dist(d.p14.xy()) - 5.0).abs() < 1e-9);
    // Points as DIMLINEAR; the options run the other dimension commands.
    assert!(matches!(kind(&mut s, json!({ "p1": [0, 0], "p2": [3, 4], "at": [1, 6] })), DimKind::Linear { .. }));
    assert_eq!(kind(&mut s, json!({ "p1": [0, 0], "p2": [3, 4], "at": [1, 6], "aligned": true })), DimKind::Aligned);
    let r = s.execute("dim", &json!({ "option": "continue", "points": [[6, 8]] })).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 1);
    let r = s.execute("dim", &json!({ "option": "ordinate", "feature": [1, 1], "leader": [1, 4] })).unwrap();
    assert!(matches!(dim(&s, hex(&r)).kind, DimKind::Ordinate { .. }));
    // Errors, not panics.
    for p in [
        json!({ "object": "FFFFFF", "at": [0, 0] }),
        json!({ "object": circle.hex(), "at": [0, 0], "type": "arc" }),
        json!({ "object": circle.hex(), "at": [0, 0], "type": "bogus" }),
        json!({ "p1": [0, 0], "p2": [0, 0], "at": [1, 1] }),
        json!({ "option": "nope" }),
        json!({ "option": "layer", "name": "NoSuchLayer" }),
        json!({ "object": flat.hex(), "second": flat.hex(), "at": [1, 1] }),
        json!({ "p1": [f64::MAX, 0], "p2": [1, "x"], "at": null }),
    ] {
        assert!(s.execute("dim", &p).is_err(), "{p}");
    }
}

#[test]
fn dimlayer_puts_new_dimensions_on_its_layer() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "QUOTE" })).unwrap();
    s.execute("dim", &json!({ "option": "layer", "name": "quote" })).unwrap();
    assert_eq!(s.doc().unwrap().header.str("DIMLAYER", ""), "QUOTE");
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [4, 0], "at": [2, 1] })).unwrap();
    s.execute("dim", &json!({ "p1": [0, 0], "p2": [0, 4], "at": [-1, 2] })).unwrap();
    s.execute("dimbaseline", &json!({ "points": [[0, 6]] })).unwrap();
    s.execute("qdim", &json!({ "points": [[0, 0], [5, 0]], "at": [2, -2] })).ok();
    assert!(dims(&s).iter().all(|(_, _, l)| l == "QUOTE"), "{:?}", dims(&s).iter().map(|d| d.2.clone()).collect::<Vec<_>>());
    // "." is the current layer again; a `layer` parameter wins for one call.
    s.execute("setvar", &json!({ "name": "DIMLAYER", "value": "." })).unwrap();
    let h = hex(&s.execute("dim", &json!({ "p1": [0, 0], "p2": [4, 0], "at": [2, 3] })).unwrap());
    assert_eq!(s.doc().unwrap().entity(h).unwrap().common.layer, "0");
    let h = hex(&s.execute("dim", &json!({ "p1": [0, 0], "p2": [4, 0], "at": [2, 4], "layer": "QUOTE" })).unwrap());
    assert_eq!(s.doc().unwrap().entity(h).unwrap().common.layer, "QUOTE");
}

#[test]
fn dim_prompts_pick_objects_place_and_repeat() {
    let mut s = Session::new();
    s.cmdline("line 0,0 10,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("circle 20,0 3").unwrap();
    s.cmdline("dim").unwrap();
    assert!(s.prompt_text().contains("Select objects or specify first extension line origin"), "{}", s.prompt_text());
    assert!(s.prompt_text().contains("Layer"));
    // Hover: the line previews a horizontal dimension, the circle a diameter, empty space nothing.
    let hover = |s: &Session, p: Vec2| -> Option<DimKind> {
        s.preview(p).into_iter().find_map(|e| if let EntityKind::Dimension(d) = e.kind { Some(d.kind) } else { None })
    };
    assert!(matches!(hover(&s, Vec2::new(5.0, 0.0)), Some(DimKind::Linear { .. })));
    assert_eq!(hover(&s, Vec2::new(23.0, 0.0)), Some(DimKind::Diameter));
    assert_eq!(hover(&s, Vec2::new(5.0, 40.0)), None);
    // Pick the line, place it; DIM asks again.
    s.cmdline("5,0").unwrap();
    assert!(s.prompt_text().contains("second line for angle"), "{}", s.prompt_text());
    s.cmdline("5,2").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "repeats: {}", s.prompt_text());
    // The circle: Radius instead of the diameter.
    s.cmdline("23,0").unwrap();
    assert!(s.prompt_text().contains("diameter dimension location"), "{}", s.prompt_text());
    s.cmdline("r").unwrap();
    assert!(s.prompt_text().contains("radius dimension location"), "{}", s.prompt_text());
    s.cmdline("26,3").unwrap();
    // Two points in empty space: a linear dimension.
    s.cmdline("0,30").unwrap();
    s.cmdline("10,30").unwrap();
    s.cmdline("5,33").unwrap();
    // Undo takes back the last one.
    s.cmdline("u").unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().starts_with("Command"), "{}", s.prompt_text());
    let kinds: Vec<DimKind> = dims(&s).into_iter().map(|d| d.1.kind).collect();
    assert_eq!(kinds.len(), 2, "{kinds:?}");
    assert!(matches!(kinds[0], DimKind::Linear { .. }));
    assert_eq!(kinds[1], DimKind::Radius);
}

#[test]
fn dim_prompts_angle_between_lines_and_sub_commands() {
    let mut s = Session::new();
    s.cmdline("line 0,0 10,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("line 0,0 0,10").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("dim").unwrap();
    s.cmdline("5,0").unwrap();
    // A second line instead of a location: the angle.
    s.cmdline("0,5").unwrap();
    assert!(s.prompt_text().contains("angular dimension location"), "{}", s.prompt_text());
    s.cmdline("3,3").unwrap();
    assert_eq!(dims(&s).last().unwrap().1.kind, DimKind::Angular);
    // Ordinate inside DIM, then back to DIM's prompt.
    s.cmdline("o").unwrap();
    assert!(s.prompt_text().contains("feature location"), "{}", s.prompt_text());
    s.cmdline("10,0").unwrap();
    s.cmdline("10,-3").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    assert!(matches!(dims(&s).last().unwrap().1.kind, DimKind::Ordinate { .. }));
    // Layer: by name, then use current.
    s.execute("layer.new", &json!({ "name": "DIMS" })).ok();
    s.cmdline("la").unwrap();
    s.cmdline("DIMS").unwrap();
    assert_eq!(s.doc().unwrap().header.str("DIMLAYER", ""), "DIMS");
    s.cmdline("20,20").unwrap();
    s.cmdline("30,20").unwrap();
    s.cmdline("25,22").unwrap();
    assert_eq!(dims(&s).last().unwrap().2, "DIMS");
    s.cmdline("la").unwrap();
    s.cmdline("u").unwrap();
    assert_eq!(s.doc().unwrap().header.str("DIMLAYER", "."), "");
    s.cmdline("").unwrap();
}

#[test]
fn hostile_smart_dim_inputs_do_not_panic() {
    let mut s = Session::new();
    s.cmdline("line 0,0 10,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("dim").unwrap();
    for t in ["1e308,1e308", "nan,0", "la", "", "5,0", "1e308,-1e308", "a", "u", "b", "c", "", "", "", "o", "", "", ""] {
        let _ = s.cmdline(t);
        let _ = s.preview(Vec2::new(f64::MAX, f64::MIN));
        let _ = s.preview(Vec2::new(5.0, 0.0));
    }
    for p in [json!(null), json!([]), json!({ "object": 5, "at": "x" }), json!({ "option": 7 }), json!({ "option": "baseline" })] {
        let _ = s.execute("dim", &p);
    }
}
