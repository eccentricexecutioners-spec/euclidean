//! Parametric sketches (Euclidean): ordinary groups whose paths are built from a recipe in
//! `NodeExtras::sketch` (`vectorcraft_doc::sketch`) over the document's user parameters
//! (`cmd::parameters`). [`refresh`] runs inside every `Session::edit`, so a parameter change, a
//! recipe edit and the sketch's rebuilt paths share one undo step; everything else in the app
//! sees plain paths in a plain group.
use super::*;
use serde_json::{Value, json};
use std::sync::Arc;
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::params::ParamRow;
use vectorcraft_doc::sketch::model::{Constraint, Entity};
use vectorcraft_doc::sketch::solve::{self, SolveError};
use vectorcraft_doc::sketch::{self, Sketch, SketchShape, output};
use vectorcraft_doc::{Appearance, Document, Node, NodeId, NodeKind, Unit, expr, params};

/// The Layers panel name of a sketch group.
pub const GROUP_NAME: &str = "Parametric Sketch";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "sketch.create",
            "New Parametric Sketch",
            [],
            None,
            "{shapes?: [{kind: rect|circle|line, id?, fields?: {x, y, w, h, r | cx, cy, d, n, pitch | x, y, len, ang}}] (default: one rectangle)} a new parametric sketch group in the current layer, selected; fields are expressions over the user parameters, lengths in mm → {id}",
            has_doc,
            create
        ),
        cmd!(
            "sketch.sample",
            "Sample: Mounting plate",
            ["Object", "Parametric Sketch"],
            None,
            "{} adds the sample's seven user parameters (those the drawing lacks) and its sketch: a rounded plate, two rows of holes and a centre slot, selected → {id}",
            has_doc,
            sample
        ),
        cmd!(
            "sketch.addShape",
            "Add Sketch Shape",
            [],
            None,
            "{id? (default: the selected sketch), kind: rect|circle|line} → {id, shape}",
            has_doc,
            add_shape
        ),
        cmd!(
            "sketch.setField",
            "Set Sketch Field",
            [],
            None,
            "{id? (default: the selected sketch), shape, field, expression} → {id, shape, field, value, error}; a field that does not evaluate keeps the shape's previous paths",
            has_doc,
            set_field
        ),
        cmd!(
            "sketch.removeShape",
            "Remove Sketch Shape",
            [],
            None,
            "{id? (default: the selected sketch), shape} → {id, shape}",
            has_doc,
            remove_shape
        ),
        cmd!(
            "sketch.new",
            "New Sketch",
            ["Object", "Parametric Sketch"],
            None,
            "{} a new, empty drawn sketch (points, lines, circles and arcs with constraints and dimensions) in the current layer, selected → {id}",
            has_doc,
            new_sketch
        ),
        cmd!(
            "sketch.addLine",
            "Sketch Line",
            [],
            None,
            "{id?, from?: [x, y], fromPoint?: point id, to?: [x, y], toPoint?: point id, length?, angle?, horizontal?: bool, vertical?: bool, construction?: bool} sketch coordinates in mm, y down; typed length (mm) and angle (degrees, counter-clockwise) become dimensions d1, d2…; fromPoint/toPoint reuse a point (a chain) → {id, line, a, b, dimensions: [{name, index}]}",
            has_doc,
            add_line
        ),
        cmd!(
            "sketch.addRect",
            "Sketch Rectangle",
            [],
            None,
            "{id?, mode?: corner|center, from: [x, y], to: [x, y], width?, height?, construction?} corner mode: opposite corners; center mode: from is the centre; typed width/height (mm) override the mouse extents and become dimensions → {id, lines: [top, right, bottom, left], points, dimensions}",
            has_doc,
            add_rect
        ),
        cmd!(
            "sketch.addCircle",
            "Sketch Circle",
            [],
            None,
            "{id?, center?: [x, y], centerPoint?: point id, diameter?, r?: mm} a typed diameter becomes a dimension; r is the mouse radius → {id, circle, center, dimensions}",
            has_doc,
            add_circle
        ),
        cmd!(
            "sketch.addArc",
            "Sketch Arc",
            [],
            None,
            "{id?, start?: [x, y], startPoint?, end?: [x, y], endPoint?, through: [x, y], radius?} a three-point arc, counter-clockwise on screen through `through`; a typed radius becomes a dimension → {id, arc, center, start, end, dimensions}",
            has_doc,
            add_arc
        ),
        cmd!(
            "sketch.fillet",
            "Sketch Fillet",
            [],
            None,
            "{id?, point: corner point id, radius} rounds a corner where two lines meet with a tangent arc and a radius dimension → {id, arc, dimensions}",
            has_doc,
            fillet
        ),
        cmd!(
            "sketch.addConstraint",
            "Add Sketch Constraint",
            [],
            None,
            "{id?, constraint: {type: coincident|pointOnLine|pointOnCircle|horizontal|vertical|parallel|perpendicular|tangent|equal|fix|midpoint|concentric|symmetric, …refs: a, b, p, line, e}} fix without x/y pins the point where it is; refused when redundant or contradictory → {id, index}",
            has_doc,
            add_constraint
        ),
        cmd!(
            "sketch.addDimension",
            "Add Sketch Dimension",
            [],
            None,
            "{id?, constraint: {type: distance|length|angle|angleBetween|radius|diameter|distancePointLine, …refs: a, b, p, line, e, along?: direct|x|y (distance: aligned, horizontal or vertical)}, expression?, at?: [x, y] (mm, where the value sits), driven?: bool (a reference dimension: it reads the geometry and does not constrain it; shown in brackets)} without expression the current measured value is used; creates a model parameter dN → {id, index, name, value, driven}",
            has_doc,
            add_dimension
        ),
        cmd!(
            "sketch.placeDimension",
            "Move Sketch Dimension",
            [],
            None,
            "{id?, name? | index?, at: [x, y] | null} puts the dimension's value at `at` (sketch mm); null lets it sit beside its geometry again → {id, name, at}",
            has_doc,
            place_dimension
        ),
        cmd!(
            "sketch.setDriven",
            "Set Sketch Dimension Driven",
            [],
            None,
            "{id?, name? | index?, driven: bool} a driven (reference) dimension reads the geometry; making it driving again is refused when that would over-constrain the sketch → {id, name, driven, value}",
            has_doc,
            set_driven
        ),
        cmd!(
            "sketch.setDimension",
            "Set Sketch Dimension",
            [],
            None,
            "{id?, name? | index?, expression} sets the dimension's model parameter; refused when the sketch no longer solves; the same value is no undo step → {id, name, value, error}",
            has_doc,
            set_dimension
        ),
        cmd!(
            "sketch.removeConstraint",
            "Remove Sketch Constraint",
            [],
            None,
            "{id?, index} removing a dimension also removes its model parameter when nothing else uses it → {id, removed}",
            has_doc,
            remove_constraint
        ),
        cmd!(
            "sketch.deleteEntities",
            "Delete Sketch Geometry",
            [],
            None,
            "{id?, entities: [entity ids]} deletes lines, circles and arcs with their constraints, dimensions and unused points → {id}",
            has_doc,
            delete_entities
        ),
        cmd!(
            "sketch.trim",
            "Sketch Trim",
            [],
            None,
            "{id?, entity: line id, at: [x, y]} removes the piece of the line between the two crossings that bracket `at` (to the line's end where there is none); the pieces left share the line's end points, new points at the cuts lie on what cut them; a line with no crossing is deleted; arcs and circles are refused → {id, kept: [line ids], removed: [[x, y], [x, y]]}",
            has_doc,
            trim
        ),
        cmd!(
            "sketch.offset",
            "Sketch Offset",
            [],
            None,
            "{id?, entities: [line ids forming a chain or loop], distance (expression, mm), side?: 1|-1 (1 = left of the first line as drawn, as seen on screen)} copies the lines offset by the distance, joined at their intersections, each parallel to its source; the distance becomes a dimension (model parameter dN) from the first line → {id, lines, points, dimensions}",
            has_doc,
            offset
        ),
        cmd!(
            "sketch.mirror",
            "Sketch Mirror",
            [],
            None,
            "{id?, entities: [shape ids], line: line id} copies the shapes mirrored about the line, with a symmetric constraint for each mirrored point pair → {id, entities, points}",
            has_doc,
            mirror
        ),
        cmd!(
            "sketch.setConstruction",
            "Toggle Construction",
            [],
            None,
            "{id?, entities: [entity ids], construction?: bool} construction geometry is drawn but not output; absent toggles each → {id}",
            has_doc,
            set_construction
        ),
        cmd!(
            query "sketch.inspect",
            "Inspect Parametric Sketch",
            [],
            None,
            "{id? (default: the selected sketch)} → {id, origin: [x, y] (mm), shapes: [{id, kind, fields: {field: {expression, value, error}}}]}; a drawn sketch adds points: [{id, x, y, constrained}], entities: [{id, kind, a?, b?, c?, r?, construction, constrained}], constraints: [{index, type, …refs, param?, expression?, value?, error?}], dof, fullyConstrained, solveError?",
            has_doc,
            inspect
        ),
    ]
}

