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
    for want in ["Scissors Tool", "Puppet Warp Tool", "Mirror & Cut Tool"] {
        assert!(text.contains(want), "missing {want}:\n{text}");
    }
    let text = rig.click("label:arrange");
    for want in ["Pathfinder", "Group", "Ungroup", "Bring to Front"] {
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
fn sketch_shows_parameters_at_the_top_of_the_right_panel_not_the_left() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.app.ui.workbench.tab = "sketch".into();
    let text = rig.settle();
    let names = rig.app.ui.workbench.hits.iter().map(|h| h.0.clone()).collect::<Vec<_>>();
    assert!(!names.iter().any(|n| n == "section:parameters"), "{names:?}");
    let params = hit_rect(&rig, "section:right-pinned/parameters");
    assert!(params.left() > 800.0, "{params:?}");
    let collapse = hit_rect(&rig, "tool-panel:collapse");
    assert!(params.top() < collapse.top(), "{params:?} vs {collapse:?}");
    assert!(text.contains("Parameters"), "{text}");
}

#[test]
fn draw_and_type_pin_no_right_panels_and_output_pins_sheet_and_nest() {
    let mut rig = Rig::new(Layout::Fusion);
    for (tab, left, pinned) in [
        ("draw", "section:properties", &[][..]),
        ("type", "section:character", &[][..]),
        ("output", "section:artboards", &["section:right-pinned/nest"][..]),
    ] {
        rig.app.ui.workbench.tab = tab.into();
        rig.settle();
        let names = rig.app.ui.workbench.hits.iter().map(|h| h.0.clone()).collect::<Vec<_>>();
        let right: Vec<&String> = names.iter().filter(|n| n.starts_with("section:right-pinned/")).collect();
        assert_eq!(right, pinned.iter().collect::<Vec<_>>(), "{tab}: {names:?}");
        assert!(names.iter().any(|n| n == left), "{tab} lacks {left}: {names:?}");
    }
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
fn opening_parameters_on_sketch_unfolds_the_right_copy_without_duplicating() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.app.ui.workbench.tab = "sketch".into();
    rig.settle();
    rig.click("section:right-pinned/parameters");
    assert!(rig.app.ui.workbench.collapsed.contains("right-pinned/parameters"));
    super::draw::open_panel(&mut rig.app, "parameters");
    super::draw::open_panel(&mut rig.app, "parameters");
    assert!(!rig.app.ui.workbench.collapsed.contains("right-pinned/parameters"));
    assert!(rig.app.ui.workbench.right_panels.is_empty());
    rig.settle();
    // One panel section named parameters (the group row's own Parameters buttons are not panels).
    let n = rig.app.ui.workbench.hits.iter().filter(|h| h.0.starts_with("section:") && h.0.ends_with("parameters")).count();
    assert_eq!(n, 1, "{:?}", rig.app.ui.workbench.hits);
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
fn the_strip_draws_one_square_per_step() {
    let (mut rig, _) = timeline_rig();
    rig.settle();
    let names = rig.app.ui.workbench.hits.iter().map(|h| h.0.clone()).collect::<Vec<_>>();
    for i in 0..4 {
        assert!(names.contains(&format!("chip:{i}")), "{names:?}");
    }
    assert!(!names.contains(&"chip:4".to_string()));
    assert_eq!(rig.app.ui.workbench.chip_colors.len(), 4);
    assert_eq!(rig.app.ui.workbench.chip_hues.len(), 4);
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

impl Rig {
    /// Hover `name` with a zero tooltip delay, then let the frames run.
    fn hover(&mut self, name: &str) -> String {
        self.ctx.global_style_mut(|s| s.interaction.tooltip_delay = 0.0);
        let p = self.hit(name);
        let mut text = self.frame(vec![Event::PointerMoved(p)]);
        for _ in 0..3 {
            text = self.frame(vec![]);
        }
        text
    }
}

fn timeline_list(rig: &mut Rig) -> serde_json::Value {
    rig.app.session.execute("timeline.list", &json!({})).unwrap()
}

#[test]
fn strip_draws_one_icon_per_step_and_no_label() {
    let mut rig = Rig::new(Layout::Fusion);
    let s = &mut rig.app.session;
    s.execute("timeline.setRecording", &json!({"on": true})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 200, "height": 40})).unwrap();
    s.execute("shape.ellipse", &json!({"x": 100, "y": 50, "width": 60, "height": 30})).unwrap();
    let text = rig.settle();
    let chips: Vec<_> = (0..2).map(|i| hit_rect(&rig, &format!("chip:{i}"))).collect();
    assert!(rig.app.ui.workbench.hits.iter().filter(|h| h.0.starts_with("chip:")).count() == 2);
    for r in chips {
        assert!((r.width() - r.height()).abs() < 1.0, "{r:?}");
        assert!((r.width() - 28.0).abs() < 1.0, "{r:?}");
    }
    let labels: Vec<String> = timeline_list(&mut rig)["steps"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap().to_string()).collect();
    assert_eq!(labels.len(), 2);
    for l in labels {
        assert!(!text.contains(&l), "label {l} drawn:\n{text}");
    }
}

