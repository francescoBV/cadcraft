//! The Dimension Style Manager (DIMSTYLE), laid out as AutoCAD for Windows lays it out: the
//! drawing's styles with a preview and a description, Set Current, New, Modify, Override, Compare
//! (and Rename and Delete on a style's shortcut menu). Modify, Override and New show the style's
//! variables on AutoCAD's tabs (Lines, Symbols and Arrows, Text, Fit, Primary Units, Alternate
//! Units, Tolerances) beside a live preview; OK applies the changes through the `dimstyle` and
//! `dimstyle.overrides` commands (one undo step each), Cancel drops them. The tabs are built from
//! the engine's table of variables ([`cadcraft_engine::dimprops`]).

use std::hash::{Hash, Hasher};

use cadcraft_engine::dimprops::{self, DIM_PROPS, DIM_TABS, DimProp, DimPropKind, dim_value, join_post, split_post};
use cadcraft_engine::doc::{DimStyle, Drawing, EntityKind};
use egui::{RichText, vec2};
use serde_json::{Map, Value, json};

use crate::CadApp;
use crate::parametric::expr_field;
use crate::theme::Tokens;

/// The current style's overrides in the list.
const OVERRIDES: &str = "<style overrides>";
const STATE: &str = "dimstyle_manager";

fn tab_label(i: usize) -> &'static str {
    match i {
        0 => crate::tl!("Lines"),
        1 => crate::tl!("Symbols and Arrows"),
        2 => crate::tl!("Text"),
        3 => crate::tl!("Fit"),
        4 => crate::tl!("Primary Units"),
        5 => crate::tl!("Alternate Units"),
        _ => crate::tl!("Tolerances"),
    }
}

#[derive(Clone, Default, PartialEq)]
enum Page {
    #[default]
    List,
    New,
    Edit,
    Compare,
}

/// What the editing pages change: an existing style, the current style's overrides, or a style
/// being created.
#[derive(Clone, Default, PartialEq)]
enum Target {
    #[default]
    Modify,
    Override,
    Create,
}

#[derive(Clone, Default)]
struct State {
    page: Page,
    /// The style chosen in the list, or [`OVERRIDES`].
    selected: String,
    in_use_only: bool,
    new_name: String,
    start_with: String,
    new_annotative: bool,
    /// The style the editing pages show, its settings as edited, and what OK does with them.
    editing: String,
    target: Target,
    values: Map<String, Value>,
    original: Map<String, Value>,
    tab: usize,
    compare_with: String,
    /// (style, new name) while renaming.
    renaming: Option<(String, String)>,
}

fn from_json(m: &Map<String, Value>) -> DimStyle {
    serde_json::from_value(Value::Object(m.clone())).unwrap_or_default()
}

/// Names of the dimension styles some dimension, leader or tolerance uses.
fn in_use(d: &Drawing) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for st in stores {
        for e in st.iter() {
            let name = match &e.kind {
                EntityKind::Dimension(dm) => &dm.style,
                EntityKind::Leader(l) => &l.style,
                EntityKind::Tolerance(t) => &t.style,
                _ => continue,
            };
            if !out.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                out.push(name.clone());
            }
        }
    }
    out
}

pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> super::Action {
    let Ok(d) = app.session.doc() else {
        super::no_drawing(ctx, crate::tl!("Dimension Style Manager"), open);
        return None;
    };
    let mut st: State = super::temp(ctx, STATE).unwrap_or_default();
    let current = d.header.str("DIMSTYLE", "Standard");
    let overrides = d.dim_overrides();
    let valid = |n: &str| n == OVERRIDES && !overrides.is_empty() || d.dim_style(n).is_some();
    if !valid(&st.selected) {
        st.selected.clone_from(&current);
    }
    let mut actions: Vec<(&'static str, Value)> = Vec::new();
    let mut close = false;
    let title = match (&st.page, &st.target) {
        (Page::List, _) => crate::tl!("Dimension Style Manager").to_string(),
        (Page::New, _) => crate::tl!("Create New Dimension Style").to_string(),
        (Page::Edit, Target::Modify) => crate::tf!("Modify Dimension Style: {style}", style = st.editing),
        (Page::Edit, Target::Override) => crate::tf!("Override Current Style: {style}", style = st.editing),
        (Page::Edit, Target::Create) => crate::tf!("New Dimension Style: {style}", style = st.editing),
        (Page::Compare, _) => crate::tl!("Compare Dimension Styles").to_string(),
    };
    egui::Window::new(title)
        .id(egui::Id::unique(STATE))
        .open(open)
        .default_size(vec2(760.0, 520.0))
        .min_width(760.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| match st.page {
            Page::List => list_page(ui, d, &mut st, &current, &overrides, &mut actions, &mut close),
            Page::New => new_page(ui, d, &mut st),
            Page::Edit => edit_page(ui, d, &mut st, &mut actions),
            Page::Compare => compare_page(ui, d, &mut st),
        });
    if close {
        *open = false;
        st.page = Page::List;
    }
    super::set_temp(ctx, STATE, st);
    // Errors are echoed on the command line by `run`; a failed step stops the rest.
    for (cmd, p) in actions {
        if app.run(cmd, p).is_err() {
            break;
        }
    }
    None
}

fn list_page(
    ui: &mut egui::Ui,
    d: &Drawing,
    st: &mut State,
    current: &str,
    overrides: &Map<String, Value>,
    actions: &mut Vec<(&'static str, Value)>,
    close: &mut bool,
) {
    let t = Tokens::get();
    ui.label(crate::tf!("Current dimension style: {style}", style = current));
    ui.add_space(4.0);
    let used = in_use(d);
    ui.horizontal_top(|ui| {
        // The styles, the current style's overrides under it.
        ui.vertical(|ui| {
            ui.set_width(190.0);
            ui.label(crate::tl!("Styles:"));
            egui::Frame::group(ui.style()).fill(t.field).show(ui, |ui| {
                ui.set_min_height(250.0);
                egui::ScrollArea::vertical().id_salt("dimstyle_list").max_height(250.0).auto_shrink([false, true]).show(ui, |ui| {
                    for s in &d.dim_styles {
                        if st.in_use_only && !used.iter().any(|n| n.eq_ignore_ascii_case(&s.name)) && !s.name.eq_ignore_ascii_case(current) {
                            continue;
                        }
                        let mut label = RichText::new(&s.name);
                        if s.name.eq_ignore_ascii_case(current) {
                            label = label.strong();
                        }
                        let resp = ui.selectable_label(st.selected == s.name, label);
                        if resp.clicked() {
                            st.selected.clone_from(&s.name);
                        }
                        resp.context_menu(|ui| {
                            if ui.add_enabled(!s.name.eq_ignore_ascii_case(current), egui::Button::new(crate::tl!("Set Current"))).clicked() {
                                actions.push(("dimstyle.current", json!({ "name": s.name })));
                                ui.close();
                            }
                            if ui.button(crate::tl!("Rename")).clicked() {
                                st.renaming = Some((s.name.clone(), s.name.clone()));
                                ui.close();
                            }
                            if ui.add_enabled(deletable(&s.name, current, &used), egui::Button::new(crate::tl!("Delete"))).clicked() {
                                actions.push(("dimstyle.delete", json!({ "name": s.name })));
                                ui.close();
                            }
                        });
                        if s.name.eq_ignore_ascii_case(current) && !overrides.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                let resp = ui.selectable_label(st.selected == OVERRIDES, crate::tl!("<style overrides>"));
                                if resp.clicked() {
                                    st.selected = OVERRIDES.into();
                                }
                                resp.context_menu(|ui| {
                                    if ui.button(crate::tl!("Save to current style")).clicked() {
                                        let mut p = overrides.clone();
                                        p.insert("name".into(), json!(current));
                                        actions.push(("dimstyle", Value::Object(p)));
                                        ui.close();
                                    }
                                    if ui.button(crate::tl!("Delete")).clicked() {
                                        actions.push(("dimstyle.current", json!({ "name": current })));
                                        ui.close();
                                    }
                                });
                            });
                        }
                    }
                });
            });
            let list_caption = ui.label(crate::tl!("List:"));
            craft_ui::ComboBox::from_id_salt("dimstyle_list_filter")
                .width(180.0)
                .selected_text(if st.in_use_only { crate::tl!("Styles in use") } else { crate::tl!("All styles") })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut st.in_use_only, false, crate::tl!("All styles"));
                    ui.selectable_value(&mut st.in_use_only, true, crate::tl!("Styles in use"));
                })
                .response
                .labelled_by(list_caption.id);
            if let Some((from, to)) = &mut st.renaming {
                let from = from.clone();
                let mut done = false;
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(to).desired_width(120.0)).accessible_name(crate::tl!("New name"));
                    if ui.button(crate::tl!("OK")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        if !to.trim().is_empty() && *to != from {
                            actions.push(("dimstyle.rename", json!({ "from": from, "to": to.trim() })));
                            if st.selected == from {
                                st.selected = to.trim().to_string();
                            }
                        }
                        done = true;
                    }
                });
                if done {
                    st.renaming = None;
                }
            }
        });
        // The preview and description of the chosen style.
        ui.vertical(|ui| {
            let shown = if st.selected == OVERRIDES {
                d.dim_style(current).cloned().unwrap_or_default().with_overrides(overrides)
            } else {
                d.dim_style(&st.selected).cloned().unwrap_or_default()
            };
            let label = if st.selected == OVERRIDES {
                crate::tf!("Preview of: {style}", style = format!("{current} {OVERRIDES}"))
            } else {
                crate::tf!("Preview of: {style}", style = st.selected)
            };
            ui.label(label);
            preview(ui, d, &shown, vec2(340.0, 250.0));
            ui.add_space(6.0);
            ui.label(crate::tl!("Description"));
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(330.0);
                ui.label(description(d, &st.selected, current, overrides));
            });
        });
        // The buttons.
        ui.vertical(|ui| {
            let w = vec2(112.0, 24.0);
            let is_style = st.selected != OVERRIDES;
            ui.add_space(18.0);
            if ui
                .add_enabled(is_style && !st.selected.eq_ignore_ascii_case(current), egui::Button::new(crate::tl!("Set Current")).min_size(w))
                .clicked()
            {
                actions.push(("dimstyle.current", json!({ "name": st.selected })));
            }
            if ui.add(egui::Button::new(crate::tl!("New...")).min_size(w)).clicked() {
                st.start_with = if is_style { st.selected.clone() } else { current.to_string() };
                st.new_name = new_name(d, &st.start_with);
                st.new_annotative = d.dim_style(&st.start_with).is_some_and(|s| s.annotative);
                st.page = Page::New;
            }
            if ui.add_enabled(is_style, egui::Button::new(crate::tl!("Modify...")).min_size(w)).clicked()
                && let Some(s) = d.dim_style(&st.selected)
            {
                start_edit(st, s.name.clone(), Target::Modify, dimprops::style_fields(s));
            }
            if ui.add(egui::Button::new(crate::tl!("Override...")).min_size(w)).clicked()
                && let Some(s) = d.dim_style(current)
            {
                // The current settings: the style with the overrides it already has.
                start_edit(st, s.name.clone(), Target::Override, dimprops::style_fields(&s.with_overrides(overrides)));
                st.original = dimprops::style_fields(s);
            }
            if ui.add(egui::Button::new(crate::tl!("Compare...")).min_size(w)).clicked() {
                st.compare_with = current.to_string();
                st.page = Page::Compare;
            }
            if ui.add_enabled(is_style && deletable(&st.selected, current, &used), egui::Button::new(crate::tl!("Delete")).min_size(w)).clicked() {
                actions.push(("dimstyle.delete", json!({ "name": st.selected })));
            }
        });
    });
    ui.separator();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.add(egui::Button::new(crate::tl!("Close")).min_size(vec2(80.0, 24.0))).clicked() {
            *close = true;
        }
    });
}

