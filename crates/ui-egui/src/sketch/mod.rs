//! Sketch mode (Euclidean): drawing a parametric sketch on the canvas the way Fusion 360 does.
//!
//! The sketch group is isolated (everything else dims and cannot be picked), the sketch toolbar
//! replaces the Control bar, the keyboard belongs to the sketch (tool letters, heads-up value
//! boxes) and the pointer to the active sketch tool. All edits go through the `sketch.*` commands,
//! so they are undo steps and show up in the control channel like every other action.
//!
//! Coordinates here are millimetres in the sketch's frame (y down, relative to `Sketch.origin`);
//! [`to_screen`] and [`to_mm`] convert to and from screen points.

pub mod hit;
pub mod hud;
pub mod overlay;
pub mod params_dialog;
pub mod strip;
pub mod toolbar;
pub mod tools;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use egui::{Event, Key, Pos2};
use serde_json::{Value, json};
use vectorcraft_doc::expr::Solved;
use vectorcraft_doc::params::{self, ParamRow};
use vectorcraft_doc::sketch::model::{Constraint, Entity};
use vectorcraft_doc::sketch::{Sketch, solve};
use vectorcraft_doc::{NodeId, Unit};
use vectorcraft_geom::Point;

use crate::VectorcraftApp;
use crate::canvas::Xf;
use hud::{Hud, HudEvent, HudOutcome};

/// A place in the sketch, millimetres.
pub type Mm = [f64; 2];

/// The sketch tool in use. Only `None` (select) and `Line` draw in this phase; the others are
/// toolbar buttons that set the tool and say what is coming.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SketchTool {
    #[default]
    None,
    Line,
    Rect2,
    RectCenter,
    Circle,
    Arc3,
    Fillet,
    Dimension,
}

/// Something in the sketch the pointer can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Point(u32),
    Entity(u32),
    /// The value label of the dimension constraint with this index.
    Dim(usize),
}

/// What the pointer snapped to while drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Snap {
    #[default]
    None,
    /// An existing sketch point.
    Point(u32),
    /// Within 3 degrees of horizontal.
    Horizontal,
    /// Within 3 degrees of vertical.
    Vertical,
}

/// Where the segment being drawn starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Start {
    /// An existing point (reused: the coincidence of a chain).
    Point(u32),
    /// A new place.
    At(Mm),
}

/// The sketch being edited and what the user is in the middle of.
pub struct SketchMode {
    /// The sketch group.
    pub id: NodeId,
    /// The document (index in the session) it lives in.
    pub doc: usize,
    pub tool: SketchTool,
    /// The heads-up boxes of the segment being drawn.
    pub hud: Option<Hud>,
    /// Where the segment being drawn starts.
    pub start: Option<Start>,
    /// The end point of the last segment of the chain.
    pub chain: Option<u32>,
    /// The first point of the chain (clicking it closes the loop).
    pub chain_start: Option<u32>,
    /// The mouse, snapped, in sketch millimetres.
    pub rubber: Option<Mm>,
    pub hover: Option<Pick>,
    pub selected: Vec<Pick>,
    /// A dimension being edited in place: its constraint index and the box.
    pub edit_dim: Option<(usize, Hud)>,
    pub snap: Snap,
    /// The press that began this gesture came with a start already set (a release commits).
    pub armed: bool,
    cache: Option<Arc<Cache>>,
}

impl SketchMode {
    pub fn new(id: NodeId, doc: usize) -> SketchMode {
        SketchMode {
            id,
            doc,
            tool: SketchTool::None,
            hud: None,
            start: None,
            chain: None,
            chain_start: None,
            rubber: None,
            hover: None,
            selected: vec![],
            edit_dim: None,
            snap: Snap::None,
            armed: false,
            cache: None,
        }
    }
}

/// The sketch's recipe with its parameters and solver verdicts, solved again only when either
/// changed.
pub struct Cache {
    pub sketch: Sketch,
    pub rows: Vec<ParamRow>,
    pub params: Solved,
    /// Entities that are fully constrained.
    pub constrained: BTreeSet<u32>,
    /// Points that are fully constrained.
    pub point_constrained: BTreeSet<u32>,
    /// Constraint index to its dimension's error.
    pub errors: BTreeMap<usize, String>,
}

