//! What sketch mode draws over the canvas: the sketch's geometry (blue until fully constrained),
//! dimensions, the rubber band, snap markers and the heads-up boxes.

use egui::epaint::{PathShape, Shape};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use vectorcraft_doc::sketch::model::{Along, Constraint, Entity};

use super::hud::{BoxUnit, Hud, HudBox};
use super::tools::{arc_center, arc_polyline, corner_points, length_angle, rect_corners, start_pos, typed};
use super::{Cache, Pick, SketchMode, SketchTool, Snap, px_per_mm, to_screen};
use crate::VectorcraftApp;
use crate::canvas::Xf;
use crate::theme::Tokens;

/// Geometry that is not fully constrained.
const BLUE: Color32 = Color32::from_rgb(0x1f, 0x6f, 0xeb);
const BLACK: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
/// Dimension lines, arrowheads and extension lines: they are drawn on the white artboard, so they
/// are dark whatever the theme.
const DIM_INK: Color32 = Color32::from_rgb(0x4a, 0x4a, 0x4a);
/// The value chip's fill and text.
const CHIP: Color32 = Color32::from_rgb(0xf4, 0xf4, 0xf4);
const CHIP_TEXT: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
/// Distance of a dimension line from the geometry it measures, in screen points (past the
/// constraint glyphs, which sit 12 pt off the line: E1, was 16).
const DIM_OFFSET: f32 = 30.0;
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

/// Every dimension of the sketch, laid out for the view, each at its stored place.
pub fn dimension_geoms(cache: &Cache, xf: &Xf) -> Vec<DimGeom> {
    dimension_geoms_dragging(cache, xf, None)
}

/// [`dimension_geoms`] with dimension `drag.0` at `drag.1` instead (a value being dragged).
pub fn dimension_geoms_dragging(cache: &Cache, xf: &Xf, drag: Option<(usize, super::Mm)>) -> Vec<DimGeom> {
    let sk = &cache.sketch;
    let mut out = vec![];
    for (index, c) in sk.constraints.iter().enumerate() {
        let Some(param) = c.param() else { continue };
        let driven = sk.driven.contains(param);
        let value = if driven { sk.measure(c) } else { cache.params.value(param).or_else(|| sk.measure(c)) };
        let number = value.map_or_else(|| "?".to_string(), trim2);
        let place = match drag {
            Some((i, at)) if i == index => Some(at),
            _ => sk.places.get(param).copied(),
        };
        let Some(mut g) = layout(cache, xf, index, c, place, &number) else { continue };
        if driven {
            g.text = format!("({})", g.text);
        }
        g.error = cache.errors.get(&index).cloned();
        out.push(g);
    }
    out
}

/// The dimension the Sketch Dimension tool is making, laid out at `place` (the cursor, or where
/// it was clicked), with its measured value.
pub fn preview_geom(cache: &Cache, xf: &Xf, c: &Constraint, place: Option<super::Mm>) -> Option<DimGeom> {
    let number = cache.sketch.measure(c).map_or_else(|| "?".to_string(), trim2);
    layout(cache, xf, usize::MAX, c, place, &number)
}

