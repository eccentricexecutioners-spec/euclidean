//! The Fusion workbench through whole headless frames.

use egui::{Event, Modifiers, PointerButton, Pos2};
use serde_json::json;

use crate::VectorcraftApp;
use crate::state::{Layout, UiState};

struct Rig {
    app: VectorcraftApp,
    ctx: egui::Context,
}

impl Rig {
    fn new(layout: Layout) -> Rig {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width": 400, "height": 300})).unwrap();
        app.ui.layout = layout;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut rig = Rig { app, ctx };
        rig.settle();
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
        // The status bar's timing line differs from frame to frame.
        crate::tests_labels::shapes_text(&out).lines().filter(|l| !l.starts_with("render ")).collect::<Vec<_>>().join("\n")
    }

    fn settle(&mut self) -> String {
        let mut text = String::new();
        for _ in 0..4 {
            text = self.frame(vec![]);
        }
        text
    }

    fn hit(&self, name: &str) -> Pos2 {
        let found = self.app.ui.workbench.hits.iter().find(|h| h.0 == name);
        found.unwrap_or_else(|| panic!("no hit '{name}' in {:?}", self.app.ui.workbench.hits.iter().map(|h| &h.0).collect::<Vec<_>>())).1.center()
    }

    /// Click the drawn button `name`, then let the frames settle; the text of the last frame.
    fn click(&mut self, name: &str) -> String {
        let p = self.hit(name);
        self.frame(vec![Event::PointerMoved(p)]);
        let button = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
        self.settle()
    }

    fn tool(&self) -> String {
        self.app.session.tool_id().to_string()
    }
}

const CLASSIC_MARKERS: [&str; 1] = ["Libraries"];

#[test]
fn fusion_shows_the_tab_row_and_the_active_tabs_groups() {
    let mut rig = Rig::new(Layout::Fusion);
    let text = rig.settle();
    for want in ["DRAW", "SKETCH", "TYPE", "OUTPUT", "SELECT", "CREATE", "MODIFY", "STYLE", "INSPECT", "INSERT"] {
        assert!(text.contains(want), "missing {want}:\n{text}");
    }
}

#[test]
fn fusion_hides_the_tool_column_control_bar_and_dock() {
    let mut classic = Rig::new(Layout::Classic);
    let c = classic.settle();
    let mut fusion = Rig::new(Layout::Fusion);
    let f = fusion.settle();
    for m in CLASSIC_MARKERS {
        assert!(c.contains(m), "Classic lacks {m}:\n{c}");
        assert!(!f.contains(m), "Fusion still draws {m}:\n{f}");
    }
    assert!(fusion.app.ui.dock && fusion.app.ui.toolbar, "the Classic switches stay on");
}

#[test]
fn clicking_a_groups_big_button_selects_its_tool() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.click("group:create");
    assert_eq!(rig.tool(), "pen");
    rig.click("group:modify");
    assert_eq!(rig.tool(), "addAnchor");
}

#[test]
fn the_dropdown_lists_every_item_of_the_group() {
    let mut rig = Rig::new(Layout::Fusion);
    let text = rig.click("label:modify");
    for want in ["Scissors Tool", "Pathfinder", "Group", "Ungroup", "Puppet Warp Tool"] {
        assert!(text.contains(want), "missing {want}:\n{text}");
    }
}

#[test]
fn classic_draws_what_it_drew_before() {
    // The else branch of `ui_frame` is the code of commit 70b66067, byte for byte but for one more
    // level of indentation.
    let mut rig = Rig::new(Layout::Classic);
    let before = rig.settle();
    for m in CLASSIC_MARKERS {
        assert!(before.contains(m), "{m}");
    }
    rig.app.run("view.layout", json!({"layout": "fusion"})).unwrap();
    let fusion = rig.settle();
    assert!(fusion.contains("DRAW"));
    rig.app.run("view.layout", json!({"layout": "classic"})).unwrap();
    let after = rig.settle();
    assert_eq!(before, after);
}

#[test]
fn layers_and_colour_are_pinned_in_every_tab() {
    let mut rig = Rig::new(Layout::Fusion);
    for tab in ["draw", "sketch", "type", "output"] {
        rig.app.ui.workbench.tab = tab.into();
        let text = rig.settle();
        assert!(text.contains("Layers") && text.contains("Colour"), "{tab}:\n{text}");
    }
}

