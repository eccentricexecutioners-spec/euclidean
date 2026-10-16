//! Parameters panel (Euclidean): the drawing's user parameters and the parametric sketch they
//! drive. Favourites across the top with a slider and − / + each; then every parameter as a
//! row (star, name, unit, expression, value, comment); then the selected sketch's shapes with
//! their expression fields. Everything runs a `parameters.*` / `sketch.*` command, so the panel,
//! the control bar's sliders and an agent drive the drawing the same way; a slider drag previews
//! through the session's interaction, so one drag is one undo step.

use egui::Ui;
use serde_json::json;
use vectorcraft_doc::params::{self, ParamRow};
use vectorcraft_doc::sketch;
use vectorcraft_doc::{Node, NodeId};
use vectorcraft_engine::DocState;

use crate::VectorcraftApp;
use crate::widgets::{self, Live};

/// The panel id, as the dock, the Window menu and `window.panel` name it.
pub const ID: &str = "parameters";

/// One parameter as the panel shows it.
#[derive(Clone)]
pub(crate) struct Row {
    pub row: ParamRow,
    /// The solved value in the row's unit.
    pub value: Option<f64>,
    pub error: Option<String>,
    /// A plain number with a range: what a slider can drive.
    pub slides: bool,
}

/// Every parameter, solved.
pub(crate) fn rows(st: &DocState) -> Vec<Row> {
    let rows = params::rows(&st.doc);
    let solved = params::solve(&rows);
    rows.into_iter()
        .map(|row| {
            let value = solved.value(&row.name).map(|v| params::to_unit(v, &row.unit));
            let error = solved.error(&row.name).map(str::to_string);
            let slides = row.is_plain() && row.min.is_some() && row.max.is_some() && error.is_none();
            Row { row, value, error, slides }
        })
        .collect()
}

/// The one selected parametric sketch, if the selection is exactly that.
pub(crate) fn selected_sketch(st: &DocState) -> Option<NodeId> {
    match st.selection.objects.as_slice() {
        [one] if st.doc.node(*one).is_some_and(|n| n.extras().sketch.is_some()) => Some(*one),
        _ => None,
    }
}

/// The value a row shows: `120 mm`, `4`, or `error`.
fn shown_value(r: &Row) -> String {
    if r.error.is_some() { tl!("error").to_string() } else { params::format_value(r.value.map(|v| params::from_unit(v, &r.row.unit)), &r.row.unit) }
}

/// Set `name` to `value` (in its unit) as a plain expression: live while a slider drags.
pub(crate) fn set_value(app: &mut VectorcraftApp, row: &ParamRow, value: f64, phase: Live) {
    let v = match (row.min, row.max) {
        (Some(a), Some(b)) if a <= b => value.clamp(a, b),
        _ => value,
    };
    let params = json!({"name": row.name, "expression": params::plain_expression(v, &row.unit)});
    crate::panels::live_run(app, "Set Parameter", "parameters.set", params, phase);
}

/// A shape field's label, translated (literals, so the catalogue gate sees them).
fn field_label(kind: &str, field: &str) -> &'static str {
    match sketch::field_label(kind, field) {
        "x" => tl!("x"),
        "y" => tl!("y"),
        "width" => tl!("width"),
        "height" => tl!("height"),
        "corner radius" => tl!("corner radius"),
        "centre x" => tl!("centre x"),
        "centre y" => tl!("centre y"),
        "diameter" => tl!("diameter"),
        "copies" => tl!("copies"),
        "pitch" => tl!("pitch"),
        "length" => tl!("length"),
        "angle" => tl!("angle"),
        other => other,
    }
}

/// A shape kind's label, translated.
fn kind_label(kind: &str) -> &str {
    match kind {
        "rect" => tl!("rect"),
        "circle" => tl!("circle"),
        "line" => tl!("line"),
        other => other,
    }
}

