//! Offsets of polylines with arc segments (bulges), as AutoCAD's OFFSET draws them.
//!
//! The method is the classic raw-offset-and-clean-up one used by CAD kernels:
//!
//! 1. **Raw offset.** Every segment is offset on its own: a line moves sideways, an arc keeps its
//!    centre and sweep (so its bulge) and changes radius. An arc whose radius would fall to zero
//!    or below collapses into the straight piece between its offset ends (it only survives in a
//!    loop the clean-up removes).
//! 2. **Joins.** Neighbouring offsets are joined where their source segments meet. On the inside
//!    of a turn they overlap and are trimmed to their crossing (or, when they don't cross,
//!    bridged by a straight piece that the clean-up removes). On the outside of a turn they leave
//!    a gap, filled as OFFSETGAPTYPE says ([`GapType`]): both pieces extended to meet (a line
//!    runs on straight, an arc round its circle; a round fill where they never meet), a round
//!    fillet of the offset distance about the source vertex, or a chamfer whose distance from the
//!    vertex is the offset distance.
//! 3. **Clean-up.** The raw offset is cut at every point where it crosses itself (and, for an
//!    open polyline, where it crosses the circles of the offset distance round the source's two
//!    ends). A slice that comes closer to the source than the offset distance, or crosses it, is a
//!    loop of the raw offset that the true offset doesn't have, and is dropped. The slices left
//!    are stitched end to end into one or more polylines: an inward offset of a dumbbell gives
//!    two closed polylines, an offset that collapses gives none.
//!
//! Widths are ignored (the offset is of the centre line, as in AutoCAD). Every loop is bounded
//! ([`MAX_PIECES`]); degenerate or hostile input gives no result, never a panic. Tolerances come
//! from [`crate::tol::Tol`], relative to the size of the polyline and the offset.

use crate::tol::Tol;
use crate::{Arc, Bounds2, Circle, Line, PolyVertex, Polyline, TAU, Vec2, circle_circle, line_circle, line_line_infinite, norm_angle};

/// How OFFSET fills the gap an offset leaves at the outside of a polyline corner (the
/// OFFSETGAPTYPE system variable).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GapType {
    /// 0: extend both offset segments until they meet (arcs round their circles).
    #[default]
    Extend,
    /// 1: fill with an arc of the offset distance about the corner.
    Fillet,
    /// 2: fill with a chamfer line at the offset distance from the corner.
    Chamfer,
}

impl GapType {
    /// From the OFFSETGAPTYPE value (anything else extends).
    pub fn from_index(i: i64) -> GapType {
        match i {
            1 => GapType::Fillet,
            2 => GapType::Chamfer,
            _ => GapType::Extend,
        }
    }
}

/// Most pieces (source segments, raw offset pieces, slices) one offset works on.
pub const MAX_PIECES: usize = 20_000;
/// Most self-intersections the clean-up splits at.
const MAX_SPLITS: usize = 50_000;
/// Slack on a segment parameter (a fraction of the segment) for crossings at its ends.
const U_SLACK: f64 = 1e-9;
/// Below this sine of the angle between them, two pieces that meet only touch (OFFSETGAPTYPE 0).
const TOUCH: f64 = 1e-6;

/// The circle of an arc piece: centre, radius, the start angle and the signed sweep.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ArcOf {
    pub c: Vec2,
    pub r: f64,
    pub a0: f64,
    pub sweep: f64,
}

/// One polyline segment: from `a` to `b`, straight (`bulge` 0) or a circular arc (`bulge` =
/// tan(sweep / 4), positive counter-clockwise), as in a polyline vertex.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Piece {
    pub a: Vec2,
    pub b: Vec2,
    pub bulge: f64,
}

impl Piece {
    pub fn line(a: Vec2, b: Vec2) -> Piece {
        Piece { a, b, bulge: 0.0 }
    }