#[test]
fn a_sketch_step_takes_sketch_colour_and_a_step_in_no_tab_takes_draw() {
    let t = crate::theme::Tokens::get(&egui::Context::default());
    let lay = super::resolve(&super::default_layout()).0;
    use super::step_map::{step_color, step_tab};
    // The commands sketch mode really records (the layout places the UI command sketch.edit.new).
    for cmd in ["sketch.new", "sketch.addRect"] {
        assert_eq!(step_tab(&lay, cmd).map(|x| x.id.as_str()), Some("sketch"), "{cmd}");
        assert_eq!(step_color(&t, &lay, cmd), t.tab_sketch, "{cmd}");
    }
    assert!(step_tab(&lay, "document.setup").is_none(), "pick another command if a tab gains it");
    assert_eq!(step_color(&t, &lay, "document.setup"), t.tab_draw);
    assert_eq!(step_color(&t, &lay, "shape.rectangle"), t.tab_draw);
    assert_eq!(step_color(&t, &lay, "text.create"), t.tab_type);
}

#[test]
fn hovering_a_step_shows_its_label_and_status() {
    let (mut rig, _) = timeline_rig();
    let before = rig.settle();
    assert!(!before.contains("Step 1"), "{before}");
    let text = rig.hover("chip:0");
    assert!(text.contains("Step 1"), "{text}");
    assert!(text.contains("Rectangle"), "{text}");
    assert!(text.contains("OK"), "{text}");
    for b in ["card:edit", "card:suppress", "card:roll", "card:delete"] {
        assert!(rig.app.ui.workbench.hits.iter().any(|h| h.0 == b), "no {b}");
    }
}

#[test]
fn suppress_from_the_card_fades_the_step_and_runs_suppress_step() {
    let (mut rig, _) = timeline_rig();
    rig.hover("chip:0");
    let normal = rig.app.ui.workbench.chip_colors[0];
    // Straight from the chip onto the card button, so the card stays open.
    let p = rig.hit("card:suppress");
    rig.frame(vec![Event::PointerMoved(p)]);
    let p = rig.hit("card:suppress");
    let button = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    rig.frame(vec![button(true)]);
    rig.frame(vec![button(false)]);
    rig.settle();
    let steps = timeline_list(&mut rig);
    assert_eq!(steps["steps"][0]["suppressed"], json!(true), "{steps}");
    assert!(rig.app.ui.workbench.chip_suppressed[0]);
    assert!(!rig.app.ui.workbench.chip_suppressed[1]);
    assert_ne!(rig.app.ui.workbench.chip_colors[0], normal, "faded");
}

fn hit_rect(rig: &Rig, name: &str) -> egui::Rect {
    rig.app.ui.workbench.hits.iter().find(|h| h.0 == name).unwrap_or_else(|| panic!("no hit '{name}'")).1
}

#[test]
fn fusion_left_column_spans_full_height() {
    let rig = Rig::new(Layout::Fusion);
    let (left, tabs, nav) = (hit_rect(&rig, "panel:left"), hit_rect(&rig, "panel:tabs"), hit_rect(&rig, "panel:nav"));
    assert!(left.top() <= tabs.top() + 0.5, "{left:?} {tabs:?}");
    // (1.5, not 0.5: the left panel's rect stops 1px short of the nav bar's, a frame stroke.)
    assert!(left.bottom() >= nav.bottom() - 1.5, "{left:?} {nav:?}");
    assert!(tabs.left() >= left.right() - 0.5, "{left:?} {tabs:?}");
}

