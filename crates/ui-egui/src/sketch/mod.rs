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

/// The sketch tool in use. `Dimension` is a toolbar button that says what is coming.
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
    /// The glyph of the (non-dimension) constraint with this index.
    Glyph(usize),
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
    /// Where the segment being drawn starts (a rectangle's first corner, a circle's centre, an
    /// arc's start, a fillet's corner).
    pub start: Option<Start>,
    /// The 3-Point Arc's end, once clicked.
    pub second: Option<Start>,
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
    /// A dimension being placed by the Sketch Dimension tool: what it measures (its parameter
    /// name is empty until the engine makes it) and the box, prefilled with the measured value.
    pub new_dim: Option<(Constraint, Hud)>,
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
            second: None,
            chain: None,
            chain_start: None,
            rubber: None,
            hover: None,
            selected: vec![],
            edit_dim: None,
            new_dim: None,
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
    /// Degrees of freedom left; `None` when the sketch does not solve.
    pub dof: Option<usize>,
}

fn build_cache(sketch: Sketch, rows: Vec<ParamRow>) -> Cache {
    let params = params::solve(&rows);
    let (constrained, point_constrained, errors, dof) = match solve::solve(&sketch, &params) {
        Ok(s) => (s.constrained, s.point_constrained, s.errors, Some(s.dof)),
        Err(_) => Default::default(),
    };
    Cache { sketch, rows, params, constrained, point_constrained, errors, dof }
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
    let was = m.tool;
    m.tool = tool;
    tools::end_chain(m);
    m.edit_dim = None;
    m.new_dim = None;
    if tool == SketchTool::Dimension || was == SketchTool::Dimension {
        m.selected.clear();
    }
    if tool == SketchTool::Fillet {
        tools::fillet_from_selection(app);
    }
}

/// The hint-bar text for the sketch tool in use (what to do next).
pub fn hint(app: &VectorcraftApp) -> Option<&'static str> {
    let m = app.sketch.as_ref()?;
    Some(match (m.tool, m.start.is_some(), m.second.is_some()) {
        (SketchTool::None, ..) => tl!("Pick a sketch tool, or click geometry to select it · Esc finishes the sketch"),
        (SketchTool::Line, ..) => tl!("Click to start a line · type a length, Tab, an angle, Enter · Esc ends the chain"),
        (SketchTool::Rect2, false, _) => tl!("Click the first corner · Esc cancels"),
        (SketchTool::RectCenter, false, _) => tl!("Click the centre · Esc cancels"),
        (SketchTool::Rect2 | SketchTool::RectCenter, true, _) => tl!("Click, or type a width, Tab, a height, Enter · Esc cancels"),
        (SketchTool::Circle, false, _) => tl!("Click the centre · Esc cancels"),
        (SketchTool::Circle, true, _) => tl!("Click, or type a diameter, Enter · Esc cancels"),
        (SketchTool::Arc3, false, _) => tl!("Click the arc's start · Esc cancels"),
        (SketchTool::Arc3, true, false) => tl!("Click the arc's end · Esc cancels"),
        (SketchTool::Arc3, true, true) => tl!("Click to set the bulge, or type a radius, Enter · Esc cancels"),
        (SketchTool::Fillet, false, _) => tl!("Click a corner where two lines meet · Esc cancels"),
        (SketchTool::Fillet, true, _) => tl!("Type a radius, Enter · Esc cancels"),
        (SketchTool::Dimension, ..) if m.new_dim.is_some() => {
            tl!("Type a value, Enter places the dimension · click another line for the angle · Esc cancels")
        }
        (SketchTool::Dimension, ..) if m.selected.is_empty() => tl!("Click a line, circle or arc, or a point · Esc leaves the tool"),
        (SketchTool::Dimension, ..) => tl!("Click a second point for the distance · Esc leaves the tool"),
    })
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
    // Select All is the sketch's: every entity of the sketch, and the page selection stays as it is.
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::A)) {
        select_all(app);
    }
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

/// Cmd+A: select every entity of the sketch.
fn select_all(app: &mut VectorcraftApp) {
    let Some(cache) = snapshot(app) else { return };
    if let Some(m) = app.sketch.as_mut() {
        m.selected = cache.sketch.entities.iter().map(|e| Pick::Entity(e.id())).collect();
    }
}