#[test]
fn each_tab_shows_its_own_left_panels() {
    let mut rig = Rig::new(Layout::Fusion);
    for (tab, want) in [("draw", "Properties"), ("sketch", "Parameters"), ("type", "Paragraph"), ("output", "Artboards")] {
        rig.app.ui.workbench.tab = tab.into();
        let text = rig.settle();
        assert!(text.contains(want), "{tab} lacks {want}:\n{text}");
        let names = rig.app.ui.workbench.hits.iter().map(|h| h.0.clone()).collect::<Vec<_>>();
        assert!(names.iter().any(|n| n.starts_with("section:")), "{names:?}");
    }
    rig.app.ui.workbench.tab = "draw".into();
    let text = rig.settle();
    assert!(!text.contains("Parameters"), "{text}");
}

#[test]
fn collapsing_a_section_persists() {
    let mut rig = Rig::new(Layout::Fusion);
    let before = rig.settle();
    assert!(before.contains("Layers"));
    rig.click("section:layers");
    assert!(rig.app.ui.workbench.collapsed.contains("layers"), "{:?}", rig.app.ui.workbench.collapsed);
    let after = rig.settle();
    // (Not a length comparison: the timeline strip shortens the column, so folding Layers also brings Swatches into view.)
    assert!(before.contains("Search All") && !after.contains("Search All"), "the body should be gone");
    assert!(after.contains("Layers"), "the header stays");
    let saved = serde_json::to_string(&rig.app.ui).unwrap();
    let back: UiState = serde_json::from_str(&saved).unwrap();
    assert!(back.workbench.collapsed.contains("layers"));
    // A panel a section folds by default is flipped open by a click, and back.
    assert!(!rig.app.ui.workbench.collapsed.contains("colour/colorGuide"));
    // Unfold "More" so the folded-by-default panels' headers are on screen.
    rig.app.ui.workbench.collapsed.insert("colour/more".into());
    rig.settle();
    rig.click("section:colour/colorGuide");
    assert!(rig.app.ui.workbench.collapsed.contains("colour/colorGuide"));
    rig.click("section:colour/colorGuide");
    assert!(!rig.app.ui.workbench.collapsed.contains("colour/colorGuide"));
}

#[test]
fn the_colour_section_is_a_tab_strip() {
    let mut rig = Rig::new(Layout::Fusion);
    let first = rig.settle();
    let names = rig.app.ui.workbench.hits.iter().map(|h| h.0.clone()).collect::<Vec<_>>();
    for want in ["sectiontab:colour/color", "sectiontab:colour/swatches", "sectiontab:colour/stroke", "section:colour/more"] {
        assert!(names.contains(&want.to_string()), "{want} in {names:?}");
    }
    assert!(!names.iter().any(|n| n == "section:colour/colorGuide"), "More starts folded");
    let after = rig.click("sectiontab:colour/swatches");
    assert_eq!(rig.app.ui.workbench.section_tab.get("colour").map(String::as_str), Some("swatches"));
    assert_ne!(first, after, "the body changed");
    let saved = serde_json::to_string(&rig.app.ui).unwrap();
    let back: UiState = serde_json::from_str(&saved).unwrap();
    assert_eq!(back.workbench.section_tab.get("colour").map(String::as_str), Some("swatches"));
}

#[test]
fn drawing_tab_shows_properties_without_scrolling() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.settle();
    let rect = rig.app.ui.workbench.hits.iter().find(|h| h.0 == "section:properties").map(|h| h.1).expect("Properties header drawn");
    assert!(rect.bottom() < 850.0, "{rect:?}");
}

#[test]
fn selecting_the_rectangle_tool_shows_its_options_in_the_tool_panel() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.app.select_tool("rectangle");
    let text = rig.settle();
    assert!(text.contains("Rectangle Tool"), "{text}");
    assert!(text.contains("Tool Options…"), "{text}");
}

#[test]
fn pathfinder_opens_in_the_tool_panel_and_closes_with_x() {
    let mut rig = Rig::new(Layout::Fusion);
    let before = rig.settle();
    let items = rig.app.ui.workbench.layout().tabs[0].groups.iter().flat_map(|g| g.items.clone()).collect::<Vec<_>>();
    let item = items.iter().find(|i| matches!(i, super::Item::Panel { id, .. } if id == "pathfinder")).unwrap().clone();
    super::draw::use_item(&mut rig.app, &item);
    let text = rig.settle();
    assert_eq!(rig.app.ui.workbench.right_panels, ["pathfinder"]);
    assert!(text.len() > before.len() && text.contains("Pathfinder"), "{text}");
    rig.click("close:pathfinder");
    assert!(rig.app.ui.workbench.right_panels.is_empty());
}

