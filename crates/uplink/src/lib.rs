//! Cargo Bay uplink for Euclidean: link this copy with a code, check for a signed update, stage
//! it, and swap it in at the next start. The wire contract is cargo-bay's
//! `docs/update-contract.md`; the Python client in `clients/fusion-uplink` is the reference.
//!
//! The crate never deletes anything: old files are renamed or moved aside.

pub mod canon;
pub mod client;
pub mod clock;
pub mod keyring;
pub mod messages;
pub mod stage;
pub mod state;
pub mod swap;
#[cfg(any(test, feature = "uplink-test-key"))]
pub mod test_key;
pub mod trust;
pub mod zipcheck;

#[cfg(all(test, unix))]
mod tests;
#[cfg(all(test, unix))]
mod testutil;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use client::{CheckResult, Client, LinkError, Offer};
pub use stage::{StageError, Staged};
pub use state::{Link, StateDir};
pub use swap::{StartOutcome, current_bundle, finish_pending, relaunch, unshown_update};
pub use trust::Trust;

/// This tool's id at Cargo Bay.
pub const TOOL_ID: &str = "euclidean";
/// The only host the crate talks to.
pub const DEFAULT_BASE_URL: &str = "https://cargo-bay.eccentricexecutioners.ca";
pub const CLAIM_PATH: &str = "/api/v1/installs/claim";
pub const CHECK_PATH: &str = "/api/v1/update-check";
/// How long one request may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// The folder name inside the release zip.
pub const BUNDLE_NAME: &str = "Euclidean.app";

/// Everything an [`Updater`] needs.
#[derive(Clone, Debug)]
pub struct Config {
    pub base_url: String,
    pub state: StateDir,
    pub installed_version: String,
    /// `mac`, `windows` or `linux`; only `mac` stages an update.
    pub platform: &'static str,
    pub trust: Trust,
    pub timeout: Duration,
}

impl Config {
    /// The normal configuration: `EUCLIDEAN_CARGO_BAY_URL` and `EUCLIDEAN_CARGO_BAY_STATE_DIR`
    /// override the defaults (for tests and the scratch end-to-end only).
    pub fn from_env(installed_version: &str) -> Config {
        let base_url = match std::env::var("EUCLIDEAN_CARGO_BAY_URL").ok().filter(|v| !v.is_empty()) {
            Some(url) => match client::check_base_url(&url) {
                Ok(ok) => ok,
                Err(why) => {
                    log::warn!("uplink: ignoring EUCLIDEAN_CARGO_BAY_URL ({why})");
                    DEFAULT_BASE_URL.to_string()
                }
            },
            None => DEFAULT_BASE_URL.to_string(),
        };
        let state = StateDir::default();
        let trust = Trust::builtin(&state);
        Config { base_url, state, installed_version: installed_version.to_string(), platform: client::platform(), trust, timeout: TIMEOUT }
    }
}

fn off_reason_with(var: &dyn Fn(&str) -> Option<String>, headless: bool, exe: &Path, mac: bool) -> Option<&'static str> {
    let set = |k: &str| var(k).is_some_and(|v| !v.is_empty());
    if var("EUCLIDEAN_NO_UPDATE").as_deref() == Some("1") {
        return Some("EUCLIDEAN_NO_UPDATE is set");
    }
    if set("VECTORCRAFT_NO_PREFS") {
        return Some("VECTORCRAFT_NO_PREFS is set");
    }
    if headless {
        return Some("headless run");
    }
    if mac {
        if current_bundle(exe).is_none() {
            return Some("not running from an app bundle");
        }
    } else if exe.components().any(|c| c.as_os_str() == "target") {
        return Some("development run");
    }
    None
}

/// Why updates are off in this run, or `None` when they are on.
pub fn off_reason(headless: bool, exe: &Path) -> Option<&'static str> {
    off_reason_with(&|k| std::env::var(k).ok(), headless, exe, cfg!(target_os = "macos"))
}

/// What one check came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    NotLinked,
    UpToDate,
    Staged {
        version: String,
        already: bool,
    },
    /// An update exists but this platform does not stage it.
    Elsewhere(String),
    LinkRemoved,
    NoAccess,
    Offline,
    TlsFailed,
    ServerError(String),
    VerifyFailed(String),
    StageFailed(String),
}

