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
    /// The distance between two points, mm: direct, or (E1) horizontal / vertical only (`along`;
    /// `b` is to the right of / below `a`, so the value is positive).
    Distance {
        a: u32,
        b: u32,
        #[serde(default, skip_serializing_if = "Along::is_direct")]
        along: Along,
        param: String,
    },
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
    /// The distance from point `p` to the infinite line `line`, mm (on the side it was drawn).
    DistancePointLine { p: u32, line: u32, param: String },
    /// Points `a` and `b` are mirror images about the line `line`.
    Symmetric { a: u32, b: u32, line: u32 },
}

/// How a point-to-point distance is measured (E1: Fusion's horizontal, vertical and aligned
/// dimensions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Along {
    /// The straight-line (aligned) distance.
    #[default]
    Direct,
    /// The horizontal offset only.
    X,
    /// The vertical offset only.
    Y,
}

impl Along {
    pub fn is_direct(&self) -> bool {
        *self == Along::Direct
    }
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
            | Constraint::Diameter { param, .. }
            | Constraint::DistancePointLine { param, .. } => Some(param),
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
            Constraint::DistancePointLine { p, line, .. } => r(&[*p], &[*line]),
            Constraint::Symmetric { a, b, line } => r(&[*a, *b], &[*line]),
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
            | Constraint::DistancePointLine { param, .. }
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
        Constraint::Distance { a, b, along, .. } => {
            let (p, q) = (pos(*a)?, pos(*b)?);
            match along {
                Along::Direct => dist(p, q),
                Along::X => (q.0 - p.0).abs(),
                Along::Y => (q.1 - p.1).abs(),
            }
        }
        Constraint::Length { line: l, .. } => {
            let (p, q) = line(*l)?;
            dist(p, q)
        }
        Constraint::Angle { line: l, .. } => wrap(angle(line(*l)?)),
        Constraint::AngleBetween { a, b, .. } => wrap(angle(line(*b)?) - angle(line(*a)?)),
        Constraint::Radius { e, .. } => radius(*e)?,
        Constraint::Diameter { e, .. } => 2.0 * radius(*e)?,
        Constraint::DistancePointLine { p, line: l, .. } => {
            let (a, b) = line(*l)?;
            let x = pos(*p)?;
            let len = dist(a, b);
            if len < 1e-12 {
                return None;
            }
            ((b.0 - a.0) * (x.1 - a.1) - (b.1 - a.1) * (x.0 - a.0)).abs() / len
        }
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
        let c = (i < self.constraints.len()).then(|| self.constraints.remove(i))?;
        self.forget_dimension_extras();
        Some(c)
    }

    /// Drop the placements and driven flags of dimensions the recipe no longer has.
    pub fn forget_dimension_extras(&mut self) {
        let names: BTreeSet<String> = self.constraints.iter().filter_map(Constraint::param).map(str::to_string).collect();
        self.places.retain(|k, _| names.contains(k));
        self.driven.retain(|k| names.contains(k));
    }

    /// Is the dimension `c` driven (a reference dimension: it reads the geometry and does not
    /// constrain it)?
    pub fn is_driven(&self, c: &Constraint) -> bool {
        c.param().is_some_and(|p| self.driven.contains(p))
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
        self.forget_dimension_extras();
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
        self.places.retain(|_, at| at.iter().all(|v| v.is_finite()));
        self.forget_dimension_extras();
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
                *con = Constraint::Distance { a: corner, b: far, along: Along::Direct, param: std::mem::take(param) };
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

// ---- the modify tools: trim, offset, mirror ----

/// A place where a line is cut by another shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    /// How far along the line, 0 at its first point to 1 at its second.
    pub t: f64,
    /// The place, millimetres.
    pub at: (f64, f64),
    /// The entity it was cut against.
    pub against: u32,
}

/// What Trim would do for a click: the piece that goes (from `from` to `to`) and the cuts that
/// bound it. With neither cut the whole line goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrimPlan {
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub lo: Option<Cut>,
    pub hi: Option<Cut>,
}

/// A chain or loop of lines in traversal order.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    /// `(line, from point, to point)`, each line oriented along the traversal.
    pub lines: Vec<(u32, u32, u32)>,
    /// The last line ends where the first begins.
    pub closed: bool,
}

