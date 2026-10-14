//! What sketch mode draws over the canvas: the sketch's geometry (blue until fully constrained),
//! dimensions, the rubber band, snap markers and the heads-up boxes.

use egui::epaint::{PathShape, Shape};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use vectorcraft_doc::sketch::model::{Constraint, Entity};

use super::hud::{BoxUnit, Hud, HudBox};
use super::tools::{length_angle, start_pos};
use super::{Cache, Pick, SketchMode, SketchTool, Snap, px_per_mm, to_screen};
use crate::VectorcraftApp;
use crate::canvas::Xf;
use crate::theme::Tokens;

/// Geometry that is not fully constrained.
const BLUE: Color32 = Color32::from_rgb(0x1f, 0x6f, 0xeb);
const BLACK: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
const LIGHT: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
/// Distance of a dimension line from the geometry it measures, in screen points.
const DIM_OFFSET: f32 = 16.0;
const ARC_RADIUS: f32 = 30.0;

/// One dimension, laid out in screen space.
#[derive(Clone, Debug)]
pub struct DimGeom {
    /// The constraint's index in the recipe.
    pub index: usize,
    pub segments: Vec<[Pos2; 2]>,
    /// Arrowheads: the tip and the direction from the tip into the line.
    pub arrows: Vec<(Pos2, Vec2)>,
    pub arc: Vec<Pos2>,
    /// Centre of the value text.
    pub label: Pos2,
    pub text: String,
    pub error: Option<String>,
}

impl DimGeom {
    /// The value text's box (what a double-click edits).
    pub fn label_rect(&self) -> Rect {
        Rect::from_center_size(self.label, vec2(self.text.chars().count() as f32 * 7.0 + 12.0, 18.0))
    }
}

