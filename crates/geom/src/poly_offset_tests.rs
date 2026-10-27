//! Polyline offsets against analytic answers.

use super::*;
use crate::{PI, PolyVertex, Polyline, Vec2};

fn poly(pts: &[(f64, f64)], closed: bool) -> Polyline {
    Polyline::from_points(&pts.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>(), closed)
}

/// A counter-clockwise rectangle from the origin.
fn rect(w: f64, h: f64) -> Polyline {
    poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)], true)
}

/// A counter-clockwise `w` × `h` rectangle with corners rounded to radius `r`.
fn rounded(w: f64, h: f64, r: f64) -> Polyline {
    let b = (PI / 8.0).tan();
    let v = |x: f64, y: f64, bulge: f64| PolyVertex::with_bulge(Vec2::new(x, y), bulge);
    Polyline {
        vertices: vec![
            v(r, 0.0, 0.0),
            v(w - r, 0.0, b),
            v(w, r, 0.0),
            v(w, h - r, b),
            v(w - r, h, 0.0),
            v(r, h, b),
            v(0.0, h - r, 0.0),
            v(0.0, r, b),
        ],
        closed: true,
    }
}

fn one(r: Vec<Polyline>) -> Polyline {
    assert_eq!(r.len(), 1, "{r:#?}");
    r.into_iter().next().unwrap()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * (1.0 + a.abs().max(b.abs()))
}

/// Every vertex and segment midpoint of the offset lies at least `d` from the source, and the
/// segment midpoints of round joins and offset segments lie at exactly `d`.
fn check_distance(src: &Polyline, out: &Polyline, d: f64, exact: bool) {
    let tol = Tol::of_bounds(&src.bounds());
    let ss = pieces_of(src, &tol);
    let dist = |p: Vec2| ss.iter().map(|s| s.dist(p)).fold(f64::INFINITY, f64::min);
    for p in pieces_of(out, &tol) {
        for u in [0.0, 0.5, 1.0] {
            let x = p.at(u);
            assert!(dist(x) >= d.abs() - 1e-9 * (1.0 + d.abs()), "{x:?} at {} from the source", dist(x));
        }
        if exact {
            let m = p.at(0.5);
            assert!(close(dist(m), d.abs(), 1e-9), "{m:?}: {} vs {}", dist(m), d.abs());
        }
    }
}

#[test]
fn rectangle_outward_with_each_gap_type() {
    let r = rect(10.0, 6.0);
    // Right of a counter-clockwise loop is outside.
    let ext = one(r.offset(-1.0, GapType::Extend));
    assert!(ext.closed && ext.vertices.len() == 4);
    assert!(close(ext.area(), 12.0 * 8.0, 1e-12), "{}", ext.area());
    let fil = one(r.offset(-1.0, GapType::Fillet));
    assert_eq!(fil.vertices.len(), 8);
    assert!(close(fil.area(), 60.0 + 32.0 + PI, 1e-12), "{}", fil.area());
    check_distance(&r, &fil, 1.0, true);
    let ch = one(r.offset(-1.0, GapType::Chamfer));
    assert_eq!(ch.vertices.len(), 8);
    let cut = (2f64.sqrt() - 1.0).powi(2);
    assert!(close(ch.area(), 96.0 - 4.0 * cut, 1e-12), "{}", ch.area());
    check_distance(&r, &ch, 1.0, false);
}

#[test]
fn vertices_of_the_source_are_kept() {
    // A vertex in the middle of a side stays a vertex of the offset, as in AutoCAD.
    let r = poly(&[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0)], true);
    let o = one(r.offset(1.0, GapType::Extend));
    assert_eq!(o.vertices.len(), 5, "{o:?}");
    assert!(o.vertices[0].p.near(Vec2::new(1.0, 1.0), 1e-12) && o.vertices[1].p.near(Vec2::new(5.0, 1.0), 1e-12), "{o:?}");
}

