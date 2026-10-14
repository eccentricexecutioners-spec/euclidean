//! The adapter between a [`Sketch`] recipe and the `sketch-solver` crate: it builds a fresh
//! solver sketch for every solve, feeds it the dimensions' values from the parameters, and reads
//! the solved points back.
//!
//! The solver works with y up; the recipe has y down. Coordinates are flipped on the way in and
//! out, so the solver's counter-clockwise is counter-clockwise on screen, as angles and arcs are
//! in the recipe.

use std::collections::{BTreeMap, BTreeSet};

use sketch_solver as ss;

use super::Sketch;
use super::model::{Constraint, Entity};
use crate::expr::Solved;

/// The solver refuses sketches with more unknowns than this (its cost grows with the cube).
const MAX_UNKNOWNS: usize = 2000;
/// A coordinate that can move by less than this (of 1) counts as pinned.
const PINNED: f64 = 1e-6;

/// A solved sketch.
#[derive(Clone, Debug)]
pub struct Solution {
    /// The sketch with its points and circle radii updated (y flipped back).
    pub sketch: Sketch,
    /// Ids of the entities that are fully constrained.
    pub constrained: BTreeSet<u32>,
    /// Ids of the points that are fully constrained.
    pub point_constrained: BTreeSet<u32>,
    /// Constraint index to the error of its dimension's parameter.
    pub errors: BTreeMap<usize, String>,
    /// Degrees of freedom left.
    pub dof: usize,
}

/// Why a sketch could not be solved.
#[derive(Debug, Clone, PartialEq)]
pub enum SolveError {
    /// The solver did not converge (the caller keeps the previous geometry).
    NoSolution,
    /// Constraints contradict each other, or a constraint adds nothing.
    OverConstrained,
}

/// The solver sketch and how it maps back to the recipe.
struct Built {
    s: ss::Sketch,
    points: BTreeMap<u32, ss::EntityId>,
    entities: BTreeMap<u32, ss::EntityId>,
    /// Recipe constraint index to its solver id, when it was added.
    added: Vec<Option<ss::ConstraintId>>,
    errors: BTreeMap<usize, String>,
}

fn flip(y: f64) -> f64 {
    0.0 - y
}

/// The value of dimension `c` from its parameter, checked for the kind of quantity.
fn dimension(c: &Constraint, params: &Solved) -> Result<f64, String> {
    let Some(param) = c.param() else { return Err("not a dimension".into()) };
    let v = params.eval(param)?;
    if !v.is_finite() {
        return Err("not a number".into());
    }
    if !c.is_angular() && v < 0.0 {
        return Err("must not be negative".into());
    }
    Ok(v)
}

fn build(sketch: &Sketch, params: &Solved) -> Option<Built> {
    let mut s = ss::Sketch::new();
    let mut points = BTreeMap::new();
    for p in &sketch.points {
        points.insert(p.id, s.point(Some([p.x, flip(p.y)])).ok()?);
    }
    let mut entities = BTreeMap::new();
    for e in &sketch.entities {
        let made = match e {
            Entity::Line { a, b, .. } => s.line(*points.get(a)?, *points.get(b)?),
            Entity::Circle { c, r, .. } => s.circle(*points.get(c)?, Some(*r)),
            Entity::Arc { c, a, b, .. } => s.arc(*points.get(c)?, *points.get(a)?, *points.get(b)?, false),
        };
        // An entity the solver refuses is left out; what refers to it is left out too.
        if let Ok(id) = made {
            entities.insert(e.id(), id);
        }
    }
    let mut b = Built { s, points, entities, added: vec![None; sketch.constraints.len()], errors: BTreeMap::new() };
    let mut horizontal: Option<ss::EntityId> = None;
    for (i, c) in sketch.constraints.iter().enumerate() {
        let value = if c.is_dimension() {
            match dimension(c, params) {
                Ok(v) => Some(v),
                Err(e) => {
                    b.errors.insert(i, e);
                    continue;
                }
            }
        } else {
            None
        };
        if matches!(c, Constraint::Angle { .. }) && horizontal.is_none() {
            horizontal = hidden_horizontal(&mut b);
        }
        let Some(sc) = convert(&b, sketch, c, value, horizontal) else { continue };
        if let Ok(id) = b.s.add(sc) {
            b.added[i] = Some(id);
        }
    }
    Some(b)
}

