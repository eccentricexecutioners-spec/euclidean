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

/// A rectangle, an ellipse, the rectangle offset by 5 (step 2), then the ellipse filled red
/// (step 3): → (rectangle id, ellipse id, offset path id).
fn offset_then_fill_ellipse(s: &mut Session) -> (u64, u64, u64) {
    run(s, "file.new", json!({"width": 300, "height": 200}));
    let rect = run(s, "shape.rectangle", json!({"x": 10, "y": 10, "width": 50, "height": 40}))["id"].as_u64().unwrap();
    let ellipse = run(s, "shape.ellipse", json!({"x": 100, "y": 50, "width": 60, "height": 30}))["id"].as_u64().unwrap();
    run(s, "select.set", json!({"ids": [rect]}));
    run(s, "object.path.offsetPath", json!({"offset": 5}));
    let offset = s.doc().unwrap().selection.objects[0].0;
    run(s, "select.set", json!({"ids": [ellipse]}));
    run(s, "paint.setFill", json!({"color": "#ff0000"}));
    (rect, ellipse, offset)
}

fn fill_hex(s: &Session, id: u64) -> String {
    match s.doc().unwrap().doc.node(NodeId(id)).unwrap().appearance.fill_paint() {
        vectorcraft_color::Paint::Solid { color, .. } => color.to_hex().to_lowercase(),
        other => panic!("not a solid fill: {other:?}"),
    }
}

fn has(s: &Session, id: u64) -> bool {
    s.doc().unwrap().doc.node(NodeId(id)).is_some()
}

fn timeline_json(s: &Session) -> Value {
    s.doc().unwrap().doc.unknown.get(vectorcraft_engine::timeline::DOC_KEY).cloned().unwrap()
}

fn statuses(s: &mut Session) -> Vec<String> {
    steps(s).iter().map(|v| v["status"].as_str().unwrap().to_string()).collect()
}

#[test]
fn suppressing_the_offset_step_leaves_the_rectangle_unoffset_and_fill_still_applies() {
    let mut s = session();
    let (rect, ellipse, offset) = offset_then_fill_ellipse(&mut s);
    assert!(has(&s, offset) && (width_of(&s, offset) - 60.0).abs() < 0.5);
    assert_eq!(fill_hex(&s, ellipse), "#ff0000");
    let r = run(&mut s, "timeline.suppressStep", json!({"index": 2, "on": true}));
    assert_eq!(r["failed"], Value::Null);
    assert_eq!(r["steps"], 4);
    assert!(!has(&s, offset), "the offset path is not made");
    assert!((width_of(&s, rect) - 50.0).abs() < 1e-6, "{}", width_of(&s, rect));
    assert_eq!(fill_hex(&s, ellipse), "#ff0000");
    assert_eq!(statuses(&mut s), ["ok", "ok", "ok", "ok"]);
    assert_eq!(steps(&mut s)[2]["suppressed"], true);
    assert_eq!(steps(&mut s)[3]["suppressed"], false);
}

#[test]
fn unsuppressing_brings_the_offset_back() {
    let mut s = session();
    let (_, ellipse, offset) = offset_then_fill_ellipse(&mut s);
    run(&mut s, "timeline.suppressStep", json!({"index": 2, "on": true}));
    assert!(!has(&s, offset));
    let r = run(&mut s, "timeline.suppressStep", json!({"index": 2, "on": false}));
    assert_eq!(r["failed"], Value::Null);
    assert!(has(&s, offset));
    assert!((width_of(&s, offset) - 60.0).abs() < 0.5, "{}", width_of(&s, offset));
    assert_eq!(fill_hex(&s, ellipse), "#ff0000");
    assert_eq!(steps(&mut s)[2]["suppressed"], false);
}

