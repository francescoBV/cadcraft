//! The AutoSnap magnet (AUTOSNAP bit 2, on by default as in AutoCAD): with the real pointer near
//! a line's end, the Endpoint marker is drawn there and the crosshair locks onto it; a click takes
//! the snapped point. With the magnet off the marker stays and the crosshair follows the pointer.
//! Profiles saved before the bit could be kept leave the magnet on.

use cadcraft_engine::doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::snap::mode;
use cadcraft_engine::{Session, sysvars};
use cadcraft_ui_egui::theme::Tokens;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, Shape, vec2};
use egui_kittest::Harness;
use serde_json::json;

const END: Vec2 = Vec2 { x: -2.0, y: -8.0 };

fn harness(scale: f32) -> Harness<'static, CadApp> {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(-10.0, -8.0, 0.0), b: END.to3(0.0) })).unwrap();
    let mut session = Session::empty();
    session.open_drawing(d, "Magnet", None);
    session.execute("view.set", &json!({"center": [-5, -5], "height": 30})).unwrap();
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_pixels_per_point(scale).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn screen(h: &Harness<'_, CadApp>, w: Vec2) -> Pos2 {
    h.state().canvas.xf.expect("drawing area laid out").to_screen(w)
}

fn hover(h: &mut Harness<'_, CadApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.run_steps(3);
}

fn all_shapes(h: &Harness<'_, CadApp>) -> Vec<Shape> {
    fn flatten(s: &Shape, out: &mut Vec<Shape>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
            s => out.push(s.clone()),
        }
    }
    let mut out = Vec::new();
    for c in &h.output().shapes {
        flatten(&c.shape, &mut out);
    }
    out
}

/// The crosshair's horizontal arms meet at `at` (crosshair colour, at its height, either side).
fn crosshair_at(h: &Harness<'_, CadApp>, at: Pos2) -> bool {
    let color = Tokens::get().crosshair;
    let arms: Vec<[Pos2; 2]> = all_shapes(h)
        .iter()
        .filter_map(|s| match s {
            Shape::LineSegment { points, stroke } if stroke.color == color && (points[0].y - points[1].y).abs() < 0.01 => Some(*points),
            _ => None,
        })
        .filter(|p| (p[0].y - at.y).abs() < 0.5)
        .collect();
    arms.iter().any(|p| p[1].x <= at.x + 0.5 && p[1].x >= at.x - 12.0) && arms.iter().any(|p| p[0].x >= at.x - 0.5 && p[0].x <= at.x + 12.0)
}

fn magnet_with_the_pointer(scale: f32) {
    let mut h = harness(scale);
    h.state_mut().start("line");
    h.run_steps(2);
    assert_eq!(sysvars::get(&h.state().session, "AUTOSNAP").unwrap().as_u64().map(|v| v & 2), Some(2), "the magnet is on by default");
    // A few points off the line's end.
    let pos = screen(&h, END) + vec2(3.0, 2.0);
    hover(&mut h, pos);
    let hit = h.state().canvas.snap.unwrap_or_else(|| panic!("no snap at the line's end at {scale}x"));
    assert_eq!(hit.mode, mode::END, "at {scale}x");
    let at = screen(&h, END);
    assert!(crosshair_at(&h, at), "the magnet puts the crosshair on the snap point ({scale}x)");
    assert!(!crosshair_at(&h, pos), "not at the pointer ({scale}x)");
    // The click takes the snapped point.
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    let base = h.state().session.current_prompt().and_then(|p| p.base).expect("LINE asks for the next point");
    assert!(base.near(END, 1e-9), "{base:?}");
    // Magnet off (AUTOSNAP without bit 2): the snap is still found, the crosshair follows the pointer.
    h.state_mut().run("setvar", json!({"name": "AUTOSNAP", "value": 61})).unwrap();
    let pos = pos + vec2(1.0, 0.0);
    hover(&mut h, pos);
    assert_eq!(h.state().canvas.snap.map(|s| s.mode), Some(mode::END));
    assert!(crosshair_at(&h, pos), "no magnet: the crosshair stays at the pointer ({scale}x)");
    assert!(!crosshair_at(&h, at), "no magnet: not on the snap point ({scale}x)");
}

#[test]
fn magnet_locks_the_crosshair_at_100_percent() {
    magnet_with_the_pointer(1.0);
}

#[test]
fn magnet_locks_the_crosshair_at_150_percent() {
    magnet_with_the_pointer(1.5);
}

#[test]
fn a_profile_saved_before_the_magnet_bit_keeps_the_magnet_on() {
    let mut a = CadApp::new(Session::default(), Services::default());
    // What earlier versions saved: AUTOSNAP 61 (no magnet bit: it could not be set then).
    a.load_profile(r#"{"version":1,"sysvars":{"AUTOSNAP":61,"OSMODE":20517}}"#);
    assert!(a.session.settings.autosnap_magnet);
    assert_eq!(a.session.settings.osmode, 20517, "OSNAP off stays off");
    // A profile saved now keeps the choice.
    a.run("setvar", json!({"name": "AUTOSNAP", "value": 61})).unwrap();
    let saved = a.profile_json();
    let mut b = CadApp::new(Session::default(), Services::default());
    b.load_profile(&saved);
    assert!(!b.session.settings.autosnap_magnet);
    assert_eq!(sysvars::get(&b.session, "AUTOSNAP").unwrap(), json!(61));
}
