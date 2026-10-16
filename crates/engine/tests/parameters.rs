//! Euclidean: user parameters and parametric sketches through the commands every surface uses.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use vectorcraft_doc::{Node, NodeId, Unit, params, sketch::Sketch};
use vectorcraft_engine::Session;

const MM: f64 = 72.0 / 25.4;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 600, "height": 400})).unwrap();
    s
}

fn sample(s: &mut Session) -> NodeId {
    NodeId(s.execute("sketch.sample", &json!({})).unwrap()["id"].as_u64().unwrap())
}

fn bounds(s: &Session, id: NodeId) -> vectorcraft_geom::Rect {
    s.doc().unwrap().doc.node(id).unwrap().geometric_bounds().unwrap()
}

fn children(s: &Session, id: NodeId) -> Vec<(String, u64)> {
    s.doc().unwrap().doc.node(id).and_then(Node::children).unwrap().iter().map(|c| (c.name.clone().unwrap_or_default(), c.id.0)).collect()
}

fn undo_len(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

fn row<'a>(list: &'a Value, name: &str) -> &'a Value {
    list["rows"].as_array().unwrap().iter().find(|r| r["name"] == name).unwrap()
}

#[test]
fn the_sample_is_a_plate_of_ten_paths_selected_in_the_current_layer() {
    let mut s = session();
    let id = sample(&mut s);
    assert_eq!(s.doc().unwrap().selection.objects, vec![id]);
    let kids = children(&s, id);
    let names: Vec<&str> = kids.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["R1", "C1", "C1", "C1", "C1", "C2", "C2", "C2", "C2", "R2"]);
    let b = bounds(&s, id);
    assert!((b.width() - 120.0 * MM).abs() < 1e-6 && (b.height() - 80.0 * MM).abs() < 1e-6, "{b:?}");
    assert!(b.x0.abs() < 1e-9 && b.y0.abs() < 1e-9, "the plate hangs from the artboard's top-left: {b:?}");
    let list = s.execute("parameters.list", &json!({})).unwrap();
    assert_eq!(list["rows"].as_array().unwrap().len(), 7);
    assert_eq!(row(&list, "slot_w")["value"], json!(60.0));
    assert_eq!(row(&list, "holes")["unit"], "");
    assert_eq!(row(&list, "width")["favourite"], true);
    // The group is a plain group to everything else.
    let inspect = s.execute("document.inspect", &json!({})).unwrap();
    assert!(inspect.to_string().contains("Parametric Sketch"), "{inspect}");
}

#[test]
fn setting_width_widens_the_plate_in_one_undo_step() {
    let mut s = session();
    let id = sample(&mut s);
    let before = undo_len(&s);
    let r = s.execute("parameters.set", &json!({"name": "width", "expression": "200 mm"})).unwrap();
    assert_eq!(r, json!({"name": "width", "value": 200.0, "error": null}));
    assert_eq!(undo_len(&s), before + 1);
    let b = bounds(&s, id);
    assert!((b.width() - 200.0 * MM).abs() < 1e-6, "{b:?}");
    // The holes spread with it: pitch (200 - 24) / 3 mm.
    let kids = s.doc().unwrap().doc.node(id).and_then(Node::children).unwrap().clone();
    let c = |i: usize| kids[i].geometric_bounds().unwrap().center().x;
    assert!((c(2) - c(1) - (176.0 / 3.0) * MM).abs() < 1e-6);
    s.execute("edit.undo", &json!({})).unwrap();
    let b = bounds(&s, id);
    assert!((b.width() - 120.0 * MM).abs() < 1e-6, "{b:?}");
    assert_eq!(s.execute("parameters.list", &json!({})).unwrap()["rows"][0]["expression"], "120 mm");
    s.execute("edit.redo", &json!({})).unwrap();
    assert!((bounds(&s, id).width() - 200.0 * MM).abs() < 1e-6);
    // Setting the same value again is not an undo step.
    let n = undo_len(&s);
    s.execute("parameters.set", &json!({"name": "width", "expression": "200 mm"})).unwrap();
    assert_eq!(undo_len(&s), n);
}

#[test]
fn six_holes_rebuild_twelve_circles_and_a_broken_field_keeps_the_old_paths() {
    let mut s = session();
    let id = sample(&mut s);
    s.execute("parameters.set", &json!({"name": "holes", "expression": "6"})).unwrap();
    assert_eq!(children(&s, id).iter().filter(|(n, _)| n == "C1" || n == "C2").count(), 12);
    let before = children(&s, id);
    let r = s.execute("sketch.setField", &json!({"id": id.0, "shape": "C1", "field": "d", "expression": "hole_dia +"})).unwrap();
    assert_eq!(r["error"], "unfinished");
    let after = children(&s, id);
    // C1 keeps its six previous paths (same ids); the rest were rebuilt.
    let c1 = |k: &[(String, u64)]| k.iter().filter(|(n, _)| n == "C1").cloned().collect::<Vec<_>>();
    assert_eq!(c1(&before), c1(&after));
    assert_eq!(after.len(), 14);
    let inspect = s.execute("sketch.inspect", &json!({})).unwrap();
    assert_eq!(inspect["shapes"][1]["error"], "d: unfinished");
    assert_eq!(inspect["shapes"][1]["fields"]["d"]["error"], "unfinished");
    assert_eq!(inspect["shapes"][0]["fields"]["w"]["value"], 120.0);
    // A parameter that does not evaluate is reported by the list.
    s.execute("parameters.set", &json!({"name": "corner", "expression": "nope * 2"})).unwrap();
    let list = s.execute("parameters.list", &json!({})).unwrap();
    assert_eq!(row(&list, "corner")["error"], "no parameter \"nope\"");
    assert_eq!(row(&list, "corner")["value"], Value::Null);
}

