//! DIMBREAK Auto breaks remember the object whose crossing makes them (`DimBreak::object`) and
//! follow it: moved, the break moves with it; the dimension moved, the breaks are found again
//! where the object crosses it; the object erased, its breaks go. Undo and manual breaks too.

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
fn dimbreak_breaks_follow_moved_and_erased_objects() {
    let mut s = Session::new();
    let d = add(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 2] }));
    let line = add(&mut s, "line", json!({ "points": [[4, -1], [4, 5]] }));
    s.execute("dimbreak", &json!({ "handles": [d.hex()], "size": 0.5 })).unwrap();
    let breaks = dim(&s, d).marks.breaks;
    assert!(!breaks.is_empty());
    assert!(breaks.iter().all(|b| b.object == Some(line) && b.auto && (b.size - 0.5).abs() < 1e-12), "{breaks:?}");
    assert!(breaks.iter().any(|b| (b.at.x - 4.0).abs() < 1e-9 && (b.at.y - 2.0).abs() < 1e-9));
    // The line moves: its break moves along.
    s.execute("move", &json!({ "handles": [line.hex()], "delta": [2, 0] })).unwrap();
    let breaks = dim(&s, d).marks.breaks;
    assert!(breaks.iter().any(|b| (b.at.x - 6.0).abs() < 1e-9 && (b.at.y - 2.0).abs() < 1e-9), "{breaks:?}");
    assert!(!breaks.iter().any(|b| (b.at.x - 4.0).abs() < 1e-9));
    // The dimension moves: the breaks are found again where the line crosses it.
    s.execute("move", &json!({ "handles": [d.hex()], "delta": [0, 1] })).unwrap();
    let breaks = dim(&s, d).marks.breaks;
    assert!(breaks.iter().any(|b| (b.at.x - 6.0).abs() < 1e-9 && (b.at.y - 3.0).abs() < 1e-9), "{breaks:?}");
    // A manual break stays put; erasing the line takes its breaks away.
    s.execute("dimbreak", &json!({ "handles": [d.hex()], "mode": "manual", "first": [8, 3], "second": [8.5, 3] })).unwrap();
    s.execute("erase", &json!({ "handles": [line.hex()] })).unwrap();
    let breaks = dim(&s, d).marks.breaks;
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert!(!breaks[0].auto && breaks[0].object.is_none());
}

#[test]
fn undo_brings_the_breaks_back_and_a_circle_breaks_twice() {
    let mut s = Session::new();
    let d = add(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 2] }));
    let circle = add(&mut s, "circle", json!({ "center": [5, 2], "radius": 1 }));
    s.execute("dimbreak", &json!({ "handles": [d.hex()] })).unwrap();
    let before = dim(&s, d).marks.breaks;
    assert_eq!(before.len(), 2, "{before:?}");
    assert!(before.iter().all(|b| b.object == Some(circle)));
    s.execute("move", &json!({ "handles": [circle.hex()], "delta": [1, 0] })).unwrap();
    let moved = dim(&s, d).marks.breaks;
    assert!(moved.iter().any(|b| (b.at.x - 7.0).abs() < 1e-9), "{moved:?}");
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(dim(&s, d).marks.breaks, before);
    // The circle moved off the dimension: no crossing, no break; back on it, the breaks return
    // only with the next DIMBREAK (the link goes with the last break).
    s.execute("move", &json!({ "handles": [circle.hex()], "delta": [0, 10] })).unwrap();
    assert!(dim(&s, d).marks.breaks.is_empty());
}