    /// The arc's circle, or `None` for a straight piece.
    pub fn arc(&self) -> Option<ArcOf> {
        let b = self.bulge;
        if !(b.is_finite() && b.abs() >= 1e-12) || self.a == self.b {
            return None;
        }
        // The centre sits off the chord's midpoint by (chord / 2)·cot(sweep / 2) to the left.
        let ch = self.b - self.a;
        let c = self.a.mid(self.b) + ch.perp() * ((1.0 - b * b) / (4.0 * b));
        let r = ch.len() * (1.0 + b * b) / (4.0 * b.abs());
        let sweep = 4.0 * b.atan();
        (c.is_finite() && r.is_finite() && r > 0.0).then(|| ArcOf { c, r, a0: (self.a - c).angle(), sweep })
    }

    #[allow(dead_code)]
    pub fn len(&self) -> f64 {
        match self.arc() {
            Some(k) => k.r * k.sweep.abs(),
            None => self.a.dist(self.b),
        }
    }

    /// The point at `u` (0 at `a`, 1 at `b`; outside that the line or circle runs on).
    pub fn at(&self, u: f64) -> Vec2 {
        match self.arc() {
            Some(k) => Vec2::polar(k.c, k.r, k.a0 + k.sweep * u),
            None => self.a.lerp(self.b, u),
        }
    }

    /// The parameter of `p` (a point of the line or circle): on an arc, before the start (negative)
    /// for up to half the rest of the circle, after the end (above 1) for the other half.
    pub fn param(&self, p: Vec2) -> f64 {
        match self.arc() {
            Some(k) => arc_u(&k, p),
            None => {
                let d = self.b - self.a;
                let l2 = d.len2();
                if l2 > 0.0 { (p - self.a).dot(d) / l2 } else { 0.0 }
            }
        }
    }

    /// The unit direction of travel at `u`.
    pub fn tangent(&self, u: f64) -> Vec2 {
        match self.arc() {
            Some(k) => {
                let t = Vec2::from_angle(k.a0 + k.sweep * u).perp();
                if k.sweep > 0.0 { t } else { -t }
            }
            None => (self.b - self.a).normalized(),
        }
    }

    #[allow(dead_code)]
    pub fn reversed(&self) -> Piece {
        Piece { a: self.b, b: self.a, bulge: -self.bulge }
    }

    /// The part between parameters `u0` and `u1` with the given end points (on the piece).
    pub fn sub(&self, u0: f64, p0: Vec2, u1: f64, p1: Vec2) -> Piece {
        match self.arc() {
            Some(k) => Piece { a: p0, b: p1, bulge: (k.sweep * (u1 - u0) / 4.0).tan() },
            None => Piece::line(p0, p1),
        }
    }

    pub fn bounds(&self) -> Bounds2 {
        match self.arc() {
            Some(k) => {
                let (s, e) = if k.sweep > 0.0 { (k.a0, k.a0 + k.sweep) } else { (k.a0 + k.sweep, k.a0) };
                Arc::new(k.c, k.r, s, e).bounds().union(&Bounds2::new(self.a, self.b))
            }
            None => Bounds2::new(self.a, self.b),
        }
    }

    /// The point of the piece nearest `p`.
    pub fn closest(&self, p: Vec2) -> Vec2 {
        match self.arc() {
            Some(k) => {
                let u = arc_u(&k, p);
                let v = p - k.c;
                if (0.0..=1.0).contains(&u) && v.len() > 0.0 {
                    k.c + v * (k.r / v.len())
                } else if p.dist(self.a) <= p.dist(self.b) {
                    self.a
                } else {
                    self.b
                }
            }
            None => Line::new(self.a, self.b).closest(p),
        }
    }

    pub fn dist(&self, p: Vec2) -> f64 {
        self.closest(p).dist(p)
    }