/// Check for an update and, on a Mac, stage it. Blocking. `on_offer` hears the new version
/// before the download starts.
pub fn run_check(cfg: &Config, on_offer: &dyn Fn(&str)) -> Outcome {
    let Some(key) = cfg.state.read_key() else {
        return Outcome::NotLinked;
    };
    let client = match Client::with_timeout(&cfg.base_url, cfg.timeout) {
        Ok(c) => c,
        Err(why) => return Outcome::ServerError(why),
    };
    let masters = cfg.trust.masters();
    let serial = keyring::held_serial(&cfg.state, &masters);
    let result = client.check(&key, &cfg.installed_version, cfg.platform, serial, &|ring| {
        keyring::consider(&cfg.state, &masters, ring);
    });
    match result {
        CheckResult::UpToDate => Outcome::UpToDate,
        CheckResult::NoAccess => Outcome::NoAccess,
        CheckResult::Offline => Outcome::Offline,
        CheckResult::TlsFailed => Outcome::TlsFailed,
        CheckResult::ServerError(d) => Outcome::ServerError(d),
        CheckResult::LinkRemoved => {
            if let Err(e) = cfg.state.retire_key() {
                log::warn!("uplink: could not set the refused key aside: {e}");
            }
            Outcome::LinkRemoved
        }
        CheckResult::Update(offer) => {
            if cfg.platform != "mac" {
                return Outcome::Elsewhere(offer.version);
            }
            on_offer(&offer.version);
            // A key list that just arrived may name the key this release is signed with.
            let mut trust = cfg.trust.clone();
            trust.release = keyring::load(&cfg.state, &masters).map(|k| k.keys).unwrap_or_default();
            match stage::stage(&cfg.state, &trust, &client, &offer, &cfg.installed_version) {
                Ok(s) => Outcome::Staged { version: s.version, already: s.already },
                Err(StageError::Offline) => Outcome::Offline,
                Err(StageError::TlsFailed) => Outcome::TlsFailed,
                Err(StageError::VerifyFailed(why)) => Outcome::VerifyFailed(why),
                Err(StageError::Failed(why)) => Outcome::StageFailed(why),
            }
        }
    }
}

/// Trade a code for a key and save it. Blocking.
pub fn run_link(cfg: &Config, code: &str) -> Result<(), LinkError> {
    let client = Client::with_timeout(&cfg.base_url, cfg.timeout).map_err(LinkError::ServerError)?;
    let key = client.claim(code)?;
    cfg.state.save_link(&Link::new(&key, &cfg.base_url)).map_err(|_| LinkError::ServerError("could not save the key".into()))
}

fn link_message(e: &LinkError) -> &'static str {
    match e {
        LinkError::BadCode => messages::BAD_CODE,
        LinkError::NotGranted => messages::NO_ACCESS,
        LinkError::RateLimited => messages::RATE_LIMITED,
        LinkError::Offline => messages::OFFLINE,
        LinkError::TlsFailed => messages::TLS_FAILED,
        LinkError::ServerError(_) => messages::SERVER_ERROR,
    }
}

/// The line for an outcome.
pub fn outcome_message(outcome: &Outcome, installed: &str) -> String {
    match outcome {
        Outcome::NotLinked => messages::NOT_LINKED.into(),
        Outcome::UpToDate => messages::fill(messages::UP_TO_DATE, &[("version", installed)]),
        Outcome::Staged { version, .. } => messages::fill(messages::STAGED, &[("new", version)]),
        Outcome::Elsewhere(v) => messages::fill(messages::ELSEWHERE, &[("new", v)]),
        Outcome::LinkRemoved => messages::LINK_REMOVED.into(),
        Outcome::NoAccess => messages::NO_ACCESS.into(),
        Outcome::Offline => messages::OFFLINE.into(),
        Outcome::TlsFailed => messages::TLS_FAILED.into(),
        Outcome::ServerError(_) => messages::SERVER_ERROR.into(),
        Outcome::VerifyFailed(_) => messages::VERIFY_FAILED.into(),
        Outcome::StageFailed(_) => messages::STAGE_FAILED.into(),
    }
}

