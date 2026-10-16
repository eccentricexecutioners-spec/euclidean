//! Sheet & Nest (Euclidean, E1 Part F2): the OUTPUT tab's panel that lays the selected parts out
//! on sheets of a chosen size (`nest.apply`: one artboard per sheet, a cut-path copy of each part
//! on it) and exports the sheets, one file per artboard at the sheet's true size, through the
//! Export As dialog (SVG or DXF) with the range preset to the sheets just made.

use egui::Ui;
use serde_json::{Value, json};

use super::{pstate, selection_len, set_pstate};
use crate::VectorcraftApp;
use crate::widgets;

/// The panel id, as the dock, the Window menu and `window.panel` name it.
pub const ID: &str = "nest";

/// The panel's settings (mm), kept for the session.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Settings {
    width: f64,
    height: f64,
    margin: f64,
    spacing: f64,
    rotate: bool,
}

impl Default for Settings {
    /// A 600 × 400 mm laser bed, 5 mm from the edge, 3 mm between parts, parts may turn.
    fn default() -> Self {
        Settings { width: 600.0, height: 400.0, margin: 5.0, spacing: 3.0, rotate: true }
    }
}

/// What the last nest made: the sheets' artboard indices (0-based), how many copies, the parts
/// that fit no sheet, or the error it gave.
#[derive(Clone, Debug, Default, PartialEq)]
struct Last {
    boards: Vec<usize>,
    copies: usize,
    unplaced: usize,
    error: Option<String>,
}

const LABEL_W: f32 = 96.0;

/// The `nest.apply` parameters for `s`.
fn params(s: &Settings) -> Value {
    json!({"sheet": {"width": s.width, "height": s.height}, "margin": s.margin, "spacing": s.spacing, "rotate": s.rotate})
}

/// The Export As range ("3-4") for the sheets' artboards, 1-based as the dialog counts them.
fn range(boards: &[usize]) -> Option<String> {
    let (lo, hi) = (boards.iter().min()? + 1, boards.iter().max()? + 1);
    Some(if lo == hi { lo.to_string() } else { format!("{lo}-{hi}") })
}

/// The export of the sheets in `format`: one file per sheet artboard (all artboards before a nest),
/// sized in millimetres (DXF writes millimetres by default; SVG gets `sizeUnit: mm`).
fn export_params(format: &str, boards: &[usize]) -> Value {
    let mut p = json!({"format": format, "useArtboards": true});
    if let Some(r) = range(boards) {
        p["range"] = json!(r);
    }
    if format == "svg" {
        p["sizeUnit"] = json!("mm");
        p["responsive"] = json!(false);
    }
    p
}

fn nest(app: &mut VectorcraftApp, s: &Settings) -> Last {
    let ids = |r: &Value, k: &str| r[k].as_array().map_or(0, Vec::len);
    match app.run("nest.apply", params(s)) {
        Ok(r) => Last {
            boards: r["artboards"].as_array().map(|a| a.iter().filter_map(Value::as_u64).map(|i| i as usize).collect()).unwrap_or_default(),
            copies: ids(&r, "copies"),
            unplaced: ids(&r, "unplaced"),
            error: None,
        },
        Err(e) => Last { error: Some(e.to_string()), ..Default::default() },
    }
}

/// A length row: the label, then a field (mm) that takes the rest of the width.
fn length(ui: &mut Ui, id: &str, label: &str, v: &mut f64) {
    widgets::label_row(ui, label, LABEL_W, |ui| {
        let w = (ui.available_width() - 4.0).max(48.0);
        if let Some(n) = widgets::plain_field(ui, ("nest", id), *v, " mm", 1, w).filter(|n| n.is_finite() && *n >= 0.0) {
            *v = n;
        }
    });
}

pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let mut s: Settings = pstate(&ctx, "nest-settings");
    let before = s;
    widgets::subheader(ui, tl!("Sheet"));
    length(ui, "width", tl!("Width:"), &mut s.width);
    length(ui, "height", tl!("Height:"), &mut s.height);
    length(ui, "margin", tl!("Margin:"), &mut s.margin);
    length(ui, "spacing", tl!("Spacing:"), &mut s.spacing);
    if widgets::check(ui, tl!("Parts may turn 90°"), s.rotate, true) {
        s.rotate = !s.rotate;
    }
    if s != before {
        set_pstate(&ctx, "nest-settings", s);
    }
    ui.add_space(8.0);
    let n = selection_len(app);
    let resp = ui.add_enabled_ui(n > 0, |ui| widgets::primary_button(ui, tl!("Nest Selection"))).inner;
    if n == 0 {
        resp.on_hover_text(tl!("Select the parts to nest"));
    } else if resp.clicked() {
        let last = nest(app, &s);
        set_pstate(&ctx, "nest-last", last);
    }
    let last: Last = pstate(&ctx, "nest-last");
    if let Some(e) = &last.error {
        widgets::dim_label(ui, e);
    } else if !last.boards.is_empty() {
        let mut line = crate::i18n::fmt(
            if last.boards.len() == 1 { tl!("{parts} parts on one sheet") } else { tl!("{parts} parts on {sheets} sheets") },
            &[("parts", &last.copies.to_string()), ("sheets", &last.boards.len().to_string())],
        );
        if last.unplaced > 0 {
            line.push_str(" · ");
            line.push_str(&crate::i18n::fmt(tl!("{n} too big for the sheet"), &[("n", &last.unplaced.to_string())]));
        }
        widgets::dim_label(ui, &line);
    }
    ui.add_space(8.0);
    widgets::subheader(ui, tl!("Export"));
    widgets::dim_label(ui, tl!("One file per sheet, at true size, cut paths only."));
    ui.horizontal_wrapped(|ui| {
        for (format, label) in [("svg", tl!("Sheets as SVG…")), ("dxf", tl!("Sheets as DXF…"))] {
            if widgets::secondary_button(ui, label).clicked() {
                // Straight to the save panel: the SVG Options dialog starts Responsive (no size).
                if let Err(e) = crate::io::export(app, Some(format), None, &export_params(format, &last.boards)) {
                    set_pstate(&ctx, "nest-last", Last { error: Some(e), ..last.clone() });
                }
            }
        }
    });
}

pub fn menu(app: &mut VectorcraftApp, ui: &mut Ui) {
    let s: Settings = pstate(ui.ctx(), "nest-settings");
    if widgets::menu_item(ui, tl!("Nest Selection"), selection_len(app) > 0, false) {
        let last = nest(app, &s);
        set_pstate(ui.ctx(), "nest-last", last);
    }
    if widgets::menu_item(ui, tl!("Reset Sheet Settings"), true, false) {
        set_pstate(ui.ctx(), "nest-settings", Settings::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": 600, "height": 400})).unwrap();
        app
    }

    #[test]
    fn draws_headless_with_its_rows() {
        let mut app = app();
        let text = crate::tests_labels::painted_text(&mut app, show);
        for s in ["Sheet", "Width:", "Height:", "Margin:", "Spacing:", "Parts may turn 90°", "Nest Selection", "Sheets as SVG…", "Sheets as DXF…"]
        {
            assert!(text.contains(s), "{s:?} missing from:\n{text}");
        }
    }

    #[test]
    fn nesting_two_plates_makes_one_sheet_and_presets_the_export_range() {
        let mut app = app();
        let mm = 72.0 / 25.4;
        for x in [10.0, 100.0] {
            app.run("shape.rectangle", json!({"x": x * mm, "y": 10.0 * mm, "width": 50.0 * mm, "height": 30.0 * mm})).unwrap();
        }
        app.run("select.all", json!({})).unwrap();
        let last = nest(&mut app, &Settings { width: 150.0, height: 100.0, ..Default::default() });
        assert_eq!((last.boards.len(), last.copies, last.unplaced, last.error), (1, 2, 0, None));
        let board = app.session.active().unwrap().doc.artboards[last.boards[0]].rect;
        assert!((board.width() / mm - 150.0).abs() < 1e-6 && (board.height() / mm - 100.0).abs() < 1e-6, "{board:?}");
        assert_eq!(range(&last.boards), Some((last.boards[0] + 1).to_string()));
        assert_eq!(range(&[2, 3, 4]).as_deref(), Some("3-5"));
        // What the SVG button writes: the sheet alone, 150 × 100 mm.
        let r = app.run("document.export", export_params("svg", &last.boards)).unwrap();
        let svg = String::from_utf8(vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap()).unwrap();
        assert!(svg.contains("width=\"150mm\" height=\"100mm\""), "{}", &svg[..svg.len().min(400)]);
    }
}
