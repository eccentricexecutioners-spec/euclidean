//! The sketch tools: Line, the rectangles, the circle, the 3-point arc and Fillet.

use egui::Pos2;
use serde_json::{Value, json};
use vectorcraft_doc::expr::Solved;
use vectorcraft_doc::sketch::Sketch;
use vectorcraft_doc::sketch::model::Entity;

use super::hit::{self, POINT_SNAP};
use super::hud::{BoxUnit, Hud, HudEvent};
use super::{Cache, Mm, Pick, PointerInput, SketchMode, SketchTool, Snap, Start, snapshot, to_mm, to_screen};
use crate::VectorcraftApp;
use crate::canvas::Xf;

/// The length and angle boxes of a line being drawn.
pub fn new_line_hud() -> Hud {
    Hud::new(&[("length", BoxUnit::Mm), ("angle", BoxUnit::Deg)])
}

/// The boxes of a fresh gesture of `tool`.
pub fn new_hud(tool: SketchTool) -> Hud {
    match tool {
        SketchTool::Line => new_line_hud(),
        SketchTool::Rect2 | SketchTool::RectCenter => Hud::new(&[("width", BoxUnit::Mm), ("height", BoxUnit::Mm)]),
        SketchTool::Circle => Hud::new(&[("diameter", BoxUnit::Mm)]),
        SketchTool::Arc3 | SketchTool::Fillet => Hud::new(&[("radius", BoxUnit::Mm)]),
        SketchTool::Offset => Hud::new(&[("distance", BoxUnit::Mm)]),
        SketchTool::None | SketchTool::Dimension | SketchTool::Trim | SketchTool::Mirror => Hud::new(&[]),
    }
}

/// Box `i`'s typed (or locked) text as a number, when it has one that evaluates.
pub fn typed(hud: &Hud, i: usize, solved: &Solved) -> Option<f64> {
    hud.value(i, solved).and_then(Result::ok)
}

/// The place `len` mm from `from` at `angle` degrees (counter-clockwise on screen).
pub fn polar(from: Mm, len: f64, angle: f64) -> Mm {
    let a = angle.to_radians();
    [from[0] + len * a.cos(), from[1] - len * a.sin()]
}

/// Stop drawing: no rubber band, no boxes, no chain.
pub fn end_chain(m: &mut SketchMode) {
    m.second = None;
    m.start = None;
    m.chain = None;
    m.chain_start = None;
    m.hud = None;
    m.rubber = None;
    m.snap = Snap::None;
    m.modify.clear();
    m.armed = false;
}

/// Where `start` is.
pub fn start_pos(sketch: &Sketch, start: Start) -> Mm {
    match start {
        Start::At(p) => p,
        Start::Point(id) => sketch.point(id).map_or([0.0, 0.0], |p| [p.x, p.y]),
    }
}

/// Length (mm) and angle (degrees, counter-clockwise on screen) from `a` to `b`.
pub fn length_angle(a: Mm, b: Mm) -> (f64, f64) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    (dx.hypot(dy), (-dy).atan2(dx).to_degrees())
}

/// Within this many degrees of an axis the line snaps to it.
const AXIS_SNAP_DEG: f64 = 3.0;

/// Where the segment would end for the mouse at `pos`: on a sketch point within 8 px, else on an
/// axis through the start when within 3 degrees of it, else at the mouse.
fn end_for(cache: &Cache, xf: &Xf, start: Option<Start>, pos: Pos2) -> (Mm, Snap) {
    let exclude = match start {
        Some(Start::Point(p)) => Some(p),
        _ => None,
    };
    if let Some(p) = hit::nearest_point(cache, xf, pos, POINT_SNAP, exclude)
        && let Some(sp) = cache.sketch.point(p)
    {
        return ([sp.x, sp.y], Snap::Point(p));
    }
    let mouse = to_mm(xf, &cache.sketch, pos);
    let Some(start) = start else { return (mouse, Snap::None) };
    let s = start_pos(&cache.sketch, start);
    let (dx, dy) = (mouse[0] - s[0], mouse[1] - s[1]);
    let len = dx.hypot(dy);
    if len > 1e-9 {
        let tolerance = AXIS_SNAP_DEG.to_radians().sin();
        if dy.abs() / len < tolerance {
            return ([mouse[0], s[1]], Snap::Horizontal);
        }
        if dx.abs() / len < tolerance {
            return ([s[0], mouse[1]], Snap::Vertical);
        }
    }
    (mouse, Snap::None)
}

