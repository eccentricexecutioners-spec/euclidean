//! Parameters panel (Euclidean): the drawing's user parameters and the parametric sketch they
//! drive. Favourites across the top with a slider and − / + each; then every parameter as a
//! row (star, name, unit, expression, value, comment); then the selected sketch's shapes with
//! their expression fields. Everything runs a `parameters.*` / `sketch.*` command, so the panel,
//! the control bar's sliders and an agent drive the drawing the same way; a slider drag previews
//! through the session's interaction, so one drag is one undo step.

use egui::Ui;
use serde_json::json;
use vectorcraft_doc::params::{self, ParamRow};
use vectorcraft_doc::sketch;
use vectorcraft_doc::{Node, NodeId};
use vectorcraft_engine::DocState;

use crate::VectorcraftApp;
use crate::widgets::{self, Live};

/// The panel id, as the dock, the Window menu and `window.panel` name it.
pub const ID: &str = "parameters";

/// One parameter as the panel shows it.
pub(crate) struct Row {
    pub row: ParamRow,
    /// The solved value in the row's unit.
    pub value: Option<f64>,
    pub error: Option<String>,
    /// A plain number with a range: what a slider can drive.
    pub slides: bool,
}

/// Every parameter, solved.
pub(crate) fn rows(st: &DocState) -> Vec<Row> {
    let rows = params::rows(&st.doc);
    let solved = params::solve(&rows);
    rows.into_iter()
        .map(|row| {
            let value = solved.value(&row.name).map(|v| params::to_unit(v, &row.unit));
            let error = solved.error(&row.name).map(str::to_string);
            let slides = row.is_plain() && row.min.is_some() && row.max.is_some() && error.is_none();
            Row { row, value, error, slides }
        })
        .collect()
}

/// The one selected parametric sketch, if the selection is exactly that.
pub(crate) fn selected_sketch(st: &DocState) -> Option<NodeId> {
    match st.selection.objects.as_slice() {
        [one] if st.doc.node(*one).is_some_and(|n| n.extras().sketch.is_some()) => Some(*one),
        _ => None,
    }
}

/// The value a row shows: `120 mm`, `4`, or `error`.
fn shown_value(r: &Row) -> String {
    if r.error.is_some() { tl!("error").to_string() } else { params::format_value(r.value.map(|v| params::from_unit(v, &r.row.unit)), &r.row.unit) }
}

/// Set `name` to `value` (in its unit) as a plain expression: live while a slider drags.
pub(crate) fn set_value(app: &mut VectorcraftApp, row: &ParamRow, value: f64, phase: Live) {
    let v = match (row.min, row.max) {
        (Some(a), Some(b)) if a <= b => value.clamp(a, b),
        _ => value,
    };
    let params = json!({"name": row.name, "expression": params::plain_expression(v, &row.unit)});
    crate::panels::live_run(app, "Set Parameter", "parameters.set", params, phase);
}

/// A favourite's slider with − and + : the shared drawing of the panel and the control bar.
/// Returns true when it drew a slider (a formula draws its value only).
pub(crate) fn slider_row(app: &mut VectorcraftApp, ui: &mut Ui, r: &Row, width: f32) -> bool {
    if !r.slides {
        return false;
    }
    let (min, max) = (r.row.min.unwrap_or(0.0), r.row.max.unwrap_or(1.0));
    let step = r.row.step.filter(|s| *s > 0.0).unwrap_or(1.0);
    let mut v = r.value.unwrap_or(min);
    if widgets::icon_button(ui, "minus", tl!("Decrease"), false, 18.0).clicked() {
        set_value(app, &r.row, v - step, Live::Released);
    }
    let resp = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = width;
            widgets::slider(ui, egui::Slider::new(&mut v, min.min(max)..=max.max(min)).step_by(step).show_value(false))
        })
        .inner;
    if resp.changed() || resp.drag_stopped() {
        let phase = if resp.dragged() && !resp.drag_stopped() { Live::Dragging } else { Live::Released };
        set_value(app, &r.row, v, phase);
    }
    if widgets::icon_button(ui, "plus", tl!("Increase"), false, 18.0).clicked() {
        set_value(app, &r.row, v + step, Live::Released);
    }
    true
}

