//! User parameters (Euclidean): the rows in `document.unknown["euclidean.params"]`
//! (`vectorcraft_doc::params`) that parametric sketches follow. Every surface reaches them
//! here: the Parameters panel, the control bar's sliders, the CLI (`vectorcraft-cli run --cmd
//! parameters.list`), MCP `command_run` and the control channel. `parameters.list` is the
//! Mothership's read; `parameters.csv` is the HBC Client Params form.
use super::*;
use serde_json::{Value, json};
use vectorcraft_doc::params::{self, ParamRow};
use vectorcraft_doc::{Document, expr};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "parameters.list",
            "List Parameters",
            [],
            None,
            "{} → {rows: [{name, unit: mm|in|'', expression, value (in the row's unit, null on error), error, comment, favourite, min, max, step}]} the drawing's user parameters, solved",
            has_doc,
            list
        ),
        cmd!(
            query "parameters.csv",
            "Parameters as CSV",
            [],
            None,
            "{} → {csv} the user parameters as `name,unit,expression,comment` rows under a header",
            has_doc,
            csv
        ),
        cmd!(
            "parameters.add",
            "Add Parameter",
            [],
            None,
            "{name? (default param1, param2…), unit?: mm|in|'' (mm), expression? ('10 mm'), comment?, favourite?, min?, max?, step?} → {name}",
            has_doc,
            add
        ),
        cmd!(
            "parameters.set",
            "Set Parameter",
            [],
            None,
            "{name, expression?, unit?, comment?, favourite?, min?, max?, step? (null clears a range value)} one undo step; starring a row without a range gives it half…double its value → {name, value, error}",
            has_doc,
            set
        ),
        cmd!("parameters.remove", "Remove Parameter", [], None, "{name} → {name}", has_doc, remove),
        cmd!(
            "parameters.rename",
            "Rename Parameter",
            [],
            None,
            "{from, to} renames the parameter and rewrites every whole-word reference in the other parameters and in every sketch field → {from, to}",
            has_doc,
            rename
        ),
    ]
}

/// A trimmed name param that is a legal parameter name.
fn name_param(p: &Value, key: &str, cmd: &str) -> Result<String> {
    let Some(name) = str_param(p, key).map(str::trim).filter(|n| !n.is_empty()) else { return Err(bad(cmd, format!("missing `{key}`"))) };
    if !params::valid_name(name) {
        return Err(bad(cmd, format!("`{key}` must start with a letter and use letters, digits and _ (not a function or unit name)")));
    }
    Ok(name.to_string())
}

fn expression_param(p: &Value, cmd: &str) -> Result<Option<String>> {
    match p.get("expression") {
        None => Ok(None),
        Some(Value::String(s)) if s.len() <= expr::MAX_LEN => Ok(Some(s.trim().to_string())),
        Some(Value::String(_)) => Err(bad(cmd, format!("`expression` is longer than {} characters", expr::MAX_LEN))),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(_) => Err(bad(cmd, "`expression` must be text")),
    }
}

fn unit_param(p: &Value, cmd: &str) -> Result<Option<String>> {
    match str_param(p, "unit") {
        None => Ok(None),
        Some(u) if params::valid_unit(u) => Ok(Some(u.to_string())),
        Some(_) => Err(bad(cmd, "`unit` must be mm, in or an empty string")),
    }
}

/// `Some(Some(v))` to set, `Some(None)` to clear (`null`), `None` to leave alone.
fn range_param(p: &Value, key: &str, cmd: &str) -> Result<Option<Option<f64>>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => {
            v.as_f64().filter(|f| f.is_finite()).map(|f| Some(Some(f))).ok_or_else(|| bad(cmd, format!("`{key}` must be a finite number or null")))
        }
    }
}

fn comment_param(p: &Value, cmd: &str) -> Result<Option<String>> {
    match p.get("comment") {
        None => Ok(None),
        Some(Value::String(s)) if s.chars().count() <= 1000 => Ok(Some(s.to_string())),
        Some(_) => Err(bad(cmd, "`comment` must be text of at most 1000 characters")),
    }
}

/// Apply the optional fields of `add` / `set` to `row`.
fn apply(row: &mut ParamRow, p: &Value, cmd: &str) -> Result<()> {
    if let Some(e) = expression_param(p, cmd)? {
        row.expression = e;
    }
    if let Some(u) = unit_param(p, cmd)? {
        row.unit = u;
    }
    if let Some(c) = comment_param(p, cmd)? {
        row.comment = c;
    }
    if let Some(f) = p.get("favourite") {
        row.favourite = f.as_bool().ok_or_else(|| bad(cmd, "`favourite` must be true or false"))?;
    }
    for (key, slot) in [("min", &mut row.min), ("max", &mut row.max), ("step", &mut row.step)] {
        if let Some(v) = range_param(p, key, cmd)? {
            *slot = v;
        }
    }
    row.sanitize();
    Ok(())
}

/// A starred row without a range gets the sandbox's default (half to double its value).
fn default_range(row: &mut ParamRow, rows: &[ParamRow]) {
    if row.favourite && (row.min.is_none() || row.max.is_none()) {
        let solved = params::solve(rows);
        let value = params::to_unit(solved.value(&row.name).unwrap_or(0.0), &row.unit);
        let (min, max, step) = params::default_range(value, &row.unit);
        row.min.get_or_insert(min);
        row.max.get_or_insert(max);
        row.step.get_or_insert(step);
    }
}

