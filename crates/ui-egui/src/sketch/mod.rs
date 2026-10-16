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
use vectorcraft_doc::sketch::model::{Along, Constraint, Entity};
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
    Trim,
    Offset,
    Mirror,
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
    /// A dimension being made by the Sketch Dimension tool: what it measures (its parameter
    /// name is empty until the engine makes it) and the box, prefilled with the measured value.
    /// It follows the cursor until [`SketchMode::dim_place`] is set.
    pub new_dim: Option<(Constraint, Hud)>,
    /// Where the new dimension's value was placed (sketch mm): a click set it and the box opened.
    pub dim_place: Option<Mm>,
    /// A dimension's value being dragged to a new place.
    pub drag_dim: Option<DimDrag>,
    /// The engine refused the new dimension as over-constraining: offer it as a driven one.
    pub driven_offer: Option<DrivenOffer>,
    /// Show dimensions and constraint glyphs (the eye in the toolbar).
    pub show_dims: bool,
    pub snap: Snap,
    /// The lines (or shapes, for Mirror) Offset and Mirror work on.
    pub modify: Vec<u32>,
    /// Offset's side: 1 is left of the first line as drawn, -1 right.
    pub side: f64,
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
            dim_place: None,
            drag_dim: None,
            driven_offer: None,
            show_dims: true,
            snap: Snap::None,
            modify: vec![],
            side: 1.0,
            armed: false,
            cache: None,
        }
    }
}

/// A dimension's value being dragged: the constraint, where the press was (screen), and where the
/// value is now (sketch mm) once the pointer has moved.
#[derive(Clone, Debug, PartialEq)]
pub struct DimDrag {
    pub index: usize,
    pub from: Pos2,
    pub at: Option<Mm>,
}

