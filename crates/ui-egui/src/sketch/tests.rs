//! Sketch mode through whole frames: the toolbar, drawing a line with the heads-up boxes, keys.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use serde_json::{Value, json};

use super::*;
use crate::canvas::Xf;

struct Rig {
    app: VectorcraftApp,
    ctx: egui::Context,
}

impl Rig {
    fn new() -> Rig {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width": 400, "height": 300})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut rig = Rig { app, ctx };
        for _ in 0..3 {
            rig.frame(vec![]);
        }
        rig
    }

    fn frame(&mut self, events: Vec<Event>) -> String {
        let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 850.0));
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(egui::RawInput { events, screen_rect: Some(screen), ..Default::default() }, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        crate::tests_labels::shapes_text(&out)
    }

    fn new_sketch(&mut self) -> NodeId {
        let id = new_sketch(&mut self.app).unwrap()["id"].as_u64().unwrap();
        self.frame(vec![]);
        NodeId(id)
    }

    /// The screen point of sketch place `mm`.
    fn at(&self, mm: Mm) -> Pos2 {
        let id = self.app.sketch.as_ref().unwrap().id;
        let (sketch, _) = read(&self.app, id).unwrap();
        let xf = Xf::new(self.app.canvas_rect.unwrap(), self.app.view().unwrap());
        to_screen(&xf, &sketch, mm)
    }

    fn mv(&mut self, mm: Mm) -> String {
        let p = self.at(mm);
        self.frame(vec![Event::PointerMoved(p)])
    }

    fn click(&mut self, mm: Mm) {
        let p = self.at(mm);
        self.frame(vec![Event::PointerMoved(p)]);
        let button = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
    }

    fn button(&mut self, p: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// Two clicks as a double-click: each event in its own frame, or all four in one frame (what
    /// automation and a fast trackpad send).
    fn double_click(&mut self, mm: Mm, one_frame: bool) {
        let p = self.at(mm);
        let (down, up) = (self.button(p, true), self.button(p, false));
        if one_frame {
            self.frame(vec![Event::PointerMoved(p), down.clone(), up.clone(), down, up]);
        } else {
            self.frame(vec![Event::PointerMoved(p)]);
            for e in [down.clone(), up.clone(), down, up] {
                self.frame(vec![e]);
            }
        }
    }

    fn key(&mut self, key: Key) -> String {
        let e = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![e(true), e(false)])
    }

    /// Type `text` one character a frame, as a keyboard does (a key press and its text).
    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            let key = Key::from_name(&c.to_ascii_uppercase().to_string());
            let mut events = vec![];
            if let Some(key) = key {
                events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
            }
            events.push(Event::Text(c.to_string()));
            self.frame(events);
        }
    }

    fn inspect(&mut self, id: NodeId) -> Value {
        self.app.session.execute("sketch.inspect", &json!({"id": id.0})).unwrap()
    }
}

#[test]
fn the_toolbar_shows_its_groups_in_sketch_mode() {
    let mut rig = Rig::new();
    let before = rig.frame(vec![]);
    assert!(!before.contains("FINISH SKETCH"), "{before}");
    rig.new_sketch();
    assert!(active(&rig.app));
    let text = rig.frame(vec![]);
    for want in ["CREATE", "MODIFY", "CONSTRAINTS", "FINISH SKETCH"] {
        assert!(text.contains(want), "{want} in {text}");
    }
}

#[test]
fn drawing_a_line_paints_the_length_and_angle_boxes() {
    let mut rig = Rig::new();
    rig.new_sketch();
    rig.type_text("l");
    assert_eq!(rig.app.sketch.as_ref().unwrap().tool, SketchTool::Line);
    rig.mv([20.0, 20.0]);
    rig.click([20.0, 20.0]);
    let text = rig.mv([60.0, 30.0]);
    let text = format!("{text}{}", rig.mv([60.0, 30.0]));
    for want in ["length", "angle", "41.2", "14.0"] {
        assert!(text.contains(want), "{want} in {text}");
    }
}