/// Whether style `name` can be deleted: not Standard, the current style or a style in use.
fn deletable(name: &str, current: &str, used: &[String]) -> bool {
    !name.eq_ignore_ascii_case("Standard") && !name.eq_ignore_ascii_case(current) && !used.iter().any(|n| n.eq_ignore_ascii_case(name))
}

fn start_edit(st: &mut State, name: String, target: Target, values: Map<String, Value>) {
    st.editing = name;
    st.target = target;
    st.original = values.clone();
    st.values = values;
    st.page = Page::Edit;
}

/// "Copy of <style>", numbered when that is taken.
fn new_name(d: &Drawing, base: &str) -> String {
    let first = format!("{} {base}", crate::tl!("Copy of"));
    (1..1000).map(|i| if i == 1 { first.clone() } else { format!("{first} ({i})") }).find(|n| d.dim_style(n).is_none()).unwrap_or(first)
}

/// The chosen style's description: its name, or the current style and what its overrides change.
fn description(d: &Drawing, selected: &str, current: &str, overrides: &Map<String, Value>) -> String {
    if selected != OVERRIDES {
        let anno = if d.dim_style(selected).is_some_and(|s| s.annotative) { crate::tl!("Annotative").to_string() } else { String::new() };
        return [selected.to_string(), anno].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
    }
    let base = d.dim_style(current).cloned().unwrap_or_default();
    let mut parts = vec![current.to_string()];
    let values = Value::Object(dimprops::style_fields(&base.with_overrides(overrides)));
    for p in DIM_PROPS.iter().filter(|p| overrides.contains_key(p.field)) {
        parts.push(format!("{} = {}", crate::i18n::t(p.label), shown(p, &values)));
    }
    parts.join(" + ")
}

