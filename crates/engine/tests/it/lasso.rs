//! Lasso selection (LASSOMODE): window, crossing and fence lassos, idle and at "Select objects",
//! PICKADD and Shift, hidden objects, and hostile paths.

use cadcraft_engine::doc::Handle;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::select::LassoKind;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn hx(v: &Value) -> Handle {
    Handle::parse_hex(v.as_str().unwrap()).unwrap()
}

/// Three objects: a short line inside the lasso, a long line crossing it, a circle outside.
fn three(s: &mut Session) -> (Handle, Handle, Handle) {
    let inside = hx(&s.execute("line", &json!({ "points": [[1, 1], [2, 1]] })).unwrap()["handles"][0]);
    let crossing = hx(&s.execute("line", &json!({ "points": [[1, 3], [20, 3]] })).unwrap()["handles"][0]);
    let outside = hx(&s.execute("circle", &json!({ "center": [30, 30], "radius": 1 })).unwrap()["handle"]);
    (inside, crossing, outside)
}

/// A lasso round (0, 0)–(5, 5), drawn as a rough diamond-ish loop.
fn loop_pts() -> Vec<Vec2> {
    [(0.0, 0.0), (5.0, -0.5), (5.5, 5.0), (0.0, 5.5), (-0.5, 0.5)].iter().map(|&(x, y)| Vec2::new(x, y)).collect()
}

fn sorted(mut v: Vec<Handle>) -> Vec<Handle> {
    v.sort();
    v
}

#[test]
fn lasso_direction_and_kinds() {
    let pts = loop_pts();
    assert_eq!(LassoKind::by_direction(&pts), Some(LassoKind::Window));
    let rev: Vec<Vec2> = pts.iter().map(|p| Vec2::new(-p.x, p.y)).collect();
    assert_eq!(LassoKind::by_direction(&rev), Some(LassoKind::Crossing));
    assert_eq!(LassoKind::by_direction(&[Vec2::ZERO, Vec2::new(0.0, 3.0)]), None);
    assert_eq!(LassoKind::by_direction(&[]), None);
    assert_eq!(LassoKind::Window.next(), LassoKind::Crossing);
    assert_eq!(LassoKind::Crossing.next(), LassoKind::Fence);
    assert_eq!(LassoKind::Fence.next(), LassoKind::Window);
    assert_eq!(LassoKind::Crossing.label(), "Crossing Lasso");
}

#[test]
fn idle_lasso_selects_like_a_window_crossing_or_fence() {
    let mut s = Session::new();
    let (inside, crossing, outside) = three(&mut s);
    assert!(s.lasso_allowed());
    s.lasso(&loop_pts(), LassoKind::Window, false).unwrap();
    assert_eq!(s.selection(), vec![inside]);
    s.set_selection(Vec::new());
    s.lasso(&loop_pts(), LassoKind::Crossing, false).unwrap();
    let sel = sorted(s.selection());
    assert_eq!(sel, sorted(vec![inside, crossing]));
    assert!(!sel.contains(&outside));
    // A fence lasso takes what its path crosses: the long line only.
    s.set_selection(Vec::new());
    s.lasso(&loop_pts(), LassoKind::Fence, false).unwrap();
    assert_eq!(s.selection(), vec![crossing]);
    // A quick straight drag encloses nothing: it selects as the rectangle it spans.
    s.set_selection(Vec::new());
    s.lasso(&[Vec2::new(-2.0, -2.0), Vec2::new(5.0, 0.0), Vec2::new(12.0, 2.0)], LassoKind::Window, false).unwrap();
    assert_eq!(s.selection(), vec![inside]);
    // Lassos add to the selection (PICKADD on), without duplicates.
    s.lasso(&loop_pts(), LassoKind::Crossing, false).unwrap();
    assert_eq!(sorted(s.selection()), sorted(vec![inside, crossing]));
    // Shift removes (PICKADD on).
    s.lasso(&loop_pts(), LassoKind::Crossing, true).unwrap();
    assert!(s.selection().is_empty());
    // LASSOMODE 0: no lasso.
    s.execute("setvar", &json!({ "name": "LASSOMODE", "value": 0 })).unwrap();
    assert!(!s.lasso_allowed());
    s.lasso(&loop_pts(), LassoKind::Crossing, false).unwrap();
    assert!(s.selection().is_empty());
    assert_eq!(s.execute("getvar", &json!({ "name": "LASSOMODE" })).unwrap()["value"], json!(0));
    assert!(s.execute("setvar", &json!({ "name": "LASSOMODE", "value": 2 })).is_err());
    // PICKDRAG 0 (two clicks only) turns the lasso off too.
    s.execute("setvar", &json!({ "name": "LASSOMODE", "value": 1 })).unwrap();
    s.execute("setvar", &json!({ "name": "PICKDRAG", "value": 0 })).unwrap();
    assert!(!s.lasso_allowed());
}

