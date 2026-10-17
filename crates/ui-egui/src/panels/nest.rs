//! Sheet & Nest (Euclidean, E1 Part F2; E4 Part B): the OUTPUT tab's panel that lays the selected
//! parts out on sheets (`nest.apply`: one artboard per sheet, a cut-path copy of each part on it)
//! and exports the sheets, one file per artboard at the sheet's true size, through the Export As
//! dialog (SVG or DXF).
//!
//! The panel follows the active artboard: when it is a sheet, the panel shows that sheet's
//! settings, and a change there resizes the sheet and re-nests its parts (`nest.updateSheet`);
//! Nest Selection then fills that sheet first. Otherwise it shows the settings a new sheet gets
//! (kept for the session) and Nest Selection makes new sheets. Add Sheet adds an empty one.

use egui::Ui;
use serde_json::{Value, json};
use vectorcraft_engine::cmd::nest::{SheetSpec, sheet_of};

use super::{pstate, selection_len, set_pstate};
use crate::VectorcraftApp;
use crate::widgets;

/// The panel id, as the dock, the Window menu and `window.panel` name it.
pub const ID: &str = "nest";

/// The panel's settings (mm): a new sheet's (kept for the session), or the active sheet's.
#[derive(Clone, Debug, PartialEq)]
struct Settings {
    material: String,
    width: f64,
    height: f64,
    thickness: f64,
    margin: f64,
    spacing: f64,
    rotate: bool,
}

impl Default for Settings {
    /// A 600 × 400 mm laser bed, 5 mm from the edge, 3 mm between parts, parts may turn.
    fn default() -> Self {
        Settings { material: String::new(), width: 600.0, height: 400.0, thickness: 0.0, margin: 5.0, spacing: 3.0, rotate: true }
    }
}

impl From<SheetSpec> for Settings {
    fn from(s: SheetSpec) -> Self {
        Settings {
            material: s.material,
            width: s.width,
            height: s.height,
            thickness: s.thickness,
            margin: s.margin,
            spacing: s.spacing,
            rotate: s.rotate,
        }
    }
}

/// The material presets: name, width × height × thickness (mm). Custom keeps what is typed.
pub(crate) const MATERIALS: [(&str, f64, f64, f64); 3] =
    [("Plywood", 1220.0, 2440.0, 6.0), ("MDF", 1220.0, 2440.0, 3.0), ("Acrylic", 600.0, 400.0, 3.0)];

/// A preset's name in the UI language (the document keeps the English one).
fn material_label(name: &str) -> &str {
    match name {
        "Plywood" => tl!("Plywood"),
        "MDF" => tl!("MDF"),
        "Acrylic" => tl!("Acrylic"),
        other => other,
    }
}

/// What the last action made: the sheets' artboard indices (0-based), how many copies, the parts
/// that fit no sheet, or the error it gave; `line` is a status of its own (a sheet changed).
#[derive(Clone, Debug, Default, PartialEq)]
struct Last {
    boards: Vec<usize>,
    copies: usize,
    unplaced: usize,
    error: Option<String>,
    line: Option<String>,
}

const LABEL_W: f32 = 96.0;

/// The `settings` object of `nest.addSheet` / `nest.updateSheet` for `s`.
fn spec_json(s: &Settings) -> Value {
    json!({"material": s.material, "width": s.width, "height": s.height, "thickness": s.thickness, "margin": s.margin, "spacing": s.spacing, "rotate": s.rotate})
}

/// The `nest.apply` parameters for `s`, onto sheet `onto` when one is active.
fn params(s: &Settings, onto: Option<usize>) -> Value {
    let mut p = json!({"sheet": {"width": s.width, "height": s.height}, "margin": s.margin, "spacing": s.spacing, "rotate": s.rotate, "material": s.material, "thickness": s.thickness});
    if let Some(i) = onto {
        p["onto"] = json!(i);
    }
    p
}

/// The Export As range ("3-4" or "2,5") for the sheets' artboards, 1-based as the dialog counts.
fn range(boards: &[usize]) -> Option<String> {
    let mut b: Vec<usize> = boards.iter().map(|i| i + 1).collect();
    b.sort_unstable();
    b.dedup();
    let (lo, hi) = (*b.first()?, *b.last()?);
    Some(if lo == hi {
        lo.to_string()
    } else if hi - lo + 1 == b.len() {
        format!("{lo}-{hi}")
    } else {
        b.iter().map(usize::to_string).collect::<Vec<_>>().join(",")
    })
}