fn build_cache(sketch: Sketch, rows: Vec<ParamRow>) -> Cache {
    let params = params::solve(&rows);
    let (constrained, point_constrained, errors) = match solve::solve(&sketch, &params) {
        Ok(s) => (s.constrained, s.point_constrained, s.errors),
        Err(_) => Default::default(),
    };
    Cache { sketch, rows, params, constrained, point_constrained, errors }
}

/// Pixels per millimetre at this view.
pub fn px_per_mm(xf: &Xf) -> f64 {
    xf.zoom * Unit::Millimeters.to_pt(1.0)
}

/// Sketch millimetres to a screen point.
pub fn to_screen(xf: &Xf, sketch: &Sketch, p: Mm) -> Pos2 {
    xf.to_screen(Point::new(Unit::Millimeters.to_pt(p[0] + sketch.origin[0]), Unit::Millimeters.to_pt(p[1] + sketch.origin[1])))
}

/// A screen point to sketch millimetres.
pub fn to_mm(xf: &Xf, sketch: &Sketch, pos: Pos2) -> Mm {
    let d = xf.to_doc(pos);
    let mm = Unit::Millimeters.to_pt(1.0);
    [d.x / mm - sketch.origin[0], d.y / mm - sketch.origin[1]]
}

/// The sketch group's recipe and the document's parameter rows.
fn read(app: &VectorcraftApp, id: NodeId) -> Option<(Sketch, Vec<ParamRow>)> {
    let st = app.session.active()?;
    let sketch = st.doc.node(id)?.extras().sketch.as_deref()?.clone();
    Some((sketch, params::rows(&st.doc)))
}

/// Is `m` still a sketch being edited: its document is the active one, still isolated on the
/// sketch group, and the group is still a drawn sketch?
fn valid(app: &VectorcraftApp, m: &SketchMode) -> bool {
    app.session.active_index() == Some(m.doc)
        && app.session.active().is_some_and(|st| {
            st.isolation == Some(m.id) && st.doc.node(m.id).is_some_and(|n| n.extras().sketch.as_deref().is_some_and(Sketch::is_v2))
        })
}

/// Is sketch mode on (and its sketch still there)?
pub fn active(app: &VectorcraftApp) -> bool {
    app.sketch.as_ref().is_some_and(|m| valid(app, m))
}

/// Leave sketch mode when its sketch went away (undo of the new sketch, another document, an
/// isolation exit by other means).
pub fn sync(app: &mut VectorcraftApp) {
    if app.sketch.as_ref().is_some_and(|m| !valid(app, m)) {
        app.sketch = None;
    }
}

/// The recipe and verdicts (solved again when the recipe or parameters changed).
pub fn snapshot(app: &mut VectorcraftApp) -> Option<Arc<Cache>> {
    let id = app.sketch.as_ref()?.id;
    let (sketch, rows) = read(app, id)?;
    let m = app.sketch.as_mut()?;
    if let Some(c) = &m.cache
        && c.sketch == sketch
        && c.rows == rows
    {
        return Some(c.clone());
    }
    let c = Arc::new(build_cache(sketch, rows));
    m.cache = Some(c.clone());
    Some(c)
}

/// Enter sketch mode on sketch group `id`: isolate it (dimming and locking everything else).
pub fn enter(app: &mut VectorcraftApp, id: NodeId) -> Result<(), String> {
    app.run("object.isolate", json!({"id": id.0}))?;
    enter_mode(app, id);
    Ok(())
}

/// Sketch mode on `id`, which is already isolated.
fn enter_mode(app: &mut VectorcraftApp, id: NodeId) {
    if let Some(doc) = app.session.active_index() {
        app.sketch = Some(SketchMode::new(id, doc));
    }
}

/// Finish the sketch: leave isolation (which selects the sketch group again) and sketch mode.
pub fn finish(app: &mut VectorcraftApp) {
    let Some(m) = app.sketch.take() else { return };
    if valid(app, &m) {
        let _ = app.run("object.exitIsolation", json!({}));
    }
}