/// Lay out dimension `c` (constraint `index`) with its value `number`, its value at `place`
/// (sketch mm) or, with none, beside its geometry.
fn layout(cache: &Cache, xf: &Xf, index: usize, c: &Constraint, place: Option<super::Mm>, number: &str) -> Option<DimGeom> {
    let sk = &cache.sketch;
    let pt = |id: u32| sk.point(id).map(|p| to_screen(xf, sk, [p.x, p.y]));
    let at = place.map(|p| to_screen(xf, sk, p));
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
    let mut g = DimGeom { index, segments: vec![], arrows: vec![], arc: vec![], label: Pos2::ZERO, text: String::new(), error: None };
    let laid = match c {
        Constraint::Length { line, .. } => line_ends(*line).map(|(p, q)| linear(&mut g, p, q, at)),
        Constraint::Distance { a, b, along, .. } => pt(*a).zip(pt(*b)).map(|(p, q)| match along {
            Along::Direct => linear(&mut g, p, q, at),
            Along::X => axis_linear(&mut g, p, q, at, true),
            Along::Y => axis_linear(&mut g, p, q, at, false),
        }),
        Constraint::Angle { line, .. } => line_ends(*line).map(|(p, _)| {
            let theta = sk.measure(c).unwrap_or(0.0) as f32;
            let r = at.map_or(ARC_RADIUS, |a| a.distance(p).max(12.0));
            g.segments.push([p, p + vec2(r + 10.0, 0.0)]);
            g.arc = arc_points(p, r, 0.0, theta);
            g.label = at.unwrap_or_else(|| on_circle(p, r + 16.0, theta / 2.0));
        }),
        Constraint::AngleBetween { a, b, .. } => line_ends(*a).zip(line_ends(*b)).map(|((p1, q1), (p2, q2))| {
            let theta = sk.measure(c).unwrap_or(0.0) as f32;
            let vertex = intersection(p1, q1, p2, q2).unwrap_or(p1);
            let from = screen_angle(q1 - p1);
            let r = at.map_or(ARC_RADIUS, |a| a.distance(vertex).max(12.0));
            g.arc = arc_points(vertex, r, from, theta);
            g.label = at.unwrap_or_else(|| on_circle(vertex, r + 16.0, from + theta / 2.0));
        }),
        Constraint::DistancePointLine { p, line, .. } => pt(*p).zip(line_ends(*line)).map(|(q, (a, b))| {
            let d = b - a;
            let len2 = d.length_sq();
            let foot = if len2 > 1e-6 { a + d * ((q - a).dot(d) / len2) } else { a };
            linear(&mut g, foot, q, at);
        }),
        Constraint::Radius { e, .. } => round(*e).map(|(centre, r)| {
            let theta = at.map_or(45.0, |a| screen_angle(a - centre));
            let tip = on_circle(centre, r, theta);
            let label = at.unwrap_or_else(|| on_circle(centre, r + 14.0, theta));
            g.segments.push([centre, tip]);
            if label.distance(centre) > r {
                g.segments.push([tip, label]);
            }
            g.arrows.push((tip, centre - tip));
            g.label = label;
        }),
        Constraint::Diameter { e, .. } => round(*e).map(|(centre, r)| {
            let theta = at.map_or(45.0, |a| screen_angle(a - centre));
            let (tip, tail) = (on_circle(centre, r, theta), on_circle(centre, r, theta + 180.0));
            let label = at.unwrap_or_else(|| on_circle(centre, r + 16.0, theta));
            g.segments.push([tail, tip]);
            if label.distance(centre) > r {
                g.segments.push([tip, label]);
            }
            g.arrows.push((tip, tail - tip));
            g.arrows.push((tail, tip - tail));
            g.label = label;
        }),
        _ => None,
    };
    laid?;
    g.text = match c {
        Constraint::Angle { .. } | Constraint::AngleBetween { .. } => format!("{number}\u{b0}"),
        Constraint::Radius { .. } => format!("R{number}"),
        Constraint::Diameter { .. } => format!("\u{d8}{number}"),
        _ => number.to_string(),
    };
    Some(g)
}

/// A length dimension between `p` and `q`: extension lines, a dimension line offset to the side
/// `at` is on (or 16 pt to the left), arrowheads, the value centred on it.
fn linear(g: &mut DimGeom, p: Pos2, q: Pos2, at: Option<Pos2>) {
    let d = q - p;
    let len = d.length();
    let u = if len > 1e-3 { d / len } else { vec2(1.0, 0.0) };
    let n = vec2(u.y, -u.x);
    let offset = at.map_or(DIM_OFFSET, |a| (a - p).dot(n));
    let ext = offset + 4.0 * offset.signum();
    let (p2, q2) = (p + n * offset, q + n * offset);
    g.segments.push([p, p + n * ext]);
    g.segments.push([q, q + n * ext]);
    g.segments.push([p2, q2]);
    g.arrows.push((p2, u));
    g.arrows.push((q2, -u));
    g.label = p2 + (q2 - p2) * 0.5;
    if let Some(a) = at {
        // Along the line, the value sits where it was put (clamped to the dimension line).
        let t = ((a - p2).dot(u) / len.max(1e-3)).clamp(0.0, 1.0);
        g.label = p2 + u * (t * len);
    }
}

