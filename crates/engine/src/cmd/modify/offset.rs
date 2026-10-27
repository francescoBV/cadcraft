use super::*;

/// Whether OFFSET can work on this kind of object at all.
fn offsettable(k: &EntityKind) -> bool {
    matches!(
        k,
        EntityKind::Line(_)
            | EntityKind::Circle(_)
            | EntityKind::Arc(_)
            | EntityKind::Ellipse(_)
            | EntityKind::LwPolyline(_)
            | EntityKind::Spline(_)
            | EntityKind::XLine(_)
            | EntityKind::Ray(_)
    )
}

/// Offset an entity by `dist` towards `side` (OFFSETGAPTYPE `gap` for polyline corners).
/// Returns the new geometry: usually one object, several where a polyline's offset splits, none
/// where it collapses.
pub(crate) fn offset_kinds(k: &EntityKind, dist: f64, side: Vec2, gap: GapType) -> Vec<EntityKind> {
    match k {
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            let Some(left) = polyline_side_is_left(&pl, side) else { return Vec::new() };
            // Widths don't move the offset (it is of the centre line); a constant width carries over.
            pl.offset(if left { dist } else { -dist }, gap)
                .into_iter()
                .map(|o| {
                    let mut k = lwpoly(o.vertices, o.closed);
                    if let EntityKind::LwPolyline(n) = &mut k {
                        n.const_width = p.const_width;
                        n.elevation = p.elevation;
                        n.plinegen = p.plinegen;
                    }
                    k
                })
                .collect()
        }
        _ => offset_kind(k, dist, side).into_iter().collect(),
    }
}

/// Whether `side` lies to the left of a polyline's direction of travel, judged at its nearest
/// point (on an arc: inside a counter-clockwise one, outside a clockwise one).
fn polyline_side_is_left(pl: &Polyline, side: Vec2) -> Option<bool> {
    let segs = pl.segments();
    let nearest =
        segs.iter().filter(|s| s.closest(side).is_finite()).min_by(|a, b| a.closest(side).dist(side).total_cmp(&b.closest(side).dist(side)))?;
    Some(match nearest {
        Segment::Line(l) => l.side(side) > 0.0,
        Segment::Arc { arc, ccw } => (side.dist(arc.center) < arc.radius) == *ccw,
    })
}

/// The exact distance from `p` to an object OFFSET works on (Through mode), if it is one.
fn offset_distance(k: &EntityKind, p: Vec2) -> Option<f64> {
    let d = match k {
        EntityKind::Line(l) => Line::new(l.a.xy(), l.b.xy()).dist(p),
        EntityKind::Circle(c) => (p.dist(c.center.xy()) - c.radius).abs(),
        EntityKind::Arc(a) => Segment::Arc { arc: Arc::new(a.center.xy(), a.radius, a.start, a.end), ccw: true }.closest(p).dist(p),
        EntityKind::LwPolyline(pl) => Polyline { vertices: pl.vertices.clone(), closed: pl.closed }.closest(p)?.dist(p),
        EntityKind::XLine(r) | EntityKind::Ray(r) => Line::new(r.base.xy(), r.base.xy() + r.dir.xy()).project(p).dist(p),
        _ => return None,
    };
    d.is_finite().then_some(d)
}