/// Object › Parametric Sketch › New Sketch: a new drawn sketch, entered at once.
pub fn new_sketch(app: &mut VectorcraftApp) -> Result<Value, String> {
    let r = app.run("sketch.new", json!({}))?;
    let id = r.get("id").and_then(Value::as_u64).ok_or("sketch.new returned no id")?;
    enter(app, NodeId(id))?;
    Ok(json!({"id": id}))
}

/// After a pointer event ran in the engine: a double-click that isolated a drawn sketch enters
/// sketch mode on it.
pub fn after_dispatch(app: &mut VectorcraftApp, ev: &vectorcraft_tools::PointerEvent) {
    if app.sketch.is_some() || ev.kind != vectorcraft_tools::PointerKind::DoubleClick {
        return;
    }
    let Some(iso) = app
        .session
        .active()
        .and_then(|st| st.isolation.filter(|i| st.doc.node(*i).is_some_and(|n| n.extras().sketch.as_deref().is_some_and(Sketch::is_v2))))
    else {
        return;
    };
    enter_mode(app, iso);
}

/// Select tool `tool` (ending what the old one was drawing).
pub fn set_tool(app: &mut VectorcraftApp, tool: SketchTool) {
    let Some(m) = app.sketch.as_mut() else { return };
    m.tool = tool;
    tools::end_chain(m);
    m.edit_dim = None;
    if !matches!(tool, SketchTool::None | SketchTool::Line) {
        app.status(tl!("Coming next"));
    }
}

// ---- keys ----

/// Let go of a focus egui moved on to a button (Tab does that): only a text field keeps it.
pub fn release_button_focus(ctx: &egui::Context) {
    if !ctx.text_edit_focused() {
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }
}

/// Take the keyboard: every character and every key without Command/Ctrl goes to sketch mode (the
/// events are removed, so the tools' letters, Tab, Enter, arrows and Delete never see them);
/// Command chords are left to the normal path.
pub fn keys(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let taken: Vec<Event> = ctx.input_mut(|i| {
        let mut taken = vec![];
        i.events.retain(|e| {
            let mine = match e {
                Event::Text(_) => true,
                Event::Key { modifiers, .. } => !modifiers.command && !modifiers.ctrl,
                _ => false,
            };
            if mine {
                taken.push(e.clone());
            }
            !mine
        });
        taken
    });
    for e in taken {
        match e {
            Event::Text(s) => s.chars().filter(|c| !c.is_control()).for_each(|c| character(app, c)),
            Event::Key { key, pressed: true, repeat, .. } => key_press(app, key, repeat),
            _ => {}
        }
    }
}

fn hud_up(app: &VectorcraftApp) -> bool {
    app.sketch.as_ref().is_some_and(|m| m.hud.is_some() || m.edit_dim.is_some())
}

/// A typed character: into the focused box while one is up, else a tool letter.
fn character(app: &mut VectorcraftApp, c: char) {
    if app.sketch.is_none() {
        return;
    }
    if hud_up(app) {
        feed(app, HudEvent::Char(c));
        return;
    }
    match c.to_ascii_lowercase() {
        'l' => set_tool(app, SketchTool::Line),
        'r' => set_tool(app, SketchTool::Rect2),
        'c' => set_tool(app, SketchTool::Circle),
        'd' => set_tool(app, SketchTool::Dimension),
        't' | 'o' => app.status(tl!("Coming next")),
        'x' => toggle_construction(app),
        _ => {}
    }
}

fn key_press(app: &mut VectorcraftApp, key: Key, repeat: bool) {
    if app.sketch.is_none() {
        return;
    }
    match key {
        Key::Escape if !repeat => escape(app),
        Key::Enter if !repeat => {
            feed(app, HudEvent::Enter);
        }
        Key::Tab if !repeat => {
            feed(app, HudEvent::Tab);
        }
        Key::Backspace | Key::Delete if !(hud_up(app) && feed(app, HudEvent::Backspace)) => delete_selection(app),
        _ => {}
    }
}