/// A horizontal (`horizontal`) or vertical distance between `p` and `q`: the dimension line runs
/// along the axis at the level of `at` (or 16 pt above / left of the pair), extension lines reach
/// it from each point.
fn axis_linear(g: &mut DimGeom, p: Pos2, q: Pos2, at: Option<Pos2>, horizontal: bool) {
    if horizontal {
        let y = at.map_or(p.y.min(q.y) - DIM_OFFSET, |a| a.y);
        let (p2, q2) = (pos2(p.x, y), pos2(q.x, y));
        for (from, to) in [(p, p2), (q, q2)] {
            let dir = (to.y - from.y).signum();
            g.segments.push([from, pos2(to.x, to.y + 4.0 * dir)]);
        }
        let u = vec2((q.x - p.x).signum(), 0.0);
        g.segments.push([p2, q2]);
        g.arrows.push((p2, u));
        g.arrows.push((q2, -u));
        let x = at.map_or((p.x + q.x) * 0.5, |a| a.x.clamp(p.x.min(q.x), p.x.max(q.x)));
        g.label = pos2(x, y);
    } else {
        let x = at.map_or(p.x.min(q.x) - DIM_OFFSET, |a| a.x);
        let (p2, q2) = (pos2(x, p.y), pos2(x, q.y));
        for (from, to) in [(p, p2), (q, q2)] {
            let dir = (to.x - from.x).signum();
            g.segments.push([from, pos2(to.x + 4.0 * dir, to.y)]);
        }
        let u = vec2(0.0, (q.y - p.y).signum());
        g.segments.push([p2, q2]);
        g.arrows.push((p2, u));
        g.arrows.push((q2, -u));
        let y = at.map_or((p.y + q.y) * 0.5, |a| a.y.clamp(p.y.min(q.y), p.y.max(q.y)));
        g.label = pos2(x, y);
    }
}

/// What a constraint glyph draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sym {
    Horizontal,
    Vertical,
    Perpendicular,
    Parallel,
    Equal,
    Tangent,
    Fix,
    Midpoint,
    Concentric,
    Symmetric,
    Dot,
}

/// One constraint's glyph, laid out in screen space.
#[derive(Clone, Debug)]
pub struct Glyph {
    /// The constraint's index in the recipe.
    pub index: usize,
    pub rect: Rect,
    pub sym: Sym,
    pub name: &'static str,
}

pub const GLYPH: f32 = 14.0;