/// A refused dimension the user may keep as a driven (reference) one.
#[derive(Clone, Debug, PartialEq)]
pub struct DrivenOffer {
    pub constraint: Constraint,
    pub at: Option<Mm>,
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

/// Is `id` a finished drawn (v2) sketch group in the active document?
fn is_sketch(app: &VectorcraftApp, id: NodeId) -> bool {
    app.session.active().is_some_and(|st| st.doc.node(id).is_some_and(|n| n.extras().sketch.as_deref().is_some_and(Sketch::is_v2)))
}

/// The sketch group "Edit Sketch" offers: the one selected object, when it is a drawn sketch
/// that sketch mode is not already on.
pub fn editable(app: &VectorcraftApp) -> Option<NodeId> {
    let st = app.session.active()?;
    let [id] = st.selection.objects.as_slice() else { return None };
    (is_sketch(app, *id) && !app.sketch.as_ref().is_some_and(|m| m.id == *id && valid(app, m))).then_some(*id)
}

/// `sketch.edit.open {id?}`: re-enter sketch mode on a finished sketch (the selected one when
/// `id` is left out), exactly as the double-click does. A sketch being edited is finished first.
pub fn open(app: &mut VectorcraftApp, params: &Value) -> Result<Value, String> {
    let id = match params.get("id") {
        Some(v) => NodeId(v.as_u64().ok_or("select one sketch, or pass `id`")?),
        None => match app.session.active().map(|st| st.selection.objects.as_slice()) {
            Some([one]) => *one,
            _ => return Err("select one sketch, or pass `id`".into()),
        },
    };
    if !is_sketch(app, id) {
        return Err(format!("object {} is not a drawn sketch", id.0));
    }
    if active(app) && app.sketch.as_ref().is_some_and(|m| m.id == id) {
        return Ok(json!({"id": id.0}));
    }
    finish(app);
    enter(app, id)?;
    Ok(json!({"id": id.0}))
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
    m.dim_place = None;
    m.driven_offer = None;
    if tool == SketchTool::Dimension || was == SketchTool::Dimension {
        m.selected.clear();
    }
    match tool {
        SketchTool::Fillet => tools::fillet_from_selection(app),
        SketchTool::Offset => tools::offset_from_selection(app),
        SketchTool::Mirror => tools::mirror_from_selection(app),
        _ => {}
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
        (SketchTool::Dimension, ..) if m.driven_offer.is_some() => tl!("Enter keeps it as a driven dimension · Esc drops it"),
        (SketchTool::Dimension, ..) if m.new_dim.is_some() && m.dim_place.is_some() => {
            tl!("Type a value, an expression or a parameter name, Enter applies · Esc keeps the measured value")
        }
        (SketchTool::Dimension, ..) if m.new_dim.is_some() => {
            tl!("Move to where the dimension goes and click to place it · click another line for the angle, a point for an offset")
        }
        (SketchTool::Dimension, ..) if m.selected.is_empty() => {
            tl!("Click a line, circle or arc, or a point · drag a value to move it, double-click to edit · Esc leaves the tool")
        }
        (SketchTool::Dimension, ..) => {
            tl!("Click a second point (horizontal, vertical or aligned by where you place it) or a line · Esc starts over")
        }
        (SketchTool::Trim, ..) => tl!("Click the part of a line to remove · Esc leaves the tool"),
        (SketchTool::Offset, ..) if m.modify.is_empty() => tl!("Click a line to offset its whole chain · Esc leaves the tool"),
        (SketchTool::Offset, ..) => tl!("Move the mouse to the side you want, or type a distance, Enter · Esc cancels"),
        (SketchTool::Mirror, ..) => tl!("Click the mirror line · Esc leaves the tool"),
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
        't' => set_tool(app, SketchTool::Trim),
        'o' => set_tool(app, SketchTool::Offset),
        'x' => toggle_construction(app),
        _ => {}
    }
}

fn key_press(app: &mut VectorcraftApp, key: Key, repeat: bool) {
    if app.sketch.is_none() {
        return;
    }
    if app.sketch.as_ref().is_some_and(|m| m.driven_offer.is_some()) {
        match key {
            Key::Enter if !repeat => accept_driven(app),
            Key::Escape if !repeat => {
                if let Some(m) = app.sketch.as_mut() {
                    m.driven_offer = None;
                }
            }
            _ => {}
        }
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
    // A placed dimension is kept at its measured value (Fusion): Esc only closes the box.
    if m.new_dim.is_some() && m.dim_place.is_some() {
        commit_new_dim(app, true);
        return;
    }
    if m.edit_dim.take().is_some() || m.new_dim.take().is_some() {
        m.dim_place = None;
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
    if m.start.is_some() || m.chain.is_some() || !m.modify.is_empty() {
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
            HudOutcome::Commit(_) => commit_new_dim(app, false),
            HudOutcome::Cancel => {
                if let Some(m) = app.sketch.as_mut() {
                    m.new_dim = None;
                    m.dim_place = None;
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

// ---- the Sketch Dimension tool (E1 Part D: Fusion's Sketch Dimension) ----

/// The dimension `c` as `sketch.addDimension` takes it (the engine names the parameter).
fn dim_json(c: &Constraint) -> Value {
    match c {
        Constraint::Length { line, .. } => json!({"type": "length", "line": line}),
        Constraint::Distance { a, b, along, .. } => json!({"type": "distance", "a": a, "b": b, "along": along}),
        Constraint::DistancePointLine { p, line, .. } => json!({"type": "distancePointLine", "p": p, "line": line}),
        Constraint::AngleBetween { a, b, .. } => json!({"type": "angleBetween", "a": a, "b": b}),
        Constraint::Radius { e, .. } => json!({"type": "radius", "e": e}),
        Constraint::Diameter { e, .. } => json!({"type": "diameter", "e": e}),
        _ => json!({}),
    }
}

/// The box for a dimension of kind `c`: label and unit.
fn dim_box(c: &Constraint) -> (&'static str, hud::BoxUnit) {
    match c {
        Constraint::Angle { .. } | Constraint::AngleBetween { .. } => ("angle", hud::BoxUnit::Deg),
        Constraint::Radius { .. } => ("radius", hud::BoxUnit::Mm),
        Constraint::Diameter { .. } => ("diameter", hud::BoxUnit::Mm),
        Constraint::Distance { along: Along::X, .. } => ("horizontal", hud::BoxUnit::Mm),
        Constraint::Distance { along: Along::Y, .. } => ("vertical", hud::BoxUnit::Mm),
        Constraint::Distance { .. } | Constraint::DistancePointLine { .. } => ("distance", hud::BoxUnit::Mm),
        _ => ("length", hud::BoxUnit::Mm),
    }
}

/// Start making the dimension `c`: it follows the cursor until a click places it.
fn start_dim(app: &mut VectorcraftApp, cache: &Cache, c: Constraint) {
    let (label, unit) = dim_box(&c);
    let mut hud = Hud::new(&[(label, unit)]);
    hud.boxes[0].text = overlay::trim2(cache.sketch.measure(&c).unwrap_or(0.0));
    if let Some(m) = app.sketch.as_mut() {
        m.new_dim = Some((c, hud));
        m.dim_place = None;
    }
}

/// Which way a point-to-point dimension measures, from where the cursor is (Fusion): above or
/// below the pair gives the horizontal distance, beside it the vertical, anywhere else the
/// aligned one. Points on one horizontal or vertical always measure along it.
pub fn along_for(sk: &Sketch, a: u32, b: u32, cursor: Mm) -> Along {
    let (Some(p), Some(q)) = (sk.point(a), sk.point(b)) else { return Along::Direct };
    let (dx, dy) = ((q.x - p.x).abs(), (q.y - p.y).abs());
    if dy < 1e-6 {
        return Along::X;
    }
    if dx < 1e-6 {
        return Along::Y;
    }
    let (x0, x1) = (p.x.min(q.x), p.x.max(q.x));
    let (y0, y1) = (p.y.min(q.y), p.y.max(q.y));
    let in_x = (x0..=x1).contains(&cursor[0]);
    let in_y = (y0..=y1).contains(&cursor[1]);
    match (in_x, in_y) {
        (true, false) => Along::X,
        (false, true) => Along::Y,
        _ => Along::Direct,
    }
}

/// Enter in a new dimension's box (or Esc, `measured`: the box's text is ignored):
/// `sketch.addDimension` with the box's text and the place. A dimension the solver refuses is
/// offered as a driven one.
fn commit_new_dim(app: &mut VectorcraftApp, measured: bool) {
    let Some(m) = app.sketch.as_ref() else { return };
    let Some((c, hud)) = m.new_dim.as_ref() else { return };
    let (constraint, text, id, at) = (dim_json(c), hud.boxes[0].text.clone(), m.id.0, m.dim_place);
    let mut params = json!({"id": id, "constraint": constraint});
    if !measured && hud.boxes[0].typed {
        params["expression"] = json!(text);
    }
    if let Some(at) = at {
        params["at"] = json!(at);
    }
    match app.run("sketch.addDimension", params) {
        Ok(_) => {
            if let Some(m) = app.sketch.as_mut() {
                m.new_dim = None;
                m.dim_place = None;
                m.selected.clear();
            }
        }
        Err(e) => {
            let e = refusal(e);
            // A refused sketch changes nothing. Over-constrained: offer a driven dimension; a bad
            // number stays in the box to be fixed.
            if e == tl!("This would over-constrain the sketch")
                && let Some(m) = app.sketch.as_mut()
                && let Some((c, _)) = m.new_dim.take()
            {
                m.driven_offer = Some(DrivenOffer { constraint: c, at: m.dim_place.take() });
                m.selected.clear();
            }
            app.status(e);
        }
    }
}

/// Keep the refused dimension as a driven (reference) one: it reads the geometry, in brackets.
pub fn accept_driven(app: &mut VectorcraftApp) {
    let Some(m) = app.sketch.as_mut() else { return };
    let Some(offer) = m.driven_offer.take() else { return };
    let id = m.id.0;
    let mut params = json!({"id": id, "constraint": dim_json(&offer.constraint), "driven": true});
    if let Some(at) = offer.at {
        params["at"] = json!(at);
    }
    match app.run("sketch.addDimension", params) {
        Ok(_) => app.status(tl!("Driven dimension added: it reads the geometry")),
        Err(e) => app.status(refusal(e)),
    }
}

/// Dragging a dimension's value (the Dimension and Select tools): a press on a value starts it,
/// the value follows while the button is down, the release runs `sketch.placeDimension`. Returns
/// whether the pointer was a drag's (a press and release that did not move fall through as a
/// click).
fn drag_dim_pointer(app: &mut VectorcraftApp, cache: &Cache, xf: &Xf, pos: Pos2, input: PointerInput) -> bool {
    let Some(m) = app.sketch.as_mut() else { return false };
    let id = m.id.0;
    if let Some(d) = m.drag_dim.as_mut() {
        if input.down {
            if d.at.is_some() || pos.distance(d.from) > 3.0 {
                d.at = Some(to_mm(xf, &cache.sketch, pos));
            }
            return true;
        }
        let d = m.drag_dim.take().unwrap_or(DimDrag { index: 0, from: pos, at: None });
        if let Some(at) = d.at {
            if let Err(e) = app.run("sketch.placeDimension", json!({"id": id, "index": d.index, "at": at})) {
                app.status(e);
            }
            return true;
        }
        return false;
    }
    // A press that is still held starts a drag (a press and release in one frame, or a
    // double-click, is a click).
    if input.pressed
        && input.down
        && !input.double
        && m.show_dims
        && let Some(i) = hit::dim_at(cache, xf, pos)
    {
        m.drag_dim = Some(DimDrag { index: i, from: pos, at: None });
        return true;
    }
    false
}

/// The Sketch Dimension tool. Pick a line for its length (a second line turns that into the angle
/// between them, a point into the point's offset from the line), a circle for its diameter (Shift:
/// radius), an arc for its radius, two points for their distance (horizontal, vertical or aligned
/// by where the cursor goes), a point and a line for the offset. The dimension follows the cursor;
/// a click places it and opens its box. A value can be dragged, and double-clicked to edit.
fn dim_pointer(app: &mut VectorcraftApp, cache: &Cache, xf: &Xf, pos: Pos2, input: PointerInput) {
    if drag_dim_pointer(app, cache, xf, pos, input) {
        return;
    }
    let sk = &cache.sketch;
    let cursor = to_mm(xf, sk, pos);
    let Some(m) = app.sketch.as_mut() else { return };
    m.rubber = Some(cursor);
    // The box is open on a placed dimension: a click elsewhere applies it as it stands.
    if m.new_dim.is_some() && m.dim_place.is_some() {
        if input.clicked {
            commit_new_dim(app, false);
        }
        return;
    }
    if input.double {
        if let Some(i) = hit::dim_at(cache, xf, pos) {
            begin_edit(app, i);
        }
        return;
    }
    let pick = hit::pick(cache, xf, pos, false);
    m.hover = pick;
    // A point-to-point dimension changes kind with the cursor until it is placed.
    if let Some((Constraint::Distance { a, b, along, .. }, hud)) = m.new_dim.as_mut() {
        let now = along_for(sk, *a, *b, cursor);
        if now != *along {
            *along = now;
            let c = Constraint::Distance { a: *a, b: *b, along: now, param: String::new() };
            let (label, unit) = dim_box(&c);
            *hud = Hud::new(&[(label, unit)]);
            hud.boxes[0].text = overlay::trim2(sk.measure(&c).unwrap_or(0.0));
        }
    }
    if !input.clicked {
        return;
    }
    let kind = |e: u32| sk.entity(e);
    let is_line = |e: u32| matches!(kind(e), Some(Entity::Line { .. }));
    let none = String::new;
    if let Some((c, _)) = m.new_dim.as_ref() {
        // Following the cursor: a second pick refines the kind; anything else places it.
        let refined = match (c, pick) {
            (Constraint::Length { line, .. }, Some(Pick::Entity(b))) if b != *line && is_line(b) => {
                // The angle is read the way round that makes it positive (Fusion shows the
                // interior angle).
                let mut c = Constraint::AngleBetween { a: *line, b, param: none() };
                if sk.measure(&c).is_some_and(|v| v < 0.0) {
                    c = Constraint::AngleBetween { a: b, b: *line, param: none() };
                }
                Some((c, vec![Pick::Entity(*line), Pick::Entity(b)]))
            }
            (Constraint::Length { line, .. }, Some(Pick::Point(p))) => {
                Some((Constraint::DistancePointLine { p, line: *line, param: none() }, vec![Pick::Point(p), Pick::Entity(*line)]))
            }
            _ => None,
        };
        match refined {
            Some((c, selected)) => {
                m.selected = selected;
                start_dim(app, cache, c);
            }
            None => {
                m.dim_place = Some(cursor);
                let measured = sk.measure(c).unwrap_or(0.0);
                if let Some((_, hud)) = m.new_dim.as_mut() {
                    hud.boxes[0].text = overlay::trim2(measured);
                    hud.boxes[0].typed = false;
                }
            }
        }
        return;
    }
    let first = match m.selected.as_slice() {
        [Pick::Point(a)] => Some(*a),
        _ => None,
    };
    let made = match (first, pick) {
        (Some(a), Some(Pick::Point(b))) if a != b => Some(Constraint::Distance { a, b, along: along_for(sk, a, b, cursor), param: none() }),
        (Some(a), Some(Pick::Entity(l))) if is_line(l) => Some(Constraint::DistancePointLine { p: a, line: l, param: none() }),
        (_, Some(Pick::Entity(e))) => match kind(e) {
            Some(Entity::Line { .. }) => Some(Constraint::Length { line: e, param: none() }),
            Some(Entity::Circle { .. }) if input.shift => Some(Constraint::Radius { e, param: none() }),
            Some(Entity::Circle { .. }) => Some(Constraint::Diameter { e, param: none() }),
            Some(Entity::Arc { .. }) => Some(Constraint::Radius { e, param: none() }),
            None => None,
        },
        _ => None,
    };
    match (made, pick) {
        (Some(c), Some(p)) => {
            m.selected = match first {
                Some(a) if matches!(c, Constraint::Distance { .. } | Constraint::DistancePointLine { .. }) => vec![Pick::Point(a), p],
                _ => vec![p],
            };
            start_dim(app, cache, c);
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
    /// The primary button is held.
    pub down: bool,
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
    let (pos, pressed, down, released, clicked, double, shift) = ui.input(|i| {
        let b = egui::PointerButton::Primary;
        (
            i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()),
            i.pointer.button_pressed(b),
            i.pointer.button_down(b),
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
    let input =
        PointerInput { pos, pressed: pressed && over, down, released: released && over, clicked: clicked && over, double: double && over, shift };
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
        SketchTool::None => {
            if !drag_dim_pointer(app, &cache, xf, pos, input) {
                select_pointer(app, &cache, xf, pos, input);
            }
        }
        SketchTool::Rect2 | SketchTool::RectCenter | SketchTool::Circle | SketchTool::Arc3 => tools::shape_pointer(app, &cache, xf, pos, input),
        SketchTool::Fillet => tools::fillet_pointer(app, &cache, xf, pos, input),
        SketchTool::Dimension => dim_pointer(app, &cache, xf, pos, input),
        SketchTool::Trim => tools::trim_pointer(app, &cache, xf, pos, input),
        SketchTool::Offset => tools::offset_pointer(app, &cache, xf, pos, input),
        SketchTool::Mirror => tools::mirror_pointer(app, &cache, xf, pos, input),
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
    driven_offer_ui(app, ui, xf, &cache);
}

/// The over-constraint prompt (Fusion's): the refused dimension can be kept as a driven one.
fn driven_offer_ui(app: &mut VectorcraftApp, ui: &egui::Ui, xf: &Xf, cache: &Cache) {
    let Some(offer) = app.sketch.as_ref().and_then(|m| m.driven_offer.clone()) else { return };
    let at = overlay::preview_geom(cache, xf, &offer.constraint, offer.at).map_or_else(|| ui.max_rect().center(), |g| g.label);
    let mut keep = false;
    let mut drop = false;
    egui::Area::new(egui::Id::new("sketch-driven-offer")).order(egui::Order::Foreground).fixed_pos(at + egui::vec2(12.0, 12.0)).show(
        ui.ctx(),
        |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(260.0);
                ui.label(tl!("This dimension would over-constrain the sketch."));
                ui.label(egui::RichText::new(tl!("Keep it as a driven dimension? It reads the geometry and is shown in brackets.")).small());
                ui.horizontal(|ui| {
                    keep = ui.button(tl!("Driven dimension")).clicked();
                    drop = ui.button(tl!("Cancel")).clicked();
                });
            });
        },
    );
    if keep {
        accept_driven(app);
    } else if drop && let Some(m) = app.sketch.as_mut() {
        m.driven_offer = None;
    }
}

/// The eye: show or hide the dimensions and constraint glyphs.
pub fn toggle_dims(app: &mut VectorcraftApp) {
    if let Some(m) = app.sketch.as_mut() {
        m.show_dims = !m.show_dims;
    }
}
