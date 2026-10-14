//! Picking in screen space: sketch points, entities and dimension labels.

use egui::{Pos2, Vec2};
use vectorcraft_doc::sketch::model::Entity;

use super::{Cache, Pick, to_screen};
use crate::canvas::Xf;

/// How near (screen points) the pointer must be to snap to a point.
pub const POINT_SNAP: f32 = 8.0;
/// How near the pointer must be to a line, circle or arc to pick it.
pub const ENTITY_PICK: f32 = 6.0;

/// What is under `pos`: a dimension label first (it is drawn on top), then a point, then an
/// entity.
pub fn pick(cache: &Cache, xf: &Xf, pos: Pos2, with_dims: bool) -> Option<Pick> {
    if with_dims && let Some(i) = dim_at(cache, xf, pos) {
        return Some(Pick::Dim(i));
    }
    if let Some(p) = nearest_point(cache, xf, pos, POINT_SNAP, None) {
        return Some(Pick::Point(p));
    }
    entity_at(cache, xf, pos, ENTITY_PICK).map(Pick::Entity)
}

/// The sketch point nearest `pos` within `within` screen points (`exclude` is skipped).
pub fn nearest_point(cache: &Cache, xf: &Xf, pos: Pos2, within: f32, exclude: Option<u32>) -> Option<u32> {
    let mut best: Option<(f32, u32)> = None;
    for p in cache.sketch.points.iter().filter(|p| Some(p.id) != exclude) {
        let d = to_screen(xf, &cache.sketch, [p.x, p.y]).distance(pos);
        if d <= within && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, p.id));
        }
    }
    best.map(|(_, id)| id)
}

/// The dimension whose value label is under `pos`.
pub fn dim_at(cache: &Cache, xf: &Xf, pos: Pos2) -> Option<usize> {
    super::overlay::dimension_geoms(cache, xf).into_iter().find(|g| g.label_rect().contains(pos)).map(|g| g.index)
}

/// The entity nearest `pos` within `within` screen points.
pub fn entity_at(cache: &Cache, xf: &Xf, pos: Pos2, within: f32) -> Option<u32> {
    let at = |id: u32| cache.sketch.point(id).map(|p| to_screen(xf, &cache.sketch, [p.x, p.y]));
    let mut best: Option<(f32, u32)> = None;
    for e in &cache.sketch.entities {
        let d = match e {
            Entity::Line { a, b, .. } => match (at(*a), at(*b)) {
                (Some(p), Some(q)) => segment_distance(pos, p, q),
                _ => continue,
            },
            Entity::Circle { c, r, .. } => match at(*c) {
                Some(c) => (c.distance(pos) - (*r * super::px_per_mm(xf)) as f32).abs(),
                None => continue,
            },
            Entity::Arc { c, a, b, .. } => match (at(*c), at(*a), at(*b)) {
                (Some(c), Some(a), Some(b)) => arc_distance(pos, c, a, b),
                _ => continue,
            },
        };
        if d <= within && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, e.id()));
        }
    }
    best.map(|(_, id)| id)
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab: Vec2 = b - a;
    let len2 = ab.length_sq();
    if len2 < 1e-9 {
        return p.distance(a);
    }
    let t = (((p - a).dot(ab)) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Angle of `p` around `c` on screen, counter-clockwise (y is down on screen).
fn angle(c: Pos2, p: Pos2) -> f32 {
    (-(p.y - c.y)).atan2(p.x - c.x)
}

/// Distance from `p` to the arc around `c` from `a` counter-clockwise to `b`.
fn arc_distance(p: Pos2, c: Pos2, a: Pos2, b: Pos2) -> f32 {
    let r = c.distance(a);
    let tau = std::f32::consts::TAU;
    let sweep = (angle(c, b) - angle(c, a)).rem_euclid(tau);
    let along = (angle(c, p) - angle(c, a)).rem_euclid(tau);
    if along <= sweep { (c.distance(p) - r).abs() } else { p.distance(a).min(p.distance(b)) }
}
