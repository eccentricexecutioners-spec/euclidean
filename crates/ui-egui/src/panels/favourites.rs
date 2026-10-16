//! Favorites palette (Euclidean, E1 Part C): the starred parameters as big controls, the way
//! FusionMonkey's palette drives Fusion 360. One card per starred parameter: its name and value,
//! a − / slider / + row with a typed field and the unit, and the slider's ends, editable. No
//! expression editing here: that is the Parameters table's. Changes apply live (a drag is one undo
//! step, as the table's slider was). Looks: a named set of values (`parameters.looks.*`), saved
//! in the document, recalled from a row of chips.

use egui::Ui;
use serde_json::{Value, json};
use vectorcraft_doc::params;

use super::parameters::{Row, rows, set_value};
use crate::VectorcraftApp;
use crate::widgets::{self, Live};

/// The panel id, as the dock, the Window menu and `window.panel` name it.
pub const ID: &str = "favourites";

/// The slider's ends: the row's own range, else a quarter to four times the value (a zero value
/// gets 0 … 10), as the prompt asks; the step the row's, else 1 (no unit, degrees), 0.5 (mm) or
/// 0.125 (in).
fn range(r: &Row) -> (f64, f64, f64) {
    let v = r.value.unwrap_or(0.0);
    let (dmin, dmax, dstep) = params::default_range(v, &r.row.unit);
    let min = r.row.min.unwrap_or(dmin);
    let max = r.row.max.unwrap_or(dmax);
    let step = r.row.step.filter(|s| *s > 0.0).unwrap_or(dstep);
    if max > min { (min, max, step) } else { (min, min + 10.0, step) }
}

/// A number as the ends and the field show it.
fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

/// Set one end of the range (`min` or `max`) or the step.
fn set_range(app: &mut VectorcraftApp, name: &str, key: &str, v: f64) {
    app.run("parameters.set", json!({"name": name, key: v})).ok();
}

/// One favourite as a card: name and value, the − / slider / + row with its typed field and unit,
/// the ends of the slider under it. A formula (an expression that is not a plain number) shows its
/// value and says where to edit it.
fn card(app: &mut VectorcraftApp, ui: &mut Ui, r: &Row) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let name = r.row.name.clone();
    let unit = r.row.unit.clone();
    let shown = match &r.error {
        Some(_) => tl!("error").to_string(),
        None => params::format_value(r.value.map(|v| params::from_unit(v, &unit)), &unit),
    };
    egui::Frame::NONE.fill(t.panel_darker).corner_radius(8).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&name).font(crate::theme::semibold(13.0)).color(t.text_strong));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(shown).size(13.0).color(if r.error.is_some() { t.error } else { t.text }));
            });
        });
        if !r.row.is_plain() || r.error.is_some() {
            widgets::dim_label(ui, tl!("Follows a formula: edit it in the Parameters table."));
            return;
        }
        let (min, max, step) = range(r);
        let mut v = r.value.unwrap_or(min);
        // − slider + field unit: the slider takes what the rest leaves.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if widgets::icon_button(ui, "minus", tl!("Decrease"), false, 22.0).clicked() {
                set_value(app, &r.row, v - step, Live::Released);
            }
            let field_w = 64.0;
            let unit_w = if unit.is_empty() { 0.0 } else { 26.0 };
            let slider_w = (ui.available_width() - 22.0 - field_w - unit_w - 20.0).max(60.0);
            let resp = ui
                .scope(|ui| {
                    ui.spacing_mut().slider_width = slider_w;
                    widgets::slider(ui, egui::Slider::new(&mut v, min..=max).step_by(step).show_value(false))
                })
                .inner;
            if resp.changed() || resp.drag_stopped() {
                let phase = if resp.dragged() && !resp.drag_stopped() { Live::Dragging } else { Live::Released };
                set_value(app, &r.row, v, phase);
            }
            if widgets::icon_button(ui, "plus", tl!("Increase"), false, 22.0).clicked() {
                set_value(app, &r.row, v + step, Live::Released);
            }
            if let Some(text) = widgets::exact_text_field(ui, ("fav-value", &name), &num(v), field_w)
                && let Ok(n) = text.trim().parse::<f64>()
            {
                set_value(app, &r.row, n, Live::Released);
            }
            if !unit.is_empty() {
                widgets::dim_label(ui, &unit);
            }
        });
        // The ends and the step, editable.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(text) = widgets::exact_text_field(ui, ("fav-min", &name), &num(min), 56.0)
                && let Ok(n) = text.trim().parse::<f64>()
            {
                set_range(app, &name, "min", n);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(text) = widgets::exact_text_field(ui, ("fav-max", &name), &num(max), 56.0)
                    && let Ok(n) = text.trim().parse::<f64>()
                {
                    set_range(app, &name, "max", n);
                }
                widgets::dim_label(ui, &format!("{} {}", tl!("step"), num(step)));
            });
        });
    });
    ui.add_space(6.0);
}