#[test]
fn typing_50_tab_30_enter_commits_a_line() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.type_text("l");
    rig.mv([20.0, 20.0]);
    rig.click([20.0, 20.0]);
    rig.mv([60.0, 60.0]);
    rig.type_text("50");
    rig.key(Key::Tab);
    rig.type_text("30");
    rig.key(Key::Enter);
    let r = rig.inspect(id);
    let lines: Vec<&Value> = r["entities"].as_array().unwrap().iter().filter(|e| e["kind"] == "line").collect();
    assert_eq!(lines.len(), 1, "{r}");
    let dims: Vec<&Value> = r["constraints"].as_array().unwrap().iter().filter(|c| c.get("param").is_some()).collect();
    let names: Vec<&str> = dims.iter().map(|c| c["param"].as_str().unwrap()).collect();
    assert_eq!(names, ["d1", "d2"], "{r}");
    let value = |n: &str| dims.iter().find(|c| c["param"] == n).unwrap()["value"].as_f64().unwrap();
    assert!((value("d1") - 50.0).abs() < 1e-6 && (value("d2") - 30.0).abs() < 1e-6, "{r}");
    let rows = rig.app.session.execute("parameters.list", &json!({})).unwrap();
    let rows = rows["rows"].as_array().unwrap();
    let unit = |n: &str| rows.iter().find(|p| p["name"] == n).unwrap()["unit"].as_str().unwrap().to_string();
    assert_eq!((unit("d1"), unit("d2")), ("mm".to_string(), "deg".to_string()));
    // The chain goes on from the line's end, with fresh boxes.
    assert!(rig.app.sketch.as_ref().unwrap().start.is_some());
    assert!(!rig.app.sketch.as_ref().unwrap().hud.as_ref().unwrap().any_typed());
}

#[test]
fn sketch_keys_do_not_leak_to_tools_and_outside_sketch_mode_nothing_changes() {
    let mut rig = Rig::new();
    let before = rig.app.session.tool_id().to_string();
    rig.new_sketch();
    rig.type_text("l");
    assert_eq!(rig.app.session.tool_id(), before, "L is the sketch's Line tool, not the Ellipse tool");
    assert_eq!(rig.app.sketch.as_ref().unwrap().tool, SketchTool::Line);
    rig.type_text("r");
    assert_eq!(rig.app.sketch.as_ref().unwrap().tool, SketchTool::Rect2);
    finish(&mut rig.app);
    rig.frame(vec![]);
    assert!(!active(&rig.app));
    rig.type_text("l");
    assert_eq!(rig.app.session.tool_id(), "ellipse", "after Finish, L selects the Ellipse tool as before");
}

#[test]
fn escape_finishes_the_sketch_and_selects_it() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    assert_eq!(rig.app.session.active().unwrap().isolation, Some(id));
    rig.key(Key::Escape);
    assert!(rig.app.sketch.is_none());
    let st = rig.app.session.active().unwrap();
    assert_eq!(st.isolation, None);
    assert_eq!(st.selection.objects, [id]);
}

#[test]
fn escape_goes_one_level_at_a_time() {
    let mut rig = Rig::new();
    rig.new_sketch();
    rig.type_text("l");
    rig.mv([20.0, 20.0]);
    rig.click([20.0, 20.0]);
    rig.mv([50.0, 40.0]);
    rig.type_text("5");
    rig.key(Key::Escape);
    let m = rig.app.sketch.as_ref().unwrap();
    assert!(m.start.is_some() && !m.hud.as_ref().unwrap().any_typed(), "the typed text went");
    rig.key(Key::Escape);
    assert!(rig.app.sketch.as_ref().unwrap().start.is_none(), "then the rubber band");
    rig.key(Key::Escape);
    assert_eq!(rig.app.sketch.as_ref().unwrap().tool, SketchTool::None, "then the tool");
    rig.key(Key::Escape);
    assert!(rig.app.sketch.is_none(), "then the sketch");
}

