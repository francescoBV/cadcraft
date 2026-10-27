//! MPOLYGON drawing (gap F13): the boundary loops in the entity's colour, the fill (pattern or
//! solid) in the MPOLYGON's own fill colour; ByLayer fill follows the entity's layer.

use cadcraft_color::{Color, Rgb, aci_rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, Hatch, HatchLoop, Layer, MPolygon, Space};
use cadcraft_geom::{PolyVertex, Vec2};
use cadcraft_render::{DisplayList, Kind, Options, build};

fn square(lo: f64, hi: f64, outer: bool) -> HatchLoop {
    let v = [(lo, lo), (hi, lo), (hi, hi), (lo, hi)].into_iter().map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).collect();
    HatchLoop { vertices: v, outer, sources: Vec::new() }
}

fn mpolygon(pattern: &str, fill: Color) -> Hatch {
    Hatch {
        pattern: pattern.into(),
        solid: pattern == "SOLID",
        loops: vec![square(0.0, 10.0, true), square(4.0, 6.0, false)],
        scale: 1.0,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: Vec::new(),
        mpolygon: Some(MPolygon { fill_color: fill, ..MPolygon::default() }),
    }
}

fn draw(h: Hatch, color: Color) -> DisplayList {
    let mut d = Drawing::new_imperial();
    d.layers.push(Layer { color: Color::Index(6), ..Layer::new("MAGENTA") });
    let common = Common { layer: "MAGENTA".into(), color, ..Common::default() };
    d.add(&Space::Model, common, EntityKind::Hatch(h)).expect("add");
    build(&d, &Space::Model, &Options::default())
}

fn rgb(i: u8) -> Rgb {
    aci_rgb(i)
}

/// Closed runs of `color` that go round a square of side `side` (the boundary loops).
fn outlines(l: &DisplayList, color: Rgb, side: f64) -> usize {
    l.prims
        .iter()
        .filter(|p| p.kind == Kind::Polyline && p.color == color)
        .filter(|p| {
            let pts = l.points(p);
            let (lo, hi) =
                pts.iter().fold((Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
                    (lo.min(*q), hi.max(*q))
                });
            pts.len() >= 5 && ((hi.x - lo.x) - side).abs() < 1e-6 && ((hi.y - lo.y) - side).abs() < 1e-6
        })
        .count()
}

#[test]
fn pattern_mpolygon_draws_its_boundary_and_fills_in_its_own_colour() {
    let l = draw(mpolygon("ANSI31", Color::Index(1)), Color::Index(5));
    assert_eq!(outlines(&l, rgb(5), 10.0), 1, "the outer loop in the entity's colour");
    assert_eq!(outlines(&l, rgb(5), 2.0), 1, "the island too");
    let red = l.prims.iter().filter(|p| p.kind == Kind::Polyline && p.color == rgb(1)).count();
    assert!(red > 10, "pattern lines in the fill colour: {red}");
}

#[test]
fn solid_mpolygon_fills_around_its_island_and_bylayer_follows_the_layer() {
    let l = draw(mpolygon("SOLID", Color::ByLayer), Color::Index(3));
    let area: f64 = l
        .prims
        .iter()
        .filter(|p| p.kind == Kind::Tris && p.color == rgb(6))
        .flat_map(|p| l.points(p).chunks(3))
        .map(|t| ((t[1] - t[0]).cross(t[2] - t[0]) / 2.0).abs())
        .sum();
    assert!((area - 96.0).abs() < 1e-6, "{area}");
    assert_eq!(outlines(&l, rgb(3), 10.0), 1);
}

#[test]
fn a_plain_hatch_draws_no_boundary() {
    let mut h = mpolygon("ANSI31", Color::Index(1));
    h.mpolygon = None;
    let l = draw(h, Color::Index(5));
    assert_eq!(outlines(&l, rgb(5), 10.0), 0);
}
