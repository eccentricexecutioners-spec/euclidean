//! Sketch v2 output: the solved lines, arcs and circles as paths in page points. Lines and arcs
//! that share vertices chain into closed profiles or open chains; circles stand alone;
//! construction geometry is skipped.

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, TAU};

use vectorcraft_geom::{Anchor, PathData, Point, Rect, SubPath, shapes};

use super::Sketch;
use super::model::{Constraint, Entity};
use crate::Unit;

/// Coordinates past this (points) are refused: the canvas has limits.
const MAX_COORD: f64 = 1.0e7;

/// One drawable edge between two vertices (union-find representatives).
struct Edge {
    /// Index into `Sketch::entities`.
    entity: usize,
    from: u32,
    to: u32,
}

/// An edge walked in a direction.
struct Step {
    entity: usize,
    forward: bool,
}

struct Vertices {
    parent: BTreeMap<u32, u32>,
}

impl Vertices {
    fn find(&mut self, mut p: u32) -> u32 {
        while let Some(&up) = self.parent.get(&p) {
            if up == p {
                break;
            }
            p = up;
        }
        p
    }

    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            let (lo, hi) = (ra.min(rb), ra.max(rb));
            self.parent.insert(hi, lo);
        }
    }
}

fn to_pt(mm: f64, origin: f64) -> f64 {
    Unit::Millimeters.to_pt(mm + origin)
}

/// The paths of `sketch` in page points (the origin applied), each with its name: `Profile n` for
/// a closed loop, `Chain n` for an open chain, `Circle n` for a circle. A path with a coordinate
/// past the canvas's limits is left out.
pub fn paths(sketch: &Sketch) -> Vec<(String, PathData)> {
    let pos = |id: u32| sketch.point(id).map(|p| Point::new(to_pt(p.x, sketch.origin[0]), to_pt(p.y, sketch.origin[1])));
    let mut vertices = Vertices { parent: sketch.points.iter().map(|p| (p.id, p.id)).collect() };
    for c in &sketch.constraints {
        if let Constraint::Coincident { a, b } = c
            && vertices.parent.contains_key(a)
            && vertices.parent.contains_key(b)
        {
            vertices.union(*a, *b);
        }
    }
    let mut edges = Vec::new();
    for (i, e) in sketch.entities.iter().enumerate() {
        if e.construction() {
            continue;
        }
        let (a, b) = match e {
            Entity::Line { a, b, .. } | Entity::Arc { a, b, .. } => (*a, *b),
            Entity::Circle { .. } => continue,
        };
        let (from, to) = (vertices.find(a), vertices.find(b));
        if from != to {
            edges.push(Edge { entity: i, from, to });
        }
    }

    let mut out = Vec::new();
    let (mut profiles, mut chains) = (0, 0);
    for (trail, closed) in trails(&edges) {
        let Some(path) = trail_path(sketch, &edges, &trail, closed, &pos) else { continue };
        let name = if closed {
            profiles += 1;
            format!("Profile {profiles}")
        } else {
            chains += 1;
            format!("Chain {chains}")
        };
        out.push((name, path));
    }
    let mut circles = 0;
    for e in &sketch.entities {
        if let Entity::Circle { c, r, construction: false, .. } = e
            && let Some(centre) = pos(*c)
        {
            let r = Unit::Millimeters.to_pt(*r);
            if ok(centre.x.abs() + r) && ok(centre.y.abs() + r) && r > 0.0 {
                circles += 1;
                out.push((format!("Circle {circles}"), shapes::ellipse(Rect::new(centre.x - r, centre.y - r, centre.x + r, centre.y + r))));
            }
        }
    }
    out
}

fn ok(v: f64) -> bool {
    v.is_finite() && v <= MAX_COORD
}

/// Split the edges into trails: open ones from odd-degree vertices first, then the cycles left.
/// Each comes with whether it is closed (it ends where it started).
fn trails(edges: &[Edge]) -> Vec<(Vec<Step>, bool)> {
    let mut at: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, e) in edges.iter().enumerate() {
        at.entry(e.from).or_default().push(i);
        at.entry(e.to).or_default().push(i);
    }
    let mut used = vec![false; edges.len()];
    let mut out = Vec::new();
    let odd: Vec<u32> = at.iter().filter(|(_, v)| v.len() % 2 == 1).map(|(k, _)| *k).collect();
    let starts = odd.into_iter().chain(edges.iter().map(|e| e.from));
    for start in starts {
        loop {
            let (mut cur, mut trail) = (start, Vec::new());
            while let Some(i) = at.get(&cur).and_then(|v| v.iter().copied().find(|i| !used[*i])) {
                used[i] = true;
                let e = &edges[i];
                let forward = e.from == cur;
                trail.push(Step { entity: i, forward });
                cur = if forward { e.to } else { e.from };
            }
            if trail.is_empty() {
                break;
            }
            let closed = cur == start && trail.len() > 1;
            out.push((trail, closed));
        }
    }
    out
}

