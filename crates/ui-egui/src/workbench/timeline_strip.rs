//! The timeline strip: one chip per recorded step, the end marker after the last applied one,
//! and the buttons that move the marker. Double-clicking a chip opens its parameters form
//! (`step_form.rs`); dragging the marker between chips rolls the document back to there.

use egui::{Align, Color32, Rect, Sense, Stroke, Ui, vec2};
use serde_json::{Value, json};
use vectorcraft_engine::timeline::DOC_KEY;

use super::StepForm;
use super::step_map::{step_color, step_icon};
use crate::VectorcraftApp;
use crate::theme::Tokens;

const STRIP_HEIGHT: f32 = 38.0;
const CHIP_HEIGHT: f32 = 20.0;
/// A step's square.
const STEP_SIZE: f32 = 28.0;
const ICON_SIZE: f32 = 16.0;
/// How much of a suppressed step's colour is left.
const SUPPRESSED_FADE: f32 = 0.35;

/// A change the hover card asks for, applied once the strip is drawn.
enum Action {
    Suppress(usize, bool),
    Delete(usize),
}

/// What a chip needs to be drawn.
struct Chip {
    cmd: String,
    label: String,
    /// `ok`, `failed` or `pending`.
    status: String,
    error: Option<String>,
    rolled_back: bool,
    suppressed: bool,
}

/// The document's steps and the end marker (how many steps are applied), read from the stored
/// timeline: no command runs per frame.
fn read_chips(app: &VectorcraftApp) -> (Vec<Chip>, usize) {
    let Ok(st) = app.session.doc() else { return (vec![], 0) };
    let Some(tl) = st.doc.unknown.get(DOC_KEY) else { return (vec![], 0) };
    let steps = tl.get("steps").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let flag = |s: &Value| ["rolled_back", "rolledBack"].iter().any(|k| s.get(*k).and_then(Value::as_bool) == Some(true));
    let chips: Vec<Chip> = steps
        .iter()
        .map(|s| Chip {
            cmd: s.get("cmd").and_then(Value::as_str).unwrap_or_default().to_string(),
            label: s.get("label").and_then(Value::as_str).unwrap_or_default().to_string(),
            status: s.get("status").and_then(Value::as_str).unwrap_or("ok").to_string(),
            error: s.get("error").and_then(Value::as_str).map(str::to_string),
            rolled_back: flag(s),
            suppressed: s.get("suppressed").and_then(Value::as_bool) == Some(true),
        })
        .collect();
    let marker = tl
        .get("marker")
        .and_then(Value::as_u64)
        .map(|m| m as usize)
        .unwrap_or_else(|| chips.iter().take_while(|c| !c.rolled_back).count())
        .min(chips.len());
    (chips, marker)
}

fn roll_to(app: &mut VectorcraftApp, index: usize) {
    if let Err(e) = app.run("timeline.rollTo", json!({"index": index})) {
        app.status(e);
    }
}

fn open_form(app: &mut VectorcraftApp, index: usize, label: &str) {
    let params = app
        .session
        .doc()
        .ok()
        .and_then(|st| st.doc.unknown.get(DOC_KEY)?.get("steps")?.get(index)?.get("params")?.as_object().cloned())
        .unwrap_or_default();
    app.ui.workbench.step_form = Some(StepForm::new(index, label, params));
}