    /// Offset by `d` to the left of travel; `None` when nothing is left. The flag is set when an
    /// arc collapsed (its radius fell to zero or below) into a straight stand-in.
    fn offset(&self, d: f64, tol: &Tol) -> Option<(Piece, bool)> {
        match self.arc() {
            None => {
                let n = (self.b - self.a).normalized().perp() * d;
                (n != Vec2::ZERO || d == 0.0).then(|| (Piece::line(self.a + n, self.b + n), false))
            }
            Some(k) => {
                // Left of a counter-clockwise arc is towards its centre.
                let r = if k.sweep > 0.0 { k.r - d } else { k.r + d };
                let (ua, ub) = ((self.a - k.c) / k.r, (self.b - k.c) / k.r);
                let (a, b) = (k.c + ua * r, k.c + ub * r);
                if r > tol.dist {
                    Some((Piece { a, b, bulge: self.bulge }, false))
                } else if a.dist(b) > tol.dist {
                    Some((Piece::line(a, b), true))
                } else {
                    None
                }
            }
        }
    }
}

/// The parameter of `p` round an arc (see [`Piece::param`]).
fn arc_u(k: &ArcOf, p: Vec2) -> f64 {
    let th = k.sweep.abs();
    if th.is_nan() || th <= 0.0 {
        return 0.0;
    }
    let phi = (p - k.c).angle();
    let mut delta = norm_angle(k.sweep.signum() * (phi - k.a0));
    if delta > th + (TAU - th) / 2.0 {
        delta -= TAU;
    }
    delta / th
}

/// Where two pieces' lines and circles cross: (parameter on `p`, on `q`, point). With `finite`,
/// only crossings on both pieces (within [`U_SLACK`], or within `tol` of an end).
pub(crate) fn crossings(p: &Piece, q: &Piece, finite: bool, tol: f64) -> Vec<(f64, f64, Vec2)> {
    let mut pts: Vec<Vec2> = Vec::new();
    match (p.arc(), q.arc()) {
        (None, None) => {
            if let Some((x, _, _)) = line_line_infinite(p.a, p.b, q.a, q.b) {
                pts.push(x);
            }
        }
        (None, Some(k)) => pts.extend(line_circle(&Line::new(p.a, p.b), &Circle::new(k.c, k.r)).into_iter().map(|(x, _)| x)),
        (Some(k), None) => pts.extend(line_circle(&Line::new(q.a, q.b), &Circle::new(k.c, k.r)).into_iter().map(|(x, _)| x)),
        // Concentric arcs (within the tolerance) never cross: their circles coincide or nest, and
        // nearly coincident centres would put spurious crossings anywhere round them.
        (Some(k), Some(m)) if k.c.near(m.c, tol) => {}
        (Some(k), Some(m)) => pts.extend(circle_circle(&Circle::new(k.c, k.r), &Circle::new(m.c, m.r))),
    }
    let on = |s: &Piece, x: Vec2, u: f64| (-U_SLACK..=1.0 + U_SLACK).contains(&u) || x.near(s.a, tol) || x.near(s.b, tol);
    pts.into_iter()
        .filter(|x| x.is_finite())
        .map(|x| (p.param(x), q.param(x), x))
        .filter(|(up, uq, x)| !finite || (on(p, *x, *up) && on(q, *x, *uq)))
        .collect()
}

/// The pieces of a polyline (repeated vertices skipped).
pub(crate) fn pieces_of(pl: &Polyline, tol: &Tol) -> Vec<Piece> {
    let n = pl.vertices.len();
    let count = if pl.closed { n } else { n.saturating_sub(1) };
    let mut out = Vec::with_capacity(count.min(MAX_PIECES));
    for i in 0..count.min(MAX_PIECES) {
        let (Some(v), Some(w)) = (pl.vertices.get(i), pl.vertices.get((i + 1) % n.max(1))) else { continue };
        if !(v.p.is_finite() && w.p.is_finite()) || tol.near(v.p, w.p) {
            continue;
        }
        let bulge = if v.bulge.is_finite() && v.bulge.abs() >= 1e-12 { v.bulge } else { 0.0 };
        out.push(Piece { a: v.p, b: w.p, bulge });
    }
    out
}

/// A piece of the raw offset and where it came from.
#[derive(Clone, Copy, Debug)]
struct Raw {
    piece: Piece,
    /// Index of the source segment.
    src: usize,
    collapsed: bool,
}

