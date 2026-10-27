//! Lasso selection: pressing and dragging in an empty area (LASSOMODE 1, PICKDRAG 2) draws a
//! free-form loop instead of a rectangle. Dragged to the right first it is a window lasso
//! (objects inside it), to the left a crossing lasso (inside or crossed); Space while dragging
//! steps through window, crossing and fence (what the path crosses). Letting go selects through
//! [`cadcraft_engine::Session::lasso`].

use cadcraft_engine::select::LassoKind;
use cadcraft_geom::Vec2;
use egui::{Color32, Pos2, Shape, Stroke};

use super::cursor::tooltip;
use super::{Xf, selection_area_fill};
use crate::CadApp;
use crate::theme::Tokens;

/// A lasso being dragged.
#[derive(Clone, Debug, Default)]
pub struct Lasso {
    /// The path in world coordinates, a point every few pixels.
    pub pts: Vec<Vec2>,
    /// Where the button went down and the last point kept, on screen.
    origin: Pos2,
    last: Pos2,
    /// Window or crossing once the drag has gone sideways; what Space chose after that.
    pub kind: Option<LassoKind>,
}

/// Pixels the pointer moves before the path takes another point.
const STEP_PX: f32 = 3.0;
/// Pixels sideways that decide window (right) or crossing (left).
const SIDEWAYS_PX: f32 = 4.0;

impl Lasso {
    pub fn new(origin: Pos2, world: Vec2) -> Lasso {
        Lasso { pts: vec![world], origin, last: origin, kind: None }
    }

    /// Follow the pointer to `at` (world point `w`).
    pub fn extend(&mut self, at: Pos2, w: Vec2) {
        if self.kind.is_none() && (at.x - self.origin.x).abs() >= SIDEWAYS_PX {
            self.kind = Some(if at.x > self.origin.x { LassoKind::Window } else { LassoKind::Crossing });
        }
        if at.distance(self.last) >= STEP_PX && w.is_finite() && self.pts.len() < cadcraft_engine::select::MAX_LASSO_POINTS {
            self.pts.push(w);
            self.last = at;
        }
    }

    /// The kind the lasso selects with now (a window until the drag has gone sideways).
    pub fn kind(&self) -> LassoKind {
        self.kind.unwrap_or(LassoKind::Window)
    }

    /// Space: the next kind.
    pub fn cycle(&mut self) {
        self.kind = Some(self.kind().next());
    }
}

/// Space pressed while a lasso is dragged steps its kind, and is used up (it is not Enter).
pub fn space(app: &mut CadApp, ui: &egui::Ui) {
    let Some(l) = app.canvas.lasso.as_mut() else { return };
    let presses =
        ui.input(|i| i.events.iter().filter(|e| matches!(e, egui::Event::Key { key: egui::Key::Space, pressed: true, repeat: false, .. })).count());
    for _ in 0..presses {
        l.cycle();
    }
    ui.ctx().input_mut(|i| {
        i.events.retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::Space, .. }) && !matches!(e, egui::Event::Text(t) if t == " "))
    });
}

/// Draw the lasso being dragged, with its kind beside the cursor.
pub fn draw(app: &CadApp, painter: &egui::Painter, xf: &Xf, cursor: Option<Pos2>) {
    let Some(l) = app.canvas.lasso.as_ref() else { return };
    let t = Tokens::get();
    let mut pts: Vec<Pos2> = l.pts.iter().map(|p| xf.to_screen(*p)).collect();
    if let Some(c) = cursor {
        pts.push(c);
    }
    let kind = l.kind();
    let crossing = kind != LassoKind::Window;
    let stroke = Stroke::new(1.0, if crossing { t.crossing_stroke } else { t.window_stroke });
    if kind != LassoKind::Fence && pts.len() >= 3 {
        if let Some(fill) = selection_area_fill(&app.session.settings, crossing) {
            fill_polygon(painter, &pts, fill);
        }
        if let Some(first) = pts.first().copied() {
            pts.push(first);
        }
    }
    if crossing {
        painter.extend(Shape::dashed_line(&pts, stroke, 5.0, 3.0));
    } else {
        painter.add(Shape::line(pts, stroke));
    }
    if let Some(c) = cursor {
        let label = match kind {
            LassoKind::Window => crate::tl!("Window Lasso"),
            LassoKind::Crossing => crate::tl!("Crossing Lasso"),
            LassoKind::Fence => crate::tl!("Fence Lasso"),
        };
        let hint = crate::tl!("Press Space to cycle options");
        tooltip(painter, c + egui::vec2(16.0, 16.0), &format!("{label}  ·  {hint}"));
    }
}

/// Fill a closed (possibly concave or self-crossing) polygon, even-odd.
fn fill_polygon(painter: &egui::Painter, pts: &[Pos2], color: Color32) {
    let ring: Vec<Vec2> = pts.iter().map(|p| Vec2::new(f64::from(p.x), f64::from(p.y))).collect();
    let tris = cadcraft_render::triangulate_evenodd(&[ring]);
    let mut mesh = egui::Mesh::default();
    for p in &tris {
        let i = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
        mesh.colored_vertex(Pos2::new(p.x as f32, p.y as f32), color);
        mesh.indices.push(i);
    }
    if !mesh.indices.is_empty() {
        painter.add(Shape::mesh(mesh));
    }
}
