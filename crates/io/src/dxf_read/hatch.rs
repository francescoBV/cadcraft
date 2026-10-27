use super::common::{T, common};
use super::*;

pub(super) fn hatch(tags: &[Tag], t: &T) -> Option<EntityKind> {
    let (origin, background) = hatch_xdata(tags);
    let pattern = t.s(2).unwrap_or_else(|| "SOLID".into());
    let solid = t.i(70).unwrap_or(0) == 1;
    let n_loops = t.i(91).unwrap_or(0).clamp(0, 100_000) as usize;
    let mut loops = Vec::new();
    // Walk the boundary data after the 91 tag.
    let start = tags.iter().position(|x| x.code == 91).map(|i| i + 1)?;
    let mut i = start;
    let get = |i: usize| tags.get(i);
    for _ in 0..n_loops {
        // 92: loop type flags.
        while get(i).is_some_and(|x| x.code != 92) {
            i += 1;
        }
        let flags = get(i)?.i64();
        i += 1;
        let outer = flags & 1 != 0;
        let mut verts: Vec<PolyVertex> = Vec::new();
        if flags & 2 != 0 {
            // Polyline boundary: 72 has-bulge, 73 closed, 93 count, then 10/20/42.
            let has_bulge = get(i).filter(|x| x.code == 72).map(Tag::i64).unwrap_or(0) != 0;
            while get(i).is_some_and(|x| x.code != 93) {
                i += 1;
            }
            let n = get(i)?.i64().clamp(0, 1_000_000) as usize;
            i += 1;
            for _ in 0..n {
                let x = get(i).filter(|t| t.code == 10)?.f64();
                let y = get(i + 1).filter(|t| t.code == 20)?.f64();
                i += 2;
                let mut b = 0.0;
                if has_bulge && get(i).is_some_and(|t| t.code == 42) {
                    b = get(i)?.f64();
                    i += 1;
                }
                verts.push(PolyVertex::with_bulge(Vec2::new(x, y), b));
            }
        } else {
            // Edge boundary: 93 edge count, each 72 edge type.
            while get(i).is_some_and(|x| x.code != 93) {
                i += 1;
            }
            let n = get(i)?.i64().clamp(0, 1_000_000) as usize;
            i += 1;
            for _ in 0..n {
                while get(i).is_some_and(|x| x.code != 72) {
                    i += 1;
                }
                let ty = get(i)?.i64();
                i += 1;
                let f = |i: usize| get(i).map(Tag::f64).unwrap_or(0.0);
                match ty {
                    1 => {
                        let a = Vec2::new(f(i), f(i + 1));
                        let b = Vec2::new(f(i + 2), f(i + 3));
                        i += 4;
                        if verts.last().is_none_or(|l| !l.p.near(a, 1e-9)) {
                            verts.push(PolyVertex::new(a));
                        }
                        verts.push(PolyVertex::new(b));
                    }
                    2 => {
                        let c = Vec2::new(f(i), f(i + 1));
                        let r = f(i + 2);
                        let sa = f(i + 3).to_radians();
                        let ea = f(i + 4).to_radians();
                        let ccw = get(i + 5).map(Tag::i64).unwrap_or(1) != 0;
                        i += 6;
                        let (s, e) = if ccw { (sa, ea) } else { (-sa, -ea) };
                        let p0 = Vec2::polar(c, r, s);
                        let p1 = Vec2::polar(c, r, e);
                        let sweep = if ccw { cadcraft_geom::ccw_sweep(s, e) } else { -cadcraft_geom::ccw_sweep(e, s) };
                        if verts.last().is_none_or(|l| !l.p.near(p0, 1e-9)) {
                            verts.push(PolyVertex::new(p0));
                        }
                        // A full circle (0 to 360) starts and ends on the same point, which one
                        // bulge can't span: it goes in as two half circles.
                        let full = (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9;
                        let bulge = cadcraft_geom::arc_to_bulge(if full { sweep / 2.0 } else { sweep });
                        if let Some(l) = verts.last_mut() {
                            l.bulge = bulge;
                        }
                        if full {
                            verts.push(PolyVertex::with_bulge(Vec2::polar(c, r, s + sweep / 2.0), bulge));
                        }
                        verts.push(PolyVertex::new(p1));
                    }
                    3 => {
                        // Elliptic arc: approximate with points.
                        let c = Vec2::new(f(i), f(i + 1));
                        let m = Vec2::new(f(i + 2), f(i + 3));
                        let ratio = f(i + 4);
                        let sa = f(i + 5).to_radians();
                        let ea = f(i + 6).to_radians();
                        let ccw = get(i + 7).filter(|t| t.code == 73).map(Tag::i64).unwrap_or(1) != 0;
                        i += 8;
                        // A clockwise edge stores its parameters mirrored, as an arc edge does: it
                        // runs from -start clockwise to -end, the ccw sweep from -end to -start
                        // walked backwards.
                        let (s, e) = if ccw { (sa, ea) } else { (-ea, -sa) };
                        let e = cadcraft_geom::Ellipse { center: c, major: m, ratio, start: s, end: e };
                        let mut pts = Vec::new();
                        e.tessellate(m.len() * 1e-3, &mut pts);
                        if !ccw {
                            pts.reverse();
                        }
                        verts.extend(pts.into_iter().map(PolyVertex::new));
                    }
                    4 => {
                        // Spline edge: 94 degree, 73 rational, 74 periodic, 95 knot count, 96
                        // control point count, then the 40 knots, the 10/20 control points (each
                        // with a 42 weight when rational), and 97 fit point count with 11/21 fit
                        // points and 12/22, 13/23 end tangents (R2010+).
                        let (mut degree, mut nk, mut nc) = (3, 0, 0);
                        while let Some(x) = get(i).filter(|x| matches!(x.code, 94 | 73 | 74 | 95 | 96)) {
                            let n = x.i64().clamp(0, 100_000) as usize;
                            match x.code {
                                94 => degree = n.clamp(1, 10), // same cap as the SPLINE reader
                                95 => nk = n,
                                96 => nc = n,
                                _ => {}
                            }
                            i += 1;
                        }
                        let mut knots = Vec::new();
                        while knots.len() < nk && get(i).is_some_and(|x| x.code == 40) {
                            knots.push(f(i));
                            i += 1;
                        }
                        let (mut control, mut weights) = (Vec::new(), Vec::new());
                        while control.len() < nc && get(i).is_some_and(|x| x.code == 10) {
                            control.push(Vec2::new(f(i), f(i + 1)));
                            i += 2;
                            if get(i).is_some_and(|t| t.code == 42) {
                                weights.push(f(i));
                                i += 1;
                            }
                        }
                        let mut fit = Vec::new();
                        if get(i).is_some_and(|x| x.code == 97) {
                            let n = get(i).map(Tag::i64).unwrap_or(0).clamp(0, 100_000) as usize;
                            i += 1;
                            while fit.len() < n && get(i).is_some_and(|x| x.code == 11) {
                                fit.push(Vec2::new(f(i), f(i + 1)));
                                i += 2;
                            }
                        }
                        if weights.len() != control.len() {
                            weights.clear();
                        }
                        let sp = Spline { degree, knots, control, weights, ..Spline::default() };
                        let sp = if sp.is_valid() {
                            sp
                        } else if sp.control.is_empty() && fit.len() > 1 {
                            Spline::from_fit_points(&fit)
                        } else {
                            Spline::from_control(sp.control, degree)
                        };
                        let pts = sp.tessellate(1e-3);
                        let skip = usize::from(verts.last().zip(pts.first()).is_some_and(|(l, p)| l.p.near(*p, 1e-9)));
                        verts.extend(pts.into_iter().skip(skip).map(PolyVertex::new));
                    }
                    _ => {}
                }
            }
        }
        if verts.len() > 1 && verts.first().map(|f| f.p).zip(verts.last().map(|l| l.p)).is_some_and(|(a, b)| a.near(b, 1e-9)) {
            verts.pop();
        }
        // 97: number of source boundary objects, then their handles (330). A spline edge's fit
        // data count is a 97 too, followed by its fit points instead.
        let mut sources = Vec::new();
        for _ in 0..2 {
            while get(i).is_some_and(|x| !matches!(x.code, 97 | 92 | 75)) {
                i += 1;
            }
            if get(i).is_none_or(|x| x.code != 97) {
                break;
            }
            let n = get(i).map(Tag::i64).unwrap_or(0).clamp(0, MAX_HATCH_SOURCES as i64) as usize;
            i += 1;
            if get(i).is_some_and(|x| !matches!(x.code, 330 | 92 | 75)) {
                continue;
            }
            while sources.len() < n
                && let Some(x) = get(i).filter(|x| x.code == 330)
            {
                sources.extend(Handle::parse_hex(&x.str()).filter(|h| h.0 != 0));
                i += 1;
            }
            break;
        }
        loops.push(HatchLoop { vertices: verts, outer, sources });
    }
    let solid = solid || pattern.eq_ignore_ascii_case("SOLID");
    let (scale, angle) = (t.fd(41, 1.0), t.fd(52, 0.0).to_radians());
    // Keep the file's definition lines even for a name the library knows: the library's versions
    // of some patterns (AR-CONC) only approximate AutoCAD's, and drawing them at the file's scale
    // could be hundreds of times denser than the hatch the author saw.
    let pattern_lines = if solid { Vec::new() } else { pattern_definition(tags, scale, angle, origin) };
    Some(EntityKind::Hatch(Hatch {
        pattern: pattern.to_ascii_uppercase(),
        solid,
        loops,
        scale,
        angle,
        associative: t.i(71).unwrap_or(0) != 0,
        style: t.i(75).unwrap_or(0) as u8,
        // The first 30 is the elevation point's z (boundary points are 2D).
        elevation: t.f(30).filter(|z| z.is_finite()).unwrap_or(0.0),
        gradient: gradient(tags),
        origin,
        background,
        pattern_lines,
        mpolygon: None,
    }))
}

/// Where an MPOLYGON's own record (`100 AcDbMPolygon`) starts; 0 when the marker is missing.
fn mpolygon_start(tags: &[Tag]) -> usize {
    tags.iter().position(|x| x.code == 100 && matches!(&x.value, cadcraft_dxf::Value::Str(s) if s == "AcDbMPolygon")).unwrap_or(0)
}

/// An MPOLYGON's common properties: its own record has a 62 too (the fill colour), so they
/// are read from the groups before it; the extrusion (210) is in the record.
pub(super) fn mpolygon_common(tags: &[Tag]) -> Common {
    let at = mpolygon_start(tags);
    let mut c = common(&T(tags.get(..at).unwrap_or(&[])));
    let own = T(tags.get(at..).unwrap_or(&[]));
    c.extrusion = Vec3::new(own.fd(210, 0.0), own.fd(220, 0.0), own.fd(230, 1.0));
    c
}

/// MPOLYGON (DXF Reference): the boundary loops and pattern as HATCH has them, in the record
/// `AcDbMPolygon` (a version 70 may come before the solid-fill 70), then 62 the fill colour
/// and 11/21 the pattern's X direction. An MPOLYGON is never associative.
pub(super) fn mpolygon(tags: &[Tag]) -> Option<EntityKind> {
    let own = tags.get(mpolygon_start(tags)..).unwrap_or(&[]);
    let EntityKind::Hatch(mut h) = hatch(own, &T(own))? else { return None };
    let loops_at = own.iter().position(|x| x.code == 91).unwrap_or(own.len());
    let solid_flag = own.iter().take(loops_at).rfind(|x| x.code == 70).map(Tag::i64);
    h.solid = solid_flag == Some(1) || h.pattern.eq_ignore_ascii_case("SOLID");
    if h.solid {
        h.pattern_lines.clear();
    } else if h.pattern_lines.is_empty() {
        h.pattern_lines = pattern_definition(own, h.scale, h.angle, h.origin);
    }
    h.associative = false;
    h.loops.iter_mut().for_each(|l| l.sources.clear());
    mark_outer_loops(&mut h.loops);
    let fill_color = own.iter().find(|x| x.code == 62).map_or(Color::ByLayer, |x| Color::from_aci(i16::try_from(x.i64()).unwrap_or(256)));
    // The X direction follows the pattern data (an 11 earlier on is a boundary edge's end).
    let after = own.iter().rposition(|x| x.code == 76).unwrap_or(loops_at);
    let tail = T(own.get(after..).unwrap_or(&[]));
    let x_dir = Vec2::new(tail.fd(11, 1.0), tail.fd(21, 0.0));
    h.mpolygon = Some(cadcraft_doc::MPolygon { fill_color, x_dir: if x_dir.is_finite() { x_dir } else { Vec2::X } });
    Some(EntityKind::Hatch(h))
}

/// Upper bound on the loops whose nesting [`mark_outer_loops`] works out (pairs compared grow
/// with the square).
const MAX_NESTED_LOOPS: usize = 2000;

/// When no loop is flagged outer (an MPOLYGON from a DWG, whose loops carry no flags), a loop is
/// outer when it lies inside an even number of the others.
fn mark_outer_loops(loops: &mut [HatchLoop]) {
    if loops.iter().any(|l| l.outer) || loops.len() > MAX_NESTED_LOOPS {
        return;
    }
    let rings: Vec<Vec<Vec2>> = loops.iter().map(|l| l.vertices.iter().map(|v| v.p).collect()).collect();
    for (i, l) in loops.iter_mut().enumerate() {
        let Some(p) = rings.get(i).and_then(|r| r.first()).copied().filter(|p| p.is_finite()) else { continue };
        let depth = rings.iter().enumerate().filter(|(j, r)| *j != i && r.len() > 2 && cadcraft_geom::point_in_polygon(r, p)).count();
        l.outer = depth % 2 == 0;
    }
}

/// Upper bound on the source boundary objects of one HATCH loop.
const MAX_HATCH_SOURCES: usize = 4096;

/// Upper bounds on a HATCH's pattern definition lines and on the dashes of one line.
const MAX_PATTERN_LINES: usize = 1024;

const MAX_PATTERN_DASHES: usize = 256;

/// The pattern definition lines of a HATCH (groups 78, then per line 53 angle, 43/44 base
/// point, 45/46 offset, 79 dash count, 49 dash lengths). A file holds them as drawn: scaled,
/// rotated with the hatch and the offset as a world vector. They are kept the library's way
/// (unit scale, no hatch rotation, base point relative to the hatch origin, offset as shift
/// along the line and spacing), which the writer and the renderer turn back. Lines with
/// non-finite values are skipped.
fn pattern_definition(tags: &[Tag], scale: f64, angle: f64, origin: Vec2) -> Vec<cadcraft_doc::library::PatternLine> {
    let Some(at) = tags.iter().position(|x| x.code == 78) else { return Vec::new() };
    let scale = if scale.is_finite() && scale > 1e-9 { scale } else { 1.0 };
    let angle = if angle.is_finite() { angle } else { 0.0 };
    // Raw lines: angle (degrees), base, offset, dashes.
    let mut raw: Vec<(f64, Vec2, Vec2, Vec<f64>)> = Vec::new();
    for x in tags.iter().skip(at + 1) {
        let cur = raw.last_mut();
        match (x.code, cur) {
            (53, _) => {
                if raw.len() >= MAX_PATTERN_LINES {
                    break;
                }
                raw.push((x.f64(), Vec2::ZERO, Vec2::ZERO, Vec::new()));
            }
            (43, Some(l)) => l.1.x = x.f64(),
            (44, Some(l)) => l.1.y = x.f64(),
            (45, Some(l)) => l.2.x = x.f64(),
            (46, Some(l)) => l.2.y = x.f64(),
            (79, Some(_)) => {}
            (49, Some(l)) => {
                if l.3.len() < MAX_PATTERN_DASHES {
                    l.3.push(x.f64());
                }
            }
            _ => break,
        }
    }
    raw.into_iter()
        .filter(|(a, b, o, d)| a.is_finite() && b.is_finite() && o.is_finite() && d.iter().all(|v| v.is_finite()))
        .map(|(a, base, offset, dashes)| {
            let local = (base - origin).rotate(-angle) * (1.0 / scale);
            let delta = offset.rotate(-a.to_radians()) * (1.0 / scale);
            cadcraft_doc::library::PatternLine {
                angle: a - angle.to_degrees(),
                origin: (local.x, local.y),
                delta: (delta.x, delta.y),
                dashes: dashes.into_iter().map(|d| d / scale).collect(),
            }
        })
        .collect()
}

/// Hatch origin and background colour from `CADCRAFT` xdata: `1000 HATCH`, `1011` origin,
/// `1000` background colour name (empty for none). Defaults when absent.
fn hatch_xdata(tags: &[Tag]) -> (Vec2, Option<Color>) {
    let x = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP);
    let Some(at) = x.windows(2).position(|w| matches!(w, [m, o] if m.code == 1000 && m.str() == "HATCH" && o.code == 1011)) else {
        return (Vec2::ZERO, None);
    };
    let val = |k: usize, code: i32| x.get(at + k).filter(|t| t.code == code);
    let coord = |k: usize, code: i32| val(k, code).map(Tag::f64).filter(|v| v.is_finite()).unwrap_or(0.0);
    let origin = Vec2::new(coord(1, 1011), coord(2, 1021));
    (origin, val(4, 1000).and_then(|t| Color::parse(&t.str())))
}