/// Nearer than this (mm) counts as the same place.
const NEAR: f64 = 1e-6;

const TRIM_LINES_ONLY: &str = "trim works on lines for now; arcs and circles cannot be trimmed yet";

/// Where the line from `p0` along `r` crosses the circle around `c` of radius `rad`, as
/// parameters along the line.
fn circle_hits(p0: (f64, f64), r: (f64, f64), c: (f64, f64), rad: f64) -> Vec<f64> {
    let f = (p0.0 - c.0, p0.1 - c.1);
    let qa = r.0 * r.0 + r.1 * r.1;
    let qb = 2.0 * (r.0 * f.0 + r.1 * f.1);
    let qc = f.0 * f.0 + f.1 * f.1 - rad * rad;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 || qa < 1e-18 {
        return vec![];
    }
    let root = disc.sqrt();
    vec![(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)]
}

impl Sketch {
    fn xy(&self, id: u32) -> Option<(f64, f64)> {
        self.point(id).map(|p| (p.x, p.y))
    }

    /// The cuts of `line` by the other non-construction shapes, sorted along the line, strictly
    /// inside it.
    fn crossings(&self, line: u32) -> Result<Vec<Cut>, String> {
        let Some(Entity::Line { a, b, .. }) = self.entity(line) else { return Err(TRIM_LINES_ONLY.into()) };
        let (p0, p1) = self.xy(*a).zip(self.xy(*b)).ok_or("the line has no end points")?;
        let r = (p1.0 - p0.0, p1.1 - p0.1);
        let len = r.0.hypot(r.1);
        if len < NEAR {
            return Err("the line has no length".into());
        }
        let margin = NEAR / len;
        let mut cuts = Vec::new();
        for e in self.entities.iter().filter(|e| e.id() != line && !e.construction()) {
            let ts: Vec<f64> = match e {
                Entity::Line { a, b, .. } => {
                    let Some((o0, o1)) = self.xy(*a).zip(self.xy(*b)) else { continue };
                    let s = (o1.0 - o0.0, o1.1 - o0.1);
                    let slen = s.0.hypot(s.1);
                    let det = r.0 * s.1 - r.1 * s.0;
                    if slen < NEAR || det.abs() < 1e-9 * len * slen {
                        continue;
                    }
                    let d = (o0.0 - p0.0, o0.1 - p0.1);
                    let t = (d.0 * s.1 - d.1 * s.0) / det;
                    let u = (d.0 * r.1 - d.1 * r.0) / det;
                    let um = NEAR / slen;
                    if u >= -um && u <= 1.0 + um { vec![t] } else { vec![] }
                }
                Entity::Circle { c, r: rad, .. } => match self.xy(*c) {
                    Some(c) => circle_hits(p0, r, c, *rad),
                    None => continue,
                },
                Entity::Arc { c, a, b, .. } => {
                    let Some(((c, a), b)) = self.xy(*c).zip(self.xy(*a)).zip(self.xy(*b)) else { continue };
                    let ang = |q: (f64, f64)| (-(q.1 - c.1)).atan2(q.0 - c.0);
                    let tau = std::f64::consts::TAU;
                    let sweep = (ang(b) - ang(a)).rem_euclid(tau);
                    circle_hits(p0, r, c, (a.0 - c.0).hypot(a.1 - c.1))
                        .into_iter()
                        .filter(|t| {
                            let along = (ang((p0.0 + t * r.0, p0.1 + t * r.1)) - ang(a)).rem_euclid(tau);
                            along <= sweep + 1e-9 || tau - along < 1e-9
                        })
                        .collect()
                }
            };
            for t in ts {
                if t.is_finite() && t > margin && t < 1.0 - margin {
                    cuts.push(Cut { t, at: (p0.0 + t * r.0, p0.1 + t * r.1), against: e.id() });
                }
            }
        }
        cuts.sort_by(|x, y| x.t.total_cmp(&y.t));
        cuts.dedup_by(|x, y| (x.t - y.t).abs() < margin);
        Ok(cuts)
    }

