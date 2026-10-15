//! The "Euclidean Updates" window: what Help → Check for Updates… shows while it checks, downloads
//! and gets an update ready. A non-modal window with one area that shows exactly one state, derived
//! from the host's [`UpdateView`] every frame ([`state`]). It is open while
//! `VectorcraftApp::updates_window` is set; the quiet check at start never opens it.

use super::cargo_bay::{self, UpdateView};
use crate::VectorcraftApp;
use crate::theme::Tokens;

/// What the window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateState {
    /// Not linked to Cargo Bay: a Link… button.
    NotLinked,
    /// A staged version waits for a restart: Restart now, Later.
    Ready,
    /// Asking Cargo Bay (or about to).
    Checking,
    /// The download is being checked.
    Verifying,
    /// A newer version is being downloaded (a Mac): notes and a progress bar.
    Downloading,
    /// A newer version this platform cannot install by itself (Windows/Linux).
    Elsewhere,
    /// The last check failed: Try again.
    Error,
    /// Nothing newer.
    UpToDate,
}

/// The state for a view. `mac` is whether this platform downloads updates itself (otherwise an offer
/// heard during a check is only an offer).
pub(crate) fn state(v: &UpdateView, mac: bool) -> UpdateState {
    if !v.linked {
        UpdateState::NotLinked
    } else if v.ready.is_some() && !v.busy {
        UpdateState::Ready
    } else if v.busy && (v.offered.is_none() || !mac) {
        UpdateState::Checking
    } else if v.busy && v.verifying {
        UpdateState::Verifying
    } else if v.busy {
        UpdateState::Downloading
    } else if v.elsewhere.is_some() {
        UpdateState::Elsewhere
    } else if v.error.is_some() {
        UpdateState::Error
    } else if v.up_to_date {
        UpdateState::UpToDate
    } else {
        UpdateState::Checking
    }
}

/// Megabytes (1,000,000 bytes, one decimal) for the progress line.
fn megabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_000_000.0)
}

/// What a click in the window asks for.
enum Act {
    Link,
    Restart,
    Retry,
    Close,
}