#[test]
fn double_clicking_a_dimension_edits_it() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.app.run("sketch.addLine", json!({"id": id.0, "from": [10.0, 40.0], "to": [60.0, 40.0], "length": "50"})).unwrap();
    rig.frame(vec![]);
    let cache = snapshot(&mut rig.app).unwrap();
    let xf = Xf::new(rig.app.canvas_rect.unwrap(), rig.app.view().unwrap());
    let geom = overlay::dimension_geoms(&cache, &xf).into_iter().next().expect("the length is drawn as a dimension");
    assert_eq!(geom.text, "50");
    assert_eq!(hit::dim_at(&cache, &xf, geom.label), Some(geom.index));
    let input = PointerInput { pos: Some(geom.label), double: true, ..Default::default() };
    handle_pointer(&mut rig.app, &xf, input);
    let edit = rig.app.sketch.as_ref().unwrap().edit_dim.as_ref().expect("the in-place box opened");
    assert_eq!(edit.1.boxes[0].text, "50 mm", "prefilled with the expression");
    // The box is on the canvas, and takes the keys.
    let painted = rig.frame(vec![]);
    assert!(painted.contains("50 mm"), "{painted}");
    rig.type_text("80");
    rig.key(Key::Enter);
    assert!(rig.app.sketch.as_ref().unwrap().edit_dim.is_none());
    let r = rig.inspect(id);
    let dim = r["constraints"].as_array().unwrap().iter().find(|c| c["param"] == "d1").unwrap();
    assert_eq!(dim["value"].as_f64(), Some(80.0), "{r}");
    // A bad value keeps the box and says why.
    handle_pointer(&mut rig.app, &xf, input);
    rig.type_text("nope");
    rig.key(Key::Enter);
    assert!(rig.app.sketch.as_ref().unwrap().edit_dim.is_some());
}

#[test]
fn undoing_the_new_sketch_leaves_sketch_mode() {
    let mut rig = Rig::new();
    rig.new_sketch();
    rig.app.session.execute("edit.undo", &json!({})).unwrap();
    rig.frame(vec![]);
    rig.frame(vec![]);
    assert!(!active(&rig.app));
    assert!(rig.app.sketch.is_none());
}

fn dims(r: &Value) -> Vec<(String, f64)> {
    r["constraints"].as_array().unwrap().iter().filter_map(|c| Some((c.get("param")?.as_str()?.to_string(), c["value"].as_f64()?))).collect()
}