    /// What Trim does to `line` for a click at `at`: the piece between the two cuts that bracket
    /// the click goes (to the line's end where there is no cut on a side).
    pub fn trim_plan(&self, line: u32, at: (f64, f64)) -> Result<TrimPlan, String> {
        let Some(Entity::Line { a, b, .. }) = self.entity(line) else { return Err(TRIM_LINES_ONLY.into()) };
        let (p0, p1) = self.xy(*a).zip(self.xy(*b)).ok_or("the line has no end points")?;
        let cuts = self.crossings(line)?;
        let r = (p1.0 - p0.0, p1.1 - p0.1);
        let t_click = (((at.0 - p0.0) * r.0 + (at.1 - p0.1) * r.1) / (r.0 * r.0 + r.1 * r.1)).clamp(0.0, 1.0);
        let lo = cuts.iter().rev().find(|c| c.t <= t_click).copied();
        let hi = cuts.iter().find(|c| c.t > t_click).copied();
        Ok(TrimPlan { from: lo.map_or(p0, |c| c.at), to: hi.map_or(p1, |c| c.at), lo, hi })
    }

    /// Trim `line` for a click at `at` (see [`Sketch::trim_plan`]). The pieces left share the
    /// line's end points; new points at the cuts lie on what cut them. Horizontal, vertical,
    /// parallel and perpendicular constraints stay on each piece; other constraints of the line are
    /// dropped. Returns the ids of the remaining pieces (none when the whole line went).
    pub fn trim_line(&mut self, line: u32, at: (f64, f64)) -> Result<Vec<u32>, String> {
        let plan = self.trim_plan(line, at)?;
        let Some(Entity::Line { a, b, construction, .. }) = self.entity(line).cloned() else { return Err("no such line".into()) };
        if plan.lo.is_none() && plan.hi.is_none() {
            self.remove_entities(&[line]);
            return Ok(vec![]);
        }
        let (p0, p1) = self.xy(a).zip(self.xy(b)).ok_or("the line has no end points")?;
        let along = |q: (f64, f64)| {
            let r = (p1.0 - p0.0, p1.1 - p0.1);
            ((q.0 - p0.0) * r.0 + (q.1 - p0.1) * r.1) / (r.0 * r.0 + r.1 * r.1)
        };
        let lo_point = plan.lo.map(|c| self.add_point(c.at.0, c.at.1));
        let hi_point = plan.hi.map(|c| self.add_point(c.at.0, c.at.1));
        // The pieces: (line id, its range along the original).
        let mut pieces: Vec<(u32, f64, f64)> = Vec::new();
        let mut reuse = Some(line);
        if let (Some(cut), Some(pl)) = (plan.lo, lo_point) {
            self.set_line_points(line, a, pl);
            pieces.push((line, 0.0, cut.t));
            reuse = None;
        }
        if let (Some(cut), Some(ph)) = (plan.hi, hi_point) {
            match reuse {
                Some(id) => {
                    self.set_line_points(id, ph, b);
                    pieces.push((id, cut.t, 1.0));
                }
                None => {
                    let id = self.add_line(ph, b, construction);
                    pieces.push((id, cut.t, 1.0));
                }
            }
        }
        // Constraints of the original line: carry some to the pieces.
        let old = std::mem::take(&mut self.constraints);
        let mut next = Vec::with_capacity(old.len() + 4);
        for c in old {
            if !c.refs().entities.contains(&line) {
                next.push(c);
                continue;
            }
            match &c {
                Constraint::Horizontal { .. } => next.extend(pieces.iter().map(|(id, ..)| Constraint::Horizontal { line: *id })),
                Constraint::Vertical { .. } => next.extend(pieces.iter().map(|(id, ..)| Constraint::Vertical { line: *id })),
                Constraint::Parallel { a, b } => {
                    let other = if *a == line { *b } else { *a };
                    next.extend(pieces.iter().map(|(id, ..)| Constraint::Parallel { a: *id, b: other }));
                }
                Constraint::Perpendicular { a, b } => {
                    let other = if *a == line { *b } else { *a };
                    next.extend(pieces.iter().map(|(id, ..)| Constraint::Perpendicular { a: *id, b: other }));
                }
                Constraint::PointOnLine { p, line: l } if *l == line => {
                    let t = self.xy(*p).map(along);
                    if let Some((id, ..)) = t.and_then(|t| pieces.iter().find(|(_, from, to)| t >= *from - 1e-9 && t <= *to + 1e-9)) {
                        next.push(Constraint::PointOnLine { p: *p, line: *id });
                    }
                }
                _ => {}
            }
        }
        self.constraints = next;
        // The cut points lie on what cut them.
        for (cut, point) in [(plan.lo, lo_point), (plan.hi, hi_point)] {
            let (Some(cut), Some(p)) = (cut, point) else { continue };
            let on = match self.entity(cut.against) {
                Some(Entity::Line { .. }) => Constraint::PointOnLine { p, line: cut.against },
                _ => Constraint::PointOnCircle { p, e: cut.against },
            };
            self.constraints.push(on);
        }
        // An end that went with the piece may be left with nothing.
        let used: BTreeSet<u32> = self.entities.iter().flat_map(Entity::points).collect();
        let orphans: BTreeSet<u32> =
            [plan.lo.is_none().then_some(a), plan.hi.is_none().then_some(b)].into_iter().flatten().filter(|p| *p != 0 && !used.contains(p)).collect();
        if !orphans.is_empty() {
            self.points.retain(|p| !orphans.contains(&p.id));
            self.constraints.retain(|c| !c.refs().points.iter().any(|p| orphans.contains(p)));
        }
        Ok(pieces.into_iter().map(|(id, ..)| id).collect())
    }