#[test]
fn left_column_collapses_to_a_rail_and_back() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.click("left:collapse");
    assert!(hit_rect(&rig, "panel:left").width() < 60.0);
    rig.hit("left:expand");
    assert!(rig.app.ui.workbench.left_collapsed);
    let saved = serde_json::to_string(&rig.app.ui).unwrap();
    let back: UiState = serde_json::from_str(&saved).unwrap();
    assert!(back.workbench.left_collapsed);
    rig.click("left:expand");
    assert!(hit_rect(&rig, "panel:left").width() > 200.0);
    assert!(!rig.app.ui.workbench.left_collapsed);
    rig.click("left:collapse");
    rig.app.ui.workbench.collapsed.insert("layers".into());
    rig.click("rail:layers");
    assert!(!rig.app.ui.workbench.left_collapsed);
    assert!(!rig.app.ui.workbench.collapsed.contains("layers"));
    assert!(hit_rect(&rig, "panel:left").width() > 200.0);
}

#[test]
fn active_tab_pill_is_filled_with_its_colour() {
    let mut rig = Rig::new(Layout::Fusion);
    let t = crate::theme::Tokens::get(&rig.ctx);
    let fills = |rig: &Rig| rig.app.ui.workbench.tab_fills.clone();
    let fill_of = |rig: &Rig, id: &str| fills(rig).into_iter().find(|f| f.0 == id).map(|f| f.1);
    assert_eq!(fill_of(&rig, "draw"), Some(t.tab_color(Some("draw"))));
    assert_eq!(fill_of(&rig, "sketch"), Some(t.surface));
    rig.click("tab:sketch");
    assert_eq!(fill_of(&rig, "sketch"), Some(t.tab_sketch));
    assert_eq!(fill_of(&rig, "draw"), Some(t.surface));
    // The canvas side sees the unchanged tokens after the workbench drew with its washed one.
    assert_eq!(crate::theme::Tokens::get(&rig.ctx).row_selected, t.row_selected);
}

fn tab_key(pressed: bool) -> Event {
    Event::Key { key: egui::Key::Tab, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }
}

impl Rig {
    fn press_tab(&mut self) -> String {
        self.frame(vec![tab_key(true)]);
        self.frame(vec![tab_key(false)]);
        self.settle()
    }

    fn names(&self) -> Vec<String> {
        self.app.ui.workbench.hits.iter().map(|h| h.0.clone()).collect()
    }
}

#[test]
fn tab_hides_the_workbench_and_tab_brings_it_back() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.click("tool-panel:collapse");
    assert!(rig.app.ui.workbench.right_collapsed);
    let left = rig.app.ui.workbench.left_collapsed;
    let shown = rig.names();
    for want in ["panel:left", "panel:tabs", "panel:nav", "tl:first", "tool-panel:expand"] {
        assert!(shown.iter().any(|n| n == want), "missing {want} before Tab: {shown:?}");
    }
    assert!(shown.iter().any(|n| n.starts_with("group:")));
    let before = rig.settle();
    let text = rig.press_tab();
    assert!(!rig.app.ui.dock && !rig.app.ui.toolbar);
    let hidden = rig.names();
    for n in &hidden {
        assert!(
            !(n == "panel:left"
                || n == "panel:tabs"
                || n == "panel:nav"
                || n.starts_with("group:")
                || n == "tl:first"
                || n.starts_with("tool-panel:")),
            "{n} still drawn after Tab: {hidden:?}"
        );
    }
    // The status bar stays: its rotation box reads "0°".
    assert!(text.contains('°'), "status bar gone:\n{text}");
    assert!(rig.app.ui.workbench.right_collapsed);
    assert_eq!(rig.app.ui.workbench.left_collapsed, left);
    rig.press_tab();
    assert!(rig.app.ui.dock && rig.app.ui.toolbar);
    assert_eq!(rig.names(), shown);
    assert!(rig.app.ui.workbench.right_collapsed);
    assert_eq!(rig.app.ui.workbench.left_collapsed, left);
    assert_eq!(rig.settle(), before);
}

