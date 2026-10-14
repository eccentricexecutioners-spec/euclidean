//! The sketch toolbar: it replaces the Control bar while sketch mode is on. Groups of icon
//! buttons with the group's name and a dropdown listing everything in it, and the green
//! FINISH SKETCH button at the right.

use egui::{Align, Color32, Layout, Sense, Stroke, Ui, vec2};

use super::SketchTool;
use crate::VectorcraftApp;
use crate::theme::{self, Tokens};

/// What a button does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Tool(SketchTool),
    Constrain(&'static str),
    /// Not built yet: says so in the status line.
    Soon,
    Params,
    Construction,
}

/// A button or a dropdown row.
struct Entry {
    icon: &'static str,
    label: &'static str,
    key: &'static str,
    act: Act,
}

const fn e(icon: &'static str, label: &'static str, key: &'static str, act: Act) -> Entry {
    Entry { icon, label, key, act }
}

fn create() -> [Entry; 7] {
    [
        e("sketch-line", tl!("Line"), "L", Act::Tool(SketchTool::Line)),
        e("sketch-rect", tl!("2-Point Rectangle"), "R", Act::Tool(SketchTool::Rect2)),
        e("sketch-rect-center", tl!("Center Rectangle"), "", Act::Tool(SketchTool::RectCenter)),
        e("sketch-circle", tl!("Center Diameter Circle"), "C", Act::Tool(SketchTool::Circle)),
        e("sketch-arc", tl!("3-Point Arc"), "", Act::Tool(SketchTool::Arc3)),
        e("sketch-fillet", tl!("Fillet"), "", Act::Tool(SketchTool::Fillet)),
        e("sketch-line", tl!("Construction"), "X", Act::Construction),
    ]
}

fn modify() -> [Entry; 4] {
    [
        e("sketch-fillet", tl!("Fillet"), "", Act::Tool(SketchTool::Fillet)),
        e("sketch-trim", tl!("Trim"), "T", Act::Soon),
        e("sketch-offset", tl!("Offset"), "O", Act::Soon),
        e("sketch-params", tl!("Change Parameters"), "fx", Act::Params),
    ]
}

fn constraints() -> [Entry; 9] {
    [
        e("sketch-coincident", tl!("Coincident"), "", Act::Constrain("coincident")),
        e("sketch-horizontal", tl!("Horizontal/Vertical"), "", Act::Constrain("horizontal")),
        e("sketch-perpendicular", tl!("Perpendicular"), "", Act::Constrain("perpendicular")),
        e("sketch-parallel", tl!("Parallel"), "", Act::Constrain("parallel")),
        e("sketch-tangent", tl!("Tangent"), "", Act::Constrain("tangent")),
        e("sketch-equal", tl!("Equal"), "", Act::Constrain("equal")),
        e("sketch-fix", tl!("Fix/Unfix"), "", Act::Constrain("fix")),
        e("sketch-midpoint", tl!("Midpoint"), "", Act::Constrain("midpoint")),
        e("sketch-concentric", tl!("Concentric"), "", Act::Constrain("concentric")),
    ]
}

fn run(app: &mut VectorcraftApp, act: Act) {
    match act {
        Act::Tool(tool) => super::set_tool(app, tool),
        Act::Constrain(kind) => super::constrain(app, kind),
        Act::Soon => app.status(tl!("Coming next")),
        Act::Construction => super::toggle_construction(app),
        Act::Params => {
            app.ui.change_parameters = true;
        }
    }
}

fn active_act(app: &VectorcraftApp, act: Act) -> bool {
    matches!(act, Act::Tool(t) if app.sketch.as_ref().is_some_and(|m| m.tool == t))
}

/// An icon button of the row; returns whether it was pressed.
fn button(ui: &mut Ui, t: &Tokens, entry: &Entry, on: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    if on {
        ui.painter().rect_filled(rect, 4.0, t.tool_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover);
    }
    crate::icons::paint(ui, entry.icon, rect.shrink(3.0), t.icon);
    let tip = if entry.key.is_empty() { entry.label.to_string() } else { format!("{} ({})", entry.label, entry.key) };
    resp.on_hover_text(tip).clicked()
}

/// A group: its buttons in a row, its name and a dropdown under them.
fn group(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, name: &str, buttons: &[Entry], list: &[Entry]) {
    let mut chosen = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
        ui.horizontal(|ui| {
            for b in buttons {
                if button(ui, t, b, active_act(app, b.act)) {
                    chosen = Some(b.act);
                }
            }
        });
        let resp = ui
            .horizontal(|ui| {
                let label = ui.add(egui::Label::new(egui::RichText::new(name).font(theme::semibold(9.5)).color(t.text_dim)).sense(Sense::click()));
                let (r, chevron) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::click());
                crate::icons::paint(ui, "chevron-down", r, t.text_dim);
                label | chevron
            })
            .inner;
        egui::Popup::menu(&resp).show(|ui| {
            for item in list {
                let text = if item.key.is_empty() { item.label.to_string() } else { format!("{}    {}", item.label, item.key) };
                if ui.button(text).clicked() {
                    chosen = Some(item.act);
                    ui.close();
                }
            }
        });
    });
    if let Some(act) = chosen {
        run(app, act);
    }
}

/// Draw the sketch toolbar (a panel at the top of the window).
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("sketch_toolbar")
        .exact_size(46.0)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(10, 2)).stroke(Stroke::new(1.0, t.border)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let c = create();
                group(app, ui, &t, tl!("CREATE"), &c[..], &c[..]);
                ui.separator();
                let m = modify();
                group(app, ui, &t, tl!("MODIFY"), &m[1..], &m[..]);
                ui.separator();
                let k = constraints();
                group(app, ui, &t, tl!("CONSTRAINTS"), &k[..], &k[..]);
                ui.separator();
                let d = [e("sketch-dimension", tl!("Sketch Dimension"), "D", Act::Tool(SketchTool::Dimension))];
                group(app, ui, &t, tl!("SKETCH DIMENSION"), &d[..], &[]);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    finish_button(app, ui);
                    dof_label(app, ui, &t);
                });
            });
        });
}

/// `Fully constrained`, or how many degrees of freedom are left.
fn dof_label(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens) {
    let Some(dof) = super::snapshot(app).and_then(|c| c.dof) else { return };
    let text = match dof {
        0 => tl!("Fully constrained").to_string(),
        1 => tl!("1 degree of freedom").to_string(),
        n => format!("{n} {}", tl!("degrees of freedom")),
    };
    ui.label(egui::RichText::new(text).font(theme::semibold(11.5)).color(if dof == 0 { t.text_strong } else { t.text_dim }));
}

/// The green FINISH SKETCH button.
fn finish_button(app: &mut VectorcraftApp, ui: &mut Ui) {
    let label = tl!("FINISH SKETCH");
    let font = theme::semibold(12.0);
    let width = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE).size().x + 52.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let green = if resp.hovered() { Color32::from_rgb(0x2e, 0xa4, 0x4f) } else { Color32::from_rgb(0x23, 0x86, 0x3b) };
    ui.painter().rect_filled(rect, 5.0, green);
    crate::icons::paint(ui, "sketch-finish", egui::Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(20.0, 20.0)), Color32::WHITE);
    ui.painter().text(rect.left_center() + vec2(34.0, 0.0), egui::Align2::LEFT_CENTER, label, font, Color32::WHITE);
    if resp.on_hover_text(tl!("Finish Sketch")).clicked() {
        super::finish(app);
    }
}
