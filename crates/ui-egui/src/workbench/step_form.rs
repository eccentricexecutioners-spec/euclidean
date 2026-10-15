//! The step form: the parameters of a timeline step as one editable row each, opened by
//! double-clicking its chip. OK runs `timeline.editStep` with the fields that changed.
//!
//! Why not the command's own dialog: `state::Dialog` has no target to redirect OK to, and the
//! dialogs module is not this workbench's to change, so the form is the workbench's own.

use egui::Ui;
use serde_json::{Map, Value, json};

use crate::VectorcraftApp;

#[derive(Clone, Debug)]
pub struct StepForm {
    pub index: usize,
    pub label: String,
    /// The fields as edited.
    pub params: Map<String, Value>,
    original: Map<String, Value>,
}

impl StepForm {
    pub fn new(index: usize, label: &str, params: Map<String, Value>) -> Self {
        Self { index, label: label.to_string(), original: params.clone(), params }
    }

    /// The fields whose value differs from the step's.
    fn changes(&self) -> Map<String, Value> {
        self.params.iter().filter(|(k, v)| self.original.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}

/// One field's editor.
fn field(ui: &mut Ui, key: &str, value: &mut Value) {
    match value {
        Value::Number(n) => {
            let was_int = n.is_i64() || n.is_u64();
            let mut x = n.as_f64().unwrap_or(0.0);
            if ui.add(egui::DragValue::new(&mut x).speed(0.5)).changed() {
                *value = if was_int && x.fract() == 0.0 { json!(x as i64) } else { json!(x) };
            }
        }
        Value::String(s) => {
            ui.text_edit_singleline(s);
        }
        Value::Bool(b) => {
            ui.checkbox(b, "");
        }
        other => {
            ui.label(egui::RichText::new(other.to_string()).monospace().weak());
        }
    }
    let _ = key;
}

/// Draw the form window, if one is open.
pub(super) fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.workbench.step_form.take() else { return };
    let (mut ok, mut cancel) = (false, false);
    let mut open = true;
    egui::Window::new(&form.label).id(egui::Id::new("wb-step-form")).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        egui::Grid::new("wb-step-form-grid").num_columns(2).show(ui, |ui| {
            for (k, v) in form.params.iter_mut() {
                ui.label(k.as_str());
                field(ui, k, v);
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let o = ui.button(tl!("OK"));
            app.ui.workbench.hits.push(("form:ok".into(), o.rect));
            ok = o.clicked();
            let c = ui.button(tl!("Cancel"));
            app.ui.workbench.hits.push(("form:cancel".into(), c.rect));
            cancel = c.clicked();
        });
    });
    if ok {
        match app.run("timeline.editStep", json!({"index": form.index, "params": form.changes()})) {
            Ok(r) => {
                if let Some(n) = r.get("failed").and_then(Value::as_u64) {
                    let err = r.get("error").and_then(Value::as_str).unwrap_or_default();
                    app.status(tl!("Step {n} failed: {error}").replace("{n}", &(n + 1).to_string()).replace("{error}", err));
                }
            }
            Err(e) => app.status(e),
        }
    } else if open && !cancel {
        app.ui.workbench.step_form = Some(form);
    }
}
