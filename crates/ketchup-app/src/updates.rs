//! Desktop updates. Network and package work never runs on the editor thread.
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use velopack::{UpdateCheck, UpdateManager, VelopackAsset, sources::GithubSource};

const REPOSITORY: &str = "https://github.com/Ketchup3D/app";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub fn initialize() {
    // The default also applies a previously downloaded update on next launch.
    velopack::VelopackApp::build().run();
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    automatic_updates: bool,
    last_check: Option<u64>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            automatic_updates: true,
            last_check: None,
        }
    }
}

impl Preferences {
    fn due(&self, now: u64) -> bool {
        self.automatic_updates
            && self.last_check.is_none_or(|last| {
                // A backwards clock adjustment must not suppress checks forever.
                now < last || now - last >= CHECK_INTERVAL.as_secs()
            })
    }

    fn load() -> Result<Self, String> {
        match std::fs::read(crate::storage::data_dir()?.join("updates.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn save(&self) -> Result<(), String> {
        write_json("updates.json", self)
    }
}

fn write_json(name: &str, value: &impl Serialize) -> Result<(), String> {
    let directory = crate::storage::data_dir()?;
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    crate::project_files::write(
        &directory.join(name),
        &serde_json::to_vec(value).map_err(|e| e.to_string())?,
    )
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

enum State {
    Unavailable,
    Idle,
    Checking,
    Downloading(String),
    Current,
    Ready(VelopackAsset),
    Failed(String),
}

enum Event {
    Downloading(String),
    Finished(Result<Option<VelopackAsset>, String>),
}

pub(crate) struct Updater {
    manager: Option<UpdateManager>,
    preferences: Preferences,
    worker: Option<Receiver<Event>>,
    state: State,
    pub open: bool,
    pub error: Option<String>,
    pub restarting: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            manager: None,
            preferences: Preferences::default(),
            worker: None,
            state: State::Unavailable,
            open: false,
            error: None,
            restarting: false,
        }
    }
}

impl Updater {
    pub fn new(ctx: &egui::Context) -> Self {
        // Locating an installation is local-only. Debug builds and bare binaries
        // have no Velopack manifest and must never check or replace themselves.
        let Ok(manager) =
            UpdateManager::new(GithubSource::new(REPOSITORY, None, false), None, None)
        else {
            return Self::default();
        };
        let (preferences, error) = match Preferences::load() {
            Ok(preferences) => (preferences, None),
            Err(error) => (
                Preferences {
                    automatic_updates: false,
                    ..Default::default()
                },
                Some(format!("Could not read update preferences: {error}")),
            ),
        };
        let mut updater = Self {
            manager: Some(manager),
            preferences,
            state: State::Idle,
            error,
            ..Default::default()
        };
        updater.check(ctx, true);
        updater
    }

    pub fn available(&self) -> bool {
        self.manager.is_some()
    }

    pub fn busy(&self) -> bool {
        self.worker.is_some() || self.restarting
    }

    pub fn ready(&self) -> bool {
        matches!(self.state, State::Ready(_))
    }

    pub fn automatic_updates(&self) -> bool {
        self.preferences.automatic_updates
    }

    pub fn set_automatic_updates(&mut self, enabled: bool, ctx: &egui::Context) {
        let previous = self.preferences.automatic_updates;
        self.preferences.automatic_updates = enabled;
        if let Err(error) = self.preferences.save() {
            self.preferences.automatic_updates = previous;
            self.error = Some(format!("Could not save update preferences: {error}"));
        } else {
            self.error = None;
            if enabled {
                self.check(ctx, true);
            }
        }
    }

