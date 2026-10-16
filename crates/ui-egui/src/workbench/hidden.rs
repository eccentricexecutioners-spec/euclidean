//! What Euclidean hides from its UI (E1, Part A): one registry, by id. A hidden feature is taken
//! out of the Fusion tabs, the menus (in-window and native), the command palette, the right-click
//! menus, the shortcuts and the shortcut editor, the Window menu's panel list, the dock, the
//! export-format and unit pickers, the New Document categories, the workspaces and the
//! Preferences categories. Nothing is deleted: the commands still run from the control channel
//! and from MCP, and deleting an id from a list below brings the feature back.
//!
//! The verdicts come from `_WORKFLOW/app-euclidean/investigations/E1-inventory.md`: CUT is
//! web/screen production and duplicates; ON ICE is C1's list (graphs, Flare, Touch Type,
//! Variables, SVG Interactivity, Actions).

/// Engine and UI command ids hidden everywhere. An entry ending in `.` hides every id with that
/// prefix (`object.slice.` hides `object.slice.make`…).
pub const COMMANDS: &[&str] = &[
    // Slices (web production).
    "object.slice.",
    "view.slices.",
    "select.object.slices",
    // Save for Web, Export for Screens, Asset Export, CSS.
    "document.exportForWeb",
    "document.exportForWeb.preview",
    "webExport.",
    "document.exportForScreens",
    "document.exportSettings",
    "file.exportForScreens",
    "file.saveForWeb",
    "file.saveForWeb.browser",
    "assets.",
    "css.",
    // Office export (a PNG export), pixel grid, video effects, image-map hyperlinks.
    "document.exportForOffice",
    "object.makePixelPerfect",
    "view.pixelPreview",
    "view.snapToPixel",
    "attributes.openUrl",
    // Illustrator-script tooling.
    "script.check",
    "ui.checkScript",
    // Duplicate entry: `text.fitHeadline` stays, this twin goes.
    "type.fitHeadline",
    // On ice.
    "shape.flare",
    "graph.",
    "variable.",
    "dataset.",
];

/// Effect ids (the effect catalogue, `effect.apply {effect}`) hidden from the Effect menu.
pub const EFFECTS: &[&str] = &["video.deinterlace", "video.ntscColors"];

/// Catalogue tool ids hidden everywhere (`tool:` items, the palette, shortcut letters).
pub const TOOLS: &[&str] = &[
    "slice",
    "sliceSelection",
    "touchType",
    "flare",
    "columnGraph",
    "stackedColumnGraph",
    "barGraph",
    "stackedBarGraph",
    "lineGraph",
    "areaGraph",
    "scatterGraph",
    "pieGraph",
    "radarGraph",
];

/// Panel ids hidden from the Window menu, the dock, the layout and `panel:` items.
pub const PANELS: &[&str] = &["assetExport", "cssProperties", "actions", "variables"];

/// Built-in workspace names hidden from Window › Workspace.
pub const WORKSPACES: &[&str] = &["Web", "Automation"];

/// New Document preset categories hidden from the dialog and the Home screen.
pub const PRESET_CATEGORIES: &[&str] = &["Mobile", "Web", "Social", "Film & Video"];

/// Export format ids hidden from the save, export and Save a Copy dialogs.
pub const EXPORT_FORMATS: &[&str] = &["png8", "gif", "webp", "bmp", "tga"];

/// Document units hidden from the unit pickers (by their English label).
pub const UNITS: &[&str] = &["Picas", "Yards", "Feet", "Pixels"];

/// Preferences categories hidden from the Preferences dialog.
pub const PREF_CATEGORIES: &[&str] = &["Slices"];

/// Disabled placeholder menu items hidden by their (English) label.
pub const MENU_LABELS: &[&str] =
    &["Show Video Rulers", "SVG Interactivity", "Column…", "Other Script…", "SVG Filters", "3D and Materials", "Apply SVG Filter…", "Materials…"];

