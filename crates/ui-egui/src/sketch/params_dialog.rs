//! Change Parameters (Euclidean): the Fusion 360 dialog. A resizable window over the canvas with
//! every parameter in a table: user parameters first, then the model parameters (the dimensions)
//! grouped under the sketch that owns them, each saying which dimension it drives. Every edit runs
//! a `parameters.*` command, so it is one undo step and the control channel sees it.

use egui::{Context, Pos2, Ui, Vec2};
use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_doc::sketch::Sketch;
use vectorcraft_doc::sketch::model::{Constraint, Entity};

use crate::VectorcraftApp;
use crate::panels::parameters::{self, Row};
use crate::widgets;

/// The dialog's first size, points: small enough to leave most of the canvas visible.
pub const DEFAULT_SIZE: Vec2 = egui::vec2(560.0, 340.0);
/// The smallest the dialog resizes to.
const MIN_SIZE: Vec2 = egui::vec2(360.0, 200.0);
/// Its distance from the canvas's top-right corner.
const INSET: f32 = 24.0;

fn window_id() -> egui::Id {
    egui::Id::new("change_parameters_window")
}

/// What a dimension of `sketch` named `param` measures: `Length of line 3`, `Radius of arc 7`…
fn drives(sketch: &Sketch, param: &str) -> Option<String> {
    let c = sketch.constraints.iter().find(|c| c.param() == Some(param))?;
    let radius_of = |e: u32, what: &str| match sketch.entities.iter().find(|x| x.id() == e) {
        Some(Entity::Arc { .. }) => format!("{what} {} {e}", tl!("arc")),
        _ => format!("{what} {} {e}", tl!("circle")),
    };
    Some(match c {
        Constraint::Length { line, .. } => format!("{} {line}", tl!("Length of line")),
        Constraint::Angle { line, .. } => format!("{} {line}", tl!("Angle of line")),
        Constraint::AngleBetween { a, b, .. } => format!("{} {a} {} {b}", tl!("Angle between lines"), tl!("and")),
        Constraint::Distance { a, b, .. } => format!("{} {a} {} {b}", tl!("Distance between points"), tl!("and")),
        Constraint::Radius { e, .. } => radius_of(*e, tl!("Radius of")),
        Constraint::Diameter { e, .. } => radius_of(*e, tl!("Diameter of")),
        _ => return None,
    })
}

/// A model parameter group: the sketch that owns it and the rows it owns.
struct Group {
    title: String,
    sketch: Option<Sketch>,
    rows: Vec<Row>,
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

fn body(app: &mut VectorcraftApp, ui: &mut Ui) {
    let Some(st) = app.session.active() else {
        widgets::dim_label(ui, tl!("No document"));
        return;
    };
    // Owned snapshots: the rows below run commands, which borrow the app mutably.
    let (mut user, mut groups): (Vec<Row>, Vec<Group>) = (vec![], vec![]);
    for r in parameters::rows(st) {
        match r.row.sketch {
            None => user.push(r),
            Some(id) => {
                let at = match groups.iter().position(|g| g.title.ends_with(&format!(" {id}"))) {
                    Some(at) => at,
                    None => {
                        let node = st.doc.node(NodeId(id));
                        let name = node.map_or_else(|| tl!("Sketch").to_string(), |n| n.display_name());
                        let sketch = node.and_then(|n| n.extras().sketch.as_deref().cloned());
                        groups.push(Group { title: format!("{name} {id}"), sketch, rows: vec![] });
                        groups.len() - 1
                    }
                };
                groups[at].rows.push(r);
            }
        }
    }
    let t = crate::theme::Tokens::get(ui.ctx());
    let units = parameters::unit_labels();
    let unit_names: Vec<&str> = units.iter().map(String::as_str).collect();

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("User Parameters")).size(12.5).color(t.text_strong));
        if widgets::icon_button(ui, "plus", tl!("New Parameter"), false, 22.0).clicked() {
            app.run("parameters.add", json!({})).ok();
        }
    });
    table(app, ui, "user", &user, &unit_names, None);

    ui.add_space(6.0);
    ui.label(egui::RichText::new(tl!("Model Parameters")).size(12.5).color(t.text_strong));
    if groups.is_empty() {
        widgets::dim_label(ui, tl!("None"));
    }
    for g in &groups {
        egui::CollapsingHeader::new(egui::RichText::new(&g.title).size(12.0).color(t.text)).id_salt(("group", &g.title)).default_open(true).show(
            ui,
            |ui| {
                table(app, ui, &g.title, &g.rows, &unit_names, Some(g));
            },
        );
    }
}

/// One table of rows; a model group adds the "Drives" column.
fn table(app: &mut VectorcraftApp, ui: &mut Ui, id: &str, rows: &[Row], unit_names: &[&str], group: Option<&Group>) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let head = |ui: &mut Ui, text: &str| {
        widgets::field_label(ui, egui::RichText::new(text).size(11.0).color(t.text_dim));
    };
    egui::Grid::new(("change-parameters", id)).num_columns(if group.is_some() { 7 } else { 6 }).spacing([6.0, 3.0]).show(ui, |ui| {
        ui.label("");
        head(ui, tl!("Parameter"));
        head(ui, tl!("Unit"));
        head(ui, tl!("Expression"));
        head(ui, tl!("Value"));
        head(ui, tl!("Comment"));
        if group.is_some() {
            head(ui, tl!("Drives"));
        }
        ui.end_row();
        for r in rows {
            ui.push_id(("row", &r.row.name), |ui| {
                parameters::edit_row(app, ui, r, unit_names);
                if let Some(g) = group {
                    let text = g.sketch.as_ref().and_then(|s| drives(s, &r.row.name)).unwrap_or_default();
                    ui.label(egui::RichText::new(text).size(12.0).color(t.text_dim));
                }
            });
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(at("User Parameters") < at("width") && at("width") < at("Model Parameters") && at("Model Parameters") < at("d1"), "{text}");
        assert!(text.contains("Length of line"), "which dimension it drives:\n{text}");
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