pub(super) fn show(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens) {
    let (chips, marker) = read_chips(app);
    let n = chips.len();
    let grew = n > app.ui.workbench.strip_steps;
    app.ui.workbench.strip_steps = n;
    let (hues, active_hue) = {
        let wb = &mut app.ui.workbench;
        let tab = wb.tab.clone();
        let lay = wb.layout();
        let hues: Vec<Color32> = chips.iter().map(|c| step_color(t, lay, &c.cmd)).collect();
        let active = super::draw::active_tab(lay, &tab).and_then(|tab| tab.color.clone());
        (hues, t.tab_color(active.as_deref()))
    };
    let mut colors = Vec::with_capacity(n);
    let mut suppressed = Vec::with_capacity(n);
    let mut card_hits: Vec<(String, Rect)> = vec![];
    let mut roll: Option<usize> = None;
    let mut edit: Option<usize> = None;
    let mut action: Option<Action> = None;
    egui::Panel::bottom("wb_timeline")
        .exact_size(STRIP_HEIGHT)
        .frame(egui::Frame::NONE.fill(t.panel_darker).inner_margin(egui::Margin::symmetric(8, 2)).stroke(Stroke::new(1.0, t.border)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let buttons: [(&str, &str, &str, bool, usize); 4] = [
                    ("tl:first", "wb-tl-first", tl!("First step"), marker > 0, 0),
                    ("tl:prev", "wb-tl-prev", tl!("Previous step"), marker > 0, marker.saturating_sub(1)),
                    ("tl:next", "wb-tl-next", tl!("Next step"), marker < n, marker + 1),
                    ("tl:last", "wb-tl-last", tl!("Last step"), marker < n, n),
                ];
                for (name, icon, tip, enabled, target) in buttons {
                    let resp = ui.add_enabled(enabled, egui::Button::new("").min_size(vec2(22.0, CHIP_HEIGHT))).on_hover_text(tip);
                    let tint = if enabled { t.icon } else { t.text_dim };
                    crate::icons::paint(ui, icon, egui::Rect::from_center_size(resp.rect.center(), vec2(14.0, 14.0)), tint);
                    app.ui.workbench.hits.push((name.into(), resp.rect));
                    if resp.clicked() {
                        roll = Some(target);
                    }
                }
                ui.separator();
                let mut rects: Vec<Rect> = Vec::with_capacity(n);
                let mut marker_resp = None;
                egui::ScrollArea::horizontal().id_salt("wb-timeline-scroll").auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (i, c) in chips.iter().enumerate() {
                            if i == marker {
                                marker_resp = Some(marker_bar(ui, t, active_hue));
                            }
                            let (rect, resp) = ui.allocate_exact_size(vec2(STEP_SIZE, STEP_SIZE), Sense::click());
                            let hue = hues.get(i).copied().unwrap_or(t.tab_draw);
                            let failed = c.status == "failed";
                            let grey = c.rolled_back || c.status == "pending";
                            let (mut fill, mut ink, mut border) = if failed {
                                (t.error, t.on_accent, t.error)
                            } else if grey {
                                (t.panel_darker, t.text_dim, t.border)
                            } else {
                                (t.wash(hue, if resp.hovered() { 0.20 } else { 0.10 }), hue, t.wash(hue, 0.30))
                            };
                            if c.suppressed {
                                let fade = |x: Color32| x.gamma_multiply(SUPPRESSED_FADE);
                                (fill, ink, border) = (fade(fill), fade(ink), fade(border));
                            }
                            ui.painter().rect_filled(rect, 8.0, fill);
                            ui.painter().rect_stroke(rect, 8.0, Stroke::new(1.0, border), egui::StrokeKind::Inside);
                            let icon_rect = Rect::from_center_size(rect.center(), vec2(ICON_SIZE, ICON_SIZE));
                            crate::icons::paint(ui, step_icon(&c.cmd), icon_rect, ink);
                            if c.suppressed {
                                let r = rect.shrink(6.0);
                                ui.painter().line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.5, t.text_dim));
                            }
                            colors.push(fill);
                            suppressed.push(c.suppressed);
                            rects.push(rect);
                            app.ui.workbench.hits.push((format!("chip:{i}"), rect));
                            if grew && i + 1 == n {
                                resp.scroll_to_me(Some(Align::RIGHT));
                            }
                            if resp.double_clicked() {
                                edit = Some(i);
                            }
                            resp.on_hover_ui(|ui| hover_card(ui, t, i, c, &mut card_hits, &mut edit, &mut roll, &mut action));
                        }
                        if marker == n {
                            marker_resp = Some(marker_bar(ui, t, active_hue));
                        }
                    });
                });
                if let Some(m) = marker_resp {
                    app.ui.workbench.hits.push(("marker".into(), m.rect));
                    if m.dragged()
                        && let Some(p) = m.interact_pointer_pos()
                    {
                        let k = rects.iter().filter(|r| r.center().x < p.x).count();
                        if let Some(r) = rects.get(k).or(rects.last()) {
                            let x = if k < rects.len() { r.left() - 2.0 } else { r.right() + 2.0 };
                            ui.painter().vline(x, m.rect.y_range(), Stroke::new(2.0, active_hue));
                        }
                    }
                    if m.drag_stopped()
                        && let Some(p) = m.interact_pointer_pos()
                    {
                        let k = rects.iter().filter(|r| r.center().x < p.x).count();
                        if k != marker {
                            roll = Some(k);
                        }
                    }
                }
            });
        });
    app.ui.workbench.chip_colors = colors;
    app.ui.workbench.chip_hues = hues;
    app.ui.workbench.chip_suppressed = suppressed;
    app.ui.workbench.hits.extend(card_hits);
    if let Some(i) = edit {
        let label = chips.get(i).map(|c| c.label.clone()).unwrap_or_default();
        open_form(app, i, &label);
    }
    match action {
        Some(Action::Suppress(index, on)) => run(app, "timeline.suppressStep", json!({"index": index, "on": on})),
        Some(Action::Delete(index)) => run(app, "timeline.deleteStep", json!({"index": index})),
        None => {}
    }
    if let Some(k) = roll {
        roll_to(app, k);
    }
}