#[test]
fn opening_a_left_column_panel_unfolds_it_instead_of_duplicating() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.click("section:layers");
    assert!(rig.app.ui.workbench.collapsed.contains("layers"));
    super::draw::open_panel(&mut rig.app, "layers");
    assert!(!rig.app.ui.workbench.collapsed.contains("layers"));
    assert!(rig.app.ui.workbench.right_panels.is_empty());
    super::draw::open_panel(&mut rig.app, "colorGuide");
    assert!(rig.app.ui.workbench.collapsed.contains("colour/colorGuide"));
    assert!(rig.app.ui.workbench.right_panels.is_empty());
    super::draw::open_panel(&mut rig.app, "pathfinder");
    super::draw::open_panel(&mut rig.app, "pathfinder");
    assert_eq!(rig.app.ui.workbench.right_panels, ["pathfinder"]);
    rig.settle();
}

#[test]
fn an_unknown_id_in_the_user_layout_file_is_one_status_line_warning() {
    let mut f = super::default_layout();
    f.tabs[0].groups[0].items.push("tool:noSuchTool".into());
    f.tabs[0].groups[0].items.push("panel:noSuchPanel".into());
    let path = std::env::temp_dir().join(format!("c1-layout-warn-{}.json", std::process::id()));
    std::fs::write(&path, serde_json::to_string(&f).unwrap()).unwrap();
    let mut rig = Rig::new(Layout::Classic);
    rig.app.ui.workbench.layout_path = Some(path.clone());
    rig.app.ui.layout = Layout::Fusion;
    let text = rig.settle();
    std::fs::remove_file(&path).ok();
    assert!(rig.app.ui.status.contains("noSuchTool"), "{}", rig.app.ui.status);
    assert!(rig.app.ui.status.contains("(+1 more)"), "{}", rig.app.ui.status);
    // The rest of the layout still draws.
    assert!(text.contains("SELECT") && text.contains("MODIFY"));
}

// ---------------------------------------------------------------- sketch tab

#[test]
fn new_sketch_from_any_tab_switches_to_sketch_and_shows_b3s_groups() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.app.ui.workbench.tab = "type".into();
    rig.settle();
    crate::sketch::new_sketch(&mut rig.app).unwrap();
    let text = rig.settle();
    assert_eq!(rig.app.ui.workbench.tab, "sketch");
    assert_eq!(rig.app.ui.workbench.prev_tab.as_deref(), Some("type"));
    assert!(text.contains("FINISH SKETCH") && text.contains("CONSTRAINTS"), "{text}");
}

#[test]
fn finish_sketch_returns_to_the_previous_tab() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.app.ui.workbench.tab = "type".into();
    rig.settle();
    crate::sketch::new_sketch(&mut rig.app).unwrap();
    rig.settle();
    assert_eq!(rig.app.ui.workbench.tab, "sketch");
    crate::sketch::finish(&mut rig.app);
    rig.settle();
    assert_eq!(rig.app.ui.workbench.tab, "type");
}

#[test]
fn the_nav_bars_hand_and_zoom_select_those_tools() {
    let mut rig = Rig::new(Layout::Fusion);
    for id in ["zoom", "hand"] {
        let label = vectorcraft_tools::tool_info(id).unwrap().label;
        rig.click(&format!("nav:{label}"));
        assert_eq!(rig.tool(), id);
    }
}

// ---------------------------------------------------------------- timeline strip

/// A Fusion rig that records the timeline, with the drawing of the engine's timeline tests:
/// a rectangle, an ellipse, the rectangle offset by 5 and filled red (four steps).
fn timeline_rig() -> (Rig, u64) {
    let mut rig = Rig::new(Layout::Fusion);
    let s = &mut rig.app.session;
    s.execute("timeline.setRecording", &json!({"on": true})).unwrap();
    s.execute("file.new", &json!({"width": 300, "height": 200})).unwrap();
    let rect = s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 50, "height": 40})).unwrap()["id"].as_u64().unwrap();
    s.execute("shape.ellipse", &json!({"x": 100, "y": 50, "width": 60, "height": 30})).unwrap();
    s.execute("select.set", &json!({"ids": [rect]})).unwrap();
    s.execute("object.path.offsetPath", &json!({"offset": 5})).unwrap();
    let offset = s.doc().unwrap().selection.objects[0].0;
    s.execute("paint.setFill", &json!({"color": "#ff0000"})).unwrap();
    rig.settle();
    (rig, offset)
}