/// Gradient values missing from a file fall back to the GRADIENT command's defaults.
fn default_gradient() -> Gradient {
    Gradient { name: "LINEAR".into(), color1: Color::Index(5), color2: Color::Index(7), angle: 0.0, centered: true }
}

/// A hatch's gradient fill: the native groups when present (another program may have edited
/// them), otherwise CADCraft's `GRADIENT` xdata (kept by R2000 files and DWG conversions).
fn gradient(tags: &[Tag]) -> Option<Gradient> {
    if tags.iter().any(|x| x.code == 450) { native_gradient(tags) } else { xdata_gradient(tags) }
}

/// `CADCRAFT` xdata: `1000 GRADIENT`, then name, 1040 angle, 1070 centered, colour 1 and 2 names.
fn xdata_gradient(tags: &[Tag]) -> Option<Gradient> {
    let x = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP);
    let at = x.iter().position(|t| t.code == 1000 && t.str() == "GRADIENT")?;
    let o = x.get(at + 1..).unwrap_or(&[]);
    let d = default_gradient();
    let strs: Vec<String> = o.iter().filter(|t| t.code == 1000).take(3).map(Tag::str).collect();
    let color = |i: usize, d: Color| strs.get(i).and_then(|s| Color::parse(s)).unwrap_or(d);
    Some(Gradient {
        name: strs.first().cloned().unwrap_or(d.name),
        color1: color(1, d.color1),
        color2: color(2, d.color2),
        angle: o.iter().find(|t| t.code == 1040).map(Tag::f64).filter(|a| a.is_finite()).unwrap_or(d.angle),
        centered: o.iter().find(|t| t.code == 1070).map_or(d.centered, |t| t.i64() != 0),
    })
}