/// Offset an entity other than a polyline by `dist` towards `side`.
fn offset_kind(k: &EntityKind, dist: f64, side: Vec2) -> Option<EntityKind> {
    match k {
        EntityKind::Line(l) => {
            let ln = Line::new(l.a.xy(), l.b.xy());
            let sgn = if ln.side(side) >= 0.0 { 1.0 } else { -1.0 };
            let o = ln.offset(dist * sgn);
            Some(line(o.a, o.b))
        }
        EntityKind::Circle(c) => {
            let inside = side.dist(c.center.xy()) < c.radius;
            let r = if inside { c.radius - dist } else { c.radius + dist };
            (r > 1e-12).then(|| circle(c.center.xy(), r))
        }
        EntityKind::Arc(a) => {
            let inside = side.dist(a.center.xy()) < a.radius;
            let r = if inside { a.radius - dist } else { a.radius + dist };
            (r > 1e-12).then(|| arc(&Arc::new(a.center.xy(), r, a.start, a.end)))
        }
        EntityKind::Ellipse(e) => {
            // Approximate with a polyline offset of the tessellation.
            let ge = cadcraft_geom::Ellipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end };
            // Inside = on the centre side of the full ellipse, even for an open arc.
            let inside = {
                let a = e.major.xy().len();
                let b = a * e.ratio.abs();
                let u = e.major.xy().normalized();
                let d = side - e.center.xy();
                let (x, y) = (d.dot(u) * b, d.dot(u.perp()) * a);
                x * x + y * y < (a * b) * (a * b)
            };
            // offset_points shifts along the left normal, which points into a counter-clockwise ellipse.
            let mut pts = Vec::new();
            ge.tessellate(e.major.xy().len() * 1e-3, &mut pts);
            let off = offset_points(&pts, if inside { dist } else { -dist }, ge.is_full());
            Some(lwpoly(off.into_iter().map(PolyVertex::new).collect(), ge.is_full()))
        }
        EntityKind::Spline(sp) => {
            let pts = sp.tessellate(1e-3);
            let n = pts.len();
            let mid = pts.get(n / 2).copied()?;
            let next = pts.get((n / 2 + 1).min(n.saturating_sub(1))).copied()?;
            let left = (next - mid).cross(side - mid) > 0.0;
            let off = offset_points(&pts, if left { dist } else { -dist }, sp.closed);
            // A closed spline stays closed (periodic), smooth across its start point.
            Some(EntityKind::Spline(cadcraft_geom::Spline::from_fit(&decimate(&off, 40), sp.closed)))
        }
        EntityKind::XLine(r) | EntityKind::Ray(r) => {
            let ln = Line::new(r.base.xy(), r.base.xy() + r.dir.xy());
            let sgn = if ln.side(side) >= 0.0 { 1.0 } else { -1.0 };
            let o = ln.offset(dist * sgn);
            let rl = cadcraft_doc::RayLine { base: v3(o.a), dir: r.dir };
            Some(if matches!(k, EntityKind::XLine(_)) { EntityKind::XLine(rl) } else { EntityKind::Ray(rl) })
        }
        _ => None,
    }
}

fn decimate(pts: &[Vec2], max: usize) -> Vec<Vec2> {
    if pts.len() <= max {
        return pts.to_vec();
    }
    let step = pts.len() as f64 / max as f64;
    let mut v: Vec<Vec2> = (0..max).filter_map(|i| pts.get((i as f64 * step) as usize).copied()).collect();
    if let Some(l) = pts.last() {
        v.push(*l);
    }
    v
}

fn offset_points(pts: &[Vec2], d: f64, closed: bool) -> Vec<Vec2> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = pts.get(i).copied().unwrap_or_default();
        let prev = if i > 0 {
            pts.get(i - 1)
        } else if closed {
            pts.get(n.saturating_sub(2))
        } else {
            None
        };
        let next = if i + 1 < n {
            pts.get(i + 1)
        } else if closed {
            pts.get(1)
        } else {
            None
        };
        let t = match (prev, next) {
            (Some(a), Some(b)) => (*b - *a).normalized(),
            (None, Some(b)) => (*b - p).normalized(),
            (Some(a), None) => (p - *a).normalized(),
            _ => Vec2::X,
        };
        out.push(p + t.perp() * d);
    }
    out
}