/// The solver-only horizontal reference line for `Angle` constraints: two points fixed at (0, 0)
/// and (1, 0). Never written back.
fn hidden_horizontal(b: &mut Built) -> Option<ss::EntityId> {
    let p = b.s.point(Some([0.0, 0.0])).ok()?;
    let q = b.s.point(Some([1.0, 0.0])).ok()?;
    let line = b.s.line(p, q).ok()?;
    for e in [p, q] {
        b.s.add(ss::Constraint::Fix { entity: e, at: None }).ok()?;
    }
    Some(line)
}

fn centre_of(sketch: &Sketch, entity: u32) -> Option<u32> {
    match sketch.entity(entity)? {
        Entity::Circle { c, .. } | Entity::Arc { c, .. } => Some(*c),
        Entity::Line { .. } => None,
    }
}

/// The solver form of `c`, or `None` when something it refers to is not in the solver.
fn convert(b: &Built, sketch: &Sketch, c: &Constraint, value: Option<f64>, horizontal: Option<ss::EntityId>) -> Option<ss::Constraint> {
    let p = |id: &u32| b.points.get(id).copied();
    let e = |id: &u32| b.entities.get(id).copied();
    let v = value.unwrap_or(0.0);
    Some(match c {
        Constraint::Coincident { a, b } => ss::Constraint::Coincident(p(a)?, p(b)?),
        Constraint::PointOnLine { p: pt, line } => ss::Constraint::On { point: p(pt)?, curve: e(line)? },
        Constraint::PointOnCircle { p: pt, e: en } => ss::Constraint::On { point: p(pt)?, curve: e(en)? },
        Constraint::Horizontal { line } => ss::Constraint::Horizontal(ss::Pair::Line(e(line)?)),
        Constraint::Vertical { line } => ss::Constraint::Vertical(ss::Pair::Line(e(line)?)),
        Constraint::Parallel { a, b } => ss::Constraint::Parallel(e(a)?, e(b)?),
        Constraint::Perpendicular { a, b } => ss::Constraint::Perpendicular(e(a)?, e(b)?),
        Constraint::Tangent { a, b } => ss::Constraint::Tangent(e(a)?, e(b)?),
        Constraint::Equal { a, b } => ss::Constraint::Equal(e(a)?, e(b)?),
        Constraint::Fix { p: pt, x, y } => ss::Constraint::Fix { entity: p(pt)?, at: Some([*x, flip(*y)]) },
        Constraint::Midpoint { p: pt, line } => ss::Constraint::Midpoint { point: p(pt)?, line: e(line)? },
        Constraint::Concentric { a, b } => ss::Constraint::Coincident(p(&centre_of(sketch, *a)?)?, p(&centre_of(sketch, *b)?)?),
        Constraint::Distance { a, b, .. } => ss::Constraint::Distance { a: p(a)?, b: p(b)?, value: v, along: ss::Along::Direct },
        Constraint::DistancePointLine { p: pt, line, .. } => ss::Constraint::Distance { a: p(pt)?, b: e(line)?, value: v, along: ss::Along::Direct },
        Constraint::Symmetric { a, b, line } => ss::Constraint::Symmetric { a: p(a)?, b: p(b)?, about: e(line)? },
        Constraint::Length { line, .. } => ss::Constraint::Length { line: e(line)?, value: v },
        Constraint::Angle { line, .. } => ss::Constraint::Angle { from: horizontal?, to: e(line)?, degrees: v },
        Constraint::AngleBetween { a, b, .. } => ss::Constraint::Angle { from: e(a)?, to: e(b)?, degrees: v },
        Constraint::Radius { e: en, .. } => ss::Constraint::Radius { curve: e(en)?, value: v },
        Constraint::Diameter { e: en, .. } => ss::Constraint::Diameter { curve: e(en)?, value: v },
    })
}

fn run(b: &Built) -> Result<ss::Solution, SolveError> {
    let opt = ss::SolveOptions { max_unknowns: MAX_UNKNOWNS, ..Default::default() };
    let raw = b.s.solve_with(&opt).map_err(|_| SolveError::NoSolution)?;
    if !raw.conflicts.is_empty() {
        return Err(SolveError::OverConstrained);
    }
    if raw.status == ss::Status::NotConverged {
        return Err(SolveError::NoSolution);
    }
    Ok(raw)
}

