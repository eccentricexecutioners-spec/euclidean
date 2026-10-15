//! The layout file: parsing, resolving, the unplaced-tool guard, and the layout switch.

use super::*;
use crate::state::{Layout, UiState};
use serde_json::json;

fn app() -> crate::VectorcraftApp {
    crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default())
}

fn temp_file(name: &str, text: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("euclidean-workbench-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("layout.json");
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn default_layout_parses() {
    serde_json::from_str::<LayoutFile>(default_json()).unwrap();
    let f = default_layout();
    assert_eq!(f.tabs.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["draw", "sketch", "type", "output"]);
    assert_eq!(f.pinned_left, ["layers", "colour"]);
}

#[test]
fn every_default_item_resolves() {
    let (r, warnings) = resolve(&default_layout());
    assert!(warnings.is_empty(), "{warnings:#?}");
    assert!(r.tabs.iter().all(|t| t.groups.iter().all(|g| !g.items.is_empty())));
}

#[test]
fn every_catalogue_tool_is_placed_or_iced() {
    let missing = unplaced_tools(&default_layout());
    assert!(missing.is_empty(), "tools not in the layout or ice: {missing:?} — place them in workbench/default-layout.json");
}

#[test]
fn removing_a_tool_from_layout_and_ice_is_caught() {
    let mut f = default_layout();
    let gone = |s: &String| s != "tool:rectangle";
    for g in f.tabs.iter_mut().flat_map(|t| t.groups.iter_mut()) {
        g.items.retain(gone);
    }
    f.nav.retain(gone);
    f.ice.retain(gone);
    assert_eq!(unplaced_tools(&f), ["rectangle"]);
}

#[test]
fn user_file_overrides_default() {
    let path = temp_file("override", r#"{"tabs":[{"id":"only","label":"ONLY","groups":[{"id":"g","label":"G","items":["tool:pen"]}]}]}"#);
    let (f, w) = load(Some(&path));
    assert!(w.is_empty());
    assert_eq!(f.tabs.len(), 1);
    assert_eq!(f.tabs[0].id, "only");
    let bad = temp_file("bad", "{ nope");
    let (f, w) = load(Some(&bad));
    assert_eq!(f.tabs.len(), 4, "a broken file gives the default");
    assert_eq!(w.len(), 1);
    assert!(w[0].contains("layout.json"), "{w:?}");
    let (f, w) = load(Some(&path.with_file_name("missing.json")));
    assert!(w.is_empty() && f.tabs.len() == 4);
}

#[test]
fn unknown_id_is_skipped_with_one_warning() {
    let mut f = default_layout();
    f.tabs[0].groups[0].items.push("tool:foo".into());
    f.tabs[0].groups[0].items.push("cmd:no.such".into());
    let (r, w) = resolve(&f);
    assert_eq!(w, ["Layout: unknown tool id 'foo' skipped", "Layout: unknown command id 'no.such' skipped"]);
    assert!(!r.tabs[0].groups[0].items.iter().any(|i| matches!(i, Item::Tool(t) if t.id == "foo")));
}

#[test]
fn menu_item_expands_to_commands_under_that_path() {
    let f = LayoutFile {
        version: 1,
        pinned_left: vec![],
        tabs: vec![TabDef {
            id: "t".into(),
            label: "T".into(),
            groups: vec![GroupDef { id: "g".into(), label: "G".into(), items: vec!["menu:Object/Path".into()] }],
            left: vec![],
            color: None,
        }],
        nav: vec![],
        sections: Default::default(),
        ice: vec![],
    };
    let (r, w) = resolve(&f);
    assert!(w.is_empty(), "{w:?}");
    let got: Vec<&str> = r.tabs[0].groups[0].items.iter().map(|i| if let Item::Cmd { id, .. } = i { id.as_str() } else { "?" }).collect();
    let want: Vec<&str> = vectorcraft_engine::command_specs().iter().filter(|c| c.menu.starts_with(&["Object", "Path"])).map(|c| c.id).collect();
    assert!(!want.is_empty());
    assert_eq!(got, want);
}

#[test]
fn ui_state_default_is_classic_saved_without_key_is_fusion() {
    assert_eq!(UiState::default().layout, Layout::Classic);
    let mut v = serde_json::to_value(UiState::default()).unwrap();
    assert_eq!(v["layout"], "classic");
    v.as_object_mut().unwrap().remove("layout");
    assert_eq!(serde_json::from_value::<UiState>(v).unwrap().layout, Layout::Fusion);
    assert_eq!(serde_json::from_str::<UiState>("{}").unwrap().layout, Layout::Fusion);
}

#[test]
fn view_layout_switches_and_checked_reports_it() {
    let mut app = app();
    assert_eq!(app.ui.layout, Layout::Classic);
    let was = serde_json::to_string(&app.session.active().map(|d| d.selection.objects.len())).unwrap();
    app.run("view.layout", json!({"layout": "fusion"})).unwrap();
    assert_eq!(app.ui.layout, Layout::Fusion);
    assert_eq!(crate::menus::checked(&app, "view.layout", &json!({"layout": "fusion"})), Some(true));
    assert_eq!(crate::menus::checked(&app, "view.layout", &json!({"layout": "classic"})), Some(false));
    app.run("view.layout", json!({})).unwrap();
    assert_eq!(app.ui.layout, Layout::Classic);
    app.run("view.layout", json!({})).unwrap();
    assert_eq!(app.ui.layout, Layout::Fusion);
    assert!(app.run("view.layout", json!({"layout": "wide"})).is_err());
    assert_eq!(was, serde_json::to_string(&app.session.active().map(|d| d.selection.objects.len())).unwrap());
}

#[test]
fn reload_reports_and_reveal_needs_a_path() {
    let mut app = app();
    assert!(app.run("view.layout.reveal", json!({})).is_err());
    app.run("view.layout.reload", json!({})).unwrap();
    assert_eq!(app.ui.status, "Layout reloaded");
    let path = temp_file("reload", r#"{"tabs":[{"id":"a","label":"A","groups":[{"id":"g","label":"G","items":["tool:nope"]}]}]}"#);
    app.ui.workbench.layout_path = Some(path);
    app.run("view.layout.reload", json!({})).unwrap();
    assert!(app.ui.status.contains("unknown tool id 'nope'"), "{}", app.ui.status);
    assert_eq!(app.ui.workbench.layout().tabs[0].id, "a");
}

#[test]
fn default_layout_gives_each_tab_its_colour() {
    let (r, w) = resolve(&default_layout());
    assert!(w.is_empty(), "{w:?}");
    for t in &r.tabs {
        assert_eq!(t.color.as_deref(), Some(t.id.as_str()), "tab {}", t.id);
    }
    let f: LayoutFile = serde_json::from_value(json!({"tabs":[{"id":"a","label":"A"}]})).unwrap();
    let (r, w) = resolve(&f);
    assert!(w.is_empty());
    assert_eq!(r.tabs[0].color, None);
    let tk = crate::theme::Tokens::for_brightness(crate::theme::Brightness::Dark);
    assert_eq!(tk.tab_color(None), tk.accent);
    assert_eq!(tk.tab_color(Some("sketch")), tk.tab_sketch);
}

#[test]
fn unknown_tab_colour_is_one_warning() {
    let f: LayoutFile = serde_json::from_value(json!({"tabs":[{"id":"a","label":"A","color":"mauve"}]})).unwrap();
    let (r, w) = resolve(&f);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("mauve"));
    assert_eq!(r.tabs[0].color, None);
}