/// `p` of a style's `values` as the dialog shows it: a choice's label (a precision as 0.00, an
/// arrowhead by name), a color's name, Yes or No.
fn shown(p: &DimProp, values: &Value) -> String {
    let v = dim_value(p, values);
    if let Some(o) = p.kind.options().into_iter().find(|o| o.value == v) {
        return crate::i18n::t(&o.label).to_string();
    }
    if p.kind == DimPropKind::Color
        && let Ok(c) = serde_json::from_value::<cadcraft_color::Color>(v.clone())
    {
        return crate::i18n::t(&c.name()).to_string();
    }
    match v {
        Value::String(s) if s.is_empty() && p.kind == DimPropKind::Linetype => crate::tl!("ByBlock").to_string(),
        Value::String(s) => s,
        Value::Bool(b) => (if b { crate::tl!("Yes") } else { crate::tl!("No") }).to_string(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

fn new_page(ui: &mut egui::Ui, d: &Drawing, st: &mut State) {
    egui::Grid::new("dimstyle_new").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
        ui.label(crate::tl!("New Style Name:"));
        ui.add(egui::TextEdit::singleline(&mut st.new_name).desired_width(240.0));
        ui.end_row();
        ui.label(crate::tl!("Start With:"));
        craft_ui::ComboBox::from_id_salt("dimstyle_start_with").selected_text(st.start_with.clone()).width(240.0).show_ui(ui, |ui| {
            for s in &d.dim_styles {
                ui.selectable_value(&mut st.start_with, s.name.clone(), &s.name);
            }
        });
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut st.new_annotative, crate::tl!("Annotative"));
        ui.end_row();
        ui.label(crate::tl!("Use for:"));
        ui.label(crate::tl!("All dimensions"));
        ui.end_row();
    });
    let name = st.new_name.trim().to_string();
    let taken = d.dim_style(&name).is_some();
    if taken {
        ui.label(RichText::new(crate::tl!("A dimension style with this name already exists.")).color(Tokens::get().text_dim));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.add_enabled(!name.is_empty() && !taken, egui::Button::new(crate::tl!("Continue")).min_size(vec2(80.0, 24.0))).clicked() {
            let mut values = d.dim_style(&st.start_with).map(dimprops::style_fields).unwrap_or_default();
            values.insert("name".into(), json!(name));
            values.insert("annotative".into(), json!(st.new_annotative));
            start_edit(st, name, Target::Create, values);
        }
        if ui.add(egui::Button::new(crate::tl!("Cancel")).min_size(vec2(80.0, 24.0))).clicked() {
            st.page = Page::List;
        }
    });
}

fn edit_page(ui: &mut egui::Ui, d: &Drawing, st: &mut State, actions: &mut Vec<(&'static str, Value)>) {
    let t = Tokens::get();
    // The tabs, as a row of tab buttons over the page.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for i in 0..DIM_TABS.len() {
            let on = st.tab == i;
            let text = RichText::new(tab_label(i));
            let b = egui::Button::new(if on { text.strong() } else { text })
                .fill(if on { t.tab_active } else { t.panel })
                .corner_radius(egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 })
                .selected(on);
            if ui.add(b).clicked() {
                st.tab = i;
            }
        }
    });
    ui.separator();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(410.0);
            egui::ScrollArea::vertical().id_salt(("dimstyle_tab", st.tab)).max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                fields(ui, d, st);
            });
        });
        ui.vertical(|ui| {
            preview(ui, d, &from_json(&st.values), vec2(300.0, 240.0));
        });
    });
    ui.separator();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let w = vec2(80.0, 24.0);
        if ui.add(egui::Button::new(crate::tl!("Cancel")).min_size(w)).clicked() {
            st.page = Page::List;
        }
        if ui.add(egui::Button::new(crate::tl!("OK")).min_size(w)).clicked() {
            let changed: Map<String, Value> = st
                .values
                .iter()
                .filter(|(k, v)| k.as_str() != "name" && st.original.get(k.as_str()) != Some(*v))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            match st.target {
                Target::Modify if !changed.is_empty() => {
                    let mut p = changed;
                    p.insert("name".into(), json!(st.editing));
                    p.insert("current".into(), json!(false));
                    actions.push(("dimstyle", Value::Object(p)));
                }
                // What differs from the style is the whole override set: a value set back to the
                // style's drops its override.
                Target::Override if !changed.is_empty() || !d.dim_overrides().is_empty() => {
                    let mut p: Map<String, Value> = changed.into_iter().filter(|(k, _)| k != "annotative").collect();
                    p.insert("clear".into(), json!(true));
                    actions.push(("dimstyle.overrides", Value::Object(p)));
                }
                Target::Create => {
                    // Every variable, so the new style is what was shown whatever the current style is.
                    let mut p: Map<String, Value> =
                        st.values.iter().filter(|(k, _)| k.as_str() != "name").map(|(k, v)| (k.clone(), v.clone())).collect();
                    p.insert("name".into(), json!(st.editing));
                    p.insert("current".into(), json!(false));
                    actions.push(("dimstyle", Value::Object(p)));
                }
                _ => {}
            }
            st.selected = if st.target == Target::Override { OVERRIDES.into() } else { st.editing.clone() };
            st.page = Page::List;
        }
    });
}

