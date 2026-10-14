//! The sketch tools. The Line tool now; the rest come next.

use egui::Pos2;
use serde_json::{Value, json};
use vectorcraft_doc::sketch::Sketch;

use super::hit::{self, POINT_SNAP};
use super::hud::{BoxUnit, Hud, HudEvent};
use super::{Cache, Mm, Pick, PointerInput, SketchMode, Snap, Start, snapshot, to_mm, to_screen};
use crate::VectorcraftApp;
use crate::canvas::Xf;

/// The length and angle boxes of a line being drawn.
pub fn new_line_hud() -> Hud {
    Hud::new(&[("length", BoxUnit::Mm), ("angle", BoxUnit::Deg)])
}

/// Stop drawing: no rubber band, no boxes, no chain.
pub fn end_chain(m: &mut SketchMode) {
    m.start = None;
    m.chain = None;
    m.chain_start = None;
    m.hud = None;
    m.rubber = None;
    m.snap = Snap::None;
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
    let (end, snap) = end_for(cache, xf, m.start, pos);
    m.rubber = Some(end);
    m.snap = snap;
    m.hover = hit::nearest_point(cache, xf, pos, POINT_SNAP, None).map(Pick::Point);
    if let (Some(start), Some(hud)) = (m.start, m.hud.as_mut()) {
        let (len, ang) = length_angle(start_pos(&cache.sketch, start), end);
        hud.follow_mouse(&[len, ang]);
    }
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
