//! The sketch v2 model: points, lines, circles and arcs, the constraints between them and the
//! dimensions that bind them to named parameters.
//!
//! Coordinates are millimetres in the page's frame (x right, y down), relative to the sketch's
//! origin. Angles are degrees, counter-clockwise as seen on screen. An arc runs counter-clockwise
//! on screen from its start point to its end point around its centre.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::Sketch;

/// Points a sketch holds at most.
pub const MAX_POINTS: usize = 4096;
/// Entities a sketch holds at most.
pub const MAX_ENTITIES: usize = 4096;
/// Constraints a sketch holds at most.
pub const MAX_CONSTRAINTS: usize = 8192;

/// A point of the sketch: its id and its current solved position (millimetres).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SPoint {
    pub id: u32,
    pub x: f64,
    pub y: f64,
}

fn not(b: &bool) -> bool {
    !*b
}

/// A line, circle or arc over the sketch's points. Entity ids and point ids are separate spaces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Entity {
    /// A segment from point `a` to point `b`.
    Line {
        id: u32,
        a: u32,
        b: u32,
        #[serde(default, skip_serializing_if = "not")]
        construction: bool,
    },
    /// A circle around point `c`; `r` is the current radius (a solver variable).
    Circle {
        id: u32,
        c: u32,
        r: f64,
        #[serde(default, skip_serializing_if = "not")]
        construction: bool,
    },
    /// An arc around `c`, counter-clockwise on screen from `a` to `b`; its radius is `|a - c|`.
    Arc {
        id: u32,
        c: u32,
        a: u32,
        b: u32,
        #[serde(default, skip_serializing_if = "not")]
        construction: bool,
    },
}

impl Entity {
    /// The entity's id.
    pub fn id(&self) -> u32 {
        match self {
            Entity::Line { id, .. } | Entity::Circle { id, .. } | Entity::Arc { id, .. } => *id,
        }
    }

    /// Is this construction geometry (drawn, but not output)?
    pub fn construction(&self) -> bool {
        match self {
            Entity::Line { construction, .. } | Entity::Circle { construction, .. } | Entity::Arc { construction, .. } => *construction,
        }
    }

    /// The point ids this entity is made of.
    pub fn points(&self) -> Vec<u32> {
        match self {
            Entity::Line { a, b, .. } => vec![*a, *b],
            Entity::Circle { c, .. } => vec![*c],
            Entity::Arc { c, a, b, .. } => vec![*c, *a, *b],
        }
    }
}

/// A constraint or dimension. Dimensions carry the name of the model parameter that holds their
/// value (`d1`, `d2`… in `euclidean.params`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Constraint {
    /// Two points are one point.
    Coincident { a: u32, b: u32 },
    /// A point lies on a line (the infinite line through it).
    PointOnLine { p: u32, line: u32 },
    /// A point lies on a circle or arc.
    PointOnCircle { p: u32, e: u32 },
    /// A line is horizontal.
    Horizontal { line: u32 },
    /// A line is vertical.
    Vertical { line: u32 },
    /// Two lines are parallel.
    Parallel { a: u32, b: u32 },
    /// Two lines are perpendicular.
    Perpendicular { a: u32, b: u32 },
    /// Line–circle/arc or circle/arc–circle/arc tangency (the solver keeps the drawn side).
    Tangent { a: u32, b: u32 },
    /// Two lines have equal length, or two circles/arcs equal radius.
    Equal { a: u32, b: u32 },
    /// A point is pinned at `x`, `y` (millimetres).
    Fix { p: u32, x: f64, y: f64 },
    /// A point is the midpoint of a line.
    Midpoint { p: u32, line: u32 },
    /// Two circles/arcs share a centre.
    Concentric { a: u32, b: u32 },
    /// The distance between two points, mm.
    Distance { a: u32, b: u32, param: String },
    /// The length of a line, mm.
    Length { line: u32, param: String },
    /// A line's angle from horizontal, degrees counter-clockwise on screen.
    Angle { line: u32, param: String },
    /// The angle from line `a` to line `b`, degrees counter-clockwise.
    AngleBetween { a: u32, b: u32, param: String },
    /// The radius of a circle/arc, mm.
    Radius { e: u32, param: String },
    /// The diameter of a circle/arc, mm.
    Diameter { e: u32, param: String },
}