/// The unit dropdown's entries (`""` shows as "No unit").
fn unit_label(unit: &str) -> String {
    if unit.is_empty() { tl!("No unit").to_string() } else { unit.to_string() }
}

/// The unit dropdown's entries, as shown.
pub(crate) fn unit_labels() -> Vec<String> {
    params::UNITS.iter().map(|u| unit_label(u)).collect()
}

// ---------------------------------------------------------------- the table (E1 Part B/C)

/// The columns of the parameter table, in order. Fixed ones have a width; flexible ones share
/// what is left in proportion to their weight, never below their minimum.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Col {
    Star,
    Name,
    Unit,
    Expression,
    Value,
    Comment,
    Drives,
    Delete,
}

impl Col {
    fn label(self) -> &'static str {
        match self {
            Col::Star => "",
            Col::Name => tl!("Name"),
            Col::Unit => tl!("Unit"),
            Col::Expression => tl!("Expression"),
            Col::Value => tl!("Value"),
            Col::Comment => tl!("Comment"),
            Col::Drives => tl!("Drives"),
            Col::Delete => "",
        }
    }

    /// `(fixed width)` or `(weight, minimum)`. The minimums add up to 258 with the gaps: a
    /// 280 pt panel (260 inside) holds the six main columns.
    fn size(self, value_w: f32) -> (Option<f32>, f32, f32) {
        match self {
            Col::Star | Col::Delete => (Some(18.0), 0.0, 18.0),
            Col::Unit => (Some(56.0), 0.0, 56.0),
            Col::Value => (Some(value_w), 0.0, value_w),
            Col::Name => (None, 1.0, 48.0),
            Col::Expression => (None, 1.6, 56.0),
            Col::Comment => (None, 1.2, 60.0),
            Col::Drives => (None, 1.2, 80.0),
        }
    }

    fn sortable(self) -> bool {
        matches!(self, Col::Name | Col::Unit | Col::Expression | Col::Value | Col::Comment)
    }
}

/// How a table is sorted: the column and the direction (kept per table id in egui memory).
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct Sort {
    pub col: Option<u8>,
    pub desc: bool,
}

const COL_GAP: f32 = 4.0;

/// Below this width the Comment column is left out; below the second, Drives too.
const COMMENT_MIN: f32 = 360.0;
const DRIVES_MIN: f32 = 470.0;

/// The column widths for `available` points: fixed columns as they are, the rest shared by weight.
pub(crate) fn column_widths(cols: &[Col], available: f32, value_w: f32) -> Vec<f32> {
    let gaps = COL_GAP * cols.len().saturating_sub(1) as f32;
    let fixed: f32 = cols.iter().filter_map(|c| c.size(value_w).0).sum();
    let weights: f32 = cols.iter().filter(|c| c.size(value_w).0.is_none()).map(|c| c.size(value_w).1).sum();
    let mut left = (available - gaps - fixed).max(0.0);
    let mut out: Vec<f32> = cols.iter().map(|c| c.size(value_w).0.unwrap_or(0.0)).collect();
    // A flexible column whose share is under its minimum takes the minimum; the rest share what is left.
    let mut pending: Vec<usize> = (0..cols.len()).filter(|i| cols[*i].size(value_w).0.is_none()).collect();
    let mut weights = weights;
    loop {
        let mut settled = false;
        for &i in &pending {
            let (_, weight, min) = cols[i].size(value_w);
            if left * weight / weights.max(1e-6) < min {
                out[i] = min;
                left = (left - min).max(0.0);
                weights -= weight;
                settled = true;
            }
        }
        if settled {
            pending.retain(|i| out[*i] == 0.0);
            continue;
        }
        for &i in &pending {
            let (_, weight, _) = cols[i].size(value_w);
            out[i] = left * weight / weights.max(1e-6);
        }
        break;
    }
    out
}