    pub fn check(&mut self, ctx: &egui::Context, automatic: bool) {
        if self.busy() || self.ready() || (automatic && !self.preferences.due(now())) {
            return;
        }
        let Some(manager) = self.manager.clone() else {
            return;
        };
        // Persist attempts too: repeated launches while offline must not retry
        // continuously. A manual check always bypasses the daily limit.
        self.preferences.last_check = Some(now());
        if let Err(error) = self.preferences.save() {
            self.error = Some(format!("Could not save update preferences: {error}"));
            return;
        }
        self.error = None;
        self.state = State::Checking;
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("ketchup-updates".into())
            .spawn(move || {
                let result = (|| {
                    if let Some(asset) = manager.get_update_pending_restart() {
                        return Ok(Some(asset));
                    }
                    match manager.check_for_updates().map_err(|e| e.to_string())? {
                        UpdateCheck::UpdateAvailable(update) => {
                            let _ = sender
                                .send(Event::Downloading(update.TargetFullRelease.Version.clone()));
                            ctx.request_repaint();
                            // Velopack validates package size and checksums before
                            // exposing a complete package for installation.
                            manager
                                .download_updates(&update, None)
                                .map_err(|e| e.to_string())?;
                            Ok(Some(update.TargetFullRelease))
                        }
                        UpdateCheck::NoUpdateAvailable => Ok(None),
                        UpdateCheck::RemoteIsEmpty => {
                            Err("No updates have been published for this platform yet.".into())
                        }
                    }
                })();
                let _ = sender.send(Event::Finished(result));
                ctx.request_repaint();
            }) {
            Ok(_) => self.worker = Some(receiver),
            Err(error) => self.state = State::Failed(error.to_string()),
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Some(worker) = &self.worker {
            match worker.try_recv() {
                Ok(Event::Downloading(version)) => self.state = State::Downloading(version),
                Ok(Event::Finished(result)) => {
                    self.worker = None;
                    self.state = match result {
                        Ok(Some(asset)) => State::Ready(asset),
                        Ok(None) => State::Current,
                        Err(error) => State::Failed(error),
                    };
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.state = State::Failed(
                        "The update check did not complete. Please try again.".into(),
                    );
                }
            }
        }
        // Recheck during long sessions as well as after relaunch. No network
        // work runs here until the persisted daily interval expires.
        self.check(ctx, true);
        if self.available() {
            ctx.request_repaint_after(Duration::from_secs(60));
        }
    }

    pub fn status(&self) -> String {
        match &self.state {
            State::Unavailable => "Automatic updates are available in the packaged desktop app. Download it from GitHub Releases.".into(),
            State::Idle => "Ready to check for updates.".into(),
            State::Checking => "Checking for updates…".into(),
            State::Downloading(version) => format!("Downloading KetchUp {version}… You can keep working."),
            State::Current => "KetchUp is up to date.".into(),
            State::Ready(asset) => format!("KetchUp {} is ready. It will install on your next launch, or you can restart now.", asset.Version),
            State::Failed(error) => format!("Could not update KetchUp: {error}"),
        }
    }

    pub fn restart(&mut self) -> Result<(), String> {
        let (Some(manager), State::Ready(asset)) = (&self.manager, &self.state) else {
            return Err("No downloaded update is ready.".into());
        };
        // Let eframe exit normally so its shutdown and window cleanup run.
        manager
            .wait_exit_then_apply_updates(asset, false, true, std::iter::empty::<String>())
            .map_err(|e| e.to_string())?;
        self.restarting = true;
        Ok(())
    }
}

// The model itself stays in the normal autosave. Associate this metadata with
// that exact snapshot so an interrupted restart cannot restore a stale path.
#[derive(Serialize, Deserialize)]
pub(crate) struct Resume {
    document: String,
    pub path: Option<PathBuf>,
    pub modified: bool,
}

impl Resume {
    pub fn save(
        doc: &ketchup_core::Document,
        path: Option<PathBuf>,
        modified: bool,
    ) -> Result<(), String> {
        write_json(
            "update-resume.json",
            &Self {
                document: serde_json::to_string(doc).map_err(|e| e.to_string())?,
                path,
                modified,
            },
        )
    }