/// Whether variable `field` can be edited with the settings `v` (AutoCAD greys the ones that
/// don't apply).
fn applies(field: &str, v: &Map<String, Value>) -> bool {
    let on = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    let tol = on("tolerance") || on("limits");
    match field {
        "fixedExtLength" => on("fixedExt"),
        "textFillColor" => v.get("textFill").and_then(Value::as_u64) == Some(2),
        "scale" => !on("annotative"),
        "altUnit" | "altDecimals" | "altFactor" | "altRound" | "altPost" | "altZeroSuppression" => on("alt"),
        "tolDecimals" | "tolPlus" | "tolMinus" | "tolJust" | "tolZeroSuppression" => tol,
        "altTolDecimals" | "altTolZeroSuppression" => tol && on("alt"),
        _ => true,
    }
}

/// The fields of the chosen tab, a frame per group.
fn fields(ui: &mut egui::Ui, d: &Drawing, st: &mut State) {
    let tab = st.tab;
    let mut groups: Vec<&'static str> = Vec::new();
    for p in dimprops::tab_props(tab) {
        if !groups.contains(&p.group) {
            groups.push(p.group);
        }
    }
    for group in groups {
        ui.label(RichText::new(crate::i18n::t(group)).strong());
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(390.0);
            egui::Grid::new(("dimstyle_fields", tab, group)).num_columns(2).spacing(vec2(10.0, 5.0)).min_col_width(170.0).show(ui, |ui| {
                // The Fit tab's scale: Annotative first, as in AutoCAD.
                if tab == 3 && group == "Scale for dimension features" {
                    let mut a = st.values.get("annotative").and_then(Value::as_bool).unwrap_or(false);
                    ui.label(crate::tl!("Annotative"));
                    if ui.checkbox(&mut a, "").changed() {
                        st.values.insert("annotative".into(), json!(a));
                    }
                    ui.end_row();
                }
                for p in dimprops::tab_props(tab).filter(|p| p.group == group) {
                    let enabled = applies(p.field, &st.values);
                    ui.add_enabled_ui(enabled, |ui| ui.label(crate::i18n::t(p.label)));
                    ui.add_enabled_ui(enabled, |ui| field(ui, d, st, p));
                    ui.end_row();
                }
            });
        });
        ui.add_space(4.0);
    }
}