fn sort_rows<'a>(rows: &[&'a Row], sort: Sort) -> Vec<&'a Row> {
    let mut v: Vec<&Row> = rows.to_vec();
    let Some(col) = sort.col else { return v };
    let key = |r: &Row| -> (Option<f64>, String) {
        match col {
            1 => (None, r.row.name.to_lowercase()),
            2 => (None, r.row.unit.clone()),
            3 => (None, r.row.expression.to_lowercase()),
            4 => (Some(r.value.unwrap_or(f64::MAX)), String::new()),
            _ => (None, r.row.comment.to_lowercase()),
        }
    };
    v.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        let o = match (ka.0, kb.0) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            _ => ka.1.cmp(&kb.1),
        };
        if sort.desc { o.reverse() } else { o }
    });
    v
}

/// One table: a header row aligned with its columns (click a header to sort by it), then a row per
/// parameter: ★ · Name · Unit · Expression · Value · Comment (· Drives, for a sketch's dimensions)
/// · delete. Every edit runs a `parameters.*` command (one undo step each); an expression that does
/// not evaluate shows its error on the row, in the Value cell. The columns share the width the
/// caller gives (E1 Part B): widen the panel and the fields widen.
pub(crate) fn table(app: &mut VectorcraftApp, ui: &mut Ui, id: &str, rows: &[&Row], unit_names: &[&str], drives: Option<&dyn Fn(&Row) -> String>) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let available = ui.available_width();
    let mut cols = vec![Col::Star, Col::Name, Col::Unit, Col::Expression, Col::Value];
    if available >= COMMENT_MIN {
        cols.push(Col::Comment);
    }
    if drives.is_some() && available >= DRIVES_MIN {
        cols.push(Col::Drives);
    }
    cols.push(Col::Delete);
    let font = egui::FontId::proportional(12.0);
    let value_w =
        rows.iter().map(|r| ui.painter().layout_no_wrap(shown_value(r), font.clone(), t.text).size().x + 6.0).fold(44.0_f32, f32::max).min(120.0);
    let widths = column_widths(&cols, available, value_w);
    let sort_key = format!("params-sort/{id}");
    let mut sort: Sort = super::pstate(ui.ctx(), &sort_key);
    ui.push_id(("param-table", id), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(COL_GAP, 3.0);
        // The header row.
        ui.horizontal(|ui| {
            for (i, (c, w)) in cols.iter().zip(&widths).enumerate() {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(*w, 18.0), if c.sortable() { egui::Sense::click() } else { egui::Sense::hover() });
                let sorted = sort.col == Some(i as u8);
                let text = match (sorted, sort.desc) {
                    (true, false) => format!("{} ▲", c.label()),
                    (true, true) => format!("{} ▼", c.label()),
                    _ => c.label().to_string(),
                };
                let colour = if resp.hovered() || sorted { t.text_strong } else { t.text_dim };
                ui.painter().with_clip_rect(rect).text(rect.left_center(), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(11.0), colour);
                if resp.clicked() {
                    sort = if sorted && !sort.desc {
                        Sort { col: Some(i as u8), desc: true }
                    } else if sorted {
                        Sort::default()
                    } else {
                        Sort { col: Some(i as u8), desc: false }
                    };
                    super::set_pstate(ui.ctx(), &sort_key, sort);
                }
            }
        });
        let hr = ui.available_rect_before_wrap();
        ui.painter().hline(hr.x_range(), hr.top(), egui::Stroke::new(1.0, t.divider));
        ui.add_space(2.0);
        for r in sort_rows(rows, sort) {
            ui.push_id(("row", &r.row.name), |ui| {
                ui.horizontal(|ui| {
                    for (c, w) in cols.iter().zip(&widths) {
                        // Each cell is exactly its column wide: a control that wants more is clipped
                        // inside it, so a row never passes the table's edge.
                        ui.allocate_ui_with_layout(egui::vec2(*w, 26.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.set_min_size(egui::vec2(*w, 26.0));
                            ui.set_max_width(*w);
                            ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                            cell(app, ui, *c, *w, r, unit_names, drives);
                        });
                    }
                });
            });
        }
    });
}