/// The card shown over a hovered step: its number and label, its status, and four buttons.
#[allow(clippy::too_many_arguments)]
fn hover_card(
    ui: &mut Ui,
    t: &Tokens,
    i: usize,
    c: &Chip,
    hits: &mut Vec<(String, Rect)>,
    edit: &mut Option<usize>,
    roll: &mut Option<usize>,
    action: &mut Option<Action>,
) {
    ui.label(egui::RichText::new(format!("{} {} · {}", tl!("Step"), i + 1, c.label)).strong());
    if c.rolled_back {
        ui.label(tl!("Rolled back"));
    } else if c.status == "pending" {
        ui.label(tl!("Pending"));
    } else if c.status == "failed" {
        ui.colored_label(t.error, format!("{}: {}", tl!("Failed"), c.error.as_deref().unwrap_or_default()));
    } else {
        ui.label(tl!("OK"));
    }
    if c.suppressed {
        ui.label(tl!("Suppressed"));
    }
    ui.horizontal(|ui| {
        let r = ui.small_button(tl!("Edit…"));
        hits.push(("card:edit".into(), r.rect));
        if r.clicked() {
            *edit = Some(i);
        }
        let r = ui.small_button(if c.suppressed { tl!("Unsuppress") } else { tl!("Suppress") });
        hits.push(("card:suppress".into(), r.rect));
        if r.clicked() {
            *action = Some(Action::Suppress(i, !c.suppressed));
        }
        let r = ui.small_button(tl!("Roll to here"));
        hits.push(("card:roll".into(), r.rect));
        if r.clicked() {
            *roll = Some(i + 1);
        }
        let r = ui.small_button(tl!("Delete"));
        hits.push(("card:delete".into(), r.rect));
        if r.clicked() {
            *action = Some(Action::Delete(i));
        }
    });
}

fn run(app: &mut VectorcraftApp, cmd: &str, params: Value) {
    if let Err(e) = app.run(cmd, params) {
        app.status(e);
    }
}

/// The end marker: a vertical bar, draggable.
fn marker_bar(ui: &mut Ui, t: &Tokens, hue: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(8.0, CHIP_HEIGHT + 4.0), Sense::drag());
    let colour = if resp.hovered() || resp.dragged() { t.text_strong } else { hue };
    ui.painter().rect_filled(Rect::from_center_size(rect.center(), vec2(3.0, rect.height())), 1.5, colour);
    resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
}