/// The export of the sheets in `format`: one file per sheet artboard (all artboards when there is
/// no sheet), sized in millimetres (DXF writes millimetres by default; SVG gets `sizeUnit: mm`).
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

/// The active artboard and its sheet settings, when it is a sheet.
fn active_sheet(app: &VectorcraftApp) -> Option<(usize, SheetSpec, bool)> {
    let i = app.view()?.artboard;
    let d = app.session.active()?;
    sheet_of(&d.doc, i).map(|(spec, stored)| (i, spec, stored))
}

/// Every sheet's artboard index.
fn all_sheets(app: &VectorcraftApp) -> Vec<usize> {
    let Some(d) = app.session.active() else { return vec![] };
    (0..d.doc.artboards.len()).filter(|i| sheet_of(&d.doc, *i).is_some()).collect()
}

/// The sheets an export writes: the active sheet, else every sheet (else every artboard).
pub(crate) fn export_boards(app: &VectorcraftApp) -> Vec<usize> {
    active_sheet(app).map_or_else(|| all_sheets(app), |(i, ..)| vec![i])
}

fn nest(app: &mut VectorcraftApp, s: &Settings, onto: Option<usize>) -> Last {
    let ids = |r: &Value, k: &str| r[k].as_array().map_or(0, Vec::len);
    match app.run("nest.apply", params(s, onto)) {
        Ok(r) => Last {
            boards: r["artboards"].as_array().map(|a| a.iter().filter_map(Value::as_u64).map(|i| i as usize).collect()).unwrap_or_default(),
            copies: ids(&r, "copies"),
            unplaced: ids(&r, "unplaced"),
            ..Default::default()
        },
        Err(e) => Last { error: Some(e.to_string()), ..Default::default() },
    }
}

/// Change sheet `index` to `s`: it resizes and its parts re-nest. The status says where they went.
fn update(app: &mut VectorcraftApp, index: usize, s: &Settings) -> Last {
    match app.run("nest.updateSheet", json!({"artboard": index, "settings": spec_json(s)})) {
        Ok(r) => {
            let n = |k: &str| r[k].as_u64().unwrap_or(0) as usize;
            let unplaced = r["unplaced"].as_array().map_or(0, Vec::len);
            let mut line = crate::i18n::fmt(tl!("Sheet changed: {parts} parts re-nested"), &[("parts", &n("moved").to_string())]);
            if n("overflow") > 0 {
                line.push_str(" · ");
                line.push_str(&crate::i18n::fmt(tl!("{n} moved to a new sheet"), &[("n", &n("overflow").to_string())]));
            }
            if unplaced > 0 {
                line.push_str(" · ");
                line.push_str(&crate::i18n::fmt(tl!("{n} too big for the sheet"), &[("n", &unplaced.to_string())]));
            }
            let boards = r["artboards"].as_array().map(|a| a.iter().filter_map(Value::as_u64).map(|i| i as usize).collect()).unwrap_or_default();
            Last { boards, line: Some(line), ..Default::default() }
        }
        Err(e) => Last { error: Some(e.to_string()), ..Default::default() },
    }
}

/// Add an empty sheet with `s` and make it the active artboard, so the panel shows it.
fn add_sheet(app: &mut VectorcraftApp, s: &Settings) -> Last {
    match app.run("nest.addSheet", json!({"settings": spec_json(s)})) {
        Ok(r) => {
            let i = r["artboard"].as_u64().unwrap_or(0) as usize;
            if let Some(v) = app.view_mut() {
                v.artboard = i;
            }
            let name = app.session.active().and_then(|d| d.doc.artboards.get(i)).map(|a| a.name.clone()).unwrap_or_default();
            Last { boards: vec![i], line: Some(crate::i18n::fmt(tl!("{name} added: empty"), &[("name", &name)])), ..Default::default() }
        }
        Err(e) => Last { error: Some(e.to_string()), ..Default::default() },
    }
}