fn cell(app: &mut VectorcraftApp, ui: &mut Ui, col: Col, width: f32, r: &Row, unit_names: &[&str], drives: Option<&dyn Fn(&Row) -> String>) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let name = r.row.name.clone();
    match col {
        Col::Star => {
            if widgets::icon_button(ui, "star", tl!("Favorite"), r.row.favourite, 18.0).clicked() {
                app.run("parameters.set", json!({"name": name, "favourite": !r.row.favourite})).ok();
            }
        }
        Col::Name => {
            if let Some(to) = widgets::exact_text_field(ui, "name", &name, width)
                && let Err(e) = app.run("parameters.rename", json!({"from": name, "to": to.trim()}))
            {
                app.ui.status = e;
            }
        }
        Col::Unit => {
            // The combo draws its arrow outside the width it is given, and "No unit" is long: a dash.
            let current = if r.row.unit.is_empty() { "—".to_string() } else { unit_label(&r.row.unit) };
            if let Some(i) = widgets::dropdown_names(ui, "unit", &current, unit_names, width - 8.0)
                && let Some(u) = params::UNITS.get(i)
            {
                app.run("parameters.set", json!({"name": name, "unit": u})).ok();
            }
        }
        Col::Expression => {
            if let Some(e) = widgets::exact_text_field(ui, "expr", &r.row.expression, width) {
                app.run("parameters.set", json!({"name": name, "expression": e})).ok();
            }
        }
        Col::Value => {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
            let (text, colour) = match &r.error {
                Some(e) => (e.clone(), t.error),
                None => (shown_value(r), t.text),
            };
            ui.painter().with_clip_rect(rect).text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                text.clone(),
                egui::FontId::proportional(12.0),
                colour,
            );
            if r.error.is_some() {
                ui.interact(rect, ui.id().with("value-err"), egui::Sense::hover()).on_hover_text(text);
            }
        }
        Col::Comment => {
            if let Some(c) = widgets::exact_text_field(ui, "comment", &r.row.comment, width) {
                app.run("parameters.set", json!({"name": name, "comment": c})).ok();
            }
        }
        Col::Drives => {
            let text = drives.map(|f| f(r)).unwrap_or_default();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
            ui.painter().with_clip_rect(rect).text(rect.left_center(), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(12.0), t.text_dim);
        }
        Col::Delete => {
            if widgets::icon_button(ui, "trash-2", tl!("Delete Parameter"), false, 18.0).clicked() {
                app.run("parameters.remove", json!({"name": r.row.name})).ok();
            }
        }
    }
}

/// A model-parameter group: the sketch that owns the rows.
pub(crate) struct SketchGroup {
    pub id: NodeId,
    pub title: String,
    pub sketch: Option<sketch::Sketch>,
    pub rows: Vec<Row>,
}

/// The rows split as Fusion's Change Parameters lists them: the user parameters, then the model
/// parameters grouped by the sketch that owns them (in document order).
pub(crate) fn grouped(st: &DocState, rows: Vec<Row>) -> (Vec<Row>, Vec<SketchGroup>) {
    let (mut user, mut groups): (Vec<Row>, Vec<SketchGroup>) = (vec![], vec![]);
    for r in rows {
        match r.row.sketch {
            None => user.push(r),
            Some(id) => {
                let at = match groups.iter().position(|g| g.id.0 == id) {
                    Some(at) => at,
                    None => {
                        let node = st.doc.node(NodeId(id));
                        let name = node.map_or_else(|| tl!("Sketch").to_string(), |n| n.display_name());
                        let sketch = node.and_then(|n| n.extras().sketch.as_deref().cloned());
                        groups.push(SketchGroup { id: NodeId(id), title: format!("{name} {id}"), sketch, rows: vec![] });
                        groups.len() - 1
                    }
                };
                if let Some(g) = groups.get_mut(at) {
                    g.rows.push(r);
                }
            }
        }
    }
    (user, groups)
}