/// The glyph of every non-dimension constraint: beside its first entity (a line's middle, offset
/// 12 px; a circle's centre) or, with no entity, its first point; several on one thing sit side
/// by side.
pub fn glyph_geoms(cache: &Cache, xf: &Xf) -> Vec<Glyph> {
    let sk = &cache.sketch;
    let pt = |id: u32| sk.point(id).map(|p| to_screen(xf, sk, [p.x, p.y]));
    let mut stacked: std::collections::BTreeMap<(bool, u32), usize> = Default::default();
    let mut mirrored = std::collections::BTreeSet::new();
    let mut out = vec![];
    for (index, c) in sk.constraints.iter().enumerate() {
        // The origin's own pin is the sketch's, not the user's.
        if c.is_dimension() || matches!(c, Constraint::Fix { p: 0, .. }) {
            continue;
        }
        // A mirror makes many symmetric pairs about one line: one glyph shows them.
        if let Constraint::Symmetric { line, .. } = c
            && !mirrored.insert(*line)
        {
            continue;
        }
        let refs = c.refs();
        let (key, base, along) = if let Some(e) = refs.entities.first() {
            let laid = match sk.entity(*e) {
                Some(Entity::Line { a, b, .. }) => pt(*a).zip(pt(*b)).map(|(p, q)| {
                    let d = q - p;
                    let u = if d.length() > 1e-3 { d.normalized() } else { vec2(1.0, 0.0) };
                    (p + d * 0.5 + vec2(u.y, -u.x) * 12.0, u)
                }),
                Some(Entity::Circle { c, .. } | Entity::Arc { c, .. }) => pt(*c).map(|c| (c + vec2(12.0, -12.0), vec2(1.0, 0.0))),
                None => None,
            };
            let Some((base, along)) = laid else { continue };
            ((true, *e), base, along)
        } else if let Some(p) = refs.points.first() {
            let Some(at) = pt(*p) else { continue };
            ((false, *p), at + vec2(12.0, -12.0), vec2(1.0, 0.0))
        } else {
            continue;
        };
        let k = stacked.entry(key).or_insert(0);
        let centre = base + along * (*k as f32 * (GLYPH + 2.0));
        *k += 1;
        let (sym, name) = match c {
            Constraint::Horizontal { .. } => (Sym::Horizontal, tl!("Horizontal")),
            Constraint::Vertical { .. } => (Sym::Vertical, tl!("Vertical")),
            Constraint::Perpendicular { .. } => (Sym::Perpendicular, tl!("Perpendicular")),
            Constraint::Parallel { .. } => (Sym::Parallel, tl!("Parallel")),
            Constraint::Equal { .. } => (Sym::Equal, tl!("Equal")),
            Constraint::Tangent { .. } => (Sym::Tangent, tl!("Tangent")),
            Constraint::Fix { .. } => (Sym::Fix, tl!("Fixed")),
            Constraint::Midpoint { .. } => (Sym::Midpoint, tl!("Midpoint")),
            Constraint::Concentric { .. } => (Sym::Concentric, tl!("Concentric")),
            Constraint::Symmetric { .. } => (Sym::Symmetric, tl!("Symmetric")),
            Constraint::PointOnLine { .. } => (Sym::Dot, tl!("Point on line")),
            Constraint::PointOnCircle { .. } => (Sym::Dot, tl!("Point on circle")),
            _ => (Sym::Dot, tl!("Coincident")),
        };
        out.push(Glyph { index, rect: Rect::from_center_size(centre, Vec2::splat(GLYPH)), sym, name });
    }
    out
}