/// A shape field's label, translated (literals, so the catalogue gate sees them).
fn field_label(kind: &str, field: &str) -> &'static str {
    match sketch::field_label(kind, field) {
        "x" => tl!("x"),
        "y" => tl!("y"),
        "width" => tl!("width"),
        "height" => tl!("height"),
        "corner radius" => tl!("corner radius"),
        "centre x" => tl!("centre x"),
        "centre y" => tl!("centre y"),
        "diameter" => tl!("diameter"),
        "copies" => tl!("copies"),
        "pitch" => tl!("pitch"),
        "length" => tl!("length"),
        "angle" => tl!("angle"),
        other => other,
    }
}

/// A shape kind's label, translated.
fn kind_label(kind: &str) -> &str {
    match kind {
        "rect" => tl!("rect"),
        "circle" => tl!("circle"),
        "line" => tl!("line"),
        other => other,
    }
}

/// The unit dropdown's entries (`""` shows as "No unit").
fn unit_label(unit: &str) -> String {
    if unit.is_empty() { tl!("No unit").to_string() } else { unit.to_string() }
}

/// The unit dropdown's entries, as shown.
pub(crate) fn unit_labels() -> Vec<String> {
    params::UNITS.iter().map(|u| unit_label(u)).collect()
}

/// One parameter's cells: star, name, unit, expression, value, comment. Each edit is one command
/// (`parameters.set` / `parameters.rename`), so one undo step. Shared by this panel's table and
/// the Change Parameters dialog's.
pub(crate) fn edit_row(app: &mut VectorcraftApp, ui: &mut Ui, r: &Row, unit_names: &[&str]) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let name = r.row.name.clone();
    if widgets::icon_button(ui, "star", tl!("Favorite"), r.row.favourite, 18.0).clicked() {
        app.run("parameters.set", json!({"name": name, "favourite": !r.row.favourite})).ok();
    }
    if let Some(to) = widgets::exact_text_field(ui, "name", &name, 72.0)
        && let Err(e) = app.run("parameters.rename", json!({"from": name, "to": to.trim()}))
    {
        app.ui.status = e;
    }
    let current = unit_label(&r.row.unit);
    if let Some(i) = widgets::dropdown_names(ui, "unit", &current, unit_names, 64.0)
        && let Some(u) = params::UNITS.get(i)
    {
        app.run("parameters.set", json!({"name": name, "unit": u})).ok();
    }
    if let Some(e) = widgets::exact_text_field(ui, "expr", &r.row.expression, 96.0) {
        app.run("parameters.set", json!({"name": name, "expression": e})).ok();
    }
    let value = ui.label(egui::RichText::new(shown_value(r)).size(12.0).color(if r.error.is_some() { t.accent_strong } else { t.text }));
    if let Some(err) = &r.error {
        value.on_hover_text(err);
    }
    if let Some(c) = widgets::exact_text_field(ui, "comment", &r.row.comment, 96.0) {
        app.run("parameters.set", json!({"name": name, "comment": c})).ok();
    }
}

pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let Some(st) = app.session.active() else {
        widgets::dim_label(ui, tl!("No document"));
        return;
    };
    // Owned snapshots: the rows below run commands, which borrow the app mutably.
    let rows = rows(st);
    let sketch = selected_sketch(st).and_then(|id| st.doc.node(id).and_then(|n| n.extras().sketch.as_deref().cloned()).map(|s| (id, s)));
    let built = sketch.as_ref().map(|(_, s)| sketch::build(s, &params::solve(&params::rows(&st.doc))));
    let t = crate::theme::Tokens::get(ui.ctx());

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Favourites: a slider each, or the value of a formula.
        widgets::subheader(ui, tl!("Favorites"));
        let favourites: Vec<&Row> = rows.iter().filter(|r| r.row.favourite).collect();
        if favourites.is_empty() {
            widgets::dim_label(ui, tl!("No favorites yet. Star a parameter below."));
        }
        for r in favourites {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&r.row.name).size(12.5).color(t.text_strong));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(shown_value(r)).size(12.5).color(if r.error.is_some() { t.accent_strong } else { t.text }));
                });
            });
            ui.horizontal(|ui| {
                ui.push_id(("fav", &r.row.name), |ui| {
                    if !slider_row(app, ui, r, (ui.available_width() - 60.0).max(60.0)) {
                        widgets::dim_label(ui, tl!("follows a formula · edit it below"));
                    }
                });
            });
        }

        // Every parameter.
        ui.add_space(4.0);
        widgets::subheader(ui, tl!("Parameters"));
        if rows.is_empty() {
            widgets::dim_label(ui, tl!("None"));
        }
        let units = unit_labels();
        let unit_names: Vec<&str> = units.iter().map(String::as_str).collect();
        egui::Grid::new("parameters-table").num_columns(7).spacing([4.0, 2.0]).show(ui, |ui| {
            ui.label("");
            widgets::field_label(ui, egui::RichText::new(tl!("Name")).size(11.0).color(t.text_dim));
            widgets::field_label(ui, egui::RichText::new(tl!("Unit")).size(11.0).color(t.text_dim));
            widgets::field_label(ui, egui::RichText::new(tl!("Expression")).size(11.0).color(t.text_dim));
            widgets::field_label(ui, egui::RichText::new(tl!("Value")).size(11.0).color(t.text_dim));
            widgets::field_label(ui, egui::RichText::new(tl!("Comment")).size(11.0).color(t.text_dim));
            ui.label("");
            ui.end_row();
            for r in &rows {
                ui.push_id(("row", &r.row.name), |ui| {
                    edit_row(app, ui, r, &unit_names);
                    if widgets::icon_button(ui, "trash-2", tl!("Delete Parameter"), false, 18.0).clicked() {
                        app.run("parameters.remove", json!({"name": r.row.name})).ok();
                    }
                });
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            if widgets::icon_button(ui, "plus", tl!("New Parameter"), false, 24.0).clicked() {
                app.run("parameters.add", json!({})).ok();
            }
        });

        // The selected sketch's shapes.
        ui.add_space(4.0);
        widgets::subheader(ui, tl!("Selected Sketch"));
        let Some(((id, sketch), built)) = sketch.zip(built) else {
            widgets::dim_label(ui, tl!("Select a parametric sketch to see what drives it."));
            return;
        };
        for (shape, b) in sketch.shapes.iter().zip(&built) {
            ui.push_id(("shape", &shape.id), |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{} · {}", shape.id, kind_label(&shape.kind))).size(12.5).color(t.text_strong));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, "trash-2", tl!("Remove Shape"), false, 18.0).clicked() {
                            app.run("sketch.removeShape", json!({"id": id.0, "shape": shape.id})).ok();
                        }
                    });
                });
                egui::Grid::new("fields").num_columns(3).spacing([6.0, 2.0]).show(ui, |ui| {
                    for field in sketch::fields_of(&shape.kind).unwrap_or(&[]) {
                        widgets::field_label(ui, egui::RichText::new(field_label(&shape.kind, field)).size(12.0).color(t.text_dim));
                        let expression = shape.fields.get(*field).cloned().unwrap_or_default();
                        if let Some(e) = widgets::exact_text_field(ui, ("field", *field), &expression, 110.0) {
                            app.run("sketch.setField", json!({"id": id.0, "shape": shape.id, "field": field, "expression": e})).ok();
                        }
                        let unit = if sketch::unitless(field) { "" } else { "mm" };
                        let (text, err) = match b.fields.get(*field) {
                            Some(Ok(v)) => (params::format_value(Some(*v), unit), None),
                            Some(Err(e)) => (tl!("error").to_string(), Some(e.clone())),
                            None => ("—".to_string(), None),
                        };
                        let label = ui.label(egui::RichText::new(text).size(12.0).color(if err.is_some() { t.accent_strong } else { t.text }));
                        if let Some(e) = err {
                            label.on_hover_text(e);
                        }
                        ui.end_row();
                    }
                });
            });
        }
        ui.horizontal(|ui| {
            let tips = [("rect", "square", tl!("Add Rectangle")), ("circle", "circle", tl!("Add Circle")), ("line", "dc-line", tl!("Add Line"))];
            for (kind, icon, tip) in tips {
                if widgets::icon_button(ui, icon, tip, false, 24.0).clicked() {
                    app.run("sketch.addShape", json!({"id": id.0, "kind": kind})).ok();
                }
            }
        });
    });
}

/// The panel's (≡) menu.
pub fn menu(app: &mut VectorcraftApp, ui: &mut Ui) {
    let Some(st) = app.session.active() else { return };
    let has_rows = !params::rows(&st.doc).is_empty();
    let sketch = selected_sketch(st);
    if widgets::menu_item(ui, tl!("New Parameter"), true, false) {
        app.run("parameters.add", json!({})).ok();
    }
    if widgets::menu_item(ui, tl!("Copy Parameters as CSV"), has_rows, false)
        && let Ok(v) = app.run("parameters.csv", json!({}))
        && let Some(csv) = v["csv"].as_str()
    {
        ui.ctx().copy_text(csv.to_string());
    }
    ui.separator();
    for (label, kind) in [("New Rectangle", "rect"), ("New Circle", "circle"), ("New Line", "line")] {
        if widgets::menu_item(ui, tl!(label), true, false) {
            match sketch {
                Some(id) => app.run("sketch.addShape", json!({"id": id.0, "kind": kind})).ok(),
                None => app.run("sketch.create", json!({"shapes": [{"kind": kind}]})).ok(),
            };
        }
    }
    if widgets::menu_item(ui, tl!("Sample: Mounting plate"), true, false) {
        app.run("sketch.sample", json!({})).ok();
    }
}