/// The points and entities a constraint refers to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refs {
    pub points: Vec<u32>,
    pub entities: Vec<u32>,
}

impl Constraint {
    /// The parameter a dimension is bound to, or `None` for a geometric constraint.
    pub fn param(&self) -> Option<&str> {
        match self {
            Constraint::Distance { param, .. }
            | Constraint::Length { param, .. }
            | Constraint::Angle { param, .. }
            | Constraint::AngleBetween { param, .. }
            | Constraint::Radius { param, .. }
            | Constraint::Diameter { param, .. } => Some(param),
            _ => None,
        }
    }

    /// Is this a dimension (bound to a parameter)?
    pub fn is_dimension(&self) -> bool {
        self.param().is_some()
    }

    /// Does this dimension measure degrees (as opposed to millimetres)?
    pub fn is_angular(&self) -> bool {
        matches!(self, Constraint::Angle { .. } | Constraint::AngleBetween { .. })
    }

    /// The points and entities referenced (what deleting them must also delete).
    pub fn refs(&self) -> Refs {
        let r = |points: &[u32], entities: &[u32]| Refs { points: points.to_vec(), entities: entities.to_vec() };
        match self {
            Constraint::Coincident { a, b } | Constraint::Distance { a, b, .. } => r(&[*a, *b], &[]),
            Constraint::PointOnLine { p, line } | Constraint::Midpoint { p, line } => r(&[*p], &[*line]),
            Constraint::PointOnCircle { p, e } => r(&[*p], &[*e]),
            Constraint::Horizontal { line } | Constraint::Vertical { line } | Constraint::Length { line, .. } | Constraint::Angle { line, .. } => {
                r(&[], &[*line])
            }
            Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Tangent { a, b }
            | Constraint::Equal { a, b }
            | Constraint::Concentric { a, b }
            | Constraint::AngleBetween { a, b, .. } => r(&[], &[*a, *b]),
            Constraint::Fix { p, .. } => r(&[*p], &[]),
            Constraint::Radius { e, .. } | Constraint::Diameter { e, .. } => r(&[], &[*e]),
        }
    }

    /// Rename the bound parameter `from` to `to` (a whole-name match).
    pub fn rename_param(&mut self, from: &str, to: &str) {
        match self {
            Constraint::Distance { param, .. }
            | Constraint::Length { param, .. }
            | Constraint::Angle { param, .. }
            | Constraint::AngleBetween { param, .. }
            | Constraint::Radius { param, .. }
            | Constraint::Diameter { param, .. }
                if param == from =>
            {
                *param = to.to_string();
            }
            _ => {}
        }
    }
}

/// The current value of dimension `c`'s quantity in `sketch` (millimetres or degrees, in the
/// sketch's conventions), or `None` for a geometric constraint or one with missing geometry.
pub fn measure(sketch: &Sketch, c: &Constraint) -> Option<f64> {
    let pos = |id: u32| sketch.point(id).map(|p| (p.x, p.y));
    let line = |id: u32| match sketch.entity(id)? {
        Entity::Line { a, b, .. } => Some((pos(*a)?, pos(*b)?)),
        _ => None,
    };
    let dist = |p: (f64, f64), q: (f64, f64)| (q.0 - p.0).hypot(q.1 - p.1);
    let angle = |(p, q): ((f64, f64), (f64, f64))| (-(q.1 - p.1)).atan2(q.0 - p.0).to_degrees();
    let wrap = |d: f64| {
        let w = d.rem_euclid(360.0);
        if w > 180.0 { w - 360.0 } else { w }
    };
    let radius = |id: u32| match sketch.entity(id)? {
        Entity::Circle { r, .. } => Some(*r),
        Entity::Arc { c, a, .. } => Some(dist(pos(*c)?, pos(*a)?)),
        _ => None,
    };
    let v = match c {
        Constraint::Distance { a, b, .. } => dist(pos(*a)?, pos(*b)?),
        Constraint::Length { line: l, .. } => {
            let (p, q) = line(*l)?;
            dist(p, q)
        }
        Constraint::Angle { line: l, .. } => wrap(angle(line(*l)?)),
        Constraint::AngleBetween { a, b, .. } => wrap(angle(line(*b)?) - angle(line(*a)?)),
        Constraint::Radius { e, .. } => radius(*e)?,
        Constraint::Diameter { e, .. } => 2.0 * radius(*e)?,
        _ => return None,
    };
    v.is_finite().then_some(v)
}