#[test]
fn save_and_load_keep_every_row_and_the_recipe() {
    let mut s = session();
    let id = sample(&mut s);
    s.execute("parameters.set", &json!({"name": "margin", "favourite": true, "comment": "edge"})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let bytes = vectorcraft_format::save_file(&doc);
    let back = vectorcraft_format::load_file(&bytes).unwrap().doc;
    assert_eq!(params::rows(&back), params::rows(&doc));
    assert_eq!(params::rows(&back).len(), 7);
    let n = back.node(id).unwrap();
    assert_eq!(n.extras().sketch.as_deref(), Some(&Sketch::sample()));
    assert_eq!(n.children().unwrap().len(), 10);
    // Reopened, the parameters still drive it.
    let mut s2 = Session::new();
    s2.add_document(back, None);
    s2.execute("parameters.set", &json!({"name": "height", "expression": "100 mm"})).unwrap();
    assert!((bounds(&s2, id).height() - 100.0 * MM).abs() < 1e-6);
}

#[test]
fn renaming_rewrites_the_other_parameters_and_the_sketch() {
    let mut s = session();
    let id = sample(&mut s);
    let before = bounds(&s, id);
    s.execute("parameters.rename", &json!({"from": "width", "to": "plate_w"})).unwrap();
    let list = s.execute("parameters.list", &json!({})).unwrap();
    assert_eq!(row(&list, "slot_w")["expression"], "plate_w / 2");
    assert_eq!(row(&list, "plate_w")["value"], 120.0);
    assert!(list["rows"].as_array().unwrap().iter().all(|r| r["error"].is_null()), "{list}");
    let inspect = s.execute("sketch.inspect", &json!({"id": id.0})).unwrap();
    assert_eq!(inspect["shapes"][0]["fields"]["w"]["expression"], "plate_w");
    assert_eq!(inspect["shapes"][1]["fields"]["pitch"]["expression"], "(plate_w - 2*margin) / (holes - 1)");
    assert_eq!(bounds(&s, id), before);
    // Collisions and bad names are refused.
    assert!(s.execute("parameters.rename", &json!({"from": "height", "to": "margin"})).is_err());
    assert!(s.execute("parameters.rename", &json!({"from": "height", "to": "2h"})).is_err());
    assert!(s.execute("parameters.rename", &json!({"from": "height", "to": "sqrt"})).is_err());
}

#[test]
fn csv_is_the_client_params_form() {
    let mut s = session();
    sample(&mut s);
    let csv = s.execute("parameters.csv", &json!({})).unwrap()["csv"].as_str().unwrap().to_string();
    assert_eq!(
        csv,
        "name,unit,expression,comment\n\
         width,mm,120 mm,plate width\n\
         height,mm,80 mm,plate height\n\
         margin,mm,12 mm,hole centre from edge\n\
         hole_dia,mm,8 mm,bolt clearance\n\
         holes,,4,holes per row\n\
         corner,mm,6 mm,corner radius\n\
         slot_w,mm,width / 2,centre slot follows width\n"
    );
}

#[test]
fn add_remove_and_starring_follow_the_sandbox() {
    let mut s = session();
    let r = s.execute("parameters.add", &json!({})).unwrap();
    assert_eq!(r["name"], "param1");
    let r = s.execute("parameters.add", &json!({"unit": ""})).unwrap();
    assert_eq!(r["name"], "param2");
    let list = s.execute("parameters.list", &json!({})).unwrap();
    assert_eq!(list["rows"][0]["expression"], "10 mm");
    assert_eq!(row(&list, "param2")["expression"], "10");
    // Starring without a range: a quarter to four times (E1), stepping by the unit's step.
    s.execute("parameters.set", &json!({"name": "param1", "favourite": true})).unwrap();
    let list = s.execute("parameters.list", &json!({})).unwrap();
    assert_eq!(
        (list["rows"][0]["min"].as_f64(), list["rows"][0]["max"].as_f64(), list["rows"][0]["step"].as_f64()),
        (Some(3.0), Some(40.0), Some(0.5))
    );
    // A starred row always has a range: clearing `min` refills it; unstarred, null clears it.
    s.execute("parameters.set", &json!({"name": "param1", "min": null})).unwrap();
    assert_eq!(s.execute("parameters.list", &json!({})).unwrap()["rows"][0]["min"], 3.0);
    s.execute("parameters.set", &json!({"name": "param1", "favourite": false, "min": null})).unwrap();
    assert!(s.execute("parameters.list", &json!({})).unwrap()["rows"][0]["min"].is_null());
    s.execute("parameters.remove", &json!({"name": "param1"})).unwrap();
    assert_eq!(s.execute("parameters.list", &json!({})).unwrap()["rows"].as_array().unwrap().len(), 1);
    for bad in [
        json!({"name": "1x"}),
        json!({"name": "pi"}),
        json!({"name": "mm"}),
        json!({"name": "ok", "unit": "cm"}),
        json!({"name": "ok", "favourite": "yes"}),
    ] {
        assert!(s.execute("parameters.add", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("parameters.set", &json!({"name": "gone", "expression": "1"})).is_err());
}

#[test]
fn create_and_shape_commands_build_and_edit_a_sketch() {
    let mut s = session();
    s.execute("parameters.add", &json!({"name": "size", "expression": "25 mm"})).unwrap();
    let id = NodeId(
        s.execute("sketch.create", &json!({"shapes": [{"kind": "rect", "fields": {"w": "size", "h": "size * 2"}}]})).unwrap()["id"].as_u64().unwrap(),
    );
    let b = bounds(&s, id);
    assert!((b.width() - 25.0 * MM).abs() < 1e-6 && (b.height() - 50.0 * MM).abs() < 1e-6, "{b:?}");
    let r = s.execute("sketch.addShape", &json!({"kind": "circle"})).unwrap();
    assert_eq!(r["shape"], "C1");
    let r = s.execute("sketch.addShape", &json!({"kind": "line"})).unwrap();
    assert_eq!(r["shape"], "L1");
    assert_eq!(children(&s, id).len(), 3);
    s.execute("sketch.removeShape", &json!({"shape": "C1"})).unwrap();
    assert_eq!(children(&s, id).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["R1", "L1"]);
    // A default sketch is one rectangle; bad shapes are refused.
    let id2 = NodeId(s.execute("sketch.create", &json!({})).unwrap()["id"].as_u64().unwrap());
    assert_eq!(children(&s, id2).len(), 1);
    assert!(s.execute("sketch.create", &json!({"shapes": [{"kind": "hexagon"}]})).is_err());
    assert!(s.execute("sketch.create", &json!({"shapes": [{"kind": "rect", "fields": {"zz": "1"}}]})).is_err());
    assert!(s.execute("sketch.setField", &json!({"id": id2.0, "shape": "R1", "field": "cx", "expression": "1"})).is_err());
    // Not a sketch: refused.
    let plain = s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap()["id"].as_u64().unwrap();
    assert!(s.execute("sketch.inspect", &json!({"id": plain})).is_err());
    // Undo removes the group.
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.node(id2).is_none());
}

#[test]
fn moving_a_sketch_keeps_the_move_through_the_next_rebuild() {
    let mut s = session();
    let id = sample(&mut s);
    s.execute("object.transform", &json!({"matrix": [1, 0, 0, 1, 100, 50], "ids": [id.0]})).unwrap();
    let b = bounds(&s, id);
    assert!((b.x0 - 100.0).abs() < 1e-6 && (b.y0 - 50.0).abs() < 1e-6, "{b:?}");
    let origin = s.execute("sketch.inspect", &json!({"id": id.0})).unwrap()["origin"].clone();
    assert!((origin[0].as_f64().unwrap() - Unit::Millimeters.from_pt(100.0)).abs() < 1e-9, "{origin}");
    s.execute("parameters.set", &json!({"name": "width", "expression": "150 mm"})).unwrap();
    let b = bounds(&s, id);
    assert!((b.x0 - 100.0).abs() < 1e-6 && (b.y0 - 50.0).abs() < 1e-6 && (b.width() - 150.0 * MM).abs() < 1e-6, "{b:?}");
    // Scaling is not folded into the recipe: the paths stay scaled until the next rebuild, which
    // draws the recipe at the (moved) origin again.
    s.execute("object.transform", &json!({"matrix": [2, 0, 0, 2, 0, 0], "ids": [id.0]})).unwrap();
    assert!((bounds(&s, id).width() - 300.0 * MM).abs() < 1e-6);
    s.execute("parameters.set", &json!({"name": "width", "expression": "160 mm"})).unwrap();
    let b = bounds(&s, id);
    assert!((b.width() - 160.0 * MM).abs() < 1e-6 && (b.x0 - 100.0).abs() < 1e-6, "{b:?}");
}

#[test]
fn a_copied_sketch_follows_the_parameters_too() {
    let mut s = session();
    let id = sample(&mut s);
    s.execute("edit.copy", &json!({})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    let copy = s.doc().unwrap().selection.objects[0];
    assert_ne!(copy, id);
    s.execute("parameters.set", &json!({"name": "height", "expression": "90 mm"})).unwrap();
    assert!((bounds(&s, copy).height() - 90.0 * MM).abs() < 1e-6);
    assert!((bounds(&s, id).height() - 90.0 * MM).abs() < 1e-6);
}