/// Is `n` a parametric sketch group?
pub fn is_sketch(n: &Node) -> bool {
    n.extras().sketch.is_some()
}

/// The sketch `p.id` names, or the one selected sketch.
fn target(s: &Session, p: &Value, cmd: &str) -> Result<NodeId> {
    let st = s.doc()?;
    let id = match id_param(p, "id") {
        Some(id) => id,
        None => match st.selection.objects.as_slice() {
            [one] => *one,
            _ => return Err(bad(cmd, "select one parametric sketch")),
        },
    };
    match st.doc.node(id) {
        None => Err(EngineError::NoNode(id)),
        Some(n) if !is_sketch(n) => Err(bad(cmd, format!("node {} is not a parametric sketch", id.0))),
        Some(_) if !st.doc.is_editable(id) => Err(bad(cmd, format!("node {} is locked or hidden", id.0))),
        Some(_) => Ok(id),
    }
}

fn recipe_of(doc: &Document, id: NodeId) -> Option<Sketch> {
    doc.node(id).and_then(|n| n.extras().sketch.as_deref().cloned())
}

/// Store `sketch` on `id`; the refresh at the end of the edit rebuilds the paths.
fn set_recipe(doc: &mut Document, id: NodeId, sketch: Sketch) -> Result<()> {
    let n = doc.node_mut(id).ok_or(EngineError::NoNode(id))?;
    n.extras_mut().sketch = Some(Box::new(sketch));
    Ok(())
}

fn kind_param(p: &Value, cmd: &str) -> Result<String> {
    match str_param(p, "kind") {
        Some(k) if sketch::fields_of(k).is_some() => Ok(k.to_string()),
        _ => Err(bad(cmd, "`kind` must be rect, circle or line")),
    }
}

fn expression_value(v: &Value, cmd: &str) -> Result<String> {
    match v {
        Value::String(s) if s.len() <= expr::MAX_LEN => Ok(s.trim().to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::String(_) => Err(bad(cmd, format!("an expression is longer than {} characters", expr::MAX_LEN))),
        _ => Err(bad(cmd, "an expression must be text")),
    }
}

/// One shape of a `sketch.create` `shapes` entry: the kind's defaults, then the given fields.
fn shape_param(sketch: &Sketch, v: &Value, cmd: &str) -> Result<SketchShape> {
    let kind = kind_param(v, cmd)?;
    let id = match str_param(v, "id").map(str::trim) {
        Some(id) if !id.is_empty() && id.chars().count() <= 32 && !sketch.shapes.iter().any(|s| s.id == id) => id.to_string(),
        Some(_) => return Err(bad(cmd, "shape ids are 1–32 characters and unique within the sketch")),
        None => sketch.next_id(&kind),
    };
    let mut shape = SketchShape::default_of(&kind, id).ok_or_else(|| bad(cmd, "`kind` must be rect, circle or line"))?;
    if let Some(fields) = v.get("fields") {
        let obj = fields.as_object().ok_or_else(|| bad(cmd, "`fields` must be an object"))?;
        for (k, e) in obj {
            if !sketch::fields_of(&kind).is_some_and(|f| f.contains(&k.as_str())) {
                return Err(bad(cmd, format!("a {kind} has no field `{k}`")));
            }
            shape.fields.insert(k.clone(), expression_value(e, cmd)?);
        }
    }
    Ok(shape)
}

/// Insert a new sketch group holding `sketch` in the current layer and select it.
fn insert(s: &mut Session, label: &str, sketch: Sketch) -> Result<NodeId> {
    let parent = s.doc()?.target_parent()?;
    s.edit(label, |d, sel| {
        let id = d.alloc_id();
        let mut n = Node::group(id, vec![]);
        n.name = Some(GROUP_NAME.into());
        n.extras_mut().sketch = Some(Box::new(sketch));
        d.insert(parent, usize::MAX, n)?;
        sel.set([id]);
        Ok(id)
    })
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.create";
    let mut sketch = Sketch::default();
    match p.get("shapes") {
        None => sketch.shapes.push(SketchShape::default_of("rect", "R1".into()).ok_or_else(|| bad(C, "no default shape"))?),
        Some(Value::Array(shapes)) => {
            if shapes.len() > sketch::MAX_SHAPES {
                return Err(bad(C, format!("a sketch holds at most {} shapes", sketch::MAX_SHAPES)));
            }
            for v in shapes {
                let shape = shape_param(&sketch, v, C)?;
                sketch.shapes.push(shape);
            }
        }
        Some(_) => return Err(bad(C, "`shapes` must be a list")),
    }
    let id = insert(s, "New Parametric Sketch", sketch)?;
    Ok(json!({"id": id.0}))
}

fn sample(s: &mut Session, _p: &Value) -> Result<Value> {
    let mut rows = params::rows(&s.doc()?.doc);
    for row in Sketch::sample_params() {
        if !rows.iter().any(|r| r.name == row.name) {
            rows.push(row);
        }
    }
    let parent = s.doc()?.target_parent()?;
    let id = s.edit("Sample Parametric Sketch", |d, sel| {
        params::store(d, &rows);
        let id = d.alloc_id();
        let mut n = Node::group(id, vec![]);
        n.name = Some(GROUP_NAME.into());
        n.extras_mut().sketch = Some(Box::new(Sketch::sample()));
        d.insert(parent, usize::MAX, n)?;
        sel.set([id]);
        Ok(id)
    })?;
    Ok(json!({"id": id.0}))
}

fn add_shape(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.addShape";
    let id = target(s, p, C)?;
    let kind = kind_param(p, C)?;
    let mut sketch = recipe_of(&s.doc()?.doc, id).unwrap_or_default();
    if sketch.shapes.len() >= sketch::MAX_SHAPES {
        return Err(bad(C, format!("a sketch holds at most {} shapes", sketch::MAX_SHAPES)));
    }
    let shape_id = sketch.next_id(&kind);
    let shape = SketchShape::default_of(&kind, shape_id.clone()).ok_or_else(|| bad(C, "`kind` must be rect, circle or line"))?;
    sketch.shapes.push(shape);
    s.edit("Add Sketch Shape", |d, _| set_recipe(d, id, sketch))?;
    Ok(json!({"id": id.0, "shape": shape_id}))
}

fn shape_index(sketch: &Sketch, p: &Value, cmd: &str) -> Result<usize> {
    let name = str_param(p, "shape").ok_or_else(|| bad(cmd, "missing `shape`"))?;
    sketch.shapes.iter().position(|s| s.id == name).ok_or_else(|| bad(cmd, format!("no shape `{name}` in this sketch")))
}

fn set_field(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.setField";
    let id = target(s, p, C)?;
    let mut sketch = recipe_of(&s.doc()?.doc, id).unwrap_or_default();
    let i = shape_index(&sketch, p, C)?;
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.to_string();
    let kind = sketch.shapes.get(i).map(|s| s.kind.clone()).unwrap_or_default();
    if !sketch::fields_of(&kind).is_some_and(|f| f.contains(&field.as_str())) {
        return Err(bad(C, format!("a {kind} has no field `{field}`")));
    }
    let expression = expression_value(p.get("expression").unwrap_or(&Value::Null), C)?;
    let shape_id = sketch.shapes.get(i).map(|s| s.id.clone()).unwrap_or_default();
    if let Some(shape) = sketch.shapes.get_mut(i) {
        shape.fields.insert(field.clone(), expression.clone());
    }
    let solved = params::solve(&params::rows(&s.doc()?.doc));
    let (value, error) = match solved.eval(&expression) {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e)),
    };
    if recipe_of(&s.doc()?.doc, id).as_ref() != Some(&sketch) {
        s.edit("Set Sketch Field", |d, _| set_recipe(d, id, sketch))?;
    }
    Ok(json!({"id": id.0, "shape": shape_id, "field": field, "value": value, "error": error}))
}

