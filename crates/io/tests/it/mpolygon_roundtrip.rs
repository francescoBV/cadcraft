//! MPOLYGON (gap F13): a hatch that draws its own boundary and fills in a colour of its own
//! (DXF Reference, MPOLYGON). Drawings made here in code are saved as DXF and DWG and reopened
//! unchanged, and hostile records never panic.

use cadcraft_color::Color;
use cadcraft_doc::*;
use cadcraft_geom::{PolyVertex, Vec2};

fn ring(pts: &[(f64, f64)], bulge: f64) -> Vec<PolyVertex> {
    pts.iter().map(|&(x, y)| PolyVertex { bulge, ..PolyVertex::new(Vec2::new(x, y)) }).collect()
}

fn mpolygon(pattern: &str, fill: Color, x_dir: Vec2) -> Hatch {
    let solid = pattern == "SOLID";
    Hatch {
        pattern: pattern.into(),
        solid,
        loops: vec![
            HatchLoop { vertices: ring(&[(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)], 0.0), outer: true, sources: Vec::new() },
            HatchLoop { vertices: ring(&[(4.0, 4.0), (8.0, 4.0), (8.0, 6.0), (4.0, 6.0)], 0.25), outer: false, sources: Vec::new() },
        ],
        scale: 2.0,
        angle: 30f64.to_radians(),
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::new(1.0, 2.0),
        background: None,
        pattern_lines: Vec::new(),
        mpolygon: Some(MPolygon { fill_color: fill, x_dir }),
    }
}

fn sample() -> Drawing {
    let mut d = Drawing::new_metric();
    let common = Common { color: Color::Index(5), ..Common::default() };
    d.add(&Space::Model, common.clone(), EntityKind::Hatch(mpolygon("ANSI31", Color::Index(1), Vec2::X))).unwrap();
    d.add(&Space::Model, common, EntityKind::Hatch(mpolygon("SOLID", Color::Index(3), Vec2::new(0.0, 1.0)))).unwrap();
    // An ordinary hatch beside them stays a hatch.
    let mut h = mpolygon("ANSI37", Color::ByLayer, Vec2::X);
    h.mpolygon = None;
    d.add(&Space::Model, Common::default(), EntityKind::Hatch(h)).unwrap();
    d
}

fn polys(d: &Drawing) -> Vec<(Hatch, Color)> {
    d.model.iter().filter_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some((h.clone(), e.common.color)) } else { None }).collect()
}

/// `d` saved with extension `ext` and reopened.
fn reopened(d: &Drawing, ext: &str) -> Drawing {
    let name = format!("x.{ext}");
    let bytes = cadcraft_io::write(d, &name).unwrap();
    cadcraft_io::read(&bytes, &name).unwrap()
}

#[test]
fn mpolygons_survive_dxf_and_dwg() {
    let d = sample();
    let text = cadcraft_io::write_dxf(&d);
    assert_eq!(text.lines().filter(|l| l.trim() == "MPOLYGON").count(), 3, "two entities and the class");
    assert!(text.contains("AcDbMPolygon"));
    for to in ["dxf", "dwg"] {
        // Saved twice: a drawing opened from a file comes back the same too.
        let back = reopened(&reopened(&d, to), to);
        let (before, after) = (polys(&d), polys(&back));
        assert_eq!(after.len(), 3, "{to:?}");
        for ((a, ca), (b, cb)) in before.iter().zip(&after) {
            assert_eq!(a.mpolygon.as_ref().map(|m| m.fill_color), b.mpolygon.as_ref().map(|m| m.fill_color), "{to:?}");
            assert_eq!((a.pattern.as_str(), a.solid, a.loops.len()), (b.pattern.as_str(), b.solid, b.loops.len()), "{to:?}");
            assert_eq!(ca, cb, "{to:?}: the boundary's colour");
            assert!(!b.associative && b.loops.iter().all(|l| l.sources.is_empty()), "{to:?}");
            for (la, lb) in a.loops.iter().zip(&b.loops) {
                assert_eq!(la.vertices.len(), lb.vertices.len(), "{to:?}");
                for (va, vb) in la.vertices.iter().zip(&lb.vertices) {
                    assert!(va.p.dist(vb.p) < 1e-9 && (va.bulge - vb.bulge).abs() < 1e-9, "{to:?}");
                }
            }
            if let (Some(ma), Some(mb)) = (&a.mpolygon, &b.mpolygon) {
                assert!(ma.x_dir.dist(mb.x_dir) < 1e-9, "{to:?}");
            }
        }
        let k = &back.model.iter().next().unwrap().kind;
        assert_eq!((k.type_name(), k.dxf_name()), ("MPolygon", "MPOLYGON"));
    }
}

