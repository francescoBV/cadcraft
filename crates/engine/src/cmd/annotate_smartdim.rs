//! DIM, AutoCAD's smart dimension command: one command for every kind of dimension. Hovering an
//! object previews the dimension that suits it (a line: aligned, or horizontal/vertical when the
//! cursor is beyond its ends; a circle: diameter; an arc: radius; a polyline: the segment under
//! the cursor); picking it asks where to place that dimension, with options to change its kind
//! (Radius/Diameter, Arc length, Angular, a second line for an angle). Picking empty space starts
//! a linear dimension from two points. Angular, Baseline, Continue and Ordinate run those commands'
//! prompts inside DIM; Layer sets DIMLAYER, the layer new dimensions go on. DIM repeats until
//! Enter, and Undo takes back the last dimension it made.

use cadcraft_doc::{DimKind, EntityKind, Handle};
use cadcraft_geom::{Segment, Vec2};
use serde_json::{Value, json};

use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

const CMD: &str = "dim";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(CMD, "Dimension", run)
            .menu(&["Dimension", "Dimension"])
            .params(
                "{object: hex, at, pick?: [x,y] (where the object was picked: a polyline's segment), type?: linear|aligned|radius|diameter|arc|angular (default: what suits the object), second?: hex (a second line: an angle, or the distance between parallel lines), layer?} \
                 | {p1, p2, at, rotation?, aligned?: bool} | {option: baseline|continue, points} | {option: ordinate, feature, leader} | {option: angular, vertex, p1, p2, at} | {option: layer, name (\".\" = current layer)} → {handle(s)}",
            )
            .interactive(|_| Ok(Box::new(SmartDimM::default()))),
    ]
}

/// What a picked object is dimensioned as.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    /// A straight piece: a line or a polyline segment (its ends).
    Line(Vec2, Vec2),
    /// A circle (centre, radius).
    Circle(Vec2, f64),
    /// An arc or an arc segment of a polyline.
    Arc(cadcraft_geom::Arc),
}

/// The kind a dimension of a picked object is made as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Made {
    /// Aligned within the line's span, horizontal or vertical beyond its ends.
    Auto,
    Linear,
    Aligned,
    Radius,
    Diameter,
    ArcLength,
    Angular,
}

/// The piece of object `h` picked at `p`.
fn target_of(s: &Session, h: Handle, p: Vec2) -> Option<Target> {
    let d = s.doc().ok()?;
    match &d.entity(h)?.kind {
        EntityKind::Line(l) => Some(Target::Line(l.a.xy(), l.b.xy())),
        EntityKind::Circle(c) => Some(Target::Circle(c.center.xy(), c.radius)),
        EntityKind::Arc(a) => Some(Target::Arc(cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end))),
        EntityKind::LwPolyline(_) => match super::segment_at(s, h, p)? {
            Segment::Line(l) => Some(Target::Line(l.a, l.b)),
            Segment::Arc { arc, .. } => Some(Target::Arc(arc)),
        },
        _ => None,
    }
    .filter(|t| match t {
        Target::Line(a, b) => a.is_finite() && b.is_finite() && a.dist(*b) > 1e-12,
        Target::Circle(c, r) => c.is_finite() && r.is_finite() && *r > 0.0,
        Target::Arc(a) => a.center.is_finite() && a.radius.is_finite() && a.radius > 0.0,
    })
}

/// The kind that suits a target when none is asked for.
fn default_made(t: &Target) -> Made {
    match t {
        Target::Line(..) => Made::Auto,
        Target::Circle(..) => Made::Diameter,
        Target::Arc(..) => Made::Radius,
    }
}