#[test]
fn suppressing_the_rectangle_step_moves_the_ellipse_up_and_the_offset_follows_it() {
    let mut s = session();
    run(&mut s, "file.new", json!({"width": 300, "height": 200}));
    let rect = run(&mut s, "shape.rectangle", json!({"x": 10, "y": 10, "width": 50, "height": 40}))["id"].as_u64().unwrap();
    // Deselect so the ellipse step records an empty selection (selection commands are not steps).
    run(&mut s, "select.set", json!({"ids": []}));
    let ellipse = run(&mut s, "shape.ellipse", json!({"x": 100, "y": 50, "width": 60, "height": 30}))["id"].as_u64().unwrap();
    run(&mut s, "select.set", json!({"ids": [ellipse]}));
    run(&mut s, "object.path.offsetPath", json!({"offset": 5}));
    run(&mut s, "paint.setFill", json!({"color": "#ff0000"}));
    assert_eq!(cmds(&mut s), ["shape.rectangle", "shape.ellipse", "object.path.offsetPath", "paint.setFill"]);
    // Ids are handed out in order, so without the rectangle the ellipse takes the id the
    // rectangle had. E1: the offset step follows the ellipse to its new id (C3 left it failing).
    let r = run(&mut s, "timeline.suppressStep", json!({"index": 0, "on": true}));
    assert_eq!(r["failed"], Value::Null, "{r}");
    assert_eq!(statuses(&mut s), ["ok", "ok", "ok", "ok"]);
    let st = steps(&mut s);
    assert_eq!(st.len(), 4);
    assert_eq!(st[1]["created"], json!([ellipse]), "stored ids stay as recorded");
    assert!(has(&s, rect), "the ellipse moved up to the rectangle's id (and the offset path to the ellipse's)");
    assert!((width_of(&s, rect) - 60.0).abs() < 1e-6, "it is the ellipse (60 wide): {}", width_of(&s, rect));
    assert!(st[2]["created"].as_array().is_some_and(|c| !c.is_empty()), "the offset step made its path: {st:?}");
    // A step whose object a suppressed step made fails instead: suppress the ellipse too.
    let r = run(&mut s, "timeline.suppressStep", json!({"index": 1, "on": true}));
    assert_eq!(r["failed"], 2, "{r}");
    assert_eq!(statuses(&mut s), ["ok", "ok", "failed", "pending"]);
    assert!(!has(&s, ellipse) && !has(&s, rect));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(statuses(&mut s), ["ok", "ok", "ok", "ok"]);
    // Undo goes back to the full drawing.
    run(&mut s, "edit.undo", json!({}));
    assert!(has(&s, rect));
    assert_eq!(statuses(&mut s), ["ok", "ok", "ok", "ok"]);
}

#[test]
fn deleting_a_step_removes_it_and_later_steps_replay() {
    let mut s = session();
    let (rect, ellipse, offset) = offset_then_fill_ellipse(&mut s);
    let r = run(&mut s, "timeline.deleteStep", json!({"index": 2}));
    assert_eq!(r["failed"], Value::Null);
    assert_eq!(r["steps"], 3);
    assert_eq!(cmds(&mut s), ["shape.rectangle", "shape.ellipse", "paint.setFill"]);
    assert_eq!(steps(&mut s)[2]["index"], 2, "later steps are renumbered");
    assert!(!has(&s, offset) && has(&s, rect));
    assert_eq!(fill_hex(&s, ellipse), "#ff0000");
    assert!(s.execute("timeline.deleteStep", &json!({"index": 9})).is_err());
    assert!(s.execute("timeline.suppressStep", &json!({"index": 9, "on": true})).is_err());
    assert!(s.execute("timeline.suppressStep", &json!({"index": 0})).is_err());
    // A marker after the deleted step moves up with it.
    run(&mut s, "timeline.rollTo", json!({"index": 2}));
    run(&mut s, "timeline.deleteStep", json!({"index": 0}));
    assert_eq!(run(&mut s, "timeline.list", json!({}))["marker"], 1);
}

#[test]
fn suppress_and_delete_are_one_undo_step_each() {
    let mut s = session();
    let (_, _, _) = offset_then_fill_ellipse(&mut s);
    let nodes = |s: &Session| s.doc().unwrap().doc.node_count();
    for (cmd, params) in [("timeline.suppressStep", json!({"index": 2, "on": true})), ("timeline.deleteStep", json!({"index": 2}))] {
        let (before_doc, before_tl, before_nodes) = (doc_json(&s), timeline_json(&s), nodes(&s));
        let undo_len = s.doc().unwrap().history.undo.len();
        run(&mut s, cmd, params);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1, "{cmd}");
        assert_ne!(timeline_json(&s), before_tl);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(timeline_json(&s), before_tl, "{cmd}");
        assert_eq!(nodes(&s), before_nodes, "{cmd}");
        assert_eq!(doc_json(&s), before_doc, "{cmd}");
    }
}