/// The Line tool's pointer: hover snaps; a press sets the start (or arms the commit); the release
/// of an armed press commits one segment; a double-click ends the chain.
pub fn line_pointer(app: &mut VectorcraftApp, cache: &Cache, xf: &Xf, pos: Pos2, input: PointerInput) {
    let Some(m) = app.sketch.as_mut() else { return };
    let (mut end, snap) = end_for(cache, xf, m.start, pos);
    m.snap = snap;
    m.hover = hit::nearest_point(cache, xf, pos, POINT_SNAP, None).map(Pick::Point);
    if let (Some(start), Some(hud)) = (m.start, m.hud.as_mut()) {
        let from = start_pos(&cache.sketch, start);
        let (len, ang) = length_angle(from, end);
        hud.follow_mouse(&[len, ang]);
        // The segment takes the typed or locked values; only the other quantity follows the mouse.
        let (tl, ta) = (typed(hud, 0, &cache.params), typed(hud, 1, &cache.params));
        if tl.is_some() || ta.is_some() {
            end = polar(from, tl.unwrap_or(len), ta.unwrap_or(ang));
        }
    }
    m.rubber = Some(end);
    if input.pressed {
        if m.start.is_none() {
            m.start = Some(match snap {
                Snap::Point(p) => Start::Point(p),
                _ => Start::At(end),
            });
            m.hud = Some(new_line_hud());
            m.armed = false;
        } else {
            m.armed = true;
        }
    }
    if input.double {
        end_chain(m);
        return;
    }
    if input.released && m.armed {
        m.armed = false;
        super::feed(app, HudEvent::Enter);
    }
}

/// Commit the segment being drawn with the boxes' typed texts: `sketch.addLine`. On an error the
/// status line says why and the rubber band stays.
pub fn commit_line(app: &mut VectorcraftApp, texts: Vec<Option<String>>) {
    let Some(m) = app.sketch.as_ref() else { return };
    let (Some(start), Some(rubber)) = (m.start, m.rubber) else { return };
    let (id, snap) = (m.id.0, m.snap);
    let Some(cache) = snapshot(app) else { return };
    let typed_length = texts.first().cloned().flatten();
    let typed_angle = texts.get(1).cloned().flatten();
    let any_typed = typed_length.is_some() || typed_angle.is_some();
    let from = start_pos(&cache.sketch, start);
    if !any_typed && (rubber[0] - from[0]).hypot(rubber[1] - from[1]) < 1e-6 {
        // The click that set the start, or a double-click's second click.
        return;
    }
    let mut p = json!({"id": id});
    match start {
        Start::Point(a) => p["fromPoint"] = json!(a),
        Start::At(xy) => p["from"] = json!(xy),
    }
    match snap {
        Snap::Point(b) if !any_typed => p["toPoint"] = json!(b),
        _ => p["to"] = json!(rubber),
    }
    if let Some(l) = &typed_length {
        p["length"] = json!(l);
    }
    if let Some(a) = &typed_angle {
        p["angle"] = json!(a);
    } else {
        match snap {
            Snap::Horizontal => p["horizontal"] = json!(true),
            Snap::Vertical => p["vertical"] = json!(true),
            _ => {}
        }
    }
    match app.run("sketch.addLine", p) {
        Ok(v) => {
            let point = |k: &str| v.get(k).and_then(Value::as_u64).map(|n| n as u32);
            let (Some(a), Some(b)) = (point("a"), point("b")) else { return };
            let Some(m) = app.sketch.as_mut() else { return };
            let first = m.chain_start.unwrap_or(a);
            m.chain_start = Some(first);
            if b == first {
                // Back on the chain's first point: the loop is closed; the tool is ready for a new chain.
                end_chain(m);
            } else {
                m.chain = Some(b);
                m.start = Some(Start::Point(b));
                m.hud = Some(new_line_hud());
                m.armed = false;
            }
        }
        Err(e) => app.status(e),
    }
}

