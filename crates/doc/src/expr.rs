//! Expressions for Euclidean's user parameters and parametric sketches: numbers with a unit,
//! arithmetic, a few functions, and the names of other parameters.
//!
//! Lengths evaluate to millimetres (`1 in + 2 mm` is 27.4); a bare number is a plain number.
//! Errors are plain text for the Parameters panel: `no parameter "x"`, `x loops back to itself`,
//! `divide by zero`, `unexpected "…"`, `unfinished`. Nothing here panics on any input: the text
//! comes from files, the panel and agents.

use std::collections::{BTreeMap, BTreeSet};

/// The longest expression evaluated (longer ones are an error, not a stall).
pub const MAX_LEN: usize = 2000;
/// Parentheses and function calls nest at most this deep.
const MAX_DEPTH: usize = 64;
/// A parameter chain (`a` uses `b` uses `c`…) resolves at most this deep.
const MAX_CHAIN: usize = 256;

/// Millimetres in one `unit` suffix, for the units an expression may carry.
pub fn unit_mm(unit: &str) -> Option<f64> {
    Some(match unit {
        "mm" => 1.0,
        "cm" => 10.0,
        "m" => 1000.0,
        "in" => 25.4,
        "ft" => 304.8,
        "pt" => 25.4 / 72.0,
        _ => return None,
    })
}

/// The unit suffixes, longest match first (`mm` before `m`).
const UNITS: [&str; 6] = ["mm", "cm", "in", "ft", "pt", "m"];

/// The functions an expression may call (trigonometry in degrees).
pub const FUNCTIONS: [&str; 11] = ["sqrt", "abs", "round", "floor", "ceil", "min", "max", "sin", "cos", "tan", "pi"];

/// Is `name` a function or constant, so not a parameter name?
pub fn is_function(name: &str) -> bool {
    FUNCTIONS.contains(&name)
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Id(String),
    Op(char),
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}
fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    if s.len() > MAX_LEN {
        return Err("too long".into());
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let mut n: f64 = text.parse().map_err(|_| format!("bad number \"{text}\""))?;
            // An optional unit after the number (spaces allowed): `12 mm`, `1in`. A longer word
            // (`2 margin`) is a name, not a unit.
            let mut k = i;
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            let rest: String = chars[k..].iter().take(3).collect();
            if let Some(u) = UNITS.iter().find(|u| rest.starts_with(**u) && !chars.get(k + u.len()).is_some_and(|c| is_ident(*c))) {
                n *= unit_mm(u).unwrap_or(1.0);
                i = k + u.len();
            }
            out.push(Tok::Num(n));
        } else if is_ident_start(c) {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            out.push(Tok::Id(chars[start..i].iter().collect()));
        } else if "+-*/^(),".contains(c) {
            out.push(Tok::Op(c));
            i += 1;
        } else {
            return Err(format!("unexpected \"{c}\""));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<Tok>,
    p: usize,
    depth: usize,
    resolve: &'a mut dyn FnMut(&str) -> Result<f64, String>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.p)
    }
    fn peek_op(&self, ops: &[char]) -> Option<char> {
        match self.peek() {
            Some(Tok::Op(c)) if ops.contains(c) => Some(*c),
            _ => None,
        }
    }
    fn expect(&mut self, c: char) -> Result<(), String> {
        if self.peek_op(&[c]).is_some() {
            self.p += 1;
            Ok(())
        } else if self.peek().is_none() {
            Err("unfinished".into())
        } else {
            Err(format!("expected \"{c}\""))
        }
    }
    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(op) = self.peek_op(&['+', '-']) {
            self.p += 1;
            let r = self.term()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.power()?;
        while let Some(op) = self.peek_op(&['*', '/']) {
            self.p += 1;
            let r = self.power()?;
            if op == '/' {
                if r == 0.0 {
                    return Err("divide by zero".into());
                }
                v /= r;
            } else {
                v *= r;
            }
        }
        Ok(v)
    }
    fn power(&mut self) -> Result<f64, String> {
        let v = self.unary()?;
        if self.peek_op(&['^']).is_some() {
            self.p += 1;
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                return Err("too deeply nested".into());
            }
            let e = self.power()?;
            self.depth -= 1;
            return Ok(v.powf(e));
        }
        Ok(v)
    }
    fn unary(&mut self) -> Result<f64, String> {
        match self.peek_op(&['-', '+']) {
            Some('-') => {
                self.p += 1;
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err("too deeply nested".into());
                }
                let v = self.unary()?;
                self.depth -= 1;
                Ok(-v)
            }
            Some(_) => {
                self.p += 1;
                self.unary()
            }
            None => self.primary(),
        }
    }
    fn primary(&mut self) -> Result<f64, String> {
        let Some(t) = self.peek().cloned() else { return Err("unfinished".into()) };
        self.p += 1;
        match t {
            Tok::Num(n) => Ok(n),
            Tok::Op('(') => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err("too deeply nested".into());
                }
                let v = self.sum()?;
                self.expect(')')?;
                self.depth -= 1;
                Ok(v)
            }
            Tok::Op(c) => Err(format!("unexpected \"{c}\"")),
            Tok::Id(name) => {
                if name == "pi" {
                    return Ok(std::f64::consts::PI);
                }
                if self.peek_op(&['(']).is_some() && is_function(&name) {
                    self.p += 1;
                    self.depth += 1;
                    if self.depth > MAX_DEPTH {
                        return Err("too deeply nested".into());
                    }
                    let mut args = Vec::new();
                    if self.peek_op(&[')']).is_none() {
                        args.push(self.sum()?);
                        while self.peek_op(&[',']).is_some() {
                            self.p += 1;
                            args.push(self.sum()?);
                        }
                    }
                    self.expect(')')?;
                    self.depth -= 1;
                    return call(&name, &args);
                }
                if is_function(&name) {
                    return Err(format!("{name} needs parentheses"));
                }
                (self.resolve)(&name)
            }
        }
    }
}