/// The symbol of a glyph, drawn with strokes (no font has all of them).
fn paint_sym(painter: &Painter, rect: Rect, sym: Sym, color: Color32) {
    let c = rect.center();
    let s = Stroke::new(1.3, color);
    let h = 4.0;
    match sym {
        Sym::Horizontal => {
            painter.line_segment([c - vec2(h, 0.0), c + vec2(h, 0.0)], s);
        }
        Sym::Vertical => {
            painter.line_segment([c - vec2(0.0, h), c + vec2(0.0, h)], s);
        }
        Sym::Perpendicular => {
            painter.line_segment([c + vec2(-h, h), c + vec2(h, h)], s);
            painter.line_segment([c + vec2(0.0, h), c + vec2(0.0, -h)], s);
        }
        Sym::Parallel => {
            painter.line_segment([c + vec2(-1.8, h), c + vec2(-1.8, -h)], s);
            painter.line_segment([c + vec2(1.8, h), c + vec2(1.8, -h)], s);
        }
        Sym::Equal => {
            painter.line_segment([c + vec2(-h, -1.8), c + vec2(h, -1.8)], s);
            painter.line_segment([c + vec2(-h, 1.8), c + vec2(h, 1.8)], s);
        }
        Sym::Tangent => {
            painter.circle_stroke(c, h, s);
        }
        Sym::Concentric => {
            painter.circle_stroke(c, h, s);
            painter.circle_filled(c, 1.5, color);
        }
        Sym::Midpoint => {
            painter.add(PathShape::closed_line(vec![c + vec2(0.0, -h), c + vec2(h, h * 0.8), c + vec2(-h, h * 0.8)], s));
        }
        Sym::Fix => {
            painter.rect_filled(Rect::from_center_size(c + vec2(0.0, 2.0), vec2(7.0, 5.0)), 1.0, color);
            painter.add(PathShape::line(vec![c + vec2(-2.0, 0.0), c + vec2(-2.0, -2.5), c + vec2(2.0, -2.5), c + vec2(2.0, 0.0)], s));
        }
        Sym::Symmetric => {
            painter.line_segment([c + vec2(0.0, -h), c + vec2(0.0, h)], s);
            painter.circle_filled(c - vec2(3.5, 0.0), 1.5, color);
            painter.circle_filled(c + vec2(3.5, 0.0), 1.5, color);
        }
        Sym::Dot => {
            painter.circle_filled(c, 2.2, color);
        }
    }
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

/// The geometry's ink: near-black when fully constrained, blue until then. The geometry sits on
/// the white artboard, so this does not follow the theme.
pub fn ink(constrained: bool) -> Color32 {
    if constrained { BLACK } else { BLUE }
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
            (ink(cache.constrained.contains(&e.id())), 1.5)
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
            ink(cache.point_constrained.contains(&p.id))
        };
        let half = if selected(pick) || m.hover == Some(pick) { 3.5 } else { 2.0 };
        painter.rect_filled(Rect::from_center_size(at, Vec2::splat(half * 2.0)), 0.0, color);
    }
    let origin = to_screen(xf, sk, [0.0, 0.0]);
    let cross = Stroke::new(1.0, t.text_dim);
    painter.line_segment([origin - vec2(6.0, 0.0), origin + vec2(6.0, 0.0)], cross);
    painter.line_segment([origin - vec2(0.0, 6.0), origin + vec2(0.0, 6.0)], cross);

    // Dimensions (at the place a drag is taking one to).
    let drag = m.drag_dim.as_ref().and_then(|d| d.at.map(|at| (d.index, at)));
    let geoms = if m.show_dims { dimension_geoms_dragging(cache, xf, drag) } else { vec![] };
    for g in &geoms {
        let pick = Pick::Dim(g.index);
        let color = if g.error.is_some() {
            t.error
        } else if selected(pick) {
            t.accent_strong
        } else if m.hover == Some(pick) {
            t.accent
        } else {
            DIM_INK
        };
        paint_dim(painter, g, color, t.error, false);
    }

    // Constraint glyphs.
    let mut tip = None;
    for g in if m.show_dims { glyph_geoms(cache, xf) } else { vec![] } {
        let pick = Pick::Glyph(g.index);
        let color = if selected(pick) {
            t.accent_strong
        } else if m.hover == Some(pick) {
            t.accent
        } else {
            DIM_INK
        };
        painter.rect_filled(g.rect, 3.0, CHIP);
        painter.rect_stroke(g.rect, 3.0, Stroke::new(if selected(pick) { 2.0 } else { 1.0 }, color), egui::StrokeKind::Inside);
        paint_sym(painter, g.rect, g.sym, color);
        if m.hover == Some(pick) {
            tip = Some((g.rect, g.name));
        }
    }
    if let Some((rect, name)) = tip {
        let w = painter.layout_no_wrap(name.to_string(), FontId::proportional(11.0), CHIP_TEXT).size().x + 12.0;
        let r = Rect::from_min_size(rect.left_bottom() + vec2(0.0, 4.0), vec2(w, 18.0));
        painter.rect_filled(r, 4.0, CHIP);
        painter.rect_stroke(r, 4.0, Stroke::new(1.0, DIM_INK), egui::StrokeKind::Inside);
        painter.text(r.center(), Align2::CENTER_CENTER, name, FontId::proportional(11.0), CHIP_TEXT);
    }

    // What the modify tools would do.
    modify_preview(painter, xf, cache, t, m);

    // The dimension being made: it follows the cursor until placed; then its box opens on it.
    if let Some((c, hud)) = &m.new_dim {
        let place = m.dim_place.or(m.rubber);
        match preview_geom(cache, xf, c, place) {
            Some(g) => {
                paint_dim(painter, &g, t.accent, t.error, m.dim_place.is_none());
                if m.dim_place.is_some() {
                    paint_hud(painter, t, hud, &[g.label], cache);
                }
            }
            None => {
                let at = refs_centre(cache, xf, c) + vec2(0.0, -34.0);
                paint_hud(painter, t, hud, &[at], cache);
            }
        }
    }

    // The shape being drawn.
    if m.tool != SketchTool::None {
        if let Some(Pick::Point(p)) = m.hover
            && let Some(at) = pt(p)
        {
            painter.circle_stroke(at, 6.0, Stroke::new(1.5, t.accent));
        }
        shape_rubber(painter, xf, cache, t, m);
    }

    // A dimension being edited in place.
    if let Some((index, hud)) = &m.edit_dim
        && let Some(g) = geoms.iter().find(|g| g.index == *index)
    {
        paint_hud(painter, t, hud, &[g.label], cache);
    }
}