/// Screen position of the start of the segment being drawn.
pub fn start_screen(cache: &Cache, xf: &Xf, start: Start) -> Pos2 {
    to_screen(xf, &cache.sketch, start_pos(&cache.sketch, start))
}

// ---- the shape tools: rectangles, circle, 3-point arc ----

/// Where a rectangle's opposite corner is for the mouse at `mouse`, with typed width and height
/// applied, and the mouse's own width and height. A centre rectangle's `from` is its centre, so
/// its width and height are twice the distance to the mouse.
pub fn rect_to(tool: SketchTool, from: Mm, mouse: Mm, hud: &Hud, solved: &Solved) -> (Mm, [f64; 2]) {
    let (dx, dy) = (mouse[0] - from[0], mouse[1] - from[1]);
    let k = if tool == SketchTool::RectCenter { 2.0 } else { 1.0 };
    let shown = [k * dx.abs(), k * dy.abs()];
    let w = typed(hud, 0, solved).unwrap_or(shown[0]);
    let h = typed(hud, 1, solved).unwrap_or(shown[1]);
    let (sx, sy) = (if dx < 0.0 { -1.0 } else { 1.0 }, if dy < 0.0 { -1.0 } else { 1.0 });
    ([from[0] + sx * w / k, from[1] + sy * h / k], shown)
}

/// The four corners of the rectangle from `from` to `to`, in order around it.
pub fn rect_corners(tool: SketchTool, from: Mm, to: Mm) -> [Mm; 4] {
    if tool == SketchTool::RectCenter {
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        [[from[0] - dx, from[1] - dy], [from[0] + dx, from[1] - dy], [from[0] + dx, from[1] + dy], [from[0] - dx, from[1] + dy]]
    } else {
        [from, [to[0], from[1]], to, [from[0], to[1]]]
    }
}

/// The circle's rubber point: `radius` (twice the typed diameter's half) along the way to the mouse.
fn circle_to(centre: Mm, mouse: Mm, hud: &Hud, solved: &Solved) -> (Mm, f64) {
    let (dx, dy) = (mouse[0] - centre[0], mouse[1] - centre[1]);
    let r_mouse = dx.hypot(dy);
    let r = typed(hud, 0, solved).map_or(r_mouse, |d| d / 2.0);
    let (ux, uy) = if r_mouse > 1e-9 { (dx / r_mouse, dy / r_mouse) } else { (1.0, 0.0) };
    ([centre[0] + ux * r, centre[1] + uy * r], 2.0 * r_mouse)
}

/// The centre of the circle through three places.
pub fn circumcentre(a: Mm, b: Mm, c: Mm) -> Option<Mm> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-9 {
        return None;
    }
    let (a2, b2, c2) = (a[0] * a[0] + a[1] * a[1], b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]);
    let x = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
    let y = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
    (x.is_finite() && y.is_finite()).then_some([x, y])
}

