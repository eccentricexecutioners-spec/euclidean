//! The heads-up input boxes of sketch mode (Fusion's): a few small value boxes beside the thing
//! being drawn. Typing goes straight into the focused box; Tab locks it and moves on; Enter
//! commits. Egui-free: the overlay paints the boxes and the key hook feeds the events.

use vectorcraft_doc::expr::Solved;

/// What a box measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxUnit {
    /// Millimetres (a plain number is mm).
    Mm,
    /// Degrees (a plain number is degrees).
    Deg,
}

/// One box.
#[derive(Clone, Debug, PartialEq)]
pub struct HudBox {
    /// `length`, `angle`, `width`, `height`, `diameter`, `radius`…
    pub label: &'static str,
    /// What the box shows: the mouse's value, or what was typed.
    pub text: String,
    /// Tab locked it: it keeps its value while the mouse moves.
    pub locked: bool,
    pub unit: BoxUnit,
    /// The user typed (or kept) this text, as opposed to the mouse supplying it.
    pub typed: bool,
}

impl HudBox {
    /// The typed text, when there is some.
    fn typed_text(&self) -> Option<&str> {
        (self.typed && !self.text.trim().is_empty()).then_some(self.text.as_str())
    }
}

/// A key the boxes understand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudEvent {
    Char(char),
    Backspace,
    Tab,
    Enter,
    Escape,
}

/// What an event did.
#[derive(Clone, Debug, PartialEq)]
pub enum HudOutcome {
    None,
    /// Enter: per box, `Some(typed text)` when it was typed or locked.
    Commit(Vec<Option<String>>),
    Cancel,
    /// Enter was refused: this typed text does not evaluate.
    Blocked(String),
}

/// The boxes and which one has the keyboard.
#[derive(Clone, Debug, PartialEq)]
pub struct Hud {
    pub boxes: Vec<HudBox>,
    pub focus: usize,
    /// Show the boxes' errors (after Tab or a refused Enter, not while the text is still being typed).
    pub show_errors: bool,
}