fn finish(sketch: &Sketch, b: Built, raw: &ss::Solution) -> Solution {
    let mut out = sketch.clone();
    for p in &mut out.points {
        if let Some([x, y]) = b.points.get(&p.id).and_then(|id| raw.point(*id)) {
            (p.x, p.y) = (x, flip(y));
        }
    }
    for e in &mut out.entities {
        if let Entity::Circle { id, r, .. } = e
            && let Some(v) = b.entities.get(id).and_then(|e| raw.radius(*e))
        {
            *r = v.abs();
        }
    }
    let free: BTreeSet<(ss::EntityId, ss::Coordinate)> = raw.free.iter().filter(|f| f.mobility > PINNED).map(|f| (f.entity, f.coordinate)).collect();
    let point_ok = |id: &u32| b.points.get(id).is_some_and(|e| !free.contains(&(*e, ss::Coordinate::X)) && !free.contains(&(*e, ss::Coordinate::Y)));
    let point_constrained: BTreeSet<u32> = sketch.points.iter().map(|p| p.id).filter(point_ok).collect();
    let constrained = sketch
        .entities
        .iter()
        .filter(|e| match e {
            Entity::Line { .. } | Entity::Arc { .. } => e.points().iter().all(point_ok),
            Entity::Circle { id, c, .. } => point_ok(c) && b.entities.get(id).is_some_and(|e| !free.contains(&(*e, ss::Coordinate::Radius))),
        })
        .map(Entity::id)
        .collect();
    Solution { sketch: out, constrained, point_constrained, errors: b.errors, dof: raw.dof }
}

/// Solve `sketch` with its dimensions read from `params`. `NoSolution` when the solver does not
/// converge (the caller keeps the previous geometry), `OverConstrained` when constraints
/// contradict each other. Redundant constraints that agree are accepted (a file may hold them).
pub fn solve(sketch: &Sketch, params: &Solved) -> Result<Solution, SolveError> {
    let b = build(sketch, params).ok_or(SolveError::NoSolution)?;
    let raw = run(&b)?;
    Ok(finish(sketch, b, &raw))
}

/// Add `c` (at the end) and solve. Refused as `OverConstrained` when the solver reports the new
/// constraint's equations as redundant or conflicting, and as `NoSolution` when it does not
/// converge. The sketch itself is not changed; the result carries the new one.
pub fn try_add(sketch: &Sketch, c: Constraint, params: &Solved) -> Result<Solution, SolveError> {
    let mut next = sketch.clone();
    next.constraints.push(c);
    let index = next.constraints.len() - 1;
    let b = build(&next, params).ok_or(SolveError::NoSolution)?;
    let opt = ss::SolveOptions { max_unknowns: MAX_UNKNOWNS, ..Default::default() };
    let raw = b.s.solve_with(&opt).map_err(|_| SolveError::NoSolution)?;
    if let Some(id) = b.added.get(index).copied().flatten() {
        let mine = ss::Source::Constraint(id);
        if raw.conflicts.iter().chain(&raw.redundant).any(|d| d.source == mine) {
            return Err(SolveError::OverConstrained);
        }
    }
    if !raw.conflicts.is_empty() {
        return Err(SolveError::OverConstrained);
    }
    if raw.status == ss::Status::NotConverged {
        return Err(SolveError::NoSolution);
    }
    Ok(finish(&next, b, &raw))
}

#[cfg(test)]
mod tests {
    use super::super::model::tests::rectangle;
    use super::*;
    use crate::params::{self, ParamRow};

    fn rows(pairs: &[(&str, &str, f64)]) -> Solved {
        let rows: Vec<ParamRow> = pairs.iter().map(|(n, u, v)| ParamRow { sketch: Some(1), ..ParamRow::plain(n, u, *v) }).collect();
        params::solve(&rows)
    }