/// A dimension of target `t` as `made`, located at `at`; `None` when that kind doesn't apply.
fn make(s: &Session, t: &Target, made: Made, at: Vec2) -> Option<EntityKind> {
    let k = match (*t, made) {
        (Target::Line(a, b), Made::Auto | Made::Linear | Made::Aligned) => {
            let u = b - a;
            let along = (at - a).dot(u) / u.dot(u);
            let axis = u.x.abs() < 1e-12 * u.len() || u.y.abs() < 1e-12 * u.len();
            let kind = match made {
                Made::Linear => DimKind::Linear { rotation: super::auto_rotation(a, b, at) },
                Made::Aligned => DimKind::Aligned,
                // Within the span: along the line (a horizontal or vertical one as such); beyond
                // its ends: horizontal or vertical, as AutoCAD's DIM previews it.
                _ if (0.0..=1.0).contains(&along) && axis => {
                    DimKind::Linear { rotation: if u.y.abs() > u.x.abs() { std::f64::consts::FRAC_PI_2 } else { 0.0 } }
                }
                _ if (0.0..=1.0).contains(&along) => DimKind::Aligned,
                _ => DimKind::Linear { rotation: super::auto_rotation(a, b, at) },
            };
            super::dim(s, kind, at, a, b, Vec2::ZERO, "")
        }
        (Target::Circle(c, r), Made::Radius | Made::Diameter)
        | (Target::Arc(cadcraft_geom::Arc { center: c, radius: r, .. }), Made::Radius | Made::Diameter) => {
            let dir = if at.dist(c) > 1e-12 { (at - c).normalized() } else { Vec2::X };
            let pt = c + dir * r;
            let k = if made == Made::Diameter {
                super::dim(s, DimKind::Diameter, c + (c - pt), Vec2::ZERO, Vec2::ZERO, pt, "")
            } else {
                super::dim(s, DimKind::Radius, c, Vec2::ZERO, Vec2::ZERO, pt, "")
            };
            super::text_at(k, at)
        }
        (Target::Arc(g), Made::ArcLength) => super::dim(s, DimKind::ArcLength, at, g.start_point(), g.end_point(), g.center, ""),
        (Target::Arc(g), Made::Angular) => super::dim(s, DimKind::Angular3P, at, g.start_point(), g.end_point(), g.center, ""),
        _ => return None,
    };
    Some(k)
}

/// Twice the current style's text height (scaled): how far from the object a hover preview sits.
fn preview_offset(s: &Session) -> f64 {
    let Ok(d) = s.doc() else { return 0.36 };
    let st = d.dim_style(&d.header.str("DIMSTYLE", "Standard")).cloned().unwrap_or_default().with_overrides(&d.dim_overrides());
    let h = 2.0 * st.text_height * st.effective_scale(d.header.f64("DIMSCALE", 1.0));
    if h.is_finite() && h > 0.0 { h } else { 0.36 }
}

/// Where the hover preview of target `t` is placed for cursor `c`: a little off the object, on
/// the cursor's side.
fn hover_at(s: &Session, t: &Target, c: Vec2) -> Vec2 {
    let off = preview_offset(s);
    match *t {
        Target::Line(a, b) => {
            let n = (b - a).perp().normalized();
            let side = if (c - a).dot(n) >= 0.0 { 1.0 } else { -1.0 };
            a.mid(b) + n * (off * side)
        }
        Target::Circle(ctr, r) | Target::Arc(cadcraft_geom::Arc { center: ctr, radius: r, .. }) => {
            let dir = if c.dist(ctr) > 1e-12 { (c - ctr).normalized() } else { Vec2::X };
            ctr + dir * (r + off)
        }
    }
}

fn other(m: impl Into<String>) -> crate::EngineError {
    crate::EngineError::Other(m.into())
}

/// DIMLAYER from a layer name ("." or "" = the current layer).
fn set_dim_layer(s: &mut Session, name: &str) -> Result<Value> {
    let name = name.trim();
    let value = if name.is_empty() || name == "." || name.eq_ignore_ascii_case("use current") {
        ".".to_string()
    } else {
        s.doc()?.layer(name).map(|l| l.name.clone()).ok_or_else(|| bad(CMD, format!("no layer `{name}`")))?
    };
    crate::sysvars::set(s, "DIMLAYER", &json!(value))?;
    Ok(json!({ "dimlayer": value }))
}