impl Hud {
    pub fn new(labels: &[(&'static str, BoxUnit)]) -> Hud {
        Hud {
            boxes: labels.iter().map(|(label, unit)| HudBox { label, text: String::new(), locked: false, unit: *unit, typed: false }).collect(),
            focus: 0,
            show_errors: false,
        }
    }

    /// Has the user typed into any box?
    pub fn any_typed(&self) -> bool {
        self.boxes.iter().any(|b| b.typed_text().is_some())
    }

    /// Unlocked boxes with no typed text show the mouse's value.
    pub fn follow_mouse(&mut self, values: &[f64]) {
        for (b, v) in self.boxes.iter_mut().zip(values) {
            if !b.typed && !b.locked {
                b.text = format_number(*v);
            }
        }
    }

    /// Box `i`'s typed or locked text, evaluated (mm or degrees); `None` when the mouse supplies it.
    pub fn value(&self, i: usize, solved: &Solved) -> Option<Result<f64, String>> {
        self.boxes.get(i)?.typed_text().map(|t| solved.eval(t))
    }

    /// Box `i`'s typed text does not evaluate: why.
    pub fn error(&self, i: usize, solved: &Solved) -> Option<String> {
        self.value(i, solved)?.err()
    }

    pub fn handle(&mut self, ev: HudEvent, solved: &Solved) -> HudOutcome {
        if self.boxes.is_empty() {
            return if ev == HudEvent::Escape { HudOutcome::Cancel } else { HudOutcome::None };
        }
        self.focus = self.focus.min(self.boxes.len() - 1);
        match ev {
            HudEvent::Char(c) => {
                if c.is_control() {
                    return HudOutcome::None;
                }
                let b = &mut self.boxes[self.focus];
                if !b.typed {
                    // The first typed character replaces the value the mouse showed.
                    b.text.clear();
                    b.typed = true;
                }
                b.locked = false;
                b.text.push(c);
                self.show_errors = false;
                HudOutcome::None
            }
            HudEvent::Backspace => {
                let b = &mut self.boxes[self.focus];
                if b.typed {
                    b.text.pop();
                    if b.text.is_empty() {
                        b.typed = false;
                        b.locked = false;
                    }
                } else {
                    // The shown value is selected as a whole: Backspace clears it.
                    b.text.clear();
                    b.locked = false;
                }
                self.show_errors = false;
                HudOutcome::None
            }
            HudEvent::Tab => {
                let ok = self.value(self.focus, solved).is_some_and(|r| r.is_ok());
                let b = &mut self.boxes[self.focus];
                if ok {
                    b.locked = true;
                } else if b.typed_text().is_some() {
                    self.show_errors = true;
                }
                self.focus = (self.focus + 1) % self.boxes.len();
                HudOutcome::None
            }
            HudEvent::Enter => {
                for i in 0..self.boxes.len() {
                    if let Some(e) = self.error(i, solved) {
                        self.show_errors = true;
                        self.focus = i;
                        return HudOutcome::Blocked(e);
                    }
                }
                HudOutcome::Commit(self.boxes.iter().map(|b| b.typed_text().map(str::to_string)).collect())
            }
            HudEvent::Escape => HudOutcome::Cancel,
        }
    }
}

/// A number as the boxes show it: two decimals (`42.35`).
pub fn format_number(v: f64) -> String {
    if v.is_finite() { format!("{v:.2}") } else { "0.00".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_hud() -> Hud {
        Hud::new(&[("length", BoxUnit::Mm), ("angle", BoxUnit::Deg)])
    }

    fn type_text(h: &mut Hud, s: &str, solved: &Solved) {
        for c in s.chars() {
            h.handle(HudEvent::Char(c), solved);
        }
    }

    #[test]
    fn typing_50_sets_the_length() {
        let solved = Solved::default();
        let mut h = line_hud();
        h.follow_mouse(&[42.35, 10.0]);
        assert_eq!(h.boxes[0].text, "42.35");
        type_text(&mut h, "50", &solved);
        assert_eq!(h.boxes[0].text, "50", "the first character replaced the mouse's value");
        assert_eq!(h.value(0, &solved), Some(Ok(50.0)));
        assert_eq!(h.value(1, &solved), None, "the angle is still the mouse's");
    }

    #[test]
    fn tab_moves_to_the_angle_box_and_locks_the_length() {
        let solved = Solved::default();
        let mut h = line_hud();
        type_text(&mut h, "50", &solved);
        assert_eq!(h.handle(HudEvent::Tab, &solved), HudOutcome::None);
        assert!(h.boxes[0].locked);
        assert_eq!(h.focus, 1);
        h.handle(HudEvent::Tab, &solved);
        assert_eq!(h.focus, 0, "Tab cycles");
        assert!(!h.boxes[1].locked, "an untouched box is not locked");
    }

    #[test]
    fn enter_commits_length_50_at_30_degrees() {
        let solved = Solved::default();
        let mut h = line_hud();
        type_text(&mut h, "50", &solved);
        h.handle(HudEvent::Tab, &solved);
        type_text(&mut h, "30", &solved);
        assert_eq!(h.handle(HudEvent::Enter, &solved), HudOutcome::Commit(vec![Some("50".into()), Some("30".into())]));
        // Nothing typed: the mouse supplies both.
        let mut h = line_hud();
        assert_eq!(h.handle(HudEvent::Enter, &solved), HudOutcome::Commit(vec![None, None]));
    }

    #[test]
    fn a_locked_box_survives_mouse_moves() {
        let solved = Solved::default();
        let mut h = line_hud();
        type_text(&mut h, "50", &solved);
        h.handle(HudEvent::Tab, &solved);
        h.follow_mouse(&[12.0, 45.0]);
        assert_eq!(h.boxes[0].text, "50");
        assert_eq!(h.boxes[1].text, "45.00");
    }

    #[test]
    fn escape_cancels_without_a_commit() {
        let solved = Solved::default();
        let mut h = line_hud();
        type_text(&mut h, "5", &solved);
        assert_eq!(h.handle(HudEvent::Escape, &solved), HudOutcome::Cancel);
    }

    #[test]
    fn width_over_2_evaluates_against_the_parameters() {
        let solved = Solved::solve(&[("width".to_string(), "100 mm".to_string())]);
        let mut h = line_hud();
        type_text(&mut h, "width / 2", &solved);
        assert_eq!(h.value(0, &solved), Some(Ok(50.0)));
        assert_eq!(h.error(0, &solved), None);
        h.handle(HudEvent::Tab, &solved);
        type_text(&mut h, "90", &solved);
        assert_eq!(h.value(1, &solved), Some(Ok(90.0)), "a plain number in a degrees box is degrees");
    }

    #[test]
    fn a_bad_expression_shows_its_error_and_blocks_enter() {
        let solved = Solved::default();
        let mut h = line_hud();
        type_text(&mut h, "nope", &solved);
        assert!(h.error(0, &solved).is_some());
        match h.handle(HudEvent::Enter, &solved) {
            HudOutcome::Blocked(e) => assert!(!e.is_empty()),
            other => panic!("{other:?}"),
        }
        assert!(h.show_errors);
        // Fixing it lets Enter through.
        for _ in 0..4 {
            h.handle(HudEvent::Backspace, &solved);
        }
        type_text(&mut h, "7", &solved);
        assert!(matches!(h.handle(HudEvent::Enter, &solved), HudOutcome::Commit(_)));
    }

    #[test]
    fn one_box_tools_commit_on_enter() {
        let solved = Solved::default();
        for label in ["diameter", "radius"] {
            let mut h = Hud::new(&[(label, BoxUnit::Mm)]);
            h.follow_mouse(&[12.5]);
            assert_eq!(h.handle(HudEvent::Enter, &solved), HudOutcome::Commit(vec![None]), "the mouse supplies it");
            type_text(&mut h, "20", &solved);
            assert_eq!(h.handle(HudEvent::Tab, &solved), HudOutcome::None);
            assert_eq!(h.focus, 0, "Tab cycles back to the only box");
            assert_eq!(h.handle(HudEvent::Enter, &solved), HudOutcome::Commit(vec![Some("20".into())]));
        }
    }
}