/// An MPOLYGON written from the DXF Reference's group list (not by CADCraft): version, elevation,
/// extrusion, pattern name, solid flag, one polyline loop, style, pattern type, then the fill
/// colour, X direction and degenerate loop count.
#[test]
fn a_reference_style_mpolygon_reads() {
    let text = "0\nSECTION\n2\nENTITIES\n0\nMPOLYGON\n5\n2A\n100\nAcDbEntity\n8\nWalls\n62\n4\n100\nAcDbMPolygon\n70\n1\n10\n0.0\n20\n0.0\n30\n0.0\n210\n0.0\n220\n0.0\n230\n1.0\n2\nSOLID\n70\n1\n91\n1\n92\n2\n72\n0\n73\n1\n93\n3\n10\n0.0\n20\n0.0\n10\n5.0\n20\n0.0\n10\n5.0\n20\n5.0\n75\n0\n76\n1\n62\n2\n11\n1.0\n21\n0.0\n99\n0\n0\nENDSEC\n0\nEOF\n";
    let d = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let e = d.model.iter().next().expect("one entity");
    let EntityKind::Hatch(h) = &e.kind else { panic!("{:?}", e.kind) };
    assert_eq!((e.common.layer.as_str(), e.common.color), ("Walls", Color::Index(4)), "the entity's colour, not the fill's");
    assert!(h.solid);
    assert_eq!(h.loops.len(), 1);
    assert_eq!(h.loops[0].vertices.len(), 3);
    assert_eq!(h.mpolygon.as_ref().map(|m| m.fill_color), Some(Color::Index(2)));
}

#[test]
fn hostile_mpolygons_never_panic() {
    let rec = |body: &str| format!("0\nSECTION\n2\nENTITIES\n0\nMPOLYGON\n100\nAcDbEntity\n8\n0\n{body}0\nENDSEC\n0\nEOF\n");
    let cases = [
        rec(""),
        rec("100\nAcDbMPolygon\n"),
        rec("100\nAcDbMPolygon\n70\n-9\n2\n\n70\n77\n91\n2147483647\n92\n-1\n93\n99999999\n10\nnan\n20\ninf\n"),
        rec(
            "100\nAcDbMPolygon\n2\nANSI31\n70\n0\n91\n1\n92\n2\n72\n1\n73\n1\n93\n2\n10\n0\n20\n0\n42\n1e308\n10\n1\n20\n0\n75\n9\n76\n1\n52\nnan\n41\n-1\n77\n0\n78\n-5\n62\n99999\n11\nnan\n21\ninf\n99\n-3\n",
        ),
        rec("100\nAcDbMPolygon\n62\n-32768\n11\n1e309\n"),
    ];
    for t in &cases {
        let d = cadcraft_io::read_dxf(t.as_bytes()).unwrap();
        let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
        let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
        for e in back.model.iter() {
            if let EntityKind::Hatch(h) = &e.kind
                && let Some(m) = &h.mpolygon
            {
                assert!(m.x_dir.is_finite());
            }
        }
        let _ = cadcraft_io::write(&d, "x.dwg");
    }
}