fn remove_shape(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.removeShape";
    let id = target(s, p, C)?;
    let mut sketch = recipe_of(&s.doc()?.doc, id).unwrap_or_default();
    let i = shape_index(&sketch, p, C)?;
    let shape_id = sketch.shapes.get(i).map(|s| s.id.clone()).unwrap_or_default();
    sketch.shapes.remove(i);
    s.edit("Remove Sketch Shape", |d, _| set_recipe(d, id, sketch))?;
    Ok(json!({"id": id.0, "shape": shape_id}))
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.inspect";
    let id = target(s, p, C)?;
    let doc = &s.doc()?.doc;
    let sketch = recipe_of(doc, id).unwrap_or_default();
    let solved = params::solve(&params::rows(doc));
    let shapes: Vec<Value> = sketch::build(&sketch, &solved)
        .into_iter()
        .map(|b| {
            let shape = sketch.shapes.iter().find(|s| s.id == b.id);
            let fields: serde_json::Map<String, Value> = sketch::fields_of(&b.kind)
                .unwrap_or(&[])
                .iter()
                .map(|f| {
                    let expression = shape.and_then(|s| s.fields.get(*f)).cloned().unwrap_or_default();
                    let (value, error) = match b.fields.get(*f) {
                        Some(Ok(v)) => (Some(*v), None),
                        Some(Err(e)) => (None, Some(e.clone())),
                        None => (None, None),
                    };
                    (f.to_string(), json!({"expression": expression, "value": value, "error": error}))
                })
                .collect();
            json!({"id": b.id, "kind": b.kind, "fields": fields, "error": b.paths.err()})
        })
        .collect();
    let mut out = json!({"id": id.0, "origin": sketch.origin, "shapes": shapes});
    if sketch.is_v2()
        && let Some(o) = out.as_object_mut()
    {
        o.extend(inspect_v2(&sketch, &params::rows(doc), &solved));
    }
    Ok(out)
}

/// What a sketch's paths look like: the document's default 1 pt black stroke, no fill.
fn appearance() -> Appearance {
    Appearance::basic(Paint::None, Paint::solid(Color::BLACK), 1.0)
}

/// Rebuild the children of sketch `id` from `sketch`: new path nodes per shape, named after the
/// shape; a shape that does not evaluate keeps its previous paths.
fn rebuild(doc: &mut Document, id: NodeId, sketch: &Sketch, solved: &expr::Solved) {
    let old: Vec<Arc<Node>> = doc.node(id).and_then(Node::children).cloned().unwrap_or_default();
    let mut children = Vec::new();
    for b in sketch::build(sketch, solved) {
        match b.paths {
            Ok(paths) => {
                for path in paths {
                    let cid = doc.alloc_id();
                    let mut n = Node::path(cid, path, appearance());
                    n.name = Some(b.id.clone());
                    children.push(Arc::new(n));
                }
            }
            Err(_) => children.extend(old.iter().filter(|c| c.name.as_deref() == Some(b.id.as_str())).cloned()),
        }
    }
    if sketch.is_v2() {
        for (name, path) in output::paths(sketch) {
            let cid = doc.alloc_id();
            let mut n = Node::path(cid, path, appearance());
            n.name = Some(name);
            children.push(Arc::new(n));
        }
    }
    if let Some(n) = doc.node_mut(id) {
        n.kind = NodeKind::Group { children, clip: false };
    }
}

/// Called inside `Session::edit`, after the edit: every sketch whose recipe or whose parameters
/// changed in this edit is rebuilt from the recipe, in the same undo step. A sketch the edit
/// only moved (the Selection tool) keeps the move: the offset goes into the recipe's origin. A
/// sketch scaled or rotated keeps its transformed paths until the next rebuild.
pub(crate) fn refresh(before: &Document, doc: &mut Document) {
    let mut ids = Vec::new();
    doc.walk(|n| {
        if is_sketch(n) {
            ids.push(n.id);
        }
    });
    if ids.is_empty() {
        return;
    }
    let params_changed = params::changed(before, doc);
    let solved = params::solve(&params::rows(doc));
    for id in ids {
        let Some(node) = doc.node(id) else { continue };
        let Some(sketch) = node.extras().sketch.as_deref().cloned() else { continue };
        let old = before.node(id).filter(|n| is_sketch(n));
        let recipe_changed = old.and_then(|n| n.extras().sketch.as_deref()) != Some(&sketch);
        if params_changed || recipe_changed {
            let mut sketch = sketch;
            sketch.sanitize();
            // A drawn sketch is solved first; the solved positions are written back so the next
            // solve starts there. When it does not solve, the stored positions are drawn.
            if sketch.is_v2()
                && let Ok(solution) = solve::solve(&sketch, &solved)
                && solution.sketch != sketch
            {
                sketch = solution.sketch;
                if let Some(n) = doc.node_mut(id) {
                    n.extras_mut().sketch = Some(Box::new(sketch.clone()));
                }
                if !sketch.driven.is_empty() {
                    let mut rows = params::rows(doc);
                    if read_driven(&sketch, id, &mut rows) {
                        params::store(doc, &rows);
                    }
                }
            }
            rebuild(doc, id, &sketch, &solved);
            continue;
        }
        // Same recipe, same parameters: did the edit move the group?
        let (Some(b0), Some(b1)) = (old.and_then(Node::geometric_bounds), node.geometric_bounds()) else { continue };
        if b0 == b1 {
            continue;
        }
        let same_size = (b0.width() - b1.width()).abs() < 1e-6 && (b0.height() - b1.height()).abs() < 1e-6;
        if same_size {
            let mut moved = sketch;
            moved.origin[0] += Unit::Millimeters.from_pt(b1.x0 - b0.x0);
            moved.origin[1] += Unit::Millimeters.from_pt(b1.y0 - b0.y0);
            moved.sanitize();
            if let Some(n) = doc.node_mut(id) {
                n.extras_mut().sketch = Some(Box::new(moved));
            }
        }
    }
}

// ---------- drawn sketches (v2): points, lines, circles and arcs with constraints ----------

/// The message when a new constraint or value contradicts, or repeats, what is already there.
const OVER_CONSTRAINED: &str = "This would over-constrain the sketch";
/// The message when the solver does not converge.
const NO_SOLUTION: &str = "The sketch cannot be solved with this value";
/// The message (inspect) for a stored sketch whose constraints contradict each other.
const CONTRADICTION: &str = "The constraints contradict each other";

/// A drawn sketch being edited: the group, the would-be recipe and the would-be parameter rows.
/// Nothing reaches the document until [`save`].
struct Work {
    id: NodeId,
    sketch: Sketch,
    rows: Vec<ParamRow>,
}

/// A typed value: its expression text and what it evaluates to (mm or degrees).
struct Typed {
    expression: String,
    value: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Length,
    Angle,
}

impl Kind {
    fn unit(self) -> &'static str {
        match self {
            Kind::Length => "mm",
            Kind::Angle => "deg",
        }
    }
}

fn work(s: &Session, p: &Value, cmd: &str) -> Result<Work> {
    let id = target(s, p, cmd)?;
    let doc = &s.doc()?.doc;
    let sketch = recipe_of(doc, id).unwrap_or_default();
    if !sketch.is_v2() {
        return Err(bad(cmd, "this sketch is made of shapes; use a sketch from sketch.new"));
    }
    Ok(Work { id, sketch, rows: params::rows(doc) })
}

fn solve_error(e: SolveError, cmd: &str) -> EngineError {
    match e {
        SolveError::OverConstrained => bad(cmd, OVER_CONSTRAINED),
        SolveError::NoSolution => bad(cmd, NO_SOLUTION),
    }
}

fn check_caps(sketch: &Sketch, cmd: &str) -> Result<()> {
    use sketch::model::{MAX_CONSTRAINTS, MAX_ENTITIES, MAX_POINTS};
    if sketch.points.len() > MAX_POINTS || sketch.entities.len() > MAX_ENTITIES || sketch.constraints.len() > MAX_CONSTRAINTS {
        return Err(bad(cmd, format!("a sketch holds at most {MAX_POINTS} points, {MAX_ENTITIES} shapes and {MAX_CONSTRAINTS} constraints")));
    }
    Ok(())
}

/// Solve the would-be recipe with the would-be parameters; the solved recipe replaces it. Fails
/// (and the caller edits nothing) when the sketch is over-constrained or does not solve.
fn solve_work(w: &mut Work, cmd: &str) -> Result<()> {
    check_caps(&w.sketch, cmd)?;
    let solved = params::solve(&w.rows);
    w.sketch = solve::solve(&w.sketch, &solved).map_err(|e| solve_error(e, cmd))?.sketch;
    Ok(())
}

