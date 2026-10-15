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
    assert!(after.len() < before.len(), "the body should be gone");
    assert!(after.contains("Layers"), "the header stays");
    let saved = serde_json::to_string(&rig.app.ui).unwrap();
    let back: UiState = serde_json::from_str(&saved).unwrap();
    assert!(back.workbench.collapsed.contains("layers"));
    // A panel a section folds by default is flipped open by a click, and back.
    assert!(!rig.app.ui.workbench.collapsed.contains("colour/colorGuide"));
    // Fold the tall ones above it so its header is on screen.
    rig.app.ui.workbench.collapsed.extend(["colour/color", "colour/swatches", "colour/stroke"].map(String::from));
    rig.settle();
    rig.click("section:colour/colorGuide");
    assert!(rig.app.ui.workbench.collapsed.contains("colour/colorGuide"));
    rig.click("section:colour/colorGuide");
    assert!(!rig.app.ui.workbench.collapsed.contains("colour/colorGuide"));
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