fn listed(list: &[&str], id: &str) -> bool {
    list.iter().any(|h| {
        if let Some(prefix) = h.strip_suffix('.') {
            id.starts_with(prefix) && id.get(prefix.len()..).is_some_and(|r| r.starts_with('.'))
        } else {
            *h == id
        }
    })
}

/// Is command `id` (engine or UI) hidden?
pub fn command(id: &str) -> bool {
    listed(COMMANDS, id)
}

/// Is effect `id` hidden from the Effect menu?
pub fn effect(id: &str) -> bool {
    EFFECTS.contains(&id)
}

/// Is tool `id` hidden?
pub fn tool(id: &str) -> bool {
    TOOLS.contains(&id)
}

/// Is panel `id` hidden?
pub fn panel(id: &str) -> bool {
    PANELS.contains(&id)
}

pub fn workspace(name: &str) -> bool {
    WORKSPACES.contains(&name)
}

pub fn preset_category(name: &str) -> bool {
    PRESET_CATEGORIES.contains(&name)
}

pub fn export_format(id: &str) -> bool {
    EXPORT_FORMATS.contains(&id)
}

/// Is the unit with this English label hidden?
pub fn unit(label: &str) -> bool {
    UNITS.contains(&label)
}

pub fn pref_category(name: &str) -> bool {
    PREF_CATEGORIES.contains(&name)
}

/// Is a menu item with this (English) label a hidden placeholder?
pub fn menu_label(label: &str) -> bool {
    MENU_LABELS.contains(&label)
}

/// A layout item string (`tool:pen`, `cmd:file.print`, `panel:align`, `menu:Object/Path`): hidden
/// when it names a hidden tool, command or panel (a `menu:` item is filtered command by command).
pub fn layout_item(s: &str) -> bool {
    match s.split_once(':') {
        Some(("tool", id)) => tool(id),
        Some(("cmd", id)) => command(id),
        Some(("panel", id)) => panel(id),
        None if s != "sep" => tool(s),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_hide_the_family_and_nothing_beside_it() {
        assert!(command("object.slice.make"));
        assert!(!command("object.slicer"));
        assert!(!command("object.group"));
        assert!(command("type.fitHeadline"));
        assert!(!command("text.fitHeadline"));
        assert!(command("css.copy") && command("css.export"));
        assert!(tool("slice") && !tool("pen"));
        assert!(panel("actions") && !panel("layers"));
        assert!(layout_item("tool:slice") && layout_item("cmd:file.saveForWeb") && layout_item("panel:cssProperties"));
        assert!(!layout_item("menu:Object/Path") && !layout_item("sep"));
    }

    #[test]
    fn every_hidden_id_names_something_real() {
        for id in COMMANDS {
            let exact = id.strip_suffix('.').is_none();
            let known = |c: &str| vectorcraft_engine::cmd::find_command(c).is_some() || crate::menus::UI_COMMANDS.iter().any(|u| u.0 == c);
            if exact {
                assert!(known(id), "{id} is no command");
            } else {
                let prefix = id.trim_end_matches('.');
                let any = vectorcraft_engine::cmd::command_specs()
                    .iter()
                    .any(|c| c.id.starts_with(prefix) && c.id.get(prefix.len()..).is_some_and(|r| r.starts_with('.')))
                    || crate::menus::UI_COMMANDS
                        .iter()
                        .any(|u| u.0.starts_with(prefix) && u.0.get(prefix.len()..).is_some_and(|r| r.starts_with('.')));
                assert!(any, "{id} matches no command");
            }
        }
        for t in TOOLS {
            assert!(vectorcraft_tools::tool_info(t).is_some(), "{t} is no tool");
        }
        for p in PANELS {
            assert!(crate::state::ICON_PANELS.iter().any(|(id, ..)| id == p), "{p} is no panel");
        }
    }
}