/// The arc's centre for the three places, or with `radius` on the chord's bisector at the side
/// nearest the three-place circle (the engine's rule, so the preview is what gets committed).
pub fn arc_center(start: Mm, end: Mm, through: Mm, radius: Option<f64>) -> Option<Mm> {
    let Some(r) = radius else { return circumcentre(start, end, through) };
    let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
    let chord = dx.hypot(dy);
    if chord < 1e-9 || 2.0 * r < chord - 1e-9 {
        return None;
    }
    let k = (r * r - chord * chord / 4.0).max(0.0).sqrt();
    let mid = [(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0];
    let n = [-dy / chord, dx / chord];
    let (c1, c2) = ([mid[0] + n[0] * k, mid[1] + n[1] * k], [mid[0] - n[0] * k, mid[1] - n[1] * k]);
    let near = |c: Mm, to: Mm| (c[0] - to[0]).hypot(c[1] - to[1]);
    Some(match circumcentre(start, end, through) {
        Some(guess) => {
            if near(c1, guess) <= near(c2, guess) {
                c1
            } else {
                c2
            }
        }
        None => {
            if (through[0] - mid[0]) * n[0] + (through[1] - mid[1]) * n[1] > 0.0 {
                c2
            } else {
                c1
            }
        }
    })
}

/// Points along the arc from `start` to `end` around `centre` that passes through `through`.
pub fn arc_polyline(start: Mm, end: Mm, through: Mm, centre: Mm) -> Vec<Mm> {
    let ang = |q: Mm| (-(q[1] - centre[1])).atan2(q[0] - centre[0]);
    let tau = std::f64::consts::TAU;
    let ccw = (ang(end) - ang(start)).rem_euclid(tau);
    let through_sweep = (ang(through) - ang(start)).rem_euclid(tau);
    let sweep = if through_sweep <= ccw { ccw } else { ccw - tau };
    let r = (start[0] - centre[0]).hypot(start[1] - centre[1]);
    let from = ang(start);
    let n = ((sweep.abs().to_degrees() / 5.0).ceil() as usize).clamp(2, 120);
    (0..=n)
        .map(|i| {
            let a = from + sweep * i as f64 / n as f64;
            [centre[0] + r * a.cos(), centre[1] - r * a.sin()]
        })
        .collect()
}

/// The pointer of the rectangle, circle and arc tools: hover snaps; clicks place the points; the
/// release of an armed click commits.
pub fn shape_pointer(app: &mut VectorcraftApp, cache: &Cache, xf: &Xf, pos: Pos2, input: PointerInput) {
    let Some(m) = app.sketch.as_mut() else { return };
    let tool = m.tool;
    let (mouse, snap) = end_for(cache, xf, None, pos);
    m.snap = snap;
    m.hover = hit::nearest_point(cache, xf, pos, POINT_SNAP, None).map(Pick::Point);
    // A circle's centre and an arc's ends keep the point they snap to; a rectangle takes its place.
    let place = |snap: Snap, at: Mm| match snap {
        Snap::Point(p) if matches!(tool, SketchTool::Circle | SketchTool::Arc3) => Start::Point(p),
        _ => Start::At(at),
    };
    match (m.start, m.second) {
        (None, _) => {
            m.rubber = Some(mouse);
            if input.pressed {
                m.start = Some(place(snap, mouse));
                m.hud = (tool != SketchTool::Arc3).then(|| new_hud(tool));
                m.armed = false;
            }
        }
        (Some(start), None) if tool == SketchTool::Arc3 => {
            m.rubber = Some(mouse);
            let s = start_pos(&cache.sketch, start);
            let apart = (mouse[0] - s[0]).hypot(mouse[1] - s[1]) > 1e-6;
            if input.pressed && apart {
                m.second = Some(place(snap, mouse));
                m.hud = Some(new_hud(tool));
                m.armed = false;
            }
        }
        (Some(start), second) => {
            let s = start_pos(&cache.sketch, start);
            let Some(hud) = m.hud.as_mut() else { return };
            let solved = &cache.params;
            let rubber = match tool {
                SketchTool::Rect2 | SketchTool::RectCenter => {
                    let (to, shown) = rect_to(tool, s, mouse, hud, solved);
                    hud.follow_mouse(&shown);
                    to
                }
                SketchTool::Circle => {
                    let (to, d) = circle_to(s, mouse, hud, solved);
                    hud.follow_mouse(&[d]);
                    to
                }
                _ => {
                    // The bulge is the mouse itself, not snapped.
                    let through = to_mm(xf, &cache.sketch, pos);
                    let e = second.map_or(through, |e| start_pos(&cache.sketch, e));
                    let r = circumcentre(s, e, through).map_or(0.0, |c| (c[0] - s[0]).hypot(c[1] - s[1]));
                    hud.follow_mouse(&[r]);
                    through
                }
            };
            m.rubber = Some(rubber);
            if input.pressed {
                m.armed = true;
            }
            if input.released && m.armed {
                m.armed = false;
                super::feed(app, HudEvent::Enter);
            }
        }
    }
}

/// What the commands need to know about a start or end: an id when it is an existing point.
fn put(p: &mut Value, at: Start, point_key: &str, xy_key: &str) {
    match at {
        Start::Point(id) => p[point_key] = json!(id),
        Start::At(xy) => p[xy_key] = json!(xy),
    }
}

/// Enter (or the armed click's release) with the boxes' typed texts: the active tool's command.
pub fn commit(app: &mut VectorcraftApp, texts: Vec<Option<String>>) {
    let Some(tool) = app.sketch.as_ref().map(|m| m.tool) else { return };
    match tool {
        SketchTool::Line => commit_line(app, texts),
        SketchTool::Rect2 | SketchTool::RectCenter => commit_rect(app, tool, texts),
        SketchTool::Circle => commit_circle(app, texts),
        SketchTool::Arc3 => commit_arc(app, texts),
        SketchTool::Fillet => commit_fillet(app, texts),
        SketchTool::Offset => commit_offset(app, texts),
        SketchTool::None | SketchTool::Dimension | SketchTool::Trim | SketchTool::Mirror => {}
    }
}

/// Run `command`; on success the gesture is over (the tool stays), else the status line says why.
fn run_shape(app: &mut VectorcraftApp, command: &str, p: Value) {
    match app.run(command, p) {
        Ok(_) => {
            if let Some(m) = app.sketch.as_mut() {
                end_chain(m);
            }
        }
        Err(e) => app.status(e),
    }
}

fn commit_rect(app: &mut VectorcraftApp, tool: SketchTool, texts: Vec<Option<String>>) {
    let Some(m) = app.sketch.as_ref() else { return };
    let (Some(start), Some(to)) = (m.start, m.rubber) else { return };
    let id = m.id.0;
    let Some(cache) = snapshot(app) else { return };
    let from = start_pos(&cache.sketch, start);
    let (width, height) = (texts.first().cloned().flatten(), texts.get(1).cloned().flatten());
    if width.is_none() && height.is_none() && (to[0] - from[0]).abs() < 1e-6 && (to[1] - from[1]).abs() < 1e-6 {
        // The click that set the corner.
        return;
    }
    let mode = if tool == SketchTool::RectCenter { "center" } else { "corner" };
    let mut p = json!({"id": id, "mode": mode, "from": from, "to": to});
    if let Some(w) = width {
        p["width"] = json!(w);
    }
    if let Some(h) = height {
        p["height"] = json!(h);
    }
    run_shape(app, "sketch.addRect", p);
}

fn commit_circle(app: &mut VectorcraftApp, texts: Vec<Option<String>>) {
    let Some(m) = app.sketch.as_ref() else { return };
    let (Some(start), Some(to)) = (m.start, m.rubber) else { return };
    let id = m.id.0;
    let Some(cache) = snapshot(app) else { return };
    let centre = start_pos(&cache.sketch, start);
    let r = (to[0] - centre[0]).hypot(to[1] - centre[1]);
    let diameter = texts.first().cloned().flatten();
    if diameter.is_none() && r < 1e-6 {
        return;
    }
    let mut p = json!({"id": id, "r": r});
    put(&mut p, start, "centerPoint", "center");
    if let Some(d) = diameter {
        p["diameter"] = json!(d);
    }
    run_shape(app, "sketch.addCircle", p);
}

fn commit_arc(app: &mut VectorcraftApp, texts: Vec<Option<String>>) {
    let Some(m) = app.sketch.as_ref() else { return };
    let (Some(start), Some(end), Some(through)) = (m.start, m.second, m.rubber) else { return };
    let id = m.id.0;
    let mut p = json!({"id": id, "through": through});
    put(&mut p, start, "startPoint", "start");
    put(&mut p, end, "endPoint", "end");
    if let Some(r) = texts.first().cloned().flatten() {
        p["radius"] = json!(r);
    }
    run_shape(app, "sketch.addArc", p);
}

// ---- fillet ----

/// The points where exactly two (non-construction) lines meet: what Fillet can round.
pub fn corner_points(sk: &Sketch) -> Vec<u32> {
    sk.points
        .iter()
        .map(|p| p.id)
        .filter(|id| sk.entities.iter().filter(|e| matches!(e, Entity::Line { a, b, construction: false, .. } if a == id || b == id)).count() == 2)
        .collect()
}

/// The Fillet tool's pointer: hover marks a corner, a click picks it and opens the radius box.
pub fn fillet_pointer(app: &mut VectorcraftApp, cache: &Cache, xf: &Xf, pos: Pos2, input: PointerInput) {
    let Some(m) = app.sketch.as_mut() else { return };
    let corners = corner_points(&cache.sketch);
    let mut best: Option<(f32, u32)> = None;
    for id in corners {
        let Some(p) = cache.sketch.point(id) else { continue };
        let d = to_screen(xf, &cache.sketch, [p.x, p.y]).distance(pos);
        if d <= POINT_SNAP && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, id));
        }
    }
    m.hover = best.map(|(_, id)| Pick::Point(id));
    if input.pressed
        && let Some((_, id)) = best
    {
        m.start = Some(Start::Point(id));
        m.hud = Some(new_hud(SketchTool::Fillet));
    }
}