    fn at(s: &Sketch, id: u32) -> (f64, f64) {
        s.point(id).map(|p| (p.x, p.y)).unwrap_or((f64::NAN, f64::NAN))
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// The origin fixed, plus two free points drawn at `a` and `b` joined by a line (entity 0).
    fn free_line(a: (f64, f64), b: (f64, f64)) -> Sketch {
        let mut s = Sketch::new_v2();
        let (p, q) = (s.add_point(a.0, a.1), s.add_point(b.0, b.1));
        s.add_line(p, q, false);
        s
    }

    #[test]
    fn a_typed_triangle_closes() {
        let mut s = Sketch::new_v2();
        let (p1, p2) = (s.add_point(47.0, 3.0), s.add_point(16.0, 26.0));
        let (l0, l1, l2) = (s.add_line(0, p1, false), s.add_line(p1, p2, false), s.add_line(p2, 0, false));
        s.constraints.extend([
            Constraint::Horizontal { line: l0 },
            Constraint::Length { line: l0, param: "d1".into() },
            Constraint::Length { line: l1, param: "d2".into() },
            Constraint::Length { line: l2, param: "d3".into() },
        ]);
        let sol = solve(&s, &rows(&[("d1", "mm", 50.0), ("d2", "mm", 40.0), ("d3", "mm", 30.0)])).unwrap();
        let (o, a, b) = (at(&sol.sketch, 0), at(&sol.sketch, p1), at(&sol.sketch, p2));
        assert!(near(a.0 - o.0, 50.0) && near(a.1, o.1), "{a:?}");
        assert!(near((b.0 - o.0).hypot(b.1 - o.1), 30.0) && near((b.0 - a.0).hypot(b.1 - a.1), 40.0), "{b:?}");
        // It closes because the three lines share their points; the drawn side is kept.
        assert!(b.1 > 0.0 && sol.errors.is_empty());
        assert_eq!(sol.dof, 0);
        assert!(sol.constrained.len() == 3 && sol.point_constrained.len() == 3);
    }

    #[test]
    fn editing_d1_moves_the_connected_segments() {
        let s = rectangle();
        let sol = solve(&s, &rows(&[("d1", "mm", 50.0), ("d2", "mm", 30.0)])).unwrap();
        assert_eq!((at(&sol.sketch, 1), at(&sol.sketch, 2)), ((50.0, 0.0), (50.0, 30.0)));
        let sol = solve(&sol.sketch, &rows(&[("d1", "mm", 80.0), ("d2", "mm", 30.0)])).unwrap();
        let (p1, p2, p3) = (at(&sol.sketch, 1), at(&sol.sketch, 2), at(&sol.sketch, 3));
        assert!(near(p1.0, 80.0) && near(p1.1, 0.0), "{p1:?}");
        assert!(near(p2.0, 80.0) && near(p2.1, 30.0), "right side moved, still vertical: {p2:?}");
        assert!(near(p3.0, 0.0) && near(p3.1, 30.0), "top still horizontal: {p3:?}");
        assert_eq!(sol.dof, 0);
        assert_eq!(sol.constrained.len(), 4);
    }

    #[test]
    fn an_over_constraining_dimension_is_refused() {
        let s = rectangle();
        let p = rows(&[("d1", "mm", 80.0), ("d2", "mm", 30.0), ("d3", "mm", 70.0), ("d4", "mm", 80.0)]);
        let before = s.clone();
        // Contradicts d1 (the opposite side is 80, not 70).
        assert_eq!(try_add(&s, Constraint::Length { line: 2, param: "d3".into() }, &p).err(), Some(SolveError::OverConstrained));
        // Agrees with d1, but adds nothing.
        assert_eq!(try_add(&s, Constraint::Length { line: 2, param: "d4".into() }, &p).err(), Some(SolveError::OverConstrained));
        assert_eq!(s, before);
        // The same file solved as-is accepts the agreeing redundancy.
        let mut agreeing = s.clone();
        agreeing.constraints.push(Constraint::Length { line: 2, param: "d4".into() });
        assert!(solve(&agreeing, &p).is_ok());
    }

    #[test]
    fn a_fillet_is_tangent_to_both_lines() {
        let mut s = rectangle();
        let (arc, t1, t2) = s.fillet(2, 5.0).unwrap();
        s.constraints.push(Constraint::Radius { e: arc, param: "d3".into() });
        let (c, a, b) = match s.entity(arc) {
            Some(Entity::Arc { c, a, b, .. }) => (at(&s, *c), at(&s, *a), at(&s, *b)),
            other => panic!("{other:?}"),
        };
        let (p1, p2, p3) = (at(&s, t1), at(&s, t2), at(&s, 2));
        // Corner 2 is (50, 30): T1 up the right side at (50, 25), T2 along the top at (45, 30).
        assert!(near(p1.0, 50.0) && near(p1.1, 25.0) && near(p2.0, 45.0) && near(p2.1, 30.0), "{p1:?} {p2:?}");
        for t in [a, b] {
            assert!(near((c.0 - t.0).hypot(c.1 - t.1), 5.0));
        }
        // C - T1 is perpendicular to L1 (vertical), C - T2 to L2 (horizontal).
        assert!(near(c.1, p1.1) && near(c.0, p2.0), "{c:?}");
        // The sharp corner stays, on both lines, and the radius dimension is there.
        assert_eq!(p3, (50.0, 30.0));
        let on = |l| s.constraints.contains(&Constraint::PointOnLine { p: 2, line: l });
        assert!(on(1) && on(2));
        assert!(s.constraints.iter().any(|k| matches!(k, Constraint::Radius { e, .. } if *e == arc)));
        // L1's length dimension now measures to the sharp corner.
        assert!(s.constraints.contains(&Constraint::Distance { a: 2, b: 1, param: "d2".into() }));
        // It runs counter-clockwise on screen with a short sweep: the points are ordered T2 -> T1.
        assert_eq!((a, b), (p2, p1));
        // And the solver agrees with it.
        let sol = solve(&s, &rows(&[("d1", "mm", 50.0), ("d2", "mm", 30.0), ("d3", "mm", 5.0)])).unwrap();
        let (c2, a2) = (at(&sol.sketch, 6), at(&sol.sketch, t2));
        assert!(near((c2.0 - a2.0).hypot(c2.1 - a2.1), 5.0), "{c2:?} {a2:?}");
        assert!(near(at(&sol.sketch, t1).1, 25.0));
        // A fillet too big, a lone line and an unknown corner are refused.
        assert!(rectangle().fillet(2, 40.0).is_err());
        assert!(rectangle().fillet(77, 2.0).is_err());
        assert!(rectangle().fillet(2, 0.0).is_err());
    }

    #[test]
    fn under_constrained_geometry_stays_near_where_drawn() {
        let mut s = free_line((10.0, 10.0), (40.0, 12.0));
        s.constraints.push(Constraint::Horizontal { line: 0 });
        let sol = solve(&s, &Solved::default()).unwrap();
        let (p, q) = (at(&sol.sketch, 1), at(&sol.sketch, 2));
        assert!(near(p.1, q.1), "{p:?} {q:?}");
        let moved = |now: (f64, f64), was: (f64, f64)| (now.0 - was.0).hypot(now.1 - was.1);
        assert!(moved(p, (10.0, 10.0)) < 1.0 + 1e-6 && moved(q, (40.0, 12.0)) < 1.0 + 1e-6, "{p:?} {q:?}");
        assert!(sol.dof > 0);
    }

    #[test]
    fn fully_constrained_flags_follow_the_constraints() {
        let mut s = free_line((10.0, 10.0), (40.0, 12.0));
        let (r, t) = (s.add_point(5.0, 5.0), s.add_point(30.0, 8.0));
        let free = s.add_line(r, t, false);
        s.constraints.extend([
            Constraint::Fix { p: 1, x: 10.0, y: 10.0 },
            Constraint::Horizontal { line: 0 },
            Constraint::Length { line: 0, param: "d1".into() },
        ]);
        let sol = solve(&s, &rows(&[("d1", "mm", 30.0)])).unwrap();
        assert!(sol.constrained.contains(&0) && !sol.constrained.contains(&free), "{:?}", sol.constrained);
        assert!(sol.point_constrained.contains(&1) && sol.point_constrained.contains(&2));
        assert!(!sol.point_constrained.contains(&r) && !sol.point_constrained.contains(&t));
        assert!(sol.point_constrained.contains(&0), "the origin is fixed");
    }

    #[test]
    fn a_dimension_that_does_not_evaluate_is_reported_not_added() {
        let mut s = free_line((10.0, 10.0), (40.0, 12.0));
        s.constraints.push(Constraint::Length { line: 0, param: "nope".into() });
        s.constraints.push(Constraint::Length { line: 0, param: "d1".into() });
        let sol = solve(&s, &rows(&[("d1", "mm", -3.0)])).unwrap();
        assert_eq!(sol.errors.len(), 2, "{:?}", sol.errors);
        assert!(sol.errors[&1].contains("nope"), "{:?}", sol.errors);
        assert_eq!(sol.errors[&2], "must not be negative");
        // Nothing moved.
        assert_eq!(at(&sol.sketch, 2), (40.0, 12.0));
    }

    #[test]
    fn angles_measure_from_horizontal_counter_clockwise_on_screen() {
        let mut s = free_line((0.0, 0.0), (28.0, -3.0));
        s.constraints.extend([Constraint::Fix { p: 1, x: 0.0, y: 0.0 }, Constraint::Length { line: 0, param: "d1".into() }]);
        let sol = try_add(&s, Constraint::Angle { line: 0, param: "d2".into() }, &rows(&[("d1", "mm", 10.0), ("d2", "deg", 30.0)])).unwrap();
        let q = at(&sol.sketch, 2);
        // Counter-clockwise on screen lifts the far end: y goes negative.
        assert!(near(q.0, 10.0 * 30f64.to_radians().cos()) && near(q.1, -5.0), "{q:?}");
        assert_eq!(sol.sketch.measure(&Constraint::Angle { line: 0, param: String::new() }).map(|a| a.round()), Some(30.0));
    }

    #[test]
    fn parallel_perpendicular_tangent_equal_midpoint_concentric_solve() {
        let p = rows(&[("d1", "mm", 10.0)]);
        // Two lines: the first pinned at both ends, the second pinned at its start.
        let mut s = free_line((0.0, 0.0), (30.0, 0.0));
        let (r, t) = (s.add_point(0.0, 10.0), s.add_point(25.0, 12.0));
        s.add_line(r, t, false);
        s.constraints.extend([
            Constraint::Fix { p: 1, x: 0.0, y: 0.0 },
            Constraint::Fix { p: 2, x: 30.0, y: 0.0 },
            Constraint::Fix { p: r, x: 0.0, y: 10.0 },
        ]);
        let dir = |s: &Sketch| (at(s, t).0 - at(s, r).0, at(s, t).1 - at(s, r).1);

        let sol = try_add(&s, Constraint::Parallel { a: 0, b: 1 }, &p).unwrap();
        assert!(near(dir(&sol.sketch).1, 0.0), "{:?}", dir(&sol.sketch));
        let sol = try_add(&s, Constraint::Perpendicular { a: 0, b: 1 }, &p).unwrap();
        assert!(near(dir(&sol.sketch).0, 0.0), "{:?}", dir(&sol.sketch));
        let sol = try_add(&s, Constraint::Equal { a: 0, b: 1 }, &p).unwrap();
        let d = dir(&sol.sketch);
        assert!(near(d.0.hypot(d.1), 30.0), "{d:?}");
        // Midpoint: the second line's end sits at the middle of the first.
        let sol = try_add(&s, Constraint::Midpoint { p: t, line: 0 }, &p).unwrap();
        assert!(near(at(&sol.sketch, t).0, 15.0) && near(at(&sol.sketch, t).1, 0.0), "{:?}", at(&sol.sketch, t));

        // A circle of radius 10 about the origin, a line tangent to it, and a concentric circle.
        let mut s = Sketch::new_v2();
        let circle = s.add_circle(0, 9.0);
        let (a, b) = (s.add_point(-20.0, 10.5), s.add_point(20.0, 9.5));
        let tan = s.add_line(a, b, false);
        s.constraints.extend([Constraint::Fix { p: a, x: -20.0, y: 10.5 }, Constraint::Radius { e: circle, param: "d1".into() }]);
        let sol = try_add(&s, Constraint::Tangent { a: tan, b: circle }, &p).unwrap();
        let (qa, qb) = (at(&sol.sketch, a), at(&sol.sketch, b));
        let dist = (qb.0 - qa.0) * qa.1 - (qb.1 - qa.1) * qa.0;
        assert!(near(dist.abs() / (qb.0 - qa.0).hypot(qb.1 - qa.1), 10.0), "{qa:?} {qb:?}");
        assert!(near(sol.sketch.measure(&Constraint::Radius { e: circle, param: String::new() }).unwrap_or(0.0), 10.0));
        let centre = s.add_point(2.0, 1.0);
        let other = s.add_circle(centre, 4.0);
        let sol = try_add(&s, Constraint::Concentric { a: circle, b: other }, &p).unwrap();
        assert!(sol.sketch.points.iter().any(|q| q.id == centre) && near(at(&sol.sketch, centre).0, 0.0), "{:?}", at(&sol.sketch, centre));
    }
}