/// What joins two neighbouring raw pieces: their new ends and the pieces between them.
struct Join {
    end: Vec2,
    start: Vec2,
    fill: Vec<Piece>,
}

/// The part of `p` from `a` to `b`, two points on its line or circle (ends moved by joins).
fn with_ends(p: &Piece, a: Vec2, b: Vec2) -> Piece {
    match p.arc() {
        Some(k) => {
            let (ua, ub) = (arc_u(&k, a), arc_u(&k, b));
            Piece { a, b, bulge: (k.sweep * (ub - ua) / 4.0).tan() }
        }
        None => Piece::line(a, b),
    }
}

/// Join the raw offsets `p` (ending) and `n` (starting) round the source vertex `v`, where the
/// source turns from `tin` to `tout`.
fn join(p: &Raw, n: &Raw, v: Vec2, tin: Vec2, tout: Vec2, d: f64, gap: GapType, tol: &Tol) -> Join {
    let (pe, ns) = (p.piece.b, n.piece.a);
    let bridge = || Join { end: pe, start: ns, fill: vec![Piece::line(pe, ns)] };
    if tol.near(pe, ns) {
        let m = pe.mid(ns);
        return Join { end: m, start: m, fill: Vec::new() };
    }
    let round = || {
        let (ra, rb) = (pe - v, ns - v);
        let mut phi = ra.cross(rb).atan2(ra.dot(rb));
        let s = -d.signum();
        if phi * s < 0.0 {
            phi += s * TAU;
        }
        Join { end: pe, start: ns, fill: vec![Piece { a: pe, b: ns, bulge: (phi / 4.0).tan() }] }
    };
    if p.collapsed || n.collapsed {
        // Round the vertex next to a collapsed arc: the offset's true shape there is the circle
        // of the offset distance about it (the clean-up keeps what of it lies far enough out).
        return round();
    }
    let turn = tin.cross(tout);
    let outside = turn * d < 0.0 || (turn.abs() <= 1e-12 && tin.dot(tout) < 0.0);
    if !outside {
        // Overlap: trim both to the crossing nearest the joint.
        let best = crossings(&p.piece, &n.piece, true, tol.dist)
            .into_iter()
            .min_by(|x, y| ((1.0 - x.0).abs() + x.1.abs()).total_cmp(&((1.0 - y.0).abs() + y.1.abs())));
        return match best {
            Some((_, _, x)) => Join { end: x, start: x, fill: Vec::new() },
            None => bridge(),
        };
    }
    match gap {
        GapType::Fillet => round(),
        GapType::Extend => {
            // Where the line or circle of each runs on to meet the other, nearest the corner. A
            // touch (the two run on tangent to each other) is no meeting: rounding decides whether
            // a tangency is found at all, so it would extend one corner of a symmetric shape and
            // round the other; it is rounded like a miss.
            let best = crossings(&p.piece, &n.piece, false, tol.dist)
                .into_iter()
                .filter(|(up, un, _)| *up >= 1.0 - U_SLACK && *un <= U_SLACK)
                .filter(|(up, un, _)| p.piece.tangent(*up).cross(n.piece.tangent(*un)).abs() > TOUCH)
                .min_by(|x, y| x.2.dist(v).total_cmp(&y.2.dist(v)));
            match best {
                Some((_, _, x)) => Join { end: x, start: x, fill: Vec::new() },
                None => round(),
            }
        }
        GapType::Chamfer => {
            let (ta, tb) = (p.piece.tangent(1.0), n.piece.tangent(0.0));
            let (ra, rb) = (pe - v, ns - v);
            let m = (ra + rb).normalized();
            let (ka, kb) = (ta.dot(m), tb.dot(m));
            if m == Vec2::ZERO || ka.abs() < 1e-9 || kb.abs() < 1e-9 {
                return round();
            }
            let pa = pe + ta * ((d.abs() - ra.dot(m)) / ka);
            let pb = ns + tb * ((d.abs() - rb.dot(m)) / kb);
            if !(pa.is_finite() && pb.is_finite()) || (pa - pe).dot(ta) < -tol.dist || (pb - ns).dot(tb) > tol.dist {
                return round();
            }
            // A line runs on to the chamfer; an arc keeps its end and runs on along its tangent.
            let mut fill = Vec::new();
            let end = if p.piece.arc().is_none() {
                pa
            } else {
                fill.push(Piece::line(pe, pa));
                pe
            };
            fill.push(Piece::line(pa, pb));
            let start = if n.piece.arc().is_none() {
                pb
            } else {
                fill.push(Piece::line(pb, ns));
                ns
            };
            Join { end, start, fill }
        }
    }
}