#[test]
fn rectangle_inward_and_collapse() {
    let r = rect(10.0, 6.0);
    for gap in [GapType::Extend, GapType::Fillet, GapType::Chamfer] {
        let i = one(r.offset(1.0, gap));
        assert!(close(i.area(), 8.0 * 4.0, 1e-12), "{gap:?}: {}", i.area());
        let thin = one(r.offset(2.9, gap));
        assert!(close(thin.area(), 4.2 * 0.2, 1e-9), "{gap:?}: {}", thin.area());
        // Past half the width nothing is left (not an inverted rectangle).
        assert!(r.offset(3.0 + 1e-6, gap).is_empty(), "{gap:?}");
        assert!(r.offset(4.0, gap).is_empty(), "{gap:?}");
    }
}

#[test]
fn rounded_rectangle_radii_grow_shrink_and_collapse() {
    let (w, h, r) = (12.0, 8.0, 2.0);
    let src = rounded(w, h, r);
    let expect = |d: f64, rr: f64| (w + 2.0 * d) * (h + 2.0 * d) - (4.0 - PI) * rr * rr;
    // Outward (right of travel): every radius grows by d; the arcs stay arcs.
    let out = one(src.offset(-1.0, GapType::Extend));
    assert_eq!(out.vertices.len(), 8);
    assert_eq!(out.vertices.iter().filter(|v| v.bulge != 0.0).count(), 4);
    assert!(close(out.area(), expect(1.0, 3.0), 1e-12), "{}", out.area());
    check_distance(&src, &out, 1.0, true);
    // Inward by less than the radius: radii shrink.
    let inn = one(src.offset(1.0, GapType::Extend));
    assert!(close(inn.area(), expect(-1.0, 1.0), 1e-12), "{}", inn.area());
    check_distance(&src, &inn, 1.0, true);
    // Inward by the radius or more: the arcs collapse and the lines meet at sharp corners.
    for d in [2.0, 3.0] {
        let sharp = one(src.offset(d, GapType::Fillet));
        assert!(sharp.vertices.iter().all(|v| v.bulge == 0.0), "{d}: {sharp:?}");
        assert!(close(sharp.area(), (w - 2.0 * d) * (h - 2.0 * d), 1e-9), "{d}: {}", sharp.area());
        check_distance(&src, &sharp, d, false);
    }
}

#[test]
fn concentric_arcs_and_circles() {
    // An open semicircle of radius 5 (counter-clockwise from (5, 0) to (-5, 0)).
    let semi = Polyline { vertices: vec![PolyVertex::with_bulge(Vec2::new(5.0, 0.0), 1.0), PolyVertex::new(Vec2::new(-5.0, 0.0))], closed: false };
    for (d, r) in [(1.0, 4.0), (-1.0, 6.0), (4.5, 0.5)] {
        let o = one(semi.offset(d, GapType::Extend));
        assert!(!o.closed && o.vertices.len() == 2);
        assert!(o.vertices[0].p.near(Vec2::new(r, 0.0), 1e-12) && o.vertices[1].p.near(Vec2::new(-r, 0.0), 1e-12), "{o:?}");
        assert!(close(o.vertices[0].bulge, 1.0, 1e-12));
    }
    // The radius falls to zero or below: nothing.
    assert!(semi.offset(5.0, GapType::Extend).is_empty());
    assert!(semi.offset(7.0, GapType::Extend).is_empty());
    // A circle drawn as a closed polyline of two arcs.
    let circle = Polyline {
        vertices: vec![PolyVertex::with_bulge(Vec2::new(3.0, 0.0), 1.0), PolyVertex::with_bulge(Vec2::new(-3.0, 0.0), 1.0)],
        closed: true,
    };
    let o = one(circle.offset(2.0, GapType::Extend));
    assert!(close(o.area(), PI, 1e-12), "{}", o.area());
    let o = one(circle.offset(-2.0, GapType::Fillet));
    assert!(close(o.area(), 25.0 * PI, 1e-12), "{}", o.area());
    assert!(circle.offset(3.5, GapType::Extend).is_empty());
}