fn kinds(r: &Value, kind: &str) -> usize {
    r["entities"].as_array().unwrap().iter().filter(|e| e["kind"] == kind).count()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn the_rubber_band_follows_typed_and_locked_values() {
    let mut rig = Rig::new();
    rig.new_sketch();
    rig.type_text("l");
    rig.mv([20.0, 20.0]);
    rig.click([20.0, 20.0]);
    rig.mv([80.0, 80.0]);
    rig.type_text("50");
    rig.key(Key::Tab);
    rig.type_text("30");
    rig.mv([90.0, 20.0]);
    let end = rig.app.sketch.as_ref().unwrap().rubber.unwrap();
    let want = tools::polar([20.0, 20.0], 50.0, 30.0);
    assert!(near(end[0], want[0]) && near(end[1], want[1]), "{end:?} vs {want:?}");
    // Only the untyped quantity follows the mouse: length typed, the angle is the mouse's.
    let mut rig = Rig::new();
    rig.new_sketch();
    rig.type_text("l");
    rig.mv([20.0, 20.0]);
    rig.click([20.0, 20.0]);
    rig.mv([20.0, 70.0]);
    rig.type_text("30");
    rig.mv([20.0, 90.0]);
    let end = rig.app.sketch.as_ref().unwrap().rubber.unwrap();
    assert!(near(end[0], 20.0) && near(end[1], 50.0), "30 mm straight down: {end:?}");
}

#[test]
fn the_rectangle_rubber_band_follows_the_typed_width() {
    let mut rig = Rig::new();
    rig.new_sketch();
    rig.type_text("r");
    rig.mv([20.0, 20.0]);
    rig.click([20.0, 20.0]);
    rig.mv([50.0, 60.0]);
    rig.type_text("40");
    rig.mv([55.0, 62.0]);
    let end = rig.app.sketch.as_ref().unwrap().rubber.unwrap();
    assert!(near(end[0], 60.0) && near(end[1], 62.0), "{end:?}");
}

#[test]
fn double_clicking_a_dimension_edits_it_through_whole_frames() {
    for one_frame in [false, true] {
        let mut rig = Rig::new();
        let id = rig.new_sketch();
        rig.app.run("sketch.addLine", json!({"id": id.0, "from": [10.0, 40.0], "to": [60.0, 40.0], "length": "50"})).unwrap();
        rig.frame(vec![]);
        let cache = snapshot(&mut rig.app).unwrap();
        let xf = Xf::new(rig.app.canvas_rect.unwrap(), rig.app.view().unwrap());
        let geom = overlay::dimension_geoms(&cache, &xf).into_iter().next().unwrap();
        let screen = geom.label;
        let at = Mm::from(to_mm(&xf, &cache.sketch, screen));
        rig.double_click(at, one_frame);
        let edit = rig.app.sketch.as_ref().unwrap().edit_dim.as_ref().unwrap_or_else(|| panic!("the box opened (one frame: {one_frame})"));
        assert_eq!(edit.1.boxes[0].text, "50 mm");
        let painted = rig.frame(vec![]);
        assert!(painted.contains("50 mm"), "{painted}");
        rig.type_text("70");
        let painted = rig.frame(vec![]);
        assert!(painted.contains("70"), "the typed text shows: {painted}");
        rig.key(Key::Enter);
        assert!(rig.app.sketch.as_ref().unwrap().edit_dim.is_none());
        let r = rig.inspect(id);
        assert!(dims(&r).iter().any(|(n, v)| n == "d1" && near(*v, 70.0)), "{r}");
    }
}

#[test]
fn cmd_a_selects_the_sketch_entities_and_not_the_page() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.app.run("sketch.addLine", json!({"id": id.0, "from": [10.0, 40.0], "to": [60.0, 40.0]})).unwrap();
    rig.app.run("sketch.addLine", json!({"id": id.0, "from": [10.0, 50.0], "to": [60.0, 70.0]})).unwrap();
    rig.frame(vec![]);
    let before = rig.app.session.active().unwrap().selection.objects.clone();
    let e = Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND };
    rig.frame(vec![e]);
    assert_eq!(rig.app.sketch.as_ref().unwrap().selected.len(), 2, "both lines");
    assert_eq!(rig.app.session.active().unwrap().selection.objects, before, "the page selection did not change");
}

#[test]
fn the_hint_bar_shows_the_sketch_tools_hint() {
    let mut rig = Rig::new();
    rig.new_sketch();
    let text = rig.frame(vec![]).replace('\n', "");
    assert!(text.contains("Pick a sketch tool"), "{text}");
    assert!(!text.contains("to select multiple objects"), "{text}");
    rig.type_text("l");
    let text = rig.frame(vec![]).replace('\n', "");
    assert!(text.contains("Click to start a line"), "{text}");
}

#[test]
fn rectangle_tool_typed_width_and_height_commit_one_rectangle() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.type_text("r");
    rig.mv([20.0, 20.0]);
    rig.click([20.0, 20.0]);
    rig.mv([50.0, 50.0]);
    rig.type_text("40");
    rig.key(Key::Tab);
    rig.type_text("25");
    rig.key(Key::Enter);
    let r = rig.inspect(id);
    assert_eq!(kinds(&r, "line"), 4, "{r}");
    let d = dims(&r);
    assert!(d.iter().any(|(_, v)| near(*v, 40.0)) && d.iter().any(|(_, v)| near(*v, 25.0)), "{r}");
    assert!(rig.app.sketch.as_ref().unwrap().start.is_none(), "the gesture is over");
}

#[test]
fn center_rectangle_commits_around_the_centre() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    set_tool(&mut rig.app, SketchTool::RectCenter);
    rig.mv([50.0, 40.0]);
    rig.click([50.0, 40.0]);
    rig.mv([60.0, 50.0]);
    rig.type_text("40");
    rig.key(Key::Tab);
    rig.type_text("20");
    rig.key(Key::Enter);
    let r = rig.inspect(id);
    let d = dims(&r);
    assert!(d.iter().any(|(_, v)| near(*v, 40.0)) && d.iter().any(|(_, v)| near(*v, 20.0)), "{r}");
    let pts = r["points"].as_array().unwrap();
    let has = |x: f64, y: f64| pts.iter().any(|p| near(p["x"].as_f64().unwrap(), x) && near(p["y"].as_f64().unwrap(), y));
    assert!(has(30.0, 30.0) && has(70.0, 50.0) && has(50.0, 40.0), "corners around the centre: {r}");
}

