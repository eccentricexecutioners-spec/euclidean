//! Euclidean: drawn (v2) parametric sketches through the commands every surface uses.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use vectorcraft_doc::{Node, NodeId, params};
use vectorcraft_engine::Session;

const MM: f64 = 72.0 / 25.4;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 600, "height": 400})).unwrap();
    s
}

fn new_sketch(s: &mut Session) -> NodeId {
    NodeId(s.execute("sketch.new", &json!({})).unwrap()["id"].as_u64().unwrap())
}

fn undo_len(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

fn bounds(s: &Session, id: NodeId) -> vectorcraft_geom::Rect {
    s.doc().unwrap().doc.node(id).unwrap().geometric_bounds().unwrap()
}

fn child_names(s: &Session, id: NodeId) -> Vec<String> {
    s.doc().unwrap().doc.node(id).and_then(Node::children).unwrap().iter().map(|c| c.name.clone().unwrap_or_default()).collect()
}

fn inspect(s: &mut Session) -> Value {
    s.execute("sketch.inspect", &json!({})).unwrap()
}

fn point(v: &Value, id: u64) -> (f64, f64) {
    let p = v["points"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap();
    (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap())
}

/// A 50 × 30 rectangle from the origin with its first corner fixed: (id, rect result).
fn rectangle(s: &mut Session) -> (NodeId, Value) {
    let id = new_sketch(s);
    let r = s.execute("sketch.addRect", &json!({"from": [0, 0], "to": [10, 10], "width": 50, "height": 30})).unwrap();
    let first = r["points"][0].as_u64().unwrap();
    s.execute("sketch.addConstraint", &json!({"constraint": {"type": "fix", "p": first}})).unwrap();
    (id, r)
}

#[test]
fn a_chain_of_three_typed_lines_makes_d_parameters_one_undo_step_each() {
    let mut s = session();
    let id = new_sketch(&mut s);
    let n = undo_len(&s);
    let l1 = s.execute("sketch.addLine", &json!({"fromPoint": 0, "to": [30, 0], "length": 50, "angle": 0})).unwrap();
    assert_eq!(undo_len(&s), n + 1);
    assert_eq!(l1["dimensions"], json!([{"name": "d1", "index": 1}, {"name": "d2", "index": 2}]));
    let (p1, p2) = (l1["a"].as_u64().unwrap(), l1["b"].as_u64().unwrap());
    assert_eq!(p1, 0);
    let l2 = s.execute("sketch.addLine", &json!({"fromPoint": p2, "to": [60, -10], "length": 40, "angle": 90})).unwrap();
    assert_eq!(undo_len(&s), n + 2);
    let p3 = l2["b"].as_u64().unwrap();
    s.execute("sketch.addLine", &json!({"fromPoint": p3, "toPoint": p1})).unwrap();
    assert_eq!(undo_len(&s), n + 3);

    let rows = params::rows(&s.doc().unwrap().doc);
    let got: Vec<(&str, &str, &str, Option<u64>)> =
        rows.iter().map(|r| (r.name.as_str(), r.unit.as_str(), r.expression.as_str(), r.sketch)).collect();
    assert_eq!(
        got,
        vec![("d1", "mm", "50 mm", Some(id.0)), ("d2", "deg", "0", Some(id.0)), ("d3", "mm", "40 mm", Some(id.0)), ("d4", "deg", "90", Some(id.0)),]
    );
    assert_eq!(child_names(&s, id), ["Profile 1"]);
    let v = inspect(&mut s);
    let (x, y) = point(&v, p3);
    assert!((x - 50.0).abs() < 1e-6 && (y + 40.0).abs() < 1e-6, "{x} {y}");
    let b = bounds(&s, id);
    assert!((b.width() - 50.0 * MM).abs() < 1e-6 && (b.height() - 40.0 * MM).abs() < 1e-6, "{b:?}");
}

#[test]
fn set_dimension_d1_moves_the_connected_geometry_in_one_undo_step() {
    let mut s = session();
    let (id, r) = rectangle(&mut s);
    assert_eq!(r["dimensions"][0]["name"], "d1");
    let b = bounds(&s, id);
    assert!((b.width() - 50.0 * MM).abs() < 1e-6 && (b.height() - 30.0 * MM).abs() < 1e-6, "{b:?}");
    assert!(inspect(&mut s)["fullyConstrained"].as_bool().unwrap());
    let before = inspect(&mut s);
    let right = r["points"][1].as_u64().unwrap();
    let n = undo_len(&s);
    let out = s.execute("sketch.setDimension", &json!({"name": "d1", "expression": "80"})).unwrap();
    assert_eq!((out["name"].as_str(), out["value"].as_f64()), (Some("d1"), Some(80.0)));
    assert_eq!(undo_len(&s), n + 1);
    let after = inspect(&mut s);
    assert!((point(&after, right).0 - point(&before, right).0 - 30.0).abs() < 1e-6);
    assert!((point(&after, right).1 - point(&before, right).1).abs() < 1e-6);
    assert_eq!(child_names(&s, id), ["Profile 1"]);
    let b = bounds(&s, id);
    assert!((b.width() - 80.0 * MM).abs() < 1e-6 && (b.height() - 30.0 * MM).abs() < 1e-6, "{b:?}");
    // Top and bottom are still horizontal.
    let line_y = |v: &Value, i: u64| {
        let l = v["entities"].as_array().unwrap().iter().find(|e| e["id"] == i).unwrap();
        (point(v, l["a"].as_u64().unwrap()).1, point(v, l["b"].as_u64().unwrap()).1)
    };
    for i in [r["lines"][0].as_u64().unwrap(), r["lines"][2].as_u64().unwrap()] {
        let (a, b) = line_y(&after, i);
        assert!((a - b).abs() < 1e-9);
    }
    // The same value again is not an undo step.
    s.execute("sketch.setDimension", &json!({"name": "d1", "expression": "80"})).unwrap();
    assert_eq!(undo_len(&s), n + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!((bounds(&s, id).width() - 50.0 * MM).abs() < 1e-6);
    assert_eq!(params::rows(&s.doc().unwrap().doc)[0].expression, "50 mm");
    // A value that does not evaluate or does not solve changes nothing.
    let n = undo_len(&s);
    assert!(s.execute("sketch.setDimension", &json!({"name": "d1", "expression": "oops +"})).is_err());
    assert!(s.execute("sketch.setDimension", &json!({"name": "d9", "expression": "5"})).is_err());
    assert_eq!(undo_len(&s), n);
}

#[test]
fn an_over_constraining_dimension_is_refused_and_changes_nothing() {
    let mut s = session();
    let (_, r) = rectangle(&mut s);
    let before = inspect(&mut s);
    let rows = params::rows(&s.doc().unwrap().doc);
    let n = undo_len(&s);
    let bottom = r["lines"][2].as_u64().unwrap();
    let e = s.execute("sketch.addDimension", &json!({"constraint": {"type": "length", "line": bottom}})).unwrap_err();
    assert!(e.to_string().contains("This would over-constrain the sketch"), "{e}");
    let e = s.execute("sketch.addDimension", &json!({"constraint": {"type": "length", "line": bottom}, "expression": "70"})).unwrap_err();
    assert!(e.to_string().contains("This would over-constrain the sketch"), "{e}");
    assert_eq!(undo_len(&s), n);
    assert_eq!(inspect(&mut s), before);
    assert_eq!(params::rows(&s.doc().unwrap().doc), rows);
}

#[test]
fn save_and_load_keep_the_v2_recipe_and_the_model_parameters() {
    let mut s = session();
    let (id, _) = rectangle(&mut s);
    let want = inspect(&mut s);
    let doc = s.doc().unwrap().doc.clone();
    let bytes = vectorcraft_format::save_file(&doc);
    let back = vectorcraft_format::load_file(&bytes).unwrap().doc;
    assert_eq!(params::rows(&back), params::rows(&doc));
    let mut s2 = Session::new();
    s2.add_document(back, None);
    assert_eq!(s2.execute("sketch.inspect", &json!({"id": id.0})).unwrap(), want);
    s2.execute("sketch.setDimension", &json!({"id": id.0, "name": "d2", "expression": "45"})).unwrap();
    let b = bounds(&s2, id);
    assert!((b.height() - 45.0 * MM).abs() < 1e-6 && (b.width() - 50.0 * MM).abs() < 1e-6, "{b:?}");
}

#[test]
fn renaming_d1_rewrites_the_dimension() {
    let mut s = session();
    let (id, _) = rectangle(&mut s);
    let before = bounds(&s, id);
    s.execute("parameters.rename", &json!({"from": "d1", "to": "side"})).unwrap();
    let v = inspect(&mut s);
    let dim = v["constraints"].as_array().unwrap().iter().find(|c| c["param"] == "side").unwrap();
    assert_eq!((dim["type"].as_str(), dim["value"].as_f64()), (Some("length"), Some(50.0)));
    assert!(v["constraints"].as_array().unwrap().iter().all(|c| c["param"] != "d1"));
    assert_eq!(bounds(&s, id), before);
    let list = s.execute("parameters.list", &json!({})).unwrap();
    assert_eq!(list["rows"][0]["name"], "side");
    assert_eq!(list["rows"][0]["sketch"], id.0);
    s.execute("sketch.setDimension", &json!({"name": "side", "expression": "60"})).unwrap();
    assert!((bounds(&s, id).width() - 60.0 * MM).abs() < 1e-6);
}

#[test]
fn fillet_adds_a_tangent_arc_with_a_radius_dimension() {
    let mut s = session();
    let (id, r) = rectangle(&mut s);
    let corner = r["points"][1].as_u64().unwrap();
    let n = undo_len(&s);
    let f = s.execute("sketch.fillet", &json!({"point": corner, "radius": 5})).unwrap();
    assert_eq!(undo_len(&s), n + 1);
    let arc = f["arc"].as_u64().unwrap();
    let v = inspect(&mut s);
    let e = v["entities"].as_array().unwrap().iter().find(|e| e["id"] == arc).unwrap();
    assert_eq!(e["kind"], "arc");
    assert!((e["r"].as_f64().unwrap() - 5.0).abs() < 1e-6);
    let dim = v["constraints"].as_array().unwrap().iter().find(|c| c["type"] == "radius").unwrap();
    assert_eq!(dim["param"], f["dimensions"][0]["name"]);
    assert_eq!(dim["value"].as_f64(), Some(5.0));
    assert_eq!(child_names(&s, id), ["Profile 1"]);
    // A radius too large for the lines is refused.
    assert!(s.execute("sketch.fillet", &json!({"point": r["points"][2], "radius": 500})).is_err());
    assert_eq!(undo_len(&s), n + 1);
}

#[test]
fn circle_and_arc_with_typed_values() {
    let mut s = session();
    let id = new_sketch(&mut s);
    let c = s.execute("sketch.addCircle", &json!({"center": [40, 40], "diameter": 20})).unwrap();
    assert_eq!(c["dimensions"][0]["name"], "d1");
    let v = inspect(&mut s);
    let circle = v["entities"].as_array().unwrap().iter().find(|e| e["id"] == c["circle"]).unwrap();
    assert!((circle["r"].as_f64().unwrap() - 10.0).abs() < 1e-6);
    assert_eq!(v["constraints"][1]["type"], "diameter");
    assert_eq!(v["constraints"][1]["value"].as_f64(), Some(20.0));
    let a = s.execute("sketch.addArc", &json!({"start": [0, 0], "end": [40, 0], "through": [20, -15], "radius": 25})).unwrap();
    assert_eq!(a["dimensions"][0]["name"], "d2");
    let v = inspect(&mut s);
    let arc = v["entities"].as_array().unwrap().iter().find(|e| e["id"] == a["arc"]).unwrap();
    assert!((arc["r"].as_f64().unwrap() - 25.0).abs() < 1e-6, "{arc}");
    // The arc bulges toward `through` (up the page: negative y at its middle).
    let (cx, cy) = point(&v, arc["c"].as_u64().unwrap());
    assert!((cx - 20.0).abs() < 1e-6 && cy > 0.0, "{cx} {cy}");
    assert_eq!(child_names(&s, id), ["Chain 1", "Circle 1"]);
    // Without a radius, the three-point circle is used.
    let b = s.execute("sketch.addArc", &json!({"start": [100, 0], "end": [100, 20], "through": [110, 10]})).unwrap();
    let v = inspect(&mut s);
    let arc = v["entities"].as_array().unwrap().iter().find(|e| e["id"] == b["arc"]).unwrap();
    assert!((arc["r"].as_f64().unwrap() - 10.0).abs() < 1e-6);
    // Too small a radius is refused.
    let n = undo_len(&s);
    assert!(s.execute("sketch.addArc", &json!({"start": [0, 0], "end": [40, 0], "through": [20, -15], "radius": 10})).is_err());
    assert!(s.execute("sketch.addCircle", &json!({"center": [0, 0], "diameter": "nope"})).is_err());
    assert_eq!(undo_len(&s), n);
}

#[test]
fn constraints_apply_and_invalid_refs_are_refused() {
    let mut s = session();
    new_sketch(&mut s);
    let a = s.execute("sketch.addLine", &json!({"from": [0, 0], "to": [40, 5]})).unwrap();
    let b = s.execute("sketch.addLine", &json!({"from": [10, 30], "to": [20, 60]})).unwrap();
    assert!(a["dimensions"].as_array().unwrap().is_empty());
    let n = undo_len(&s);
    let r = s.execute("sketch.addConstraint", &json!({"constraint": {"type": "perpendicular", "a": a["line"], "b": b["line"]}})).unwrap();
    assert_eq!(undo_len(&s), n + 1);
    let index = r["index"].as_u64().unwrap();
    // Perpendicular lines: the dot product of their directions is zero.
    let v = inspect(&mut s);
    let dir = |l: &Value| {
        let (p, q) = (point(&v, l["a"].as_u64().unwrap()), point(&v, l["b"].as_u64().unwrap()));
        (q.0 - p.0, q.1 - p.1)
    };
    let (d1, d2) = (dir(&a), dir(&b));
    assert!((d1.0 * d2.0 + d1.1 * d2.1).abs() < 1e-6);
    // Refused: a point id where a line is needed, an unknown id, a repeat, a dimension type.
    let bad_point = a["a"].as_u64().unwrap() + 100;
    for c in [
        json!({"type": "parallel", "a": a["line"], "b": bad_point}),
        json!({"type": "horizontal", "line": 99}),
        json!({"type": "coincident", "a": 1, "b": 1}),
        json!({"type": "perpendicular", "a": a["line"], "b": b["line"]}),
        json!({"type": "length", "line": a["line"], "param": "d1"}),
        json!({"type": "wobble"}),
    ] {
        assert!(s.execute("sketch.addConstraint", &json!({"constraint": c})).is_err(), "{c}");
    }
    assert_eq!(undo_len(&s), n + 1);
    s.execute("sketch.removeConstraint", &json!({"index": index})).unwrap();
    assert_eq!(undo_len(&s), n + 2);
    assert_eq!(inspect(&mut s)["constraints"].as_array().unwrap().len(), 1);
    assert!(s.execute("sketch.removeConstraint", &json!({"index": 40})).is_err());
    assert_eq!(undo_len(&s), n + 2);
}

#[test]
fn removing_a_dimension_or_its_geometry_removes_its_parameter() {
    let mut s = session();
    let (id, r) = rectangle(&mut s);
    // d1 is the width: removing it frees the width and drops the row.
    let index = inspect(&mut s)["constraints"].as_array().unwrap().iter().position(|c| c["param"] == "d1").unwrap();
    s.execute("sketch.removeConstraint", &json!({"index": index})).unwrap();
    let names: Vec<String> = params::rows(&s.doc().unwrap().doc).into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["d2"]);
    // Deleting the left line drops d2 (the height) with it.
    s.execute("sketch.deleteEntities", &json!({"entities": [r["lines"][3]]})).unwrap();
    assert!(params::rows(&s.doc().unwrap().doc).is_empty());
    assert_eq!(child_names(&s, id), ["Chain 1"]);
    // Construction geometry is drawn but not output.
    s.execute("sketch.setConstruction", &json!({"entities": [r["lines"][0], r["lines"][1]]})).unwrap();
    assert_eq!(child_names(&s, id), ["Chain 1"]);
    s.execute("sketch.setConstruction", &json!({"entities": [r["lines"][2]], "construction": true})).unwrap();
    assert!(child_names(&s, id).is_empty());
    let v = inspect(&mut s);
    assert_eq!(v["entities"][0]["construction"], true);
}

#[test]
fn a_center_rectangle_and_a_user_parameter_dimension() {
    let mut s = session();
    let id = new_sketch(&mut s);
    s.execute("parameters.add", &json!({"name": "w", "expression": "60 mm"})).unwrap();
    let r = s.execute("sketch.addRect", &json!({"mode": "center", "from": [100, 50], "to": [110, 60], "width": "w", "height": "w / 2"})).unwrap();
    assert_eq!(r["points"].as_array().unwrap().len(), 5);
    let b = bounds(&s, id);
    assert!((b.width() - 60.0 * MM).abs() < 1e-6 && (b.height() - 30.0 * MM).abs() < 1e-6, "{b:?}");
    // The dimensions follow the user parameter.
    s.execute("parameters.set", &json!({"name": "w", "expression": "80 mm"})).unwrap();
    let b = bounds(&s, id);
    assert!((b.width() - 80.0 * MM).abs() < 1e-6 && (b.height() - 40.0 * MM).abs() < 1e-6, "{b:?}");
    // The centre stays at the middle of the diagonal.
    let v = inspect(&mut s);
    let centre = point(&v, r["points"][4].as_u64().unwrap());
    assert!((centre.0 - 100.0).abs() < 1e-6 && (centre.1 - 50.0).abs() < 1e-6, "{centre:?}");
    // Removing a dimension's parameter is allowed; the dimension then reports the missing name.
    s.execute("parameters.remove", &json!({"name": "d1"})).unwrap();
    let v = inspect(&mut s);
    let dim = v["constraints"].as_array().unwrap().iter().find(|c| c["type"] == "length").unwrap();
    assert!(dim["error"].as_str().unwrap().contains("no parameter"), "{dim}");
}

#[test]
fn b2_sketches_still_build() {
    let mut s = session();
    let id = NodeId(s.execute("sketch.sample", &json!({})).unwrap()["id"].as_u64().unwrap());
    s.execute("parameters.set", &json!({"name": "width", "expression": "200 mm"})).unwrap();
    assert!((bounds(&s, id).width() - 200.0 * MM).abs() < 1e-6);
    // The drawn-sketch commands refuse a shape sketch.
    assert!(s.execute("sketch.addLine", &json!({"from": [0, 0], "to": [10, 0]})).is_err());
    let list = s.execute("parameters.list", &json!({})).unwrap();
    assert!(list["rows"].as_array().unwrap().iter().all(|r| r["sketch"].is_null()));
}

fn constraints_of(v: &Value, kind: &str) -> usize {
    v["constraints"].as_array().unwrap().iter().filter(|c| c["type"] == kind).count()
}

#[test]
fn trim_removes_the_clicked_piece_between_two_crossings() {
    let mut s = session();
    let (_, r) = rectangle(&mut s);
    // A horizontal 70 mm line through the rectangle's middle, from x = -10 to 60.
    let l = s.execute("sketch.addLine", &json!({"from": [-10, 15], "to": [60, 15], "length": 70, "horizontal": true})).unwrap();
    let line = l["line"].as_u64().unwrap();
    assert_eq!(params::rows(&s.doc().unwrap().doc).iter().filter(|p| p.name == "d3").count(), 1);
    let n = undo_len(&s);
    let t = s.execute("sketch.trim", &json!({"entity": line, "at": [25, 15]})).unwrap();
    assert_eq!(undo_len(&s), n + 1);
    assert_eq!(t["removed"], json!([[0.0, 15.0], [50.0, 15.0]]));
    assert_eq!(t["kept"].as_array().unwrap().len(), 2);
    let v = inspect(&mut s);
    // Two pieces: one from the old start to the left side, one from the right side to the old end.
    let lines: Vec<&Value> = v["entities"].as_array().unwrap().iter().filter(|e| t["kept"].as_array().unwrap().contains(&e["id"])).collect();
    let mut spans: Vec<(f64, f64)> = lines
        .iter()
        .map(|e| {
            let (a, b) = (point(&v, e["a"].as_u64().unwrap()), point(&v, e["b"].as_u64().unwrap()));
            (a.0.min(b.0), a.0.max(b.0))
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!((spans[0].0 + 10.0).abs() < 1e-6 && spans[0].1.abs() < 1e-6, "{spans:?}");
    assert!((spans[1].0 - 50.0).abs() < 1e-6 && (spans[1].1 - 60.0).abs() < 1e-6, "{spans:?}");
    // The new cut points lie on the rectangle's left and right sides; both pieces stay horizontal.
    let on: Vec<u64> =
        v["constraints"].as_array().unwrap().iter().filter(|c| c["type"] == "pointOnLine").map(|c| c["line"].as_u64().unwrap()).collect();
    assert_eq!(on.len(), 2, "{on:?}");
    assert!(on.contains(&r["lines"][1].as_u64().unwrap()) && on.contains(&r["lines"][3].as_u64().unwrap()), "{on:?}");
    assert_eq!(constraints_of(&v, "horizontal"), 2 + 2);
    // The length dimension went with its parameter.
    let names: Vec<String> = params::rows(&s.doc().unwrap().doc).into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["d1", "d2"]);
    // Undo is one step.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(inspect(&mut s)["entities"].as_array().unwrap().len(), 5);

    // A line nothing crosses goes whole; an arc or circle is refused with no change.
    let far = s.execute("sketch.addLine", &json!({"from": [0, 100], "to": [30, 100]})).unwrap()["line"].as_u64().unwrap();
    let circle = s.execute("sketch.addCircle", &json!({"center": [200, 200], "diameter": 20})).unwrap()["circle"].as_u64().unwrap();
    let n = undo_len(&s);
    let err = s.execute("sketch.trim", &json!({"entity": circle, "at": [210, 200]})).unwrap_err().to_string();
    assert!(err.contains("lines"), "{err}");
    assert_eq!(undo_len(&s), n);
    s.execute("sketch.trim", &json!({"entity": far, "at": [10, 100]})).unwrap();
    assert!(!inspect(&mut s)["entities"].as_array().unwrap().iter().any(|e| e["id"] == far));
}

#[test]
fn offset_a_rectangle_makes_a_parallel_loop_with_a_distance_parameter() {
    let mut s = session();
    let (_, r) = rectangle(&mut s);
    let lines = r["lines"].clone();
    let n = undo_len(&s);
    // Side -1 is to the right of the first line's direction: into the rectangle.
    let o = s.execute("sketch.offset", &json!({"entities": lines, "distance": 5, "side": -1})).unwrap();
    assert_eq!(undo_len(&s), n + 1);
    assert_eq!(o["lines"].as_array().unwrap().len(), 4);
    assert_eq!(o["dimensions"], json!([{"name": "d3", "index": o["dimensions"][0]["index"]}]));
    let v = inspect(&mut s);
    let pts: Vec<(f64, f64)> = o["points"].as_array().unwrap().iter().map(|p| point(&v, p.as_u64().unwrap())).collect();
    let want = [(5.0, 5.0), (45.0, 5.0), (45.0, 25.0), (5.0, 25.0)];
    for (p, w) in pts.iter().zip(want) {
        assert!((p.0 - w.0).abs() < 1e-6 && (p.1 - w.1).abs() < 1e-6, "{pts:?}");
    }
    assert_eq!(constraints_of(&v, "parallel"), 4);
    let dim = v["constraints"].as_array().unwrap().iter().find(|c| c["type"] == "distancePointLine").unwrap().clone();
    assert_eq!((dim["param"].as_str(), dim["line"].as_u64()), (Some("d3"), lines[0].as_u64()));
    let row = params::rows(&s.doc().unwrap().doc).into_iter().find(|p| p.name == "d3").unwrap();
    assert_eq!((row.unit.as_str(), row.expression.as_str()), ("mm", "5 mm"));
    // Changing the parameter moves the whole copy.
    s.execute("sketch.setDimension", &json!({"name": "d3", "expression": "8"})).unwrap();
    let v = inspect(&mut s);
    let p0 = point(&v, o["points"][0].as_u64().unwrap());
    assert!((p0.1 - 8.0).abs() < 1e-6, "{p0:?}");
    // Refused with no change: not a chain, no distance, a circle.
    let n = undo_len(&s);
    assert!(s.execute("sketch.offset", &json!({"entities": [lines[0], lines[2]], "distance": 5})).is_err());
    assert!(s.execute("sketch.offset", &json!({"entities": [lines[0]]})).is_err());
    assert!(s.execute("sketch.offset", &json!({"entities": [lines[0]], "distance": 0})).is_err());
    assert_eq!(undo_len(&s), n);
    // An open chain: one line gives one copy, joined to nothing.
    let o = s.execute("sketch.offset", &json!({"entities": [lines[0]], "distance": 3})).unwrap();
    assert_eq!(o["lines"].as_array().unwrap().len(), 1);
    assert_eq!(o["points"].as_array().unwrap().len(), 2);
}

#[test]
fn mirror_copies_with_symmetric_constraints() {
    let mut s = session();
    let (_, r) = rectangle(&mut s);
    // A vertical mirror line at x = 100.
    let axis =
        s.execute("sketch.addLine", &json!({"from": [100, -10], "to": [100, 60], "vertical": true, "construction": true})).unwrap()["line"].clone();
    let circle = s.execute("sketch.addCircle", &json!({"center": [20, 10], "diameter": 8})).unwrap()["circle"].clone();
    let mut shapes = r["lines"].as_array().unwrap().clone();
    shapes.push(circle);
    let n = undo_len(&s);
    let m = s.execute("sketch.mirror", &json!({"entities": shapes, "line": axis})).unwrap();
    assert_eq!(undo_len(&s), n + 1);
    assert_eq!(m["entities"].as_array().unwrap().len(), 5);
    let v = inspect(&mut s);
    // Four corners and one centre, each mirrored about x = 100.
    assert_eq!(constraints_of(&v, "symmetric"), 5);
    assert_eq!(m["points"].as_array().unwrap().len(), 5);
    let xs: Vec<f64> = m["points"].as_array().unwrap().iter().map(|p| point(&v, p.as_u64().unwrap()).0).collect();
    for want in [150.0, 200.0, 180.0] {
        assert!(xs.iter().any(|x| (x - want).abs() < 1e-6), "{want} in {xs:?}");
    }
    assert_eq!(constraints_of(&v, "equal"), 1);
    // The mirror line itself cannot be mirrored; nothing changes on a refusal.
    let n = undo_len(&s);
    assert!(s.execute("sketch.mirror", &json!({"entities": [axis], "line": axis})).is_err());
    assert!(s.execute("sketch.mirror", &json!({"entities": [circle_id(&v)], "line": 999})).is_err());
    assert_eq!(undo_len(&s), n);
}

fn circle_id(v: &Value) -> u64 {
    v["entities"].as_array().unwrap().iter().find(|e| e["kind"] == "circle").unwrap()["id"].as_u64().unwrap()
}

// ---- E1 Part D: Fusion-style dimensions (horizontal / vertical, placement, driven) ----

fn dim_rows(s: &mut Session) -> Vec<(String, String)> {
    let list = s.execute("parameters.list", &json!({})).unwrap();
    list["rows"].as_array().unwrap().iter().map(|r| (r["name"].as_str().unwrap().to_string(), r["expression"].as_str().unwrap().to_string())).collect()
}

#[test]
fn a_horizontal_distance_holds_the_x_offset_and_a_vertical_the_y() {
    let mut s = session();
    let id = new_sketch(&mut s);
    // A free line from (10, 10) to (40, 30): points 1 and 2.
    s.execute("sketch.addLine", &json!({"id": id.0, "from": [10.0, 10.0], "to": [40.0, 30.0]})).unwrap();
    // Picked right to left: the engine turns it round so the value is positive.
    let r = s.execute("sketch.addDimension", &json!({"id": id.0, "constraint": {"type": "distance", "a": 2, "b": 1, "along": "x"}, "expression": "50"})).unwrap();
    assert_eq!(r["name"], "d1");
    let v = inspect(&mut s);
    let (a, b) = (point(&v, 1), point(&v, 2));
    assert!((b.0 - a.0 - 50.0).abs() < 1e-6, "x offset 50: {a:?} {b:?}");
    assert_eq!(v["constraints"][1]["along"], "x");
    assert_eq!((v["constraints"][1]["a"].as_u64(), v["constraints"][1]["b"].as_u64()), (Some(1), Some(2)), "turned round");
    let r = s.execute("sketch.addDimension", &json!({"id": id.0, "constraint": {"type": "distance", "a": 1, "b": 2, "along": "y"}, "expression": "20"})).unwrap();
    assert_eq!(r["value"], 20.0);
    let v = inspect(&mut s);
    let (a, b) = (point(&v, 1), point(&v, 2));
    assert!((b.1 - a.1 - 20.0).abs() < 1e-6 && (b.0 - a.0 - 50.0).abs() < 1e-6, "y offset 20 below, x kept: {a:?} {b:?}");
    // The recipe measures them too.
    assert_eq!(v["constraints"][2]["value"], 20.0);
    // A third, aligned, over-constrains.
    let e = s.execute("sketch.addDimension", &json!({"id": id.0, "constraint": {"type": "distance", "a": 1, "b": 2}})).unwrap_err().to_string();
    assert!(e.contains("over-constrain"), "{e}");
}

#[test]
fn a_dimension_is_placed_moved_renamed_and_the_place_survives_save_and_open() {
    let mut s = session();
    let id = new_sketch(&mut s);
    s.execute("sketch.addLine", &json!({"id": id.0, "from": [10.0, 10.0], "to": [40.0, 10.0]})).unwrap();
    let r = s.execute("sketch.addDimension", &json!({"id": id.0, "constraint": {"type": "length", "line": 0}, "at": [25.0, -8.0]})).unwrap();
    assert_eq!(r["name"], "d1");
    assert_eq!(inspect(&mut s)["constraints"][1]["at"], json!([25.0, -8.0]));
    let n = undo_len(&s);
    s.execute("sketch.placeDimension", &json!({"id": id.0, "name": "d1", "at": [25.0, 20.0]})).unwrap();
    assert_eq!(undo_len(&s), n + 1, "moving a dimension is one undo step");
    assert_eq!(inspect(&mut s)["constraints"][1]["at"], json!([25.0, 20.0]));
    s.execute("parameters.rename", &json!({"from": "d1", "to": "width"})).unwrap();
    let v = inspect(&mut s);
    assert_eq!((v["constraints"][1]["param"].as_str(), v["constraints"][1]["at"].clone()), (Some("width"), json!([25.0, 20.0])), "the place follows the name");
    let doc = s.doc().unwrap().doc.clone();
    let bytes = vectorcraft_format::save_file(&doc);
    let back = vectorcraft_format::load_file(&bytes).unwrap().doc;
    let sketch = back.node(id).unwrap().extras().sketch.as_deref().unwrap().clone();
    assert_eq!(sketch.places.get("width"), Some(&[25.0, 20.0]));
    s.execute("sketch.placeDimension", &json!({"id": id.0, "index": 1, "at": null})).unwrap();
    assert!(inspect(&mut s)["constraints"][1]["at"].is_null(), "null lets it sit beside its line again");
    assert!(s.execute("sketch.placeDimension", &json!({"id": id.0, "index": 0, "at": [1.0, 1.0]})).is_err(), "a geometric constraint has no place");
    // Deleting the dimension forgets its place.
    s.execute("sketch.placeDimension", &json!({"id": id.0, "index": 1, "at": [1.0, 1.0]})).unwrap();
    s.execute("sketch.removeConstraint", &json!({"id": id.0, "index": 1})).unwrap();
    let sketch = s.doc().unwrap().doc.node(id).unwrap().extras().sketch.as_deref().unwrap().clone();
    assert!(sketch.places.is_empty());
}

#[test]
fn a_driven_dimension_reads_the_geometry_and_can_become_driving_when_room_allows() {
    let mut s = session();
    let id = new_sketch(&mut s);
    // A line whose length is already dimensioned (d1 = 40).
    s.execute("sketch.addLine", &json!({"id": id.0, "from": [10.0, 10.0], "to": [50.0, 10.0], "length": 40, "horizontal": true})).unwrap();
    let dof_before = inspect(&mut s)["dof"].clone();
    let e = s.execute("sketch.addDimension", &json!({"id": id.0, "constraint": {"type": "length", "line": 0}})).unwrap_err().to_string();
    assert!(e.contains("over-constrain"), "{e}");
    // The same, driven: accepted, in brackets, and it reads 40.
    let r = s.execute("sketch.addDimension", &json!({"id": id.0, "constraint": {"type": "length", "line": 0}, "expression": "99", "driven": true})).unwrap();
    assert_eq!((r["name"].as_str(), r["driven"].as_bool(), r["value"].as_f64()), (Some("d2"), Some(true), Some(40.0)), "a driven dimension ignores a typed value");
    let v = inspect(&mut s);
    assert_eq!(v["constraints"].as_array().unwrap().last().unwrap()["driven"], true);
    assert_eq!(v["dof"], dof_before, "a driven dimension takes no freedom");
    assert!(dim_rows(&mut s).contains(&("d2".into(), "40 mm".into())), "{:?}", dim_rows(&mut s));
    // Change d1: the driven d2 follows the geometry.
    s.execute("sketch.setDimension", &json!({"id": id.0, "name": "d1", "expression": "60"})).unwrap();
    assert!(dim_rows(&mut s).contains(&("d2".into(), "60 mm".into())), "{:?}", dim_rows(&mut s));
    // And through a parameter edit outside the sketch commands too.
    s.execute("parameters.set", &json!({"name": "d1", "expression": "70 mm"})).unwrap();
    assert!(dim_rows(&mut s).contains(&("d2".into(), "70 mm".into())), "{:?}", dim_rows(&mut s));
    // A driven dimension cannot be typed over.
    let e = s.execute("sketch.setDimension", &json!({"id": id.0, "name": "d2", "expression": "10"})).unwrap_err().to_string();
    assert!(e.contains("driven"), "{e}");
    // Driving again is refused while d1 holds the length; after d1 goes it drives.
    let e = s.execute("sketch.setDriven", &json!({"id": id.0, "name": "d2", "driven": false})).unwrap_err().to_string();
    assert!(e.contains("over-constrain"), "{e}");
    let d1 = inspect(&mut s)["constraints"].as_array().unwrap().iter().position(|c| c["param"] == "d1").unwrap();
    s.execute("sketch.removeConstraint", &json!({"id": id.0, "index": d1})).unwrap();
    let r = s.execute("sketch.setDriven", &json!({"id": id.0, "name": "d2", "driven": false})).unwrap();
    assert_eq!(r["driven"], false);
    s.execute("sketch.setDimension", &json!({"id": id.0, "name": "d2", "expression": "25"})).unwrap();
    let v = inspect(&mut s);
    let (a, b) = (point(&v, 1), point(&v, 2));
    assert!(((b.0 - a.0).hypot(b.1 - a.1) - 25.0).abs() < 1e-6, "d2 drives now: {a:?} {b:?}");
    // Back to driven is always allowed and survives save and open.
    s.execute("sketch.setDriven", &json!({"id": id.0, "name": "d2", "driven": true})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let back = vectorcraft_format::load_file(&vectorcraft_format::save_file(&doc)).unwrap().doc;
    assert!(back.node(id).unwrap().extras().sketch.as_deref().unwrap().driven.contains("d2"));
}