/// A path under construction: each anchor with its handles.
struct Builder {
    anchors: Vec<(Point, Point, Point)>,
}

impl Builder {
    fn line_to(&mut self, p: Point) {
        self.anchors.push((p, p, p));
    }

    fn curve_to(&mut self, c1: Point, c2: Point, p: Point) {
        if let Some(last) = self.anchors.last_mut() {
            last.2 = c1;
        }
        self.anchors.push((p, c2, p));
    }
}

/// Add the arc around `c` from `from` to `to` (both in page points), `ccw` being counter-clockwise
/// on screen, as cubic béziers of at most 90° each.
fn arc_to(b: &mut Builder, c: Point, from: Point, to: Point, ccw: bool) {
    // Screen angle: counter-clockwise on a y-down page.
    let ang = |p: Point| (-(p.y - c.y)).atan2(p.x - c.x);
    let at = |phi: f64, r: f64| Point::new(c.x + r * phi.cos(), c.y - r * phi.sin());
    let r = (from - c).hypot();
    let sweep = if ccw { (ang(to) - ang(from)).rem_euclid(TAU) } else { -(ang(from) - ang(to)).rem_euclid(TAU) };
    let n = ((sweep.abs() / FRAC_PI_2) - 1e-9).ceil().max(1.0) as usize;
    let delta = sweep / n as f64;
    let k = 4.0 / 3.0 * (delta / 4.0).tan();
    let mut phi = ang(from);
    for i in 0..n {
        let next = phi + delta;
        let (p0, p3) = (at(phi, r), if i + 1 == n { to } else { at(next, r) });
        // d/dphi of (cos, -sin) is (-sin, -cos).
        let c1 = Point::new(p0.x + k * r * -phi.sin(), p0.y + k * r * -phi.cos());
        let p3_ideal = at(next, r);
        let c2 = Point::new(p3_ideal.x - k * r * -next.sin(), p3_ideal.y - k * r * -next.cos());
        b.curve_to(c1, c2, p3);
        phi = next;
    }
}

fn trail_path(sketch: &Sketch, edges: &[Edge], trail: &[Step], closed: bool, pos: &dyn Fn(u32) -> Option<Point>) -> Option<PathData> {
    let mut b = Builder { anchors: Vec::new() };
    for (i, step) in trail.iter().enumerate() {
        let entity = sketch.entities.get(edges.get(step.entity)?.entity)?;
        let (from, to) = match entity {
            Entity::Line { a, b, .. } | Entity::Arc { a, b, .. } => {
                if step.forward {
                    (pos(*a)?, pos(*b)?)
                } else {
                    (pos(*b)?, pos(*a)?)
                }
            }
            Entity::Circle { .. } => return None,
        };
        if i == 0 {
            b.line_to(from);
        }
        match entity {
            Entity::Arc { c, .. } => arc_to(&mut b, pos(*c)?, from, to, step.forward),
            _ => b.line_to(to),
        }
    }
    if closed && b.anchors.len() > 1 {
        let (_, h_in, _) = b.anchors.pop()?;
        b.anchors.first_mut()?.1 = h_in;
    }
    let inside = |p: Point| p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD;
    if !b.anchors.iter().all(|(p, i, o)| inside(*p) && inside(*i) && inside(*o)) {
        return None;
    }
    let anchors = b.anchors.into_iter().map(|(p, i, o)| Anchor::with_handles(p, i, o)).collect();
    Some(PathData::new(vec![SubPath::new(anchors, closed)]))
}

#[cfg(test)]
mod tests {
    use super::super::model::tests::rectangle;
    use super::*;

    fn mm(v: f64) -> f64 {
        Unit::Millimeters.to_pt(v)
    }

