//! The timeline (Euclidean, C1): every journaled command that changes the document is a step,
//! stored in the document; editing a step replays the ones after it, as one undo step.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_engine::Session;

/// A session that records the timeline, as the app's does.
fn session() -> Session {
    let mut s = Session::new();
    run(&mut s, "timeline.setRecording", json!({"on": true}));
    s
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn steps(s: &mut Session) -> Vec<Value> {
    run(s, "timeline.list", json!({}))["steps"].as_array().unwrap().clone()
}

fn cmds(s: &mut Session) -> Vec<String> {
    steps(s).iter().map(|v| v["cmd"].as_str().unwrap().to_string()).collect()
}

fn doc_json(s: &Session) -> String {
    serde_json::to_string(&*s.doc().unwrap().doc).unwrap()
}

fn width_of(s: &Session, id: u64) -> f64 {
    s.doc().unwrap().doc.bounds_of(&[NodeId(id)], false).unwrap().width()
}

/// A new document, a rectangle and an ellipse, the rectangle selected and offset by 5, its fill
/// changed: → (rectangle id, offset path id).
fn drawing(s: &mut Session) -> (u64, u64) {
    run(s, "file.new", json!({"width": 300, "height": 200}));
    let rect = run(s, "shape.rectangle", json!({"x": 10, "y": 10, "width": 50, "height": 40}))["id"].as_u64().unwrap();
    run(s, "shape.ellipse", json!({"x": 100, "y": 50, "width": 60, "height": 30}));
    run(s, "select.set", json!({"ids": [rect]}));
    run(s, "object.path.offsetPath", json!({"offset": 5}));
    let offset = s.doc().unwrap().selection.objects[0].0;
    run(s, "paint.setFill", json!({"color": "#ff0000"}));
    (rect, offset)
}

#[test]
fn a_new_document_records_one_step_per_journaled_command_and_none_for_queries() {
    let mut s = session();
    drawing(&mut s);
    run(&mut s, "timeline.list", json!({}));
    run(&mut s, "parameters.list", json!({}));
    // `select.set` changes only the selection: it is recorded with the step after it, not as one.
    assert_eq!(cmds(&mut s), ["shape.rectangle", "shape.ellipse", "object.path.offsetPath", "paint.setFill"]);
    let st = steps(&mut s);
    assert_eq!(st[2]["selection"], json!([2]));
    assert_eq!(st[2]["label"], "Offset Path 5");
    assert_eq!(run(&mut s, "timeline.list", json!({}))["editable"], true);
}

#[test]
fn a_drag_records_one_step() {
    let mut s = session();
    drawing(&mut s);
    s.begin_interaction("Move").unwrap();
    s.preview("object.move", &json!({"dx": 3.0, "dy": 0.0})).unwrap();
    s.preview("object.move", &json!({"dx": 7.5, "dy": 2.0})).unwrap();
    s.commit_interaction().unwrap();
    let st = steps(&mut s);
    assert_eq!(st.len(), 5);
    assert_eq!(st[4]["cmd"], "object.move");
    assert_eq!(st[4]["params"]["dx"], 7.5);
    assert_eq!(st[4]["label"], "Move 7.5 × 2");
}

#[test]
fn undo_removes_the_last_step_and_redo_restores_it() {
    let mut s = session();
    drawing(&mut s);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(cmds(&mut s), ["shape.rectangle", "shape.ellipse", "object.path.offsetPath"]);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(cmds(&mut s), ["shape.rectangle", "shape.ellipse", "object.path.offsetPath", "paint.setFill"]);
}

#[test]
fn save_and_load_keep_the_steps_and_the_base() {
    let mut s = session();
    let (rect, offset) = drawing(&mut s);
    let bytes = vectorcraft_format::save(&s.doc().unwrap().doc, false);
    let loaded = vectorcraft_format::load(&bytes).unwrap();
    let mut t = session();
    t.add_document(loaded, None);
    assert_eq!(steps(&mut t), steps(&mut s));
    // The base came back too: an edit replays from it.
    run(&mut t, "timeline.editStep", json!({"index": 0, "params": {"width": 80}}));
    assert!((width_of(&t, rect) - 80.0).abs() < 1e-6);
    assert!((width_of(&t, offset) - 90.0).abs() < 0.5, "{}", width_of(&t, offset));
}

#[test]
fn editing_a_rectangles_width_replays_the_offset_after_it() {
    let mut s = session();
    let (rect, offset) = drawing(&mut s);
    assert!((width_of(&s, offset) - 60.0).abs() < 0.5);
    let r = run(&mut s, "timeline.editStep", json!({"index": 0, "params": {"width": 120}}));
    assert_eq!(r["failed"], Value::Null);
    assert!((width_of(&s, rect) - 120.0).abs() < 1e-6);
    // The offset path follows the new width: 120 + 2 × 5.
    assert!((width_of(&s, offset) - 130.0).abs() < 0.5, "{}", width_of(&s, offset));
    assert_eq!(steps(&mut s)[0]["label"], "Rectangle 120 × 40");
    assert_eq!(cmds(&mut s).len(), 4);
}

#[test]
fn replaying_unchanged_steps_gives_the_same_document() {
    let mut s = session();
    drawing(&mut s);
    s.begin_interaction("Move").unwrap();
    s.preview("object.move", &json!({"dx": 7.5, "dy": 2.0})).unwrap();
    s.commit_interaction().unwrap();
    let before = doc_json(&s);
    run(&mut s, "timeline.replay", json!({}));
    assert_eq!(doc_json(&s), before);
}

#[test]
fn a_step_whose_objects_are_gone_fails_and_replay_stops_there() {
    let mut s = session();
    run(&mut s, "file.new", json!({"width": 300, "height": 200}));
    let rect = run(&mut s, "shape.rectangle", json!({"x": 10, "y": 10, "width": 50, "height": 40}))["id"].as_u64().unwrap();
    let ellipse = run(&mut s, "shape.ellipse", json!({"x": 100, "y": 50, "width": 60, "height": 30}))["id"].as_u64().unwrap();
    run(&mut s, "edit.clear", json!({"ids": [ellipse]}));
    run(&mut s, "select.set", json!({"ids": [rect]}));
    run(&mut s, "object.path.offsetPath", json!({"offset": 5}));
    run(&mut s, "paint.setFill", json!({"color": "#ff0000"}));
    assert_eq!(cmds(&mut s), ["shape.rectangle", "shape.ellipse", "edit.clear", "object.path.offsetPath", "paint.setFill"]);
    // Clear the rectangle instead: the offset step's object is gone.
    let r = run(&mut s, "timeline.editStep", json!({"index": 2, "params": {"ids": [rect]}}));
    assert_eq!(r["failed"], 3);
    assert!(r["error"].as_str().unwrap().contains("gone"), "{r}");
    let st = steps(&mut s);
    assert_eq!(st.len(), 5, "the steps after the failed one are kept");
    assert_eq!(st[3]["status"], "failed");
    assert_eq!(st[4]["status"], "pending");
    // The document is as of the last good step: the ellipse is there, the rectangle isn't.
    let d = &s.doc().unwrap().doc;
    assert!(d.node(NodeId(ellipse)).is_some() && d.node(NodeId(rect)).is_none());
    // Undo goes back to before the edit.
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc().unwrap().doc.node(NodeId(rect)).is_some());
    assert!(steps(&mut s).iter().all(|v| v["status"] == "ok"));
}