    fn set_line_points(&mut self, line: u32, from: u32, to: u32) {
        for e in &mut self.entities {
            if let Entity::Line { id, a, b, .. } = e
                && *id == line
            {
                (*a, *b) = (from, to);
            }
        }
    }

    /// The non-construction lines connected to `line` through points where exactly two lines
    /// meet: its whole chain or loop, `line` first.
    pub fn connected_chain(&self, line: u32) -> Vec<u32> {
        let at = |p: u32| -> Vec<u32> {
            self.entities
                .iter()
                .filter_map(|e| match e {
                    Entity::Line { id, a, b, construction: false } if *a == p || *b == p => Some(*id),
                    _ => None,
                })
                .collect()
        };
        let mut out = vec![line];
        let mut i = 0;
        while let Some(&cur) = out.get(i) {
            i += 1;
            if let Some(Entity::Line { a, b, .. }) = self.entity(cur) {
                for p in [*a, *b] {
                    if let [x, y] = at(p)[..]
                        && (x == cur || y == cur)
                    {
                        let other = if x == cur { y } else { x };
                        if !out.contains(&other) {
                            out.push(other);
                        }
                    }
                }
            }
        }
        out
    }

    /// Put the lines `ids` in order as one chain or loop, the first line running from its first
    /// point to its second.
    pub fn chain_of(&self, ids: &[u32]) -> Result<Chain, String> {
        const MSG: &str = "the lines must form one chain or loop";
        let mut seen = BTreeSet::new();
        let mut ends: Vec<(u32, u32, u32)> = Vec::new();
        for id in ids {
            let Some(Entity::Line { a, b, .. }) = self.entity(*id) else { return Err("only lines can be offset for now".into()) };
            if seen.insert(*id) {
                ends.push((*id, *a, *b));
            }
        }
        let Some(&(first, a0, b0)) = ends.first() else { return Err("no lines to offset".into()) };
        let mut lines = vec![(first, a0, b0)];
        let mut used = BTreeSet::from([first]);
        let next = |used: &BTreeSet<u32>, p: u32| -> Result<Option<(u32, u32)>, String> {
            let found: Vec<&(u32, u32, u32)> = ends.iter().filter(|(id, a, b)| !used.contains(id) && (*a == p || *b == p)).collect();
            match found[..] {
                [] => Ok(None),
                [&(id, a, b)] => Ok(Some((id, if a == p { b } else { a }))),
                _ => Err(MSG.into()),
            }
        };
        let mut cur = b0;
        while cur != a0 {
            let Some((id, far)) = next(&used, cur)? else { break };
            used.insert(id);
            lines.push((id, cur, far));
            cur = far;
        }
        let closed = cur == a0;
        if !closed {
            let mut cur = a0;
            while let Some((id, far)) = next(&used, cur)? {
                used.insert(id);
                lines.insert(0, (id, far, cur));
                cur = far;
            }
        }
        if used.len() != ends.len() {
            return Err(MSG.into());
        }
        Ok(Chain { lines, closed })
    }