/// One variable's control; a change goes into the edited values.
fn field(ui: &mut egui::Ui, d: &Drawing, st: &mut State, p: &DimProp) {
    let id = egui::Id::unique(("dimstyle_field", &st.editing, p.field));
    let (lu, lp) = (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4));
    // A per-end arrowhead left empty shows the one DIMBLK draws.
    let cur = if p.kind == DimPropKind::Arrow {
        dim_value(p, &Value::Object(st.values.clone()))
    } else {
        st.values.get(p.field).cloned().unwrap_or(Value::Null)
    };
    let new = match p.kind {
        DimPropKind::Post => {
            let template = cur.as_str().unwrap_or_default().to_string();
            let (pre, suf) = split_post(&template);
            let (mut pre2, mut suf2) = (pre.clone(), suf.clone());
            ui.horizontal(|ui| {
                ui.label(crate::tl!("Prefix:"));
                ui.add(egui::TextEdit::singleline(&mut pre2).desired_width(56.0));
                ui.label(crate::tl!("Suffix:"));
                ui.add(egui::TextEdit::singleline(&mut suf2).desired_width(56.0));
            });
            (pre2 != pre || suf2 != suf).then(|| json!(join_post(&pre2, &suf2)))
        }
        DimPropKind::Zeros { leading, trailing } => {
            let bits = cur.as_u64().unwrap_or(0);
            let mut b = bits;
            ui.horizontal(|ui| {
                let (mut l, mut t) = (b & leading != 0, b & trailing != 0);
                if ui.checkbox(&mut l, crate::tl!("Leading")).changed() {
                    b ^= leading;
                }
                if ui.checkbox(&mut t, crate::tl!("Trailing")).changed() {
                    b ^= trailing;
                }
            });
            (b != bits).then(|| json!(b))
        }
        DimPropKind::Distance | DimPropKind::Number | DimPropKind::Angle => {
            // A fixed-height text style decides the text height (as in AutoCAD the field is off).
            let fixed = st.values.get("textStyle").and_then(Value::as_str).and_then(|n| d.text_style(n)).filter(|ts| ts.height > 0.0);
            if p.field == "textHeight"
                && let Some(ts) = fixed
            {
                ui.add_enabled(false, egui::Label::new(cadcraft_engine::units::format_distance(ts.height, lu, lp))).on_disabled_hover_text(
                    crate::tf!(
                        "Set by text style \"{style}\", which has a fixed height. Give the style a height of 0 to use the dimension text height.",
                        style = ts.name
                    ),
                );
                None
            } else {
                let v = cur.as_f64().unwrap_or(0.0);
                let text = if p.kind == DimPropKind::Distance { cadcraft_engine::units::format_distance(v, lu, lp) } else { super::fmt_num(v) };
                expr_field(ui, id, &text, None).and_then(|t| cadcraft_engine::units::parse_distance(&t)).filter(|f| f.is_finite()).map(|f| json!(f))
            }
        }
        DimPropKind::Bool => {
            let mut b = cur.as_bool().unwrap_or(false);
            ui.checkbox(&mut b, "").changed().then(|| json!(b))
        }
        DimPropKind::Color => {
            let c = serde_json::from_value::<cadcraft_color::Color>(cur.clone()).unwrap_or(cadcraft_color::Color::ByBlock);
            let mut out = None;
            craft_ui::ComboBox::from_id_salt(id).selected_text(crate::i18n::t(&c.name())).width(170.0).show_ui(ui, |ui| {
                for aci in [0, 256, 1, 2, 3, 4, 5, 6, 7] {
                    let option = cadcraft_color::Color::from_aci(aci);
                    if ui.selectable_label(c == option, crate::i18n::t(&option.name())).clicked() {
                        out = serde_json::to_value(option).ok();
                    }
                }
            });
            out
        }
        DimPropKind::TextStyle => combo(ui, id, &cur, d.text_styles.iter().map(|s| (json!(s.name), s.name.clone())).collect()),
        DimPropKind::Linetype => {
            let mut names = vec![(json!(""), crate::tl!("ByBlock").to_string()), (json!("ByLayer"), crate::tl!("ByLayer").to_string())];
            names.extend(
                d.linetypes
                    .iter()
                    .filter(|l| !l.name.eq_ignore_ascii_case("ByLayer") && !l.name.eq_ignore_ascii_case("ByBlock"))
                    .map(|l| (json!(l.name), l.name.clone())),
            );
            combo(ui, id, &cur, names)
        }
        _ => {
            let options = p.kind.options().into_iter().map(|o| (o.value, crate::i18n::t(&o.label).to_string())).collect();
            combo(ui, id, &cur, options)
        }
    };
    if let Some(v) = new.filter(|v| *v != cur) {
        // The first arrowhead also sets the second while they are the same, as in AutoCAD.
        if p.field == "arrowBlock1"
            && let Some(second) = dimprops::dim_prop("arrowBlock2")
            && dim_value(second, &Value::Object(st.values.clone())) == cur
        {
            st.values.insert("arrowBlock2".into(), v.clone());
        }
        st.values.insert(p.field.into(), v);
    }
}

