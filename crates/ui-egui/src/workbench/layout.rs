//! The layout file: schema, loading, and resolving names to tools, commands and panels.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use vectorcraft_tools::ToolInfo;

use crate::theme::Tokens;

/// The layout compiled in; the user's `layout.json` replaces it whole.
const DEFAULT_JSON: &str = include_str!("default-layout.json");

/// The default layout's JSON text (what Reveal Layout File writes out first).
pub fn default_json() -> &'static str {
    DEFAULT_JSON
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayoutFile {
    #[serde(default = "one")]
    pub version: u32,
    /// Left-column entries on every tab (panel ids or `sections` names).
    #[serde(default)]
    pub pinned_left: Vec<String>,
    pub tabs: Vec<TabDef>,
    /// Navigation items shown in every tab.
    #[serde(default)]
    pub nav: Vec<String>,
    #[serde(default)]
    pub sections: BTreeMap<String, SectionDef>,
    /// Items with no tab group (they stay reachable, e.g. from the tool panel).
    #[serde(default)]
    pub ice: Vec<String>,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabDef {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub groups: Vec<GroupDef>,
    /// Left-column entries for this tab, below the pinned ones.
    #[serde(default)]
    pub left: Vec<String>,
    /// The tab's colour: one of `draw`, `sketch`, `type`, `output`; else the accent is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupDef {
    pub id: String,
    pub label: String,
    pub items: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectionDef {
    pub label: String,
    pub panels: Vec<String>,
    /// Panels in `panels` that start folded.
    #[serde(default)]
    pub collapsed: Vec<String>,
}

/// One resolved layout entry.
#[derive(Clone, Debug)]
pub enum Item {
    Tool(&'static ToolInfo),
    Cmd { id: String, label: String, shortcut: Option<String> },
    Panel { id: String, label: String },
    Sep,
}

#[derive(Clone, Debug)]
pub struct ResolvedGroup {
    pub id: String,
    pub label: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub struct ResolvedTab {
    pub id: String,
    pub label: String,
    pub groups: Vec<ResolvedGroup>,
    pub left: Vec<String>,
    /// A valid colour name (see [`crate::theme::Tokens::tab_color`]), else `None`.
    pub color: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ResolvedLayout {
    pub pinned_left: Vec<String>,
    pub tabs: Vec<ResolvedTab>,
    pub nav: Vec<Item>,
    pub sections: BTreeMap<String, SectionDef>,
    pub ice: Vec<Item>,
}

/// The compiled-in layout. It must parse (a test guards it); if it ever does not, the layout is empty.
pub fn default_layout() -> LayoutFile {
    serde_json::from_str(DEFAULT_JSON).unwrap_or(LayoutFile {
        version: 1,
        pinned_left: vec![],
        tabs: vec![],
        nav: vec![],
        sections: BTreeMap::new(),
        ice: vec![],
    })
}

/// The user's layout at `path` when it exists and parses, else the default; a file that is there but
/// does not parse gives the default and one warning naming the file and the error.
pub fn load(path: Option<&Path>) -> (LayoutFile, Vec<String>) {
    let Some(path) = path else { return (default_layout(), vec![]) };
    let Ok(text) = std::fs::read_to_string(path) else { return (default_layout(), vec![]) };
    match serde_json::from_str::<LayoutFile>(&text) {
        Ok(file) => (file, vec![]),
        Err(e) => (default_layout(), vec![format!("Layout: {} is not valid ({e}); using the default", path.display())]),
    }
}

/// Resolve every name; an unknown one is skipped with one warning naming it.
pub fn resolve(file: &LayoutFile) -> (ResolvedLayout, Vec<String>) {
    let mut warns = vec![];
    let items = |list: &[String], w: &mut Vec<String>| -> Vec<Item> { list.iter().flat_map(|s| resolve_item(s, w)).collect() };
    let tabs = file
        .tabs
        .iter()
        .map(|t| ResolvedTab {
            id: t.id.clone(),
            label: t.label.clone(),
            groups: t.groups.iter().map(|g| ResolvedGroup { id: g.id.clone(), label: g.label.clone(), items: items(&g.items, &mut warns) }).collect(),
            left: t.left.clone(),
            color: t.color.clone().filter(|c| Tokens::is_tab_color(c)),
        })
        .collect();
    for t in &file.tabs {
        if let Some(c) = t.color.as_deref().filter(|c| !Tokens::is_tab_color(c)) {
            warns.push(format!("Layout: unknown colour '{c}' for tab '{}'; using the accent", t.id));
        }
    }
    let nav = items(&file.nav, &mut warns);
    let ice = items(&file.ice, &mut warns);
    let known = |id: &str| file.sections.contains_key(id) || panel_label(id).is_some();
    for id in file.pinned_left.iter().chain(file.tabs.iter().flat_map(|t| &t.left)) {
        if !known(id) {
            warns.push(format!("Layout: unknown panel or section id '{id}' skipped"));
        }
    }
    for (name, s) in &file.sections {
        for id in &s.panels {
            if panel_label(id).is_none() {
                warns.push(format!("Layout: unknown panel id '{id}' in section '{name}' skipped"));
            }
        }
    }
    let skip = |l: &[String]| l.iter().filter(|id| known(id)).cloned().collect::<Vec<_>>();
    let layout = ResolvedLayout {
        pinned_left: skip(&file.pinned_left),
        tabs: {
            let mut tabs: Vec<ResolvedTab> = tabs;
            for t in &mut tabs {
                t.left = skip(&t.left);
            }
            tabs
        },
        nav,
        sections: file.sections.clone(),
        ice,
    };
    (layout, warns)
}

fn panel_label(id: &str) -> Option<&'static str> {
    crate::state::all_panels().find(|p| p.0 == id).map(|p| p.1)
}

fn resolve_item(s: &str, warns: &mut Vec<String>) -> Vec<Item> {
    if s == "sep" {
        return vec![Item::Sep];
    }
    let (kind, rest) = s.split_once(':').unwrap_or(("tool", s));
    match kind {
        "tool" => match vectorcraft_tools::tool_info(rest) {
            Some(t) => vec![Item::Tool(t)],
            None => unknown(warns, "tool", rest),
        },
        "cmd" => {
            if let Some(c) = vectorcraft_engine::cmd::find_command(rest) {
                vec![Item::Cmd { id: c.id.into(), label: c.label.into(), shortcut: c.shortcut.map(Into::into) }]
            } else if let Some(c) = crate::menus::UI_COMMANDS.iter().find(|c| c.0 == rest) {
                vec![Item::Cmd { id: c.0.into(), label: c.1.into(), shortcut: (!c.2.is_empty()).then(|| c.2.into()) }]
            } else {
                unknown(warns, "command", rest)
            }
        }
        "panel" => match panel_label(rest) {
            Some(label) => vec![Item::Panel { id: rest.into(), label: label.into() }],
            None => unknown(warns, "panel", rest),
        },
        "menu" => {
            let path: Vec<&str> = rest.split('/').collect();
            let found: Vec<Item> = vectorcraft_engine::cmd::command_specs()
                .iter()
                .filter(|c| c.menu.len() >= path.len() && c.menu.iter().zip(&path).all(|(a, b)| a == b))
                .map(|c| Item::Cmd { id: c.id.into(), label: c.label.into(), shortcut: c.shortcut.map(Into::into) })
                .collect();
            if found.is_empty() {
                warns.push(format!("Layout: menu '{rest}' has no commands, skipped"));
            }
            found
        }
        _ => {
            warns.push(format!("Layout: unknown item '{s}' skipped"));
            vec![]
        }
    }
}

fn unknown(warns: &mut Vec<String>, what: &str, id: &str) -> Vec<Item> {
    warns.push(format!("Layout: unknown {what} id '{id}' skipped"));
    vec![]
}

/// Catalogue tools that sit in no tab group, not in `nav` and not in `ice`: a new tool must be
/// placed (or iced) on purpose.
pub fn unplaced_tools(file: &LayoutFile) -> Vec<&'static str> {
    let placed: std::collections::HashSet<&str> = file
        .tabs
        .iter()
        .flat_map(|t| t.groups.iter().flat_map(|g| &g.items))
        .chain(&file.nav)
        .chain(&file.ice)
        .filter_map(|s| match s.split_once(':') {
            Some(("tool", id)) => Some(id),
            None if s != "sep" => Some(s.as_str()),
            _ => None,
        })
        .collect();
    vectorcraft_tools::catalog::all_tools().map(|t| t.id).filter(|id| !placed.contains(id)).collect()
}