/// Escape, one level per press: a box being edited, what is being drawn (rubber band and chain),
/// the tool, the sketch.
fn escape(app: &mut VectorcraftApp) {
    let Some(m) = app.sketch.as_mut() else { return };
    if m.edit_dim.take().is_some() {
        return;
    }
    if m.hud.as_ref().is_some_and(Hud::any_typed) {
        m.hud = Some(tools::new_line_hud());
        return;
    }
    if m.start.is_some() || m.chain.is_some() {
        tools::end_chain(m);
        return;
    }
    if m.tool != SketchTool::None {
        m.tool = SketchTool::None;
        return;
    }
    finish(app);
}

/// Give `ev` to the box that has the keyboard. Returns whether a box took it.
pub(super) fn feed(app: &mut VectorcraftApp, ev: HudEvent) -> bool {
    enum Fed {
        Edit(usize, HudOutcome),
        Line(HudOutcome),
        No,
    }
    let Some(cache) = snapshot(app) else { return false };
    let fed = {
        let Some(m) = app.sketch.as_mut() else { return false };
        if let Some((index, hud)) = m.edit_dim.as_mut() {
            Fed::Edit(*index, hud.handle(ev, &cache.params))
        } else if let Some(hud) = m.hud.as_mut() {
            Fed::Line(hud.handle(ev, &cache.params))
        } else {
            Fed::No
        }
    };
    match fed {
        Fed::Edit(index, out) => match out {
            HudOutcome::Commit(texts) => commit_edit(app, index, texts.into_iter().next().flatten()),
            HudOutcome::Cancel => {
                if let Some(m) = app.sketch.as_mut() {
                    m.edit_dim = None;
                }
            }
            HudOutcome::Blocked(e) => app.status(e),
            HudOutcome::None => {}
        },
        Fed::Line(out) => match out {
            HudOutcome::Commit(texts) => tools::commit_line(app, texts),
            HudOutcome::Blocked(e) => app.status(e),
            HudOutcome::Cancel | HudOutcome::None => {}
        },
        Fed::No => return false,
    }
    true
}

/// Enter in the dimension's box: `sketch.setDimension`. An error stays in the status line and the
/// box stays open.
fn commit_edit(app: &mut VectorcraftApp, index: usize, text: Option<String>) {
    let Some(text) = text else {
        if let Some(m) = app.sketch.as_mut() {
            m.edit_dim = None;
        }
        return;
    };
    let Some(id) = app.sketch.as_ref().map(|m| m.id) else { return };
    match app.run("sketch.setDimension", json!({"id": id.0, "index": index, "expression": text})) {
        Ok(r) => match r.get("error").and_then(Value::as_str) {
            Some(e) if !e.is_empty() => app.status(e.to_string()),
            _ => {
                if let Some(m) = app.sketch.as_mut() {
                    m.edit_dim = None;
                }
            }
        },
        Err(e) => app.status(e),
    }
}

/// Start editing dimension `index` in place: a one-box heads-up prefilled with its expression,
/// selected (the first character typed replaces it).
pub fn begin_edit(app: &mut VectorcraftApp, index: usize) {
    let Some(cache) = snapshot(app) else { return };
    let Some(c) = cache.sketch.constraints.get(index) else { return };
    let Some(param) = c.param() else { return };
    let label = match c {
        Constraint::Angle { .. } | Constraint::AngleBetween { .. } => "angle",
        Constraint::Radius { .. } => "radius",
        Constraint::Diameter { .. } => "diameter",
        _ => "length",
    };
    let unit = if c.is_angular() { hud::BoxUnit::Deg } else { hud::BoxUnit::Mm };
    let mut hud = Hud::new(&[(label, unit)]);
    hud.boxes[0].text = cache.rows.iter().find(|r| r.name == param).map(|r| r.expression.clone()).unwrap_or_default();
    if let Some(m) = app.sketch.as_mut() {
        m.edit_dim = Some((index, hud));
        m.hud = None;
        tools::end_chain(m);
    }
}

fn selected_ids(m: &SketchMode) -> (Vec<u32>, Vec<u32>, Vec<usize>) {
    let (mut points, mut entities, mut dims) = (vec![], vec![], vec![]);
    for p in &m.selected {
        match p {
            Pick::Point(i) => points.push(*i),
            Pick::Entity(i) => entities.push(*i),
            Pick::Dim(i) => dims.push(*i),
        }
    }
    (points, entities, dims)
}