/// What a dimension of `sketch` named `param` measures: `Length of line 3`, `Radius of arc 7`…
pub(crate) fn drives(sketch: &sketch::Sketch, param: &str) -> Option<String> {
    use vectorcraft_doc::sketch::model::{Constraint, Entity};
    let c = sketch.constraints.iter().find(|c| c.param() == Some(param))?;
    let radius_of = |e: u32, what: &str| match sketch.entities.iter().find(|x| x.id() == e) {
        Some(Entity::Arc { .. }) => format!("{what} {} {}", tl!("arc"), e + 1),
        _ => format!("{what} {} {}", tl!("circle"), e + 1),
    };
    Some(match c {
        Constraint::Length { line, .. } => format!("{} {}", tl!("Length of line"), line + 1),
        Constraint::Angle { line, .. } => format!("{} {}", tl!("Angle of line"), line + 1),
        Constraint::AngleBetween { a, b, .. } => format!("{} {} {} {}", tl!("Angle between lines"), a + 1, tl!("and"), b + 1),
        Constraint::Distance { a, b, .. } => format!("{} {} {} {}", tl!("Distance between points"), a + 1, tl!("and"), b + 1),
        Constraint::DistancePointLine { line, .. } => format!("{} {}", tl!("Offset from line"), line + 1),
        Constraint::Radius { e, .. } => radius_of(*e, tl!("Radius of")),
        Constraint::Diameter { e, .. } => radius_of(*e, tl!("Diameter of")),
        _ => return None,
    })
}

/// The whole Fusion-style listing: Favorites (the starred rows), User Parameters (with New
/// Parameter), Model Parameters per sketch. Shared by the Parameters panel and the Change
/// Parameters dialog.
pub(crate) fn listing(app: &mut VectorcraftApp, ui: &mut Ui, id: &str) {
    let Some(st) = app.session.active() else {
        widgets::dim_label(ui, tl!("No document"));
        return;
    };
    // Owned snapshots: the rows below run commands, which borrow the app mutably.
    let all = rows(st);
    let (user, groups) = grouped(st, all.clone());
    let t = crate::theme::Tokens::get(ui.ctx());
    let units = unit_labels();
    let unit_names: Vec<&str> = units.iter().map(String::as_str).collect();
    let heading = |ui: &mut Ui, text: &str| {
        ui.label(egui::RichText::new(text).size(12.5).color(t.text_strong));
    };
    heading(ui, tl!("Favorites"));
    let favourites: Vec<&Row> = all.iter().filter(|r| r.row.favourite).collect();
    if favourites.is_empty() {
        widgets::dim_label(ui, tl!("No favorites yet. Star a parameter below."));
    } else {
        table(app, ui, &format!("{id}/favourites"), &favourites, &unit_names, None);
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        heading(ui, tl!("User Parameters"));
        if widgets::icon_button(ui, "plus", tl!("New Parameter"), false, 22.0).clicked() {
            app.run("parameters.add", json!({})).ok();
        }
    });
    if user.is_empty() {
        widgets::dim_label(ui, tl!("None"));
    } else {
        let refs: Vec<&Row> = user.iter().collect();
        table(app, ui, &format!("{id}/user"), &refs, &unit_names, None);
    }
    ui.add_space(6.0);
    heading(ui, tl!("Model Parameters"));
    if groups.is_empty() {
        widgets::dim_label(ui, tl!("None"));
    }
    for g in &groups {
        // Unindented: the table takes the panel's width, as the user table above it does.
        egui::CollapsingHeader::new(egui::RichText::new(&g.title).size(12.0).color(t.text))
            .id_salt(("group", id, &g.title))
            .default_open(true)
            .show_unindented(ui, |ui| {
                let refs: Vec<&Row> = g.rows.iter().collect();
                let drive = |r: &Row| g.sketch.as_ref().and_then(|s| drives(s, &r.row.name)).unwrap_or_default();
                table(app, ui, &format!("{id}/{}", g.id.0), &refs, &unit_names, Some(&drive));
            });
    }
}

pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let Some(st) = app.session.active() else {
        widgets::dim_label(ui, tl!("No document"));
        return;
    };
    let sketch = selected_sketch(st).and_then(|id| st.doc.node(id).and_then(|n| n.extras().sketch.as_deref().cloned()).map(|s| (id, s)));
    let built = sketch.as_ref().map(|(_, s)| sketch::build(s, &params::solve(&params::rows(&st.doc))));
    let t = crate::theme::Tokens::get(ui.ctx());
    // The host (the workbench column, the dock) scrolls; the panel lays out to the width it has.
    let width = ui.available_width();
    ui.scope(|ui| {
        ui.set_max_width(width);
        listing(app, ui, "panel");

        // The selected v1 sketch's shapes (made by Object › Parametric Sketch › New Rectangle…).
        let Some(((id, sketch), built)) = sketch.zip(built) else { return };
        if sketch.shapes.is_empty() {
            return;
        }
        ui.add_space(4.0);
        widgets::subheader(ui, tl!("Selected Sketch"));
        for (shape, b) in sketch.shapes.iter().zip(&built) {
            ui.push_id(("shape", &shape.id), |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{} · {}", shape.id, kind_label(&shape.kind))).size(12.5).color(t.text_strong));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, "trash-2", tl!("Remove Shape"), false, 18.0).clicked() {
                            app.run("sketch.removeShape", json!({"id": id.0, "shape": shape.id})).ok();
                        }
                    });
                });
                let field_w = (ui.available_width() - 110.0).max(80.0);
                egui::Grid::new("fields").num_columns(3).spacing([6.0, 2.0]).show(ui, |ui| {
                    for field in sketch::fields_of(&shape.kind).unwrap_or(&[]) {
                        widgets::field_label(ui, egui::RichText::new(field_label(&shape.kind, field)).size(12.0).color(t.text_dim));
                        let expression = shape.fields.get(*field).cloned().unwrap_or_default();
                        if let Some(e) = widgets::exact_text_field(ui, ("field", *field), &expression, field_w) {
                            app.run("sketch.setField", json!({"id": id.0, "shape": shape.id, "field": field, "expression": e})).ok();
                        }
                        let unit = if sketch::unitless(field) { "" } else { "mm" };
                        let (text, err) = match b.fields.get(*field) {
                            Some(Ok(v)) => (params::format_value(Some(*v), unit), None),
                            Some(Err(e)) => (tl!("error").to_string(), Some(e.clone())),
                            None => ("—".to_string(), None),
                        };
                        let label = ui.label(egui::RichText::new(text).size(12.0).color(if err.is_some() { t.accent_strong } else { t.text }));
                        if let Some(e) = err {
                            label.on_hover_text(e);
                        }
                        ui.end_row();
                    }
                });
            });
        }
        ui.horizontal(|ui| {
            let tips = [("rect", "square", tl!("Add Rectangle")), ("circle", "circle", tl!("Add Circle")), ("line", "dc-line", tl!("Add Line"))];
            for (kind, icon, tip) in tips {
                if widgets::icon_button(ui, icon, tip, false, 24.0).clicked() {
                    app.run("sketch.addShape", json!({"id": id.0, "kind": kind})).ok();
                }
            }
        });
    });
}

