//! User parameters (Euclidean): named values a drawing's parametric sketches follow, kept in
//! `Document::unknown["euclidean.params"]` so the file format needs no change and every exporter
//! ignores them. Rows are `{name, unit, expression, comment, favourite, min, max, step}`.
//!
//! Expressions evaluate in millimetres ([`crate::expr`]); a row's `unit` (`mm`, `in` or `""` for
//! a plain number) is how its value is shown and how its slider range is written.

use serde::{Deserialize, Serialize};

use crate::Document;
use crate::expr::{self, Solved};

/// Key of the parameter rows in `Document::unknown`.
pub const DOC_KEY: &str = "euclidean.params";
/// The units a row may have: millimetres, inches, degrees, or none (a count or a ratio).
pub const UNITS: [&str; 4] = ["mm", "in", "deg", ""];
/// The longest parameter name, in characters.
pub const MAX_NAME: usize = 64;
/// At most this many rows are kept (a file with more is cut here, not refused).
pub const MAX_ROWS: usize = 2000;

/// One user parameter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParamRow {
    pub name: String,
    /// `mm`, `in`, or `""` (no unit).
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub expression: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Starred: shown with a slider in the panel and the control bar.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favourite: bool,
    /// Slider range and step, in the row's own unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    /// The `NodeId` (`.0`) of the sketch that owns this model parameter (a dimension, `d1`, `d2`…);
    /// `None` for a user parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<u64>,
}

impl ParamRow {
    /// A row named `name` holding the plain number `value` (in `unit`).
    pub fn plain(name: &str, unit: &str, value: f64) -> Self {
        ParamRow { name: name.into(), unit: unit.into(), expression: plain_expression(value, unit), ..Default::default() }
    }

    /// Is this a model parameter, owned by a sketch's dimension?
    pub fn is_model(&self) -> bool {
        self.sketch.is_some()
    }

    /// Is the expression one number (what a slider can drive)?
    pub fn is_plain(&self) -> bool {
        expr::is_plain(&self.expression)
    }

    /// Keep the row within what the panel and the file expect: a legal unit, finite range values.
    pub fn sanitize(&mut self) {
        if !valid_unit(&self.unit) {
            self.unit = "mm".into();
        }
        for v in [&mut self.min, &mut self.max, &mut self.step] {
            if v.is_some_and(|x| !x.is_finite()) {
                *v = None;
            }
        }
        if self.step.is_some_and(|s| s <= 0.0) {
            self.step = None;
        }
    }
}

/// Is `name` a legal parameter name: a letter or `_` then letters, digits and `_`, at most
/// [`MAX_NAME`] characters, and not a function, `pi` or a unit?
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.chars().count() <= MAX_NAME
        && !expr::is_function(name)
        && expr::unit_mm(name).is_none()
}

/// Is `unit` one a row may have?
pub fn valid_unit(unit: &str) -> bool {
    UNITS.contains(&unit)
}

/// The document's parameter rows (malformed entries are skipped, see [`ParamRow::sanitize`]).
pub fn rows(doc: &Document) -> Vec<ParamRow> {
    doc.unknown.get(DOC_KEY).map(parse_rows).unwrap_or_default()
}

/// Parse a stored rows value.
pub fn parse_rows(v: &serde_json::Value) -> Vec<ParamRow> {
    let parse = |r: &serde_json::Value| {
        let mut row: ParamRow = serde_json::from_value(r.clone()).ok()?;
        valid_name(&row.name).then(|| {
            row.sanitize();
            row
        })
    };
    v.as_array().map(|a| a.iter().take(MAX_ROWS).filter_map(parse).collect()).unwrap_or_default()
}

/// Store `rows` as the document's parameters (none removes the key).
pub fn store(doc: &mut Document, rows: &[ParamRow]) {
    if rows.is_empty() {
        if doc.unknown.contains_key(DOC_KEY) {
            doc.unknown.remove(DOC_KEY);
        }
    } else {
        doc.unknown.insert(DOC_KEY.into(), serde_json::to_value(rows).unwrap_or_default());
    }
}

/// Did the stored rows change between `before` and `after`? (Cheap when the map is shared.)
pub fn changed(before: &Document, after: &Document) -> bool {
    !crate::Shared::ptr_eq(&before.unknown, &after.unknown) && before.unknown.get(DOC_KEY) != after.unknown.get(DOC_KEY)
}