/// DXF Reference HATCH groups 450–470, when 450 says gradient. Each 463 starts a colour record
/// whose 63 (ACI) and 421 (true colour) groups follow.
fn native_gradient(tags: &[Tag]) -> Option<Gradient> {
    let start = tags.iter().position(|x| x.code == 450)?;
    if tags.get(start)?.i64() != 1 {
        return None;
    }
    let mut g = default_gradient();
    // Per colour record: (ACI, true colour); a true colour wins over its ACI fallback.
    let mut colors: [(Option<Color>, Option<Rgb>); 2] = [(None, None); 2];
    let mut slot: Option<usize> = None;
    for x in tags.iter().skip(start + 1).take_while(|x| x.code != 1001) {
        let rec = slot.and_then(|s| colors.get_mut(s));
        match x.code {
            460 => g.angle = Some(x.f64()).filter(|a| a.is_finite()).unwrap_or(0.0),
            461 => g.centered = x.f64().abs() < 1e-9,
            463 => slot = Some(slot.map_or(0, |s| s.saturating_add(1))),
            63 => {
                if let Some(r) = rec {
                    r.0 = Some(Color::from_aci(i16::try_from(x.i64()).unwrap_or(256)));
                }
            }
            421 => {
                if let Some(r) = rec {
                    r.1 = Some(Rgb::from_u32(u32::try_from(x.i64() & 0xff_ffff).unwrap_or(0)));
                }
            }
            470 => g.name = x.str(),
            _ => {}
        }
    }
    let [c1, c2] = colors;
    let pick = |(aci, rgb): (Option<Color>, Option<Rgb>), d: Color| rgb.map(Color::True).or(aci).unwrap_or(d);
    g.color1 = pick(c1, g.color1);
    g.color2 = pick(c2, g.color2);
    Some(g)
}