pub(super) fn run_offset(s: &mut Session, p: &Value) -> Result<Value> {
    let h = targets(s, p)?.first().copied().ok_or_else(|| bad("offset", "`handle` is required"))?;
    let dist = f64_req("offset", p, "distance")?;
    let side = point_req("offset", p, "side")?;
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| bad("offset", "no such object"))?;
    let gap = match p.get("gapType") {
        Some(v) => GapType::from_index(v.as_i64().filter(|g| (0..=2).contains(g)).ok_or_else(|| bad("offset", "`gapType` is 0, 1 or 2"))?),
        None => offset_gap(s),
    };
    let ks = offset_kinds(&e.kind, dist.abs(), side, gap);
    if ks.is_empty() {
        return Err(bad("offset", "cannot offset that object"));
    }
    let current_layer = match str_param(p, "layer").map(str::to_ascii_lowercase).as_deref() {
        None | Some("source") => false,
        Some("current") => true,
        Some(_) => return Err(bad("offset", "`layer` is \"source\" or \"current\"")),
    };
    let hs = add_offsets(s, &e, ks, current_layer, bool_or(p, "erase", false))?;
    let first = hs.first().map(|h| h.hex()).unwrap_or_default();
    Ok(json!({ "handle": first, "handles": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// OFFSETGAPTYPE: how polyline corners are filled.
fn offset_gap(s: &Session) -> GapType {
    GapType::from_index(s.doc().map_or(0, |d| d.header.i64("OFFSETGAPTYPE", 0)))
}

/// Add the offset copies `ks` of `src` (on the current layer instead of the source's when
/// `current_layer`), then erase the source when `erase` and its layer is not locked.
fn add_offsets(s: &mut Session, src: &Entity, ks: Vec<EntityKind>, current_layer: bool, erase: bool) -> Result<Vec<Handle>> {
    let space = s.space();
    let d = s.doc_mut()?;
    let mut common = src.common.clone();
    if current_layer {
        common.layer = d.header.str("CLAYER", "0");
    }
    let mut out = Vec::with_capacity(ks.len());
    for k in ks {
        let nh = d.new_handle();
        if let Some(st) = d.space_mut(&space) {
            st.push(Entity { handle: nh, common: common.clone(), kind: k });
        }
        out.push(nh);
    }
    let locked = d.layer(&src.common.layer).is_some_and(|l| l.locked);
    if erase && !locked && !out.is_empty() {
        d.remove_entity(src.handle);
    }
    Ok(out)
}

/// An OFFSET option prompt opened from "Specify offset distance".
#[derive(Clone, Copy, PartialEq)]
enum OffsetAsk {
    Erase,
    Layer,
}

pub(super) struct OffsetM {
    dist: Option<f64>,
    distance_first: Option<Vec2>,
    through: bool,
    picked: Option<Handle>,
    ask: Option<OffsetAsk>,
    /// Multiple mode: each side point offsets the newest copy again, until Enter.
    multiple: bool,
    /// Undo: the drawing before each offset, with the object picked then.
    undo: Vec<(std::sync::Arc<cadcraft_doc::Drawing>, Option<Handle>)>,
}

impl OffsetM {
    pub(super) fn new(s: &Session) -> Self {
        let _ = s;
        OffsetM { dist: None, distance_first: None, through: false, picked: None, ask: None, multiple: false, undo: Vec::new() }
    }

    fn set_distance(&mut self, s: &mut Session, distance: f64) -> Result<()> {
        if !(distance.is_finite() && distance > 0.0) {
            return Err(EngineError::Other("Requires a positive distance.".into()));
        }
        s.doc_mut()?.header.set_f64("OFFSETDIST", distance);
        self.dist = Some(distance);
        self.distance_first = None;
        Ok(())
    }
}

/// OFFSET's remembered options: erase the source (OFFSETERASE) and put copies on the current
/// layer instead of the source's (kept in the drawing header beside it).
fn offset_options(s: &Session) -> (bool, bool) {
    s.doc().map(|d| (d.header.i64("OFFSETERASE", 0) != 0, d.header.i64("OFFSETLAYER", 0) != 0)).unwrap_or((false, false))
}

impl Interactive for OffsetM {
    fn name(&self) -> &'static str {
        "OFFSET"
    }
    /// Starts OFFSET, keeping a single preselected offsettable object (issue #1610) so the prompt
    /// sequence goes distance then side instead of asking for the object again; any other
    /// selection is cleared and the object loop asks for it. With PICKFIRST off the selection is
    /// likewise ignored and cleared.
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        // Exactly one offsettable object selected before the command is taken at once, so only the
        // side is asked for; the object loop stays for the ones after it (issue #1610). A multiple
        // selection is left for the select prompt, as AutoCAD does.
        let pre = s.selection();
        if s.settings.pickfirst
            && let [h] = pre[..]
            && s.doc()?.entity(h).is_some_and(|e| offsettable(&e.kind))
        {
            self.picked = Some(h);
            s.set_selection(vec![h]);
        } else {
            s.set_selection(Vec::new());
        }
        let (erase, current) = offset_options(s);
        let gap = s.doc().map_or(0, |d| d.header.i64("OFFSETGAPTYPE", 0));
        s.echo(format!(
            "Current settings: Erase source={}  Layer={}  OFFSETGAPTYPE={gap}",
            if erase { "Yes" } else { "No" },
            if current { "Current" } else { "Source" }
        ));
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let (erase, current) = offset_options(s);
        match self.ask {
            Some(OffsetAsk::Erase) => {
                return Prompt::new("Erase source object after offsetting?", Accept::TEXT).kw(&["Yes", "No"]).default(if erase {
                    "Yes"
                } else {
                    "No"
                });
            }
            Some(OffsetAsk::Layer) => {
                return Prompt::new("Enter layer option for offset objects", Accept::TEXT).kw(&["Current", "Source"]).default(if current {
                    "Current"
                } else {
                    "Source"
                });
            }
            None => {}
        }
        let last = s.doc().map(|d| d.header.f64("OFFSETDIST", -1.0)).unwrap_or(-1.0);
        if self.dist.is_none()
            && !self.through
            && let Some(base) = self.distance_first
        {
            return Prompt::new("Specify second point", Accept::POINT).base(base);
        }
        match (self.dist.is_some() || self.through, self.picked) {
            (false, _) => Prompt::new("Specify offset distance", Accept::POINT_OR_NUMBER).kw(&["Through", "Erase", "Layer"]).default(if last < 0.0 {
                "Through".to_string()
            } else {
                format!("{last:.4}")
            }),
            (true, None) => Prompt::new("Select object to offset", Accept::POINT).kw(&["Exit", "Undo"]).default("Exit"),
            (true, Some(_)) if self.multiple => {
                let msg = if self.through { "Specify through point" } else { "Specify point on side to offset" };
                Prompt::new(msg, Accept::POINT).kw(&["Exit", "Undo"]).default("next object")
            }
            (true, Some(_)) if self.through => Prompt::new("Specify through point", Accept::POINT).kw(&["Exit", "Multiple", "Undo"]).default("Exit"),
            (true, Some(_)) => Prompt::new("Specify point on side to offset", Accept::POINT).kw(&["Exit", "Multiple", "Undo"]).default("Exit"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(ask) = self.ask {
            let word = match &i {
                Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
                Input::Enter => String::new(),
                _ => return Ok(Step::Continue),
            };
            let (key, yes, no) = match ask {
                OffsetAsk::Erase => ("OFFSETERASE", "y", "n"),
                OffsetAsk::Layer => ("OFFSETLAYER", "c", "s"),
            };
            let v = if word.is_empty() {
                None
            } else if word.starts_with(yes) {
                Some(1)
            } else if word.starts_with(no) {
                Some(0)
            } else {
                return Err(EngineError::Other("Invalid option keyword.".into()));
            };
            if let Some(v) = v {
                s.doc_mut()?.header.set_i64(key, v);
            }
            self.ask = None;
            return Ok(Step::Continue);
        }
        if self.dist.is_none() && !self.through {
            match i {
                Input::Text(t) => {
                    let d = number(&t).ok_or_else(|| EngineError::Other("Requires a positive distance.".into()))?;
                    self.set_distance(s, d)?;
                }
                Input::Point(p) => {
                    if !p.is_finite() {
                        return Err(EngineError::Other("Invalid point.".into()));
                    }
                    match self.distance_first {
                        Some(base) => self.set_distance(s, base.dist(p))?,
                        None => self.distance_first = Some(p),
                    }
                }
                Input::Keyword(k) if k == "Through" => {
                    self.through = true;
                    self.distance_first = None;
                }
                Input::Keyword(k) if k == "Erase" => self.ask = Some(OffsetAsk::Erase),
                Input::Keyword(k) if k == "Layer" => self.ask = Some(OffsetAsk::Layer),
                Input::Enter => {
                    self.distance_first = None;
                    let last = s.doc()?.header.f64("OFFSETDIST", -1.0);
                    if last > 0.0 {
                        self.dist = Some(last);
                    } else {
                        self.through = true;
                    }
                }
                _ => {}
            }
            return Ok(Step::Continue);
        }
        match (self.picked, i) {
            (_, Input::Keyword(k)) if k == "Exit" => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (_, Input::Keyword(k)) if k == "Undo" => {
                match self.undo.pop() {
                    Some((doc, picked)) => {
                        // Settings changed meanwhile stay.
                        let header = s.doc()?.header.clone();
                        s.state_mut()?.doc = doc;
                        s.doc_mut()?.header = header;
                        self.picked = if self.multiple { picked } else { None };
                        if self.picked.is_none() {
                            self.multiple = false;
                        }
                        s.set_selection(self.picked.into_iter().collect());
                    }
                    None => s.echo("Nothing to undo."),
                }
                Ok(Step::Continue)
            }
            (Some(_), Input::Keyword(k)) if k == "Multiple" => {
                self.multiple = true;
                Ok(Step::Continue)
            }
            (Some(_), Input::Enter) if self.multiple => {
                // <next object>: back to selecting.
                self.multiple = false;
                self.picked = None;
                s.set_selection(Vec::new());
                Ok(Step::Continue)
            }
            (_, Input::Enter) => {
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
            (None, Input::Point(p)) => {
                let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
                let space = s.space();
                match crate::select::pick(s.doc()?, &space, p, ap) {
                    Some(h) => {
                        self.picked = Some(h);
                        s.set_selection(vec![h]);
                    }
                    None => s.echo("*Invalid selection*"),
                }
                Ok(Step::Continue)
            }
            (Some(h), Input::Point(p)) => {
                let e = s.doc()?.entity(h).map(|e| (**e).clone());
                let mut made = None;
                if let Some(e) = e {
                    let dist = match self.dist {
                        Some(d) => d,
                        // Through: the exact distance to the point.
                        None => match offset_distance(&e.kind, p) {
                            Some(d) => d,
                            None => {
                                let polys = crate::select::hit_polylines(s.doc()?, &e, 1e-4);
                                polys
                                    .iter()
                                    .flat_map(|pl| pl.windows(2).filter_map(|w| Some(Line::new(*w.first()?, *w.get(1)?).dist(p))))
                                    .fold(f64::INFINITY, f64::min)
                            }
                        },
                    };
                    let ks = offset_kinds(&e.kind, dist, p, offset_gap(s));
                    if ks.is_empty() {
                        s.echo("Cannot offset that object.");
                    } else {
                        let before = s.state()?.doc.clone();
                        let (erase, current) = offset_options(s);
                        made = add_offsets(s, &e, ks, current, erase)?.last().copied();
                        self.undo.push((before, Some(h)));
                    }
                }
                // Multiple mode carries on from the new copy.
                self.picked = if self.multiple { made.or(self.picked) } else { None };
                s.set_selection(self.picked.into_iter().collect());
                Ok(Step::Continue)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some(base) = self.distance_first
            && c.is_finite()
        {
            return vec![line(base, c)];
        }
        let (Some(h), Ok(d)) = (self.picked, s.doc()) else { return Vec::new() };
        let Some(e) = d.entity(h) else { return Vec::new() };
        let dist = match self.dist {
            Some(x) => x,
            None => offset_distance(&e.kind, c).unwrap_or_else(|| crate::select::entity_distance(d, e, c, 1e-3)),
        };
        offset_kinds(&e.kind, dist, c, GapType::from_index(d.header.i64("OFFSETGAPTYPE", 0)))
    }
}
