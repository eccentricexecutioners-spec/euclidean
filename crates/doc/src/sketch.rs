//! Parametric sketches (Euclidean): a recipe of shapes whose every size and position is an
//! expression over the document's user parameters ([`crate::params`]). The recipe rides on an
//! ordinary group in [`crate::NodeExtras::sketch`]; the engine rebuilds the group's children
//! (plain paths) from it whenever a parameter or the recipe changes, so exporters, hit testing,
//! the Layers panel and selection see nothing new.
//!
//! Lengths are millimetres in the recipe and points in the built paths; y is the document's,
//! downwards from the artboard's top-left corner. A line's angle is counter-clockwise on the
//! page, as the Measure tool reads it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use vectorcraft_geom::{PathData, Point, Rect, shapes};

use crate::Unit;
use crate::expr::Solved;
use crate::params::ParamRow;

/// Shape kinds and their fields, in the order the panel shows them.
pub const FIELDS: [(&str, &[&str]); 3] =
    [("rect", &["x", "y", "w", "h", "r"]), ("circle", &["cx", "cy", "d", "n", "pitch"]), ("line", &["x", "y", "len", "ang"])];
/// Fields that are plain numbers, not lengths.
pub const UNITLESS: [&str; 2] = ["n", "ang"];
/// Copies a circle row draws at most.
pub const MAX_COPIES: usize = 200;
/// Shapes a sketch holds at most.
pub const MAX_SHAPES: usize = 256;
/// Coordinates past this (points) are refused: the canvas has limits.
const MAX_COORD: f64 = 1.0e7;

/// The fields of shape kind `kind`, or none for an unknown kind.
pub fn fields_of(kind: &str) -> Option<&'static [&'static str]> {
    FIELDS.iter().find(|(k, _)| *k == kind).map(|(_, f)| *f)
}

/// The English label of a field (what the panel shows beside it).
pub fn field_label(kind: &str, field: &str) -> &'static str {
    match (kind, field) {
        ("rect", "x") | ("line", "x") => "x",
        ("rect", "y") | ("line", "y") => "y",
        ("rect", "w") => "width",
        ("rect", "h") => "height",
        ("rect", "r") => "corner radius",
        ("circle", "cx") => "centre x",
        ("circle", "cy") => "centre y",
        ("circle", "d") => "diameter",
        ("circle", "n") => "copies",
        ("circle", "pitch") => "pitch",
        ("line", "len") => "length",
        ("line", "ang") => "angle",
        _ => "",
    }
}

/// Is `field` a plain number rather than a length?
pub fn unitless(field: &str) -> bool {
    UNITLESS.contains(&field)
}

/// One shape of a sketch: its id (`R1`, `C2`, the Layers panel name of its paths), kind, and an
/// expression per field (missing fields read as empty, which is an error).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SketchShape {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

impl SketchShape {
    /// The shape a user draws with the Parametric Sketch menu: a 40 × 30 mm rectangle at the
    /// origin, a 20 mm circle, or a 50 mm line.
    pub fn default_of(kind: &str, id: String) -> Option<SketchShape> {
        let f = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let fields = match kind {
            "rect" => f(&[("x", "0"), ("y", "0"), ("w", "40 mm"), ("h", "30 mm"), ("r", "0")]),
            "circle" => f(&[("cx", "20 mm"), ("cy", "20 mm"), ("d", "20 mm"), ("n", "1"), ("pitch", "0")]),
            "line" => f(&[("x", "0"), ("y", "0"), ("len", "50 mm"), ("ang", "0")]),
            _ => return None,
        };
        Some(SketchShape { id, kind: kind.into(), fields })
    }
}

/// A sketch recipe: the shapes, and the offset (millimetres) the whole sketch was moved by with
/// the Selection tool.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub origin: [f64; 2],
    #[serde(default)]
    pub shapes: Vec<SketchShape>,
}

fn is_zero(o: &[f64; 2]) -> bool {
    *o == [0.0, 0.0]
}

impl Sketch {
    /// The next free id for a shape of `kind`: `R1`, `C1`, `L1`…
    pub fn next_id(&self, kind: &str) -> String {
        let prefix = match kind {
            "rect" => "R",
            "circle" => "C",
            _ => "L",
        };
        (1..).map(|i| format!("{prefix}{i}")).find(|id| !self.shapes.iter().any(|s| &s.id == id)).unwrap_or_else(|| format!("{prefix}0"))
    }