#[test]
fn tab_in_a_text_field_does_not_toggle_the_workbench() {
    let mut rig = Rig::new(Layout::Fusion);
    let mut text = String::new();
    let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 850.0));
    // Each frame also draws a multi-line text field in a window, focused from the first frame.
    let mut run = |rig: &mut Rig, events: Vec<Event>| {
        let app = &mut rig.app;
        let text = &mut text;
        let mut out = rig.ctx.run_ui(egui::RawInput { events, screen_rect: Some(screen), ..Default::default() }, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
            egui::Window::new("probe").show(ui.ctx(), |ui| {
                let r = ui.add(egui::TextEdit::multiline(text).id(egui::Id::new("probe-field")));
                r.request_focus();
            });
        });
        out.textures_delta.clear();
    };
    for _ in 0..4 {
        run(&mut rig, vec![]);
    }
    assert!(rig.ctx.egui_wants_keyboard_input(), "the field has the keyboard");
    run(&mut rig, vec![tab_key(true)]);
    run(&mut rig, vec![tab_key(false)]);
    run(&mut rig, vec![]);
    assert!(rig.app.ui.dock && rig.app.ui.toolbar, "Tab in a field must not hide the workbench");
}

#[test]
fn every_catalogue_tool_shortcut_selects_its_tool_in_fusion() {
    // The key as the runtime reads it: "Shift+X" is X with Shift held; a letter is typed
    // lowercase; other characters as they are.
    fn press(rig: &mut Rig, shortcut: &str) -> String {
        let (shift, key) = match shortcut.strip_prefix("Shift+") {
            Some(k) => (true, k),
            None => (false, shortcut),
        };
        let text = if key.chars().all(|c| c.is_alphabetic()) && !shift { key.to_lowercase() } else { key.to_string() };
        let held = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
        rig.frame(vec![Event::ModifiersChanged(held), Event::Text(text)]);
        rig.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
        rig.settle();
        rig.tool()
    }
    let mut differs = Vec::new();
    let mut own = 0;
    for t in vectorcraft_tools::catalog::all_tools() {
        let Some(sc) = t.shortcut else { continue };
        let mut classic = Rig::new(Layout::Classic);
        let mut fusion = Rig::new(Layout::Fusion);
        let (c, f) = (press(&mut classic, sc), press(&mut fusion, sc));
        assert_eq!(f, c, "Fusion and Classic disagree for {} ({sc})", t.id);
        if f == t.id {
            own += 1;
        } else {
            differs.push(format!("{} ({sc}) -> {f}", t.id));
        }
    }
    assert!(own > 0 && differs.len() <= own / 4, "most tools must select themselves; the others: {differs:?}");
    eprintln!("shortcuts that select another tool in both layouts: {differs:?}");
}

fn sketch_tool_rects(rig: &Rig) -> Vec<(String, egui::Rect)> {
    rig.app.ui.workbench.hits.iter().filter(|h| h.0.starts_with("sketch-tool:")).cloned().collect()
}

#[test]
fn sketch_row_icons_are_twice_their_classic_size() {
    let mut rig = Rig::new(Layout::Fusion);
    crate::sketch::new_sketch(&mut rig.app).unwrap();
    rig.settle();
    let tools = sketch_tool_rects(&rig);
    assert!(!tools.is_empty());
    for (name, r) in &tools {
        assert!((r.width() - 48.0).abs() <= 1.0 && (r.height() - 48.0).abs() <= 1.0, "{name}: {:?}", r.size());
    }
}

#[test]
fn draw_row_is_unchanged_by_the_sketch_icons() {
    let mut rig = Rig::new(Layout::Fusion);
    rig.settle();
    let groups: Vec<_> = rig.app.ui.workbench.hits.iter().filter(|h| h.0.starts_with("group:")).collect();
    assert!(!groups.is_empty());
    // E1: a group's first button is a 46 pt item button (icon over its label), 44 pt or wider.
    for (name, r) in groups {
        assert!(r.width() >= 44.0 && (r.height() - 46.0).abs() < 0.01, "{name}: {:?}", r.size());
    }
    assert!(sketch_tool_rects(&rig).is_empty());
}