#[test]
fn suppressed_survives_save_and_reopen() {
    let mut s = session();
    let (_, _, offset) = offset_then_fill_ellipse(&mut s);
    run(&mut s, "timeline.suppressStep", json!({"index": 2, "on": true}));
    let bytes = vectorcraft_format::save(&s.doc().unwrap().doc, false);
    let mut t = session();
    t.add_document(vectorcraft_format::load(&bytes).unwrap(), None);
    let st = steps(&mut t);
    assert_eq!(st[2]["suppressed"], true);
    assert_eq!(st[0]["suppressed"], false);
    // A replay after reopening still skips it.
    run(&mut t, "timeline.replay", json!({}));
    assert!(!has(&t, offset));
    assert_eq!(steps(&mut t)[2]["suppressed"], true);
}

#[test]
fn timeline_list_reports_suppressed() {
    let mut s = session();
    offset_then_fill_ellipse(&mut s);
    let flags = |s: &mut Session| steps(s).iter().map(|v| v["suppressed"].as_bool().unwrap()).collect::<Vec<_>>();
    assert_eq!(flags(&mut s), [false, false, false, false]);
    run(&mut s, "timeline.suppressStep", json!({"index": 3, "on": true}));
    assert_eq!(flags(&mut s), [false, false, false, true]);
    run(&mut s, "timeline.suppressStep", json!({"index": 3, "on": false}));
    assert_eq!(flags(&mut s), [false, false, false, false]);
}

// ---- E1 Part E: features, not clicks ----

#[test]
fn a_pen_path_of_four_clicks_is_one_step_and_suppressing_it_removes_the_path() {
    let mut s = session();
    run(&mut s, "file.new", json!({"width": 300, "height": 200}));
    let id = run(&mut s, "path.create", json!({"anchors": [{"x": 10, "y": 10}]}))["id"].as_u64().unwrap();
    for (x, y) in [(60, 10), (60, 50), (10, 50)] {
        run(&mut s, "path.appendAnchor", json!({"id": id, "x": x, "y": y}));
    }
    run(&mut s, "path.close", json!({"id": id}));
    let st = steps(&mut s);
    assert_eq!(st.len(), 1, "{st:?}");
    assert_eq!((st[0]["cmd"].as_str(), st[0]["label"].as_str(), st[0]["feature"].as_str()), (Some("path.create"), Some("Pen Path"), Some("pen")));
    assert_eq!(st[0]["children"].as_array().unwrap().len(), 4, "three more clicks and the close");
    // A second path is its own step.
    let id2 = run(&mut s, "path.create", json!({"anchors": [{"x": 100, "y": 100}]}))["id"].as_u64().unwrap();
    run(&mut s, "path.appendAnchor", json!({"id": id2, "x": 170, "y": 100}));
    assert_eq!(cmds(&mut s), ["path.create", "path.create"]);
    // Suppressing the first takes the whole path away: the second path moves up to its id and
    // its own clicks follow it; unsuppressing brings the first back closed.
    run(&mut s, "timeline.suppressStep", json!({"index": 0, "on": true}));
    assert_eq!(steps(&mut s)[1]["created"], json!([id2]), "stored ids stay as recorded: {:?}", steps(&mut s));
    assert!(s.doc().unwrap().doc.node(NodeId(id2)).is_none(), "only one path now");
    assert!((width_of(&s, id) - 70.0).abs() < 1e-6, "the second path, 70 wide, under the first's id");
    run(&mut s, "timeline.suppressStep", json!({"index": 0, "on": false}));
    assert!((width_of(&s, id) - 50.0).abs() < 1e-6, "the first path is back");
    assert!((width_of(&s, id2) - 70.0).abs() < 1e-6, "the second is back at its own id");
    // The merged step survives save and open.
    let bytes = vectorcraft_format::save_file(&s.doc().unwrap().doc.clone());
    let back = vectorcraft_format::load_file(&bytes).unwrap().doc;
    let t = vectorcraft_engine::timeline::read(&back).unwrap();
    assert_eq!((t.steps.len(), t.steps[0].children.len()), (2, 4));
}