/// Store the parameter rows and the recipe together: one undo step.
fn save(s: &mut Session, w: Work, label: &str) -> Result<()> {
    let Work { id, sketch, mut rows } = w;
    read_driven(&sketch, id, &mut rows);
    s.edit(label, |d, _| {
        params::store(d, &rows);
        set_recipe(d, id, sketch)
    })
}

/// Driven dimensions read the geometry: their rows take the measured value. Returns whether a row
/// changed.
pub(crate) fn read_driven(sketch: &Sketch, id: NodeId, rows: &mut [ParamRow]) -> bool {
    let mut changed = false;
    for c in sketch.constraints.iter().filter(|c| sketch.is_driven(c)) {
        let (Some(name), Some(v)) = (c.param(), sketch.measure(c)) else { continue };
        let unit = if c.is_angular() { "deg" } else { "mm" };
        let expression = params::plain_expression(v, unit);
        if let Some(row) = rows.iter_mut().find(|r| r.name == name && r.sketch == Some(id.0))
            && row.expression != expression
        {
            row.expression = expression;
            changed = true;
        }
    }
    changed
}

/// Solve, then store (the usual end of a command that adds something).
fn commit(s: &mut Session, mut w: Work, label: &str, cmd: &str) -> Result<()> {
    solve_work(&mut w, cmd)?;
    save(s, w, label)
}

/// A typed value under `key`: text or a number, evaluated against the document's parameters. A
/// plain length gets `mm`. Lengths must be above zero.
fn typed(w: &Work, p: &Value, key: &str, kind: Kind, cmd: &str) -> Result<Option<Typed>> {
    let raw = match p.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(v) => v,
    };
    let mut expression = expression_value(raw, cmd)?;
    if kind == Kind::Length && expr::is_plain(&expression) && expression.ends_with(|c: char| c.is_ascii_digit() || c == '.') {
        expression.push_str(" mm");
    }
    let value = params::solve(&w.rows).eval(&expression).map_err(|e| bad(cmd, e))?;
    if !value.is_finite() {
        return Err(bad(cmd, format!("`{key}` is not a number")));
    }
    if kind == Kind::Length && value <= 1e-9 {
        return Err(bad(cmd, format!("`{key}` must be greater than zero")));
    }
    Ok(Some(Typed { expression, value }))
}

/// Add a model parameter row and return its name.
fn add_param(w: &mut Work, kind: Kind, expression: &str) -> String {
    let name = params::next_model_name(&w.rows);
    w.rows.push(ParamRow { name: name.clone(), unit: kind.unit().into(), expression: expression.into(), sketch: Some(w.id.0), ..Default::default() });
    name
}

/// Add a dimension bound to a new model parameter holding `t`; returns `{name, index}`.
fn add_dim(w: &mut Work, kind: Kind, t: &Typed, make: impl FnOnce(String) -> Constraint) -> Value {
    let name = add_param(w, kind, &t.expression);
    w.sketch.constraints.push(make(name.clone()));
    json!({"name": name, "index": w.sketch.constraints.len() - 1})
}

fn xy(p: &Value, key: &str, cmd: &str) -> Result<Option<(f64, f64)>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match finite_numbers::<2>(v) {
            Some([x, y]) => Ok(Some((x, y))),
            None => Err(bad(cmd, format!("`{key}` must be [x, y] in mm"))),
        },
    }
}

fn index_value(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// An existing point id under `key`.
fn point_ref(w: &Work, p: &Value, key: &str, cmd: &str) -> Result<Option<u32>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let id = index_value(v).ok_or_else(|| bad(cmd, format!("`{key}` must be a point id")))?;
            if w.sketch.point(id).is_some() { Ok(Some(id)) } else { Err(bad(cmd, format!("no point {id} in this sketch"))) }
        }
    }
}

fn position(w: &Work, id: u32) -> (f64, f64) {
    w.sketch.point(id).map_or((0.0, 0.0), |p| (p.x, p.y))
}

fn new_sketch(s: &mut Session, _p: &Value) -> Result<Value> {
    let id = insert(s, "New Sketch", Sketch::new_v2())?;
    Ok(json!({"id": id.0}))
}

fn add_line(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.addLine";
    let mut w = work(s, p, C)?;
    let length = typed(&w, p, "length", Kind::Length, C)?;
    let angle = typed(&w, p, "angle", Kind::Angle, C)?;
    let (from_point, from_xy) = (point_ref(&w, p, "fromPoint", C)?, xy(p, "from", C)?);
    let (to_point, to_xy) = (point_ref(&w, p, "toPoint", C)?, xy(p, "to", C)?);
    let start = match (from_point, from_xy) {
        (Some(id), _) => position(&w, id),
        (None, Some(at)) => at,
        _ => return Err(bad(C, "missing `from` or `fromPoint`")),
    };
    let end = match (to_point, to_xy) {
        (Some(id), _) => position(&w, id),
        (None, mouse) => {
            let mouse_len = mouse.map(|m| (m.0 - start.0).hypot(m.1 - start.1));
            let len = length.as_ref().map(|t| t.value).or(mouse_len).ok_or_else(|| bad(C, "missing `to`, `toPoint` or a typed length"))?;
            let (ux, uy) = match (&angle, mouse) {
                (Some(a), _) => (a.value.to_radians().cos(), -a.value.to_radians().sin()),
                (None, Some(m)) if mouse_len.is_some_and(|l| l > 1e-9) => {
                    let l = mouse_len.unwrap_or(1.0);
                    ((m.0 - start.0) / l, (m.1 - start.1) / l)
                }
                _ => return Err(bad(C, "the line has no direction: give `to`, `toPoint` or a typed angle")),
            };
            (start.0 + ux * len, start.1 + uy * len)
        }
    };
    if from_point.is_some() && from_point == to_point {
        return Err(bad(C, "a line needs two different points"));
    }
    let a = from_point.unwrap_or_else(|| w.sketch.add_point(start.0, start.1));
    let b = to_point.unwrap_or_else(|| w.sketch.add_point(end.0, end.1));
    let line = w.sketch.add_line(a, b, bool_or(p, "construction", false));
    if bool_or(p, "horizontal", false) {
        w.sketch.constraints.push(Constraint::Horizontal { line });
    }
    if bool_or(p, "vertical", false) {
        w.sketch.constraints.push(Constraint::Vertical { line });
    }
    let mut dimensions = Vec::new();
    if let Some(t) = &length {
        dimensions.push(add_dim(&mut w, Kind::Length, t, |param| Constraint::Length { line, param }));
    }
    if let Some(t) = &angle {
        dimensions.push(add_dim(&mut w, Kind::Angle, t, |param| Constraint::Angle { line, param }));
    }
    let id = w.id;
    commit(s, w, "Sketch Line", C)?;
    Ok(json!({"id": id.0, "line": line, "a": a, "b": b, "dimensions": dimensions}))
}

fn add_rect(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.addRect";
    let mut w = work(s, p, C)?;
    let centred = match str_param(p, "mode") {
        None | Some("corner") => false,
        Some("center") => true,
        Some(_) => return Err(bad(C, "`mode` must be corner or center")),
    };
    let from = xy(p, "from", C)?.ok_or_else(|| bad(C, "missing `from`"))?;
    let to = xy(p, "to", C)?.ok_or_else(|| bad(C, "missing `to`"))?;
    let width = typed(&w, p, "width", Kind::Length, C)?;
    let height = typed(&w, p, "height", Kind::Length, C)?;
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
    let (wd, ht) = if centred {
        (width.as_ref().map_or(2.0 * dx.abs(), |t| t.value), height.as_ref().map_or(2.0 * dy.abs(), |t| t.value))
    } else {
        (width.as_ref().map_or(dx.abs(), |t| t.value), height.as_ref().map_or(dy.abs(), |t| t.value))
    };
    if wd <= 1e-9 || ht <= 1e-9 {
        return Err(bad(C, "the rectangle has no size"));
    }
    // Corners: top-left, top-right, bottom-right, bottom-left as drawn from the first corner.
    let (x0, y0, sx, sy) = if centred { (from.0 - wd / 2.0, from.1 - ht / 2.0, 1.0, 1.0) } else { (from.0, from.1, sign(dx), sign(dy)) };
    let corners = [(x0, y0), (x0 + sx * wd, y0), (x0 + sx * wd, y0 + sy * ht), (x0, y0 + sy * ht)];
    let mut points: Vec<u32> = corners.iter().map(|c| w.sketch.add_point(c.0, c.1)).collect();
    let construction = bool_or(p, "construction", false);
    let mut lines = Vec::new();
    for i in 0..4 {
        let (a, b) = (points.get(i).copied().unwrap_or(0), points.get((i + 1) % 4).copied().unwrap_or(0));
        lines.push(w.sketch.add_line(a, b, construction));
    }
    let [top, right, bottom, left] = lines[..] else { return Err(bad(C, "the rectangle needs four lines")) };
    w.sketch.constraints.extend([
        Constraint::Horizontal { line: top },
        Constraint::Horizontal { line: bottom },
        Constraint::Vertical { line: right },
        Constraint::Vertical { line: left },
    ]);
    let mut dimensions = Vec::new();
    if let Some(t) = &width {
        dimensions.push(add_dim(&mut w, Kind::Length, t, |param| Constraint::Length { line: top, param }));
    }
    if let Some(t) = &height {
        dimensions.push(add_dim(&mut w, Kind::Length, t, |param| Constraint::Length { line: left, param }));
    }
    let mut out = json!({});
    if centred {
        let centre = w.sketch.add_point(from.0, from.1);
        let (a, c) = (points.first().copied().unwrap_or(0), points.get(2).copied().unwrap_or(0));
        let diagonal = w.sketch.add_line(a, c, true);
        w.sketch.constraints.push(Constraint::Midpoint { p: centre, line: diagonal });
        points.push(centre);
        out = json!({"diagonal": diagonal});
    }
    let id = w.id;
    commit(s, w, "Sketch Rectangle", C)?;
    out["id"] = json!(id.0);
    out["lines"] = json!(lines);
    out["points"] = json!(points);
    out["dimensions"] = json!(dimensions);
    Ok(out)
}