/// A drop-down of `options` (value, label) showing `cur`'s label; the value picked.
fn combo(ui: &mut egui::Ui, id: egui::Id, cur: &Value, options: Vec<(Value, String)>) -> Option<Value> {
    let shown = options
        .iter()
        .find(|(v, _)| v == cur)
        .map_or_else(|| cur.as_str().map(str::to_string).unwrap_or_else(|| cur.to_string()), |(_, l)| l.clone());
    let mut out = None;
    craft_ui::ComboBox::from_id_salt(id).selected_text(shown).width(170.0).show_ui(ui, |ui| {
        for (v, label) in options {
            if ui.selectable_label(v == *cur, label).clicked() {
                out = Some(v);
            }
        }
    });
    out
}

fn compare_page(ui: &mut egui::Ui, d: &Drawing, st: &mut State) {
    let names: Vec<String> = d.dim_styles.iter().map(|s| s.name.clone()).collect();
    if !names.contains(&st.selected) {
        st.selected = d.header.str("DIMSTYLE", "Standard");
    }
    egui::Grid::new("dimstyle_compare_pick").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
        ui.label(crate::tl!("Compare:"));
        craft_ui::ComboBox::from_id_salt("dimstyle_compare_a").width(220.0).selected_text(st.selected.clone()).show_ui(ui, |ui| {
            for n in &names {
                ui.selectable_value(&mut st.selected, n.clone(), n);
            }
        });
        ui.end_row();
        ui.label(crate::tl!("With:"));
        craft_ui::ComboBox::from_id_salt("dimstyle_compare_b").width(220.0).selected_text(st.compare_with.clone()).show_ui(ui, |ui| {
            for n in &names {
                ui.selectable_value(&mut st.compare_with, n.clone(), n);
            }
        });
        ui.end_row();
    });
    let style = |n: &str| Value::Object(dimprops::style_fields(&d.dim_style(n).cloned().unwrap_or_default()));
    let (a, b) = (style(&st.selected), style(&st.compare_with));
    let same = st.selected == st.compare_with;
    let rows: Vec<&DimProp> = DIM_PROPS.iter().filter(|p| same || dim_value(p, &a) != dim_value(p, &b)).collect();
    ui.label(if same { crate::tf!("All properties of {style}", style = st.selected) } else { crate::tf!("{count} differences", count = rows.len()) });
    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
        egui::Grid::new("dimstyle_compare").num_columns(4).striped(true).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
            ui.label(RichText::new(crate::tl!("Description")).strong());
            ui.label(RichText::new(crate::tl!("Variable")).strong());
            ui.label(RichText::new(&st.selected).strong());
            if !same {
                ui.label(RichText::new(&st.compare_with).strong());
            }
            ui.end_row();
            for p in rows {
                ui.label(format!("{} ({})", crate::i18n::t(p.label), crate::i18n::t(DIM_TABS.get(p.tab).copied().unwrap_or(""))));
                ui.label(dimprops::dim_var(p.field));
                ui.label(shown(p, &a));
                if !same {
                    ui.label(shown(p, &b));
                }
                ui.end_row();
            }
        });
    });
    ui.separator();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.add(egui::Button::new(crate::tl!("Close")).min_size(vec2(80.0, 24.0))).clicked() {
            st.page = Page::List;
        }
    });
}

/// Sample dimensions drawn with `style`: a plate with a hole and a slanted edge, dimensioned
/// linear, aligned, angular, radius and diameter, sized to the style's text.
fn preview(ui: &mut egui::Ui, d: &Drawing, style: &DimStyle, size: egui::Vec2) {
    let mut hasher = std::hash::DefaultHasher::new();
    serde_json::to_string(style).unwrap_or_default().hash(&mut hasher);
    serde_json::to_string(&d.text_styles).unwrap_or_default().hash(&mut hasher);
    picture(ui, "dimstyle_preview", hasher.finish(), size, |w, h, dark| sample(d, style, w, h, dark));
}

/// A picture made by `make` (width, height, dark theme) at `size`, kept as a texture until `key`
/// changes, framed like AutoCAD's preview panes.
pub(super) fn picture(ui: &mut egui::Ui, salt: &str, key: u64, size: egui::Vec2, make: impl FnOnce(u32, u32, bool) -> Option<egui::ColorImage>) {
    let (w, h) = (size.x.max(16.0) as u32, size.y.max(16.0) as u32);
    let dark = ui.visuals().dark_mode;
    let mut hasher = std::hash::DefaultHasher::new();
    (key, w, h, dark).hash(&mut hasher);
    let key = hasher.finish();
    let id = egui::Id::unique((salt, w, h));
    let cached = ui.data(|m| m.get_temp::<(u64, egui::TextureHandle)>(id)).filter(|(k, _)| *k == key).map(|(_, t)| t);
    let tex = match cached {
        Some(t) => Some(t),
        None => make(w, h, dark).map(|img| {
            let t = ui.ctx().load_texture(salt, img, egui::TextureOptions::LINEAR);
            ui.data_mut(|m| m.insert_temp(id, (key, t.clone())));
            t
        }),
    };
    let frame = egui::Frame::new().stroke(egui::Stroke::new(1.0, Tokens::get().border));
    frame.show(ui, |ui| match tex {
        Some(t) => {
            ui.image((t.id(), size));
        }
        None => {
            ui.allocate_space(size);
        }
    });
}