fn call(name: &str, args: &[f64]) -> Result<f64, String> {
    let one = || args.first().copied().filter(|_| args.len() == 1).ok_or_else(|| format!("{name} takes one value"));
    let v = match name {
        "sqrt" => one()?.sqrt(),
        "abs" => one()?.abs(),
        "round" => one()?.round(),
        "floor" => one()?.floor(),
        "ceil" => one()?.ceil(),
        "sin" => one()?.to_radians().sin(),
        "cos" => one()?.to_radians().cos(),
        "tan" => one()?.to_radians().tan(),
        "min" => args.iter().copied().fold(f64::INFINITY, f64::min),
        "max" => args.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        _ => return Err(format!("no function \"{name}\"")),
    };
    if matches!(name, "min" | "max") && args.is_empty() {
        return Err(format!("{name} needs a value"));
    }
    Ok(v)
}

/// Evaluate `src`, asking `resolve` for the value of every parameter name it uses. Lengths come
/// back in millimetres.
pub fn evaluate(src: &str, resolve: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    let toks = tokenize(src)?;
    if toks.is_empty() {
        return Err("empty".into());
    }
    let mut parser = Parser { toks, p: 0, depth: 0, resolve };
    let v = parser.sum()?;
    if parser.p < parser.toks.len() {
        return Err("extra input".into());
    }
    if !v.is_finite() {
        return Err("not a number".into());
    }
    Ok(v)
}

/// The parameter names `src` uses (functions and `pi` are not parameters). Empty when `src`
/// does not tokenize.
pub fn references(src: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    tokenize(src)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| match t {
            Tok::Id(n) if !is_function(&n) && seen.insert(n.clone()) => Some(n),
            _ => None,
        })
        .collect()
}