#[test]
fn circle_tool_typed_diameter() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.type_text("c");
    assert_eq!(rig.app.sketch.as_ref().unwrap().tool, SketchTool::Circle);
    rig.mv([50.0, 50.0]);
    rig.click([50.0, 50.0]);
    rig.mv([60.0, 50.0]);
    rig.type_text("20");
    rig.key(Key::Enter);
    let r = rig.inspect(id);
    assert_eq!(kinds(&r, "circle"), 1, "{r}");
    assert!(dims(&r).iter().any(|(_, v)| near(*v, 20.0)), "a diameter dimension of 20: {r}");
    assert!(r["constraints"].as_array().unwrap().iter().any(|c| c["type"] == "diameter"), "{r}");
}

#[test]
fn arc_tool_typed_radius() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    set_tool(&mut rig.app, SketchTool::Arc3);
    rig.mv([20.0, 40.0]);
    rig.click([20.0, 40.0]);
    rig.mv([60.0, 40.0]);
    rig.click([60.0, 40.0]);
    assert!(rig.app.sketch.as_ref().unwrap().second.is_some());
    rig.mv([40.0, 20.0]);
    rig.type_text("25");
    rig.mv([40.0, 18.0]);
    rig.key(Key::Enter);
    let r = rig.inspect(id);
    assert_eq!(kinds(&r, "arc"), 1, "{r}");
    assert!(dims(&r).iter().any(|(_, v)| near(*v, 25.0)), "a radius dimension of 25: {r}");
}

#[test]
fn fillet_tool_rounds_a_rectangle_corner() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.app.run("sketch.addRect", json!({"id": id.0, "from": [20.0, 20.0], "to": [80.0, 60.0]})).unwrap();
    rig.frame(vec![]);
    set_tool(&mut rig.app, SketchTool::Fillet);
    rig.mv([20.0, 20.0]);
    rig.click([20.0, 20.0]);
    assert!(rig.app.sketch.as_ref().unwrap().start.is_some(), "the corner is picked");
    rig.type_text("5");
    rig.key(Key::Enter);
    let r = rig.inspect(id);
    assert_eq!(kinds(&r, "arc"), 1, "{r}");
    assert!(dims(&r).iter().any(|(_, v)| near(*v, 5.0)), "a radius dimension of 5: {r}");
}

#[test]
fn fillet_without_a_radius_says_so() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.app.run("sketch.addRect", json!({"id": id.0, "from": [20.0, 20.0], "to": [80.0, 60.0]})).unwrap();
    rig.frame(vec![]);
    set_tool(&mut rig.app, SketchTool::Fillet);
    rig.mv([20.0, 20.0]);
    rig.click([20.0, 20.0]);
    rig.key(Key::Enter);
    assert_eq!(kinds(&rig.inspect(id), "arc"), 0);
    assert!(rig.app.ui.status.contains("radius"), "{}", rig.app.ui.status);
}

/// Frames through the host's input hook (one synthetic event a frame, as the control channel's
/// `ui.click` and `ui.key` are fed in the real app) until the queue is drained.
fn drain(rig: &mut Rig) {
    for _ in 0..40 {
        let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 850.0));
        let mut raw = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let done = rig.app.synthetic.is_empty();
        rig.app.raw_input_hook(&mut raw);
        let app = &mut rig.app;
        let mut out = rig.ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        if done {
            return;
        }
    }
    panic!("the synthetic queue did not drain");
}

