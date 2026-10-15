//! The Fusion-style workbench: a tab strip of tool groups over the canvas, pinned and per-tab panels
//! in a left column, a tool panel on the right. This module holds the layout data (what goes where,
//! read from a JSON file, see `docs/layout.md`) and the workbench's saved state; drawing lives
//! elsewhere.

mod layout;

pub use layout::*;

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// A layout file read and resolved, cached on the [`WorkbenchState`].
#[derive(Clone, Debug)]
pub struct LoadedLayout {
    pub file: LayoutFile,
    pub resolved: ResolvedLayout,
}

/// What the workbench remembers between runs (the first six fields, saved in `ui.json`) and what it
/// holds for this run only.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkbenchState {
    /// The active tab's id; empty = the first tab.
    pub tab: String,
    /// Left-column section ids (panels or `sections` names) folded shut.
    pub collapsed: BTreeSet<String>,
    pub left_width: f32,
    pub right_width: f32,
    pub right_collapsed: bool,
    /// The tab before the last switch (a tool that lives on another tab may flip back).
    pub prev_tab: Option<String>,
    /// The user's `layout.json`; the app sets it (none in tests: the compiled-in default is used).
    #[serde(skip)]
    pub layout_path: Option<PathBuf>,
    /// The resolved layout, loaded on first use.
    #[serde(skip)]
    pub loaded: Option<LoadedLayout>,
    /// Warnings from the last load and resolve.
    #[serde(skip)]
    pub warnings: Vec<String>,
    /// Panel ids opened in the right tool panel.
    #[serde(skip)]
    pub right_panels: Vec<String>,
    /// The tab group whose flyout is open.
    #[serde(skip)]
    pub open_group: Option<String>,
}

impl Default for WorkbenchState {
    fn default() -> Self {
        Self {
            tab: String::new(),
            collapsed: BTreeSet::new(),
            left_width: 260.0,
            right_width: 280.0,
            right_collapsed: false,
            prev_tab: None,
            layout_path: None,
            loaded: None,
            warnings: vec![],
            right_panels: vec![],
            open_group: None,
        }
    }
}

impl WorkbenchState {
    /// The resolved layout: read and resolved once, then cached (warnings go to `self.warnings`).
    pub fn layout(&mut self) -> &ResolvedLayout {
        if self.loaded.is_none() {
            self.reload();
        }
        &self.loaded.get_or_insert_with(|| LoadedLayout { file: default_layout(), resolved: resolve(&default_layout()).0 }).resolved
    }

    /// Read the layout file again (the user's if it exists, else the default) and resolve it.
    pub fn reload(&mut self) {
        let (file, mut warnings) = load(self.layout_path.as_deref());
        let (resolved, more) = resolve(&file);
        warnings.extend(more);
        self.warnings = warnings;
        self.loaded = Some(LoadedLayout { file, resolved });
    }
}

#[cfg(test)]
mod tests;