/// The panel's (≡) menu.
pub fn menu(app: &mut VectorcraftApp, ui: &mut Ui) {
    let Some(st) = app.session.active() else { return };
    let has_rows = !params::rows(&st.doc).is_empty();
    let sketch = selected_sketch(st);
    if widgets::menu_item(ui, tl!("New Parameter"), true, false) {
        app.run("parameters.add", json!({})).ok();
    }
    if widgets::menu_item(ui, tl!("Copy Parameters as CSV"), has_rows, false)
        && let Ok(v) = app.run("parameters.csv", json!({}))
        && let Some(csv) = v["csv"].as_str()
    {
        ui.ctx().copy_text(csv.to_string());
    }
    ui.separator();
    for (label, kind) in [("New Rectangle", "rect"), ("New Circle", "circle"), ("New Line", "line")] {
        if widgets::menu_item(ui, tl!(label), true, false) {
            match sketch {
                Some(id) => app.run("sketch.addShape", json!({"id": id.0, "kind": kind})).ok(),
                None => app.run("sketch.create", json!({"shapes": [{"kind": kind}]})).ok(),
            };
        }
    }
    if widgets::menu_item(ui, tl!("Sample: Mounting plate"), true, false) {
        app.run("sketch.sample", json!({})).ok();
    }
}