    pub fn take(doc: &ketchup_core::Document) -> Result<Option<Self>, String> {
        let file = crate::storage::data_dir()?.join("update-resume.json");
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        let result = serde_json::from_slice::<Self>(&bytes).map_err(|e| e.to_string());
        std::fs::remove_file(file).map_err(|e| e.to_string())?;
        let resume = result?;
        Ok(
            (serde_json::to_string(doc).map_err(|e| e.to_string())? == resume.document)
                .then_some(resume),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_and_restart_metadata_survive_relaunch_without_stale_associations() {
        const CHILD: &str = "KETCHUP_UPDATE_RESUME_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "updates::tests::preferences_and_restart_metadata_survive_relaunch_without_stale_associations", "--nocapture"])
                .env(CHILD, "1")
                .env("KETCHUP_DATA_DIR", directory.path())
                .output().unwrap();
            assert!(
                result.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
        assert!(std::env::var_os("KETCHUP_DATA_DIR").is_some());
        assert!(Preferences::load().unwrap().automatic_updates);
        let preferences = Preferences {
            automatic_updates: false,
            last_check: Some(123),
        };
        preferences.save().unwrap();
        let loaded = Preferences::load().unwrap();
        assert!(!loaded.automatic_updates);
        assert_eq!(loaded.last_check, Some(123));
        Preferences::default().save().unwrap();
        assert!(Preferences::load().unwrap().automatic_updates);

        let mut doc = ketchup_core::Document::default();
        let path = Some(PathBuf::from("models/project.ketchup"));
        Resume::save(&doc, path.clone(), true).unwrap();
        let restored = Resume::take(&doc).unwrap().unwrap();
        assert_eq!(restored.path, path);
        assert!(restored.modified);
        assert!(Resume::take(&doc).unwrap().is_none());
        Resume::save(&doc, path, false).unwrap();
        doc.name = "Work changed after an interrupted restart".into();
        assert!(Resume::take(&doc).unwrap().is_none());
        assert!(Resume::take(&doc).unwrap().is_none());
    }

    #[test]
    fn checks_are_daily_and_handle_clock_changes_and_disabled_preference() {
        let mut preferences = Preferences::default();
        assert!(preferences.due(100));
        preferences.last_check = Some(100);
        assert!(!preferences.due(100));
        assert!(!preferences.due(100 + CHECK_INTERVAL.as_secs() - 1));
        assert!(preferences.due(100 + CHECK_INTERVAL.as_secs()));
        assert!(preferences.due(99));
        preferences.automatic_updates = false;
        assert!(!preferences.due(100 + CHECK_INTERVAL.as_secs()));
    }

    #[test]
    fn download_events_become_ready_without_replacing_running_app() {
        let ctx = egui::Context::default();
        let (sender, receiver) = mpsc::channel();
        let mut updater = Updater {
            worker: Some(receiver),
            state: State::Checking,
            ..Default::default()
        };
        sender.send(Event::Downloading("0.2.0".into())).unwrap();
        updater.poll(&ctx);
        assert!(updater.busy());
        assert!(!updater.ready());
        sender
            .send(Event::Finished(Ok(Some(VelopackAsset {
                Version: "0.2.0".into(),
                ..Default::default()
            }))))
            .unwrap();
        updater.poll(&ctx);
        assert!(updater.ready());
        assert!(!updater.busy());
        assert!(!updater.restarting);
    }

    #[test]
    fn interrupted_worker_can_be_retried() {
        let ctx = egui::Context::default();
        let (sender, receiver) = mpsc::channel();
        let mut updater = Updater {
            worker: Some(receiver),
            ..Default::default()
        };
        drop(sender);
        updater.poll(&ctx);
        assert!(!updater.busy());
        assert!(matches!(updater.state, State::Failed(_)));
        assert!(updater.restart().is_err());
    }
}