#[test]
fn an_edit_is_one_undo_step() {
    let mut s = session();
    let (rect, _) = drawing(&mut s);
    let before = doc_json(&s);
    let undo_len = s.doc().unwrap().history.undo.len();
    run(&mut s, "timeline.editStep", json!({"index": 0, "params": {"width": 120}}));
    assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1);
    assert!((width_of(&s, rect) - 120.0).abs() < 1e-6);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc_json(&s), before);
    run(&mut s, "edit.redo", json!({}));
    assert!((width_of(&s, rect) - 120.0).abs() < 1e-6);
}

#[test]
fn timeline_list_returns_the_steps() {
    let mut s = session();
    run(&mut s, "file.new", json!({"width": 300, "height": 200}));
    assert_eq!(steps(&mut s).len(), 0);
    drawing(&mut s);
    let l = run(&mut s, "timeline.list", json!({}));
    assert_eq!(l["steps"].as_array().unwrap().len(), 4);
    assert_eq!(l["steps"][0]["index"], 0);
    assert_eq!(l["steps"][0]["status"], "ok");
    assert!(l["baseBytes"].as_u64().unwrap() > 0);
    // A bad index is an error, not a crash.
    assert!(s.execute("timeline.editStep", &json!({"index": 99, "params": {}})).is_err());
    assert!(s.execute("timeline.editStep", &json!({"index": 0})).is_err());
}