#[test]
fn a_sketch_session_of_ten_operations_is_one_step_and_a_later_session_another() {
    let mut s = session();
    run(&mut s, "file.new", json!({"width": 300, "height": 200}));
    let id = run(&mut s, "sketch.new", json!({}))["id"].as_u64().unwrap();
    run(&mut s, "sketch.addRect", json!({"id": id, "from": [10.0, 10.0], "to": [60.0, 40.0]}));
    for (cx, cy) in [(20.0, 20.0), (50.0, 20.0), (20.0, 30.0), (50.0, 30.0)] {
        run(&mut s, "sketch.addCircle", json!({"id": id, "center": [cx, cy], "diameter": 4}));
    }
    let line = run(&mut s, "sketch.addLine", json!({"id": id, "from": [10.0, 50.0], "to": [60.0, 50.0]}))["line"].as_u64().unwrap();
    run(&mut s, "sketch.addDimension", json!({"id": id, "constraint": {"type": "length", "line": line}}));
    run(&mut s, "sketch.setDimension", json!({"id": id, "name": "d1", "expression": "40"}));
    run(&mut s, "sketch.addConstraint", json!({"id": id, "constraint": {"type": "horizontal", "line": line}}));
    run(&mut s, "sketch.setConstruction", json!({"id": id, "entities": [line]}));
    let st = steps(&mut s);
    assert_eq!(st.len(), 1, "{st:?}");
    assert_eq!((st[0]["label"].as_str(), st[0]["children"].as_array().unwrap().len()), (Some("Sketch 1"), 10));
    // Something else, then more sketch work: a second sketch step.
    run(&mut s, "shape.rectangle", json!({"x": 100, "y": 100, "width": 20, "height": 20}));
    run(&mut s, "sketch.addLine", json!({"id": id, "from": [10.0, 60.0], "to": [60.0, 60.0]}));
    run(&mut s, "sketch.addLine", json!({"id": id, "from": [10.0, 70.0], "to": [60.0, 70.0]}));
    let st = steps(&mut s);
    assert_eq!(st.len(), 3, "{st:?}");
    assert_eq!(st[2]["children"].as_array().unwrap().len(), 1);
    // Suppress the first sketch step: its geometry goes; the sketch group itself goes too.
    run(&mut s, "timeline.suppressStep", json!({"index": 0, "on": true}));
    let st = steps(&mut s);
    assert_eq!(st[0]["suppressed"], true);
    run(&mut s, "timeline.suppressStep", json!({"index": 0, "on": false}));
    assert_eq!(run(&mut s, "sketch.inspect", json!({"id": id}))["entities"].as_array().unwrap().len(), 11, "4 + 4 + 1 + 2 entities");
}

#[test]
fn consecutive_appearance_changes_on_one_selection_merge_and_a_step_can_be_renamed() {
    let mut s = session();
    let (rect, _) = drawing(&mut s);
    run(&mut s, "paint.setStroke", json!({"color": "#0000ff"}));
    run(&mut s, "stroke.set", json!({"weight": 3}));
    let st = steps(&mut s);
    assert_eq!(st.len(), 4, "the three appearance edits on the offset path are one step: {st:?}");
    assert_eq!((st[3]["label"].as_str(), st[3]["children"].as_array().unwrap().len()), (Some("Appearance"), 2));
    // Another object: its own appearance step.
    run(&mut s, "select.set", json!({"ids": [rect]}));
    run(&mut s, "paint.setFill", json!({"color": "#00ff00"}));
    assert_eq!(steps(&mut s).len(), 5);
    // Rename: shown instead of the label, one undo step, no replay; null takes it away.
    let n = s.doc().unwrap().history.undo.len();
    run(&mut s, "timeline.renameStep", json!({"index": 3, "name": "Red outline"}));
    assert_eq!(s.doc().unwrap().history.undo.len(), n + 1);
    assert_eq!((steps(&mut s)[3]["label"].as_str(), steps(&mut s)[3]["name"].as_str()), (Some("Red outline"), Some("Red outline")));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(steps(&mut s)[3]["label"], "Appearance");
    run(&mut s, "timeline.renameStep", json!({"index": 3, "name": "x"}));
    run(&mut s, "timeline.renameStep", json!({"index": 3, "name": null}));
    assert_eq!(steps(&mut s)[3]["label"], "Appearance");
}