fn add_circle(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.addCircle";
    let mut w = work(s, p, C)?;
    let diameter = typed(&w, p, "diameter", Kind::Length, C)?;
    let radius = match &diameter {
        Some(t) => t.value / 2.0,
        None => f64_or(p, "r", 0.0),
    };
    if !(radius.is_finite() && radius > 1e-9) {
        return Err(bad(C, "give a `diameter`, or a mouse radius `r` above zero"));
    }
    let centre = match (point_ref(&w, p, "centerPoint", C)?, xy(p, "center", C)?) {
        (Some(id), _) => id,
        (None, Some(at)) => w.sketch.add_point(at.0, at.1),
        _ => return Err(bad(C, "missing `center` or `centerPoint`")),
    };
    let circle = w.sketch.add_circle(centre, radius);
    let mut dimensions = Vec::new();
    if let Some(t) = &diameter {
        dimensions.push(add_dim(&mut w, Kind::Length, t, |param| Constraint::Diameter { e: circle, param }));
    }
    let id = w.id;
    commit(s, w, "Sketch Circle", C)?;
    Ok(json!({"id": id.0, "circle": circle, "center": centre, "dimensions": dimensions}))
}

/// The circumcentre of three points, or `None` when they lie in a line.
fn circumcentre(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> Option<(f64, f64)> {
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if d.abs() < 1e-9 {
        return None;
    }
    let (a2, b2, c2) = (a.0 * a.0 + a.1 * a.1, b.0 * b.0 + b.1 * b.1, c.0 * c.0 + c.1 * c.1);
    let x = (a2 * (b.1 - c.1) + b2 * (c.1 - a.1) + c2 * (a.1 - b.1)) / d;
    let y = (a2 * (c.0 - b.0) + b2 * (a.0 - c.0) + c2 * (b.0 - a.0)) / d;
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// The centre of an arc from `start` to `end` with radius `r`: on the chord's perpendicular
/// bisector, at the side that keeps `through` on the arc's bulge (the one nearest the three-point
/// circle when there is one).
fn centre_with_radius(start: (f64, f64), end: (f64, f64), through: (f64, f64), r: f64, cmd: &str) -> Result<(f64, f64)> {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let chord = dx.hypot(dy);
    if chord < 1e-9 {
        return Err(bad(cmd, "the arc's start and end are the same place"));
    }
    if 2.0 * r < chord - 1e-9 {
        return Err(bad(cmd, "the radius is too small for this arc"));
    }
    let k = (r * r - chord * chord / 4.0).max(0.0).sqrt();
    let mid = ((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0);
    let n = (-dy / chord, dx / chord);
    let (c1, c2) = ((mid.0 + n.0 * k, mid.1 + n.1 * k), (mid.0 - n.0 * k, mid.1 - n.1 * k));
    let near = |c: (f64, f64), to: (f64, f64)| (c.0 - to.0).hypot(c.1 - to.1);
    Ok(match circumcentre(start, end, through) {
        Some(guess) => {
            if near(c1, guess) <= near(c2, guess) {
                c1
            } else {
                c2
            }
        }
        // Collinear: the centre goes on the side away from `through`.
        None => {
            if (through.0 - mid.0) * n.0 + (through.1 - mid.1) * n.1 > 0.0 {
                c2
            } else {
                c1
            }
        }
    })
}

fn add_arc(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.addArc";
    let mut w = work(s, p, C)?;
    let radius = typed(&w, p, "radius", Kind::Length, C)?;
    let (start_point, start_xy) = (point_ref(&w, p, "startPoint", C)?, xy(p, "start", C)?);
    let (end_point, end_xy) = (point_ref(&w, p, "endPoint", C)?, xy(p, "end", C)?);
    let through = xy(p, "through", C)?.ok_or_else(|| bad(C, "missing `through`"))?;
    let start = match (start_point, start_xy) {
        (Some(id), _) => position(&w, id),
        (None, Some(at)) => at,
        _ => return Err(bad(C, "missing `start` or `startPoint`")),
    };
    let end = match (end_point, end_xy) {
        (Some(id), _) => position(&w, id),
        (None, Some(at)) => at,
        _ => return Err(bad(C, "missing `end` or `endPoint`")),
    };
    if start_point.is_some() && start_point == end_point {
        return Err(bad(C, "an arc needs two different end points"));
    }
    let centre_at = match &radius {
        Some(t) => centre_with_radius(start, end, through, t.value, C)?,
        None => circumcentre(start, end, through).ok_or_else(|| bad(C, "the three points are in a line"))?,
    };
    if (start.0 - end.0).hypot(start.1 - end.1) < 1e-9 {
        return Err(bad(C, "the arc's start and end are the same place"));
    }
    // Counter-clockwise on screen (y is down): measure angles with y flipped.
    let angle = |q: (f64, f64)| (-(q.1 - centre_at.1)).atan2(q.0 - centre_at.0);
    let sweep = (angle(end) - angle(start)).rem_euclid(std::f64::consts::TAU);
    let through_sweep = (angle(through) - angle(start)).rem_euclid(std::f64::consts::TAU);
    let start_id = start_point.unwrap_or_else(|| w.sketch.add_point(start.0, start.1));
    let end_id = end_point.unwrap_or_else(|| w.sketch.add_point(end.0, end.1));
    let centre = w.sketch.add_point(centre_at.0, centre_at.1);
    let (first, second) = if through_sweep <= sweep { (start_id, end_id) } else { (end_id, start_id) };
    let arc = w.sketch.add_arc(centre, first, second);
    let mut dimensions = Vec::new();
    if let Some(t) = &radius {
        dimensions.push(add_dim(&mut w, Kind::Length, t, |param| Constraint::Radius { e: arc, param }));
    }
    let id = w.id;
    commit(s, w, "Sketch Arc", C)?;
    Ok(json!({"id": id.0, "arc": arc, "center": centre, "start": start_id, "end": end_id, "dimensions": dimensions}))
}

fn fillet(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.fillet";
    let mut w = work(s, p, C)?;
    let corner = point_ref(&w, p, "point", C)?.ok_or_else(|| bad(C, "missing `point`"))?;
    let radius = typed(&w, p, "radius", Kind::Length, C)?.ok_or_else(|| bad(C, "missing `radius`"))?;
    let (arc, _, _) = w.sketch.fillet(corner, radius.value).map_err(|e| bad(C, e))?;
    let dimension = add_dim(&mut w, Kind::Length, &radius, |param| Constraint::Radius { e: arc, param });
    let id = w.id;
    commit(s, w, "Sketch Fillet", C)?;
    Ok(json!({"id": id.0, "arc": arc, "dimensions": [dimension]}))
}

/// Do the references of `c` exist and have the right kinds?
fn check_refs(sketch: &Sketch, c: &Constraint) -> std::result::Result<(), String> {
    let pt = |id: &u32| sketch.point(*id).is_some();
    let line = |id: &u32| matches!(sketch.entity(*id), Some(Entity::Line { .. }));
    let round = |id: &u32| matches!(sketch.entity(*id), Some(Entity::Circle { .. } | Entity::Arc { .. }));
    let ok = match c {
        Constraint::Coincident { a, b } | Constraint::Distance { a, b, .. } => pt(a) && pt(b) && a != b,
        Constraint::PointOnLine { p, line: l } | Constraint::Midpoint { p, line: l } => pt(p) && line(l),
        Constraint::PointOnCircle { p, e } => pt(p) && round(e),
        Constraint::Horizontal { line: l }
        | Constraint::Vertical { line: l }
        | Constraint::Length { line: l, .. }
        | Constraint::Angle { line: l, .. } => line(l),
        Constraint::Parallel { a, b } | Constraint::Perpendicular { a, b } | Constraint::AngleBetween { a, b, .. } => line(a) && line(b) && a != b,
        Constraint::Tangent { a, b } => a != b && ((line(a) && round(b)) || (round(a) && line(b)) || (round(a) && round(b))),
        Constraint::Equal { a, b } => a != b && ((line(a) && line(b)) || (round(a) && round(b))),
        Constraint::Concentric { a, b } => a != b && round(a) && round(b),
        Constraint::Fix { p, .. } => pt(p),
        Constraint::Radius { e, .. } | Constraint::Diameter { e, .. } => round(e),
        Constraint::DistancePointLine { p, line: l, .. } => pt(p) && line(l),
        Constraint::Symmetric { a, b, line: l } => pt(a) && pt(b) && a != b && line(l),
    };
    if ok {
        Ok(())
    } else {
        let name = serde_json::to_value(c).ok().and_then(|v| v.get("type").and_then(Value::as_str).map(str::to_string)).unwrap_or_default();
        Err(format!("a {name} constraint needs existing points or shapes of the right kind (points and shapes have separate ids)"))
    }
}

const GEOMETRIC: [&str; 13] = [
    "coincident",
    "pointOnLine",
    "pointOnCircle",
    "horizontal",
    "vertical",
    "parallel",
    "perpendicular",
    "tangent",
    "equal",
    "fix",
    "midpoint",
    "concentric",
    "symmetric",
];
const DIMENSIONS: [&str; 7] = ["distance", "length", "angle", "angleBetween", "radius", "diameter", "distancePointLine"];

/// The `constraint` object of `p`, its `type` among `allowed`.
fn constraint_object(p: &Value, allowed: &[&str], cmd: &str) -> Result<serde_json::Map<String, Value>> {
    let obj = p.get("constraint").and_then(Value::as_object).ok_or_else(|| bad(cmd, "missing `constraint`"))?;
    match obj.get("type").and_then(Value::as_str) {
        Some(t) if allowed.contains(&t) => Ok(obj.clone()),
        _ => Err(bad(cmd, format!("`constraint.type` must be one of {}", allowed.join(", ")))),
    }
}

fn add_constraint(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.addConstraint";
    let mut w = work(s, p, C)?;
    let mut obj = constraint_object(p, &GEOMETRIC, C)?;
    if obj.get("type").and_then(Value::as_str) == Some("fix") && !(obj.contains_key("x") && obj.contains_key("y")) {
        let point = obj.get("p").and_then(index_value).ok_or_else(|| bad(C, "`fix` needs a point `p`"))?;
        let at = w.sketch.point(point).ok_or_else(|| bad(C, format!("no point {point} in this sketch")))?;
        obj.insert("x".into(), json!(at.x));
        obj.insert("y".into(), json!(at.y));
    }
    let c: Constraint = serde_json::from_value(Value::Object(obj)).map_err(|e| bad(C, format!("bad constraint: {e}")))?;
    check_refs(&w.sketch, &c).map_err(|e| bad(C, e))?;
    check_caps(&w.sketch, C)?;
    let solved = params::solve(&w.rows);
    w.sketch = solve::try_add(&w.sketch, c, &solved).map_err(|e| solve_error(e, C))?.sketch;
    let (id, index) = (w.id, w.sketch.constraints.len() - 1);
    save(s, w, "Add Sketch Constraint")?;
    Ok(json!({"id": id.0, "index": index}))
}

fn add_dimension(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.addDimension";
    let mut w = work(s, p, C)?;
    let mut obj = constraint_object(p, &DIMENSIONS, C)?;
    let name = params::next_model_name(&w.rows);
    obj.insert("param".into(), json!(name));
    let mut c: Constraint = serde_json::from_value(Value::Object(obj)).map_err(|e| bad(C, format!("bad constraint: {e}")))?;
    check_refs(&w.sketch, &c).map_err(|e| bad(C, e))?;
    orient_distance(&w.sketch, &mut c);
    let at = xy(p, "at", C)?;
    let driven = p.get("driven").and_then(Value::as_bool).unwrap_or(false);
    let kind = if c.is_angular() { Kind::Angle } else { Kind::Length };
    let measured = w.sketch.measure(&c);
    let t = match typed(&w, p, "expression", kind, C)? {
        Some(t) if !driven => t,
        _ => {
            let v = measured.ok_or_else(|| bad(C, "cannot measure this yet"))?;
            if kind == Kind::Length && v <= 1e-9 {
                return Err(bad(C, "there is no length to measure here; give an `expression`"));
            }
            Typed { expression: params::plain_expression(v, kind.unit()), value: v }
        }
    };
    add_param(&mut w, kind, &t.expression);
    check_caps(&w.sketch, C)?;
    if let Some(at) = at {
        w.sketch.places.insert(name.clone(), [at.0, at.1]);
    }
    if driven {
        // A reference dimension: no equation, so nothing to refuse.
        w.sketch.driven.insert(name.clone());
        w.sketch.constraints.push(c);
    } else {
        let solved = params::solve(&w.rows);
        w.sketch = solve::try_add(&w.sketch, c, &solved).map_err(|e| solve_error(e, C))?.sketch;
    }
    let (id, index) = (w.id, w.sketch.constraints.len() - 1);
    save(s, w, "Add Sketch Dimension")?;
    Ok(json!({"id": id.0, "index": index, "name": name, "value": t.value, "driven": driven}))
}

/// A horizontal or vertical distance runs from `a` to `b` rightwards / downwards: swap the points
/// when they were picked the other way, so the value is positive.
fn orient_distance(sketch: &Sketch, c: &mut Constraint) {
    use sketch::model::Along;
    if let Constraint::Distance { a, b, along, .. } = c
        && let (Some(p), Some(q)) = (sketch.point(*a), sketch.point(*b))
    {
        let backwards = match along {
            Along::Direct => false,
            Along::X => q.x < p.x,
            Along::Y => q.y < p.y,
        };
        if backwards {
            std::mem::swap(a, b);
        }
    }
}

/// The dimension `name` or `index` of `p`: its index and parameter name.
fn find_dimension(w: &Work, p: &Value, cmd: &str) -> Result<(usize, String)> {
    let found = match (str_param(p, "name"), p.get("index").and_then(Value::as_u64)) {
        (Some(name), _) => w.sketch.constraints.iter().position(|c| c.param() == Some(name)),
        (None, Some(i)) => usize::try_from(i).ok().filter(|i| w.sketch.constraints.get(*i).is_some_and(Constraint::is_dimension)),
        (None, None) => return Err(bad(cmd, "give the dimension's `name` or `index`")),
    };
    let index = found.ok_or_else(|| bad(cmd, "no such dimension in this sketch"))?;
    let name = w.sketch.constraints[index].param().unwrap_or_default().to_string();
    Ok((index, name))
}

fn place_dimension(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.placeDimension";
    let mut w = work(s, p, C)?;
    let (_, name) = find_dimension(&w, p, C)?;
    let at = match p.get("at") {
        Some(Value::Null) => None,
        _ => Some(xy(p, "at", C)?.ok_or_else(|| bad(C, "missing `at`: [x, y] in mm, or null"))?),
    };
    match at {
        Some((x, y)) => {
            w.sketch.places.insert(name.clone(), [x, y]);
        }
        None => {
            w.sketch.places.remove(&name);
        }
    }
    let id = w.id;
    save(s, w, "Move Sketch Dimension")?;
    Ok(json!({"id": id.0, "name": name, "at": at.map(|(x, y)| json!([x, y]))}))
}

fn set_driven(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.setDriven";
    let mut w = work(s, p, C)?;
    let (index, name) = find_dimension(&w, p, C)?;
    let driven = p.get("driven").and_then(Value::as_bool).ok_or_else(|| bad(C, "missing `driven`: true or false"))?;
    let id = w.id;
    if driven == w.sketch.driven.contains(&name) {
        let value = params::solve(&w.rows).value(&name);
        return Ok(json!({"id": id.0, "name": name, "driven": driven, "value": value}));
    }
    if driven {
        w.sketch.driven.insert(name.clone());
        tidy(&mut w);
    } else {
        // Back to driving: the dimension's equation joins the sketch, which may over-constrain it.
        let c = w.sketch.constraints[index].clone();
        let mut without = w.sketch.clone();
        without.constraints.remove(index);
        without.driven.remove(&name);
        let solved = params::solve(&w.rows);
        let sol = solve::try_add(&without, c, &solved).map_err(|e| solve_error(e, C))?;
        let mut sketch = w.sketch.clone();
        sketch.driven.remove(&name);
        sketch.points = sol.sketch.points;
        sketch.entities = sol.sketch.entities;
        w.sketch = sketch;
    }
    save(s, w, if driven { "Make Sketch Dimension Driven" } else { "Make Sketch Dimension Driving" })?;
    let value = s.doc().ok().and_then(|d| params::solve(&params::rows(&d.doc)).value(&name));
    Ok(json!({"id": id.0, "name": name, "driven": driven, "value": value}))
}

fn set_dimension(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.setDimension";
    let mut w = work(s, p, C)?;
    let c = match (str_param(p, "name"), p.get("index").and_then(Value::as_u64)) {
        (Some(name), _) => w.sketch.constraints.iter().find(|c| c.param() == Some(name)),
        (None, Some(i)) => usize::try_from(i).ok().and_then(|i| w.sketch.constraints.get(i)),
        (None, None) => return Err(bad(C, "give the dimension's `name` or `index`")),
    };
    let (name, kind) = match c.and_then(|c| c.param().map(|n| (n.to_string(), c.is_angular()))) {
        Some((n, true)) => (n, Kind::Angle),
        Some((n, false)) => (n, Kind::Length),
        None => return Err(bad(C, "no such dimension in this sketch")),
    };
    if w.sketch.driven.contains(&name) {
        return Err(bad(C, format!("`{name}` is a driven dimension: it reads the geometry; sketch.setDriven {{driven: false}} makes it driving")));
    }
    let expression = typed(&w, p, "expression", kind, C)?.ok_or_else(|| bad(C, "missing `expression`"))?.expression;
    let before = w.rows.clone();
    let row = w
        .rows
        .iter_mut()
        .find(|r| r.name == name && r.sketch == Some(w.id.0))
        .ok_or_else(|| bad(C, format!("no parameter `{name}` for this sketch")))?;
    row.expression = expression;
    let solved = params::solve(&w.rows);
    let value = match solved.value(&name) {
        Some(v) => v,
        None => return Err(bad(C, solved.error(&name).unwrap_or("not a number"))),
    };
    if w.rows == before {
        return Ok(json!({"id": w.id.0, "name": name, "value": value, "error": null}));
    }
    solve_work(&mut w, C)?;
    let id = w.id;
    save(s, w, "Set Sketch Dimension")?;
    Ok(json!({"id": id.0, "name": name, "value": value, "error": null}))
}

/// Is model parameter `name` still used by another dimension of this sketch, by a dimension or a
/// shape field of another sketch, or by another row's expression?
fn param_in_use(doc: &Document, id: NodeId, current: &Sketch, rows: &[ParamRow], name: &str) -> bool {
    let uses = |sk: &Sketch| {
        sk.constraints.iter().any(|c| c.param() == Some(name))
            || sk.shapes.iter().any(|shape| shape.fields.values().any(|e| expr::references(e).iter().any(|r| r == name)))
    };
    if uses(current) || rows.iter().any(|r| r.name != name && expr::references(&r.expression).iter().any(|x| x == name)) {
        return true;
    }
    let mut used = false;
    doc.walk(|n| {
        if n.id != id
            && let Some(sk) = n.extras().sketch.as_deref()
            && uses(sk)
        {
            used = true;
        }
    });
    used
}

/// Remove the model parameters of dimensions that `before` had and the recipe no longer has,
/// unless something else still uses them.
fn prune_params(doc: &Document, w: &mut Work, before: &Sketch) {
    let dropped: Vec<String> = before
        .constraints
        .iter()
        .filter_map(Constraint::param)
        .filter(|n| !w.sketch.constraints.iter().any(|c| c.param() == Some(*n)))
        .map(str::to_string)
        .collect();
    for name in dropped {
        if !param_in_use(doc, w.id, &w.sketch, &w.rows, &name) {
            let id = w.id.0;
            w.rows.retain(|r| !(r.name == name && r.sketch == Some(id)));
        }
    }
}

/// Solve for the stored positions after something was taken away (this cannot over-constrain;
/// when the solver does not converge the recipe is kept as it is).
fn tidy(w: &mut Work) {
    if let Ok(sol) = solve::solve(&w.sketch, &params::solve(&w.rows)) {
        w.sketch = sol.sketch;
    }
}

fn remove_constraint(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.removeConstraint";
    let mut w = work(s, p, C)?;
    let before = w.sketch.clone();
    let i = p.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).ok_or_else(|| bad(C, "missing `index`"))?;
    if w.sketch.remove_constraint(i).is_none() {
        return Err(bad(C, format!("no constraint {i} in this sketch")));
    }
    prune_params(&s.doc()?.doc, &mut w, &before);
    tidy(&mut w);
    let id = w.id;
    save(s, w, "Remove Sketch Constraint")?;
    Ok(json!({"id": id.0, "removed": i}))
}

/// Entity ids under `key`, each of which must exist.
fn entity_ids(w: &Work, p: &Value, cmd: &str) -> Result<Vec<u32>> {
    let list = p.get("entities").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`entities` must be a list of shape ids"))?;
    let mut ids = Vec::new();
    for v in list {
        let id = index_value(v).ok_or_else(|| bad(cmd, "`entities` must be a list of shape ids"))?;
        if w.sketch.entity(id).is_none() {
            return Err(bad(cmd, format!("no shape {id} in this sketch")));
        }
        ids.push(id);
    }
    if ids.is_empty() {
        return Err(bad(cmd, "`entities` is empty"));
    }
    Ok(ids)
}

fn delete_entities(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.deleteEntities";
    let mut w = work(s, p, C)?;
    let ids = entity_ids(&w, p, C)?;
    let before = w.sketch.clone();
    w.sketch.remove_entities(&ids);
    prune_params(&s.doc()?.doc, &mut w, &before);
    tidy(&mut w);
    let id = w.id;
    save(s, w, "Delete Sketch Geometry")?;
    Ok(json!({"id": id.0}))
}

fn set_construction(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.setConstruction";
    let mut w = work(s, p, C)?;
    let ids = entity_ids(&w, p, C)?;
    let want = match p.get("construction") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad(C, "`construction` must be true or false"))?),
    };
    for e in &mut w.sketch.entities {
        if ids.contains(&e.id()) {
            let on = want.unwrap_or(!e.construction());
            match e {
                Entity::Line { construction, .. } | Entity::Circle { construction, .. } | Entity::Arc { construction, .. } => *construction = on,
            }
        }
    }
    let id = w.id;
    save(s, w, "Toggle Construction")?;
    Ok(json!({"id": id.0}))
}