#[test]
fn a_document_opened_from_a_file_starts_from_the_document_as_opened() {
    let mut s = session();
    run(&mut s, "file.new", json!({"width": 300, "height": 200}));
    run(&mut s, "shape.rectangle", json!({"x": 10, "y": 10, "width": 50, "height": 40}));
    // A file saved before the timeline existed (no steps in it).
    let mut d = (*s.doc().unwrap().doc).clone();
    d.unknown.remove(vectorcraft_engine::timeline::DOC_KEY);
    let mut t = session();
    t.add_document(vectorcraft_format::load(&vectorcraft_format::save(&d, false)).unwrap(), None);
    let e = run(&mut t, "shape.ellipse", json!({"x": 100, "y": 50, "width": 60, "height": 30}))["id"].as_u64().unwrap();
    assert_eq!(cmds(&mut t), ["shape.ellipse"]);
    run(&mut t, "timeline.editStep", json!({"index": 0, "params": {"width": 20}}));
    assert!((width_of(&t, e) - 20.0).abs() < 1e-6);
    // The rectangle from the file is still there.
    assert!(t.doc().unwrap().doc.node(NodeId(2)).is_some());
}

#[test]
fn rolling_back_to_step_2_of_4_shows_the_document_as_of_step_2() {
    let mut s = session();
    let (rect, offset) = drawing(&mut s);
    let r = run(&mut s, "timeline.rollTo", json!({"index": 2}));
    assert_eq!(r["marker"], 2);
    let d = &s.doc().unwrap().doc;
    assert!(d.node(NodeId(rect)).is_some() && d.node(NodeId(offset)).is_none(), "the offset path is not made yet");
    let l = run(&mut s, "timeline.list", json!({}));
    assert_eq!(l["marker"], 2);
    let rolled: Vec<bool> = l["steps"].as_array().unwrap().iter().map(|v| v["rolledBack"].as_bool().unwrap()).collect();
    assert_eq!(rolled, [false, false, true, true]);
    // Rolling forward to the end gives the document back.
    run(&mut s, "timeline.rollTo", json!({"index": 4}));
    assert!((width_of(&s, offset) - 60.0).abs() < 0.5);
    assert_eq!(run(&mut s, "timeline.list", json!({}))["marker"], 4);
}

#[test]
fn a_new_action_while_rolled_back_is_inserted_at_the_marker_and_the_later_steps_replay() {
    let mut s = session();
    let (rect, _) = drawing(&mut s);
    run(&mut s, "timeline.rollTo", json!({"index": 2}));
    let rolled_back = doc_json(&s);
    let star = run(&mut s, "shape.rectangle", json!({"x": 200, "y": 20, "width": 30, "height": 30}))["id"].as_u64().unwrap();
    assert_eq!(cmds(&mut s), ["shape.rectangle", "shape.ellipse", "shape.rectangle", "object.path.offsetPath", "paint.setFill"]);
    let l = run(&mut s, "timeline.list", json!({}));
    assert_eq!(l["marker"], 5);
    assert!(l["steps"].as_array().unwrap().iter().all(|v| v["status"] == "ok" && v["rolledBack"] == false), "{l}");
    // Steps 3 and 4 replayed after the new one: the offset path of the first rectangle exists
    // again (its id moved up by one), filled red.
    let d = &s.doc().unwrap().doc;
    assert!(d.node(NodeId(rect)).is_some() && d.node(NodeId(star)).is_some());
    let offset = star + 1;
    assert!((width_of(&s, offset) - 60.0).abs() < 0.5, "{}", width_of(&s, offset));
    // One undo step: undo goes back to the rolled-back document.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc_json(&s), rolled_back);
}
