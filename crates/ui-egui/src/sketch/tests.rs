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