#[test]
fn a_step_loses_the_loop_its_raw_offset_makes() {
    // East, a short step north, east again: offset up by 1 the raw offset crosses itself.
    let step = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 0.5), (20.0, 0.5)], false);
    let o = one(step.offset(1.0, GapType::Extend));
    let pts: Vec<Vec2> = o.vertices.iter().map(|v| v.p).collect();
    let want = [Vec2::new(0.0, 1.0), Vec2::new(9.0, 1.0), Vec2::new(9.0, 1.5), Vec2::new(20.0, 1.5)];
    assert_eq!(pts.len(), want.len(), "{pts:?}");
    for (p, q) in pts.iter().zip(want) {
        assert!(p.near(q, 1e-9), "{pts:?}");
    }
    check_distance(&step, &o, 1.0, false);
    // The other side: an extension corner over the step, a plain trim inside it.
    let o = one(step.offset(-1.0, GapType::Fillet));
    check_distance(&step, &o, 1.0, true);
    assert!(o.vertices.first().unwrap().p.near(Vec2::new(0.0, -1.0), 1e-12));
    assert!(o.vertices.last().unwrap().p.near(Vec2::new(20.0, -0.5), 1e-12));
}

#[test]
fn a_dumbbell_splits_into_two_loops() {
    let pts = [
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 4.5),
        (14.0, 4.5),
        (14.0, 0.0),
        (24.0, 0.0),
        (24.0, 10.0),
        (14.0, 10.0),
        (14.0, 5.5),
        (10.0, 5.5),
        (10.0, 10.0),
        (0.0, 10.0),
    ];
    let src = poly(&pts, true);
    let r = src.offset(1.0, GapType::Extend);
    assert_eq!(r.len(), 2, "{r:#?}");
    for l in &r {
        assert!(l.closed);
        assert!(close(l.area(), 64.0, 1e-9), "{}", l.area());
        assert_eq!(l.vertices.len(), 4, "{l:?}");
        check_distance(&src, l, 1.0, false);
    }
    // Inward by less than half the neck: one loop through the neck.
    let neck = one(src.offset(0.25, GapType::Extend));
    assert!(close(neck.area(), 2.0 * 9.5 * 9.5 + 4.5 * 0.5, 1e-9), "{}", neck.area());
}

#[test]
fn open_polyline_ends_stay_square_and_hairpins_close_up() {
    // A hairpin: out and back 1 apart. Offset outward by 2 the inner side vanishes.
    let pin = poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 1.0), (0.0, 1.0)], false);
    let o = one(pin.offset(-2.0, GapType::Extend));
    check_distance(&pin, &o, 2.0, false);
    assert!(o.vertices.first().unwrap().p.near(Vec2::new(0.0, -2.0), 1e-12));
    assert!(o.vertices.last().unwrap().p.near(Vec2::new(0.0, 3.0), 1e-12));
    // Inward by more than half the gap: the two legs overlap each other completely.
    assert!(pin.offset(0.6, GapType::Extend).is_empty());
}

#[test]
fn convex_polygons_follow_steiners_formula() {
    // The fillet offset of a convex region by d has area A + P·d + π·d² (Steiner's formula).
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for case in 0..60 {
        let n = 3 + (rnd() * 12.0) as usize;
        let mut angles: Vec<f64> = (0..n).map(|_| rnd() * crate::TAU).collect();
        angles.sort_by(f64::total_cmp);
        angles.dedup_by(|a, b| (*a - *b).abs() < 0.05);
        if angles.len() < 3 {
            continue;
        }
        let scale = 10f64.powf(rnd() * 8.0 - 4.0);
        let origin = Vec2::new(rnd() * 1e3, rnd() * 1e3) * (if case % 3 == 0 { 1e3 } else { 1.0 });
        let pts: Vec<Vec2> = angles.iter().map(|a| origin + Vec2::from_angle(*a) * scale * (1.0 + 0.2 * rnd())).collect();
        // Keep the convex hull order only when it is convex (the radial jitter can dent it).
        let convex = (0..pts.len()).all(|i| {
            let (a, b, c) = (pts[i], pts[(i + 1) % pts.len()], pts[(i + 2) % pts.len()]);
            (b - a).cross(c - b) > 0.0
        });
        if !convex {
            continue;
        }
        let src = Polyline::from_points(&pts, true);
        let (area, per) = (src.area(), src.len());
        let d = scale * (0.05 + rnd());
        let o = one(src.offset(-d, GapType::Fillet));
        let want = area + per * d + PI * d * d;
        assert!(close(o.area(), want, 1e-9), "case {case}: {} vs {want}", o.area());
        check_distance(&src, &o, d, true);
    }
}

