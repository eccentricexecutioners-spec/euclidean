//! E1 Part A: what the hidden registry takes out of the UI, and the visible tool buttons.

use egui::{Event, Modifiers, PointerButton, Pos2};
use serde_json::json;

use super::hidden;
use crate::VectorcraftApp;
use crate::state::Layout;

fn app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
    app.session.execute("file.new", &json!({"width": 400, "height": 300})).unwrap();
    app.session.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 100, "height": 50})).unwrap();
    app
}

/// Every hidden id that is a real command, tool or panel, as the five places name it.
fn hidden_ids() -> Vec<String> {
    let mut v: Vec<String> = vec![];
    for c in vectorcraft_engine::cmd::command_specs() {
        if hidden::command(c.id) {
            v.push(c.id.to_string());
        }
    }
    for c in crate::menus::UI_COMMANDS {
        if hidden::command(c.0) {
            v.push(c.0.to_string());
        }
    }
    v.extend(hidden::TOOLS.iter().map(|t| format!("tool:{t}")));
    v
}

#[test]
fn a_hidden_id_is_in_none_of_the_five_places() {
    let ids = hidden_ids();
    assert!(ids.len() > 40, "{ids:?}");
    let app = app();
    // Menus (the in-window bar and the native bar read the same pruned tree; `ui.menu.list` too).
    let entries = crate::menus::menu_entries(&app);
    for e in &entries {
        if let Some(c) = &e.command {
            assert!(!hidden::command(c), "menu row {} runs hidden {c}", e.label);
            if c == "window.panel"
                && let Some(p) = e.params.get("panel").and_then(serde_json::Value::as_str)
            {
                assert!(!hidden::panel(p), "Window lists hidden panel {p}");
            }
        }
        assert!(!hidden::menu_label(&e.label), "placeholder {} still listed", e.label);
    }
    // The command palette.
    for (label, id, _) in crate::palette::items() {
        assert!(!ids.contains(&id), "palette lists hidden {id} ({label})");
    }
    // Right-click menus.
    for e in crate::menus::context_entries(&app) {
        if let Some(c) = &e.command {
            assert!(!hidden::command(c), "context menu runs hidden {c}");
        }
    }
    // Shortcuts: none bound, none listed in the editor.
    for (sc, id, _) in crate::shortcuts::all_shortcuts() {
        assert!(!hidden::command(id), "{id} still bound to {sc:?}");
    }
    for key in ["tool:slice", "tool:columnGraph", "file.saveForWeb", "view.pixelPreview", "type.fitHeadline"] {
        assert!(crate::shortcut_editor::entry(key).is_none(), "shortcut editor lists {key}");
    }
    assert!(crate::shortcut_editor::tool_shortcut("slice").is_none());
    assert!(crate::shortcut_editor::tool_shortcut("columnGraph").is_none());
    // The Fusion tabs.
    let (lay, warnings) = super::resolve(&super::default_layout());
    assert!(warnings.is_empty(), "{warnings:?}");
    for tab in &lay.tabs {
        for g in &tab.groups {
            for it in g.items.iter().chain(&g.show) {
                if let Some(id) = it.id() {
                    let hidden = match it {
                        super::Item::Tool(_) => hidden::tool(id),
                        super::Item::Cmd { .. } => hidden::command(id),
                        super::Item::Panel { .. } => hidden::panel(id),
                        super::Item::Sep => false,
                    };
                    assert!(!hidden, "tab {} group {} holds hidden {id}", tab.id, g.id);
                }
            }
        }
    }
    assert!(!lay.tabs.iter().any(|t| t.groups.iter().any(|g| g.id == "slices")));
}

#[test]
fn hidden_panels_and_workspaces_are_gone_from_their_lists() {
    assert!(!crate::state::all_panels().any(|(id, _)| hidden::panel(id)));
    assert!(crate::menus::normalize_panel("assetExport").is_none());
    assert!(crate::menus::normalize_panel("layers").is_some());
    let ui = crate::state::UiState::default();
    assert!(!crate::workspaces::names(&ui).iter().any(|n| hidden::workspace(n)));
    assert!(crate::workspaces::find(&ui, "Web").is_none());
    assert!(crate::workspaces::find(&ui, "Essentials").is_some());
    assert!(!crate::prefs_dialog::pref_categories().contains(&"Slices"));
    let units: Vec<String> = crate::menus::ruler_unit_items()
        .iter()
        .filter_map(|i| if let crate::menus::Item::Cmd(l, ..) = i { Some(l.to_string()) } else { None })
        .collect();
    assert_eq!(units, ["Points", "Inches", "Millimeters", "Centimeters", "Feet & Inches", "Meters"]);
}

#[test]
fn fit_headline_is_listed_once() {
    let items = crate::palette::items();
    assert_eq!(items.iter().filter(|(l, ..)| l.ends_with("Fit Headline")).count(), 1, "{items:?}");
    let (lay, _) = super::resolve(&super::default_layout());
    let modify = lay.tabs.iter().find(|t| t.id == "type").unwrap().groups.iter().find(|g| g.id == "modify").unwrap();
    assert_eq!(modify.items.iter().filter(|i| i.id() == Some("text.fitHeadline")).count(), 1);
    assert!(!modify.items.iter().any(|i| i.id() == Some("type.fitHeadline")));
}

