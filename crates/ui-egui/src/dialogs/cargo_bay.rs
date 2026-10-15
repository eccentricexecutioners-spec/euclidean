//! Cargo Bay: the host trait the desktop app implements to keep Euclidean up to date, the
//! "Link to Cargo Bay" dialog, and the per-frame glue between them. This crate needs no network or
//! file code: the app injects an [`UpdateHost`] through [`crate::Services::updates`] and the UI only
//! reads its [`UpdateView`] and asks it to link, check or restart.
//!
//! Fields of the dialog: `code` (what is typed, shown as `XXXX-XXXX`).

use serde_json::{Value, json};

use super::DialogSpec;
use crate::state::Dialog;
use crate::theme::Tokens;
use crate::{VectorcraftApp, widgets};

/// The dialog kind of Link to Cargo Bay.
pub const KIND: &str = "cargo_bay.link";

/// The status line when the updater is off.
const OFF: &str = "Updates are off in this copy.";
/// The status line once a link has been made.
const LINKED: &str = "Linked. Euclidean will update itself.";

/// What the desktop app does for the updater. Implemented in `apps/vectorcraft`.
pub trait UpdateHost {
    /// A cheap snapshot, called every frame.
    fn view(&self) -> UpdateView;
    /// Start a background claim with a link code; the result shows in [`Self::view`].
    fn link(&mut self, code: &str);
    /// Start a background check (and stage the update on a Mac).
    fn check(&mut self);
    /// Remember to relaunch once this process quits.
    fn restart(&mut self);
}

/// What the UI shows of the updater.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateView {
    pub linked: bool,
    /// A link or a check is running.
    pub busy: bool,
    /// The latest English message (already filled), shown once in the status line.
    pub message: Option<String>,
    /// Bumps each time `message` is set, so the app shows each one once.
    pub message_seq: u64,
    /// A staged version waiting for Restart to update.
    pub ready: Option<String>,
    /// The last link attempt's English error, for the dialog.
    pub link_error: Option<String>,
    /// The version running now.
    pub installed: String,
    /// The new version, once a check has heard of one.
    pub offered: Option<String>,
    /// Release notes of `offered`, if any.
    pub notes: Option<String>,
    /// Bytes downloaded so far.
    pub received: u64,
    /// The download's size (the manifest `size`); 0 before an offer is heard.
    pub total: u64,
    /// The download is done and is being checked ("Checking the download…").
    pub verifying: bool,
    /// The last check found nothing newer.
    pub up_to_date: bool,
    /// The English line for the last check's error.
    pub error: Option<String>,
    /// A newer version this platform cannot install by itself (Windows/Linux).
    pub elsewhere: Option<String>,
}

pub(super) const SPEC: DialogSpec = DialogSpec {
    heading: |_| tl!("Link to Cargo Bay").into(),
    body,
    confirm,
    ok: Some("Link"),
    cancel: Some("Later"),
    min_width: 360.0,
    max_width: Some(420.0),
    ..DialogSpec::FORM
};

/// The view of the host, or the default view with no host.
fn view(app: &VectorcraftApp) -> Option<UpdateView> {
    app.services.updates.as_ref().map(|h| h.view())
}

/// Help → Link to Cargo Bay…: open the dialog, or say updates are off.
pub fn open_link(app: &mut VectorcraftApp) -> Value {
    if app.services.updates.is_none() {
        app.status(OFF);
        return json!({"started": false});
    }
    app.ui.dialog = Some(Dialog::new(KIND, json!({"code": ""})));
    json!({"started": true})
}

/// Help → Check for Updates…: start a check, or say updates are off.
pub fn check(app: &mut VectorcraftApp) -> Value {
    let Some(host) = app.services.updates.as_mut() else {
        app.status(OFF);
        return json!({"started": false});
    };
    // A version already staged shows its Restart now; asking again would only flash "Checking".
    if !host.view().ready.is_some_and(|_| !host.view().busy) {
        host.check();
    }
    app.updates_window = true;
    json!({"started": true})
}