#[test]
fn lassomode_is_a_profile_setting() {
    let mut s = Session::new();
    s.execute("options", &json!({ "LASSOMODE": 0 })).unwrap();
    let saved = cadcraft_engine::cmd::options::profile(&s);
    assert_eq!(saved["LASSOMODE"], json!(0));
    let mut t = Session::new();
    cadcraft_engine::cmd::options::restore(&mut t, &saved);
    assert!(!t.settings.lassomode);
    t.execute("options", &json!({ "reset": true })).unwrap();
    assert!(t.settings.lassomode);
}

#[test]
fn lasso_at_select_objects_feeds_the_command() {
    let mut s = Session::new();
    let (inside, crossing, _) = three(&mut s);
    s.start("erase").unwrap();
    assert!(s.lasso_allowed());
    s.lasso(&loop_pts(), LassoKind::Window, false).unwrap();
    s.input(Input::Enter).unwrap();
    let d = s.doc().unwrap();
    assert!(d.entity(inside).is_none(), "the window lasso's object is erased");
    assert!(d.entity(crossing).is_some());
    // While a command asks for a point, a drag is no lasso.
    s.start("line").unwrap();
    assert!(!s.lasso_allowed());
    s.cancel();
    // Hostile paths select nothing and never panic.
    s.lasso(&[Vec2::new(f64::NAN, 0.0), Vec2::new(f64::INFINITY, 1.0)], LassoKind::Crossing, false).unwrap();
    s.lasso(&[], LassoKind::Fence, false).unwrap();
    s.lasso(&[Vec2::new(1e308, 1e308), Vec2::new(-1e308, 1e308), Vec2::new(0.0, -1e308)], LassoKind::Window, false).unwrap();
    let many: Vec<Vec2> = (0..100_000).map(|i| Vec2::new((i as f64).sin() * 3.0, (i as f64).cos() * 3.0)).collect();
    s.lasso(&many, LassoKind::Window, false).unwrap();
    s.lasso(&many, LassoKind::Fence, false).unwrap();
}

#[test]
fn hidden_objects_escape_lassos() {
    let mut s = Session::new();
    let (inside, crossing, outside) = three(&mut s);
    s.execute("hideobjects", &json!({ "handles": [inside.hex()] })).unwrap();
    // A crossing lasso round the hidden line takes the long line only.
    s.lasso(&loop_pts(), LassoKind::Crossing, false).unwrap();
    assert_eq!(s.selection(), vec![crossing]);
    s.set_selection(Vec::new());
    // Isolating the circle hides the long line as well; ending isolation shows both.
    s.execute("isolateobjects", &json!({ "handles": [outside.hex()] })).unwrap();
    s.lasso(&loop_pts(), LassoKind::Crossing, false).unwrap();
    assert!(s.selection().is_empty());
    s.execute("unisolateobjects", &json!({})).unwrap();
    s.lasso(&loop_pts(), LassoKind::Crossing, false).unwrap();
    assert_eq!(s.selection().len(), 2);
}
