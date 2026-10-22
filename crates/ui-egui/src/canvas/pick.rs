use super::*;

/// The vertical wheel scroll this frame that zooms, in points, read from the raw wheel events so
/// each notch zooms by its whole step at once, as in standard CAD (#660). egui's smoothed delta
/// spreads a notch over several frames, so quick notches lag behind the wheel. Units convert to
/// points as egui does (a line is `line_scroll_speed` points, a page the view's height), and
/// events egui turns into a pinch zoom (⌘/Ctrl) or a sideways scroll (⇧) are left out, as it does.
pub(super) fn wheel_zoom_points(events: &[egui::Event], opts: &egui::InputOptions, page: f32) -> f32 {
    let points: f32 = events
        .iter()
        .filter_map(|e| match e {
            egui::Event::MouseWheel { unit, delta, modifiers, .. }
                if !modifiers.matches_any(opts.zoom_modifier) && !modifiers.matches_any(opts.horizontal_scroll_modifier) =>
            {
                let per = match unit {
                    egui::MouseWheelUnit::Point => 1.0,
                    egui::MouseWheelUnit::Line => opts.line_scroll_speed,
                    egui::MouseWheelUnit::Page => page,
                };
                Some(delta.y * per)
            }
            _ => None,
        })
        .sum();
    if points.is_finite() { points } else { 0.0 }
}

/// The model space tiles on screen (none without tiling): each tile's rectangle in `full`.
/// Start TEXTEDIT on the text, mtext or attribute definition at `p` (the objects its shortcut menu
/// offers Edit Text for); false when there is none.
pub(super) fn edit_text_at(app: &mut CadApp, p: Vec2) -> bool {
    use cadcraft_engine::doc::EntityKind;
    let is_text = cadcraft_engine::cmd::curves::pick_at(&app.session, p).and_then(|h| {
        app.session.doc().ok()?.entity(h).map(|e| matches!(e.kind, EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::AttDef(_)))
    });
    if is_text != Some(true) {
        return false;
    }
    app.start("textedit");
    if let Err(e) = app.session.input(Input::Point(p)) {
        app.session.echo(e.to_string());
    }
    true
}

/// The hatch under world point `w` within the pick aperture (PICKBOX × 1.5 pixels at `scale`
/// pixels per unit), if any.
pub(super) fn hatch_at(app: &CadApp, w: Vec2, scale: f64) -> Option<Handle> {
    let st = app.session.state().ok()?;
    let aperture = app.session.settings.pickbox.max(1.0) * 1.5 / scale.max(1e-12);
    let h = cadcraft_engine::select::pick(&st.doc, &st.edit_space(), w, aperture)?;
    matches!(st.doc.entity(h)?.kind, cadcraft_doc::EntityKind::Hatch(_)).then_some(h)
}

#[cfg(test)]
mod hatch_pick_tests {
    use cadcraft_engine::Session;
    use cadcraft_geom::Vec2;
    use serde_json::json;

    use crate::{CadApp, Services};

    #[test]
    fn hatch_at_finds_a_hatch_under_the_cursor_only() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.session.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap()["handle"].as_str().unwrap().to_string();
        app.session.execute("line", &json!({ "points": [[20, 0], [30, 0]] })).unwrap();
        assert_eq!(super::hatch_at(&app, Vec2::new(5.0, 5.0), 10.0).map(|h| h.hex()), Some(hx));
        assert_eq!(super::hatch_at(&app, Vec2::new(25.0, 0.0), 10.0), None, "a line is not a hatch");
        assert_eq!(super::hatch_at(&app, Vec2::new(60.0, 60.0), 10.0), None);
    }

    #[test]
    fn double_clicking_a_hatch_opens_the_hatch_edit_dialog() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.session.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap()["handle"].as_str().unwrap().to_string();
        let _ = app.session.zoom_extents();
        let ctx = egui::Context::default();
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0)));
        let frame = |app: &mut CadApp, events: Vec<egui::Event>, time: f64| {
            app.logic(&ctx);
            let input = egui::RawInput { screen_rect: screen, events, time: Some(time), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| app.ui(ui));
            out.textures_delta.clear();
        };
        frame(&mut app, Vec::new(), 0.0);
        let at = app.canvas.xf.unwrap().to_screen(Vec2::new(5.0, 5.0));
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        frame(&mut app, vec![egui::Event::PointerMoved(at)], 0.1);
        frame(&mut app, vec![button(true), button(false)], 0.2);
        frame(&mut app, vec![button(true), button(false)], 0.3);
        frame(&mut app, Vec::new(), 0.4);
        assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
        assert_eq!(app.hatch_dlg.as_ref().map(|s| s.edit.iter().map(|h| h.hex()).collect::<Vec<_>>()), Some(vec![hx]));
        // The double-click (at the hatch's centre grip) only opens the dialog: no grip edit.
        assert!(app.session.running.is_none(), "{:?}", app.session.running.as_ref().map(|r| r.machine.name()));
    }
}

/// #44: every segment LINE and PLINE add appears on the canvas as it is drawn, not only the first
/// one until the command ends.
#[cfg(test)]
mod mid_command_redraw_tests {
    use std::collections::BTreeSet;

    use cadcraft_engine::Session;

    use crate::{CadApp, Services};

    fn frame(app: &mut CadApp, ctx: &egui::Context, time: f64) {
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        app.logic(ctx);
        let input = egui::RawInput { screen_rect: screen, time: Some(time), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| app.ui(ui));
        out.textures_delta.clear();
    }

    /// Handles of the model's entities, and those the canvas's display list draws.
    fn drawn(app: &CadApp) -> (BTreeSet<u64>, BTreeSet<u64>) {
        let doc = app.session.doc().unwrap();
        let model = doc.model.iter().map(|e| e.handle.0).collect();
        let list = app.canvas.list.as_ref().map(|l| l.prims.iter().map(|p| p.handle.0).collect()).unwrap_or_default();
        (model, list)
    }

    #[test]
    fn successive_line_segments_are_drawn_while_line_runs() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0);
        app.session.cmdline("line").unwrap();
        for (i, p) in ["0,0", "10,0", "10,10", "20,10", "20,20"].iter().enumerate() {
            app.session.cmdline(p).unwrap();
            frame(&mut app, &ctx, 0.1 * (i + 1) as f64);
            let (model, list) = drawn(&app);
            assert_eq!(model.len(), i, "LINE adds a segment per point after the first");
            assert!(model.is_subset(&list), "after point {}: drawing {model:?}, canvas {list:?}", i + 1);
        }
        assert!(app.session.running.is_some(), "still in LINE");
    }

    #[test]
    fn successive_pline_segments_are_drawn_while_pline_runs() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0);
        app.session.cmdline("pline").unwrap();
        let mut last = 0;
        for (i, p) in ["0,0", "10,0", "10,10", "20,10", "20,20"].iter().enumerate() {
            app.session.cmdline(p).unwrap();
            frame(&mut app, &ctx, 0.1 * (i + 1) as f64);
            let (model, list) = drawn(&app);
            assert!(model.is_subset(&list), "after point {}: drawing {model:?}, canvas {list:?}", i + 1);
            // The polyline grows in place: the canvas draws its new vertex each time.
            let verts = app.canvas.list.as_ref().map_or(0, |l| l.verts.len());
            if i > 0 {
                assert!(verts > last, "after point {}: {verts} vertices drawn, {last} before", i + 1);
            }
            last = verts;
        }
        assert!(app.session.running.is_some(), "still in PLINE");
    }
}
