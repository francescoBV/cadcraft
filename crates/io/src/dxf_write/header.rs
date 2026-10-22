use super::*;

/// Header variables we write, with their group codes (DXF Reference, HEADER section).
/// `P2`/`P3` = 2D/3D point.
const HEADER_VARS: &[(&str, i32)] = &[
    ("INSBASE", -3),
    ("LIMMIN", -2),
    ("LIMMAX", -2),
    ("SNAPUNIT", -2),
    ("GRIDUNIT", -2),
    ("ORTHOMODE", 70),
    ("FILLMODE", 70),
    ("MIRRTEXT", 70),
    ("LTSCALE", 40),
    ("ATTMODE", 70),
    ("TEXTSIZE", 40),
    ("TRACEWID", 40),
    ("CMLSTYLE", 2),
    ("CMLJUST", 70),
    ("CMLSCALE", 40),
    ("TEXTSTYLE", 7),
    ("CLAYER", 8),
    ("CELTYPE", 6),
    ("CECOLOR", 62),
    ("CELTSCALE", 40),
    ("DIMSCALE", 40),
    ("DIMSTYLE", 2),
    ("LUNITS", 70),
    ("LUPREC", 70),
    ("AUNITS", 70),
    ("AUPREC", 70),
    ("ELEVATION", 40),
    ("THICKNESS", 40),
    ("FILLETRAD", 40),
    ("CHAMFERA", 40),
    ("CHAMFERB", 40),
    ("ANGBASE", 50),
    ("ANGDIR", 70),
    ("PDMODE", 70),
    ("PDSIZE", 40),
    ("PLINEWID", 40),
    ("CELWEIGHT", 370),
    ("LWDISPLAY", 290),
    ("INSUNITS", 70),
    ("MEASUREMENT", 70),
    ("PSLTSCALE", 70),
];

/// The current dimension style's variables as header variables (`$DIMTXT`, `$DIMDEC`…), after
/// `$DIMSTYLE`: AutoCAD draws new and updated dimensions with the header's DIM* values, which
/// otherwise stay its defaults (0.18 text, 4 decimals) whatever the current style says (#1051).
/// `$DIMSCALE` is a header variable of its own. `keep_read`: overrides read from the file are
/// written as read (not into an older version than the file's, which gets the style's values).
fn dim_header_vars(w: &mut W, d: &Drawing, keep_read: bool) {
    let name = d.header.str("DIMSTYLE", "Standard");
    let Some(style) = d.dim_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&name)) else { return };
    let v = serde_json::to_value(style).unwrap_or_default();
    let none = |_: &str| None;
    for (var, field) in cadcraft_doc::DIMVARS {
        if *var == "DIMSCALE" {
            continue;
        }
        let (Some(value), Some((_, _, kind))) = (v.get(*field), dxf_ext::DIM_CODES.iter().find(|(f, _, _)| f == field)) else { continue };
        // A value read that overrides the style (see `forget_dim_header_codes`): as read.
        if let Some(tag) = d.header.code(var).zip(d.header.get(var)).filter(|_| keep_read).and_then(|(c, hv)| header_value_tag(var, c, hv)) {
            w.s(9, format!("${var}"));
            w.t.push(tag);
            continue;
        }
        match kind {
            // Names, not handles, in the header.
            K::TextStyle | K::Block => {
                let Some(n) = value.as_str() else { continue };
                if *kind == K::TextStyle && n.trim().is_empty() {
                    continue;
                }
                w.s(9, format!("${var}"));
                w.s(if *kind == K::TextStyle { 7 } else { 1 }, n);
            }
            K::Linetype => {}
            _ => match dxf_ext::encode(*kind, value, &none, &none, &none) {
                Some(DimVal::Real(x)) => {
                    w.s(9, format!("${var}"));
                    w.f(40, x);
                }
                Some(DimVal::Int(i)) if i16::try_from(i).is_ok() => {
                    w.s(9, format!("${var}"));
                    w.i(70, i);
                }
                Some(DimVal::Str(t)) => {
                    w.s(9, format!("${var}"));
                    w.s(1, t);
                }
                _ => {}
            },
        }
    }
}