#[test]
fn double_click_edit_works_through_the_control_channel() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.app.run("sketch.addLine", json!({"id": id.0, "from": [10.0, 40.0], "to": [60.0, 40.0], "length": "50"})).unwrap();
    rig.frame(vec![]);
    let cache = snapshot(&mut rig.app).unwrap();
    let xf = Xf::new(rig.app.canvas_rect.unwrap(), rig.app.view().unwrap());
    let label = overlay::dimension_geoms(&cache, &xf).into_iter().next().unwrap().label;
    let send = |rig: &mut Rig, method: &str, params: Value| {
        crate::tests_synthetic::control(&mut rig.app, &rig.ctx, method, params);
        drain(rig);
    };
    send(&mut rig, "ui.click", json!({"x": label.x, "y": label.y, "count": 2}));
    assert!(rig.app.sketch.as_ref().unwrap().edit_dim.is_some(), "the box opened");
    for (key, text) in [("7", "7"), ("0", "0")] {
        send(&mut rig, "ui.key", json!({"key": key, "text": text}));
    }
    send(&mut rig, "ui.key", json!({"key": "Enter"}));
    let r = rig.inspect(id);
    assert!(dims(&r).iter().any(|(n, v)| n == "d1" && near(*v, 70.0)), "{r}");
}

// ---- Phase 6: constraints, the dimension tool, glyphs, colours ----

impl Rig {
    fn line(&mut self, id: NodeId, from: Mm, to: Mm) -> (u32, u32, u32) {
        let r = self.app.run("sketch.addLine", json!({"id": id.0, "from": from, "to": to})).unwrap();
        self.frame(vec![]);
        let n = |k: &str| r[k].as_u64().unwrap() as u32;
        (n("line"), n("a"), n("b"))
    }

    fn click_px(&mut self, p: Pos2) {
        self.frame(vec![Event::PointerMoved(p)]);
        let e = self.button(p, true);
        self.frame(vec![e]);
        let e = self.button(p, false);
        self.frame(vec![e]);
    }

    fn select(&mut self, picks: Vec<Pick>) {
        self.app.sketch.as_mut().unwrap().selected = picks;
    }

    fn cache(&mut self) -> Arc<Cache> {
        snapshot(&mut self.app).unwrap()
    }

    fn xf(&self) -> Xf {
        Xf::new(self.app.canvas_rect.unwrap(), self.app.view().unwrap())
    }

    fn constraint_types(&mut self, id: NodeId) -> Vec<String> {
        self.inspect(id)["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| !(c["type"] == "fix" && c["p"] == 0))
            .map(|c| c["type"].as_str().unwrap().to_string())
            .collect()
    }
}

#[test]
fn constraint_buttons_apply_to_a_valid_selection_and_refuse_an_invalid_one() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    let (l1, a1, _) = rig.line(id, [20.0, 20.0], [80.0, 30.0]);
    let (l2, ..) = rig.line(id, [20.0, 60.0], [30.0, 100.0]);
    rig.select(vec![Pick::Point(a1)]);
    constrain(&mut rig.app, "parallel");
    assert!(rig.app.ui.status.contains("Select"), "{}", rig.app.ui.status);
    assert!(rig.constraint_types(id).is_empty(), "{}", rig.inspect(id));
    rig.select(vec![Pick::Entity(l1), Pick::Entity(l2)]);
    constrain(&mut rig.app, "perpendicular");
    assert_eq!(rig.constraint_types(id), ["perpendicular"]);
    // Fix pins a line's ends, and a second press lets them go.
    rig.select(vec![Pick::Entity(l1)]);
    constrain(&mut rig.app, "fix");
    assert_eq!(rig.constraint_types(id).iter().filter(|t| *t == "fix").count(), 2);
    constrain(&mut rig.app, "fix");
    assert_eq!(rig.constraint_types(id), ["perpendicular"]);
}

#[test]
fn the_d_tool_on_a_line_adds_a_length_dimension() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.line(id, [20.0, 20.0], [80.0, 20.0]);
    rig.type_text("d");
    assert_eq!(rig.app.sketch.as_ref().unwrap().tool, SketchTool::Dimension);
    rig.click([50.0, 20.0]);
    let (_, hud) = rig.app.sketch.as_ref().unwrap().new_dim.clone().expect("the box is up");
    assert_eq!(hud.boxes[0].text, "60");
    rig.type_text("45");
    rig.key(Key::Enter);
    let r = rig.inspect(id);
    assert!(dims(&r).iter().any(|(_, v)| near(*v, 45.0)), "{r}");
    assert!(rig.app.sketch.as_ref().unwrap().new_dim.is_none());
}

