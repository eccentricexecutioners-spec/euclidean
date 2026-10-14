//! The favourites row's drawing (Euclidean): every favourite parameter that a slider can drive
//! as its name, a short slider and a value box. [`crate::sketch::strip`] puts it in a slim panel
//! over the canvas, in every workspace, while a parametric sketch is selected or being edited. The
//! sliders are the Parameters panel's ([`crate::panels::parameters::slider_row`] is not used here:
//! the strip is one row, so no − / +); a drag is one undo step, a typed value is one too.

use egui::Ui;

use crate::VectorcraftApp;
use crate::panels::parameters;
use crate::widgets::Live;

/// Width of a favourite's slider, points.
const SLIDER_WIDTH: f32 = 90.0;

/// The sketch the strip speaks for: the one being edited, else the one selected.
pub fn current(app: &VectorcraftApp) -> Option<vectorcraft_doc::NodeId> {
    if crate::sketch::active(app) {
        return app.sketch.as_ref().map(|m| m.id);
    }
    app.session.active().and_then(parameters::selected_sketch)
}

/// Draw the favourites in a row (call inside a horizontal layout); false when no sketch is current.
pub fn draw(app: &mut VectorcraftApp, ui: &mut Ui) -> bool {
    if current(app).is_none() {
        return false;
    }
    let Some(st) = app.session.active() else { return false };
    let rows: Vec<parameters::Row> = parameters::rows(st).into_iter().filter(|r| r.row.favourite && r.slides).collect();
    let t = crate::theme::Tokens::get(ui.ctx());
    if rows.is_empty() {
        if ui.link(egui::RichText::new(tl!("Change Parameters")).size(12.0).color(t.text).underline()).clicked() {
            app.ui.change_parameters = true;
        }
        return true;
    }
    for r in &rows {
        ui.push_id(("strip", &r.row.name), |ui| {
            if ui.link(egui::RichText::new(&r.row.name).size(12.0).color(t.text).underline()).clicked() {
                app.ui.change_parameters = true;
            }
            favourite(app, ui, r);
        });
        ui.add_space(8.0);
    }
    true
}

/// A favourite's slider and value box; every change goes through [`parameters::set_value`].
fn favourite(app: &mut VectorcraftApp, ui: &mut Ui, r: &parameters::Row) {
    let (min, max) = (r.row.min.unwrap_or(0.0), r.row.max.unwrap_or(1.0));
    let (lo, hi) = (min.min(max), max.max(min));
    let step = r.row.step.filter(|s| *s > 0.0).unwrap_or(1.0);
    let mut v = r.value.unwrap_or(min);
    let slider = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = SLIDER_WIDTH;
            crate::widgets::slider(ui, egui::Slider::new(&mut v, lo..=hi).step_by(step).show_value(false))
        })
        .inner;
    let mut changed = (slider.changed() || slider.drag_stopped()).then(|| slider.dragged() && !slider.drag_stopped());
    let mut drag = egui::DragValue::new(&mut v).speed(step).range(lo..=hi);
    if !r.row.unit.is_empty() {
        drag = drag.suffix(format!(" {}", r.row.unit));
    }
    let boxed = ui.add(drag);
    if boxed.changed() || boxed.drag_stopped() {
        changed = Some(boxed.dragged() && !boxed.drag_stopped());
    }
    if let Some(dragging) = changed {
        parameters::set_value(app, &r.row, v, if dragging { Live::Dragging } else { Live::Released });
    }
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
            draw(app, ui);
        });
        for s in ["width", "height", "hole_dia", "holes", "120", "80"] {
            assert!(text.contains(s), "{s} missing:\n{text}");
        }
        assert!(!text.contains("margin"), "unstarred: {text}");
        app.session.execute("select.none", &json!({})).unwrap();
        let text = crate::tests_labels::painted_text(&mut app, |app, ui| {
            draw(app, ui);
        });
        assert!(text.trim().is_empty(), "{text}");
    }
}
