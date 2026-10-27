//! MPOLYGON: a filled area that draws its own boundary. CADCraft keeps it as a hatch with its
//! MPOLYGON data ([`cadcraft_doc::MPolygon`]): the loops are drawn in the entity's colour and the
//! fill (pattern, solid or gradient) in the MPOLYGON's fill colour. MPOLYGON makes one from
//! internal points or closed objects (interactively as HATCH picks them), or turns hatches into
//! MPOLYGONs (`convert`); HATCHEDIT `mpolygon: false` turns one back into a hatch.

use cadcraft_color::Color;
use cadcraft_doc::{EntityKind, Handle, HatchLoop, MPolygon};
use serde_json::{Value, json};

use super::{HatchM, HatchSettings, add_hatches, loops_at, loops_from_handles, patfile};
use crate::cmd::{CommandSpec, bad, bool_or, points_param, size_param, str_param, targets};
use crate::{Result, Session};

/// Upper bound on hatches converted by one call.
const MAX_CONVERTED: usize = 10_000;
/// Upper bound on internal points of one call.
const MAX_POINTS: usize = 1000;

pub(super) fn spec() -> CommandSpec {
    CommandSpec::new("mpolygon", "MPolygon", run_mpolygon)
        .params(
            "{points?: [[x,y]] (internal points) and/or handles?: [hex] (closed objects), pattern? (HPNAME), scale?, angle? (degrees), fillColor? (default ByLayer), erase?: bool (erase the closed objects used), convert?: [hex] (turn these hatches into MPOLYGONs, keeping their look)} — interactively, pick points or select objects as HATCH does",
        )
        .interactive(|s| {
            let mut m = HatchM::new(s, false);
            m.settings.mpolygon = Some(MPolygon::default());
            m.settings.associative = false;
            m.settings.separate = false;
            Ok(Box::new(m))
        })
}

/// A hatch made an MPOLYGON: no longer associative, filled in the colour it was drawn in.
pub(super) fn to_mpolygon(h: &mut cadcraft_doc::Hatch, color: Color) {
    super::assoc::detach(h);
    h.mpolygon = Some(MPolygon { fill_color: color, ..MPolygon::default() });
}

fn convert(s: &mut Session, p: &Value) -> Result<Value> {
    let raw = p.get("convert").and_then(Value::as_array).ok_or_else(|| bad("mpolygon", "`convert` is an array of hatch handles"))?;
    if raw.len() > MAX_CONVERTED {
        return Err(bad("mpolygon", format!("at most {MAX_CONVERTED} hatches")));
    }
    let hs: Vec<Handle> = raw.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex)).collect();
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in hs {
        let Some(e) = d.entity(h) else { continue };
        let EntityKind::Hatch(x) = &e.kind else { continue };
        if x.mpolygon.is_some() {
            continue;
        }
        let (mut x, color) = (x.clone(), e.common.color);
        to_mpolygon(&mut x, color);
        d.modify_entity(h, |e| e.kind = EntityKind::Hatch(x))?;
        n += 1;
    }
    if n == 0 {
        return Err(bad("mpolygon", "`convert` names no hatch"));
    }
    Ok(json!({ "converted": n, "message": format!("{n} hatch(es) converted to MPOLYGON.") }))
}

fn run_mpolygon(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("convert").is_some_and(|v| !v.is_null()) {
        return convert(s, p);
    }
    size_param("mpolygon", p, "scale", false)?;
    if let Some(up) = str_param(p, "pattern").map(str::to_ascii_uppercase)
        && up != "SOLID"
        && cadcraft_doc::library::pattern(&up).is_none()
        && patfile::user_pattern(&up).is_none()
    {
        return Err(bad("mpolygon", format!("unknown pattern {up}")));
    }
    let fill_color = match str_param(p, "fillColor") {
        Some(t) => Color::parse(t).ok_or_else(|| bad("mpolygon", "`fillColor` is a colour such as \"ByLayer\", \"5\" or \"255,128,0\""))?,
        None => Color::ByLayer,
    };
    if let Some(raw) = p.get("points").and_then(Value::as_array)
        && raw.len() > MAX_POINTS
    {
        return Err(bad("mpolygon", format!("at most {MAX_POINTS} points")));
    }
    let mut st = HatchSettings::from(s, p, false);
    st.associative = false;
    st.separate = false;
    st.mpolygon = Some(MPolygon { fill_color, ..MPolygon::default() });
    let pts = points_param(p, "points");
    let given = |k: &str| p.get(k).is_some_and(|v| !v.is_null());
    let mut loops: Vec<HatchLoop> = Vec::new();
    for q in pts.iter().flatten() {
        loops.extend(loops_at(s, *q)?);
    }
    let mut used = Vec::new();
    if pts.is_none() || given("handles") || given("handle") {
        used = targets(s, p)?;
        loops.extend(loops_from_handles(s, &used)?);
    }
    if loops.is_empty() {
        return Err(bad("mpolygon", "give internal `points` or closed `handles`"));
    }
    let made = add_hatches(s, &st, vec![loops])?;
    if bool_or(p, "erase", false) {
        let d = s.doc_mut()?;
        for h in used {
            d.remove_entity(h);
        }
    }
    let hex: Vec<String> = made.iter().map(|h| h.hex()).collect();
    Ok(json!({ "handle": hex.first(), "handles": hex }))
}