fn hud_up(app: &VectorcraftApp) -> bool {
    app.sketch.as_ref().is_some_and(|m| m.hud.is_some() || m.edit_dim.is_some() || m.new_dim.is_some())
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
    if m.edit_dim.take().is_some() || m.new_dim.take().is_some() {
        return;
    }
    if m.tool == SketchTool::Dimension && !m.selected.is_empty() {
        m.selected.clear();
        return;
    }
    if m.hud.as_ref().is_some_and(Hud::any_typed) {
        m.hud = Some(tools::new_hud(m.tool));
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
        New(HudOutcome),
        Line(HudOutcome),
        No,
    }
    let Some(cache) = snapshot(app) else { return false };
    let fed = {
        let Some(m) = app.sketch.as_mut() else { return false };
        if let Some((index, hud)) = m.edit_dim.as_mut() {
            Fed::Edit(*index, hud.handle(ev, &cache.params))
        } else if let Some((_, hud)) = m.new_dim.as_mut() {
            Fed::New(hud.handle(ev, &cache.params))
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
        Fed::New(out) => match out {
            HudOutcome::Commit(_) => commit_new_dim(app),
            HudOutcome::Cancel => {
                if let Some(m) = app.sketch.as_mut() {
                    m.new_dim = None;
                }
            }
            HudOutcome::Blocked(e) => app.status(e),
            HudOutcome::None => {}
        },
        Fed::Line(out) => match out {
            HudOutcome::Commit(texts) => tools::commit(app, texts),
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
            Pick::Dim(i) | Pick::Glyph(i) => dims.push(*i),
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
    dims.dedup();
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
pub fn toggle_construction(app: &mut VectorcraftApp) {
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
    if kind == "fix" {
        fix_toggle(app, &cache, &points, &entities);
        return;
    }
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
        _ => Err(match kind {
            "horizontal" => tl!("Select a line"),
            "perpendicular" | "parallel" => tl!("Select two lines"),
            "equal" => tl!("Select two lines, or two circles or arcs"),
            "tangent" => tl!("Select a line and a circle or arc, or two circles or arcs"),
            "concentric" => tl!("Select two circles or arcs"),
            "coincident" => tl!("Select two points, or a point and a line, circle or arc"),
            _ => tl!("Select a point and a line"),
        }
        .to_string()),
    };
    match built {
        Ok(constraint) => {
            if let Err(e) = app.run("sketch.addConstraint", json!({"id": id, "constraint": constraint})) {
                app.status(refusal(e));
            }
        }
        Err(e) => app.status(e),
    }
}

/// An engine refusal as the status line shows it: the over-constraint message stands alone.
fn refusal(e: String) -> String {
    const OVER: &str = "This would over-constrain the sketch";
    if e.contains(OVER) { tl!("This would over-constrain the sketch").to_string() } else { e }
}

/// Fix/Unfix: the selected points and the end points of the selected lines. Pins those that are
/// loose; when every one is already pinned, takes the pins away.
fn fix_toggle(app: &mut VectorcraftApp, cache: &Cache, points: &[u32], entities: &[u32]) {
    let mut targets: Vec<u32> = points.to_vec();
    for e in entities {
        if let Some(Entity::Line { a, b, .. }) = cache.sketch.entity(*e) {
            targets.extend([*a, *b]);
        }
    }
    targets.sort_unstable();
    targets.dedup();
    let Some(id) = app.sketch.as_ref().map(|m| m.id.0) else { return };
    if targets.is_empty() {
        app.status(tl!("Select a point or a line"));
        return;
    }
    let fixed_at = |p: u32| cache.sketch.constraints.iter().position(|c| matches!(c, Constraint::Fix { p: q, .. } if *q == p));
    let loose: Vec<u32> = targets.iter().copied().filter(|p| fixed_at(*p).is_none()).collect();
    if loose.is_empty() {
        let mut at: Vec<usize> = targets.iter().filter_map(|p| fixed_at(*p)).collect();
        at.sort_unstable_by(|a, b| b.cmp(a));
        for i in at {
            if let Err(e) = app.run("sketch.removeConstraint", json!({"id": id, "index": i})) {
                app.status(e);
                return;
            }
        }
        return;
    }
    for p in loose {
        if let Err(e) = app.run("sketch.addConstraint", json!({"id": id, "constraint": {"type": "fix", "p": p}})) {
            app.status(refusal(e));
            return;
        }
    }
}

// ---- the Sketch Dimension tool ----

/// The dimension `c` as `sketch.addDimension` takes it (the engine names the parameter).
fn dim_json(c: &Constraint) -> Value {
    match c {
        Constraint::Length { line, .. } => json!({"type": "length", "line": line}),
        Constraint::Distance { a, b, .. } => json!({"type": "distance", "a": a, "b": b}),
        Constraint::AngleBetween { a, b, .. } => json!({"type": "angleBetween", "a": a, "b": b}),
        Constraint::Radius { e, .. } => json!({"type": "radius", "e": e}),
        Constraint::Diameter { e, .. } => json!({"type": "diameter", "e": e}),
        _ => json!({}),
    }
}

/// Open the box for the new dimension `c`, prefilled with what it measures and selected.
fn open_dim(app: &mut VectorcraftApp, cache: &Cache, c: Constraint) {
    let (label, unit) = match &c {
        Constraint::AngleBetween { .. } => ("angle", hud::BoxUnit::Deg),
        Constraint::Radius { .. } => ("radius", hud::BoxUnit::Mm),
        Constraint::Diameter { .. } => ("diameter", hud::BoxUnit::Mm),
        _ => ("length", hud::BoxUnit::Mm),
    };
    let mut hud = Hud::new(&[(label, unit)]);
    hud.boxes[0].text = overlay::trim2(cache.sketch.measure(&c).unwrap_or(0.0));
    if let Some(m) = app.sketch.as_mut() {
        m.new_dim = Some((c, hud));
    }
}

/// Enter in a new dimension's box: `sketch.addDimension` with the box's text.
fn commit_new_dim(app: &mut VectorcraftApp) {
    let Some(m) = app.sketch.as_ref() else { return };
    let Some((c, hud)) = m.new_dim.as_ref() else { return };
    let (constraint, text, id) = (dim_json(c), hud.boxes[0].text.clone(), m.id.0);
    match app.run("sketch.addDimension", json!({"id": id, "constraint": constraint, "expression": text})) {
        Ok(_) => {
            if let Some(m) = app.sketch.as_mut() {
                m.new_dim = None;
                m.selected.clear();
            }
        }
        Err(e) => {
            let e = refusal(e);
            // A refused sketch changes nothing: the box goes, a bad number stays to be fixed.
            if e == tl!("This would over-constrain the sketch")
                && let Some(m) = app.sketch.as_mut()
            {
                m.new_dim = None;
                m.selected.clear();
            }
            app.status(e);
        }
    }
}

/// The Sketch Dimension tool: a line gives its length (a click on a second line turns that into
/// the angle between them), a circle its diameter, an arc its radius, two points their distance.
fn dim_pointer(app: &mut VectorcraftApp, cache: &Cache, xf: &Xf, pos: Pos2, input: PointerInput) {
    let pick = hit::pick(cache, xf, pos, false);
    let Some(m) = app.sketch.as_mut() else { return };
    m.hover = pick;
    if !input.clicked {
        return;
    }
    let sk = &cache.sketch;
    let kind = |pick: Pick| match pick {
        Pick::Entity(e) => sk.entity(e),
        _ => None,
    };
    let none = String::new;
    if let Some((Constraint::Length { line, .. }, _)) = &m.new_dim {
        if let Some(Pick::Entity(b)) = pick
            && b != *line
            && matches!(kind(Pick::Entity(b)), Some(Entity::Line { .. }))
        {
            let c = Constraint::AngleBetween { a: *line, b, param: none() };
            m.selected = vec![Pick::Entity(*line), Pick::Entity(b)];
            open_dim(app, cache, c);
            return;
        }
        m.new_dim = None;
        m.selected.clear();
        return;
    }
    if m.new_dim.is_some() {
        m.new_dim = None;
        m.selected.clear();
        return;
    }
    let first = match m.selected.as_slice() {
        [Pick::Point(a)] => Some(*a),
        _ => None,
    };
    let made = match (first, pick) {
        (Some(a), Some(Pick::Point(b))) if a != b => Some(Constraint::Distance { a, b, param: none() }),
        (_, Some(Pick::Entity(e))) => match kind(Pick::Entity(e)) {
            Some(Entity::Line { .. }) => Some(Constraint::Length { line: e, param: none() }),
            Some(Entity::Circle { .. }) => Some(Constraint::Diameter { e, param: none() }),
            Some(Entity::Arc { .. }) => Some(Constraint::Radius { e, param: none() }),
            None => None,
        },
        _ => None,
    };
    match (made, pick) {
        (Some(c), Some(p)) => {
            m.selected = match (&c, first) {
                (Constraint::Distance { .. }, Some(a)) => vec![Pick::Point(a), p],
                _ => vec![p],
            };
            open_dim(app, cache, c);
        }
        (None, Some(p @ Pick::Point(_))) => m.selected = vec![p],
        _ => m.selected.clear(),
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
    // The pointer's own facts, not the canvas response's: a press and release that arrive in one
    // frame (a double-click from automation, a fast trackpad) never register on a response.
    let (pos, pressed, released, clicked, double, shift) = ui.input(|i| {
        let b = egui::PointerButton::Primary;
        (
            i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()),
            i.pointer.button_pressed(b),
            i.pointer.button_released(b),
            i.pointer.button_clicked(b),
            i.pointer.button_double_clicked(b),
            i.modifiers.shift,
        )
    });
    let pos = pos.filter(|p| rect.contains(*p));
    // Over the canvas itself: the response says so, and so does nothing floating above the point
    // (a dialog or popup would take the click).
    let over = resp.hovered() || resp.dragged() || pos.is_some_and(|p| ui.ctx().layer_id_at(p) == Some(ui.layer_id()));
    let input = PointerInput { pos, pressed: pressed && over, released: released && over, clicked: clicked && over, double: double && over, shift };
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
        SketchTool::Rect2 | SketchTool::RectCenter | SketchTool::Circle | SketchTool::Arc3 => tools::shape_pointer(app, &cache, xf, pos, input),
        SketchTool::Fillet => tools::fillet_pointer(app, &cache, xf, pos, input),
        SketchTool::Dimension => dim_pointer(app, &cache, xf, pos, input),
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