/// The name for the next model parameter: `d{n}`, `n` being one more than the largest `d<digits>`
/// among all rows, so names count up across the document in creation order.
pub fn next_model_name(rows: &[ParamRow]) -> String {
    let max = rows
        .iter()
        .filter_map(|r| r.name.strip_prefix('d'))
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|n| n.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("d{}", max.saturating_add(1))
}

/// Solve every row.
pub fn solve(rows: &[ParamRow]) -> Solved {
    let pairs: Vec<(String, String)> = rows.iter().map(|r| (r.name.clone(), r.expression.clone())).collect();
    Solved::solve(&pairs)
}

/// A solved value (millimetres or a plain number) in the row's unit.
pub fn to_unit(mm: f64, unit: &str) -> f64 {
    match unit {
        "in" => mm / 25.4,
        _ => mm,
    }
}

/// A value in the row's unit as millimetres (or the plain number itself).
pub fn from_unit(v: f64, unit: &str) -> f64 {
    match unit {
        "in" => v * 25.4,
        _ => v,
    }
}

/// `value` (in `unit`) written as a plain expression: `120 mm`, `0.5 in`, `4` (also for degrees).
pub fn plain_expression(value: f64, unit: &str) -> String {
    let n = trim_number(value, 6);
    // Degrees are a plain number to the expression language.
    if unit.is_empty() || unit == "deg" { n } else { format!("{n} {unit}") }
}

fn trim_number(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.decimals$}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

/// A solved value as the panel shows it: `120 mm`, `0.315 in`, `4`; `—` for none.
pub fn format_value(mm: Option<f64>, unit: &str) -> String {
    let Some(mm) = mm.filter(|v| v.is_finite()) else { return "—".into() };
    match unit {
        "in" => format!("{} in", trim_number(mm / 25.4, 3)),
        "" => trim_number(mm, 3),
        "deg" => format!("{} deg", trim_number(mm, 3)),
        _ => format!("{} mm", trim_number(mm, 2)),
    }
}

/// The slider range a parameter gets when it is starred without one: a quarter to four times its
/// value (in its unit; E1, was half to double), stepping by 1 (no unit), 0.5 (mm) or 0.125 (in).
pub fn default_range(value: f64, unit: &str) -> (f64, f64, f64) {
    let v = if value.is_finite() { value } else { 0.0 };
    let min = (v * 0.25).round().max(0.0);
    let max = match (v * 4.0).round() {
        m if m > min => m,
        _ => min + 10.0,
    };
    let step = match unit {
        "" | "deg" => 1.0,
        "in" => 0.125,
        _ => 0.5,
    };
    (min, max, step)
}