/// The raw offset: offset pieces, joined.
fn raw_offset(src: &[Piece], closed: bool, d: f64, gap: GapType, tol: &Tol) -> Vec<Piece> {
    let raw: Vec<Raw> =
        src.iter().enumerate().filter_map(|(i, s)| s.offset(d, tol).map(|(piece, collapsed)| Raw { piece, src: i, collapsed })).collect();
    let n = raw.len();
    if n == 0 {
        return Vec::new();
    }
    let ns = src.len();
    // joins[i] sits before raw[i] (between raw[i - 1] and raw[i]).
    let mut joins: Vec<Option<Join>> = Vec::with_capacity(n);
    for i in 0..n {
        if i == 0 && !closed {
            joins.push(None);
            continue;
        }
        let (Some(p), Some(q)) = (raw.get((i + n - 1) % n), raw.get(i)) else {
            joins.push(None);
            continue;
        };
        // Neighbouring source segments meet at a vertex; across a dropped segment, bridge.
        let adjacent = (p.src + 1) % ns.max(1) == q.src;
        let j = match (adjacent, src.get(p.src), src.get(q.src)) {
            (true, Some(sp), Some(sq)) => join(p, q, sq.a, sp.tangent(1.0), sq.tangent(0.0), d, gap, tol),
            _ => Join { end: p.piece.b, start: q.piece.a, fill: vec![Piece::line(p.piece.b, q.piece.a)] },
        };
        joins.push(Some(j));
    }
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let Some(r) = raw.get(i) else { continue };
        let start = joins.get(i).and_then(|j| j.as_ref()).map_or(r.piece.a, |j| j.start);
        let next = if i + 1 < n {
            joins.get(i + 1)
        } else if closed {
            joins.first()
        } else {
            None
        };
        let next = next.and_then(|j| j.as_ref());
        let end = next.map_or(r.piece.b, |j| j.end);
        out.push(with_ends(&r.piece, start, end));
        if let Some(j) = next {
            out.extend(j.fill.iter().copied());
        }
    }
    out.retain(|p| !tol.near(p.a, p.b) && p.a.is_finite() && p.b.is_finite());
    out
}

/// A split of the raw offset: piece `k` at parameter `u`, through `x`.
#[derive(Clone, Copy, Debug)]
struct Cut {
    k: usize,
    u: f64,
    x: Vec2,
}