#[test]
fn classic_sketch_toolbar_keeps_its_size() {
    let mut rig = Rig::new(Layout::Classic);
    crate::sketch::new_sketch(&mut rig.app).unwrap();
    rig.settle();
    assert!(sketch_tool_rects(&rig).is_empty());
}

// ---------------------------------------------------------------- E1 Part E: features, right-click

impl Rig {
    fn right_click(&mut self, name: &str) -> String {
        let p = self.hit(name);
        self.frame(vec![Event::PointerMoved(p)]);
        let button = |pressed| Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
        self.settle()
    }
}

#[test]
fn right_click_on_a_step_offers_edit_suppress_delete_roll_and_rename_and_rename_names_it() {
    let (mut rig, _) = timeline_rig();
    let text = rig.right_click("chip:0");
    for item in ["Edit…", "Suppress", "Delete", "Roll to here", "Rename…"] {
        assert!(text.contains(item), "{item} missing in\n{text}");
    }
    rig.click("menu:rename");
    assert!(rig.app.ui.workbench.rename_step.is_some(), "the Rename Step window is up");
    rig.app.ui.workbench.rename_step = Some((0, String::new()));
    rig.settle();
    rig.frame(vec![Event::Text("Base plate".into())]);
    rig.settle();
    rig.click("rename:ok");
    assert!(rig.app.ui.workbench.rename_step.is_none());
    let steps = timeline_list(&mut rig);
    assert_eq!((steps["steps"][0]["label"].as_str(), steps["steps"][0]["name"].as_str()), (Some("Base plate"), Some("Base plate")), "{steps}");
    // The menu's title shows the name; Suppress from the menu fades the step.
    let text = rig.right_click("chip:0");
    assert!(text.contains("Step 1 · Base plate"), "{text}");
    rig.click("menu:suppress");
    assert_eq!(timeline_list(&mut rig)["steps"][0]["suppressed"], json!(true));
    assert!(rig.app.ui.workbench.chip_suppressed[0]);
    rig.right_click("chip:0");
    let text = rig.settle();
    assert!(text.contains("Unsuppress"), "{text}");
    rig.click("menu:suppress");
    assert_eq!(timeline_list(&mut rig)["steps"][0]["suppressed"], json!(false));
    // Roll to here from the menu.
    rig.right_click("chip:1");
    rig.click("menu:roll");
    assert_eq!(timeline_list(&mut rig)["marker"], json!(2));
    // Delete from the menu.
    rig.right_click("chip:0");
    rig.click("menu:delete");
    assert_eq!(timeline_list(&mut rig)["steps"].as_array().unwrap().len(), 3);
}

#[test]
fn a_sketch_is_one_step_and_editing_it_reopens_the_sketch() {
    let mut rig = Rig::new(Layout::Fusion);
    let s = &mut rig.app.session;
    s.execute("timeline.setRecording", &json!({"on": true})).unwrap();
    s.execute("file.new", &json!({"width": 300, "height": 200})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 50, "height": 40})).unwrap();
    let id = s.execute("sketch.new", &json!({})).unwrap()["id"].as_u64().unwrap();
    for i in 0..5 {
        s.execute("sketch.addLine", &json!({"id": id, "from": [10.0, 10.0 + i as f64 * 5.0], "to": [60.0, 10.0 + i as f64 * 5.0]})).unwrap();
    }
    s.execute("sketch.addDimension", &json!({"id": id, "constraint": {"type": "length", "line": 0}})).unwrap();
    rig.settle();
    let steps = timeline_list(&mut rig);
    assert_eq!(steps["steps"].as_array().unwrap().len(), 2, "{steps}");
    assert_eq!(steps["steps"][1]["label"], "Sketch 1");
    assert_eq!(rig.app.ui.workbench.chip_colors.len(), 2);
    let text = rig.hover("chip:1");
    assert!(text.contains("7 commands"), "{text}");
    assert!(rig.app.sketch.is_none());
    rig.double_click("chip:1");
    assert!(rig.app.sketch.as_ref().is_some_and(|m| m.id.0 == id), "Edit reopens the sketch");
}
