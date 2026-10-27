//! Isolation saved with the drawing (OBJECTISOLATIONMODE 1): ISOLATEOBJECTS and HIDEOBJECTS make
//! the objects invisible, so they stay hidden through DXF and DWG until UNISOLATEOBJECTS shows
//! them; hidden objects can't be selected; mode 0 keeps hiding for the session only.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::objects_isolated;
use cadcraft_engine::doc::{Handle, Space};
use cadcraft_engine::geom::{Bounds2, Vec2};
use serde_json::{Value, json};

fn hx(v: &Value) -> Handle {
    Handle::parse_hex(v.as_str().unwrap()).unwrap()
}

/// Two lines and a circle.
fn three(s: &mut Session) -> (Handle, Handle, Handle) {
    let a = hx(&s.execute("line", &json!({ "points": [[1, 1], [2, 1]] })).unwrap()["handles"][0]);
    let b = hx(&s.execute("line", &json!({ "points": [[1, 3], [20, 3]] })).unwrap()["handles"][0]);
    let c = hx(&s.execute("circle", &json!({ "center": [30, 30], "radius": 1 })).unwrap()["handle"]);
    (a, b, c)
}

#[test]
fn saved_isolation_survives_dxf_and_unisolate_shows_it() {
    let mut s = Session::new();
    let (a, b, c) = three(&mut s);
    assert!(!objects_isolated(s.doc().unwrap()));
    s.execute("setvar", &json!({ "name": "OBJECTISOLATIONMODE", "value": 1 })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "OBJECTISOLATIONMODE" })).unwrap()["value"], json!(1));
    let r = s.execute("isolateobjects", &json!({ "handles": [a.hex()] })).unwrap();
    assert_eq!(r["hidden"], json!(2));
    assert!(objects_isolated(s.doc().unwrap()));
    let d = s.doc().unwrap();
    assert!(d.hidden_objects.is_empty(), "saved isolation makes the objects invisible instead");
    assert!(!d.entity(b).unwrap().common.visible && !d.entity(c).unwrap().common.visible);
    // Through DXF: still hidden when reopened.
    let again = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let mut s2 = Session::new();
    s2.open_drawing(again, "iso.dxf", None);
    let d2 = s2.doc().unwrap();
    assert!(d2.entity(a).unwrap().common.visible);
    assert!(!d2.entity(b).unwrap().common.visible && !d2.entity(c).unwrap().common.visible);
    assert!(objects_isolated(d2));
    // Hidden objects can't be picked or windowed.
    assert!(cadcraft_engine::select::pick(d2, &Space::Model, Vec2::new(10.0, 3.0), 0.1).is_none());
    let all = Bounds2::new(Vec2::new(-100.0, -100.0), Vec2::new(100.0, 100.0));
    assert_eq!(cadcraft_engine::select::select_window(d2, &Space::Model, all, true), vec![a]);
    // UNISOLATEOBJECTS shows them again, undoably.
    let r = s2.execute("unisolateobjects", &json!({})).unwrap();
    assert_eq!(r["shown"], json!(2));
    assert!(s2.doc().unwrap().entity(b).unwrap().common.visible);
    assert!(!objects_isolated(s2.doc().unwrap()));
    s2.execute("undo", &json!({})).unwrap();
    assert!(!s2.doc().unwrap().entity(b).unwrap().common.visible);
    s2.execute("redo", &json!({})).unwrap();
    assert!(s2.doc().unwrap().entity(b).unwrap().common.visible);
    assert_eq!(s2.execute("unisolateobjects", &json!({})).unwrap()["shown"], json!(0));
}

#[test]
fn saved_isolation_survives_dwg() {
    let mut s = Session::new();
    let (_, b, _) = three(&mut s);
    s.execute("setvar", &json!({ "name": "OBJECTISOLATIONMODE", "value": 1 })).unwrap();
    s.execute("hideobjects", &json!({ "handles": [b.hex()] })).unwrap();
    let bytes = cadcraft_io::write(s.doc().unwrap(), "iso.dwg").unwrap();
    let back = cadcraft_io::read(&bytes, "iso.dwg").unwrap();
    let hidden: Vec<Handle> = back.model.iter().filter(|e| !e.common.visible).map(|e| e.handle).collect();
    assert_eq!(hidden.len(), 1, "{hidden:?}");
    assert!(back.model.iter().filter(|e| e.common.visible).count() >= 2);
}

#[test]
fn session_isolation_never_reaches_the_file() {
    let mut s = Session::new();
    let (a, b, _) = three(&mut s);
    s.execute("hideobjects", &json!({ "handles": [b.hex()] })).unwrap();
    assert!(objects_isolated(s.doc().unwrap()));
    assert!(s.doc().unwrap().entity(b).unwrap().common.visible);
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    assert!(back.entity(b).unwrap().common.visible && back.entity(a).unwrap().common.visible);
}

#[test]
fn modes_mix_and_hostile_values_are_refused() {
    let mut s = Session::new();
    let (a, b, c) = three(&mut s);
    // Hide one for the session, one saved with the drawing: UNISOLATE shows both.
    s.execute("hideobjects", &json!({ "handles": [a.hex()] })).unwrap();
    s.execute("setvar", &json!({ "name": "OBJECTISOLATIONMODE", "value": 1 })).unwrap();
    s.execute("hideobjects", &json!({ "handles": [b.hex(), a.hex()] })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.is_hidden_object(a) && d.entity(a).unwrap().common.visible, "already hidden: left as it was");
    assert!(!d.entity(b).unwrap().common.visible);
    // Isolating an object hidden already hides the rest, and nothing twice.
    let r = s.execute("isolateobjects", &json!({ "handles": [c.hex()] })).unwrap();
    assert_eq!(r["hidden"], json!(0));
    assert_eq!(s.execute("unisolateobjects", &json!({})).unwrap()["shown"], json!(2));
    let d = s.doc().unwrap();
    assert!(d.hidden_objects.is_empty() && d.model.iter().all(|e| e.common.visible));
    for v in [json!(2), json!(-1), json!("x"), json!(1.5), json!(null), json!(f64::MAX)] {
        assert!(s.execute("setvar", &json!({ "name": "OBJECTISOLATIONMODE", "value": v })).is_err(), "{v}");
    }
    assert_eq!(s.execute("getvar", &json!({ "name": "OBJECTISOLATIONMODE" })).unwrap()["value"], json!(1));
}

#[test]
fn objectisolationmode_is_a_profile_setting() {
    let mut s = Session::new();
    s.execute("options", &json!({ "OBJECTISOLATIONMODE": 1 })).unwrap();
    let saved = cadcraft_engine::cmd::options::profile(&s);
    assert_eq!(saved["OBJECTISOLATIONMODE"], json!(1));
    let mut t = Session::new();
    cadcraft_engine::cmd::options::restore(&mut t, &saved);
    assert!(t.settings.objectisolationmode);
    t.execute("options", &json!({ "reset": true })).unwrap();
    assert!(!t.settings.objectisolationmode);
}