    #[test]
    fn a_closed_loop_is_one_closed_path() {
        let out = paths(&rectangle());
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].0, "Profile 1");
        let subs = &out[0].1.subpaths;
        assert_eq!(subs.len(), 1);
        assert!(subs[0].closed && subs[0].anchors.len() == 4, "{:?}", subs[0].anchors.len());
        let bounds = out[0].1.bounds().unwrap();
        assert!((bounds.width() - mm(50.0)).abs() < 1e-9 && (bounds.height() - mm(30.0)).abs() < 1e-9, "{bounds:?}");
        // The origin offsets everything.
        let mut moved = rectangle();
        moved.origin = [10.0, 5.0];
        let bounds = paths(&moved)[0].1.bounds().unwrap();
        assert!((bounds.x0 - mm(10.0)).abs() < 1e-9 && (bounds.y0 - mm(5.0)).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn an_open_chain_is_one_open_path() {
        let mut s = Sketch::new_v2();
        let (p1, p2) = (s.add_point(20.0, 0.0), s.add_point(20.0, 20.0));
        s.add_line(0, p1, false);
        s.add_line(p1, p2, false);
        let out = paths(&s);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "Chain 1");
        let sub = &out[0].1.subpaths[0];
        assert!(!sub.closed && sub.anchors.len() == 3);
        // Lines drawn the other way round, and joined by a Coincident pair, still chain.
        let mut s = Sketch::new_v2();
        let (a, b, c, d) = (s.add_point(10.0, 0.0), s.add_point(10.0, 10.0), s.add_point(10.0, 10.0), s.add_point(0.0, 10.0));
        s.add_line(a, b, false);
        s.add_line(d, c, false);
        s.constraints.push(Constraint::Coincident { a: b, b: c });
        let out = paths(&s);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(!out[0].1.subpaths[0].closed && out[0].1.subpaths[0].anchors.len() == 3);
    }

    #[test]
    fn construction_is_not_output() {
        let mut s = Sketch::new_v2();
        let p = s.add_point(30.0, 0.0);
        s.add_line(0, p, true);
        let c = s.add_circle(p, 5.0);
        if let Some(Entity::Circle { construction, .. }) = s.entities.iter_mut().find(|e| e.id() == c) {
            *construction = true;
        }
        assert!(paths(&s).is_empty());
        // A circle that is not construction comes out by itself.
        s.add_circle(p, 5.0);
        let out = paths(&s);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "Circle 1");
        let bounds = out[0].1.bounds().unwrap();
        assert!((bounds.width() - mm(10.0)).abs() < 1e-6 && (bounds.center().x - mm(30.0)).abs() < 1e-6, "{bounds:?}");
    }

    #[test]
    fn a_filleted_rectangle_is_one_closed_path_with_an_arc() {
        let mut s = rectangle();
        let (arc, t1, t2) = s.fillet(2, 5.0).unwrap();
        assert!(arc < 10 && t1 != t2);
        let out = paths(&s);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].0, "Profile 1");
        let sub = &out[0].1.subpaths[0];
        assert!(sub.closed);
        // Four corners become five anchors (the rounded one is two), with a curve between.
        assert_eq!(sub.anchors.len(), 5, "{:?}", sub.anchors);
        assert_eq!(sub.anchors.iter().filter(|a| a.has_in() || a.has_out()).count(), 2);
        let bounds = out[0].1.bounds().unwrap();
        assert!((bounds.width() - mm(50.0)).abs() < 1e-9 && (bounds.height() - mm(30.0)).abs() < 1e-9, "{bounds:?}");
        // The curve passes through the arc's midpoint (45 + 5/sqrt2).
        let mid = (mm(45.0 + 5.0 / 2f64.sqrt()), mm(25.0 + 5.0 / 2f64.sqrt()));
        let curve_ok = sub.anchors.windows(2).any(|w| {
            let (a, c) = (w[0], w[1]);
            let seg = kurbo::CubicBez::new(a.p, a.h_out, c.h_in, c.p);
            let m = kurbo::ParamCurve::eval(&seg, 0.5);
            a.has_out() && (m.x - mid.0).abs() < mm(0.05) && (m.y - mid.1).abs() < mm(0.05)
        });
        assert!(curve_ok, "{:?}", sub.anchors);
    }

    #[test]
    fn arcs_run_counter_clockwise_on_screen() {
        // A quarter arc about (0,0) from (10,0) (right) to (0,-10) (up on screen).
        let mut s = Sketch::new_v2();
        let (a, b) = (s.add_point(10.0, 0.0), s.add_point(0.0, -10.0));
        s.add_arc(0, a, b);
        let out = paths(&s);
        assert_eq!(out[0].0, "Chain 1");
        let bounds = out[0].1.bounds().unwrap();
        assert!(bounds.y0 < -mm(9.9) && bounds.y1 > -1e-9 && bounds.x0 > -1e-9 && bounds.x1 < mm(10.0) + 1e-9, "{bounds:?}");
        // The long way round is three quarters of the circle.
        let mut s = Sketch::new_v2();
        let (a, b) = (s.add_point(0.0, -10.0), s.add_point(10.0, 0.0));
        s.add_arc(0, a, b);
        let bounds = paths(&s)[0].1.bounds().unwrap();
        assert!((bounds.x0 + mm(10.0)).abs() < 1e-6 && (bounds.y1 - mm(10.0)).abs() < 1e-6, "{bounds:?}");
    }

    #[test]
    fn paths_past_the_canvas_are_left_out() {
        let mut s = Sketch::new_v2();
        let p = s.add_point(1.0e9, 0.0);
        s.add_line(0, p, false);
        assert!(paths(&s).is_empty());
    }
}