impl Rig {
    fn double_click(&mut self, name: &str) {
        let p = self.hit(name);
        self.frame(vec![Event::PointerMoved(p)]);
        let button = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        for pressed in [true, false, true, false] {
            self.frame(vec![button(pressed)]);
        }
        self.settle();
    }
}

#[test]
fn the_strip_draws_one_chip_per_step() {
    let (mut rig, _) = timeline_rig();
    let text = rig.settle();
    for want in ["Rectangle", "Ellipse", "Offset Path 5"] {
        assert!(text.contains(want), "missing {want}:\n{text}");
    }
    let names = rig.app.ui.workbench.hits.iter().map(|h| h.0.clone()).collect::<Vec<_>>();
    for i in 0..4 {
        assert!(names.contains(&format!("chip:{i}")), "{names:?}");
    }
    assert!(!names.contains(&"chip:4".to_string()));
    assert_eq!(rig.app.ui.workbench.chip_colors.len(), 4);
}

#[test]
fn double_clicking_a_chip_opens_its_form() {
    let (mut rig, offset) = timeline_rig();
    rig.double_click("chip:0");
    let form = rig.app.ui.workbench.step_form.as_mut().expect("the form opened");
    assert_eq!(form.index, 0);
    assert!(form.params.contains_key("width"), "{:?}", form.params);
    form.params.insert("width".into(), json!(120));
    rig.settle();
    rig.click("form:ok");
    assert!(rig.app.ui.workbench.step_form.is_none());
    let w = rig.app.session.doc().unwrap().doc.bounds_of(&[vectorcraft_doc::NodeId(offset)], false).unwrap().width();
    assert!((w - 130.0).abs() < 1e-6, "{w}");
}

#[test]
fn cancel_closes_the_form_without_editing() {
    let (mut rig, _) = timeline_rig();
    rig.double_click("chip:1");
    assert_eq!(rig.app.ui.workbench.step_form.as_ref().map(|f| f.index), Some(1));
    let before = serde_json::to_string(&*rig.app.session.doc().unwrap().doc).unwrap();
    rig.click("form:cancel");
    assert!(rig.app.ui.workbench.step_form.is_none());
    assert_eq!(before, serde_json::to_string(&*rig.app.session.doc().unwrap().doc).unwrap());
}

#[test]
fn a_failed_step_draws_red() {
    let mut rig = Rig::new(Layout::Fusion);
    let s = &mut rig.app.session;
    s.execute("timeline.setRecording", &json!({"on": true})).unwrap();
    s.execute("file.new", &json!({"width": 300, "height": 200})).unwrap();
    let rect = s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 50, "height": 40})).unwrap()["id"].as_u64().unwrap();
    let ellipse = s.execute("shape.ellipse", &json!({"x": 100, "y": 50, "width": 60, "height": 30})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.clear", &json!({"ids": [ellipse]})).unwrap();
    s.execute("select.set", &json!({"ids": [rect]})).unwrap();
    s.execute("object.path.offsetPath", &json!({"offset": 5})).unwrap();
    s.execute("paint.setFill", &json!({"color": "#ff0000"})).unwrap();
    s.execute("timeline.editStep", &json!({"index": 2, "params": {"ids": [rect]}})).unwrap();
    rig.settle();
    let tl = vectorcraft_engine::timeline::read(&rig.app.session.doc().unwrap().doc).unwrap();
    assert_eq!(tl.steps[3].status, vectorcraft_engine::timeline::Status::Failed);
    let t = crate::theme::Tokens::get(&rig.ctx);
    let colors = rig.app.ui.workbench.chip_colors.clone();
    assert_eq!(colors.len(), 5);
    assert_eq!(colors[3], t.error, "the failed chip is the error colour");
    assert_ne!(colors[0], t.error);
    assert_ne!(colors[4], t.error, "the pending one after it is greyed, not red");
    assert_ne!(colors[4], colors[0]);
}

#[test]
fn the_four_buttons_move_the_end_marker() {
    if vectorcraft_engine::cmd::find_command("timeline.rollTo").is_none() {
        return;
    }
    let (mut rig, _) = timeline_rig();
    let marker = |rig: &mut Rig| rig.app.session.execute("timeline.list", &json!({})).unwrap()["marker"].as_u64().unwrap();
    assert_eq!(marker(&mut rig), 4);
    rig.click("tl:first");
    assert_eq!(marker(&mut rig), 0);
    rig.click("tl:next");
    assert_eq!(marker(&mut rig), 1);
    rig.click("tl:last");
    assert_eq!(marker(&mut rig), 4);
    rig.click("tl:prev");
    assert_eq!(marker(&mut rig), 3);
}
