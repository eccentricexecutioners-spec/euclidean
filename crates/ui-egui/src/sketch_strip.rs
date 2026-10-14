//! The control bar's parametric-sketch strip (Euclidean): while one parametric sketch is
//! selected, every favourite parameter that a slider can drive sits in the bar as a slider with
//! − and +, so the plate widens without opening a panel. The sliders are the Parameters panel's
//! ([`crate::panels::parameters::slider_row`]); a drag is one undo step.

use egui::Ui;

use crate::VectorcraftApp;
use crate::panels::parameters;

/// Draw the strip when exactly one parametric sketch is selected; false otherwise.
pub fn control_bar(app: &mut VectorcraftApp, ui: &mut Ui) -> bool {
    let Some(st) = app.session.active() else { return false };
    if parameters::selected_sketch(st).is_none() {
        return false;
    }
    let rows: Vec<parameters::Row> = parameters::rows(st).into_iter().filter(|r| r.row.favourite && r.slides).collect();
    let t = crate::theme::Tokens::get(ui.ctx());
    if rows.is_empty() {
        if ui.link(egui::RichText::new(tl!("Parameters")).size(12.0).color(t.text).underline()).clicked() {
            app.ui.open_panel = Some(parameters::ID.into());
        }
        ui.separator();
        return true;
    }
    for r in &rows {
        if ui.available_width() < 180.0 {
            break;
        }
        ui.push_id(("strip", &r.row.name), |ui| {
            if ui.link(egui::RichText::new(&r.row.name).size(12.0).color(t.text).underline()).clicked() {
                app.ui.open_panel = Some(parameters::ID.into());
            }
            parameters::slider_row(app, ui, r, 72.0);
            let value = vectorcraft_doc::params::format_value(r.value.map(|v| vectorcraft_doc::params::from_unit(v, &r.row.unit)), &r.row.unit);
            ui.label(egui::RichText::new(value).size(12.0).color(t.text_dim));
        });
        ui.add_space(4.0);
    }
    ui.separator();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_strip_draws_the_favourites_for_a_selected_sketch_only() {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width": 600, "height": 400})).unwrap();
        app.session.execute("sketch.sample", &json!({})).unwrap();
        let text = crate::tests_labels::painted_text(&mut app, |app, ui| {
            control_bar(app, ui);
        });
        for s in ["width", "height", "hole_dia", "holes", "120 mm"] {
            assert!(text.contains(s), "{s} missing:\n{text}");
        }
        assert!(!text.contains("margin"), "unstarred: {text}");
        app.session.execute("select.none", &json!({})).unwrap();
        let text = crate::tests_labels::painted_text(&mut app, |app, ui| {
            control_bar(app, ui);
        });
        assert!(text.trim().is_empty(), "{text}");
    }
}
