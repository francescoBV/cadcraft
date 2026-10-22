use super::*;

// =====================================================================================
// TRIM / EXTEND for ellipses and splines
// =====================================================================================

pub(super) fn ellipse_of(e: &cadcraft_doc::Ellipse) -> Ellipse {
    Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end }
}

pub(super) fn ellipse_kind(e: &Ellipse) -> EntityKind {
    EntityKind::Ellipse(cadcraft_doc::Ellipse { center: v3(e.center), major: v3(e.major), ratio: e.ratio, start: e.start, end: e.end })
}

/// Parameters on `curve` (its first parameter: ellipse angle or spline parameter) where it
/// meets the cutting edges, exactly; both parameters where it passes twice through one point.
fn cut_params(curve: &Curve, cut: &[Curve]) -> Vec<f64> {
    cut.iter().flat_map(|c| curve.intersect_params(c)).map(|h| h.t1).filter(|t| t.is_finite()).collect()
}

/// Trim objects that modify.rs does not handle itself (ellipses and splines).
pub(crate) fn trim_other(k: &EntityKind, cut: &[Curve], pick: Vec2) -> Result<Vec<EntityKind>> {
    let none = || other("Object does not intersect a cutting edge.");
    match k {
        EntityKind::Ellipse(e) => {
            let ge = ellipse_of(e);
            let full = Ellipse { start: 0.0, end: TAU, ..ge };
            let params: Vec<f64> = cut_params(&Curve::ellipse(&ge), cut).into_iter().map(norm_angle).collect();
            if ge.is_full() {
                let mut ps = params;
                ps.sort_by(f64::total_cmp);
                ps.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                if ps.len() < 2 {
                    return Err(other("Ellipse must intersect cutting edges at two or more points."));
                }
                let tp = full.param_of(pick);
                let n = ps.len();
                for i in 0..n {
                    let (Some(a0), Some(a1)) = (ps.get(i).copied(), ps.get((i + 1) % n).copied()) else { continue };
                    if cadcraft_geom::angle_in_sweep(tp, a0, a1) {
                        return Ok(vec![ellipse_kind(&Ellipse { start: a1, end: a0, ..ge })]);
                    }
                }
                return Err(none());
            }
            let sweep = ge.sweep();
            let rel = |t: f64| norm_angle(t - ge.start);
            let mut ts: Vec<f64> = params.into_iter().map(rel).filter(|t| *t > 1e-9 && *t < sweep - 1e-9).collect();
            ts.sort_by(f64::total_cmp);
            let tp = rel(full.param_of(pick)).min(sweep);
            let lo = ts.iter().copied().filter(|t| *t < tp).fold(0.0, f64::max);
            let hi = ts.iter().copied().filter(|t| *t > tp).fold(sweep, f64::min);
            if lo == 0.0 && hi == sweep {
                return Err(none());
            }
            let mut v = Vec::new();
            if lo > 0.0 {
                v.push(ellipse_kind(&Ellipse { end: norm_angle(ge.start + lo), ..ge }));
            }
            if hi < sweep {
                v.push(ellipse_kind(&Ellipse { start: norm_angle(ge.start + hi), ..ge }));
            }
            Ok(v)
        }
        EntityKind::Spline(sp) => {
            if !sp.is_valid() {
                return Err(other("Invalid spline."));
            }
            let (lo_d, hi_d) = sp.domain();
            let mut ts: Vec<f64> = cut_params(&Curve::spline(sp), cut)
                .into_iter()
                .filter(|t| *t > lo_d + 1e-9 * (hi_d - lo_d) && *t < hi_d - 1e-9 * (hi_d - lo_d))
                .collect();
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            let tp = curves::spline_param_of(sp, pick);
            if sp.closed {
                return curve_edits::trim_closed_spline(sp, &ts, tp);
            }
            let lo = ts.iter().copied().filter(|t| *t < tp).fold(lo_d, f64::max);
            let hi = ts.iter().copied().filter(|t| *t > tp).fold(hi_d, f64::min);
            if lo == lo_d && hi == hi_d {
                return Err(none());
            }
            let mut v = Vec::new();
            if lo > lo_d
                && let Some(a) = curves::sub_spline(sp, lo_d, lo)
            {
                v.push(EntityKind::Spline(a));
            }
            if hi < hi_d
                && let Some(b) = curves::sub_spline(sp, hi, hi_d)
            {
                v.push(EntityKind::Spline(b));
            }
            Ok(v)
        }
        _ => Err(other("Cannot trim that object.")),
    }
}

/// Extend objects that modify.rs does not handle itself (elliptical arcs and open splines).
pub(crate) fn extend_other(k: &EntityKind, cut: &[Curve], pick: Vec2) -> Result<EntityKind> {
    match k {
        EntityKind::Ellipse(e) => {
            let ge = ellipse_of(e);
            if ge.is_full() {
                return Err(other("Cannot extend a closed ellipse."));
            }
            let full = Ellipse { start: 0.0, end: TAU, ..ge };
            let from_end = pick.dist(ge.at_param(ge.end)) < pick.dist(ge.at_param(ge.start));
            let hits: Vec<f64> = cut_params(&Curve::ellipse(&full), cut)
                .into_iter()
                .map(norm_angle)
                .filter(|t| !cadcraft_geom::angle_in_sweep(*t, ge.start, ge.end))
                .collect();
            let t = if from_end {
                hits.iter().copied().min_by(|x, y| ccw_sweep(ge.end, *x).total_cmp(&ccw_sweep(ge.end, *y)))
            } else {
                hits.iter().copied().min_by(|x, y| ccw_sweep(*x, ge.start).total_cmp(&ccw_sweep(*y, ge.start)))
            };
            let t = t.ok_or_else(|| other("Object does not intersect an edge."))?;
            Ok(ellipse_kind(&if from_end { Ellipse { end: t, ..ge } } else { Ellipse { start: t, ..ge } }))
        }
        EntityKind::Spline(sp) => curve_edits::extend_spline(sp, cut, pick),
        _ => Err(other("Cannot extend that object.")),
    }
}