/// The Restart to update button: the host remembers to relaunch, then the app quits as Quit does
/// (unsaved documents are asked about first).
pub(crate) fn restart(app: &mut VectorcraftApp) {
    if let Some(host) = app.services.updates.as_mut() {
        host.restart();
    }
    if let Err(e) = app.run("app.quit", json!({})) {
        app.status(e);
    }
}

/// Once a frame: show the host's new messages, close the dialog once linked, and the first time
/// the app runs unlinked, offer the link dialog.
pub(crate) fn frame(app: &mut VectorcraftApp) {
    let Some(view) = view(app) else { return };
    if view.message_seq > app.updates_seen {
        app.updates_seen = view.message_seq;
        // The staged message ("{new} ready — Restart to update") repeats the status line's own
        // "Euclidean {new} ready" and button, so it is not pushed there.
        let staged = |m: &str| view.ready.as_deref().is_some_and(|r| m.starts_with(r) && m.ends_with("Restart to update"));
        if let Some(m) = view.message.clone().filter(|m| !staged(m)) {
            app.status(m);
        }
    }
    let open = app.ui.dialog.as_ref().is_some_and(|d| d.kind == KIND);
    if open && view.linked {
        app.ui.dialog = None;
        app.status(LINKED);
    }
    if !app.updates_offered && app.ui.dialog.is_none() && app.frame >= 2 {
        app.updates_offered = true;
        if !view.linked && !app.updates_later {
            open_link(app);
        }
    }
}

/// What is typed, as `XXXX-XXXX`: letters and digits only, upper case, at most eight, dashed after
/// the fourth. For display; the crate validates the code.
fn shown(typed: &str) -> String {
    let clean: String = typed.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).take(8).collect();
    if clean.len() > 4 { format!("{}-{}", &clean[..4], &clean[4..]) } else { clean }
}

fn body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    let t = Tokens::get(ui.ctx());
    let view = view(app).unwrap_or_default();
    ui.label(egui::RichText::new(tl!("Type the link code from your Tools page on the Mothership so Euclidean can update itself.")).color(t.text_dim));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Link code")).color(t.text));
        let id = ui.id().with("cargo-bay-code");
        widgets::take_dialog_focus(ui, id, &d.str("code"));
        let mut s = d.str("code");
        egui::Frame::NONE
            .fill(t.input)
            .stroke(egui::Stroke::new(1.0, t.input_border))
            .corner_radius(egui::CornerRadius::same(3))
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(ui, |ui| {
                crate::theme::strong_selected_text(ui);
                let edit = egui::TextEdit::singleline(&mut s)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .desired_width(110.0)
                    .hint_text("XXXX-XXXX")
                    .font(crate::theme::mono(12.5));
                ui.add_enabled(!view.busy, edit);
            });
        let normal = shown(&s);
        if normal != d.str("code") {
            d.fields.insert("code".into(), Value::String(normal));
        }
    });
    if view.busy {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tl!("Linking…")).color(t.text_dim));
    } else if let Some(e) = &view.link_error {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(crate::i18n::msg(e)).color(t.error));
    }
    false
}

