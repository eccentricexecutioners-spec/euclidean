//! Change Parameters (Euclidean): the Fusion 360 dialog. A resizable window over the canvas with
//! every parameter in a table: user parameters first, then the model parameters (the dimensions)
//! grouped under the sketch that owns them, each saying which dimension it drives. Every edit runs
//! a `parameters.*` command, so it is one undo step and the control channel sees it.

use egui::{Context, Pos2, Ui, Vec2};

use crate::VectorcraftApp;
use crate::panels::parameters;

/// The dialog's first size, points: small enough to leave most of the canvas visible.
pub const DEFAULT_SIZE: Vec2 = egui::vec2(560.0, 340.0);
/// The smallest the dialog resizes to.
const MIN_SIZE: Vec2 = egui::vec2(360.0, 200.0);
/// Its distance from the canvas's top-right corner.
const INSET: f32 = 24.0;

fn window_id() -> egui::Id {
    egui::Id::new("change_parameters_window")
}

/// Show the dialog while `UiState::change_parameters` is set.
pub fn show(app: &mut VectorcraftApp, ctx: &Context) {
    if !app.ui.change_parameters {
        return;
    }
    let pos = match app.canvas_rect {
        Some(r) => Pos2::new((r.right() - INSET - DEFAULT_SIZE.x).max(r.left()), r.top() + INSET),
        None => Pos2::new((ctx.content_rect().right() - INSET - DEFAULT_SIZE.x).max(0.0), INSET),
    };
    let mut open = true;
    egui::Window::new(tl!("Change Parameters"))
        .id(window_id())
        .open(&mut open)
        .default_pos(pos)
        .default_size(DEFAULT_SIZE)
        .min_size(MIN_SIZE)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| body(app, ui));
        });
    app.ui.change_parameters = open;
}

/// The dialog's body: the shared Fusion-style listing (Favorites, User Parameters, Model
/// Parameters per sketch), each table's columns sharing the dialog's width.
fn body(app: &mut VectorcraftApp, ui: &mut Ui) {
    parameters::listing(app, ui, "dialog");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn app_with_sketch() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width": 600, "height": 400})).unwrap();
        app.session.execute("sketch.sample", &json!({})).unwrap();
        let id = app.session.execute("sketch.new", &json!({})).unwrap()["id"].as_u64().unwrap();
        app.session.execute("sketch.addLine", &json!({"id": id, "from": [10.0, 40.0], "to": [60.0, 40.0], "length": "50", "angle": "30"})).unwrap();
        app
    }

    #[test]
    fn lists_user_then_model_parameters_per_sketch() {
        let mut app = app_with_sketch();
        let text = crate::tests_labels::painted_text(&mut app, body);
        let at = |s: &str| text.find(s).unwrap_or_else(|| panic!("{s} missing:\n{text}"));
        // Favorites (the sample stars some rows) come first; then the user rows, then the model rows.
        let user = at("User Parameters");
        let after = |from: usize, s: &str| from + text[from..].find(s).unwrap_or_else(|| panic!("{s} missing after {from}:\n{text}"));
        assert!(
            at("Favorites") < user
                && user < after(user, "width")
                && after(user, "width") < at("Model Parameters")
                && at("Model Parameters") < after(at("Model Parameters"), "d1"),
            "{text}"
        );
        assert!(text.contains("Length of line 1"), "which dimension it drives, numbered from 1:\n{text}");
        assert!(text.contains("Drives") && text.contains("Expression"), "the model table has its own header row:\n{text}");
        assert!(text.contains("Parametric Sketch"), "the sketch group's name:\n{text}");
    }

    #[test]
    fn opens_at_its_default_size() {
        let mut app = app_with_sketch();
        app.ui.change_parameters = true;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 850.0));
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| show(&mut app, ui.ctx()));
            out.textures_delta.clear();
        }
        let rect = ctx.memory(|m| m.area_rect(window_id())).expect("the window was shown");
        assert!((rect.width() - DEFAULT_SIZE.x).abs() < 40.0 && (rect.height() - DEFAULT_SIZE.y).abs() < 40.0, "{rect:?}");
    }

    #[test]
    fn window_parameters_opens_change_parameters() {
        let mut app = app_with_sketch();
        let entries = crate::menus::menu_entries(&app);
        let e = entries.iter().find(|e| e.path == ["Window"] && e.label == "Parameters").expect("Window > Parameters");
        assert_eq!(e.command.as_deref(), Some("window.changeParameters"));
        assert!(!app.ui.change_parameters);
        app.run("window.changeParameters", json!({})).unwrap();
        assert!(app.ui.change_parameters);
    }
}