/// Is `n` a parametric sketch group (the control bar names it)?
pub(crate) fn is_sketch(n: &Node) -> bool {
    n.extras().sketch.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_sample() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width": 600, "height": 400})).unwrap();
        app.session.execute("sketch.sample", &json!({})).unwrap();
        app
    }

    #[test]
    fn draws_headless() {
        let mut app = app_with_sample();
        let text = crate::tests_labels::painted_text(&mut app, show);
        for s in ["Favorites", "Parameters", "width", "hole_dia", "120 mm", "Selected Sketch", "R1", "C1", "pitch"] {
            assert!(text.contains(s), "{s} missing:\n{text}");
        }
        // Without a selection the sketch section says what to do.
        app.session.execute("select.none", &json!({})).unwrap();
        let text = crate::tests_labels::painted_text(&mut app, show);
        assert!(text.contains("Select a parametric sketch"), "{text}");
        // Without a document.
        let mut empty = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let text = crate::tests_labels::painted_text(&mut empty, show);
        assert!(text.contains("No document"), "{text}");
    }

    #[test]
    fn the_panel_menu_lists_its_commands() {
        let mut app = app_with_sample();
        let text = crate::tests_labels::painted_text(&mut app, menu);
        for item in ["New Parameter", "Copy Parameters as CSV", "New Rectangle", "New Circle", "New Line", "Sample: Mounting plate"] {
            assert!(text.contains(item), "{item} missing from the panel menu:\n{text}");
        }
    }

    #[test]
    fn a_slider_value_is_one_undo_step() {
        let mut app = app_with_sample();
        let width = rows(app.session.doc().unwrap()).into_iter().find(|r| r.row.name == "width").unwrap();
        assert!(width.slides);
        let before = app.session.doc().unwrap().history.undo.len();
        set_value(&mut app, &width.row, 150.0, Live::Dragging);
        set_value(&mut app, &width.row, 160.0, Live::Dragging);
        set_value(&mut app, &width.row, 170.0, Live::Released);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before + 1);
        let list = app.run("parameters.list", json!({})).unwrap();
        assert_eq!(list["rows"][0]["expression"], "170 mm");
        // Clamped to the range.
        set_value(&mut app, &width.row, 1000.0, Live::Released);
        assert_eq!(app.run("parameters.list", json!({})).unwrap()["rows"][0]["expression"], "300 mm");
    }

    #[test]
    fn the_panel_is_registered_with_its_own_icon() {
        let row = crate::state::ICON_PANELS.iter().find(|(id, ..)| *id == ID).expect("in the icon column");
        assert_eq!(row.1, "Parameters");
        assert!(crate::icons::exists(row.2), "{} is not a bundled icon", row.2);
        assert!(crate::state::ICON_PANEL_GROUPS.iter().any(|g| g.contains(&ID)), "{ID} is in no ICON_PANEL_GROUPS row");
    }

    #[test]
    fn window_parameters_opens_the_panel_and_object_has_the_submenu() {
        let app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let entries = crate::menus::menu_entries(&app);
        // Window > Parameters now opens the Change Parameters dialog; the panel stays reachable by `window.panel`.
        let dialog = entries.iter().find(|e| e.command.as_deref() == Some("window.changeParameters")).expect("Window > Parameters is listed");
        assert_eq!((dialog.path.clone(), dialog.label.as_str()), (vec!["Window".to_string()], "Parameters"));
        assert!(
            !entries
                .iter()
                .any(|e| e.command.as_deref() == Some("window.panel") && e.params.get("panel").and_then(serde_json::Value::as_str) == Some(ID)),
            "the panel is no longer a Window menu row"
        );
        assert!(crate::menus::UI_COMMANDS.iter().any(|c| c.0 == "window.panel"));
        let sketch: Vec<&str> = entries.iter().filter(|e| e.path == ["Object", "Parametric Sketch"]).map(|e| e.label.as_str()).collect();
        assert_eq!(sketch, ["New Sketch", "New Rectangle", "New Circle", "New Line", "Sample: Mounting plate"]);
    }
}