/// What the app shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub linked: bool,
    pub busy: bool,
    pub message: Option<String>,
    /// The staged version, waiting for a restart.
    pub ready: Option<String>,
    /// A newer version this platform cannot install by itself.
    pub elsewhere: Option<String>,
    /// Bumps each time `message` is set, so the app shows each message once.
    pub seq: u64,
    /// The last link attempt's English error, cleared when the next attempt starts or works.
    pub link_error: Option<String>,
}

impl View {
    fn say(&mut self, message: String) {
        self.message = Some(message);
        self.seq += 1;
    }
}

type OnChange = Arc<dyn Fn() + Send + Sync>;

struct Inner {
    cfg: Config,
    view: Mutex<View>,
    on_change: Mutex<Option<OnChange>>,
}

/// Runs links and checks on a background thread and keeps a [`View`] for the app.
#[derive(Clone)]
pub struct Updater {
    inner: Arc<Inner>,
}

/// The staged version in the pending file, if it is newer than what runs now.
fn pending_version(cfg: &Config) -> Option<String> {
    let text = std::fs::read_to_string(cfg.state.pending_file()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    if v.get("staged").and_then(serde_json::Value::as_bool) != Some(true) {
        return None;
    }
    let version = v.get("version")?.as_str()?.to_string();
    (version != cfg.installed_version).then_some(version)
}

impl Updater {
    pub fn new(cfg: Config) -> Updater {
        let view = View { linked: cfg.state.read_key().is_some(), ready: pending_version(&cfg), ..View::default() };
        Updater { inner: Arc::new(Inner { cfg, view: Mutex::new(view), on_change: Mutex::new(None) }) }
    }

    /// Ask to be told (to repaint) whenever the view changes.
    pub fn set_on_change(&self, f: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut slot) = self.inner.on_change.lock() {
            *slot = Some(Arc::new(f));
        }
    }

    pub fn config(&self) -> &Config {
        &self.inner.cfg
    }

    pub fn view(&self) -> View {
        self.inner.view.lock().map(|v| v.clone()).unwrap_or_default()
    }

    fn update(&self, f: impl FnOnce(&mut View)) {
        if let Ok(mut v) = self.inner.view.lock() {
            f(&mut v);
        }
        let hook = self.inner.on_change.lock().ok().and_then(|s| s.clone());
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Mark busy; false when something is already running.
    fn begin(&self) -> bool {
        let mut started = false;
        if let Ok(mut v) = self.inner.view.lock()
            && !v.busy
        {
            v.busy = true;
            v.message = None;
            v.link_error = None;
            started = true;
        }
        if started {
            self.update(|_| {});
        }
        started
    }

    /// Link with a code from the Tools page. Returns at once; the view changes when done.
    pub fn link(&self, code: &str) {
        if !self.begin() {
            return;
        }
        let me = self.clone();
        let code = code.to_string();
        std::thread::spawn(move || {
            let result = run_link(&me.inner.cfg, &code);
            me.update(|v| {
                v.busy = false;
                match &result {
                    Ok(()) => {
                        v.linked = true;
                        v.say(messages::LINKED.into());
                    }
                    Err(e) => {
                        v.link_error = Some(link_message(e).into());
                        v.say(link_message(e).into());
                    }
                }
            });
        });
    }

    /// Check for an update (and stage it on a Mac). Returns at once.
    pub fn check(&self) {
        if !self.begin() {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let cfg = &me.inner.cfg;
            let outcome = run_check(cfg, &|new| {
                me.update(|v| v.say(messages::fill(messages::STAGING, &[("new", new)])));
            });
            let message = outcome_message(&outcome, &cfg.installed_version);
            me.update(|v| {
                v.busy = false;
                v.say(message);
                match &outcome {
                    Outcome::Staged { version, .. } => v.ready = Some(version.clone()),
                    Outcome::Elsewhere(version) => v.elsewhere = Some(version.clone()),
                    Outcome::LinkRemoved | Outcome::NotLinked => v.linked = false,
                    _ => {}
                }
            });
        });
    }
}