/// Is `n` a parametric sketch group (the control bar names it)?
pub(crate) fn is_sketch(n: &Node) -> bool {
    n.extras().sketch.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_sample() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width": 600, "height": 400})).unwrap();
        app.session.execute("sketch.sample", &json!({})).unwrap();
        app
    }

    #[test]
    fn draws_headless() {
        let mut app = app_with_sample();
        let text = crate::tests_labels::painted_text(&mut app, show);
        for s in ["Favorites", "User Parameters", "Name", "Expression", "width", "hole_dia", "120 mm", "Selected Sketch", "R1", "C1", "pitch"] {
            assert!(text.contains(s), "{s} missing:\n{text}");
        }
        // Without a selection the v1 shape section is left out.
        app.session.execute("select.none", &json!({})).unwrap();
        let text = crate::tests_labels::painted_text(&mut app, show);
        assert!(!text.contains("Selected Sketch"), "{text}");
        // Without a document.
        let mut empty = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let text = crate::tests_labels::painted_text(&mut empty, show);
        assert!(text.contains("No document"), "{text}");
    }

    #[test]
    fn the_panel_menu_lists_its_commands() {
        let mut app = app_with_sample();
        let text = crate::tests_labels::painted_text(&mut app, menu);
        for item in ["New Parameter", "Copy Parameters as CSV", "New Rectangle", "New Circle", "New Line", "Sample: Mounting plate"] {
            assert!(text.contains(item), "{item} missing from the panel menu:\n{text}");
        }
    }

    #[test]
    fn a_slider_value_is_one_undo_step() {
        let mut app = app_with_sample();
        let width = rows(app.session.doc().unwrap()).into_iter().find(|r| r.row.name == "width").unwrap();
        assert!(width.slides);
        let before = app.session.doc().unwrap().history.undo.len();
        set_value(&mut app, &width.row, 150.0, Live::Dragging);
        set_value(&mut app, &width.row, 160.0, Live::Dragging);
        set_value(&mut app, &width.row, 170.0, Live::Released);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before + 1);
        let list = app.run("parameters.list", json!({})).unwrap();
        assert_eq!(list["rows"][0]["expression"], "170 mm");
        // Clamped to the range.
        set_value(&mut app, &width.row, 1000.0, Live::Released);
        assert_eq!(app.run("parameters.list", json!({})).unwrap()["rows"][0]["expression"], "300 mm");
    }

    #[test]
    fn the_columns_share_the_width_and_the_header_lines_up() {
        let cols = [Col::Star, Col::Name, Col::Unit, Col::Expression, Col::Value, Col::Comment, Col::Delete];
        let narrow = column_widths(&cols, 380.0, 48.0);
        let wide = column_widths(&cols, 640.0, 48.0);
        let sum = |w: &[f32]| w.iter().sum::<f32>() + COL_GAP * (w.len() - 1) as f32;
        assert!((sum(&wide) - 640.0).abs() < 0.5, "{wide:?}");
        // At 380 the flexible columns share what is left; at 640 they grew and the fixed ones did not.
        assert!((sum(&narrow) - 380.0).abs() < 0.5, "{narrow:?}");
        let six = [Col::Star, Col::Name, Col::Unit, Col::Expression, Col::Value, Col::Delete];
        let at_260 = column_widths(&six, 260.0, 44.0);
        assert!((sum(&at_260) - 260.0).abs() < 0.5, "the six main columns fill a 280 pt panel exactly: {at_260:?}");
        // Minimums win when there is no room, and nothing goes below them.
        let tiny = column_widths(&six, 100.0, 44.0);
        assert_eq!(tiny, [18.0, 48.0, 56.0, 56.0, 44.0, 18.0]);
        assert_eq!((narrow[0], narrow[2], narrow[4], narrow[6]), (wide[0], wide[2], wide[4], wide[6]));
        assert!(wide[1] > narrow[1] && wide[3] > narrow[3] && wide[5] > narrow[5]);
        assert!(wide[3] > wide[1], "Expression is the widest flexible column");
    }

    #[test]
    fn clicking_a_header_sorts_and_an_error_shows_on_the_row() {
        let mut app = app_with_sample();
        app.run("parameters.add", json!({"name": "zz", "expression": "nosuch + 1"})).unwrap();
        app.run("parameters.add", json!({"name": "aa", "expression": "3 mm"})).unwrap();
        let text = crate::tests_labels::painted_text(&mut app, show);
        let at = |s: &str| text.find(s).unwrap_or_else(|| panic!("{s} missing:\n{text}"));
        assert!(text.contains("nosuch"), "the error text is on the row:\n{text}");
        // Document order first; sorted by name after a click on the Name header.
        assert!(at("zz") < at("aa"));
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let sort_key = "params-sort/panel/user";
        crate::panels::set_pstate(&ctx, sort_key, Sort { col: Some(1), desc: false });
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| show(&mut app, ui));
        out.textures_delta.clear();
        let text = crate::tests_labels::shapes_text(&out);
        let at = |s: &str| text.find(s).unwrap_or_else(|| panic!("{s} missing:\n{text}"));
        assert!(at("aa") < at("zz") && text.contains("Name ▲"), "{text}");
    }

    #[test]
    fn the_panel_is_registered_with_its_own_icon() {
        let row = crate::state::ICON_PANELS.iter().find(|(id, ..)| *id == ID).expect("in the icon column");
        assert_eq!(row.1, "Parameters");
        assert!(crate::icons::exists(row.2), "{} is not a bundled icon", row.2);
        assert!(crate::state::ICON_PANEL_GROUPS.iter().any(|g| g.contains(&ID)), "{ID} is in no ICON_PANEL_GROUPS row");
    }

    #[test]
    fn window_parameters_opens_the_panel_and_object_has_the_submenu() {
        let app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let entries = crate::menus::menu_entries(&app);
        // Window > Parameters now opens the Change Parameters dialog; the panel stays reachable by `window.panel`.
        let dialog = entries.iter().find(|e| e.command.as_deref() == Some("window.changeParameters")).expect("Window > Parameters is listed");
        assert_eq!((dialog.path.clone(), dialog.label.as_str()), (vec!["Window".to_string()], "Parameters"));
        assert!(
            !entries
                .iter()
                .any(|e| e.command.as_deref() == Some("window.panel") && e.params.get("panel").and_then(serde_json::Value::as_str) == Some(ID)),
            "the panel is no longer a Window menu row"
        );
        assert!(crate::menus::UI_COMMANDS.iter().any(|c| c.0 == "window.panel"));
        let sketch: Vec<&str> = entries.iter().filter(|e| e.path == ["Object", "Parametric Sketch"]).map(|e| e.label.as_str()).collect();
        assert_eq!(sketch, ["New Sketch", "New Rectangle", "New Circle", "New Line", "Sample: Mounting plate"]);
    }
}