#[test]
fn colours_follow_the_fully_constrained_flag() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    let (fixed, a, _) = rig.line(id, [20.0, 20.0], [80.0, 20.0]);
    let (free, ..) = rig.line(id, [20.0, 60.0], [80.0, 90.0]);
    for c in [json!({"type": "fix", "p": a}), json!({"type": "horizontal", "line": fixed})] {
        rig.app.run("sketch.addConstraint", json!({"id": id.0, "constraint": c})).unwrap();
    }
    rig.app.run("sketch.addDimension", json!({"id": id.0, "constraint": {"type": "length", "line": fixed}})).unwrap();
    let cache = rig.cache();
    assert_eq!(overlay::ink(cache.constrained.contains(&fixed)), egui::Color32::from_rgb(0x1a, 0x1a, 0x1a));
    assert_eq!(overlay::ink(cache.constrained.contains(&free)), egui::Color32::from_rgb(0x1f, 0x6f, 0xeb));
    assert!(cache.dof.is_some_and(|d| d > 0), "the free line leaves freedom");
}

#[test]
fn deleting_a_glyph_removes_its_constraint_in_one_undo_step() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    let (l, ..) = rig.line(id, [20.0, 20.0], [80.0, 30.0]);
    rig.app.run("sketch.addConstraint", json!({"id": id.0, "constraint": {"type": "horizontal", "line": l}})).unwrap();
    rig.frame(vec![]);
    let cache = rig.cache();
    let glyphs = overlay::glyph_geoms(&cache, &rig.xf());
    assert_eq!(glyphs.len(), 1, "{}", rig.inspect(id));
    rig.mv([0.0, 0.0]);
    rig.click_px(glyphs[0].rect.center());
    assert_eq!(rig.app.sketch.as_ref().unwrap().selected, [Pick::Glyph(glyphs[0].index)]);
    rig.key(Key::Delete);
    assert!(rig.constraint_types(id).is_empty());
    rig.app.session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(rig.constraint_types(id), ["horizontal"], "one undo brings it back");
}

#[test]
fn an_over_constraining_dimension_is_refused_with_a_status_message() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.app.run("sketch.addLine", json!({"id": id.0, "from": [20.0, 20.0], "to": [60.0, 20.0], "length": 40})).unwrap();
    rig.frame(vec![]);
    rig.type_text("d");
    rig.click([40.0, 20.0]);
    rig.key(Key::Enter);
    assert_eq!(rig.app.ui.status, "This would over-constrain the sketch");
    assert_eq!(dims(&rig.inspect(id)).len(), 1, "nothing changed");
}

#[test]
fn x_toggles_construction_on_the_selected_entities() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    let (l, ..) = rig.line(id, [20.0, 20.0], [80.0, 30.0]);
    rig.select(vec![Pick::Entity(l)]);
    rig.type_text("x");
    assert_eq!(rig.inspect(id)["entities"][0]["construction"], true);
    rig.type_text("x");
    assert_eq!(rig.inspect(id)["entities"][0]["construction"], false);
}