fn with_layer(s: &mut Session, p: &Value, h: Handle) -> Result<()> {
    if let Some(l) = str_param(p, "layer").map(str::trim).filter(|l| !l.is_empty() && *l != ".") {
        let layer = s.doc()?.layer(l).map(|x| x.name.clone()).ok_or_else(|| bad(CMD, format!("no layer `{l}`")))?;
        s.doc_mut()?.modify_entity(h, |e| e.common.layer = layer)?;
    }
    Ok(())
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    match str_param(p, "option").map(|o| o.trim().to_ascii_lowercase()).as_deref() {
        Some("baseline") => return super::chain_run(s, p, true),
        Some("continue") => return super::chain_run(s, p, false),
        Some("ordinate") => return super::run_ordinate(s, p),
        Some("angular") => return super::run_angular(s, p),
        Some("layer") => return set_dim_layer(s, str_param(p, "name").unwrap_or(".")),
        Some(o) if !o.is_empty() => return Err(bad(CMD, format!("unknown option `{o}` (baseline, continue, ordinate, angular, layer)"))),
        _ => {}
    }
    let h = match super::hex_param(p, "object") {
        Some(obj) => {
            let at = point_req(CMD, p, "at")?;
            let pick = point_param(p, "pick").unwrap_or(at);
            if let Some(second) = super::hex_param(p, "second") {
                two_lines(s, obj, second, at)?
            } else {
                let t = target_of(s, obj, pick).ok_or_else(|| bad(CMD, "`object` must be a line, circle, arc or polyline"))?;
                let made = match str_param(p, "type").map(|t| t.trim().to_ascii_lowercase()).as_deref() {
                    None | Some("") => default_made(&t),
                    Some("linear") => Made::Linear,
                    Some("aligned") => Made::Aligned,
                    Some("radius") => Made::Radius,
                    Some("diameter") => Made::Diameter,
                    Some("arc") | Some("arclength") => Made::ArcLength,
                    Some("angular") => Made::Angular,
                    Some(o) => return Err(bad(CMD, format!("unknown type `{o}`"))),
                };
                let k = make(s, &t, made, at).ok_or_else(|| bad(CMD, "that kind of dimension doesn't apply to this object"))?;
                super::add_dim(s, k)?
            }
        }
        None => {
            let (a, b, at) = (point_req(CMD, p, "p1")?, point_req(CMD, p, "p2")?, point_req(CMD, p, "at")?);
            if a.near(b, 1e-12) {
                return Err(bad(CMD, "`p1` and `p2` must differ"));
            }
            let rot = p.get("rotation").and_then(Value::as_f64).filter(|r| r.is_finite()).map(f64::to_radians);
            let kind = super::linear_kind(a, b, at, rot, bool_or(p, "aligned", false));
            let k = super::dim(s, kind, at, a, b, Vec2::ZERO, str_param(p, "text").unwrap_or(""));
            super::add_dim(s, k)?
        }
    };
    with_layer(s, p, h)?;
    Ok(json!({ "handle": h.hex() }))
}

/// Two lines: the angle between them, or the distance between them when they are parallel.
fn two_lines(s: &mut Session, h1: Handle, h2: Handle, at: Vec2) -> Result<Handle> {
    let line = |s: &Session, h: Handle| match s.doc().ok().and_then(|d| d.entity(h)).map(|e| &e.kind) {
        Some(EntityKind::Line(l)) => Some((l.a.xy(), l.b.xy())),
        _ => None,
    };
    let ((a1, a2), (b1, b2)) =
        (line(s, h1).ok_or_else(|| bad(CMD, "`object` must be a line"))?, line(s, h2).ok_or_else(|| bad(CMD, "`second` must be a line"))?);
    let parallel = (a2 - a1).normalized().perp().dot((b2 - b1).normalized()).abs() < 1e-9;
    let k = if parallel {
        // From the first line's start to its foot on the second, aligned with that gap.
        let u = (b2 - b1).normalized();
        let foot = b1 + u * (a1 - b1).dot(u);
        if foot.dist(a1) < 1e-12 {
            return Err(bad(CMD, "the lines overlap"));
        }
        super::dim(s, DimKind::Aligned, at, a1, foot, Vec2::ZERO, "")
    } else {
        super::angular_from_lines(s, h1, h2, at).ok_or_else(|| bad(CMD, "objects must be two lines"))?
    };
    super::add_dim(s, k)
}