    /// Rename parameter `from` to `to` in every field.
    pub fn rename_references(&mut self, from: &str, to: &str) {
        for s in &mut self.shapes {
            for v in s.fields.values_mut() {
                *v = crate::expr::rename_references(v, from, to);
            }
        }
    }

    /// Keep the recipe within its caps (a file may hold anything).
    pub fn sanitize(&mut self) {
        self.shapes.truncate(MAX_SHAPES);
        if !self.origin.iter().all(|v| v.is_finite()) {
            self.origin = [0.0, 0.0];
        }
    }

    /// The sample sketch (Object › Parametric Sketch › Sample: Mounting plate): a rounded plate
    /// with two rows of holes and a centre slot, driven by [`Sketch::sample_params`].
    pub fn sample() -> Sketch {
        let shape = |id: &str, kind: &str, pairs: &[(&str, &str)]| SketchShape {
            id: id.into(),
            kind: kind.into(),
            fields: pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        Sketch {
            origin: [0.0, 0.0],
            shapes: vec![
                shape("R1", "rect", &[("x", "0"), ("y", "0"), ("w", "width"), ("h", "height"), ("r", "corner")]),
                shape(
                    "C1",
                    "circle",
                    &[("cx", "margin"), ("cy", "margin"), ("d", "hole_dia"), ("n", "holes"), ("pitch", "(width - 2*margin) / (holes - 1)")],
                ),
                shape(
                    "C2",
                    "circle",
                    &[("cx", "margin"), ("cy", "height - margin"), ("d", "hole_dia"), ("n", "holes"), ("pitch", "(width - 2*margin) / (holes - 1)")],
                ),
                shape("R2", "rect", &[("x", "(width - slot_w) / 2"), ("y", "height/2 - 5 mm"), ("w", "slot_w"), ("h", "10 mm"), ("r", "5 mm")]),
            ],
        }
    }

    /// The sample's user parameters.
    pub fn sample_params() -> Vec<ParamRow> {
        let row = |name: &str, unit: &str, expression: &str, comment: &str, favourite: bool, range: Option<(f64, f64, f64)>| ParamRow {
            name: name.into(),
            unit: unit.into(),
            expression: expression.into(),
            comment: comment.into(),
            favourite,
            min: range.map(|r| r.0),
            max: range.map(|r| r.1),
            step: range.map(|r| r.2),
        };
        vec![
            row("width", "mm", "120 mm", "plate width", true, Some((60.0, 300.0, 1.0))),
            row("height", "mm", "80 mm", "plate height", true, Some((40.0, 200.0, 1.0))),
            row("margin", "mm", "12 mm", "hole centre from edge", false, Some((5.0, 40.0, 0.5))),
            row("hole_dia", "mm", "8 mm", "bolt clearance", true, Some((3.0, 20.0, 0.5))),
            row("holes", "", "4", "holes per row", true, Some((2.0, 10.0, 1.0))),
            row("corner", "mm", "6 mm", "corner radius", false, Some((0.0, 30.0, 0.5))),
            row("slot_w", "mm", "width / 2", "centre slot follows width", false, None),
        ]
    }
}

/// One shape, built: each field's value (millimetres, or the plain number) or error, and its
/// paths (points) or the first error that stopped them.
#[derive(Clone, Debug, PartialEq)]
pub struct BuiltShape {
    pub id: String,
    pub kind: String,
    pub fields: BTreeMap<String, Result<f64, String>>,
    pub paths: Result<Vec<PathData>, String>,
}

/// Build every shape of `sketch` against the solved parameters. A shape whose fields do not all
/// evaluate gets an error instead of paths; the others are unaffected.
pub fn build(sketch: &Sketch, solved: &Solved) -> Vec<BuiltShape> {
    sketch.shapes.iter().take(MAX_SHAPES).map(|s| build_shape(s, sketch.origin, solved)).collect()
}

fn mm_to_pt(mm: f64) -> f64 {
    Unit::Millimeters.to_pt(mm)
}

fn build_shape(shape: &SketchShape, origin: [f64; 2], solved: &Solved) -> BuiltShape {
    let mut fields = BTreeMap::new();
    let Some(names) = fields_of(&shape.kind) else {
        let kind = &shape.kind;
        return BuiltShape { id: shape.id.clone(), kind: shape.kind.clone(), fields, paths: Err(format!("unknown shape \"{kind}\"")) };
    };
    for name in names {
        let src = shape.fields.get(*name).map(String::as_str).unwrap_or("");
        fields.insert(name.to_string(), solved.eval(src));
    }
    let paths = geometry(&shape.kind, &fields, origin);
    BuiltShape { id: shape.id.clone(), kind: shape.kind.clone(), fields, paths }
}

fn geometry(kind: &str, fields: &BTreeMap<String, Result<f64, String>>, origin: [f64; 2]) -> Result<Vec<PathData>, String> {
    let get = |name: &str| -> Result<f64, String> {
        match fields.get(name) {
            Some(Ok(v)) => Ok(*v),
            Some(Err(e)) => Err(format!("{name}: {e}")),
            None => Err(format!("{name}: empty")),
        }
    };
    // Lengths to points, offset by the sketch's origin; refused past the canvas's limits.
    let pt = |mm: f64, axis: usize| -> Result<f64, String> {
        let v = mm_to_pt(mm + origin.get(axis).copied().unwrap_or(0.0));
        if v.is_finite() && v.abs() <= MAX_COORD { Ok(v) } else { Err("out of range".into()) }
    };
    let len = |mm: f64| -> Result<f64, String> {
        let v = mm_to_pt(mm);
        if v.is_finite() && v.abs() <= MAX_COORD { Ok(v) } else { Err("out of range".into()) }
    };
    match kind {
        "rect" => {
            let (x, y, w, h, r) = (pt(get("x")?, 0)?, pt(get("y")?, 1)?, len(get("w")?)?, len(get("h")?)?, len(get("r")?)?);
            let rect = Rect::new(x, y, x + w, y + h).abs();
            let radius = shapes::fitted_corner_radius(rect.width(), rect.height(), r);
            Ok(vec![if radius > 0.0 { shapes::rounded_rectangle(rect, radius) } else { shapes::rectangle(rect) }])
        }
        "circle" => {
            let (cx, cy, d) = (pt(get("cx")?, 0)?, pt(get("cy")?, 1)?, len(get("d")?)?);
            let n = get("n")?;
            let pitch = len(get("pitch")?)?;
            if !n.is_finite() {
                return Err("n: not a number".into());
            }
            let n = (n.round().max(1.0) as usize).min(MAX_COPIES);
            let r = d.abs() / 2.0;
            (0..n)
                .map(|i| {
                    let c = cx + i as f64 * pitch;
                    if c.is_finite() && c.abs() <= MAX_COORD {
                        Ok(shapes::ellipse(Rect::new(c - r, cy - r, c + r, cy + r)))
                    } else {
                        Err("pitch: out of range".into())
                    }
                })
                .collect()
        }
        "line" => {
            let (x, y, l) = (pt(get("x")?, 0)?, pt(get("y")?, 1)?, len(get("len")?)?);
            let ang = get("ang")?.to_radians();
            // Counter-clockwise on the page: y is down, so a positive angle lifts the far end.
            let b = Point::new(x + ang.cos() * l, y - ang.sin() * l);
            if !(b.x.is_finite() && b.y.is_finite()) {
                return Err("ang: not a number".into());
            }
            Ok(vec![shapes::line(Point::new(x, y), b)])
        }
        kind => Err(format!("unknown shape \"{kind}\"")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params;

    fn built(params: &[ParamRow]) -> Vec<BuiltShape> {
        build(&Sketch::sample(), &params::solve(params))
    }

    fn counts(b: &[BuiltShape]) -> Vec<(String, usize)> {
        b.iter().map(|s| (s.id.clone(), s.paths.as_ref().map(Vec::len).unwrap_or(0))).collect()
    }

    #[test]
    fn the_sample_builds_one_rect_eight_circles_and_a_slot() {
        let b = built(&Sketch::sample_params());
        assert_eq!(counts(&b), vec![("R1".into(), 1), ("C1".into(), 4), ("C2".into(), 4), ("R2".into(), 1)]);
        assert!(b.iter().all(|s| s.paths.is_ok()), "{b:#?}");
        // The plate is 120 × 80 mm in points, hanging down from the origin.
        let plate = b[0].paths.as_ref().unwrap()[0].bounds().unwrap();
        assert!((plate.x0).abs() < 1e-9 && (plate.y0).abs() < 1e-9);
        assert!((plate.width() - Unit::Millimeters.to_pt(120.0)).abs() < 1e-9, "{plate:?}");
        assert!((plate.height() - Unit::Millimeters.to_pt(80.0)).abs() < 1e-9, "{plate:?}");
        // The hole rows: C1 at margin (top), C2 at height - margin (bottom); 32 mm apart.
        let c1 = b[1].paths.as_ref().unwrap();
        let c2 = b[2].paths.as_ref().unwrap();
        let cy1 = c1[0].bounds().unwrap().center().y;
        let cy2 = c2[0].bounds().unwrap().center().y;
        assert!(cy1 < cy2);
        let pitch = c1[1].bounds().unwrap().center().x - c1[0].bounds().unwrap().center().x;
        assert!((pitch - Unit::Millimeters.to_pt(32.0)).abs() < 1e-9, "{pitch}");
        assert_eq!(b[1].fields["pitch"], Ok(32.0));
    }

    #[test]
    fn six_holes_give_twelve_circles() {
        let mut p = Sketch::sample_params();
        p.iter_mut().find(|r| r.name == "holes").unwrap().expression = "6".into();
        let b = built(&p);
        assert_eq!(counts(&b), vec![("R1".into(), 1), ("C1".into(), 6), ("C2".into(), 6), ("R2".into(), 1)]);
    }

    #[test]
    fn a_broken_field_fails_its_shape_only() {
        let mut sketch = Sketch::sample();
        sketch.shapes[1].fields.insert("d".into(), "hole_dia +".into());
        let b = build(&sketch, &params::solve(&Sketch::sample_params()));
        assert_eq!(b[1].paths, Err("d: unfinished".into()));
        assert_eq!(b[1].fields["d"], Err("unfinished".into()));
        assert!(b[0].paths.is_ok() && b[2].paths.is_ok() && b[3].paths.is_ok());
        // A missing parameter and a missing field each say so.
        sketch.shapes[0].fields.insert("w".into(), "plate_w".into());
        sketch.shapes[3].fields.remove("r");
        let b = build(&sketch, &params::solve(&Sketch::sample_params()));
        assert_eq!(b[0].paths, Err("w: no parameter \"plate_w\"".into()));
        assert_eq!(b[3].paths, Err("r: empty".into()));
    }

    #[test]
    fn copies_and_coordinates_are_capped() {
        let mut sketch = Sketch::default();
        let mut c = SketchShape::default_of("circle", "C1".into()).unwrap();
        c.fields.insert("n".into(), "1000000".into());
        sketch.shapes.push(c);
        let mut r = SketchShape::default_of("rect", "R1".into()).unwrap();
        r.fields.insert("x".into(), "10 ^ 300".into());
        sketch.shapes.push(r);
        let b = build(&sketch, &Solved::default());
        assert_eq!(b[0].paths.as_ref().unwrap().len(), MAX_COPIES);
        assert_eq!(b[1].paths, Err("out of range".into()));
    }

    #[test]
    fn lines_angle_counter_clockwise_and_the_origin_moves_everything() {
        let mut sketch = Sketch::default();
        let mut l = SketchShape::default_of("line", "L1".into()).unwrap();
        l.fields.insert("ang".into(), "90".into());
        sketch.shapes.push(l);
        sketch.origin = [10.0, 20.0];
        let b = build(&sketch, &Solved::default());
        let path = &b[0].paths.as_ref().unwrap()[0];
        let bounds = path.bounds().unwrap();
        assert!((bounds.x0 - mm_to_pt(10.0)).abs() < 1e-9 && (bounds.x1 - mm_to_pt(10.0)).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.y1 - mm_to_pt(20.0)).abs() < 1e-9 && (bounds.y0 - mm_to_pt(-30.0)).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn ids_rename_and_round_trip() {
        let mut s = Sketch::sample();
        assert_eq!((s.next_id("rect"), s.next_id("circle"), s.next_id("line")), ("R3".into(), "C3".into(), "L1".into()));
        s.rename_references("width", "plate_w");
        assert_eq!(s.shapes[0].fields["w"], "plate_w");
        assert_eq!(s.shapes[1].fields["pitch"], "(plate_w - 2*margin) / (holes - 1)");
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("origin").is_none(), "{json}");
        assert_eq!(serde_json::from_value::<Sketch>(json).unwrap(), s);
        assert_eq!(field_label("circle", "d"), "diameter");
        assert!(unitless("n") && unitless("ang") && !unitless("d"));
    }
}