/// The header variables CADCraft writes from its model. `keep_read`: see [`dim_header_vars`].
pub(super) fn header_vars(w: &mut W, d: &Drawing, keep_read: bool) {
    for (name, code) in HEADER_VARS {
        let Some(v) = d.header.get(name) else { continue };
        match (*code, v) {
            (-2 | -3, HVal::Point(p)) => {
                let p = match *name {
                    "SNAPUNIT" => d.header.snap_unit().to3(0.0),
                    "GRIDUNIT" => d.header.grid_unit().to3(0.0),
                    _ => *p,
                };
                w.s(9, format!("${name}"));
                w.f(10, p.x);
                w.f(20, p.y);
                if *code == -3 {
                    w.f(30, p.z);
                }
            }
            (-2 | -3, _) => {}
            (c @ (40 | 50), v) => {
                if let Some(f) = v.as_f64() {
                    // ANGBASE is held in radians; `$ANGBASE` is in degrees.
                    let f = if *name == "ANGBASE" { f.to_degrees() } else { f };
                    w.s(9, format!("${name}"));
                    w.f(c, f);
                }
            }
            (c @ (62 | 70 | 290 | 370), v) => {
                if let Some(i) = v.as_i64() {
                    w.s(9, format!("${name}"));
                    w.i(c, i);
                }
            }
            (c, v) => {
                if let Some(t) = v.as_str() {
                    w.s(9, format!("${name}"));
                    w.s(c, t);
                }
            }
        }
    }
    dim_header_vars(w, d, keep_read);
}

/// Header variables never written from the values read: the version, code page and handle seed
/// (written for each save), the extents (computed), the current multileader style (written from
/// the model) and the version's GUID (a new save is a new version).
const OWN_HEADER_VARS: &[&str] = &["ACADVER", "DWGCODEPAGE", "HANDSEED", "EXTMIN", "EXTMAX", "CMLEADERSTYLE", "VERSIONGUID"];

/// Every header variable read with a group code ([`Header::codes`]) that the header written
/// since `from` doesn't have yet, with the code it was read with. A pointer is written as read
/// when it names a record the drawing still knows (`dxf_raw::finish` points it at that record's
/// new handle) and as the null handle otherwise.
pub(super) fn kept_header_vars(w: &mut W, d: &Drawing, from: usize) {
    let written: std::collections::HashSet<String> =
        w.t.get(from..)
            .unwrap_or_default()
            .iter()
            .filter(|t| t.code == 9)
            .map(|t| t.str().trim().trim_start_matches('$').to_ascii_uppercase())
            .collect();
    // Built at the first pointer.
    let mut known: Option<std::collections::HashSet<Handle>> = None;
    for (name, v) in &d.header.vars {
        let Some(code) = d.header.code(name) else { continue };
        if written.contains(name) || OWN_HEADER_VARS.contains(&name.as_str()) || name.is_empty() || !name.is_ascii() {
            continue;
        }
        let var = format!("${name}");
        match (code, v) {
            (-2 | -3, HVal::Point(p)) if p.is_finite() => {
                w.s(9, var);
                w.f(10, p.x);
                w.f(20, p.y);
                if code == -3 {
                    w.f(30, p.z);
                }
            }
            (c, v) if crate::dxf_raw::is_pointer(c) => {
                let known = v
                    .as_str()
                    .and_then(Handle::parse_hex)
                    .filter(|h| h.0 != 0)
                    .is_some_and(|h| known.get_or_insert_with(|| crate::dxf_raw::known_records(d)).contains(&h));
                w.s(9, var);
                w.s(c, if known { v.as_str().unwrap_or("0").trim().to_string() } else { "0".to_string() });
            }
            (c, v) => {
                if let Some(tag) = header_value_tag(name, c, v) {
                    w.s(9, var);
                    w.t.push(tag);
                }
            }
        }
    }
}