/// The points where the raw offset crosses itself (and the extra `circles` cut it), as cuts.
fn cuts(raw: &[Piece], closed: bool, circles: &[Piece], tol: &Tol) -> Option<Vec<Cut>> {
    let m = raw.len();
    let mut out: Vec<Cut> = Vec::new();
    let boxes: Vec<Bounds2> = raw.iter().map(|p| p.bounds().expand(tol.dist)).collect();
    // Sweep over the pieces sorted by their left edge.
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|a, b| boxes.get(*a).map_or(0.0, |x| x.min.x).total_cmp(&boxes.get(*b).map_or(0.0, |x| x.min.x)));
    let add = |out: &mut Vec<Cut>, k: usize, u: f64, x: Vec2| -> Option<()> {
        out.push(Cut { k, u: u.clamp(0.0, 1.0), x });
        (out.len() <= MAX_SPLITS).then_some(())
    };
    for (oi, &i) in order.iter().enumerate() {
        let (Some(pi), Some(bi)) = (raw.get(i), boxes.get(i)) else { continue };
        for &j in order.iter().skip(oi + 1) {
            let (Some(pj), Some(bj)) = (raw.get(j), boxes.get(j)) else { continue };
            if bj.min.x > bi.max.x {
                break;
            }
            if !bi.intersects(bj) {
                continue;
            }
            let (lo, hi) = (i.min(j), i.max(j));
            // The shared ends of neighbours are not crossings.
            let mut shared: [Option<Vec2>; 2] = [None, None];
            if hi == lo + 1 {
                shared[0] = raw.get(lo).map(|p| p.b);
            }
            if closed && lo == 0 && hi + 1 == m {
                shared[1] = raw.get(lo).map(|p| p.a);
            }
            for (ui, uj, x) in crossings(pi, pj, true, tol.dist) {
                if shared.iter().flatten().any(|s| s.near(x, tol.dist * 16.0)) {
                    continue;
                }
                add(&mut out, i, ui, x)?;
                add(&mut out, j, uj, x)?;
            }
        }
    }
    for c in circles {
        let cb = c.bounds().expand(tol.dist);
        for (k, p) in raw.iter().enumerate() {
            if boxes.get(k).is_some_and(|b| b.intersects(&cb)) {
                for (u, _, x) in crossings(p, c, false, tol.dist) {
                    if (-U_SLACK..=1.0 + U_SLACK).contains(&u) {
                        add(&mut out, k, u, x)?;
                    }
                }
            }
        }
    }
    Some(out)
}

/// Positions along the raw offset, in order, without repeats: (piece index + parameter, point).
fn positions(raw: &[Piece], mut cs: Vec<Cut>, closed: bool, tol: &Tol) -> Vec<(f64, Vec2)> {
    let m = raw.len();
    // A cut at the very end of a piece is the start of the next one.
    let mut pos: Vec<(f64, Vec2)> = cs
        .drain(..)
        .filter_map(|c| {
            let p = raw.get(c.k)?;
            let at_end = c.u >= 1.0 - U_SLACK || c.x.near(p.b, tol.dist);
            let at_start = c.u <= U_SLACK || c.x.near(p.a, tol.dist);
            let s = if at_end {
                if c.k + 1 < m {
                    (c.k + 1) as f64
                } else if closed {
                    0.0
                } else {
                    m as f64
                }
            } else if at_start {
                c.k as f64
            } else {
                c.k as f64 + c.u
            };
            Some((s, c.x))
        })
        .collect();
    pos.sort_by(|a, b| a.0.total_cmp(&b.0));
    pos.dedup_by(|b, a| (b.0 - a.0).abs() <= 1e-12 || (b.1.near(a.1, tol.dist * 16.0) && (b.0 - a.0).abs() < 1.0));
    if !closed {
        pos.retain(|(s, _)| *s > 0.0 && *s < m as f64);
    }
    pos
}

/// The raw offset between positions `s0` (point `x0`) and `s1` (point `x1`), wrapping past the
/// end when `s1 <= s0` on a closed raw offset.
fn slice(raw: &[Piece], (s0, x0): (f64, Vec2), (s1, x1): (f64, Vec2), wrap: bool) -> Vec<Piece> {
    let m = raw.len();
    let mut out = Vec::new();
    let end = if wrap { s1 + m as f64 } else { s1 };
    let mut s = s0;
    let mut from = x0;
    let mut guard = 0usize;
    while s < end - 1e-12 && guard <= m + 1 {
        guard += 1;
        let k_abs = s.floor();
        let k = (k_abs as usize) % m.max(1);
        let Some(p) = raw.get(k) else { break };
        let u0 = s - k_abs;
        let piece_end = k_abs + 1.0;
        let (u1, to) = if end <= piece_end + 1e-12 { (end - k_abs, x1) } else { (1.0, p.b) };
        let q = p.sub(u0, from, u1, to);
        if !q.a.near(q.b, 0.0) {
            out.push(q);
        }
        s = piece_end;
        from = to;
    }
    out
}