fn next_id(ids: impl Iterator<Item = u32>) -> u32 {
    ids.max().map_or(0, |m| m.saturating_add(1))
}

impl Sketch {
    /// Is this a v2 sketch (has points)?
    pub fn is_v2(&self) -> bool {
        !self.points.is_empty()
    }

    /// A new v2 sketch: the origin, point 0 at (0, 0), fixed there.
    pub fn new_v2() -> Sketch {
        Sketch { points: vec![SPoint { id: 0, x: 0.0, y: 0.0 }], constraints: vec![Constraint::Fix { p: 0, x: 0.0, y: 0.0 }], ..Default::default() }
    }

    /// The next free point id.
    pub fn next_point_id(&self) -> u32 {
        next_id(self.points.iter().map(|p| p.id))
    }

    /// The next free entity id.
    pub fn next_entity_id(&self) -> u32 {
        next_id(self.entities.iter().map(Entity::id))
    }

    /// The point `id`.
    pub fn point(&self, id: u32) -> Option<&SPoint> {
        self.points.iter().find(|p| p.id == id)
    }

    /// The entity `id`.
    pub fn entity(&self, id: u32) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id() == id)
    }

    /// Add a point at `x`, `y` (millimetres); returns its id.
    pub fn add_point(&mut self, x: f64, y: f64) -> u32 {
        let id = self.next_point_id();
        self.points.push(SPoint { id, x, y });
        id
    }

    /// Add a line between points `a` and `b`; returns its entity id.
    pub fn add_line(&mut self, a: u32, b: u32, construction: bool) -> u32 {
        let id = self.next_entity_id();
        self.entities.push(Entity::Line { id, a, b, construction });
        id
    }

    /// Add a circle around point `c` of radius `r` mm; returns its entity id.
    pub fn add_circle(&mut self, c: u32, r: f64) -> u32 {
        let id = self.next_entity_id();
        self.entities.push(Entity::Circle { id, c, r, construction: false });
        id
    }

    /// Add an arc around `c`, counter-clockwise on screen from `a` to `b`; returns its entity id.
    pub fn add_arc(&mut self, c: u32, a: u32, b: u32) -> u32 {
        let id = self.next_entity_id();
        self.entities.push(Entity::Arc { id, c, a, b, construction: false });
        id
    }

    /// Remove constraint number `i`, returning it.
    pub fn remove_constraint(&mut self, i: usize) -> Option<Constraint> {
        (i < self.constraints.len()).then(|| self.constraints.remove(i))
    }

    /// Remove entities `ids`, the constraints that refer to them and the points only they used
    /// (except point 0).
    pub fn remove_entities(&mut self, ids: &[u32]) {
        let gone: BTreeSet<u32> = ids.iter().copied().collect();
        let mut candidates: BTreeSet<u32> = BTreeSet::new();
        for e in self.entities.iter().filter(|e| gone.contains(&e.id())) {
            candidates.extend(e.points());
        }
        self.entities.retain(|e| !gone.contains(&e.id()));
        let used: BTreeSet<u32> = self.entities.iter().flat_map(Entity::points).collect();
        let orphans: BTreeSet<u32> = candidates.into_iter().filter(|p| *p != 0 && !used.contains(p)).collect();
        self.points.retain(|p| !orphans.contains(&p.id));
        self.constraints.retain(|c| {
            let r = c.refs();
            !r.entities.iter().any(|e| gone.contains(e)) && !r.points.iter().any(|p| orphans.contains(p))
        });
    }

    /// The current value of dimension `c`'s quantity (see [`measure`]).
    pub fn measure(&self, c: &Constraint) -> Option<f64> {
        measure(self, c)
    }

    /// Keep the v2 data within its caps and consistent: cut to the caps, zero non-finite
    /// coordinates, and drop entities and constraints that refer to what is not there.
    pub fn sanitize_model(&mut self) {
        self.points.truncate(MAX_POINTS);
        self.entities.truncate(MAX_ENTITIES);
        self.constraints.truncate(MAX_CONSTRAINTS);
        let mut seen = BTreeSet::new();
        self.points.retain(|p| seen.insert(p.id));
        for p in &mut self.points {
            for v in [&mut p.x, &mut p.y] {
                if !v.is_finite() {
                    *v = 0.0;
                }
            }
        }
        for e in &mut self.entities {
            if let Entity::Circle { r, .. } = e {
                *r = if r.is_finite() { r.abs() } else { 0.0 };
            }
        }
        let points: BTreeSet<u32> = self.points.iter().map(|p| p.id).collect();
        let mut seen = BTreeSet::new();
        self.entities.retain(|e| {
            let sound = match e {
                Entity::Line { a, b, .. } => a != b,
                Entity::Circle { .. } => true,
                Entity::Arc { c, a, b, .. } => a != c && b != c,
            };
            sound && e.points().iter().all(|p| points.contains(p)) && seen.insert(e.id())
        });
        let entities: BTreeSet<u32> = self.entities.iter().map(Entity::id).collect();
        for c in &mut self.constraints {
            if let Constraint::Fix { x, y, .. } = c {
                for v in [x, y] {
                    if !v.is_finite() {
                        *v = 0.0;
                    }
                }
            }
        }
        self.constraints.retain(|c| {
            let r = c.refs();
            r.points.iter().all(|p| points.contains(p)) && r.entities.iter().all(|e| entities.contains(e))
        });
    }

    /// Round the corner point `corner`, where exactly two non-parallel, non-construction lines
    /// meet, with an arc of `radius_mm`. Returns `(arc id, T1, T2)`: the arc's entity id and the
    /// ids of the new tangent points on the first and second line.
    ///
    /// Each line is cut back to its tangent point; the corner point stays as a "virtual sharp"
    /// (on both lines), so a length dimension of a cut line becomes a distance from the corner and
    /// nothing jumps. The caller adds the `Radius` dimension.
    pub fn fillet(&mut self, corner: u32, radius_mm: f64) -> Result<(u32, u32, u32), String> {
        if !(radius_mm.is_finite() && radius_mm > 0.0) {
            return Err("the radius must be a positive number".into());
        }
        let p = self.point(corner).map(|p| (p.x, p.y)).ok_or("no such corner")?;
        // (entity id, the far point id) of each line meeting at the corner.
        let lines: Vec<(u32, u32)> = self
            .entities
            .iter()
            .filter_map(|e| match e {
                Entity::Line { id, a, b, construction: false } if *a == corner => Some((*id, *b)),
                Entity::Line { id, a, b, construction: false } if *b == corner => Some((*id, *a)),
                _ => None,
            })
            .collect();
        let [(l1, far1), (l2, far2)] = lines[..] else {
            return Err(format!("a fillet needs a corner where exactly two lines meet (found {})", lines.len()));
        };
        let a = self.point(far1).map(|q| (q.x, q.y)).ok_or("a line has no end point")?;
        let b = self.point(far2).map(|q| (q.x, q.y)).ok_or("a line has no end point")?;
        let (d1, d2) = ((a.0 - p.0, a.1 - p.1), (b.0 - p.0, b.1 - p.1));
        let (len1, len2) = (d1.0.hypot(d1.1), d2.0.hypot(d2.1));
        if len1 < 1e-9 || len2 < 1e-9 {
            return Err("a line has no length".into());
        }
        let (u1, u2) = ((d1.0 / len1, d1.1 / len1), (d2.0 / len2, d2.1 / len2));
        let cross = u1.0 * u2.1 - u1.1 * u2.0;
        let dot = u1.0 * u2.0 + u1.1 * u2.1;
        if cross.abs() < 1e-6 {
            return Err("the lines are parallel".into());
        }
        let theta = cross.abs().atan2(dot);
        let t = radius_mm / (theta / 2.0).tan();
        if !t.is_finite() || t >= len1 - 1e-9 || t >= len2 - 1e-9 {
            return Err("the radius is too large for these lines".into());
        }
        let t1 = (p.0 + u1.0 * t, p.1 + u1.1 * t);
        let t2 = (p.0 + u2.0 * t, p.1 + u2.1 * t);
        let (wx, wy) = (u1.0 + u2.0, u1.1 + u2.1);
        let wl = wx.hypot(wy);
        let h = radius_mm / (theta / 2.0).sin();
        let centre = (p.0 + wx / wl * h, p.1 + wy / wl * h);
        let ang = |v: (f64, f64)| (-v.1).atan2(v.0);
        let sweep = (ang((t2.0 - centre.0, t2.1 - centre.1)) - ang((t1.0 - centre.0, t1.1 - centre.1))).rem_euclid(std::f64::consts::TAU);

        let id1 = self.add_point(t1.0, t1.1);
        let id2 = self.add_point(t2.0, t2.1);
        let c = self.add_point(centre.0, centre.1);
        for (line, tp) in [(l1, id1), (l2, id2)] {
            for e in &mut self.entities {
                if let Entity::Line { id, a, b, .. } = e
                    && *id == line
                {
                    if *a == corner {
                        *a = tp;
                    } else if *b == corner {
                        *b = tp;
                    }
                }
            }
        }
        let arc = if sweep < std::f64::consts::PI { self.add_arc(c, id1, id2) } else { self.add_arc(c, id2, id1) };
        // Length dimensions of the cut lines keep measuring to the sharp corner.
        for con in &mut self.constraints {
            if let Constraint::Length { line, param } = con {
                let far = if *line == l1 {
                    far1
                } else if *line == l2 {
                    far2
                } else {
                    continue;
                };
                *con = Constraint::Distance { a: corner, b: far, param: std::mem::take(param) };
            }
        }
        self.constraints.extend([
            Constraint::PointOnLine { p: corner, line: l1 },
            Constraint::PointOnLine { p: corner, line: l2 },
            Constraint::Tangent { a: l1, b: arc },
            Constraint::Tangent { a: l2, b: arc },
        ]);
        Ok((arc, id1, id2))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 50 × 30 rectangle: corners 0 (the origin), 1, 2, 3 and lines 0..4 around them.
    pub(crate) fn rectangle() -> Sketch {
        let mut s = Sketch::new_v2();
        let p1 = s.add_point(50.0, 0.0);
        let p2 = s.add_point(50.0, 30.0);
        let p3 = s.add_point(0.0, 30.0);
        let l0 = s.add_line(0, p1, false);
        let l1 = s.add_line(p1, p2, false);
        let l2 = s.add_line(p2, p3, false);
        let l3 = s.add_line(p3, 0, false);
        s.constraints.extend([
            Constraint::Horizontal { line: l0 },
            Constraint::Vertical { line: l1 },
            Constraint::Horizontal { line: l2 },
            Constraint::Vertical { line: l3 },
            Constraint::Length { line: l0, param: "d1".into() },
            Constraint::Length { line: l1, param: "d2".into() },
        ]);
        s
    }

    #[test]
    fn v2_recipes_round_trip_through_serde() {
        let mut s = rectangle();
        s.add_circle(2, 4.0);
        s.add_arc(0, 1, 2);
        s.constraints.push(Constraint::Angle { line: 0, param: "d3".into() });
        s.constraints.push(Constraint::PointOnCircle { p: 1, e: 4 });
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["entities"][0]["kind"], "line");
        assert!(json["entities"][0].get("construction").is_none(), "{json}");
        assert_eq!(json["constraints"][0]["type"], "fix");
        assert_eq!(json["constraints"][1]["type"], "horizontal");
        assert_eq!(json["constraints"][5]["type"], "length");
        assert_eq!(json["constraints"][7]["type"], "angle");
        assert_eq!(json["constraints"][8]["type"], "pointOnCircle");
        assert_eq!(serde_json::from_value::<Sketch>(json).unwrap(), s);
        assert!(s.is_v2());
        assert_eq!((s.next_point_id(), s.next_entity_id()), (4, 6));
    }

    #[test]
    fn b2_recipes_still_load_and_build() {
        let json = serde_json::json!({
            "origin": [5.0, 0.0],
            "shapes": [
                {"id": "R1", "kind": "rect", "fields": {"x": "0", "y": "0", "w": "40 mm", "h": "30 mm", "r": "0"}},
                {"id": "L1", "kind": "line", "fields": {"x": "0", "y": "0", "len": "50 mm", "ang": "0"}}
            ]
        });
        let s: Sketch = serde_json::from_value(json).unwrap();
        assert!(!s.is_v2() && s.entities.is_empty() && s.constraints.is_empty());
        let built = super::super::build(&s, &crate::expr::Solved::default());
        assert_eq!(built.len(), 2);
        assert!(built.iter().all(|b| b.paths.is_ok()));
        let back = serde_json::to_value(&s).unwrap();
        assert!(back.get("points").is_none() && back.get("entities").is_none() && back.get("constraints").is_none(), "{back}");
    }

    #[test]
    fn renaming_a_parameter_renames_dimensions() {
        let mut s = rectangle();
        s.rename_references("d1", "width");
        assert_eq!(s.constraints[5].param(), Some("width"));
        assert_eq!(s.constraints[6].param(), Some("d2"));
        // A whole-name match only.
        s.rename_references("d", "x");
        assert_eq!(s.constraints[6].param(), Some("d2"));
        assert!(s.constraints[5].is_dimension() && !s.constraints[0].is_dimension());
    }

    #[test]
    fn sanitize_drops_what_refers_to_nothing() {
        let mut s = rectangle();
        s.points[1].x = f64::NAN;
        s.entities.push(Entity::Line { id: 9, a: 0, b: 77, construction: false });
        s.constraints.push(Constraint::Vertical { line: 9 });
        s.constraints.push(Constraint::Coincident { a: 0, b: 55 });
        s.sanitize();
        assert_eq!((s.points[1].x, s.entities.len(), s.constraints.len()), (0.0, 4, 7));
    }

    #[test]
    fn measure_reads_the_quantity_in_our_conventions() {
        let mut s = Sketch::new_v2();
        let p = s.add_point(10.0, -10.0);
        let l = s.add_line(0, p, false);
        let c = s.add_circle(p, 3.0);
        let m = |c: Constraint| s.measure(&c).unwrap();
        assert!((m(Constraint::Length { line: l, param: String::new() }) - 200f64.sqrt()).abs() < 1e-12);
        // y is down, so a point above the origin is a positive angle.
        assert!((m(Constraint::Angle { line: l, param: String::new() }) - 45.0).abs() < 1e-9);
        assert_eq!(m(Constraint::Diameter { e: c, param: String::new() }), 6.0);
        assert!(s.measure(&Constraint::Horizontal { line: l }).is_none());
    }
}