/// One dimension: its lines, arrowheads and arc, and the value on a chip (`ghost`: the chip has
/// no border, for the dimension still following the cursor).
fn paint_dim(painter: &Painter, g: &DimGeom, color: Color32, error: Color32, ghost: bool) {
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
    painter.rect_filled(rect, 4.0, CHIP);
    if !ghost {
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
    }
    painter.text(g.label, Align2::CENTER_CENTER, &g.text, FontId::proportional(12.0), if g.error.is_some() { error } else { CHIP_TEXT });
}

/// The Trim, Offset and Mirror previews: the piece Trim would remove (red, dashed), the copy
/// Offset would make (blue, dashed) with its box, the lines chosen for Offset and Mirror.
fn modify_preview(painter: &Painter, xf: &Xf, cache: &Cache, t: &Tokens, m: &SketchMode) {
    let sk = &cache.sketch;
    let sc = |p: (f64, f64)| to_screen(xf, sk, [p.0, p.1]);
    match m.tool {
        SketchTool::Trim => {
            if let (Some(Pick::Entity(e)), Some(at)) = (m.hover, m.rubber)
                && let Ok(plan) = sk.trim_plan(e, (at[0], at[1]))
            {
                dashed(painter, &[sc(plan.from), sc(plan.to)], Stroke::new(3.0, t.error));
            }
        }
        SketchTool::Offset | SketchTool::Mirror => {
            for line in &m.modify {
                if let Some(Entity::Line { a, b, .. }) = sk.entity(*line)
                    && let (Some(p), Some(q)) = (sk.point(*a), sk.point(*b))
                {
                    painter.line_segment([sc((p.x, p.y)), sc((q.x, q.y))], Stroke::new(3.0, t.accent_strong));
                }
            }
            if m.tool != SketchTool::Offset || m.modify.is_empty() {
                return;
            }
            let Some(distance) = super::tools::offset_distance(m, &cache.params).filter(|d| *d > 1e-9) else { return };
            let Ok(chain) = sk.chain_of(&m.modify) else { return };
            let Ok(places) = sk.offset_positions(&chain, distance, m.side) else { return };
            let mut pts: Vec<Pos2> = places.iter().map(|p| sc(*p)).collect();
            if chain.closed
                && let Some(first) = pts.first().copied()
            {
                pts.push(first);
            }
            dashed(painter, &pts, Stroke::new(1.5, BLUE));
            if let (Some(hud), Some(mouse)) = (&m.hud, m.rubber) {
                paint_hud(painter, t, hud, &[to_screen(xf, sk, mouse) + vec2(56.0, -22.0)], cache);
            }
        }
        _ => {}
    }
}

/// The middle of the things `c` refers to, on screen.
fn refs_centre(cache: &Cache, xf: &Xf, c: &Constraint) -> Pos2 {
    let sk = &cache.sketch;
    let refs = c.refs();
    let mut ids = refs.points.clone();
    for e in &refs.entities {
        match sk.entity(*e) {
            Some(Entity::Line { a, b, .. }) => ids.extend([*a, *b]),
            Some(Entity::Circle { c, .. } | Entity::Arc { c, .. }) => ids.push(*c),
            None => {}
        }
    }
    let pts: Vec<Pos2> = ids.iter().filter_map(|i| sk.point(*i)).map(|p| to_screen(xf, sk, [p.x, p.y])).collect();
    if pts.is_empty() { Pos2::ZERO } else { Rect::from_points(&pts).center() }
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
        // The angle box sits beside the arc on the side of the horizontal away from the line (below
        // it for a positive angle); the length box is on the other side of the line, far enough
        // out that neither box covers it.
        let below = if angle >= 0.0 { 1.0 } else { -1.0 };
        let angle_at = s + vec2(70.0, 22.0 * below);
        let side = if n.dot(angle_at - mid) > 0.0 { -1.0 } else { 1.0 };
        let reach = 8.0 + 32.0 * n.x.abs() + 10.0 * n.y.abs();
        let length_at = mid + n * (side * reach);
        paint_hud(painter, t, hud, &[length_at, angle_at], cache);
    }
}