/// Write `boards` for LightBurn (`.lbrn2`, one file per sheet) at `path`, else where a save panel
/// says. The status names the files and the layers the colours went to.
pub(crate) fn export_lightburn(app: &mut VectorcraftApp, boards: &[usize], path: Option<String>) -> Result<Value, String> {
    let path = crate::io::target_path(app, path, "lbrn2")?;
    let r = app.session.execute("nest.exportLightBurn", &json!({"artboards": boards, "path": path})).map_err(|e| e.to_string())?;
    let files = r["files"].as_array().map_or(0, Vec::len);
    let mut layers: Vec<String> = vec![];
    for l in r["layers"].as_array().into_iter().flatten() {
        let row = format!("{} → C{:02}", l["colour"].as_str().unwrap_or_default(), l["index"].as_u64().unwrap_or(0));
        if !layers.contains(&row) {
            layers.push(row);
        }
    }
    app.status(crate::i18n::fmt(tl!("Exported {n} LightBurn file(s): {layers}"), &[("n", &files.to_string()), ("layers", &layers.join(", "))]));
    Ok(r)
}

/// Write the job package (every sheet as SVG, DXF and LightBurn, a thumbnail each, job.json) to the
/// folder `path`, else to one named in a save panel (its extension dropped).
pub(crate) fn export_job(app: &mut VectorcraftApp, path: Option<String>) -> Result<Value, String> {
    let folder = match path {
        Some(p) => p,
        None => {
            let picked = crate::io::target_path(app, None, "job")?;
            picked.strip_suffix(".job").map_or(picked.clone(), str::to_string)
        }
    };
    let r = app.session.execute("nest.exportJob", &json!({"path": folder})).map_err(|e| e.to_string())?;
    let sheets = r["job"]["sheets"].as_array().map_or(0, Vec::len);
    app.status(crate::i18n::fmt(tl!("Job exported: {n} sheet(s) in {folder}"), &[("n", &sheets.to_string()), ("folder", &folder)]));
    Ok(r)
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

/// The material row: a preset menu (Plywood, MDF, Acrylic, Custom) that sets the size and
/// thickness, and the name, editable.
fn material(ui: &mut Ui, s: &mut Settings) {
    widgets::label_row(ui, tl!("Material:"), LABEL_W, |ui| {
        let w = (ui.available_width() - 4.0).max(48.0);
        let custom = tl!("Custom");
        let names: Vec<&str> = MATERIALS.iter().map(|m| material_label(m.0)).chain([custom]).collect();
        let current = MATERIALS
            .iter()
            .position(|m| m.0 == s.material || material_label(m.0) == s.material)
            .and_then(|i| names.get(i).copied())
            .unwrap_or(custom);
        if let Some(k) = widgets::dropdown_names(ui, ("nest", "material"), current, &names, w)
            && let Some((name, width, height, thick)) = MATERIALS.get(k)
        {
            (s.material, s.width, s.height, s.thickness) = ((*name).to_string(), *width, *height, *thick);
        }
    });
    widgets::label_row(ui, tl!("Name:"), LABEL_W, |ui| {
        let w = (ui.available_width() - 4.0).max(48.0);
        if let Some(n) = widgets::text_field(ui, ("nest", "material-name"), Some(&s.material), w, 1) {
            s.material = n;
        }
    });
}

pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let active = active_sheet(app);
    let mut s: Settings = match &active {
        Some((_, spec, _)) => spec.clone().into(),
        None => pstate(&ctx, "nest-settings"),
    };
    let before = s.clone();
    match &active {
        Some((i, _, stored)) => {
            let name = app.session.active().and_then(|d| d.doc.artboards.get(*i)).map(|a| a.name.clone()).unwrap_or_default();
            widgets::subheader(ui, &crate::i18n::fmt(tl!("Sheet: {name}"), &[("name", &name)]));
            if !*stored {
                widgets::dim_label(ui, tl!("Made by an earlier version: margin and spacing are the defaults."));
            }
        }
        None => {
            widgets::subheader(ui, tl!("New sheet"));
            widgets::dim_label(ui, tl!("Click a sheet's artboard to change that sheet."));
        }
    }
    material(ui, &mut s);
    length(ui, "width", tl!("Width:"), &mut s.width);
    length(ui, "height", tl!("Height:"), &mut s.height);
    length(ui, "thickness", tl!("Thickness:"), &mut s.thickness);
    length(ui, "margin", tl!("Margin:"), &mut s.margin);
    length(ui, "spacing", tl!("Spacing:"), &mut s.spacing);
    if widgets::check(ui, tl!("Parts may turn 90°"), s.rotate, true) {
        s.rotate = !s.rotate;
    }
    if s != before {
        match &active {
            Some((i, ..)) => {
                let last = update(app, *i, &s);
                set_pstate(&ctx, "nest-last", last);
            }
            None => set_pstate(&ctx, "nest-settings", s.clone()),
        }
    }
    ui.add_space(8.0);
    let n = selection_len(app);
    let onto = active.as_ref().map(|a| a.0);
    let label = if onto.is_some() { tl!("Nest Selection onto this sheet") } else { tl!("Nest Selection") };
    let resp = ui.add_enabled_ui(n > 0, |ui| widgets::primary_button(ui, label)).inner;
    app.ui.workbench.hits.push(("nest:nest".into(), resp.rect));
    if n == 0 {
        resp.on_hover_text(tl!("Select the parts to nest"));
    } else if resp.clicked() {
        let last = nest(app, &s, onto);
        set_pstate(&ctx, "nest-last", last);
    }
    let add = widgets::secondary_button(ui, tl!("Add Sheet"));
    app.ui.workbench.hits.push(("nest:add".into(), add.rect));
    if add.on_hover_text(tl!("An empty sheet with these settings, right of the art")).clicked() {
        let last = add_sheet(app, &s);
        set_pstate(&ctx, "nest-last", last);
    }
    let last: Last = pstate(&ctx, "nest-last");
    if let Some(e) = &last.error {
        widgets::dim_label(ui, e);
    } else if let Some(line) = &last.line {
        widgets::dim_label(ui, line);
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
    widgets::dim_label(
        ui,
        if onto.is_some() { tl!("This sheet, at true size, cut paths only.") } else { tl!("One file per sheet, at true size, cut paths only.") },
    );
    let boards = export_boards(app);
    ui.horizontal_wrapped(|ui| {
        for (format, label) in [("svg", tl!("Sheets as SVG…")), ("dxf", tl!("Sheets as DXF…"))] {
            if widgets::secondary_button(ui, label).clicked() {
                // Straight to the save panel: the SVG Options dialog starts Responsive (no size).
                if let Err(e) = crate::io::export(app, Some(format), None, &export_params(format, &boards)) {
                    set_pstate(&ctx, "nest-last", Last { error: Some(e), ..last.clone() });
                }
            }
        }
        let lb = widgets::secondary_button(ui, tl!("For LightBurn…"));
        app.ui.workbench.hits.push(("nest:lightburn".into(), lb.rect));
        if lb.on_hover_text(tl!("A LightBurn project (.lbrn2) per sheet: millimetres, one layer per colour")).clicked()
            && let Err(e) = export_lightburn(app, &boards, None)
        {
            set_pstate(&ctx, "nest-last", Last { error: Some(e), ..last.clone() });
        }
    });
    ui.add_space(8.0);
    widgets::subheader(ui, tl!("Job"));
    widgets::dim_label(ui, tl!("Every sheet as SVG, DXF and LightBurn, a picture of each, and job.json, in one folder."));
    let job = widgets::secondary_button(ui, tl!("Export Job…"));
    app.ui.workbench.hits.push(("nest:job".into(), job.rect));
    if job.clicked()
        && let Err(e) = export_job(app, None)
    {
        set_pstate(&ctx, "nest-last", Last { error: Some(e), ..last.clone() });
    }
}

pub fn menu(app: &mut VectorcraftApp, ui: &mut Ui) {
    let active = active_sheet(app);
    let s: Settings = match &active {
        Some((_, spec, _)) => spec.clone().into(),
        None => pstate(ui.ctx(), "nest-settings"),
    };
    if widgets::menu_item(ui, tl!("Nest Selection"), selection_len(app) > 0, false) {
        let last = nest(app, &s, active.as_ref().map(|a| a.0));
        set_pstate(ui.ctx(), "nest-last", last);
    }
    if widgets::menu_item(ui, tl!("Add Sheet"), app.session.active().is_some(), false) {
        let last = add_sheet(app, &s);
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
        let last = nest(&mut app, &Settings { width: 150.0, height: 100.0, ..Default::default() }, None);
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

    /// Two plates nested on a 150 × 100 sheet → the sheet's artboard index.
    fn two_plates(app: &mut VectorcraftApp) -> usize {
        let mm = 72.0 / 25.4;
        for x in [10.0, 100.0] {
            app.run("shape.rectangle", json!({"x": x * mm, "y": 10.0 * mm, "width": 50.0 * mm, "height": 30.0 * mm})).unwrap();
        }
        app.run("select.all", json!({})).unwrap();
        let s = Settings { material: "Acrylic".into(), width: 150.0, height: 100.0, thickness: 3.0, ..Default::default() };
        nest(app, &s, None).boards[0]
    }

    #[test]
    fn the_panel_shows_the_active_sheets_settings_and_new_sheet_defaults_elsewhere() {
        let mut app = app();
        let board = two_plates(&mut app);
        app.view_mut().unwrap().artboard = board;
        let text = crate::tests_labels::painted_text(&mut app, show);
        for want in
            ["Sheet: Sheet 1", "Acrylic", "150 mm", "100 mm", "3 mm", "Nest Selection onto this sheet", "Add Sheet", "This sheet, at true size"]
        {
            assert!(text.contains(want), "{want:?} missing from:\n{text}");
        }
        assert_eq!(export_boards(&app), [board], "the export writes the active sheet");
        app.view_mut().unwrap().artboard = 0;
        let text = crate::tests_labels::painted_text(&mut app, show);
        for want in ["New sheet", "600 mm", "400 mm", "Nest Selection", "One file per sheet"] {
            assert!(text.contains(want), "{want:?} missing from:\n{text}");
        }
        assert!(!text.contains("onto this sheet"), "{text}");
        assert_eq!(export_boards(&app), [board], "every sheet, not the drawing's artboard");
    }

    #[test]
    fn a_change_on_the_active_sheet_resizes_it_and_add_sheet_makes_the_new_one_active() {
        let mut app = app();
        let board = two_plates(&mut app);
        let mm = 72.0 / 25.4;
        let n = app.session.active().unwrap().doc.artboards.len();
        let (spec, _) = sheet_of(&app.session.active().unwrap().doc, board).unwrap();
        // Width 60: one plate a row, both fit below each other in 100 high.
        let last = update(&mut app, board, &Settings { width: 60.0, ..spec.into() });
        assert!(last.error.is_none() && last.line.as_deref().is_some_and(|l| l.contains("2 parts re-nested")), "{last:?}");
        let d = &app.session.active().unwrap().doc;
        assert_eq!(d.artboards.len(), n, "no new sheet");
        assert!((d.artboards[board].rect.width() - 60.0 * mm).abs() < 1e-6);
        let last = add_sheet(&mut app, &Settings { material: "Plywood".into(), width: 1220.0, height: 2440.0, thickness: 6.0, ..Default::default() });
        let added = last.boards[0];
        assert_eq!(app.view().unwrap().artboard, added, "the new sheet is the active artboard");
        assert_eq!(active_sheet(&app).map(|a| (a.0, a.1.material)), Some((added, "Plywood".to_string())));
        assert_eq!(range(&[1, 4]).as_deref(), Some("2,5"));
    }

    #[test]
    fn file_export_offers_lightburn_and_the_job_and_they_write_the_active_sheet() {
        let mut app = app();
        let board = two_plates(&mut app);
        app.view_mut().unwrap().artboard = board;
        let dir = std::env::temp_dir().join(format!("euclidean-nest-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let lb = dir.join("plates.lbrn2");
        crate::menus::invoke(&mut app, "file.exportLightBurn", json!({"path": lb.to_string_lossy()}));
        let xml = std::fs::read_to_string(&lb).unwrap();
        assert!(xml.contains("<LightBurnProject") && xml.matches("<Shape ").count() == 2, "{xml}");
        let job = dir.join("job");
        crate::menus::invoke(&mut app, "file.exportJob", json!({"path": job.to_string_lossy()}));
        assert!(job.join("job.json").exists() && job.join("Sheet 1.lbrn2").exists());
        let text = crate::tests_labels::painted_text(&mut app, show);
        for want in ["For LightBurn…", "Export Job…"] {
            assert!(text.contains(want), "{want:?} missing from:\n{text}");
        }
        assert!(crate::menus::enabled(&app, "file.exportLightBurn") && crate::menus::enabled(&app, "file.exportJob"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