/// What DIM asks next.
#[derive(Default)]
enum Ask {
    /// Select objects or specify first extension line origin.
    #[default]
    Start,
    /// The second extension line origin (after a first point in empty space).
    Second(Vec2),
    /// The dimension line location of a two-point dimension.
    Location { a: Vec2, b: Vec2, aligned: bool },
    /// Where to place the dimension of a picked object (or a second line for an angle).
    Object { handle: Handle, target: Target, made: Made },
    /// The location of the angle between two lines.
    Angle { first: Handle, second: Handle },
    /// The layer for new dimensions.
    Layer,
    /// Angular, Baseline, Continue or Ordinate, running inside DIM.
    Sub(Box<dyn Interactive>),
}

#[derive(Default)]
struct SmartDimM {
    ask: Ask,
    text: super::DimText,
    /// The dimensions this DIM made, for Undo.
    created: Vec<Handle>,
}

impl SmartDimM {
    fn place(&mut self, s: &mut Session, k: EntityKind) -> Result<Step> {
        let h = super::add_dim(s, self.text.apply(k))?;
        if let Some(EntityKind::Dimension(d)) = s.doc()?.entity(h).map(|e| e.kind.clone()) {
            let st = s.doc()?.dim_style(&d.style).cloned().unwrap_or_default();
            let g = cadcraft_render::dimension_geometry(&d, &st, 1.0);
            s.echo(format!("Dimension text = {}", g.value));
        }
        self.created.push(h);
        self.restart();
        Ok(Step::Continue)
    }
    fn restart(&mut self) {
        self.ask = Ask::Start;
        self.text = super::DimText::default();
    }
    /// Start a sub-command; back to the first prompt when it ends at once.
    fn sub(&mut self, s: &mut Session, mut m: Box<dyn Interactive>) -> Result<Step> {
        match m.begin(s)? {
            Step::Continue => self.ask = Ask::Sub(m),
            _ => self.restart(),
        }
        Ok(Step::Continue)
    }
    fn undo(&mut self, s: &mut Session) -> Result<Step> {
        match self.created.pop() {
            Some(h) => {
                s.doc_mut()?.remove_entity(h);
                s.last_dim = self.created.last().copied();
            }
            None => s.echo("Nothing to undo."),
        }
        self.restart();
        Ok(Step::Continue)
    }
    /// The text options (Mtext, Text, Text angle) of a placement prompt.
    fn text_keyword(&mut self, k: &str) -> bool {
        self.text.keyword(if k == "Text angle" { "Angle" } else { k })
    }
}