/// Choosing Fillet with two lines selected that meet at a corner: that corner is the one.
pub fn fillet_from_selection(app: &mut VectorcraftApp) {
    let Some(cache) = snapshot(app) else { return };
    let Some(m) = app.sketch.as_mut() else { return };
    let lines: Vec<(u32, u32)> = m
        .selected
        .iter()
        .filter_map(|p| match p {
            Pick::Entity(i) => match cache.sketch.entity(*i) {
                Some(Entity::Line { a, b, .. }) => Some((*a, *b)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let [(a1, b1), (a2, b2)] = lines[..] else { return };
    let corners = corner_points(&cache.sketch);
    if let Some(shared) = [a1, b1].into_iter().find(|p| (*p == a2 || *p == b2) && corners.contains(p)) {
        m.start = Some(Start::Point(shared));
        m.hud = Some(new_hud(SketchTool::Fillet));
    }
}

fn commit_fillet(app: &mut VectorcraftApp, texts: Vec<Option<String>>) {
    let Some(m) = app.sketch.as_ref() else { return };
    let Some(Start::Point(corner)) = m.start else { return };
    let id = m.id.0;
    let Some(radius) = texts.first().cloned().flatten() else {
        app.status(tl!("Type a radius first"));
        return;
    };
    run_shape(app, "sketch.fillet", json!({"id": id, "point": corner, "radius": radius}));
}

// ---- the modify tools: trim, offset, mirror ----

/// The line `e` as its two ends, when it is one.
fn line_ends(sk: &Sketch, e: u32) -> Option<(Mm, Mm)> {
    match sk.entity(e)? {
        Entity::Line { a, b, .. } => Some((sk.point(*a).map(|p| [p.x, p.y])?, sk.point(*b).map(|p| [p.x, p.y])?)),
        _ => None,
    }
}

/// The Trim tool's pointer: hovering a shape names it (a line shows the piece that would go); a
/// click trims at the pointer.
pub fn trim_pointer(app: &mut VectorcraftApp, cache: &Cache, xf: &Xf, pos: Pos2, input: PointerInput) {
    let Some(m) = app.sketch.as_mut() else { return };
    let hit = hit::entity_at(cache, xf, pos, hit::ENTITY_PICK);
    let at = to_mm(xf, &cache.sketch, pos);
    m.hover = hit.map(Pick::Entity);
    m.rubber = Some(at);
    let id = m.id.0;
    if input.clicked
        && let Some(entity) = hit
        && let Err(e) = app.run("sketch.trim", json!({"id": id, "entity": entity, "at": at}))
    {
        app.status(e);
    }
}

/// Choosing Offset with lines selected: one line takes its whole chain, several are used as they are.
pub fn offset_from_selection(app: &mut VectorcraftApp) {
    let Some(cache) = snapshot(app) else { return };
    let Some(m) = app.sketch.as_mut() else { return };
    let lines: Vec<u32> = m
        .selected
        .iter()
        .filter_map(|p| match p {
            Pick::Entity(i) if matches!(cache.sketch.entity(*i), Some(Entity::Line { .. })) => Some(*i),
            _ => None,
        })
        .collect();
    let chosen = match lines[..] {
        [] => return,
        [one] => cache.sketch.connected_chain(one),
        _ => lines,
    };
    m.modify = chosen;
    m.hud = Some(new_hud(SketchTool::Offset));
}

/// The Offset tool's pointer: before a chain is chosen, a press on a line takes its whole chain;
/// then the mouse picks the side and the distance (a typed distance wins), and a click commits.
pub fn offset_pointer(app: &mut VectorcraftApp, cache: &Cache, xf: &Xf, pos: Pos2, input: PointerInput) {
    let Some(m) = app.sketch.as_mut() else { return };
    let mouse = to_mm(xf, &cache.sketch, pos);
    m.rubber = Some(mouse);
    let Some(first) = m.modify.first().copied() else {
        let hit = hit::entity_at(cache, xf, pos, hit::ENTITY_PICK).filter(|e| matches!(cache.sketch.entity(*e), Some(Entity::Line { .. })));
        m.hover = hit.map(Pick::Entity);
        if input.pressed
            && let Some(line) = hit
        {
            m.modify = cache.sketch.connected_chain(line);
            m.hud = Some(new_hud(SketchTool::Offset));
            m.armed = false;
        }
        return;
    };
    m.hover = None;
    let Some((p, q)) = line_ends(&cache.sketch, first) else { return };
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    let len = dx.hypot(dy).max(1e-9);
    // How far the mouse is to the left of the first line as drawn (y is down: left is up).
    let left = ((mouse[0] - p[0]) * dy - (mouse[1] - p[1]) * dx) / len;
    m.side = if left >= 0.0 { 1.0 } else { -1.0 };
    if let Some(hud) = m.hud.as_mut() {
        hud.follow_mouse(&[left.abs()]);
    }
    if input.pressed {
        m.armed = true;
    }
    if input.released && m.armed {
        m.armed = false;
        super::feed(app, HudEvent::Enter);
    }
}

/// The distance (mm) the Offset preview uses: the typed one, else the mouse's.
pub fn offset_distance(m: &SketchMode, solved: &Solved) -> Option<f64> {
    let hud = m.hud.as_ref()?;
    typed(hud, 0, solved).or_else(|| hud.boxes.first().and_then(|b| b.text.trim().parse().ok()))
}

fn commit_offset(app: &mut VectorcraftApp, texts: Vec<Option<String>>) {
    let Some(m) = app.sketch.as_ref() else { return };
    if m.modify.is_empty() {
        return;
    }
    let (id, lines, side) = (m.id.0, m.modify.clone(), m.side);
    let text = texts.first().cloned().flatten().or_else(|| m.hud.as_ref().and_then(|h| h.boxes.first()).map(|b| b.text.clone()));
    let Some(text) = text.filter(|t| t.trim().parse::<f64>().map_or(true, |v| v > 1e-6)) else {
        app.status(tl!("Move the mouse off the line or type a distance"));
        return;
    };
    run_shape(app, "sketch.offset", json!({"id": id, "entities": lines, "distance": text, "side": side}));
}

/// Choosing Mirror: the selected shapes are the ones to mirror; with none selected the tool does
/// not start.
pub fn mirror_from_selection(app: &mut VectorcraftApp) {
    let Some(m) = app.sketch.as_mut() else { return };
    let shapes: Vec<u32> = m
        .selected
        .iter()
        .filter_map(|p| match p {
            Pick::Entity(i) => Some(*i),
            _ => None,
        })
        .collect();
    if shapes.is_empty() {
        m.tool = SketchTool::None;
        app.status(tl!("Select the shapes to mirror first"));
        return;
    }
    m.modify = shapes;
}

/// The Mirror tool's pointer: hovering a line (not one being mirrored) marks it; a click mirrors
/// the chosen shapes about it.
pub fn mirror_pointer(app: &mut VectorcraftApp, cache: &Cache, xf: &Xf, pos: Pos2, input: PointerInput) {
    let Some(m) = app.sketch.as_mut() else { return };
    let hit = hit::entity_at(cache, xf, pos, hit::ENTITY_PICK)
        .filter(|e| matches!(cache.sketch.entity(*e), Some(Entity::Line { .. })) && !m.modify.contains(e));
    m.hover = hit.map(Pick::Entity);
    let (id, shapes) = (m.id.0, m.modify.clone());
    if input.clicked
        && let Some(line) = hit
    {
        match app.run("sketch.mirror", json!({"id": id, "entities": shapes, "line": line})) {
            Ok(_) => {
                if let Some(m) = app.sketch.as_mut() {
                    tools_done(m);
                }
            }
            Err(e) => app.status(e),
        }
    }
}

/// A modify tool finished: back to selecting, with nothing selected.
fn tools_done(m: &mut SketchMode) {
    end_chain(m);
    m.tool = SketchTool::None;
    m.selected.clear();
    m.hover = None;
}
