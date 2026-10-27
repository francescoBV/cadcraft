//! OFFSET of polylines through the command: arc segments stay arcs (exact bulges), the result is
//! exact at survey coordinates, a loop the offset pinches off becomes separate polylines, and
//! hostile distances never panic.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::{PI, Polyline, Vec2, bulge_to_arc};
use serde_json::{Value, json};

fn polyline(s: &Session, h: &Value) -> Polyline {
    let h = Handle::parse_hex(h.as_str().unwrap()).unwrap();
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::LwPolyline(p) => Polyline { vertices: p.vertices.clone(), closed: p.closed },
        k => panic!("not a polyline: {k:?}"),
    }
}

/// A `w` × `h` rectangle with corners rounded to `r`, its lower-left corner at `o`, as PLINE
/// vertices (the arcs are quarter circles: bulge tan(22.5°)).
fn rounded(o: [f64; 2], w: f64, h: f64, r: f64) -> Value {
    let b = (PI / 8.0).tan();
    let [x, y] = o;
    json!([
        { "p": [x + r, y] },
        { "p": [x + w - r, y], "bulge": b },
        { "p": [x + w, y + r] },
        { "p": [x + w, y + h - r], "bulge": b },
        { "p": [x + w - r, y + h] },
        { "p": [x + r, y + h], "bulge": b },
        { "p": [x, y + h - r] },
        { "p": [x, y + r], "bulge": b },
    ])
}

#[test]
fn rounded_rectangle_at_survey_coordinates_offsets_exactly() {
    let (w, h, r, d) = (40.0, 20.0, 5.0, 1.5);
    for o in [[0.0, 0.0], [5_123_456.0, 4_987_654.0]] {
        let mut s = Session::new();
        let src = s.execute("pline", &json!({ "vertices": rounded(o, w, h, r), "closed": true })).unwrap();
        let src = polyline(&s, &src["handle"]);
        let out = s.execute("offset", &json!({ "handle": src_handle(&s), "distance": d, "side": [o[0] - 10.0, o[1] - 10.0] })).unwrap();
        assert_eq!(out["handles"].as_array().unwrap().len(), 1, "{out}");
        let p = polyline(&s, &out["handle"]);
        assert!(p.closed);
        // Four arcs, each a quarter circle round the source's corner centre, radius r + d.
        let arcs: Vec<_> = p.vertices.iter().zip(p.vertices.iter().cycle().skip(1)).filter_map(|(a, b)| bulge_to_arc(a.p, b.p, a.bulge)).collect();
        assert_eq!(arcs.len(), 4, "{:?}", p.vertices);
        for (arc, _) in &arcs {
            assert!((arc.radius - (r + d)).abs() <= 1e-9 * (r + d), "radius {} at {o:?}", arc.radius);
            let centres = [[r, r], [w - r, r], [w - r, h - r], [r, h - r]].map(|[cx, cy]| Vec2::new(o[0] + cx, o[1] + cy));
            assert!(centres.iter().any(|c| c.dist(arc.center) <= 1e-9), "centre {:?} at {o:?}", arc.center);
        }
        // Steiner: the outward offset of a convex loop adds perimeter × d + π d².
        let want = src.area().abs() + src.len() * d + PI * d * d;
        let got = p.area().abs();
        assert!((got - want).abs() <= 1e-9 * want, "area {got} vs {want} at {o:?}");
    }
}

/// The handle of the only object in the drawing.
fn src_handle(s: &Session) -> String {
    s.doc().unwrap().model.iter().next().unwrap().handle.hex()
}

#[test]
fn an_inward_offset_through_a_neck_splits_the_polyline() {
    let mut s = Session::new();
    // Two 10 × 10 rooms joined by a corridor 2 wide.
    let pts = json!([[0, 0], [10, 0], [10, 4], [20, 4], [20, 0], [30, 0], [30, 10], [20, 10], [20, 6], [10, 6], [10, 10], [0, 10]]);
    s.execute("pline", &json!({ "vertices": pts, "closed": true })).unwrap();
    let h = src_handle(&s);
    let out = s.execute("offset", &json!({ "handle": h, "distance": 1.5, "side": [5, 5] })).unwrap();
    let hs = out["handles"].as_array().unwrap();
    assert_eq!(hs.len(), 2, "the corridor closes up: one loop per room, {out}");
    for h in hs {
        let p = polyline(&s, h);
        assert!(p.closed);
        // Each is the room shrunk by 1.5 on every side: 7 × 7.
        assert!((p.area().abs() - 49.0).abs() < 1e-9, "{:?}", p.vertices);
    }
    // Undo takes both away at once.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().model.iter().count(), 1);
}

