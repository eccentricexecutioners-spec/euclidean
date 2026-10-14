//! Parametric sketches (Euclidean): ordinary groups whose paths are built from a recipe in
//! `NodeExtras::sketch` (`vectorcraft_doc::sketch`) over the document's user parameters
//! (`cmd::parameters`). [`refresh`] runs inside every `Session::edit`, so a parameter change, a
//! recipe edit and the sketch's rebuilt paths share one undo step; everything else in the app
//! sees plain paths in a plain group.
use super::*;
use serde_json::{Value, json};
use std::sync::Arc;
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::sketch::{self, Sketch, SketchShape};
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
            query "sketch.inspect",
            "Inspect Parametric Sketch",
            [],
            None,
            "{id? (default: the selected sketch)} → {id, origin: [x, y] (mm), shapes: [{id, kind, fields: {field: {expression, value, error}}}]}",
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
    Ok(json!({"id": id.0, "origin": sketch.origin, "shapes": shapes}))
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