#[test]
fn trim_tool_click_trims() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.app.run("sketch.addRect", json!({"id": id.0, "from": [20.0, 20.0], "to": [80.0, 60.0]})).unwrap();
    let (cross, ..) = rig.line(id, [0.0, 40.0], [100.0, 40.0]);
    rig.frame(vec![]);
    rig.type_text("t");
    assert_eq!(rig.app.sketch.as_ref().unwrap().tool, SketchTool::Trim);
    // Hovering the middle of the crossing line previews the piece between the two sides.
    rig.mv([50.0, 40.0]);
    let m = rig.app.sketch.as_ref().unwrap();
    assert_eq!(m.hover, Some(Pick::Entity(cross)));
    let cache = rig.cache();
    let plan = cache.sketch.trim_plan(cross, (50.0, 40.0)).unwrap();
    assert_eq!((plan.from, plan.to), ((20.0, 40.0), (80.0, 40.0)));
    let before = rig.app.session.active().unwrap().history.undo.len();
    rig.click([50.0, 40.0]);
    assert_eq!(rig.app.session.active().unwrap().history.undo.len(), before + 1);
    let r = rig.inspect(id);
    let lines = r["entities"].as_array().unwrap().iter().filter(|e| e["kind"] == "line").count();
    assert_eq!(lines, 6, "the rectangle and two pieces: {r}");
    assert!(rig.app.sketch.as_ref().unwrap().tool == SketchTool::Trim, "the tool stays");
    // A circle is refused with the engine's message.
    let circle = rig.app.run("sketch.addCircle", json!({"id": id.0, "center": [50.0, 100.0], "diameter": 20})).unwrap();
    assert!(circle["circle"].is_u64());
    rig.frame(vec![]);
    rig.mv([60.0, 100.0]);
    rig.click([60.0, 100.0]);
    assert!(rig.app.ui.status.contains("lines"), "{}", rig.app.ui.status);
}

#[test]
fn offset_tool_typed_distance() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    rig.app.run("sketch.addRect", json!({"id": id.0, "from": [20.0, 20.0], "to": [80.0, 60.0]})).unwrap();
    rig.frame(vec![]);
    rig.type_text("o");
    assert_eq!(rig.app.sketch.as_ref().unwrap().tool, SketchTool::Offset);
    // A click on one line takes the whole loop and opens the distance box.
    rig.mv([50.0, 20.0]);
    rig.click([50.0, 20.0]);
    assert_eq!(rig.app.sketch.as_ref().unwrap().modify.len(), 4);
    // The mouse inside the rectangle picks the inside; the box shows its distance.
    let text = rig.mv([50.0, 40.0]);
    assert!(text.contains("distance"), "{text}");
    assert_eq!(rig.app.sketch.as_ref().unwrap().side, -1.0);
    rig.type_text("5");
    rig.key(Key::Enter);
    let r = rig.inspect(id);
    let lines = r["entities"].as_array().unwrap().iter().filter(|e| e["kind"] == "line").count();
    assert_eq!(lines, 8, "{r}");
    let cs = r["constraints"].as_array().unwrap();
    assert_eq!(cs.iter().filter(|c| c["type"] == "parallel").count(), 4);
    let dim = cs.iter().find(|c| c["type"] == "distancePointLine").expect("a distance dimension");
    assert!(near(dim["value"].as_f64().unwrap(), 5.0), "{dim}");
    // The inner rectangle's corner is 5 mm in.
    assert!(r["points"].as_array().unwrap().iter().any(|p| near(p["x"].as_f64().unwrap(), 25.0) && near(p["y"].as_f64().unwrap(), 25.0)), "{r}");
    assert!(rig.app.sketch.as_ref().unwrap().modify.is_empty(), "ready for the next offset");
}

#[test]
fn mirror_tool_mirrors_the_selection_about_a_clicked_line() {
    let mut rig = Rig::new();
    let id = rig.new_sketch();
    let (a, ..) = rig.line(id, [20.0, 20.0], [40.0, 30.0]);
    let (axis, ..) = rig.line(id, [60.0, 0.0], [60.0, 80.0]);
    rig.frame(vec![]);
    // Nothing selected: the tool does not start.
    set_tool(&mut rig.app, SketchTool::Mirror);
    assert_eq!(rig.app.sketch.as_ref().unwrap().tool, SketchTool::None);
    rig.app.sketch.as_mut().unwrap().selected = vec![Pick::Entity(a)];
    set_tool(&mut rig.app, SketchTool::Mirror);
    rig.mv([60.0, 50.0]);
    rig.click([60.0, 50.0]);
    let r = rig.inspect(id);
    assert_eq!(r["entities"].as_array().unwrap().iter().filter(|e| e["kind"] == "line").count(), 3, "{r}");
    assert_eq!(r["constraints"].as_array().unwrap().iter().filter(|c| c["type"] == "symmetric" && c["line"] == axis).count(), 2);
}