/// A pair of numbers `[x, y]` under `key`, required.
fn required_xy(p: &Value, key: &str, cmd: &str) -> Result<(f64, f64)> {
    xy(p, key, cmd)?.ok_or_else(|| bad(cmd, format!("missing `{key}`")))
}

/// One line id under `key`.
fn line_ref(w: &Work, p: &Value, key: &str, cmd: &str) -> Result<u32> {
    let id = p.get(key).and_then(index_value).ok_or_else(|| bad(cmd, format!("`{key}` must be a line id")))?;
    match w.sketch.entity(id) {
        Some(Entity::Line { .. }) => Ok(id),
        Some(_) => Err(bad(cmd, format!("`{key}` must be a line"))),
        None => Err(bad(cmd, format!("no shape {id} in this sketch"))),
    }
}

fn trim(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.trim";
    let mut w = work(s, p, C)?;
    let at = required_xy(p, "at", C)?;
    let entity = p.get("entity").and_then(index_value).ok_or_else(|| bad(C, "`entity` must be a shape id"))?;
    if w.sketch.entity(entity).is_none() {
        return Err(bad(C, format!("no shape {entity} in this sketch")));
    }
    let before = w.sketch.clone();
    let plan = w.sketch.trim_plan(entity, at).map_err(|e| bad(C, e))?;
    let kept = w.sketch.trim_line(entity, at).map_err(|e| bad(C, e))?;
    prune_params(&s.doc()?.doc, &mut w, &before);
    let id = w.id;
    commit(s, w, "Sketch Trim", C)?;
    Ok(json!({"id": id.0, "kept": kept, "removed": [[plan.from.0, plan.from.1], [plan.to.0, plan.to.1]]}))
}