/// A dimension's number: two decimals, trimmed (`50`, `42.5`).
pub fn trim2(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

fn on_circle(c: Pos2, r: f32, deg: f32) -> Pos2 {
    let a = deg.to_radians();
    pos2(c.x + r * a.cos(), c.y - r * a.sin())
}

/// A polyline arc around `c` from `from_deg` counter-clockwise (on screen) by `sweep_deg`.
fn arc_points(c: Pos2, r: f32, from_deg: f32, sweep_deg: f32) -> Vec<Pos2> {
    let n = ((sweep_deg.abs() / 6.0).ceil() as usize).clamp(2, 90);
    (0..=n).map(|i| on_circle(c, r, from_deg + sweep_deg * i as f32 / n as f32)).collect()
}

/// Screen angle of vector `v`, counter-clockwise, degrees.
fn screen_angle(v: Vec2) -> f32 {
    (-v.y).atan2(v.x).to_degrees()
}

/// Every dimension of the sketch, laid out for the view.
pub fn dimension_geoms(cache: &Cache, xf: &Xf) -> Vec<DimGeom> {
    let sk = &cache.sketch;
    let pt = |id: u32| sk.point(id).map(|p| to_screen(xf, sk, [p.x, p.y]));
    let line_ends = |id: u32| match sk.entity(id)? {
        Entity::Line { a, b, .. } => Some((pt(*a)?, pt(*b)?)),
        _ => None,
    };
    // Centre and radius (screen) of a circle or arc.
    let round = |id: u32| match sk.entity(id)? {
        Entity::Circle { c, r, .. } => Some((pt(*c)?, (*r * px_per_mm(xf)) as f32)),
        Entity::Arc { c, a, .. } => {
            let c = pt(*c)?;
            Some((c, c.distance(pt(*a)?)))
        }
        _ => None,
    };
    let mut out = vec![];
    for (index, c) in sk.constraints.iter().enumerate() {
        let Some(param) = c.param() else { continue };
        let value = cache.params.value(param).or_else(|| sk.measure(c));
        let number = value.map_or_else(|| "?".to_string(), trim2);
        let mut g = DimGeom {
            index,
            segments: vec![],
            arrows: vec![],
            arc: vec![],
            label: Pos2::ZERO,
            text: String::new(),
            error: cache.errors.get(&index).cloned(),
        };
        let laid = match c {
            Constraint::Length { line, .. } => line_ends(*line).map(|(p, q)| linear(&mut g, p, q)),
            Constraint::Distance { a, b, .. } => pt(*a).zip(pt(*b)).map(|(p, q)| linear(&mut g, p, q)),
            Constraint::Angle { line, .. } => line_ends(*line).map(|(p, q)| {
                let theta = sk.measure(c).unwrap_or(0.0) as f32;
                let _ = q;
                g.segments.push([p, p + vec2(ARC_RADIUS + 10.0, 0.0)]);
                g.arc = arc_points(p, ARC_RADIUS, 0.0, theta);
                g.label = on_circle(p, ARC_RADIUS + 16.0, theta / 2.0);
            }),
            Constraint::AngleBetween { a, b, .. } => line_ends(*a).zip(line_ends(*b)).map(|((p1, q1), (p2, q2))| {
                let theta = sk.measure(c).unwrap_or(0.0) as f32;
                let at = intersection(p1, q1, p2, q2).unwrap_or(p1);
                let from = screen_angle(q1 - p1);
                g.arc = arc_points(at, ARC_RADIUS, from, theta);
                g.label = on_circle(at, ARC_RADIUS + 16.0, from + theta / 2.0);
            }),
            Constraint::Radius { e, .. } => round(*e).map(|(centre, r)| {
                let tip = on_circle(centre, r, 45.0);
                g.segments.push([centre, tip]);
                g.arrows.push((tip, centre - tip));
                g.label = on_circle(centre, r + 14.0, 45.0);
            }),
            Constraint::Diameter { e, .. } => round(*e).map(|(centre, r)| {
                let (tip, tail) = (on_circle(centre, r, 45.0), on_circle(centre, r, 225.0));
                g.segments.push([tail, tip]);
                g.arrows.push((tip, tail - tip));
                g.arrows.push((tail, tip - tail));
                g.label = on_circle(centre, r + 16.0, 45.0);
            }),
            _ => None,
        };
        if laid.is_none() {
            continue;
        }
        g.text = match c {
            Constraint::Angle { .. } | Constraint::AngleBetween { .. } => format!("{number}\u{b0}"),
            Constraint::Radius { .. } => format!("R{number}"),
            Constraint::Diameter { .. } => format!("\u{d8}{number}"),
            _ => number,
        };
        out.push(g);
    }
    out
}

/// A length dimension between `p` and `q`: extension lines, an offset dimension line with
/// arrowheads, the value centred on it.
fn linear(g: &mut DimGeom, p: Pos2, q: Pos2) {
    let d = q - p;
    let len = d.length();
    let u = if len > 1e-3 { d / len } else { vec2(1.0, 0.0) };
    let n = vec2(u.y, -u.x);
    let (p2, q2) = (p + n * DIM_OFFSET, q + n * DIM_OFFSET);
    g.segments.push([p, p + n * (DIM_OFFSET + 4.0)]);
    g.segments.push([q, q + n * (DIM_OFFSET + 4.0)]);
    g.segments.push([p2, q2]);
    g.arrows.push((p2, u));
    g.arrows.push((q2, -u));
    g.label = p2 + (q2 - p2) * 0.5;
}

/// Where the infinite lines `p1 q1` and `p2 q2` cross.
fn intersection(p1: Pos2, q1: Pos2, p2: Pos2, q2: Pos2) -> Option<Pos2> {
    let (d1, d2) = (q1 - p1, q2 - p2);
    let det = d1.x * d2.y - d1.y * d2.x;
    if det.abs() < 1e-6 {
        return None;
    }
    let t = ((p2.x - p1.x) * d2.y - (p2.y - p1.y) * d2.x) / det;
    Some(p1 + d1 * t)
}

fn ink(t: &Tokens, constrained: bool) -> Color32 {
    if !constrained {
        BLUE
    } else if t.dark {
        LIGHT
    } else {
        BLACK
    }
}

fn dashed(painter: &Painter, pts: &[Pos2], stroke: Stroke) {
    painter.extend(Shape::dashed_line(pts, stroke, 5.0, 4.0));
}

/// The whole overlay of a frame.
pub fn paint(app: &VectorcraftApp, painter: &Painter, xf: &Xf, cache: &Cache, t: &Tokens) {
    let Some(m) = app.sketch.as_ref() else { return };
    let sk = &cache.sketch;
    let pt = |id: u32| sk.point(id).map(|p| to_screen(xf, sk, [p.x, p.y]));
    let selected = |pick: Pick| m.selected.contains(&pick);

    // Entities.
    for e in &sk.entities {
        let pick = Pick::Entity(e.id());
        let (color, width) = if selected(pick) {
            (t.accent_strong, 3.0)
        } else if m.hover == Some(pick) {
            (t.accent, 2.5)
        } else {
            (ink(t, cache.constrained.contains(&e.id())), 1.5)
        };
        let stroke = Stroke::new(width, color);
        let pts: Vec<Pos2> = match e {
            Entity::Line { a, b, .. } => match (pt(*a), pt(*b)) {
                (Some(p), Some(q)) => vec![p, q],
                _ => continue,
            },
            Entity::Circle { c, r, .. } => match sk.point(*c) {
                Some(c) => (0..=72)
                    .map(|i| {
                        let a = std::f64::consts::TAU * f64::from(i) / 72.0;
                        to_screen(xf, sk, [c.x + r * a.cos(), c.y - r * a.sin()])
                    })
                    .collect(),
                None => continue,
            },
            Entity::Arc { c, a, b, .. } => match (sk.point(*c), sk.point(*a), sk.point(*b)) {
                (Some(c), Some(a), Some(b)) => {
                    let r = (a.x - c.x).hypot(a.y - c.y);
                    let from = (-(a.y - c.y)).atan2(a.x - c.x);
                    let to = (-(b.y - c.y)).atan2(b.x - c.x);
                    let sweep = (to - from).rem_euclid(std::f64::consts::TAU);
                    let n = ((sweep.to_degrees() / 5.0).ceil() as usize).clamp(2, 120);
                    (0..=n)
                        .map(|i| {
                            let ang = from + sweep * i as f64 / n as f64;
                            to_screen(xf, sk, [c.x + r * ang.cos(), c.y - r * ang.sin()])
                        })
                        .collect()
                }
                _ => continue,
            },
        };
        if e.construction() {
            dashed(painter, &pts, stroke);
        } else {
            painter.add(PathShape::line(pts, stroke));
        }
    }

    // Points, and the origin.
    for p in &sk.points {
        let at = to_screen(xf, sk, [p.x, p.y]);
        let pick = Pick::Point(p.id);
        let color = if selected(pick) {
            t.accent_strong
        } else if m.hover == Some(pick) {
            t.accent
        } else {
            ink(t, cache.point_constrained.contains(&p.id))
        };
        let half = if selected(pick) || m.hover == Some(pick) { 3.5 } else { 2.0 };
        painter.rect_filled(Rect::from_center_size(at, Vec2::splat(half * 2.0)), 0.0, color);
    }
    let origin = to_screen(xf, sk, [0.0, 0.0]);
    let cross = Stroke::new(1.0, t.text_dim);
    painter.line_segment([origin - vec2(6.0, 0.0), origin + vec2(6.0, 0.0)], cross);
    painter.line_segment([origin - vec2(0.0, 6.0), origin + vec2(0.0, 6.0)], cross);

    // Dimensions.
    let geoms = dimension_geoms(cache, xf);
    for g in &geoms {
        let pick = Pick::Dim(g.index);
        let color = if g.error.is_some() {
            t.error
        } else if selected(pick) {
            t.accent_strong
        } else if m.hover == Some(pick) {
            t.accent
        } else if t.dark {
            LIGHT
        } else {
            BLACK
        };
        let stroke = Stroke::new(1.0, color);
        for s in &g.segments {
            painter.line_segment(*s, stroke);
        }
        if g.arc.len() > 1 {
            painter.add(PathShape::line(g.arc.clone(), stroke));
        }
        for (tip, axis) in &g.arrows {
            let axis = if axis.length() > 1e-3 { axis.normalized() } else { vec2(1.0, 0.0) };
            let perp = vec2(axis.y, -axis.x);
            let base = *tip + axis * 8.0;
            painter.add(Shape::convex_polygon(vec![*tip, base + perp * 3.0, base - perp * 3.0], color, Stroke::NONE));
        }
        let rect = g.label_rect();
        painter.rect_filled(rect, 2.0, t.pasteboard);
        if selected(pick) || m.hover == Some(pick) {
            painter.rect_stroke(rect, 2.0, Stroke::new(1.0, color), egui::StrokeKind::Outside);
        }
        painter.text(g.label, Align2::CENTER_CENTER, &g.text, FontId::proportional(12.0), color);
    }

    // The segment being drawn.
    if m.tool == SketchTool::Line {
        if let Some(Pick::Point(p)) = m.hover
            && let Some(at) = pt(p)
        {
            painter.circle_stroke(at, 6.0, Stroke::new(1.5, t.accent));
        }
        if let (Some(start), Some(end)) = (m.start, m.rubber) {
            line_rubber(painter, xf, cache, t, m, start_pos(sk, start), end);
        }
    }

    // A dimension being edited in place.
    if let Some((index, hud)) = &m.edit_dim
        && let Some(g) = geoms.iter().find(|g| g.index == *index)
    {
        paint_hud(painter, t, hud, &[g.label], cache);
    }
}

fn line_rubber(painter: &Painter, xf: &Xf, cache: &Cache, t: &Tokens, m: &SketchMode, start: [f64; 2], end: [f64; 2]) {
    let sk = &cache.sketch;
    let (s, e) = (to_screen(xf, sk, start), to_screen(xf, sk, end));
    painter.line_segment([s, e], Stroke::new(1.5, BLUE));
    let d = e - s;
    let len = d.length();
    let u = if len > 1e-3 { d / len } else { vec2(1.0, 0.0) };
    let n = vec2(u.y, -u.x);
    let mid = s + d * 0.5;
    // The snap glyph beside the line.
    let glyph = match m.snap {
        Snap::Horizontal => Some("\u{2014}"),
        Snap::Vertical => Some("|"),
        _ => None,
    };
    if let Some(glyph) = glyph {
        painter.text(mid - n * 10.0, Align2::CENTER_CENTER, glyph, FontId::proportional(14.0), BLUE);
    }
    // The angle's arc from the horizontal, and the boxes: length beside the line's middle, angle
    // near the start.
    let (_, angle) = length_angle(start, end);
    let angle = angle as f32;
    if len > 4.0 {
        painter.add(PathShape::line(arc_points(s, 26.0, 0.0, angle), Stroke::new(1.0, t.text_dim)));
        painter.line_segment([s, s + vec2(34.0, 0.0)], Stroke::new(1.0, t.text_dim));
    }
    if let Some(hud) = &m.hud {
        let length_at = mid + n * 24.0;
        let angle_at = on_circle(s, 26.0 + 36.0, angle / 2.0);
        paint_hud(painter, t, hud, &[length_at, angle_at], cache);
    }
}

/// The boxes of `hud` at `centres` (one each).
fn paint_hud(painter: &Painter, t: &Tokens, hud: &Hud, centres: &[Pos2], cache: &Cache) {
    for (i, (b, c)) in hud.boxes.iter().zip(centres).enumerate() {
        let error = if hud.show_errors { hud.error(i, &cache.params) } else { None };
        paint_box(painter, t, *c, b, i == hud.focus, error.as_deref());
    }
}

/// One heads-up box: a small rounded rectangle, the focused one with an accent border and a
/// caret, a locked one with a lock dot, one with an error red-bordered with the error below.
fn paint_box(painter: &Painter, t: &Tokens, centre: Pos2, b: &HudBox, focused: bool, error: Option<&str>) {
    let font = FontId::proportional(12.0);
    let text_w = painter.layout_no_wrap(b.text.clone(), font.clone(), t.text_strong).size().x;
    let rect = Rect::from_center_size(centre, vec2((text_w + 24.0).max(64.0), 20.0));
    let border = if error.is_some() {
        t.error
    } else if focused {
        t.accent_strong
    } else {
        t.input_border
    };
    painter.rect_filled(rect, 4.0, t.input);
    painter.rect_stroke(rect, 4.0, Stroke::new(if focused { 1.5 } else { 1.0 }, border), egui::StrokeKind::Inside);
    let unit = match b.unit {
        BoxUnit::Mm => "",
        BoxUnit::Deg => "\u{b0}",
    };
    let shown = format!("{}{}", b.text, unit);
    painter.text(
        rect.left_center() + vec2(7.0, 0.0),
        Align2::LEFT_CENTER,
        shown,
        font.clone(),
        if error.is_some() { t.error } else { t.text_strong },
    );
    if focused {
        let x = rect.left() + 7.0 + text_w + 1.0;
        painter.line_segment([pos2(x, rect.top() + 4.0), pos2(x, rect.bottom() - 4.0)], Stroke::new(1.0, t.text_strong));
    }
    if b.locked {
        painter.circle_filled(rect.right_center() - vec2(6.0, 0.0), 2.5, t.text_dim);
    }
    painter.text(rect.center_top() - vec2(0.0, 2.0), Align2::CENTER_BOTTOM, b.label, FontId::proportional(9.5), t.text_dim);
    if let Some(e) = error {
        painter.text(rect.center_bottom() + vec2(0.0, 3.0), Align2::CENTER_TOP, e, FontId::proportional(10.5), t.error);
    }
}