#[test]
fn every_keep_tool_is_in_a_group_and_the_show_list_is_a_subset() {
    let (lay, _) = super::resolve(&super::default_layout());
    for tab in &lay.tabs {
        for g in &tab.groups {
            assert!(!g.show.is_empty(), "{}/{} shows nothing", tab.id, g.id);
            for it in &g.show {
                assert!(g.items.iter().any(|a| a.id() == it.id()), "{}/{} shows {:?} it does not hold", tab.id, g.id, it.id());
            }
            // No repeats in a group.
            let ids: Vec<&str> = g.items.iter().filter_map(|i| i.id()).collect();
            let mut dedup = ids.clone();
            dedup.sort_unstable();
            dedup.dedup();
            assert_eq!(ids.len(), dedup.len(), "{}/{} repeats an item: {ids:?}", tab.id, g.id);
        }
    }
    assert!(super::unplaced_tools(&super::default_layout()).is_empty());
}

struct Rig {
    app: VectorcraftApp,
    ctx: egui::Context,
    width: f32,
}

impl Rig {
    fn new(width: f32) -> Rig {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width": 400, "height": 300})).unwrap();
        app.ui.layout = Layout::Fusion;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut rig = Rig { app, ctx, width };
        rig.settle();
        rig
    }

    fn frame(&mut self, events: Vec<Event>) -> String {
        let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(self.width, 850.0));
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(egui::RawInput { events, screen_rect: Some(screen), ..Default::default() }, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        crate::tests_labels::shapes_text(&out)
    }

    fn settle(&mut self) -> String {
        let mut text = String::new();
        for _ in 0..4 {
            text = self.frame(vec![]);
        }
        text
    }

    fn hits(&self, prefix: &str) -> Vec<String> {
        self.app.ui.workbench.hits.iter().filter(|h| h.0.starts_with(prefix)).map(|h| h.0.clone()).collect()
    }

    fn click(&mut self, name: &str) -> String {
        let p = self.app.ui.workbench.hits.iter().find(|h| h.0 == name).unwrap_or_else(|| panic!("no hit {name}: {:?}", self.hits(""))).1.center();
        self.frame(vec![Event::PointerMoved(p)]);
        let button = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
        self.settle()
    }
}

#[test]
fn the_draw_tab_shows_its_tools_as_buttons_and_a_click_selects_one() {
    let mut rig = Rig::new(1600.0);
    let text = rig.settle();
    let items = rig.hits("item:");
    assert!(items.len() >= 20, "only {} visible buttons: {items:?}", items.len());
    for want in
        ["item:selection", "item:pen", "item:rectangle", "item:ellipse", "item:addAnchor", "item:align", "item:eyedropper", "item:file.place.queue"]
    {
        assert!(items.iter().any(|i| i == want), "{want} is not a visible button: {items:?}");
    }
    // Short labels under the icons, the group captions, and no hidden tool.
    for want in ["Pen", "Rect", "Ellipse", "SELECT ▾", "CREATE ▾", "ARRANGE ▾"] {
        assert!(text.contains(want), "missing {want}:\n{text}");
    }
    assert!(!items.iter().any(|i| i == "item:slice"));
    rig.click("item:ellipse");
    assert_eq!(rig.app.session.tool_id(), "ellipse");
    rig.click("item:pen");
    assert_eq!(rig.app.session.tool_id(), "pen");
}

#[test]
fn a_narrow_window_folds_the_last_buttons_into_the_dropdown_but_keeps_one_per_group() {
    let wide = Rig::new(1920.0).hits("item:").len();
    let narrow = Rig::new(1100.0);
    let shown = narrow.hits("item:");
    assert!(shown.len() < wide, "{} at 1920 vs {} at 1100", wide, shown.len());
    for g in ["select", "create", "modify", "arrange", "style", "inspect", "insert"] {
        assert!(narrow.hits(&format!("group:{g}")).len() == 1, "group {g} lost its first button: {shown:?}");
    }
    // Everything folded is still in the drop-down.
    let mut narrow = narrow;
    let text = narrow.click("label:create");
    assert!(text.contains("Spiral Tool") && text.contains("Pen Tool"), "{text}");
}

#[test]
fn the_output_tab_has_print_and_export_buttons_and_no_slices() {
    let mut rig = Rig::new(1600.0);
    rig.click("tab:output");
    let items = rig.hits("item:");
    for want in ["item:artboard", "item:file.print", "item:file.export.pdf", "item:file.export.svg"] {
        assert!(items.iter().any(|i| i == want), "{want} missing: {items:?}");
    }
    assert!(!items.iter().any(|i| i.contains("slice")), "{items:?}");
    assert!(rig.hits("label:slices").is_empty());
}

#[test]
fn the_type_tab_shows_the_type_tools_and_outlines() {
    let mut rig = Rig::new(1600.0);
    let text = rig.click("tab:type");
    let items = rig.hits("item:");
    for want in ["item:type", "item:areaType", "item:typeOnPath", "item:type.createOutlines", "item:text.fitHeadline", "item:glyphs"] {
        assert!(items.iter().any(|i| i == want), "{want} missing: {items:?}");
    }
    assert!(text.contains("Outlines"), "{text}");
}