/// The model space of scratch session `s` drawn as the canvas draws it, fitted to `w` × `h`.
pub(super) fn raster(s: &cadcraft_engine::Session, w: u32, h: u32, dark: bool) -> Option<egui::ColorImage> {
    let doc = s.doc().ok()?;
    let list = cadcraft_engine::render::build(doc, &cadcraft_engine::doc::Space::Model, &cadcraft_engine::render::Options::default());
    let view = cadcraft_engine::render::raster::View::fit(&list.bounds, w, h, 0.06);
    let background = if dark { cadcraft_engine::render::raster::RasterOptions::default().background } else { cadcraft_color::Rgb(255, 255, 255) };
    let pm =
        cadcraft_engine::render::raster::render(&list, &view, &cadcraft_engine::render::raster::RasterOptions { background, ..Default::default() })?;
    // An opaque background: premultiplied and straight alpha are the same.
    Some(egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data()))
}

/// The preview picture: a scratch drawing with the style current, its dimensions drawn as the
/// canvas draws them.
fn sample(d: &Drawing, style: &DimStyle, w: u32, h: u32, dark: bool) -> Option<egui::ColorImage> {
    let mut s = cadcraft_engine::Session::new();
    {
        let doc = s.doc_mut().ok()?;
        doc.text_styles.clone_from(&d.text_styles);
        doc.linetypes.clone_from(&d.linetypes);
        doc.dim_styles = vec![DimStyle { name: "Preview".into(), ..style.clone() }];
        doc.header.set_str("DIMSTYLE", "Preview");
        doc.sync_dim_vars();
    }
    // Sizes in text heights, so any style's text reads at preview size.
    let text = d.text_style(&style.text_style).map(|t| t.height).filter(|h| *h > 0.0).unwrap_or(style.text_height * style.effective_scale(1.0));
    let u = if text.is_finite() && text > 1e-9 && text < 1e9 { text } else { 0.18 };
    let (l, ht) = (14.0 * u, 9.0 * u);
    let run = |s: &mut cadcraft_engine::Session, cmd: &str, p: Value| s.execute(cmd, &p).ok();
    run(&mut s, "pline", json!({ "vertices": [[0, 0], [l, 0], [l, ht * 0.6], [l * 0.7, ht], [0, ht]], "closed": true }))?;
    let c = run(&mut s, "circle", json!({ "center": [l * 0.35, ht * 0.5], "radius": ht * 0.2 }))?;
    let circle = c.get("handle").or_else(|| c.get("handles").and_then(|h| h.get(0))).cloned().unwrap_or(Value::Null);
    let arc = run(&mut s, "arc", json!({ "center": [l * 0.7, ht * 0.25], "radius": ht * 0.12, "start": 0, "end": 180 }));
    run(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [l, 0], "at": [l / 2.0, -3.0 * u] }));
    run(&mut s, "dimlinear", json!({ "p1": [0, 0], "p2": [0, ht], "at": [-3.0 * u, ht / 2.0] }));
    run(&mut s, "dimaligned", json!({ "p1": [l, ht * 0.6], "p2": [l * 0.7, ht], "at": [l + 1.5 * u, ht + 1.5 * u] }));
    run(&mut s, "dimangular", json!({ "vertex": [l * 0.7, ht], "p1": [0, ht], "p2": [l, ht * 0.6], "at": [l * 0.55, ht * 0.8] }));
    run(&mut s, "dimdiameter", json!({ "handle": circle, "at": [l * 0.55, ht * 0.22] }));
    if let Some(a) = arc.and_then(|a| a.get("handle").cloned()) {
        run(&mut s, "dimradius", json!({ "handle": a, "at": [l * 0.85, ht * 0.45] }));
    }
    raster(&s, w, h, dark)
}