#[test]
fn hostile_input_gives_nothing() {
    let r = rect(10.0, 6.0);
    for d in [f64::NAN, f64::INFINITY, 0.0] {
        assert!(r.offset(d, GapType::Extend).is_empty());
    }
    let bad = Polyline { vertices: vec![PolyVertex::new(Vec2::new(f64::NAN, 0.0)), PolyVertex::new(Vec2::new(1.0, 1.0))], closed: false };
    assert!(bad.offset(1.0, GapType::Extend).is_empty());
    let huge =
        Polyline { vertices: vec![PolyVertex::with_bulge(Vec2::new(-1e308, 0.0), 1e300), PolyVertex::new(Vec2::new(1e308, 0.0))], closed: true };
    let _ = huge.offset(1e307, GapType::Chamfer);
    let dot = poly(&[(1.0, 1.0), (1.0, 1.0)], false);
    assert!(dot.offset(1.0, GapType::Fillet).is_empty());
    // A zigzag of many teeth offset by more than a tooth: bounded work, a sane result.
    let teeth: Vec<(f64, f64)> = (0..2000).map(|i| (i as f64 * 0.1, if i % 2 == 0 { 0.0 } else { 0.1 })).collect();
    let z = poly(&teeth, false);
    for o in z.offset(1.0, GapType::Fillet) {
        check_distance(&z, &o, 1.0, false);
    }
}

#[test]
fn a_tangent_meeting_is_rounded_like_a_miss() {
    // A square with a radius-2 apse on its top edge, offset inward by 0.5: the copy has reflex
    // corners where its radius-1.5 arc meets the top edge at y = 9.5. Offset again by 0.5, the
    // radius-1 arc and the line y = 9 only touch at (5, 9); rounding decided whether that counted
    // as meeting, so one corner was extended round the whole circle and the other rounded.
    let apse = Polyline {
        vertices: vec![
            PolyVertex::new(Vec2::new(0.0, 0.0)),
            PolyVertex::new(Vec2::new(10.0, 0.0)),
            PolyVertex::new(Vec2::new(10.0, 10.0)),
            PolyVertex::with_bulge(Vec2::new(7.0, 10.0), 1.0),
            PolyVertex::new(Vec2::new(3.0, 10.0)),
            PolyVertex::new(Vec2::new(0.0, 10.0)),
        ],
        closed: true,
    };
    let once = one(apse.offset(0.5, GapType::Extend));
    let twice = one(once.offset(0.5, GapType::Extend));
    // Both corners rounded about the copy's corners, mirror images of each other about x = 5.
    let pts: Vec<Vec2> = twice.vertices.iter().map(|v| v.p).collect();
    assert_eq!(pts.len(), 8, "{:?}", twice.vertices);
    for p in &pts {
        assert!(pts.iter().any(|q| q.near(Vec2::new(10.0 - p.x, p.y), 1e-9)), "{p:?} has no mirror image: {pts:?}");
    }
    // Each rounds 109.47° (cos = −1/3) clockwise about a corner of the first copy.
    let round = ((-1.0f64 / 3.0).acos() / 4.0).tan();
    assert_eq!(twice.vertices.iter().filter(|v| close(v.bulge, -round, 1e-6)).count(), 2, "{:?}", twice.vertices);
}
