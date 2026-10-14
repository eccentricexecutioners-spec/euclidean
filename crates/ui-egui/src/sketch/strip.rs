//! The slim favourites strip (Euclidean): one row over the canvas, in every workspace, while one
//! parametric sketch is selected or being edited. Its content is [`crate::sketch_strip::draw`].

use egui::Ui;

use crate::VectorcraftApp;

/// The strip's height, points: one row.
pub const HEIGHT: f32 = 28.0;

/// Draw the strip as a top panel of the ui it is given (the central panel, after the tabs and
/// before the canvas). Returns its height, or `None` when no sketch is current.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) -> Option<f32> {
    crate::sketch_strip::current(app)?;
    let t = crate::theme::Tokens::get(ui.ctx());
    let frame = egui::Frame::NONE.fill(t.panel).stroke(egui::Stroke::new(1.0, t.border)).inner_margin(egui::Margin::symmetric(8, 2));
    let panel = egui::Panel::top("sketch_favourites").exact_size(HEIGHT).resizable(false).frame(frame).show(ui, |ui| {
        egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                crate::sketch_strip::draw(app, ui);
            });
        });
    });
    Some(panel.response.rect.height())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn app_with_sample() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width": 600, "height": 400})).unwrap();
        app.session.execute("sketch.sample", &json!({})).unwrap();
        app
    }

    #[test]
    fn the_strip_is_one_row_for_a_selected_sketch_in_essentials() {
        let mut app = app_with_sample();
        app.ui.control_bar = false;
        let mut height = None;
        let text = crate::tests_labels::painted_text(&mut app, |app, ui| height = show(app, ui));
        for s in ["width", "height", "hole_dia", "holes"] {
            assert!(text.contains(s), "{s} missing:\n{text}");
        }
        assert!(!text.contains("margin"), "unstarred: {text}");
        let h = height.expect("the strip is drawn");
        assert!((1.0..=32.0).contains(&h), "one row, got {h}");
        app.session.execute("select.none", &json!({})).unwrap();
        let text = crate::tests_labels::painted_text(&mut app, |app, ui| height = show(app, ui));
        assert!(text.trim().is_empty() && height.is_none(), "{text}");
    }

    #[test]
    fn a_slider_drag_is_one_undo_step() {
        use crate::widgets::Live;
        let mut app = app_with_sample();
        let width = crate::panels::parameters::rows(app.session.doc().unwrap()).into_iter().find(|r| r.row.name == "width").unwrap();
        let before = app.session.doc().unwrap().history.undo.len();
        for (v, phase) in [(150.0, Live::Dragging), (160.0, Live::Dragging), (170.0, Live::Released)] {
            crate::panels::parameters::set_value(&mut app, &width.row, v, phase);
        }
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before + 1);
        assert_eq!(app.run("parameters.list", json!({})).unwrap()["rows"][0]["expression"], "170 mm");
    }
}
