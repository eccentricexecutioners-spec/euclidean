//! The timeline strip: one chip per recorded step, the end marker after the last applied one,
//! and the buttons that move the marker. Double-clicking a chip opens its parameters form
//! (`step_form.rs`); dragging the marker between chips rolls the document back to there.

use egui::{Align, Color32, FontId, Rect, Sense, Stroke, Ui, vec2};
use serde_json::{Value, json};
use vectorcraft_engine::timeline::DOC_KEY;

use super::StepForm;
use crate::VectorcraftApp;
use crate::theme::Tokens;

const STRIP_HEIGHT: f32 = 30.0;
const CHIP_HEIGHT: f32 = 20.0;
const LABEL_CHARS: usize = 18;
const ICON_SIZE: f32 = 12.0;
const ICON_SLOT: f32 = ICON_SIZE + 3.0;

/// The icon a chip wears, by the command it records: a tool's icon for what a tool makes, the
/// Color panel's for paint, else the generic command icon.
fn chip_icon(cmd: &str) -> &'static str {
    let name = match cmd {
        "shape.rectangle" => crate::icons::tool_icon("tool-rect"),
        "shape.roundedRectangle" => crate::icons::tool_icon("tool-rounded-rect"),
        "shape.ellipse" => crate::icons::tool_icon("tool-ellipse"),
        "shape.polygon" => crate::icons::tool_icon("tool-polygon"),
        "shape.star" => crate::icons::tool_icon("tool-star"),
        "shape.line" | "shape.lineSegment" => crate::icons::tool_icon("tool-line"),
        "object.move" | "object.transform" => crate::icons::tool_icon("tool-selection"),
        "object.rotate" => crate::icons::tool_icon("tool-rotate"),
        "object.scale" => crate::icons::tool_icon("tool-scale"),
        c if c.starts_with("paint.") => "palette",
        c if c.starts_with("object.path.") => crate::icons::tool_icon("tool-pen"),
        c if c.starts_with("sketch.") => "wb-sketch",
        _ => "wb-command",
    };
    if crate::icons::exists(name) { name } else { "wb-command" }
}

/// What a chip needs to be drawn.
struct Chip {
    cmd: String,
    label: String,
    /// `ok`, `failed` or `pending`.
    status: String,
    error: Option<String>,
    rolled_back: bool,
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

/// A step's params as compact JSON.
fn params_text(app: &VectorcraftApp, index: usize) -> String {
    let p =
        app.session.doc().ok().and_then(|st| st.doc.unknown.get(DOC_KEY)?.get("steps")?.get(index)?.get("params").cloned()).unwrap_or(Value::Null);
    p.to_string()
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= LABEL_CHARS {
        return s.to_string();
    }
    let mut out: String = s.chars().take(LABEL_CHARS - 1).collect();
    out.push('…');
    out
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
    let mut colors = Vec::with_capacity(n);
    let mut roll: Option<usize> = None;
    let mut edit: Option<usize> = None;
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
                                marker_resp = Some(marker_bar(ui, t));
                            }
                            let text = truncate(&c.label);
                            let galley = ui.painter().layout_no_wrap(text, FontId::proportional(11.5), t.text);
                            let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 14.0 + ICON_SLOT, CHIP_HEIGHT), Sense::click());
                            let failed = c.status == "failed";
                            let grey = c.rolled_back || c.status == "pending";
                            let (fill, ink) = if failed {
                                (t.error, Color32::WHITE)
                            } else if grey {
                                (t.panel_darker, t.text_dim)
                            } else {
                                (if resp.hovered() { t.hover } else { t.button }, t.text)
                            };
                            ui.painter().rect_filled(rect, 4.0, fill);
                            ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0, t.button_border), egui::StrokeKind::Inside);
                            let icon_rect =
                                Rect::from_center_size(egui::pos2(rect.left() + 7.0 + ICON_SIZE / 2.0, rect.center().y), vec2(ICON_SIZE, ICON_SIZE));
                            crate::icons::paint(ui, chip_icon(&c.cmd), icon_rect, ink);
                            let text_pos = egui::pos2(rect.left() + 7.0 + ICON_SLOT, rect.center().y - galley.size().y / 2.0);
                            ui.painter().galley(text_pos, galley.clone(), ink);
                            colors.push(fill);
                            rects.push(rect);
                            app.ui.workbench.hits.push((format!("chip:{i}"), rect));
                            if grew && i + 1 == n {
                                resp.scroll_to_me(Some(Align::RIGHT));
                            }
                            if resp.double_clicked() {
                                edit = Some(i);
                            }
                            let params = if resp.hovered() { params_text(app, i) } else { String::new() };
                            resp.on_hover_ui(|ui| {
                                ui.label(egui::RichText::new(&c.label).strong());
                                ui.label(egui::RichText::new(params).monospace());
                                if let Some(e) = &c.error {
                                    ui.colored_label(t.error, e);
                                }
                                ui.weak(tl!("Double-click to edit"));
                            });
                        }
                        if marker == n {
                            marker_resp = Some(marker_bar(ui, t));
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
                            ui.painter().vline(x, m.rect.y_range(), Stroke::new(2.0, t.selection));
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
    if let Some(i) = edit {
        let label = chips.get(i).map(|c| c.label.clone()).unwrap_or_default();
        open_form(app, i, &label);
    }
    if let Some(k) = roll {
        roll_to(app, k);
    }
}

/// The end marker: a vertical bar, draggable.
fn marker_bar(ui: &mut Ui, t: &Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(8.0, CHIP_HEIGHT + 4.0), Sense::drag());
    let colour = if resp.hovered() || resp.dragged() { t.text_strong } else { t.selection };
    ui.painter().rect_filled(Rect::from_center_size(rect.center(), vec2(3.0, rect.height())), 1.5, colour);
    resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
}