/// The rubber band of the rectangle, circle, arc and fillet tools, and their boxes.
fn shape_rubber(painter: &Painter, xf: &Xf, cache: &Cache, t: &Tokens, m: &SketchMode) {
    let sk = &cache.sketch;
    let sc = |p: [f64; 2]| to_screen(xf, sk, p);
    let blue = Stroke::new(1.5, BLUE);
    let (Some(start), Some(rubber)) = (m.start, m.rubber) else {
        // The corners Fillet can round.
        if m.tool == SketchTool::Fillet {
            for id in corner_points(sk) {
                if let Some(p) = sk.point(id) {
                    painter.circle_stroke(sc([p.x, p.y]), 5.0, Stroke::new(1.0, t.text_dim));
                }
            }
        }
        return;
    };
    let from = start_pos(sk, start);
    match m.tool {
        SketchTool::Line | SketchTool::None | SketchTool::Dimension | SketchTool::Trim | SketchTool::Offset | SketchTool::Mirror => {
            if m.tool == SketchTool::Line {
                line_rubber(painter, xf, cache, t, m, from, rubber);
            }
        }
        SketchTool::Rect2 | SketchTool::RectCenter => {
            let pts: Vec<Pos2> = rect_corners(m.tool, from, rubber).iter().map(|c| sc(*c)).collect();
            painter.add(PathShape::closed_line(pts.clone(), blue));
            if let Some(hud) = &m.hud {
                let bounds = Rect::from_points(&pts);
                let top = pos2(bounds.center().x, bounds.top() - 24.0);
                let right = pos2(bounds.right() + 50.0, bounds.center().y);
                paint_hud(painter, t, hud, &[top, right], cache);
            }
        }
        SketchTool::Circle => {
            let r = (rubber[0] - from[0]).hypot(rubber[1] - from[1]);
            let pts: Vec<Pos2> = (0..=72)
                .map(|i| {
                    let a = std::f64::consts::TAU * f64::from(i) / 72.0;
                    sc([from[0] + r * a.cos(), from[1] - r * a.sin()])
                })
                .collect();
            painter.add(PathShape::line(pts, blue));
            let c = sc(from);
            painter.line_segment([c, sc(rubber)], Stroke::new(1.0, t.text_dim));
            if let Some(hud) = &m.hud {
                let reach = (sc(rubber) - c).length().max(8.0);
                paint_hud(painter, t, hud, &[c + vec2(reach + 50.0, 0.0)], cache);
            }
        }
        SketchTool::Arc3 => {
            let Some(second) = m.second else {
                painter.line_segment([sc(from), sc(rubber)], blue);
                return;
            };
            let end = start_pos(sk, second);
            let radius = m.hud.as_ref().and_then(|h| typed(h, 0, &cache.params));
            match arc_center(from, end, rubber, radius) {
                Some(c) => {
                    let pts: Vec<Pos2> = arc_polyline(from, end, rubber, c).iter().map(|p| sc(*p)).collect();
                    painter.add(PathShape::line(pts, blue));
                    painter.circle_filled(sc(c), 2.0, t.text_dim);
                }
                None => {
                    painter.line_segment([sc(from), sc(end)], blue);
                }
            }
            if let Some(hud) = &m.hud {
                let far = sc(rubber);
                let away = (far - sc(arc_center(from, end, rubber, None).unwrap_or(from))).normalized();
                let away = if away.length() > 0.5 { away } else { vec2(1.0, -1.0).normalized() };
                paint_hud(painter, t, hud, &[far + away * 44.0], cache);
            }
        }
        SketchTool::Fillet => {
            let corner = sc(from);
            painter.circle_stroke(corner, 7.0, Stroke::new(1.5, t.accent_strong));
            if let Some(hud) = &m.hud {
                paint_hud(painter, t, hud, &[corner + vec2(60.0, -30.0)], cache);
            }
        }
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
    if focused && !b.typed && !b.text.is_empty() {
        // The value is selected as a whole: typing replaces it.
        let sel = Rect::from_min_size(rect.left_top() + vec2(5.0, 3.0), vec2(text_w + 4.0, rect.height() - 6.0));
        painter.rect_filled(sel, 2.0, t.accent.gamma_multiply(0.45));
    }
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