/// Draw the window while `updates_window` is set.
pub(super) fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    if !app.updates_window {
        return;
    }
    let Some(v) = app.services.updates.as_ref().map(|h| h.view()) else {
        app.updates_window = false;
        return;
    };
    // Linked from this window: check straight away.
    let was_unlinked = egui::Id::new("updates-was-unlinked");
    let before = ctx.data(|d| d.get_temp::<bool>(was_unlinked)).unwrap_or(false);
    ctx.data_mut(|d| d.insert_temp(was_unlinked, !v.linked));
    if before && v.linked && !v.busy && v.ready.is_none() {
        if let Some(host) = app.services.updates.as_mut() {
            host.check();
        }
        return ctx.request_repaint();
    }
    if v.busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(80));
    }
    let st = state(&v, cfg!(target_os = "macos"));
    let t = Tokens::get(ctx);
    let new = v.offered.clone().or_else(|| v.elsewhere.clone()).unwrap_or_default();
    let mut open = true;
    let mut act = None;
    egui::Window::new(tl!("Euclidean Updates"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16)))
        .show(ctx, |ui| {
            ui.set_width(380.0);
            let busy_line = |ui: &mut egui::Ui, text: String| {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(t.accent));
                    ui.label(egui::RichText::new(text).color(t.text));
                });
            };
            let offer = |ui: &mut egui::Ui| {
                let head = crate::i18n::fmt(tl!("Euclidean {new} is available (you have {old})."), &[("new", &new), ("old", &v.installed)]);
                ui.label(egui::RichText::new(head).strong().color(t.text));
                if let Some(notes) = v.notes.as_deref().filter(|n| !n.trim().is_empty()) {
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(120.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.label(egui::RichText::new(notes).size(11.5).color(t.text_dim));
                    });
                }
                ui.add_space(8.0);
            };
            match st {
                UpdateState::NotLinked => {
                    ui.label(egui::RichText::new(crate::i18n::msg("Link Euclidean to Cargo Bay to get updates.")).color(t.text));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button(tl!("Link…")).clicked() {
                            act = Some(Act::Link);
                        }
                        if ui.button(tl!("Close")).clicked() {
                            act = Some(Act::Close);
                        }
                    });
                }
                UpdateState::Ready => {
                    let text = crate::i18n::fmt(tl!("Euclidean {new} is ready."), &[("new", v.ready.as_deref().unwrap_or_default())]);
                    ui.label(egui::RichText::new(text).color(t.text));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button(tl!("Restart now")).clicked() {
                            act = Some(Act::Restart);
                        }
                        if ui.button(tl!("Later")).clicked() {
                            act = Some(Act::Close);
                        }
                    });
                }
                UpdateState::Checking => busy_line(ui, crate::i18n::msg("Checking for updates…")),
                UpdateState::Verifying => busy_line(ui, tl!("Checking the download…").to_string()),
                UpdateState::Downloading => {
                    offer(ui);
                    let frac = if v.total == 0 { 0.0 } else { (v.received as f32 / v.total as f32).clamp(0.0, 1.0) };
                    ui.add(egui::ProgressBar::new(frac).desired_width(380.0));
                    let text =
                        crate::i18n::fmt(tl!("{received} of {total} MB"), &[("received", &megabytes(v.received)), ("total", &megabytes(v.total))]);
                    ui.label(egui::RichText::new(text).size(11.0).color(t.text_dim));
                }
                UpdateState::Elsewhere => {
                    offer(ui);
                    let line = crate::i18n::msg("{new} is available — download it from the Mothership's Tools page".replace("{new}", &new).as_str());
                    ui.label(egui::RichText::new(line).color(t.text));
                    ui.add_space(10.0);
                    if ui.button(tl!("Close")).clicked() {
                        act = Some(Act::Close);
                    }
                }
                UpdateState::Error => {
                    ui.label(egui::RichText::new(crate::i18n::msg(v.error.as_deref().unwrap_or_default())).color(t.error));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button(tl!("Try again")).clicked() {
                            act = Some(Act::Retry);
                        }
                        if ui.button(tl!("Close")).clicked() {
                            act = Some(Act::Close);
                        }
                    });
                }
                UpdateState::UpToDate => {
                    let text = crate::i18n::fmt(tl!("Euclidean {version} is up to date."), &[("version", &v.installed)]);
                    ui.label(egui::RichText::new(text).color(t.text));
                    ui.add_space(10.0);
                    if ui.button(tl!("Close")).clicked() {
                        act = Some(Act::Close);
                    }
                }
            }
        });
    app.updates_window = open;
    match act {
        Some(Act::Link) => {
            cargo_bay::open_link(app);
        }
        Some(Act::Restart) => {
            app.updates_window = false;
            cargo_bay::restart(app);
        }
        Some(Act::Retry) => {
            if let Some(host) = app.services.updates.as_mut() {
                host.check();
            }
        }
        Some(Act::Close) => app.updates_window = false,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::dialogs::cargo_bay::tests::Rig;

    fn linked() -> UpdateView {
        UpdateView { linked: true, installed: "0.1.1".into(), ..Default::default() }
    }

    #[test]
    fn state_follows_the_view() {
        let v = linked();
        assert_eq!(state(&UpdateView::default(), true), UpdateState::NotLinked);
        assert_eq!(state(&v, true), UpdateState::Checking, "idle: a check is about to start");
        assert_eq!(state(&UpdateView { busy: true, ..v.clone() }, true), UpdateState::Checking);
        let offered = UpdateView { busy: true, offered: Some("0.1.2".into()), total: 10, ..v.clone() };
        assert_eq!(state(&offered, true), UpdateState::Downloading);
        assert_eq!(state(&offered, false), UpdateState::Checking, "no download off a Mac");
        assert_eq!(state(&UpdateView { verifying: true, ..offered.clone() }, true), UpdateState::Verifying);
        let ready = UpdateView { ready: Some("0.1.2".into()), ..v.clone() };
        assert_eq!(state(&ready, true), UpdateState::Ready);
        assert_eq!(state(&UpdateView { busy: true, ..ready }, true), UpdateState::Checking);
        assert_eq!(state(&UpdateView { elsewhere: Some("0.1.2".into()), ..v.clone() }, false), UpdateState::Elsewhere);
        assert_eq!(state(&UpdateView { error: Some("x".into()), ..v.clone() }, true), UpdateState::Error);
        assert_eq!(state(&UpdateView { up_to_date: true, ..v }, true), UpdateState::UpToDate);
    }

    fn open(rig: &mut Rig, v: UpdateView) -> String {
        *rig.host.view.borrow_mut() = v;
        rig.app.updates_window = true;
        rig.settle()
    }

    fn has(text: &str, want: &str) {
        assert!(text.lines().any(|l| l == want), "{want:?} missing in {text}");
    }

    #[test]
    fn the_update_window_draws_each_state() {
        let mac = cfg!(target_os = "macos");
        let mut rig = Rig::new(true);
        let text = open(&mut rig, linked());
        has(&text, "Euclidean Updates");
        has(&text, "Checking for updates…");

        let text = open(&mut rig, UpdateView { up_to_date: true, ..linked() });
        has(&text, "Euclidean 0.1.1 is up to date.");
        has(&text, "Close");

        let offered = UpdateView {
            busy: true,
            offered: Some("0.1.2".into()),
            notes: Some("Faster saves.".into()),
            received: 40_000_000,
            total: 100_000_000,
            ..linked()
        };
        let text = open(&mut rig, offered.clone());
        if mac {
            has(&text, "Euclidean 0.1.2 is available (you have 0.1.1).");
            has(&text, "Faster saves.");
            has(&text, "40.0 of 100.0 MB");
        } else {
            has(&text, "Checking for updates…");
        }

        if mac {
            let text = open(&mut rig, UpdateView { verifying: true, ..offered });
            has(&text, "Checking the download…");
        }

        let ready = UpdateView { ready: Some("0.1.2".into()), ..linked() };
        let text = open(&mut rig, ready);
        has(&text, "Euclidean 0.1.2 is ready.");
        has(&text, "Restart now");
        has(&text, "Later");

        let text = open(&mut rig, UpdateView { error: Some("Can't reach Cargo Bay — check your internet and try again.".into()), ..linked() });
        has(&text, "Can't reach Cargo Bay — check your internet and try again.");
        has(&text, "Try again");
        has(&text, "Close");

        let text = open(&mut rig, UpdateView::default());
        has(&text, "Link Euclidean to Cargo Bay to get updates.");
        has(&text, "Link…");

        let text = open(&mut rig, UpdateView { elsewhere: Some("0.1.2".into()), notes: Some("Faster saves.".into()), ..linked() });
        has(&text, "Euclidean 0.1.2 is available (you have 0.1.1).");
        has(&text, "0.1.2 is available — download it from the Mothership's Tools page");
        has(&text, "Close");
        assert!(!text.contains("Restart now"));
    }

    #[test]
    fn check_for_updates_opens_the_window_and_checks_once() {
        let mut rig = Rig::new(true);
        *rig.host.view.borrow_mut() = linked();
        rig.settle();
        assert!(!rig.app.updates_window);
        assert_eq!(rig.app.run("help.checkForUpdates", json!({})).unwrap(), json!({"started": true}));
        assert!(rig.app.updates_window);
        assert_eq!(rig.host.calls.borrow().checks, 1);
        let text = rig.settle();
        has(&text, "Euclidean Updates");
        assert_eq!(rig.host.calls.borrow().checks, 1, "a frame never checks by itself");
    }

    #[test]
    fn restart_now_asks_the_host_to_relaunch() {
        let mut rig = Rig::new(true);
        open(&mut rig, UpdateView { ready: Some("0.1.2".into()), ..linked() });
        rig.click("Restart now");
        assert_eq!(rig.host.calls.borrow().restarts, 1);
        assert_eq!(rig.app.ui.status, "quit");
        assert!(!rig.app.updates_window);
    }

    #[test]
    fn later_closes_the_window_and_leaves_one_status_button() {
        let mut rig = Rig::new(true);
        open(&mut rig, UpdateView { ready: Some("0.1.2".into()), ..linked() });
        rig.click("Later");
        assert!(!rig.app.updates_window);
        let text = rig.settle();
        assert_eq!(text.lines().filter(|l| *l == "Restart to update").count(), 1, "{text}");
        assert_eq!(text.lines().filter(|l| *l == "Euclidean 0.1.2 ready").count(), 1, "{text}");
        assert!(!text.contains("Restart now"), "{text}");
    }

    #[test]
    fn the_staged_message_is_not_doubled() {
        let mut rig = Rig::new(true);
        rig.app.updates_window = false;
        {
            let mut v = rig.host.view.borrow_mut();
            *v = UpdateView { ready: Some("0.1.2".into()), message: Some("0.1.2 ready — Restart to update".into()), message_seq: 1, ..linked() };
        }
        let text = rig.settle();
        assert!(!text.contains("ready — Restart to update"), "{text}");
        assert_eq!(text.matches("Restart to update").count(), 1, "{text}");
        assert!(!rig.app.ui.status.contains("Restart to update"), "{}", rig.app.ui.status);
    }

    #[test]
    fn clicking_the_ready_text_opens_the_window() {
        let mut rig = Rig::new(true);
        *rig.host.view.borrow_mut() = UpdateView { ready: Some("0.1.2".into()), ..linked() };
        rig.settle();
        assert!(!rig.app.updates_window);
        rig.click("Euclidean 0.1.2 ready");
        assert!(rig.app.updates_window);
        let text = rig.settle();
        has(&text, "Euclidean 0.1.2 is ready.");
    }

    #[test]
    fn the_background_check_does_not_open_the_window() {
        let mut rig = Rig::new(true);
        *rig.host.view.borrow_mut() = UpdateView { busy: true, ..linked() };
        rig.settle();
        *rig.host.view.borrow_mut() = UpdateView { ready: Some("0.1.2".into()), ..linked() };
        let text = rig.settle();
        assert!(!rig.app.updates_window);
        assert!(!text.contains("Euclidean Updates"), "{text}");
        has(&text, "Euclidean 0.1.2 ready");
    }

    #[test]
    fn try_again_checks_again() {
        let mut rig = Rig::new(true);
        open(&mut rig, UpdateView { error: Some("Cargo Bay had a problem — try again later.".into()), ..linked() });
        rig.click("Try again");
        assert_eq!(rig.host.calls.borrow().checks, 1);
        assert!(rig.app.updates_window, "stays open to show the new check");
    }

    #[test]
    fn link_button_opens_the_link_dialog() {
        let mut rig = Rig::new(true);
        rig.app.updates_later = true; // no first-run offer
        open(&mut rig, UpdateView::default());
        assert_eq!(rig.dialog_kind(), None);
        rig.click("Link…");
        assert_eq!(rig.dialog_kind(), Some(cargo_bay::KIND));
    }
}