fn row_json(row: &ParamRow, solved: &expr::Solved) -> Value {
    json!({
        "name": row.name,
        "unit": row.unit,
        "expression": row.expression,
        "value": solved.value(&row.name).map(|v| params::to_unit(v, &row.unit)),
        "error": solved.error(&row.name),
        "comment": row.comment,
        "favourite": row.favourite,
        "min": row.min,
        "max": row.max,
        "step": row.step,
    })
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    let rows = params::rows(&s.doc()?.doc);
    let solved = params::solve(&rows);
    Ok(json!({"rows": rows.iter().map(|r| row_json(r, &solved)).collect::<Vec<_>>()}))
}

fn csv(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({"csv": params::csv(&params::rows(&s.doc()?.doc))}))
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "parameters.add";
    let mut rows = params::rows(&s.doc()?.doc);
    if rows.len() >= params::MAX_ROWS {
        return Err(bad(C, format!("a drawing holds at most {} parameters", params::MAX_ROWS)));
    }
    let name = match p.get("name") {
        Some(_) => name_param(p, "name", C)?,
        // `param1`, `param2`…: a space would not be a name.
        None => (1..).map(|i| format!("param{i}")).find(|n| !rows.iter().any(|r| &r.name == n)).unwrap_or_else(|| "param".into()),
    };
    if rows.iter().any(|r| r.name == name) {
        return Err(bad(C, format!("parameter `{name}` already exists")));
    }
    let unit = unit_param(p, C)?.unwrap_or_else(|| "mm".into());
    let expression = match unit.as_str() {
        "" => "10",
        "in" => "1 in",
        _ => "10 mm",
    };
    let mut row = ParamRow { name: name.clone(), unit, expression: expression.into(), ..Default::default() };
    apply(&mut row, p, C)?;
    rows.push(row.clone());
    default_range(&mut row, &rows);
    if let Some(last) = rows.last_mut() {
        *last = row;
    }
    s.edit("Add Parameter", |d, _| {
        params::store(d, &rows);
        Ok(())
    })?;
    Ok(json!({"name": name}))
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "parameters.set";
    let name = name_param(p, "name", C)?;
    let mut rows = params::rows(&s.doc()?.doc);
    let i = rows.iter().position(|r| r.name == name).ok_or_else(|| bad(C, format!("no parameter `{name}`")))?;
    let mut row = rows.get(i).cloned().unwrap_or_default();
    apply(&mut row, p, C)?;
    if let Some(slot) = rows.get_mut(i) {
        *slot = row.clone();
    }
    default_range(&mut row, &rows);
    if let Some(slot) = rows.get_mut(i) {
        *slot = row.clone();
    }
    let solved = params::solve(&rows);
    let value = solved.value(&name).map(|v| params::to_unit(v, &row.unit));
    let error = solved.error(&name).map(str::to_string);
    if params::rows(&s.doc()?.doc).get(i) != Some(&row) {
        s.edit("Set Parameter", |d, _| {
            params::store(d, &rows);
            Ok(())
        })?;
    }
    Ok(json!({"name": name, "value": value, "error": error}))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "parameters.remove";
    let name = name_param(p, "name", C)?;
    let mut rows = params::rows(&s.doc()?.doc);
    let n = rows.len();
    rows.retain(|r| r.name != name);
    if rows.len() == n {
        return Err(bad(C, format!("no parameter `{name}`")));
    }
    s.edit("Remove Parameter", |d, _| {
        params::store(d, &rows);
        Ok(())
    })?;
    Ok(json!({"name": name}))
}

/// Rename `from` to `to` in every sketch recipe of `d`.
fn rename_in_sketches(d: &mut Document, from: &str, to: &str) {
    let mut ids = Vec::new();
    d.walk(|n| {
        if n.extras().sketch.is_some() {
            ids.push(n.id);
        }
    });
    for id in ids {
        if let Some(n) = d.node_mut(id)
            && let Some(e) = n.extras_if_any()
            && let Some(sk) = &mut e.sketch
        {
            sk.rename_references(from, to);
        }
    }
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "parameters.rename";
    let from = name_param(p, "from", C)?;
    let to = name_param(p, "to", C)?;
    let mut rows = params::rows(&s.doc()?.doc);
    if !rows.iter().any(|r| r.name == from) {
        return Err(bad(C, format!("no parameter `{from}`")));
    }
    if from == to {
        return Ok(json!({"from": from, "to": to}));
    }
    if rows.iter().any(|r| r.name == to) {
        return Err(bad(C, format!("parameter `{to}` already exists")));
    }
    for r in &mut rows {
        if r.name == from {
            r.name = to.clone();
        }
        r.expression = expr::rename_references(&r.expression, &from, &to);
    }
    s.edit("Rename Parameter", |d, _| {
        params::store(d, &rows);
        rename_in_sketches(d, &from, &to);
        Ok(())
    })?;
    Ok(json!({"from": from, "to": to}))
}
