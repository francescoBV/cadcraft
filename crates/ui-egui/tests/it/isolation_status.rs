//! The status bar's Isolate Objects item: lit while objects are isolated or hidden (for the
//! session or saved with the drawing, OBJECTISOLATIONMODE 1); its menu isolates or hides the
//! selection and ends the isolation. Options ▸ Selection sets OBJECTISOLATIONMODE.
//!
//! Screenshots: set `CADCRAFT_ISOLATION_SHOTS` to a directory.

use cadcraft_doc::Handle;
use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;

/// Two lines and a circle.
fn harness() -> (Harness<'static, CadApp>, [Handle; 3]) {
    let mut s = Session::new();
    let hx = |v: &serde_json::Value| Handle::parse_hex(v.as_str().unwrap()).unwrap();
    let a = hx(&s.execute("line", &json!({ "points": [[1, 1], [2, 1]] })).unwrap()["handles"][0]);
    let b = hx(&s.execute("line", &json!({ "points": [[1, 3], [20, 3]] })).unwrap()["handles"][0]);
    let c = hx(&s.execute("circle", &json!({ "center": [-8, -8], "radius": 1 })).unwrap()["handle"]);
    s.set_selection(Vec::new());
    let mut app = CadApp::new(s, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    (h, [a, b, c])
}

fn shot(h: &mut Harness<'_, CadApp>, name: &str) {
    if let Some(dir) = std::env::var_os("CADCRAFT_ISOLATION_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join(name)).unwrap();
    }
}

fn visible(h: &Harness<'_, CadApp>, e: Handle) -> bool {
    h.state().session.doc().unwrap().entity(e).unwrap().common.visible
}

#[test]
fn status_bar_item_shows_saved_isolation_and_ends_it() {
    let (mut h, [a, b, _]) = harness();
    h.state_mut().run("setvar", json!({ "name": "OBJECTISOLATIONMODE", "value": 1 })).unwrap();
    h.run_steps(2);
    let _ = h.get_by_label("Isolate Objects");
    h.state_mut().run("isolateobjects", json!({ "handles": [a.hex()] })).unwrap();
    h.run_steps(2);
    shot(&mut h, "isolate-on.png");
    assert!(!visible(&h, b));
    // The item is lit; its menu ends the isolation.
    h.get_by_label("Objects are isolated or hidden").click();
    h.run_steps(2);
    shot(&mut h, "isolate-menu.png");
    h.get_by_label("End Object Isolation").click();
    h.run_steps(3);
    assert!(visible(&h, b));
    let _ = h.get_by_label("Isolate Objects");
}

#[test]
fn status_bar_menu_hides_the_selection_for_the_session() {
    let (mut h, [a, b, c]) = harness();
    h.state_mut().session.set_selection(vec![b]);
    h.run_steps(2);
    h.get_by_label("Isolate Objects").click();
    h.run_steps(2);
    h.get_by_label("Hide Objects").click();
    h.run_steps(3);
    let d = h.state().session.doc().unwrap();
    assert!(d.is_hidden_object(b) && !d.is_hidden_object(a) && !d.is_hidden_object(c));
    assert!(visible(&h, b), "OBJECTISOLATIONMODE 0: hidden for the session only");
    let _ = h.get_by_label("Objects are isolated or hidden");
    // Undo shows it again, and the item goes dark.
    h.state_mut().run("undo", json!({})).unwrap();
    h.run_steps(2);
    let _ = h.get_by_label("Isolate Objects");
}

#[test]
fn selection_tab_checkbox_sets_objectisolationmode() {
    let (mut h, _) = harness();
    h.state_mut().ui.dialog = Some("options:selection".into());
    h.run_steps(3);
    h.get_by_label("Keep isolated objects hidden when the drawing is saved").click();
    h.run_steps(2);
    assert!(h.state().session.settings.objectisolationmode);
    let saved = h.state().profile_json();
    let mut restored = CadApp::new(Session::new(), Services::default());
    restored.load_profile(&saved);
    assert!(restored.session.settings.objectisolationmode);
}