/// The rows as `name,unit,expression,comment` CSV (a header row first), the form the HBC order
/// system's Client Params sheet takes.
pub fn csv(rows: &[ParamRow]) -> String {
    fn cell(s: &str) -> String {
        if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
    }
    let mut out = String::from("name,unit,expression,comment\n");
    for r in rows {
        out.push_str(&format!("{},{},{},{}\n", cell(&r.name), cell(&r.unit), cell(&r.expression), cell(&r.comment)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<ParamRow> {
        vec![
            ParamRow {
                name: "width".into(),
                unit: "mm".into(),
                expression: "120 mm".into(),
                comment: "plate width".into(),
                favourite: true,
                min: Some(60.0),
                max: Some(300.0),
                step: Some(1.0),
                sketch: None,
            },
            ParamRow {
                name: "holes".into(),
                unit: String::new(),
                expression: "4".into(),
                favourite: true,
                min: Some(2.0),
                max: Some(10.0),
                step: Some(1.0),
                ..Default::default()
            },
            ParamRow {
                name: "slot_w".into(),
                unit: "mm".into(),
                expression: "width / 2".into(),
                comment: "follows, \"quoted\"".into(),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn rows_round_trip_through_the_document() {
        let mut d = Document::new(100.0, 100.0);
        assert!(rows(&d).is_empty());
        store(&mut d, &sample());
        let json = serde_json::to_value(&d).unwrap();
        assert!(json["unknown"][DOC_KEY].is_array(), "{}", json["unknown"]);
        let back: Document = serde_json::from_value(json).unwrap();
        assert_eq!(rows(&back), sample());
        // Unset options are left out of the file.
        let row = serde_json::to_value(&d).unwrap()["unknown"][DOC_KEY][2].clone();
        assert!(row.get("min").is_none() && row.get("favourite").is_none(), "{row}");
        store(&mut d, &[]);
        assert!(d.unknown.get(DOC_KEY).is_none());
    }

    #[test]
    fn malformed_rows_are_skipped_and_values_sanitized() {
        let v = serde_json::json!([
            {"name": "ok", "unit": "furlongs", "expression": "1", "step": -2, "min": null},
            {"name": "2bad", "expression": "1"},
            {"name": "pi", "expression": "1"},
            {"name": "mm", "expression": "1"},
            "not a row",
            {"expression": "no name"},
        ]);
        let rows = parse_rows(&v);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].unit.as_str(), rows[0].step, rows[0].min), ("mm", None, None));
    }

    #[test]
    fn names_are_checked() {
        for ok in ["a", "_x", "width", "hole_dia2", "A1"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "1a", "a-b", "a b", "sqrt", "pi", "in", "é", &"x".repeat(MAX_NAME + 1)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn a_no_unit_parameter_stays_a_plain_number() {
        let s = solve(&sample());
        assert_eq!(s.value("holes"), Some(4.0));
        assert_eq!(s.value("slot_w"), Some(60.0));
        assert_eq!(format_value(s.value("holes"), ""), "4");
        assert_eq!(format_value(s.value("width"), "mm"), "120 mm");
        assert_eq!(format_value(Some(25.4), "in"), "1 in");
        assert_eq!(format_value(None, "mm"), "—");
        assert_eq!(plain_expression(4.0, ""), "4");
        assert_eq!(plain_expression(0.125, "in"), "0.125 in");
        assert_eq!(plain_expression(12.5, "mm"), "12.5 mm");
        assert_eq!(to_unit(25.4, "in"), 1.0);
        assert_eq!(from_unit(1.0, "in"), 25.4);
    }

    #[test]
    fn default_ranges_follow_the_sandbox() {
        assert_eq!(default_range(120.0, "mm"), (30.0, 480.0, 0.5));
        assert_eq!(default_range(4.0, ""), (1.0, 16.0, 1.0));
        assert_eq!(default_range(1.0, "in"), (0.0, 4.0, 0.125));
        assert_eq!(default_range(0.4, "in"), (0.0, 2.0, 0.125));
        assert_eq!(default_range(0.0, "mm"), (0.0, 10.0, 0.5));
    }

    #[test]
    fn csv_is_name_unit_expression_comment() {
        let csv = csv(&sample());
        assert_eq!(csv, "name,unit,expression,comment\nwidth,mm,120 mm,plate width\nholes,,4,\nslot_w,mm,width / 2,\"follows, \"\"quoted\"\"\"\n");
    }

    #[test]
    fn model_rows_load_and_b2_rows_have_no_sketch() {
        let v = serde_json::json!([
            {"name": "width", "unit": "mm", "expression": "120 mm"},
            {"name": "d1", "unit": "deg", "expression": "30", "sketch": 7},
        ]);
        let rows = parse_rows(&v);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].sketch, rows[0].is_model()), (None, false));
        assert_eq!((rows[1].sketch, rows[1].is_model(), rows[1].unit.as_str()), (Some(7), true, "deg"));
        let out = serde_json::to_value(&rows).unwrap();
        assert!(out[0].get("sketch").is_none() && out[1]["sketch"] == 7, "{out}");
        assert_eq!(format_value(Some(30.0), "deg"), "30 deg");
        assert_eq!((to_unit(30.0, "deg"), from_unit(30.0, "deg")), (30.0, 30.0));
        assert_eq!(default_range(30.0, "deg"), (8.0, 120.0, 1.0));
        assert!(valid_unit("deg"));
    }

    #[test]
    fn model_names_count_up() {
        assert_eq!(next_model_name(&[]), "d1");
        let mut rows = vec![ParamRow::plain("width", "mm", 1.0), ParamRow::plain("d", "mm", 1.0), ParamRow::plain("dx2", "mm", 1.0)];
        assert_eq!(next_model_name(&rows), "d1");
        rows.push(ParamRow::plain("d1", "mm", 5.0));
        rows.push(ParamRow::plain("d4", "deg", 5.0));
        assert_eq!(next_model_name(&rows), "d5");
    }

    #[test]
    fn changed_compares_the_stored_rows() {
        let mut a = Document::new(10.0, 10.0);
        let b = a.clone();
        assert!(!changed(&a, &b));
        store(&mut a, &sample());
        assert!(changed(&a, &b));
        let c = a.clone();
        assert!(!changed(&a, &c));
    }
}