/// `src` with every whole-word use of parameter `from` renamed `to`.
pub fn rename_references(src: &str, from: &str, to: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if is_ident_start(chars[i]) {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            // A unit after a number is not a name (`12 mm` keeps its `mm` even if a parameter
            // were called `mm`; parameter names can't be units anyway, see `params::valid_name`).
            out.push_str(if word == from { to } else { &word });
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Is `src` one number, with or without a unit (what a slider can drive)?
pub fn is_plain(src: &str) -> bool {
    let s = src.trim();
    let s = s.strip_prefix('-').unwrap_or(s).trim_start();
    let digits = s.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    let number = &s[..s.len() - digits.len()];
    if number.is_empty() || !number.chars().any(|c| c.is_ascii_digit()) || number.matches('.').count() > 1 {
        return false;
    }
    let rest = digits.trim();
    rest.is_empty() || unit_mm(rest).is_some()
}

/// Every parameter's value (millimetres, or a plain number) or its error, solved together so
/// one parameter may use another and a loop is reported, not followed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solved {
    pub values: BTreeMap<String, Result<f64, String>>,
}

impl Solved {
    /// Solve `rows` of `(name, expression)`; the first row of a repeated name counts.
    pub fn solve(rows: &[(String, String)]) -> Solved {
        let mut exprs: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, expr) in rows {
            exprs.entry(name.as_str()).or_insert(expr.as_str());
        }
        let mut values: BTreeMap<String, Result<f64, String>> = BTreeMap::new();
        let mut visiting: Vec<String> = Vec::new();
        for name in exprs.keys() {
            resolve(name, &exprs, &mut values, &mut visiting).ok();
        }
        Solved { values }
    }

    /// Evaluate `src` against the solved parameters.
    pub fn eval(&self, src: &str) -> Result<f64, String> {
        evaluate(src, &mut |name| match self.values.get(name) {
            Some(Ok(v)) => Ok(*v),
            Some(Err(e)) => Err(format!("{name}: {e}")),
            None => Err(format!("no parameter \"{name}\"")),
        })
    }

    /// The value of `name`, if it solved.
    pub fn value(&self, name: &str) -> Option<f64> {
        self.values.get(name).and_then(|r| r.as_ref().ok().copied())
    }

    /// The error of `name`, if it did not solve.
    pub fn error(&self, name: &str) -> Option<&str> {
        self.values.get(name).and_then(|r| r.as_ref().err().map(String::as_str))
    }
}