/// The looks (named value sets) of the active document: `{name, values}` rows.
fn looks(app: &VectorcraftApp) -> Vec<(String, usize)> {
    let Some(st) = app.session.active() else { return vec![] };
    let Some(v) = st.doc.unknown.get("euclidean.looks") else { return vec![] };
    v.get("looks")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|l| Some((l.get("name")?.as_str()?.to_string(), l.get("values").and_then(Value::as_object).map_or(0, |o| o.len()))))
                .collect()
        })
        .unwrap_or_default()
}

/// The Looks row: a chip per saved look (click applies it, its × removes it) and Save Look.
fn looks_row(app: &mut VectorcraftApp, ui: &mut Ui) {
    let t = crate::theme::Tokens::get(ui.ctx());
    widgets::subheader(ui, tl!("Looks"));
    let list = looks(app);
    let mut apply: Option<String> = None;
    let mut delete: Option<String> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for (name, n) in &list {
            let resp = ui.add(egui::Button::new(egui::RichText::new(name).size(12.0).color(t.text_strong)).corner_radius(10));
            if resp.on_hover_text(format!("{n} {}", tl!("values"))).clicked() {
                apply = Some(name.clone());
            }
            if widgets::icon_button(ui, "x", tl!("Remove Look"), false, 16.0).clicked() {
                delete = Some(name.clone());
            }
        }
        if widgets::icon_button(ui, "plus", tl!("Save Look"), false, 22.0).clicked() {
            app.run("parameters.looks.save", json!({})).ok();
        }
    });
    if list.is_empty() {
        widgets::dim_label(ui, tl!("Save the favourites' values as a look to come back to them."));
    }
    if let Some(name) = apply {
        app.run("parameters.looks.apply", json!({"name": name})).ok();
    }
    if let Some(name) = delete {
        app.run("parameters.looks.delete", json!({"name": name})).ok();
    }
}

pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let Some(st) = app.session.active() else {
        widgets::dim_label(ui, tl!("No document"));
        return;
    };
    // Owned snapshots: the cards below run commands, which borrow the app mutably.
    let favourites: Vec<Row> = rows(st).into_iter().filter(|r| r.row.favourite).collect();
    let width = ui.available_width();
    ui.scope(|ui| {
        ui.set_max_width(width);
        if favourites.is_empty() {
            super::empty_state(ui, "star", tl!("No favorites yet"), tl!("Star a parameter in the Parameters table and it appears here as a slider."));
        }
        for r in &favourites {
            ui.push_id(("fav", &r.row.name), |ui| card(app, ui, r));
        }
        looks_row(app, ui);
    });
}

