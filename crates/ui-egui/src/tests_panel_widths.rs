//! E1 Part B: every side-panel palette lays out to the width it is given. At 280, 420 and 640
//! points no widget's right edge passes the panel, and a palette's main fields grow with it.

use serde_json::json;
use vectorcraft_engine::Session;

use crate::VectorcraftApp;

/// A document with a selected rectangle, a text object and a drawn sketch, so every panel has
/// something to show.
fn app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Default::default());
    app.run("file.new", json!({"width": 400, "height": 300})).unwrap();
    app.run("text.create", json!({"x": 20, "y": 200, "text": "Euclidean"})).unwrap();
    app.run("sketch.sample", json!({})).unwrap();
    // A drawn sketch with dimensions: the Parameters table's model group.
    let id = app.run("sketch.new", json!({})).unwrap()["id"].as_u64().unwrap();
    app.run("sketch.addLine", json!({"id": id, "from": [10.0, 40.0], "to": [60.0, 40.0], "length": "50", "angle": "30"})).unwrap();
    app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 100, "height": 50})).unwrap();
    app
}

/// The panel drawn alone in a side panel `width` wide: every widget rect of the second frame.
pub(crate) fn panel_rects(app: &mut VectorcraftApp, id: &str, width: f32) -> (egui::Rect, Vec<egui::Rect>) {
    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    crate::theme::install_fonts(&ctx);
    crate::theme::apply(&ctx, crate::theme::Brightness::Light);
    let mut panel = egui::Rect::NOTHING;
    for _ in 0..3 {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width + 400.0, 900.0));
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            // As the workbench hosts a panel: a both-ways scroll area held to the column's width, so
            // a wide widget scrolls (and shows up here) instead of widening the column.
            let r = egui::Panel::left("test-panel").resizable(false).exact_size(width).show(ui, |ui| {
                let w = ui.available_width();
                egui::ScrollArea::both().id_salt("test-scroll").auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(w);
                    ui.push_id(id, |ui| crate::dock::panel_body(app, ui, id, w, 400.0));
                });
            });
            panel = r.response.rect;
        });
        out.textures_delta.clear();
    }
    let rects: Vec<egui::Rect> = ctx.viewport(|vp| {
        vp.prev_pass.widgets.layers().flat_map(|(_, w)| w.iter()).map(|w| w.rect).filter(|r| r.is_finite() && r.width() > 0.0).collect()
    });
    (panel, rects)
}

/// The widest field-like widget (a text field, dropdown or slider: 24–26 pt tall, at least 36
/// wide) that lies inside the panel. Button rows are 28 pt and are left out.
fn widest_field(panel: egui::Rect, rects: &[egui::Rect]) -> f32 {
    rects
        .iter()
        .filter(|r| r.height() >= 24.0 && r.height() <= 26.5 && r.width() >= 36.0 && r.width() < panel.width() - 4.0 && panel.contains_rect(**r))
        .map(|r| r.width())
        .fold(0.0, f32::max)
}

/// Every panel the Fusion layout can show (the dock tabs and the icon panels, hidden ones aside).
fn panels() -> Vec<&'static str> {
    crate::state::all_panels().map(|(id, _)| id).collect()
}

#[test]
fn no_widget_passes_the_panels_right_edge_at_three_widths() {
    let mut app = app();
    let mut bad: Vec<String> = vec![];
    for id in panels() {
        for width in [280.0, 420.0, 640.0] {
            let (panel, rects) = panel_rects(&mut app, id, width);
            // Popups and tooltips sit outside the panel on purpose; a widget that starts inside it must end inside it.
            // The panel's own rect and its scroll content's frame (they start at the panel's edge
            // and carry the frame's margin) are not widgets in it: the content starts 18 pt in.
            for r in rects.iter().filter(|r| r.left() > panel.left() + 12.0 && r.left() < panel.right() && r.top() >= panel.top() - 0.5) {
                if r.right() > panel.right() + 1.0 {
                    bad.push(format!("{id} at {width}: widget {r:?} passes the panel's right edge {}", panel.right()));
                }
            }
        }
    }
    bad.sort();
    bad.dedup();
    assert!(bad.is_empty(), "{} clipped widgets:\n{}", bad.len(), bad.join("\n"));
}

/// The palettes whose rows hold fields, sliders or dropdowns: their main field grows with the panel.
/// (Gradient's main control is its 50 pt ramp, which fills the width; its stop fields are small
/// number fields on purpose. Info shows read-only labels.)
const WITH_FIELDS: &[&str] = &[
    "color",
    "stroke",
    "properties",
    "character",
    "paragraph",
    "parameters",
    "nest",
    "layers",
    "artboards",
    "transparency",
    "navigator",
    "imageTrace",
    "links",
    "glyphs",
    "openType",
    "tabs",
];

#[test]
fn the_main_fields_grow_with_the_panel() {
    let mut app = app();
    // Character and Paragraph need the text selected.
    let mut text_app = VectorcraftApp::new(Session::new(), Default::default());
    text_app.run("file.new", json!({"width": 400, "height": 300})).unwrap();
    text_app.run("text.create", json!({"x": 20, "y": 200, "text": "Euclidean"})).unwrap();
    let mut bad: Vec<String> = vec![];
    for id in WITH_FIELDS {
        let app = if matches!(*id, "character" | "paragraph" | "glyphs" | "openType" | "tabs") { &mut text_app } else { &mut app };
        let widths: Vec<f32> = [280.0, 420.0, 640.0]
            .into_iter()
            .map(|w| {
                let (panel, rects) = panel_rects(app, id, w);
                widest_field(panel, &rects)
            })
            .collect();
        if !(widths[0] > 0.0 && widths[1] > widths[0] + 40.0 && widths[2] > widths[1] + 40.0) {
            let (panel, rects) = panel_rects(app, id, 420.0);
            let mut fields: Vec<String> = rects
                .iter()
                .filter(|r| {
                    r.height() >= 24.0 && r.height() <= 26.5 && r.width() >= 36.0 && r.width() < panel.width() - 4.0 && panel.contains_rect(**r)
                })
                .map(|r| format!("{:.0}×{:.0}@{:.0},{:.0}", r.width(), r.height(), r.left(), r.top()))
                .collect();
            fields.sort();
            fields.dedup();
            bad.push(format!(
                "{id}: widest field {:.0} / {:.0} / {:.0} at 280 / 420 / 640; field-like rects at 420: {fields:?}",
                widths[0], widths[1], widths[2]
            ));
        }
    }
    assert!(bad.is_empty(), "fields that do not grow with the panel:\n{}", bad.join("\n"));
}