#[test]
fn hostile_offset_distances_never_panic() {
    let mut s = Session::new();
    s.execute("pline", &json!({ "vertices": rounded([0.0, 0.0], 4.0, 2.0, 1.0), "closed": true })).unwrap();
    let h = src_handle(&s);
    for d in [json!(1e308), json!(-1e308), json!(0), json!(5e-324), json!("x"), json!(null)] {
        let _ = s.execute("offset", &json!({ "handle": h, "distance": d, "side": [10, 10] }));
        let _ = s.execute("offset", &json!({ "handle": h, "distance": d, "side": [1, 1] }));
    }
    for side in [json!([1e308, -1e308]), json!([0, "y"]), json!([])] {
        let _ = s.execute("offset", &json!({ "handle": h, "distance": 1, "side": side }));
    }
    // Every object left in the drawing is finite.
    for e in s.doc().unwrap().model.iter() {
        if let EntityKind::LwPolyline(p) = &e.kind {
            assert!(p.vertices.iter().all(|v| v.p.is_finite() && v.bulge.is_finite()), "{:?}", p.vertices);
        }
    }
}

/// Every point of `p` (sampled along its segments) lies at least `d` from `src`, and some lie at
/// `d` to within rounding: the offset neither cuts into the source's band nor drifts away.
fn assert_at_distance(p: &Polyline, src: &Polyline, d: f64) {
    let mut nearest = f64::INFINITY;
    for q in p.tessellate(1e-4) {
        let dist = src.closest(q).unwrap().dist(q);
        assert!(dist >= d - 1e-9, "{q:?} is {dist} from the source, closer than {d}");
        nearest = nearest.min(dist);
    }
    assert!((nearest - d).abs() < 1e-9, "the nearest point is {nearest} from the source, not {d}");
}

/// The step of the repeated offsets below (not a divisor of the radius 2, so no copy lands
/// exactly on a tangency).
const STEP: f64 = 0.35;

#[test]
fn repeated_inward_offsets_of_a_niche_and_an_apse_stay_concentric() {
    // Issue #2150: repeated inward offsets of a polyline with an arc drifted off the arc's centre
    // and left stray diagonals and circles.
    // A 10 × 10 square whose top edge has a radius-2 semicircle round (5, 10) in it: dipping into
    // the square (a niche, clockwise: bulge −1) or standing out of it (an apse, bulge 1).
    for bulge in [-1.0, 1.0] {
        let mut s = Session::new();
        let pts = json!([[0, 0], [10, 0], [10, 10], { "p": [7, 10], "bulge": bulge }, [3, 10], [0, 10]]);
        s.execute("pline", &json!({ "vertices": pts, "closed": true })).unwrap();
        let src = polyline(&s, &json!(src_handle(&s)));
        let mut h = src_handle(&s);
        // Offset inward, then the copy again, five times over.
        for k in 1..=5 {
            let out = s.execute("offset", &json!({ "handle": h, "distance": STEP, "side": [5, 3] })).unwrap();
            assert_eq!(out["handles"].as_array().unwrap().len(), 1, "offset {k} of {bulge}: {out}");
            let p = polyline(&s, &out["handle"]);
            assert!(p.closed && p.vertices.iter().all(|v| v.p.is_finite() && v.bulge.is_finite()), "{:?}", p.vertices);
            let d = STEP * k as f64;
            assert_at_distance(&p, &src, d);
            // The arc keeps its centre: a niche's grows to 2 + d, an apse's shrinks to 2 − d. Once
            // an apse's arc no longer reaches the offset top edge, the copy rounds the corner it
            // left: arcs round other centres with a radius a whole number of steps.
            let arcs: Vec<_> =
                p.vertices.iter().zip(p.vertices.iter().cycle().skip(1)).filter_map(|(a, b)| bulge_to_arc(a.p, b.p, a.bulge)).collect();
            let (concentric, fills): (Vec<_>, Vec<_>) = arcs.iter().partition(|(arc, _)| arc.center.dist(Vec2::new(5.0, 10.0)) < 1e-9);
            let r = if bulge < 0.0 { 2.0 + d } else { 2.0 - d };
            if bulge < 0.0 || k <= 2 {
                assert_eq!(concentric.len(), 1, "offset {k} of {bulge}: {:?}", p.vertices);
                assert!(fills.is_empty(), "offset {k} of {bulge}: {:?}", p.vertices);
            }
            for (arc, _) in concentric {
                assert!((arc.radius - r).abs() < 1e-9, "offset {k} of {bulge}: {arc:?}");
            }
            for (arc, _) in fills {
                let steps = arc.radius / STEP;
                assert!((steps - steps.round()).abs() < 1e-9 && arc.radius <= d + 1e-9, "offset {k} of {bulge}: {arc:?}");
            }
            h = out["handle"].as_str().unwrap().to_string();
        }
    }
}