/// Delete/Backspace: the selected geometry, and the selected dimensions.
fn delete_selection(app: &mut VectorcraftApp) {
    let Some(m) = app.sketch.as_ref() else { return };
    let (_, entities, mut dims) = selected_ids(m);
    let id = m.id.0;
    if !entities.is_empty() {
        if let Err(e) = app.run("sketch.deleteEntities", json!({"id": id, "entities": entities})) {
            app.status(e);
        }
        // Constraint numbers moved with the deletion.
        dims.clear();
    }
    // Highest first: removing one renumbers those after it.
    dims.sort_unstable_by(|a, b| b.cmp(a));
    for d in dims {
        if let Err(e) = app.run("sketch.removeConstraint", json!({"id": id, "index": d})) {
            app.status(e);
        }
    }
    if let Some(m) = app.sketch.as_mut() {
        m.selected.clear();
        m.hover = None;
    }
}

/// X: toggle construction on the selected entities.
fn toggle_construction(app: &mut VectorcraftApp) {
    let Some(m) = app.sketch.as_ref() else { return };
    let (_, entities, _) = selected_ids(m);
    if entities.is_empty() {
        app.status(tl!("Select a line, circle or arc first"));
        return;
    }
    let id = m.id.0;
    if let Err(e) = app.run("sketch.setConstruction", json!({"id": id, "entities": entities})) {
        app.status(e);
    }
}

// ---- constraints from the selection ----

/// A constraint button: add `kind` from the selection when it fits, else say what to select.
pub fn constrain(app: &mut VectorcraftApp, kind: &str) {
    let Some(cache) = snapshot(app) else { return };
    let Some(m) = app.sketch.as_ref() else { return };
    let (points, entities, _) = selected_ids(m);
    let id = m.id.0;
    let entity = |i: u32| cache.sketch.entity(i);
    let is_line = |i: u32| matches!(entity(i), Some(Entity::Line { .. }));
    let is_round = |i: u32| matches!(entity(i), Some(Entity::Circle { .. } | Entity::Arc { .. }));
    let built: Result<Value, String> = match (kind, points.as_slice(), entities.as_slice()) {
        ("horizontal", [], [l]) if is_line(*l) => {
            let (dx, dy) = line_span(&cache.sketch, *l);
            Ok(json!({"type": if dx.abs() >= dy.abs() { "horizontal" } else { "vertical" }, "line": l}))
        }
        ("perpendicular" | "parallel", [], [a, b]) if is_line(*a) && is_line(*b) => Ok(json!({"type": kind, "a": a, "b": b})),
        ("equal", [], [a, b]) if (is_line(*a) && is_line(*b)) || (is_round(*a) && is_round(*b)) => Ok(json!({"type": kind, "a": a, "b": b})),
        ("tangent", [], [a, b]) if (is_line(*a) && is_round(*b)) || (is_round(*a) && is_line(*b)) || (is_round(*a) && is_round(*b)) => {
            Ok(json!({"type": kind, "a": a, "b": b}))
        }
        ("concentric", [], [a, b]) if is_round(*a) && is_round(*b) => Ok(json!({"type": kind, "a": a, "b": b})),
        ("coincident", [a, b], []) => Ok(json!({"type": kind, "a": a, "b": b})),
        ("coincident", [p], [e]) if is_line(*e) => Ok(json!({"type": "pointOnLine", "p": p, "line": e})),
        ("coincident", [p], [e]) if is_round(*e) => Ok(json!({"type": "pointOnCircle", "p": p, "e": e})),
        ("midpoint", [p], [l]) if is_line(*l) => Ok(json!({"type": kind, "p": p, "line": l})),
        ("fix", [p], []) => Ok(json!({"type": kind, "p": p})),
        _ => Err(match kind {
            "horizontal" => tl!("Select a line"),
            "perpendicular" | "parallel" => tl!("Select two lines"),
            "equal" => tl!("Select two lines, or two circles or arcs"),
            "tangent" => tl!("Select a line and a circle or arc, or two circles or arcs"),
            "concentric" => tl!("Select two circles or arcs"),
            "coincident" => tl!("Select two points, or a point and a line, circle or arc"),
            "midpoint" => tl!("Select a point and a line"),
            _ => tl!("Select a point"),
        }
        .to_string()),
    };
    match built {
        Ok(constraint) => {
            if let Err(e) = app.run("sketch.addConstraint", json!({"id": id, "constraint": constraint})) {
                app.status(e);
            }
        }
        Err(e) => app.status(e),
    }
}

