//! Real pointer and key gestures on the canvas: a press-and-drag in an empty area draws a lasso
//! (window to the right, crossing to the left, Space cycling to a fence), LASSOMODE 0 brings the
//! rectangle back, and the Options ▸ Selection checkbox sets LASSOMODE.
//!
//! Screenshots: set `CADCRAFT_LASSO_SHOTS` to a directory.

use cadcraft_doc::Handle;
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;

/// A short line (inside the lasso), a long one (crossing it) and a circle (outside).
fn harness() -> (Harness<'static, CadApp>, [Handle; 3]) {
    let mut s = Session::new();
    let hx = |v: &serde_json::Value| Handle::parse_hex(v.as_str().unwrap()).unwrap();
    let a = hx(&s.execute("line", &json!({ "points": [[1, 1], [2, 1]] })).unwrap()["handles"][0]);
    let b = hx(&s.execute("line", &json!({ "points": [[1, 3], [20, 3]] })).unwrap()["handles"][0]);
    let c = hx(&s.execute("circle", &json!({ "center": [-8, -8], "radius": 1 })).unwrap()["handle"]);
    s.execute("view.set", &json!({ "center": [3, 2], "height": 20 })).unwrap();
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

fn button(h: &mut Harness<'_, CadApp>, p: Pos2, pressed: bool) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

fn shot(h: &mut Harness<'_, CadApp>, name: &str) {
    if let Some(dir) = std::env::var_os("CADCRAFT_LASSO_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join(name)).unwrap();
    }
}

/// Drag a loop round world (0, 0)–(5, 5), starting at (0, 0) and going right (or left when
/// `mirror`), pressing Space `spaces` times midway; returns once the button is up.
fn lasso(h: &mut Harness<'_, CadApp>, mirror: bool, spaces: usize, shot_name: Option<&str>) {
    let xf = h.state().canvas.xf.unwrap();
    let pts: Vec<Pos2> = [(0.0, 0.0), (2.5, -0.6), (5.0, -0.5), (5.6, 2.5), (5.5, 5.0), (2.5, 5.6), (0.0, 5.5), (-0.6, 2.5), (-0.4, 0.3)]
        .iter()
        .map(|&(x, y)| xf.to_screen(Vec2::new(if mirror { 5.0 - x } else { x }, y)))
        .collect();
    button(h, pts[0], true);
    for (i, w) in pts.windows(2).enumerate() {
        for k in 1..=6 {
            h.event(Event::PointerMoved(w[0] + (w[1] - w[0]) * (k as f32 / 6.0)));
            h.run_steps(1);
        }
        if i == 3 {
            for _ in 0..spaces {
                h.event(Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
                h.event(Event::Text(" ".into()));
                h.run_steps(1);
                h.event(Event::Key { key: Key::Space, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
                h.run_steps(1);
            }
            if let Some(name) = shot_name {
                shot(h, name);
            }
        }
    }
    let last = *pts.last().unwrap();
    button(h, last, false);
    h.run_steps(2);
}

#[test]
fn lasso_window_crossing_and_fence() {
    let (mut h, [inside, crossing, outside]) = harness();
    // Right first: a window lasso takes what lies inside.
    lasso(&mut h, false, 0, Some("lasso-window.png"));
    assert_eq!(h.state().session.selection(), vec![inside]);
    h.state_mut().session.set_selection(Vec::new());
    // Left first: a crossing lasso also takes what it crosses.
    lasso(&mut h, true, 0, Some("lasso-crossing.png"));
    let mut sel = h.state().session.selection();
    sel.sort();
    let mut want = vec![inside, crossing];
    want.sort();
    assert_eq!(sel, want);
    assert!(!sel.contains(&outside));
    h.state_mut().session.set_selection(Vec::new());
    // Right first, then Space twice: window → crossing → fence; only the long line is crossed.
    lasso(&mut h, false, 2, Some("lasso-fence.png"));
    assert_eq!(h.state().session.selection(), vec![crossing]);
    // The Space presses never reached the command line as Enter (no command repeated).
    assert!(h.state().session.running.is_none());
}

#[test]
fn lasso_at_select_objects_and_lassomode_off() {
    let (mut h, [inside, crossing, _]) = harness();
    h.state_mut().session.start("erase").unwrap();
    h.run_steps(2);
    lasso(&mut h, false, 0, None);
    assert!(h.state().session.running.is_some(), "still selecting");
    h.state_mut().session.input(cadcraft_engine::Input::Enter).unwrap();
    assert!(h.state().session.doc().unwrap().entity(inside).is_none());
    assert!(h.state().session.doc().unwrap().entity(crossing).is_some());
    // LASSOMODE 0: the same drag is a rectangular window from where it started to where it ends.
    h.state_mut().run("setvar", json!({ "name": "LASSOMODE", "value": 0 })).unwrap();
    h.state_mut().session.set_selection(Vec::new());
    lasso(&mut h, false, 0, None);
    assert!(h.state().session.selection().is_empty(), "a window from (0, 0) to (-0.4, 0.3) holds nothing");
}

#[test]
fn selection_tab_checkbox_sets_lassomode() {
    let (mut h, _) = harness();
    h.state_mut().ui.dialog = Some("options:selection".into());
    h.run_steps(3);
    shot(&mut h, "lasso-options.png");
    h.get_by_label("Press and drag draws a lasso").click();
    h.run_steps(2);
    assert!(!h.state().session.settings.lassomode);
    h.get_by_label("Press and drag draws a lasso").click();
    h.run_steps(2);
    assert!(h.state().session.settings.lassomode);
}