/// Whether a slice of the raw offset belongs to the true offset: no point of it comes closer
/// to the source than the offset distance, and it doesn't cross the source.
fn valid(sl: &[Piece], src: &[Piece], src_boxes: &[Bounds2], d: f64, tol: &Tol) -> bool {
    let reach = d.abs() - tol.dist * 16.0;
    let too_close = |x: Vec2| {
        let probe = Bounds2::new(x, x).expand(reach.max(0.0));
        src.iter().zip(src_boxes).any(|(s, b)| b.intersects(&probe) && s.dist(x) < reach)
    };
    for p in sl {
        for u in [0.5, 0.25, 0.75] {
            if too_close(p.at(u)) {
                return false;
            }
        }
        let pb = p.bounds().expand(tol.dist);
        for (s, b) in src.iter().zip(src_boxes) {
            if b.intersects(&pb) && !crossings(p, s, true, tol.dist).is_empty() {
                return false;
            }
        }
    }
    true
}

/// Whether `q` continues `p` on the same line or circle in the same direction, and the piece
/// they make together.
fn continues(p: &Piece, q: &Piece, tol: f64) -> Option<Piece> {
    match (p.arc(), q.arc()) {
        (None, None) => {
            let (u, v) = (p.b - p.a, q.b - q.a);
            let l = u.len().max(v.len());
            (u.dot(v) > 0.0 && u.cross(v).abs() <= tol * l && Line::new(p.a, p.b).project(q.b).dist(q.b) <= tol).then(|| Piece::line(p.a, q.b))
        }
        (Some(k), Some(m)) => {
            let sweep = k.sweep + m.sweep;
            (k.sweep * m.sweep > 0.0 && k.c.near(m.c, tol) && (k.r - m.r).abs() <= tol && sweep.abs() < TAU - 1e-6).then(|| Piece {
                a: p.a,
                b: q.b,
                bulge: (sweep / 4.0).tan(),
            })
        }
        _ => None,
    }
}

/// Stitch slices end to end into polylines. Where two slices meet on one line or circle they
/// become one piece (not at the other vertices, which stay as the source had them); `seam_cut`:
/// the slices start at cuts, so a loop's seam is one of those joints too.
fn stitch(mut slices: Vec<Vec<Piece>>, tol: f64, seam_cut: bool) -> Vec<Polyline> {
    let mut out = Vec::new();
    while !slices.is_empty() {
        let mut chain = slices.remove(0);
        let mut guard = 0usize;
        loop {
            guard += 1;
            let (Some(first), Some(last)) = (chain.first().map(|p| p.a), chain.last().map(|p| p.b)) else { break };
            if (chain.len() >= 2 || guard > 1) && first.near(last, tol) {
                break;
            }
            let next = slices
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.first().map(|p| (i, p.a.dist(last))))
                .filter(|(_, dd)| *dd <= tol)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            match next {
                Some(i) if guard <= MAX_PIECES => {
                    let mut next = slices.remove(i).into_iter();
                    if let (Some(l), Some(f)) = (chain.last_mut(), next.next()) {
                        match continues(l, &f, tol) {
                            Some(m) => *l = m,
                            None => chain.push(f),
                        }
                    }
                    chain.extend(next);
                }
                _ => break,
            }
        }
        let is_loop = chain.len() >= 2 && chain.first().zip(chain.last()).is_some_and(|(f, l)| f.a.near(l.b, tol));
        while is_loop && seam_cut && chain.len() > 2 {
            let Some(m) = chain.last().zip(chain.first()).and_then(|(l, f)| continues(l, f, tol)) else { break };
            chain.pop();
            if let Some(f) = chain.first_mut() {
                *f = m;
            }
        }
        let mut vertices: Vec<PolyVertex> = chain.iter().map(|p| PolyVertex::with_bulge(p.a, p.bulge)).collect();
        if !is_loop && let Some(l) = chain.last() {
            vertices.push(PolyVertex::new(l.b));
        }
        if vertices.len() >= 2 {
            out.push(Polyline { vertices, closed: is_loop });
        }
    }
    out
}

