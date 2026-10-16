//! The Fusion-style workbench: a tab strip of tool groups over the canvas, pinned and per-tab panels
//! in a left column, a tool panel on the right. This module holds the layout data (what goes where,
//! read from a JSON file, see `docs/layout.md`) and the workbench's saved state; drawing lives
//! elsewhere.

mod draw;
pub mod hidden;
mod layout;
mod step_form;
mod step_map;
mod timeline_strip;

pub use draw::{round_canvas, show};
pub use step_form::StepForm;

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
    /// The panel each composite section shows in its tab strip, by section id (default = the first).
    #[serde(default)]
    pub section_tab: std::collections::BTreeMap<String, String>,
    pub left_width: f32,
    pub right_width: f32,
    pub right_collapsed: bool,
    /// The left column folded to a rail of icons.
    #[serde(default)]
    pub left_collapsed: bool,
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
    /// Screen rects of the buttons drawn last frame, by name (`tab:draw`, `group:create`,
    /// `label:modify`, `section:layers`, `close:pathfinder`...): what the headless tests click.
    #[serde(skip)]
    pub hits: Vec<(String, egui::Rect)>,
    /// The status-line warning from the last load (the first unknown id, and how many more),
    /// shown once by the next frame.
    #[serde(skip)]
    pub unshown_warning: Option<String>,
    /// The fill each tab's pill was drawn with last frame, by tab id (what the tests read).
    #[serde(skip)]
    pub tab_fills: Vec<(String, egui::Color32)>,
    /// Was sketch mode on last frame (the edge that switches to the SKETCH tab and back)?
    #[serde(skip)]
    pub in_sketch: bool,
    /// The open step-parameters form (double-click a timeline chip).
    #[serde(skip)]
    pub step_form: Option<step_form::StepForm>,
    /// The fill colour each timeline chip was drawn with last frame (what the tests read).
    #[serde(skip)]
    pub chip_colors: Vec<egui::Color32>,
    /// Each timeline step's hue (its tab's colour) last frame.
    #[serde(skip)]
    pub chip_hues: Vec<egui::Color32>,
    /// Which timeline steps were drawn suppressed last frame.
    #[serde(skip)]
    pub chip_suppressed: Vec<bool>,
    /// How many steps the strip showed last frame (it scrolls to the end when this grows).
    #[serde(skip)]
    pub strip_steps: usize,
}

impl Default for WorkbenchState {
    fn default() -> Self {
        Self {
            tab: String::new(),
            collapsed: BTreeSet::new(),
            section_tab: Default::default(),
            left_width: 260.0,
            right_width: 280.0,
            right_collapsed: false,
            left_collapsed: false,
            prev_tab: None,
            layout_path: None,
            loaded: None,
            warnings: vec![],
            right_panels: vec![],
            open_group: None,
            hits: vec![],
            unshown_warning: None,
            tab_fills: vec![],
            in_sketch: false,
            step_form: None,
            chip_colors: vec![],
            chip_hues: vec![],
            chip_suppressed: vec![],
            strip_steps: 0,
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
        self.unshown_warning = warnings.first().map(|first| match warnings.len() {
            1 => first.clone(),
            n => format!("{first} {}", crate::tl!("(+{n} more)").replace("{n}", &(n - 1).to_string())),
        });
        self.warnings = warnings;
        self.loaded = Some(LoadedLayout { file, resolved });
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_frame;
#[cfg(test)]
mod tests_hidden;
