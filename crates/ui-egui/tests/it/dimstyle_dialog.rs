//! The Dimension Style Manager (gap F7): DIMSTYLE typed or from the menus opens it, a JSON call
//! runs the command without it. Set Current, New
//! (Continue, then OK on the Modify tabs), Modify with a change on a tab, Override and Compare act
//! on the drawing through the commands; Cancel changes nothing.

use cadcraft_engine::Session;
use cadcraft_engine::doc::DimStyle;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus, theme};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{By, NodeT, Queryable};
use serde_json::json;

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1100.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(3);
    h
}

/// A drawing with a second style, Arch, and the dialog open.
fn opened() -> CadApp {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("dimstyle", json!({ "name": "Arch", "current": false, "dimtxt": 0.25 })).unwrap();
    app.cmdline("dimstyle");
    app
}

fn style(app: &CadApp, name: &str) -> Option<DimStyle> {
    app.session.doc().ok()?.dim_style(name).cloned()
}

fn click(h: &mut Harness<'static, CadApp>, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(3);
}

/// Open the drop-down showing `shown` and pick `option`.
fn choose(h: &mut Harness<'static, CadApp>, shown: &str, option: &str) {
    h.get(By::new().role(Role::ComboBox).value(shown)).click();
    h.run_steps(3);
    click(h, option);
}

#[test]
fn typed_menu_and_ribbon_open_the_manager_but_json_does_not() {
    let mut app = CadApp::new(Session::new(), Services::default());
    for typed in ["dimstyle", "DIMSTYLE", "D", "ddim", "dst"] {
        app.ui.dialog = None;
        assert!(app.cmdline(typed).is_none(), "bare {typed} is not an error");
        assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"), "{typed}");
    }
    // The Format and Dimension menu items.
    for id in ["dimstyle.dimension", "dimstyle"] {
        app.ui.dialog = None;
        menus::activate(&mut app, id);
        assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"), "{id}");
    }
    app.ui.dialog = None;
    app.run("dimstyle", json!({ "name": "Shop", "dimtfill": 1, "current": false })).unwrap();
    assert_eq!(app.ui.dialog, None, "a JSON call opens nothing");
    assert_eq!(style(&app, "Shop").map(|s| s.text_fill), Some(1));
}

#[test]
fn set_current_new_and_compare() {
    let mut h = harness(opened());
    for label in ["Set Current", "New...", "Modify...", "Override...", "Compare...", "Close"] {
        assert!(h.query_by_label(label).is_some(), "{label} button");
    }
    // The current style can't be made current again.
    assert!(h.get_by_label("Set Current").accesskit_node().is_disabled());
    click(&mut h, "Arch");
    click(&mut h, "Set Current");
    assert_eq!(h.state().session.doc().unwrap().header.str("DIMSTYLE", ""), "Arch");

    // New: a copy of the chosen style, made when OK is pressed on its tabs.
    click(&mut h, "New...");
    assert!(h.query_by_label("Continue").is_some());
    click(&mut h, "Continue");
    for tab in ["Lines", "Symbols and Arrows", "Text", "Fit", "Primary Units", "Alternate Units", "Tolerances"] {
        assert!(h.query_by_label(tab).is_some(), "{tab} tab");
    }
    assert!(style(h.state(), "Copy of Arch").is_none(), "nothing until OK");
    click(&mut h, "OK");
    let copy = style(h.state(), "Copy of Arch").expect("the new style");
    assert_eq!(copy.text_height, 0.25, "it starts with Arch's settings");
    assert_eq!(h.state().session.doc().unwrap().header.str("DIMSTYLE", ""), "Arch", "New doesn't make it current");

    // Compare lists the differences.
    click(&mut h, "Compare...");
    assert!(h.query_by_label_contains("differences").is_some() || h.query_by_label_contains("All properties").is_some());
    click(&mut h, "Close");
    assert_eq!(h.state().ui.dialog.as_deref(), Some("dimstyle"), "Compare's Close goes back to the list");
    click(&mut h, "Close");
    assert_eq!(h.state().ui.dialog, None);
}

#[test]
fn modify_changes_a_setting_on_a_tab_and_cancel_drops_it() {
    let mut h = harness(opened());
    click(&mut h, "Arch");
    click(&mut h, "Modify...");
    click(&mut h, "Text");
    // The Text tab's Fill: None → Color (DIMTFILL 2); the fill color then applies.
    choose(&mut h, "None", "Color");
    click(&mut h, "Cancel");
    assert_eq!(style(h.state(), "Arch").map(|s| s.text_fill), Some(0), "Cancel changes nothing");

    click(&mut h, "Modify...");
    click(&mut h, "Text");
    choose(&mut h, "None", "Color");
    click(&mut h, "OK");
    let arch = style(h.state(), "Arch").unwrap();
    assert_eq!(arch.text_fill, 2);
    assert_eq!(arch.text_height, 0.25, "the rest is kept");
    // One undo step takes the change back.
    h.state_mut().run("undo", json!({})).unwrap();
    assert_eq!(style(h.state(), "Arch").map(|s| s.text_fill), Some(0));
}

#[test]
fn override_sets_the_current_styles_overrides() {
    let mut h = harness(opened());
    click(&mut h, "Override...");
    assert!(h.query_by_label_contains("Override Current Style").is_some());
    click(&mut h, "Text");
    choose(&mut h, "None", "Background");
    click(&mut h, "OK");
    let d = h.state().session.doc().unwrap();
    assert_eq!(d.dim_overrides().get("textFill"), Some(&json!(1)), "{:?}", d.dim_overrides());
    assert_eq!(d.dim_style("Standard").map(|s| s.text_fill), Some(0), "the style itself is unchanged");
    assert!(h.query_by_label("<style overrides>").is_some(), "the list shows the overrides");
}