impl Polyline {
    /// The offset by `d` to the left of the direction of travel (negative: to the right), as
    /// AutoCAD's OFFSET makes it: arcs stay arcs, corners join as `gap` says, the loops an
    /// offset forms where it overruns itself are removed. Usually one polyline; several where
    /// an offset splits the shape (or crossing parts survive), none where it collapses.
    pub fn offset(&self, d: f64, gap: GapType) -> Vec<Polyline> {
        if !d.is_finite() || d == 0.0 || self.vertices.len() > MAX_PIECES {
            return Vec::new();
        }
        let b = self.bounds();
        if b.is_empty() || !(b.width().is_finite() && b.height().is_finite()) {
            return Vec::new();
        }
        let mag = b.min.x.abs().max(b.min.y.abs()).max(b.max.x.abs()).max(b.max.y.abs());
        let tol = Tol::new(b.width().hypot(b.height()) + d.abs(), mag + d.abs());
        let src = pieces_of(self, &tol);
        // A polyline drawn closed by repeating its first vertex works as a closed one.
        let closed = self.closed;
        if src.is_empty() {
            return Vec::new();
        }
        let raw = raw_offset(&src, closed, d, gap, &tol);
        if raw.is_empty() || raw.len() > MAX_PIECES {
            return Vec::new();
        }
        // The ends of an open polyline: the circles of the offset distance round them.
        let caps: Vec<Piece> = if closed {
            Vec::new()
        } else {
            [src.first().map(|p| p.a), src.last().map(|p| p.b)]
                .into_iter()
                .flatten()
                .flat_map(|c| {
                    let r = d.abs();
                    let (e, w) = (c + Vec2::X * r, c - Vec2::X * r);
                    [Piece { a: e, b: w, bulge: 1.0 }, Piece { a: w, b: e, bulge: 1.0 }]
                })
                .collect()
        };
        let Some(cs) = cuts(&raw, closed, &caps, &tol) else { return Vec::new() };
        let pos = positions(&raw, cs, closed, &tol);
        let m = raw.len();
        let mut slices: Vec<Vec<Piece>> = Vec::new();
        if closed {
            if pos.is_empty() {
                slices.push(raw.clone());
            } else {
                for i in 0..pos.len() {
                    let (Some(&a), Some(&b)) = (pos.get(i), pos.get((i + 1) % pos.len())) else { continue };
                    let wrap = i + 1 == pos.len();
                    slices.push(slice(&raw, a, b, wrap));
                }
            }
        } else {
            let start = (0.0, raw.first().map_or(Vec2::ZERO, |p| p.a));
            let end = (m as f64, raw.last().map_or(Vec2::ZERO, |p| p.b));
            let mut pts = vec![start];
            pts.extend(pos.iter().copied());
            pts.push(end);
            for w in pts.windows(2) {
                if let [a, b] = w {
                    slices.push(slice(&raw, *a, *b, false));
                }
            }
        }
        let src_boxes: Vec<Bounds2> = src.iter().map(Piece::bounds).collect();
        let mut keep: Vec<Vec<Piece>> = Vec::new();
        let join_tol = tol.dist * 1e3;
        for s in slices {
            // Where raw pieces overlap along a line, the same slice comes twice: keep one.
            let same = |k: &Vec<Piece>| {
                k.len() == s.len()
                    && k.iter().zip(&s).all(|(x, y)| x.a.near(y.a, join_tol) && x.b.near(y.b, join_tol) && (x.bulge - y.bulge).abs() <= 1e-9)
            };
            if !s.is_empty() && !keep.iter().any(same) && valid(&s, &src, &src_boxes, d, &tol) {
                keep.push(s);
            }
        }
        let mut out = stitch(keep, join_tol, !pos.is_empty());
        if closed {
            // A closed polyline's offset is closed loops; slivers between overlapping pieces have
            // no area.
            let size = b.width().hypot(b.height()) + d.abs();
            out.retain(|p| p.closed && p.area().abs() > tol.dist * size);
        }
        out
    }
}

#[cfg(test)]
#[path = "poly_offset_tests.rs"]
mod tests;