/// Link: hand the code to the host and keep the dialog open until it says it is linked.
fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let code = d.str("code");
    let busy = view(app).is_some_and(|v| v.busy);
    if let Some(host) = app.services.updates.as_mut()
        && !busy
        && !code.trim().is_empty()
    {
        host.link(&code);
    }
    Ok(Value::Null)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use egui::epaint::Shape;
    use serde_json::json;

    use super::*;

    #[derive(Default)]
    pub(crate) struct Calls {
        pub(crate) links: Vec<String>,
        pub(crate) checks: usize,
        pub(crate) restarts: usize,
    }

    #[derive(Clone, Default)]
    pub(crate) struct FakeHost {
        pub(crate) view: Rc<RefCell<UpdateView>>,
        pub(crate) calls: Rc<RefCell<Calls>>,
    }

    impl UpdateHost for FakeHost {
        fn view(&self) -> UpdateView {
            self.view.borrow().clone()
        }
        fn link(&mut self, code: &str) {
            self.calls.borrow_mut().links.push(code.to_string());
        }
        fn check(&mut self) {
            self.calls.borrow_mut().checks += 1;
        }
        fn restart(&mut self) {
            self.calls.borrow_mut().restarts += 1;
        }
    }

    pub(crate) struct Rig {
        pub(crate) app: VectorcraftApp,
        pub(crate) ctx: egui::Context,
        pub(crate) host: FakeHost,
    }

    impl Rig {
        pub(crate) fn new(on: bool) -> Rig {
            let host = FakeHost::default();
            let services = crate::Services { updates: on.then(|| Box::new(host.clone()) as Box<dyn UpdateHost>), ..Default::default() };
            let app = VectorcraftApp::new(vectorcraft_engine::Session::new(), services);
            let ctx = egui::Context::default();
            crate::theme::install_fonts(&ctx);
            Rig { app, ctx, host }
        }

        pub(crate) fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(egui::RawInput { events, screen_rect: Some(screen), ..Default::default() }, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            out.textures_delta.clear();
            out
        }

        pub(crate) fn settle(&mut self) -> String {
            let mut text = String::new();
            for _ in 0..5 {
                text = crate::tests_labels::shapes_text(&self.frame(vec![]));
            }
            text
        }

        /// Where `label` is painted, in the last frame.
        pub(crate) fn find(&mut self, label: &str) -> egui::Pos2 {
            fn walk(s: &Shape, label: &str, out: &mut Option<egui::Pos2>) {
                match s {
                    Shape::Text(t) if t.galley.text() == label => *out = Some(t.pos + t.galley.rect.center().to_vec2()),
                    Shape::Vec(v) => v.iter().for_each(|s| walk(s, label, out)),
                    _ => {}
                }
            }
            let out = self.frame(vec![]);
            let mut at = None;
            out.shapes.iter().for_each(|c| walk(&c.shape, label, &mut at));
            at.unwrap_or_else(|| panic!("{label} is not on screen"))
        }

        pub(crate) fn click(&mut self, label: &str) {
            let pos = self.find(label);
            let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            self.frame(vec![egui::Event::PointerMoved(pos)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.settle();
        }

        pub(crate) fn dialog_kind(&self) -> Option<&str> {
            self.app.ui.dialog.as_ref().map(|d| d.kind.as_str())
        }
    }

    #[test]
    fn link_dialog_shows_when_unlinked_and_updater_on() {
        let mut rig = Rig::new(true);
        let text = rig.settle();
        assert_eq!(rig.dialog_kind(), Some(KIND));
        for want in ["Link to Cargo Bay", "Link code", "Link", "Later"] {
            assert!(text.lines().any(|l| l == want), "{want} missing in {text}");
        }
        assert!(text.contains("Type the link code from your Tools page on the Mothership"), "{text}");
        // Linked already: nothing to offer.
        let mut rig = Rig::new(true);
        rig.host.view.borrow_mut().linked = true;
        rig.settle();
        assert_eq!(rig.dialog_kind(), None);
    }

    #[test]
    fn later_closes_link_dialog_until_next_start() {
        let mut rig = Rig::new(true);
        rig.settle();
        assert_eq!(rig.dialog_kind(), Some(KIND));
        rig.click("Later");
        assert_eq!(rig.dialog_kind(), None);
        assert!(rig.app.updates_later);
        rig.settle();
        assert_eq!(rig.dialog_kind(), None, "not offered again this run");
        assert!(rig.host.calls.borrow().links.is_empty());
        // The Help command still opens it.
        rig.app.run("help.linkCargoBay", json!({})).unwrap();
        assert_eq!(rig.dialog_kind(), Some(KIND));
    }

    #[test]
    fn no_updater_shows_nothing_and_calls_nothing() {
        let mut rig = Rig::new(false);
        let bystander = FakeHost::default();
        let text = rig.settle();
        assert_eq!(rig.dialog_kind(), None);
        assert!(!text.contains("Restart to update") && !text.contains("Link to Cargo Bay"), "{text}");
        assert_eq!(rig.app.run("help.checkForUpdates", json!({})).unwrap(), json!({"started": false}));
        assert_eq!(rig.app.ui.status, "Updates are off in this copy.");
        rig.app.ui.status.clear();
        assert_eq!(rig.app.run("help.linkCargoBay", json!({})).unwrap(), json!({"started": false}));
        assert_eq!(rig.app.ui.status, "Updates are off in this copy.");
        rig.settle();
        assert_eq!(rig.dialog_kind(), None);
        let c = bystander.calls.borrow();
        assert!(c.links.is_empty() && c.checks == 0 && c.restarts == 0);
    }

    #[test]
    fn status_line_shows_ready_and_restart_to_update() {
        let mut rig = Rig::new(true);
        rig.host.view.borrow_mut().linked = true;
        let text = rig.settle();
        assert!(!text.contains("Restart to update"), "{text}");
        rig.host.view.borrow_mut().ready = Some("0.1.1".into());
        let text = rig.settle();
        assert!(text.lines().any(|l| l == "Euclidean 0.1.1 ready"), "{text}");
        assert!(text.lines().any(|l| l == "Restart to update"), "{text}");
    }

    #[test]
    fn restart_button_asks_host_then_quits() {
        let mut rig = Rig::new(true);
        {
            let mut v = rig.host.view.borrow_mut();
            v.linked = true;
            v.ready = Some("0.1.1".into());
        }
        rig.settle();
        rig.click("Restart to update");
        assert_eq!(rig.host.calls.borrow().restarts, 1);
        assert_eq!(rig.app.ui.status, "quit");
    }

    #[test]
    fn check_for_updates_command_calls_host_once() {
        let mut rig = Rig::new(true);
        rig.host.view.borrow_mut().linked = true;
        rig.settle();
        assert_eq!(rig.app.run("help.checkForUpdates", json!({})).unwrap(), json!({"started": true}));
        assert_eq!(rig.host.calls.borrow().checks, 1);
        assert!(rig.app.updates_window);
        rig.settle();
        assert_eq!(rig.host.calls.borrow().checks, 1, "a frame never checks by itself");
    }

    #[test]
    fn linking_closes_dialog_and_says_linked() {
        let mut rig = Rig::new(true);
        rig.settle();
        assert_eq!(rig.dialog_kind(), Some(KIND));
        rig.app.ui.dialog.as_mut().unwrap().fields.insert("code".into(), json!("abcdefgh"));
        crate::dialogs::confirm(&mut rig.app).unwrap();
        assert_eq!(rig.host.calls.borrow().links, ["abcdefgh"]);
        rig.settle();
        assert_eq!(rig.dialog_kind(), Some(KIND), "stays open until the host says linked");
        {
            let mut v = rig.host.view.borrow_mut();
            v.linked = true;
            v.message = Some("Linked. Euclidean will update itself.".into());
            v.message_seq = 1;
        }
        rig.settle();
        assert_eq!(rig.dialog_kind(), None);
        assert_eq!(rig.app.ui.status, "Linked. Euclidean will update itself.");
    }

    #[test]
    fn link_error_shows_in_the_dialog_and_messages_show_once() {
        let mut rig = Rig::new(true);
        rig.settle();
        {
            let mut v = rig.host.view.borrow_mut();
            v.link_error = Some("Too many tries — wait a few minutes, then try again.".into());
            v.message = v.link_error.clone();
            v.message_seq = 1;
        }
        let text = rig.settle();
        assert!(text.lines().any(|l| l.starts_with("Too many tries")), "{text}");
        rig.app.ui.status.clear();
        rig.settle();
        assert_eq!(rig.app.ui.status, "", "the same message is not shown twice");
    }

    #[test]
    fn typed_code_is_shown_dashed_and_upper_case() {
        assert_eq!(shown("ab"), "AB");
        assert_eq!(shown("abcd"), "ABCD");
        assert_eq!(shown("abcd-ef"), "ABCD-EF");
        assert_eq!(shown(" ab cd efgh12 "), "ABCD-EFGH");
    }
}