/// The panel's (≡) menu.
pub fn menu(app: &mut VectorcraftApp, ui: &mut Ui) {
    if widgets::menu_item(ui, tl!("Save Look"), app.session.active().is_some(), false) {
        app.run("parameters.looks.save", json!({})).ok();
    }
    if widgets::menu_item(ui, tl!("Open Parameters"), true, false) {
        app.run("window.panel", json!({"panel": super::parameters::ID})).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_engine::Session;

    /// A drawn sketch: a 50 mm line with its length dimension `d1`, and a user parameter `gap`.
    fn app() -> (VectorcraftApp, u64) {
        let mut app = VectorcraftApp::new(Session::new(), Default::default());
        app.run("file.new", json!({"width": 400, "height": 300})).unwrap();
        let id = app.run("sketch.new", json!({})).unwrap()["id"].as_u64().unwrap();
        app.run("sketch.addLine", json!({"id": id, "from": [10.0, 40.0], "to": [60.0, 40.0], "length": "50", "horizontal": true})).unwrap();
        app.run("parameters.add", json!({"name": "gap", "expression": "4 mm"})).unwrap();
        (app, id)
    }

    fn line_length(app: &mut VectorcraftApp, id: u64) -> f64 {
        let i = app.run("sketch.inspect", json!({"id": id})).unwrap();
        let pts = i["points"].as_array().unwrap().clone();
        let at = |n: u64| pts.iter().find(|p| p["id"] == n).map(|p| (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap())).unwrap();
        let e = &i["entities"][0];
        let (a, b) = (at(e["a"].as_u64().unwrap()), at(e["b"].as_u64().unwrap()));
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }

    #[test]
    fn starring_a_parameter_puts_it_in_the_palette_and_unstarring_takes_it_out() {
        let (mut app, _) = app();
        let text = crate::tests_labels::painted_text(&mut app, show);
        assert!(text.contains("No favorites yet"), "{text}");
        app.run("parameters.set", json!({"name": "d1", "favourite": true})).unwrap();
        let text = crate::tests_labels::painted_text(&mut app, show);
        assert!(text.contains("d1") && text.contains("50 mm") && !text.contains("No favorites yet"), "{text}");
        assert!(!text.contains("gap"), "only starred rows:\n{text}");
        app.run("parameters.set", json!({"name": "d1", "favourite": false})).unwrap();
        let text = crate::tests_labels::painted_text(&mut app, show);
        assert!(text.contains("No favorites yet"), "{text}");
    }

    #[test]
    fn a_slider_move_changes_the_sketch_in_one_undo_step() {
        let (mut app, id) = app();
        app.run("parameters.set", json!({"name": "d1", "favourite": true})).unwrap();
        let row = rows(app.session.doc().unwrap()).into_iter().find(|r| r.row.name == "d1").unwrap();
        // The default range: a quarter to four times the value.
        assert_eq!((row.row.min, row.row.max), (Some(12.5_f64.round()), Some(200.0)));
        assert!((line_length(&mut app, id) - 50.0).abs() < 1e-6);
        let before = app.session.doc().unwrap().history.undo.len();
        set_value(&mut app, &row.row, 60.0, Live::Dragging);
        set_value(&mut app, &row.row, 70.0, Live::Dragging);
        set_value(&mut app, &row.row, 80.0, Live::Released);
        assert!((line_length(&mut app, id) - 80.0).abs() < 1e-6, "the line follows the slider");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before + 1, "one drag is one undo step");
    }

    #[test]
    fn an_expression_that_names_another_parameter_follows_it() {
        let (mut app, id) = app();
        app.run("sketch.setDimension", json!({"id": id, "name": "d1", "expression": "gap * 10"})).unwrap();
        assert!((line_length(&mut app, id) - 40.0).abs() < 1e-6);
        app.run("parameters.set", json!({"name": "gap", "expression": "6 mm"})).unwrap();
        assert!((line_length(&mut app, id) - 60.0).abs() < 1e-6, "d1 = gap * 10 follows gap");
        // A formula is not slid: the palette says where to edit it.
        app.run("parameters.set", json!({"name": "d1", "favourite": true})).unwrap();
        let text = crate::tests_labels::painted_text(&mut app, show);
        assert!(text.contains("Follows a formula"), "{text}");
    }

    #[test]
    fn a_look_saves_the_favourites_values_and_brings_them_back() {
        let (mut app, id) = app();
        app.run("parameters.set", json!({"name": "d1", "favourite": true})).unwrap();
        let saved = app.run("parameters.looks.save", json!({"name": "Wide"})).unwrap();
        assert_eq!(saved["values"], 1);
        app.run("sketch.setDimension", json!({"id": id, "name": "d1", "expression": "30"})).unwrap();
        assert!((line_length(&mut app, id) - 30.0).abs() < 1e-6);
        let text = crate::tests_labels::painted_text(&mut app, show);
        assert!(text.contains("Wide"), "the look is a chip:\n{text}");
        let r = app.run("parameters.looks.apply", json!({"name": "Wide"})).unwrap();
        assert_eq!((r["applied"].clone(), r["missing"].as_array().unwrap().len()), (json!(1), 0));
        assert!((line_length(&mut app, id) - 50.0).abs() < 1e-6, "the look brings 50 back");
        app.run("parameters.looks.delete", json!({"name": "Wide"})).unwrap();
        assert!(app.run("parameters.looks.list", json!({})).unwrap()["looks"].as_array().unwrap().is_empty());
        // The look rides in `document.unknown`: a 0.1.3 build opens the file and keeps the key.
        assert!(app.run("parameters.looks.apply", json!({"name": "Wide"})).is_err());
    }

    #[test]
    fn the_panel_is_registered_and_sits_in_the_sketch_tabs_right_panel() {
        let row = crate::state::ICON_PANELS.iter().find(|(id, ..)| *id == ID).expect("in the icon column");
        assert_eq!(row.1, "Favorites");
        assert!(crate::icons::exists(row.2));
        let (lay, _) = crate::workbench::resolve(&crate::workbench::default_layout());
        let sketch = lay.tabs.iter().find(|t| t.id == "sketch").unwrap();
        assert_eq!(sketch.right, ["favourites", "parameters"]);
        let draw = lay.tabs.iter().find(|t| t.id == "draw").unwrap();
        assert!(draw.groups.iter().any(|g| g.show.iter().any(|i| i.id() == Some("favourites"))), "reachable in DRAW");
    }
}