fn offset(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.offset";
    let mut w = work(s, p, C)?;
    let ids = entity_ids(&w, p, C)?;
    let distance = typed(&w, p, "distance", Kind::Length, C)?.ok_or_else(|| bad(C, "missing `distance`"))?;
    let side = match p.get("side") {
        None | Some(Value::Null) => 1.0,
        Some(v) => match v.as_f64() {
            Some(n) if n == 1.0 || n == -1.0 => n,
            _ => return Err(bad(C, "`side` must be 1 or -1")),
        },
    };
    let chain = w.sketch.chain_of(&ids).map_err(|e| bad(C, e))?;
    let places = w.sketch.offset_positions(&chain, distance.value, side).map_err(|e| bad(C, e))?;
    let points: Vec<u32> = places.iter().map(|q| w.sketch.add_point(q.0, q.1)).collect();
    let mut lines = Vec::new();
    for (i, (source, ..)) in chain.lines.iter().enumerate() {
        let a = points.get(i).copied().ok_or_else(|| bad(C, "the offset lost a point"))?;
        let b =
            points.get(if chain.closed { (i + 1) % chain.lines.len() } else { i + 1 }).copied().ok_or_else(|| bad(C, "the offset lost a point"))?;
        let copy = w.sketch.add_line(a, b, false);
        w.sketch.constraints.push(Constraint::Parallel { a: *source, b: copy });
        lines.push(copy);
    }
    // The distance is a dimension from the first line given to its copy.
    let first = ids.first().copied().unwrap_or(0);
    let k = chain.lines.iter().position(|(l, ..)| *l == first).ok_or_else(|| bad(C, "the first line is not in the chain"))?;
    let from = points.get(k).copied().ok_or_else(|| bad(C, "the offset lost a point"))?;
    let dimension = add_dim(&mut w, Kind::Length, &distance, |param| Constraint::DistancePointLine { p: from, line: first, param });
    let id = w.id;
    commit(s, w, "Sketch Offset", C)?;
    Ok(json!({"id": id.0, "lines": lines, "points": points, "dimensions": [dimension]}))
}

