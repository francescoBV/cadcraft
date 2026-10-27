//! `dimstyle.overrides`, which sets the current dimension style's overrides (what the Dimension
//! Style Manager's Override edits) in one undo step, and [`regen_dims`], which redraws the
//! dimensions of a changed style.

use cadcraft_doc::{DimStyle, Drawing, EntityKind, Handle};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("dimstyle.overrides", "Dimension Style Overrides", run_overrides).params(
        "{<style fields or DIM* variables: values>, clear?: bool} → {overrides} (the current style's overrides, which new dimensions take; `clear` drops them all first)",
    )]
}

/// What new dimensions are drawn with: the current style with its overrides.
fn current_settings(d: &Drawing) -> DimStyle {
    let name = d.header.str("DIMSTYLE", "Standard");
    let mut st = d.dim_style(&name).cloned().unwrap_or_default().with_overrides(&d.dim_overrides());
    st.name = name;
    st
}

/// Redraw the dimensions `uses` picks (in the model, layouts and blocks) from their style: drop
/// the `*D` block a file gave them, which still shows the old settings.
pub(crate) fn regen_dims(d: &mut Drawing, uses: impl Fn(&cadcraft_doc::Dimension) -> bool) -> Result<()> {
    let stale = |e: &cadcraft_doc::Entity| matches!(&e.kind, EntityKind::Dimension(dm) if dm.block.is_some() && uses(dm));
    let regen = |e: &mut cadcraft_doc::Entity| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.block = None;
        }
    };
    let mut hs: Vec<Handle> = d.model.iter().filter(|e| stale(e)).map(|e| e.handle).collect();
    hs.extend(d.layouts.iter().flat_map(|l| l.entities.iter().filter(|e| stale(e)).map(|e| e.handle)));
    for h in hs {
        d.modify_entity(h, regen)?;
    }
    for block in d.blocks.values_mut() {
        let hs: Vec<Handle> = block.entities.iter().filter(|e| stale(e)).map(|e| e.handle).collect();
        if !hs.is_empty() {
            let b = std::sync::Arc::make_mut(block);
            for h in hs {
                b.entities.modify(h, regen);
            }
        }
    }
    Ok(())
}

/// `dimstyle.overrides`: set the current style's overrides (the header DIM* variables).
fn run_overrides(s: &mut Session, p: &Value) -> Result<Value> {
    let clear = crate::cmd::bool_or(p, "clear", false);
    let mut given = serde_json::Map::new();
    for (k, v) in p.as_object().into_iter().flatten() {
        if k == "clear" {
            continue;
        }
        let field = DimStyle::field_name(k)
            .filter(|f| !matches!(*f, "name" | "annotative"))
            .ok_or_else(|| bad("dimstyle.overrides", format!("`{k}` is not a dimension variable")))?;
        given.insert(field.to_string(), v.clone());
    }
    // Checked first (against the style alone when its overrides go), so nothing changes when
    // one value is unusable: a command's error doesn't undo what it did.
    let d = s.doc()?;
    let mut base = current_settings(d);
    if clear {
        base = d.dim_style(&base.name).cloned().unwrap_or_default();
    }
    let rejected = base.apply_fields(&given);
    if !rejected.is_empty() {
        return Err(bad("dimstyle.overrides", format!("unusable values for {}", rejected.join(", "))));
    }
    if clear {
        s.doc_mut()?.sync_dim_vars();
    }
    for (field, v) in &given {
        let Some((var, _)) = cadcraft_doc::DIMVARS.iter().find(|(_, f)| f == field) else { continue };
        // Colors are stored as color numbers in the header.
        let v = match serde_json::from_value::<cadcraft_color::Color>(v.clone()).ok().or_else(|| v.as_str().and_then(cadcraft_color::Color::parse)) {
            Some(c) if field.ends_with("Color") => json!(c.to_aci()),
            _ => v.clone(),
        };
        crate::sysvars::set(s, var, &v)?;
    }
    Ok(json!({ "overrides": s.doc()?.dim_overrides() }))
}