/// The group of header variable `name` with value `v` under group code `code` (not a point),
/// or `None` when the code can't hold it.
fn header_value_tag(name: &str, code: i32, v: &HVal) -> Option<Tag> {
    use cadcraft_dxf::{Kind, kind_of};
    if code <= 0 || code >= 1000 || code == 9 {
        return None;
    }
    // The variables the reader converts: $DIMJOGANG is in radians (the model holds degrees),
    // $DIMDSEP a character code (the model holds the character).
    match (name, v) {
        ("DIMJOGANG", v) => v.as_f64().filter(|f| f.is_finite()).map(|f| Tag::f(code, f.to_radians())),
        ("DIMDSEP", HVal::Str(s)) => s.chars().next().map(|ch| Tag::i(code, i64::from(u32::from(ch)))),
        (_, HVal::Point(_)) => None,
        (_, v) => match kind_of(code) {
            Kind::Real => v.as_f64().filter(|f| f.is_finite()).map(|f| Tag::f(code, f)),
            Kind::Int16 | Kind::Int32 | Kind::Int64 | Kind::Bool => v.as_i64().map(|i| Tag::i(code, i)),
            Kind::Str | Kind::Hex => match v {
                HVal::Str(s) => Some(Tag::s(code, s.clone())),
                HVal::Int(i) => Some(Tag::s(code, i.to_string())),
                _ => None,
            },
        },
    }
}

/// Forget the group codes of the header's dimension variables that only repeat the current
/// dimension style (they are written from the style, so a later change of the style shows);
/// those that differ are overrides AutoCAD keeps in the header, written back as read.
pub(crate) fn forget_dim_header_codes(d: &mut Drawing) {
    let name = d.header.str("DIMSTYLE", "Standard");
    let Some(style) = d.dim_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&name)) else { return };
    let v = serde_json::to_value(style).unwrap_or_default();
    let none = |_: &str| None;
    let same = |kind: K, sv: &serde_json::Value, hv: &HVal| match kind {
        K::Real | K::Angle => {
            matches!((sv.as_f64(), hv.as_f64()), (Some(a), Some(b)) if (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0))
        }
        K::Int | K::Bool | K::Color => match dxf_ext::encode(kind, sv, &none, &none, &none) {
            Some(DimVal::Int(i)) => hv.as_i64() == Some(i),
            _ => false,
        },
        K::Char => sv.as_str().and_then(|s| s.chars().next()) == hv.as_str().and_then(|s| s.chars().next()),
        K::Str | K::TextStyle | K::Block | K::Linetype => sv.as_str().zip(hv.as_str()).is_some_and(|(a, b)| a.trim().eq_ignore_ascii_case(b.trim())),
    };
    let mut forget = Vec::new();
    for (var, field) in cadcraft_doc::DIMVARS {
        let (Some(sv), Some((_, _, kind)), Some(hv)) = (v.get(*field), dxf_ext::DIM_CODES.iter().find(|(f, _, _)| f == field), d.header.get(var))
        else {
            continue;
        };
        if same(*kind, sv, hv) {
            forget.push(var.to_string());
        }
    }
    for var in forget {
        d.header.codes.remove(&var);
    }
}

/// `$DWGCODEPAGE`: before 2007 the code page read from the file when CADCraft knows it (the text
/// is ASCII with `\U+XXXX` escapes, right in any code page, but the file keeps its declared one);
/// otherwise, and from 2007 (UTF-8 text), `ANSI_1252`.
pub(super) fn code_page(d: &Drawing, version: Version) -> String {
    let read = d.header.get("DWGCODEPAGE").and_then(HVal::as_str).map(str::trim).unwrap_or_default();
    let known = read.is_ascii() && read.len() <= 32 && cadcraft_dxf::decode_code_page(b"", read).is_some();
    if version < Version::R2007 && known { read.to_string() } else { "ANSI_1252".into() }
}