fn mirror(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "sketch.mirror";
    let mut w = work(s, p, C)?;
    let ids = entity_ids(&w, p, C)?;
    let axis = line_ref(&w, p, "line", C)?;
    if ids.contains(&axis) {
        return Err(bad(C, "the mirror line cannot be one of the shapes to mirror"));
    }
    let Some(Entity::Line { a: la, b: lb, .. }) = w.sketch.entity(axis).cloned() else { return Err(bad(C, "`line` must be a line")) };
    let (p0, p1) = (position(&w, la), position(&w, lb));
    let (d, len) = ((p1.0 - p0.0, p1.1 - p0.1), (p1.0 - p0.0).hypot(p1.1 - p0.1));
    if len < 1e-9 {
        return Err(bad(C, "the mirror line has no length"));
    }
    let u = (d.0 / len, d.1 / len);
    let reflect = |q: (f64, f64)| {
        let v = (q.0 - p0.0, q.1 - p0.1);
        let along = v.0 * u.0 + v.1 * u.1;
        (p0.0 + 2.0 * along * u.0 - v.0, p0.1 + 2.0 * along * u.1 - v.1)
    };
    let mut map: std::collections::BTreeMap<u32, u32> = Default::default();
    let mut points = Vec::new();
    let mut mirrored = |w: &mut Work, point: u32| -> u32 {
        if let Some(m) = map.get(&point) {
            return *m;
        }
        let q = position(w, point);
        let r = reflect(q);
        // On the line, a point is its own image.
        let made = if (r.0 - q.0).hypot(r.1 - q.1) < 1e-6 {
            point
        } else {
            let n = w.sketch.add_point(r.0, r.1);
            w.sketch.constraints.push(Constraint::Symmetric { a: point, b: n, line: axis });
            points.push(n);
            n
        };
        map.insert(point, made);
        made
    };
    let mut made = Vec::new();
    for id in &ids {
        let Some(e) = w.sketch.entity(*id).cloned() else { continue };
        let new = match e {
            Entity::Line { a, b, construction, .. } => {
                let (a, b) = (mirrored(&mut w, a), mirrored(&mut w, b));
                if a == b {
                    return Err(bad(C, "a line that lies on the mirror line cannot be mirrored"));
                }
                w.sketch.add_line(a, b, construction)
            }
            Entity::Circle { id: src, c, r, construction } => {
                let c = mirrored(&mut w, c);
                let n = w.sketch.add_circle(c, r);
                set_construction_of(&mut w, n, construction);
                w.sketch.constraints.push(Constraint::Equal { a: src, b: n });
                n
            }
            Entity::Arc { c, a, b, construction, .. } => {
                let (c, a, b) = (mirrored(&mut w, c), mirrored(&mut w, a), mirrored(&mut w, b));
                // A mirror image runs the other way round.
                let n = w.sketch.add_arc(c, b, a);
                set_construction_of(&mut w, n, construction);
                n
            }
        };
        made.push(new);
    }
    let id = w.id;
    commit(s, w, "Sketch Mirror", C)?;
    Ok(json!({"id": id.0, "entities": made, "points": points}))
}

fn set_construction_of(w: &mut Work, entity: u32, on: bool) {
    for e in &mut w.sketch.entities {
        if e.id() == entity {
            match e {
                Entity::Line { construction, .. } | Entity::Circle { construction, .. } | Entity::Arc { construction, .. } => *construction = on,
            }
        }
    }
}

/// The extra fields `sketch.inspect` reports for a drawn sketch.
fn inspect_v2(sketch: &Sketch, rows: &[ParamRow], params_solved: &expr::Solved) -> serde_json::Map<String, Value> {
    let solution = solve::solve(sketch, params_solved);
    let (entities_ok, points_ok, errors, dof, solve_error) = match &solution {
        Ok(s) => (s.constrained.clone(), s.point_constrained.clone(), s.errors.clone(), Some(s.dof), None),
        Err(SolveError::OverConstrained) => (Default::default(), Default::default(), Default::default(), None, Some(CONTRADICTION)),
        Err(SolveError::NoSolution) => (Default::default(), Default::default(), Default::default(), None, Some(NO_SOLUTION)),
    };
    let points: Vec<Value> =
        sketch.points.iter().map(|q| json!({"id": q.id, "x": q.x, "y": q.y, "constrained": points_ok.contains(&q.id)})).collect();
    let entities: Vec<Value> = sketch
        .entities
        .iter()
        .map(|e| {
            let constrained = entities_ok.contains(&e.id());
            match e {
                Entity::Line { id, a, b, construction } => {
                    json!({"id": id, "kind": "line", "a": a, "b": b, "construction": construction, "constrained": constrained})
                }
                Entity::Circle { id, c, r, construction } => {
                    json!({"id": id, "kind": "circle", "c": c, "r": r, "construction": construction, "constrained": constrained})
                }
                Entity::Arc { id, c, a, b, construction } => {
                    let r = sketch.point(*c).zip(sketch.point(*a)).map(|(c, a)| (a.x - c.x).hypot(a.y - c.y));
                    json!({"id": id, "kind": "arc", "c": c, "a": a, "b": b, "r": r, "construction": construction, "constrained": constrained})
                }
            }
        })
        .collect();
    let constraints: Vec<Value> = sketch
        .constraints
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("index".into(), json!(i));
                if let Some(param) = c.param() {
                    o.insert("driven".into(), json!(sketch.driven.contains(param)));
                    o.insert("at".into(), json!(sketch.places.get(param)));
                    let expression = rows.iter().find(|r| r.name == param).map(|r| r.expression.clone());
                    let error = errors.get(&i).cloned().or_else(|| params_solved.error(param).map(str::to_string));
                    o.insert("expression".into(), json!(expression));
                    o.insert("value".into(), json!(params_solved.value(param)));
                    o.insert("error".into(), json!(error));
                }
            }
            v
        })
        .collect();
    let mut out = serde_json::Map::new();
    out.insert("points".into(), json!(points));
    out.insert("entities".into(), json!(entities));
    out.insert("constraints".into(), json!(constraints));
    out.insert("dof".into(), json!(dof));
    out.insert("fullyConstrained".into(), json!(dof == Some(0)));
    out.insert("solveError".into(), json!(solve_error));
    out
}
