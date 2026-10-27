//! MPOLYGON (gap F13): made from internal points or closed objects, from hatches (`convert`) and
//! back to hatches (HATCHEDIT `mpolygon`), never associative.

use cadcraft_engine::doc::color::Color;
use cadcraft_engine::doc::{EntityKind, Handle, Hatch};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn handle(v: &Value) -> String {
    v["handle"].as_str().unwrap().to_string()
}

fn entity(s: &Session, h: &str) -> cadcraft_engine::doc::Entity {
    (**s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap()).clone()
}

fn hatch(s: &Session, h: &str) -> Hatch {
    match entity(s, h).kind {
        EntityKind::Hatch(x) => x,
        k => panic!("not a hatch: {k:?}"),
    }
}

fn hatches(s: &Session) -> Vec<(String, Hatch)> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some((e.handle.hex(), h.clone())) } else { None }).collect()
}

fn rect(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    handle(&s.execute("rectang", &json!({ "p1": a, "p2": b })).unwrap())
}

#[test]
fn mpolygon_from_objects_and_points() {
    let mut s = Session::new();
    let outer = rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let island = rect(&mut s, [3.0, 3.0], [6.0, 6.0]);
    let m = handle(&s.execute("mpolygon", &json!({ "handles": [outer.clone(), island.clone()], "pattern": "ANSI31", "fillColor": "1" })).unwrap());
    let e = entity(&s, &m);
    assert_eq!((e.kind.type_name(), e.kind.dxf_name()), ("MPolygon", "MPOLYGON"));
    let x = hatch(&s, &m);
    assert_eq!(x.loops.len(), 2);
    assert!(!x.associative && x.loops.iter().all(|l| l.sources.is_empty()), "never associative");
    assert_eq!(x.mpolygon.as_ref().unwrap().fill_color, Color::Index(1));
    // From an internal point, erasing nothing; `erase` removes the objects used.
    let m2 = handle(&s.execute("mpolygon", &json!({ "points": [[1, 1]] })).unwrap());
    assert_eq!(hatch(&s, &m2).loops.len(), 2, "with the island");
    s.execute("mpolygon", &json!({ "handles": [island.clone()], "erase": true })).unwrap();
    assert!(s.doc().unwrap().entity(Handle::parse_hex(&island).unwrap()).is_none());
    // Interactive: HATCH's picks, one MPOLYGON at the end, even with sEparate on.
    s.start("mpolygon").unwrap();
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("mpolygon"));
    assert!(s.prompt_text().starts_with("MPOLYGON"), "{}", s.prompt_text());
    s.input(Input::Keyword("sEparate".into())).unwrap();
    s.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    s.input(Input::Point(Vec2::new(1.0, 1.0))).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(hatches(&s).iter().filter(|(_, h)| h.mpolygon.is_some()).count(), 4);
    // Hostile parameters are refused and add nothing.
    let n = s.doc().unwrap().model.len();
    for bad in [
        json!({ "handles": [outer.clone()], "fillColor": "mauve" }),
        json!({ "handles": [outer.clone()], "pattern": "NOPE" }),
        json!({ "handles": [outer.clone()], "scale": -1 }),
        json!({ "convert": "x" }),
        json!({ "convert": [] }),
        json!({ "convert": ["zz", 5, outer.clone()] }),
        json!({ "points": [[1e308, f64::MAX]] }),
        json!({}),
    ] {
        assert!(s.execute("mpolygon", &bad).is_err(), "{bad}");
        assert_eq!(s.doc().unwrap().model.len(), n, "{bad}");
    }
}

#[test]
fn hatches_and_mpolygons_convert_both_ways() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r.clone()], "pattern": "NET" })).unwrap());
    s.execute("properties.set", &json!({ "handles": [h.clone()], "color": 3 })).unwrap();
    s.execute("mpolygon", &json!({ "convert": [h.clone()] })).unwrap();
    let x = hatch(&s, &h);
    assert!(!x.associative);
    assert_eq!(x.mpolygon.as_ref().unwrap().fill_color, Color::Index(3), "filled as it was drawn");
    // Linking an MPOLYGON to objects is refused quietly: it stays non-associative.
    s.execute("hatchedit", &json!({ "handles": [h.clone()], "associative": true })).unwrap();
    assert!(!hatch(&s, &h).associative);
    // Back: a hatch in the fill colour, its boundary recreated as polylines it is linked to.
    s.execute("properties.set", &json!({ "handles": [h.clone()], "color": 5 })).unwrap();
    let out = s.execute("hatchedit", &json!({ "handles": [h.clone()], "mpolygon": false })).unwrap();
    let made = out["handles"].as_array().unwrap();
    assert_eq!(made.len(), 1);
    let x = hatch(&s, &h);
    assert!(x.mpolygon.is_none() && x.associative);
    assert_eq!(entity(&s, &h).common.color, Color::Index(3));
    let b = made[0].as_str().unwrap();
    assert_eq!(entity(&s, b).common.color, Color::Index(5), "the boundary keeps the MPOLYGON's colour");
    assert_eq!(x.loops[0].sources, vec![Handle::parse_hex(b).unwrap()]);
    // HATCHEDIT can make one too; a bad value is refused.
    s.execute("hatchedit", &json!({ "handles": [h.clone()], "mpolygon": true })).unwrap();
    assert!(hatch(&s, &h).mpolygon.is_some());
    assert!(s.execute("hatchedit", &json!({ "handles": [h.clone()], "mpolygon": "yes" })).is_err());
    // Undo goes back to the hatch.
    s.execute("undo", &json!({})).unwrap();
    assert!(hatch(&s, &h).mpolygon.is_none());
}

#[test]
fn an_mpolygon_survives_a_dxf_save() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [10.0, 10.0]);
    s.execute("mpolygon", &json!({ "points": [[5, 5]], "pattern": "SOLID", "fillColor": "2" })).unwrap();
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let m = back.model.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { h.mpolygon.clone() } else { None });
    assert_eq!(m.map(|m| m.fill_color), Some(Color::Index(2)));
}