impl Interactive for SmartDimM {
    fn name(&self) -> &'static str {
        "DIM"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let layer = super::dim_layer(s).unwrap_or_else(|| "use current".into());
        let style = s.doc()?.header.str("DIMSTYLE", "Standard");
        s.echo(format!("Current settings: Dimension style = {style}  Dimension layer = {layer}"));
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if let Some(p) = self.text.prompt() {
            return p;
        }
        match &self.ask {
            Ask::Start => Prompt::new("Select objects or specify first extension line origin", Accept::POINT)
                .kw(&["Angular", "Baseline", "Continue", "Ordinate", "Layer", "Undo"]),
            Ask::Second(a) => Prompt::new("Specify second extension line origin", Accept::POINT).base(*a).kw(&["Undo"]),
            Ask::Location { aligned, .. } => Prompt::new("Specify dimension line location", Accept::POINT).kw(&[
                "Mtext",
                "Text",
                "Text angle",
                if *aligned { "Linear" } else { "aLigned" },
                "Undo",
            ]),
            Ask::Object { target: Target::Line(..), .. } => {
                Prompt::new("Specify dimension line location or second line for angle", Accept::POINT).kw(&["Mtext", "Text", "Text angle", "Undo"])
            }
            Ask::Object { target: Target::Circle(..), made, .. } => Prompt::new(
                if *made == Made::Radius { "Specify radius dimension location" } else { "Specify diameter dimension location" },
                Accept::POINT,
            )
            .kw(&[if *made == Made::Radius { "Diameter" } else { "Radius" }, "Mtext", "Text", "Text angle", "Undo"]),
            Ask::Object { target: Target::Arc(_), made, .. } => {
                let (msg, kw) = match made {
                    Made::Diameter => ("Specify diameter dimension location", "Radius"),
                    Made::ArcLength => ("Specify arc length dimension location", "Radius"),
                    Made::Angular => ("Specify angle dimension location", "Radius"),
                    _ => ("Specify radius dimension location", "Diameter"),
                };
                Prompt::new(msg, Accept::POINT).kw(&[kw, "Angular", "Arc length", "Mtext", "Text", "Text angle", "Undo"])
            }
            Ask::Angle { .. } => Prompt::new("Specify angular dimension location", Accept::POINT).kw(&["Mtext", "Text", "Text angle", "Undo"]),
            Ask::Layer => Prompt::new("Enter layer name or select object to specify layer name", Accept { text: true, ..Accept::POINT })
                .kw(&["Use current"])
                .default(super::dim_layer(s).unwrap_or_else(|| "use current".into())),
            Ask::Sub(m) => m.prompt(s),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.text.answer(s, &i) {
            return Ok(Step::Continue);
        }
        if let Ask::Sub(m) = &mut self.ask {
            match m.input(s, i)? {
                Step::Continue => {}
                // The sub-command's dimensions can be undone from DIM too.
                _ => {
                    if let Some(h) = s.last_dim.filter(|h| !self.created.contains(h)) {
                        self.created.push(h);
                    }
                    self.restart();
                }
            }
            return Ok(Step::Continue);
        }
        match std::mem::take(&mut self.ask) {
            Ask::Start => match i {
                Input::Point(p) => {
                    match super::pick_at(s, p).and_then(|h| Some((h, target_of(s, h, p)?))) {
                        Some((handle, target)) => self.ask = Ask::Object { handle, target, made: default_made(&target) },
                        None => self.ask = Ask::Second(p),
                    }
                    Ok(Step::Continue)
                }
                Input::Keyword(k) => match k.as_str() {
                    "Angular" => self.sub(s, Box::new(super::AngularM::default())),
                    "Baseline" => self.sub(s, Box::new(super::ChainM::new(s, true))),
                    "Continue" => self.sub(s, Box::new(super::ChainM::new(s, false))),
                    "Ordinate" => self.sub(s, Box::new(super::OrdinateM::default())),
                    "Layer" => {
                        self.ask = Ask::Layer;
                        Ok(Step::Continue)
                    }
                    "Undo" => self.undo(s),
                    _ => Ok(Step::Continue),
                },
                Input::Enter | Input::Cancel => Ok(Step::Done),
                _ => Ok(Step::Continue),
            },
            Ask::Second(a) => match i {
                Input::Point(b) if !b.near(a, 1e-12) => {
                    self.ask = Ask::Location { a, b, aligned: false };
                    Ok(Step::Continue)
                }
                Input::Keyword(k) if k == "Undo" => {
                    self.restart();
                    Ok(Step::Continue)
                }
                Input::Enter | Input::Cancel => {
                    self.restart();
                    Ok(Step::Continue)
                }
                _ => {
                    self.ask = Ask::Second(a);
                    Ok(Step::Continue)
                }
            },
            Ask::Location { a, b, aligned } => match i {
                Input::Point(at) => {
                    let k = super::dim(s, super::linear_kind(a, b, at, None, aligned), at, a, b, Vec2::ZERO, "");
                    self.place(s, k)
                }
                Input::Keyword(k) => {
                    let aligned = match k.as_str() {
                        "aLigned" => true,
                        "Linear" => false,
                        "Undo" => {
                            self.restart();
                            return Ok(Step::Continue);
                        }
                        other => {
                            self.text_keyword(other);
                            aligned
                        }
                    };
                    self.ask = Ask::Location { a, b, aligned };
                    Ok(Step::Continue)
                }
                Input::Enter | Input::Cancel => {
                    self.restart();
                    Ok(Step::Continue)
                }
                _ => {
                    self.ask = Ask::Location { a, b, aligned };
                    Ok(Step::Continue)
                }
            },
            Ask::Object { handle, target, made } => match i {
                Input::Point(at) => {
                    // A second line picked: the angle between the two.
                    if let Target::Line(..) = target
                        && let Some(h2) = super::pick_at(s, at).filter(|h2| *h2 != handle)
                        && matches!(s.doc()?.entity(h2).map(|e| &e.kind), Some(EntityKind::Line(_)))
                    {
                        self.ask = Ask::Angle { first: handle, second: h2 };
                        return Ok(Step::Continue);
                    }
                    match make(s, &target, made, at) {
                        Some(k) => self.place(s, k),
                        None => {
                            self.ask = Ask::Object { handle, target, made };
                            Ok(Step::Continue)
                        }
                    }
                }
                Input::Keyword(k) => {
                    let made = match k.as_str() {
                        "Radius" => Made::Radius,
                        "Diameter" => Made::Diameter,
                        "Arc length" => Made::ArcLength,
                        "Angular" => Made::Angular,
                        "Undo" => {
                            self.restart();
                            return Ok(Step::Continue);
                        }
                        other => {
                            self.text_keyword(other);
                            made
                        }
                    };
                    self.ask = Ask::Object { handle, target, made };
                    Ok(Step::Continue)
                }
                Input::Enter | Input::Cancel => {
                    self.restart();
                    Ok(Step::Continue)
                }
                _ => {
                    self.ask = Ask::Object { handle, target, made };
                    Ok(Step::Continue)
                }
            },
            Ask::Angle { first, second } => match i {
                Input::Point(at) => match super::angular_from_lines(s, first, second, at) {
                    Some(k) => self.place(s, k),
                    None => {
                        s.echo("The lines are parallel.");
                        self.restart();
                        Ok(Step::Continue)
                    }
                },
                Input::Keyword(k) if k == "Undo" => {
                    self.restart();
                    Ok(Step::Continue)
                }
                Input::Keyword(k) => {
                    self.text_keyword(&k);
                    self.ask = Ask::Angle { first, second };
                    Ok(Step::Continue)
                }
                Input::Enter | Input::Cancel => {
                    self.restart();
                    Ok(Step::Continue)
                }
                _ => {
                    self.ask = Ask::Angle { first, second };
                    Ok(Step::Continue)
                }
            },
            Ask::Layer => {
                let r = match i {
                    Input::Point(p) => match super::pick_at(s, p).and_then(|h| s.doc().ok()?.entity(h).map(|e| e.common.layer.clone())) {
                        Some(l) => set_dim_layer(s, &l).map(|_| ()),
                        None => Err(other("No object found.")),
                    },
                    Input::Text(t) | Input::Keyword(t) => set_dim_layer(s, &t).map(|_| ()),
                    _ => Ok(()),
                };
                self.restart();
                r?;
                let layer = super::dim_layer(s).unwrap_or_else(|| "use current".into());
                s.echo(format!("Dimension layer = {layer}"));
                Ok(Step::Continue)
            }
            Ask::Sub(m) => {
                // Handled above; keep it.
                self.ask = Ask::Sub(m);
                Ok(Step::Continue)
            }
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        match &self.ask {
            // Hover: the dimension the object under the cursor gets.
            Ask::Start => {
                let Some(h) = super::pick_at(s, c) else { return Vec::new() };
                let Some(t) = target_of(s, h, c) else { return Vec::new() };
                make(s, &t, default_made(&t), hover_at(s, &t, c)).into_iter().collect()
            }
            Ask::Second(a) => vec![super::super::helpers::line(*a, c)],
            Ask::Location { a, b, aligned } => {
                vec![self.text.apply(super::dim(s, super::linear_kind(*a, *b, c, None, *aligned), c, *a, *b, Vec2::ZERO, ""))]
            }
            Ask::Object { target, made, .. } => make(s, target, *made, c).map(|k| self.text.apply(k)).into_iter().collect(),
            Ask::Angle { first, second } => super::angular_from_lines(s, *first, *second, c).map(|k| self.text.apply(k)).into_iter().collect(),
            Ask::Layer => Vec::new(),
            Ask::Sub(m) => m.preview(s, c),
        }
    }
}