#[test]
fn suppressing_an_earlier_creation_keeps_later_steps_on_their_own_objects() {
    let mut s = session();
    run(&mut s, "file.new", json!({"width": 300, "height": 200}));
    let path = run(&mut s, "path.create", json!({"anchors": [{"x": 10, "y": 10}]}))["id"].as_u64().unwrap();
    run(&mut s, "path.appendAnchor", json!({"id": path, "x": 60, "y": 10}));
    let sketch = run(&mut s, "sketch.new", json!({}))["id"].as_u64().unwrap();
    run(&mut s, "sketch.addRect", json!({"id": sketch, "from": [10.0, 10.0], "to": [60.0, 40.0]}));
    run(&mut s, "sketch.addCircle", json!({"id": sketch, "center": [20.0, 20.0], "diameter": 4}));
    assert_eq!(steps(&mut s)[1]["created"][0], json!(sketch), "{:?}", steps(&mut s));
    // Suppress the pen path: the sketch is made again, under the path's old id, and its own
    // commands follow it there.
    run(&mut s, "timeline.suppressStep", json!({"index": 0, "on": true}));
    let st = steps(&mut s);
    assert_eq!((st[1]["status"].as_str(), st[1]["error"].as_str()), (Some("ok"), None), "{st:?}");
    assert!(s.doc().unwrap().doc.node(NodeId(sketch)).is_none(), "the sketch took the free id");
    assert_eq!(run(&mut s, "sketch.inspect", json!({"id": path}))["entities"].as_array().unwrap().len(), 5, "whole, under the path's old id");
    assert_eq!(st[1]["created"][0], json!(sketch), "the stored step keeps the ids it was recorded with");
    assert_eq!(st[1]["children"][0]["params"]["id"], json!(sketch));
    // Unsuppress: the path is back first, the sketch is at its own id again, still whole.
    run(&mut s, "timeline.suppressStep", json!({"index": 0, "on": false}));
    let st = steps(&mut s);
    assert_eq!(st[1]["status"], "ok", "{st:?}");
    assert_eq!(run(&mut s, "sketch.inspect", json!({"id": sketch}))["entities"].as_array().unwrap().len(), 5);
    assert!((width_of(&s, path) - 50.0).abs() < 1e-6, "the path is back");
}

// ---------------------------------------------------------------- E4 Part A: many steps at once

#[test]
fn delete_steps_removes_exactly_those_steps_in_one_undo_and_orphans_what_worked_on_them() {
    let mut s = session();
    drawing(&mut s);
    // rectangle · ellipse · select · offset · fill (the select is its own step or merged: count them)
    let before = cmds(&mut s);
    let ellipse = before.iter().position(|c| c == "shape.ellipse").unwrap();
    let fill = before.iter().position(|c| c == "paint.setFill").unwrap();
    let doc_before = doc_json(&s);
    let r = run(&mut s, "timeline.deleteSteps", json!({"indices": [fill, ellipse, ellipse]}));
    assert_eq!(r["steps"].as_u64().unwrap() as usize, before.len() - 2, "{r}");
    let after = cmds(&mut s);
    let mut want = before.clone();
    want.remove(fill);
    want.remove(ellipse);
    assert_eq!(after, want, "exactly the two chosen steps are gone, the rest in order");
    // rectangle and offset are left: the ellipse went with its step.
    assert_eq!(run(&mut s, "select.all", json!({}))["count"], json!(2));
    // One undo brings both back, the document as it was.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(cmds(&mut s), before);
    assert_eq!(doc_json(&s), doc_before);
    // Deleting the rectangle and the ellipse together orphans the offset that worked on the rectangle.
    let rect = before.iter().position(|c| c == "shape.rectangle").unwrap();
    run(&mut s, "timeline.deleteSteps", json!({"indices": [rect, ellipse]}));
    let st = steps(&mut s);
    let offset = st.iter().find(|v| v["cmd"] == "object.path.offsetPath").unwrap();
    assert_eq!(offset["orphaned"], json!(true), "{offset}");
    assert_eq!(offset["status"], "failed", "{offset}");
    // A bad index changes nothing.
    let n = steps(&mut s).len();
    assert!(s.execute("timeline.deleteSteps", &json!({"indices": [0, 99]})).is_err());
    assert!(s.execute("timeline.deleteSteps", &json!({"indices": []})).is_err());
    assert_eq!(steps(&mut s).len(), n);
}

#[test]
fn suppress_steps_skips_several_steps_in_one_undo() {
    let mut s = session();
    drawing(&mut s);
    let before = cmds(&mut s);
    let ellipse = before.iter().position(|c| c == "shape.ellipse").unwrap();
    let fill = before.iter().position(|c| c == "paint.setFill").unwrap();
    run(&mut s, "timeline.suppressSteps", json!({"indices": [ellipse, fill], "suppressed": true}));
    let st = steps(&mut s);
    let flags: Vec<bool> = st.iter().map(|v| v["suppressed"] == json!(true)).collect();
    assert!(flags[ellipse] && flags[fill] && flags.iter().filter(|f| **f).count() == 2, "{flags:?}");
    run(&mut s, "edit.undo", json!({}));
    assert!(steps(&mut s).iter().all(|v| v["suppressed"] != json!(true)), "one undo un-suppresses both");
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "timeline.suppressSteps", json!({"indices": [ellipse, fill], "suppressed": false}));
    assert!(steps(&mut s).iter().all(|v| v["suppressed"] != json!(true)));
}