fn line_span(sketch: &Sketch, line: u32) -> (f64, f64) {
    if let Some(Entity::Line { a, b, .. }) = sketch.entity(line)
        && let (Some(p), Some(q)) = (sketch.point(*a), sketch.point(*b))
    {
        return (q.x - p.x, q.y - p.y);
    }
    (1.0, 0.0)
}

// ---- pointer ----

/// What the pointer did this frame over the canvas.
#[derive(Clone, Copy, Debug, Default)]
pub struct PointerInput {
    pub pos: Option<Pos2>,
    pub pressed: bool,
    pub released: bool,
    pub clicked: bool,
    pub double: bool,
    pub shift: bool,
}

/// The canvas hands the pointer to sketch mode.
pub fn pointer(app: &mut VectorcraftApp, ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect, xf: &Xf) {
    sync(app);
    if app.sketch.is_none() {
        return;
    }
    let (pos, pressed, released, shift) = ui.input(|i| {
        (i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()), i.pointer.primary_pressed(), i.pointer.primary_released(), i.modifiers.shift)
    });
    let pos = pos.filter(|p| rect.contains(*p));
    let over = resp.hovered() || resp.dragged();
    let input =
        PointerInput { pos, pressed: pressed && over, released: released && over, clicked: resp.clicked(), double: resp.double_clicked(), shift };
    handle_pointer(app, xf, input);
    if pos.is_some() {
        ui.ctx().request_repaint();
    }
}

/// [`pointer`] without egui: the frame's pointer facts.
pub fn handle_pointer(app: &mut VectorcraftApp, xf: &Xf, input: PointerInput) {
    let Some(cache) = snapshot(app) else { return };
    let Some(m) = app.sketch.as_mut() else { return };
    if input.pressed && m.edit_dim.is_some() {
        m.edit_dim = None;
    }
    let Some(pos) = input.pos else {
        m.hover = None;
        return;
    };
    match m.tool {
        SketchTool::Line => tools::line_pointer(app, &cache, xf, pos, input),
        SketchTool::None => select_pointer(app, &cache, xf, pos, input),
        _ => {
            m.hover = hit::pick(&cache, xf, pos, m.tool == SketchTool::None);
        }
    }
}

/// The select tool: hover, click to select (Shift adds), click on nothing to clear, double-click a
/// dimension to edit it.
fn select_pointer(app: &mut VectorcraftApp, cache: &Cache, xf: &Xf, pos: Pos2, input: PointerInput) {
    let pick = hit::pick(cache, xf, pos, true);
    if input.double {
        if let Some(Pick::Dim(i)) = pick {
            begin_edit(app, i);
        }
        return;
    }
    let Some(m) = app.sketch.as_mut() else { return };
    m.hover = pick;
    if input.clicked {
        match (pick, input.shift) {
            (Some(p), true) => {
                if let Some(at) = m.selected.iter().position(|s| *s == p) {
                    m.selected.remove(at);
                } else {
                    m.selected.push(p);
                }
            }
            (Some(p), false) => m.selected = vec![p],
            (None, false) => m.selected.clear(),
            (None, true) => {}
        }
    }
}

/// The overlay pass of the canvas: geometry, rubber band, heads-up boxes, dimensions.
pub fn overlay(app: &mut VectorcraftApp, painter: &egui::Painter, xf: &Xf, ui: &egui::Ui) {
    sync(app);
    if app.sketch.is_none() {
        return;
    }
    let Some(cache) = snapshot(app) else { return };
    let t = crate::theme::Tokens::get(ui.ctx());
    overlay::paint(app, painter, xf, &cache, &t);
}