fn resolve(
    name: &str,
    exprs: &BTreeMap<&str, &str>,
    values: &mut BTreeMap<String, Result<f64, String>>,
    visiting: &mut Vec<String>,
) -> Result<f64, String> {
    if let Some(v) = values.get(name) {
        return v.clone();
    }
    let Some(expr) = exprs.get(name) else { return Err(format!("no parameter \"{name}\"")) };
    if visiting.iter().any(|v| v == name) {
        return Err(format!("{name} loops back to itself"));
    }
    if visiting.len() >= MAX_CHAIN {
        return Err("too many parameters in a chain".into());
    }
    visiting.push(name.to_string());
    let r = evaluate(expr, &mut |n| resolve(n, exprs, values, visiting).map_err(|e| if exprs.contains_key(n) { format!("{n}: {e}") } else { e }));
    visiting.pop();
    values.insert(name.to_string(), r.clone());
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(n, e)| (n.to_string(), e.to_string())).collect()
    }

    fn sample() -> Solved {
        Solved::solve(&rows(&[("width", "120 mm"), ("margin", "12 mm"), ("holes", "4")]))
    }

    #[test]
    fn the_sample_pitch_is_32() {
        assert_eq!(sample().eval("(width - 2*margin) / (holes - 1)"), Ok(32.0));
    }

    #[test]
    fn units_add_in_millimetres() {
        let v = sample().eval("1 in + 2 mm").unwrap();
        assert!((v - 27.4).abs() < 1e-9, "{v}");
        assert_eq!(sample().eval("1 cm + 1 m + 1 ft").unwrap(), 10.0 + 1000.0 + 304.8);
        assert!((sample().eval("72 pt").unwrap() - 25.4).abs() < 1e-9);
    }

    #[test]
    fn functions_and_constants() {
        assert_eq!(sample().eval("max(3, sqrt(16)) * 2"), Ok(8.0));
        assert_eq!(sample().eval("min(3, 1, 2)"), Ok(1.0));
        assert_eq!(sample().eval("round(2.5) + floor(1.9) + ceil(1.1) + abs(-1)"), Ok(3.0 + 1.0 + 2.0 + 1.0));
        assert!((sample().eval("sin(90) + cos(0) + tan(45)").unwrap() - 3.0).abs() < 1e-9);
        assert!((sample().eval("2 * pi").unwrap() - std::f64::consts::TAU).abs() < 1e-12);
        assert_eq!(sample().eval("2 ^ 3 ^ 2"), Ok(512.0));
        assert_eq!(sample().eval("-2 ^ 2"), Ok(4.0));
        assert_eq!(sample().eval("sqrt(1, 2)"), Err("sqrt takes one value".into()));
    }

    #[test]
    fn errors_are_plain_text() {
        assert_eq!(sample().eval("width +"), Err("unfinished".into()));
        assert_eq!(sample().eval("depth * 2"), Err("no parameter \"depth\"".into()));
        assert_eq!(sample().eval("1 / 0"), Err("divide by zero".into()));
        assert_eq!(sample().eval("1 # 2"), Err("unexpected \"#\"".into()));
        assert_eq!(sample().eval(""), Err("empty".into()));
        assert_eq!(sample().eval("(1 + 2"), Err("unfinished".into()));
        assert_eq!(sample().eval("1 2"), Err("extra input".into()));
        assert_eq!(sample().eval("1.2.3"), Err("bad number \"1.2.3\"".into()));
        assert_eq!(sample().eval("10 ^ 400"), Err("not a number".into()));
        assert_eq!(sample().eval(&"(".repeat(100)), Err("too deeply nested".into()));
        assert_eq!(sample().eval(&"1".repeat(MAX_LEN + 1)), Err("too long".into()));
    }

    #[test]
    fn a_loop_is_reported_not_followed() {
        let s = Solved::solve(&rows(&[("a", "b"), ("b", "a"), ("c", "1")]));
        assert!(s.error("a").unwrap().contains("loops back to itself"), "{:?}", s.values);
        assert!(s.error("b").unwrap().contains("loops back to itself"), "{:?}", s.values);
        assert_eq!(s.value("c"), Some(1.0));
        let s = Solved::solve(&rows(&[("a", "a + 1")]));
        assert_eq!(s.error("a"), Some("a: a loops back to itself"));
    }

    #[test]
    fn a_no_unit_parameter_is_a_plain_number() {
        let s = Solved::solve(&rows(&[("holes", "4"), ("spacing", "10 mm / holes")]));
        assert_eq!(s.value("holes"), Some(4.0));
        assert_eq!(s.value("spacing"), Some(2.5));
    }

    #[test]
    fn a_parameter_error_names_the_parameter() {
        let s = Solved::solve(&rows(&[("a", "b * 2"), ("b", "1 +")]));
        assert_eq!(s.error("b"), Some("unfinished"));
        assert_eq!(s.error("a"), Some("b: unfinished"));
        assert_eq!(s.eval("a"), Err("a: b: unfinished".into()));
    }

    #[test]
    fn unit_words_are_not_names() {
        // `2 margin` is a number times nothing: `margin` is a name, not the `m` unit.
        assert_eq!(sample().eval("2 margin"), Err("extra input".into()));
        assert_eq!(sample().eval("2*margin"), Ok(24.0));
        assert_eq!(references("(width - 2*margin) / (holes - 1) + pi + sqrt(x)"), vec!["width", "margin", "holes", "x"]);
        assert_eq!(references("1 +"), vec![] as Vec<String>);
    }

    #[test]
    fn renaming_changes_whole_words_only() {
        assert_eq!(rename_references("width / 2 + width_2 + awidth + 3 mm", "width", "plate_w"), "plate_w / 2 + width_2 + awidth + 3 mm");
        assert_eq!(rename_references("m*2", "m", "n"), "n*2");
    }

    #[test]
    fn plain_numbers_are_recognised() {
        for s in ["120 mm", "4", "-0.5 in", " 12mm ", ".5", "3 ft"] {
            assert!(is_plain(s), "{s}");
        }
        for s in ["width / 2", "1 + 2", "", "mm", "1.2.3", "2 margin", "-"] {
            assert!(!is_plain(s), "{s}");
        }
    }
}