    /// Where the points of the copy of `chain` offset by `distance` mm go: one per vertex (a closed
    /// loop has no repeated first vertex). `side` 1 is the left of the traversal as seen on screen,
    /// -1 the right.
    pub fn offset_positions(&self, chain: &Chain, distance: f64, side: f64) -> Result<Vec<(f64, f64)>, String> {
        type Seg = ((f64, f64), (f64, f64), (f64, f64));
        let k = if side < 0.0 { -distance } else { distance };
        // Each line: a point on its offset, the offset's far end and the direction.
        let mut segs: Vec<Seg> = Vec::new();
        for (_, from, to) in &chain.lines {
            let (p, q) = self.xy(*from).zip(self.xy(*to)).ok_or("a line has no end points")?;
            let d = (q.0 - p.0, q.1 - p.1);
            let len = d.0.hypot(d.1);
            if len < NEAR {
                return Err("a line has no length".into());
            }
            let u = (d.0 / len, d.1 / len);
            let n = (u.1 * k, -u.0 * k);
            segs.push(((p.0 + n.0, p.1 + n.1), (q.0 + n.0, q.1 + n.1), u));
        }
        let join = |s: &Seg, t: &Seg| {
            let det = s.2.0 * t.2.1 - s.2.1 * t.2.0;
            if det.abs() < 1e-9 {
                return t.0;
            }
            let k = ((t.0.0 - s.0.0) * t.2.1 - (t.0.1 - s.0.1) * t.2.0) / det;
            (s.0.0 + s.2.0 * k, s.0.1 + s.2.1 * k)
        };
        let n = segs.len();
        let mut out = Vec::with_capacity(n + 1);
        for i in 0..n {
            let (Some(cur), Some(prev)) = (segs.get(i), segs.get(if i == 0 { n - 1 } else { i - 1 })) else { continue };
            out.push(if i > 0 || chain.closed { join(prev, cur) } else { cur.0 });
        }
        if !chain.closed
            && let Some(last) = segs.last()
        {
            out.push(last.1);
        }
        Ok(out)
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

    #[test]
    fn modify_variants_serde_measure_and_one_sided_trim() {
        let mut s = rectangle();
        s.constraints.push(Constraint::DistancePointLine { p: 2, line: 0, param: "d3".into() });
        s.constraints.push(Constraint::Symmetric { a: 1, b: 3, line: 0 });
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["constraints"][7]["type"], "distancePointLine");
        assert_eq!(json["constraints"][8]["type"], "symmetric");
        assert_eq!(serde_json::from_value::<Sketch>(json).unwrap(), s);
        assert_eq!(s.measure(&Constraint::DistancePointLine { p: 2, line: 0, param: String::new() }), Some(30.0));
        s.rename_references("d3", "gap");
        assert_eq!(s.constraints[7].param(), Some("gap"));
        assert_eq!(s.constraints[8].refs(), Refs { points: vec![1, 3], entities: vec![0] });
        // A line crossing only the right side of the rectangle: clicking outside it trims to the end.
        let mut s = rectangle();
        let (p, q) = (s.add_point(20.0, 15.0), s.add_point(80.0, 15.0));
        let l = s.add_line(p, q, false);
        let plan = s.trim_plan(l, (70.0, 15.0)).unwrap();
        assert_eq!((plan.from, plan.to), ((50.0, 15.0), (80.0, 15.0)));
        let kept = s.trim_line(l, (70.0, 15.0)).unwrap();
        assert_eq!(kept, vec![l]);
        assert!(s.points.iter().all(|pt| pt.id != q), "the far end is gone with its piece");
        // Arcs and circles are refused.
        let c = s.add_circle(p, 3.0);
        assert!(s.trim_line(c, (0.0, 0.0)).is_err());
    }
}
