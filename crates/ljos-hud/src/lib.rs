//! Read-only icedtea pane over the seat: due reviews, live claims, trust.
//!
//! The musl CLI does not link iced. `ljos hud` execs this binary.

pub mod app;
pub mod cli;
pub mod data;
pub mod detach;
pub mod graph;
pub mod install_desktop;
pub mod log;
pub mod place;
pub mod summon;
pub mod theme;
pub mod tray;
pub mod view;
pub mod wlactivate;

pub use cli::run_cli;
pub use summon::{parse_request, sanitize_token, SummonAction, SummonRequest};

/// `PATH`, `HOME`, and `XDG_*` for the calling thread.
///
/// A HUD test records those here. The process environment is not edited,
/// so another crate's `which("cat")` still finds cat. Nothing here takes
/// a lock.
#[cfg(test)]
pub(crate) mod test_env {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::ffi::{OsStr, OsString};

    thread_local! {
        static OVERLAY: RefCell<BTreeMap<OsString, Option<OsString>>> =
            const { RefCell::new(BTreeMap::new()) };
    }

    pub(crate) fn is_isolated(key: &OsStr) -> bool {
        let Some(text) = key.to_str() else {
            return false;
        };
        text == "PATH" || text == "HOME" || text.starts_with("XDG_")
    }

    enum Prev {
        Absent,
        Recorded(Option<OsString>),
    }

    #[must_use = "the recorded environment lasts until this guard drops"]
    pub(crate) struct Guard {
        key: OsString,
        prev: Prev,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OVERLAY.with(|slot| {
                let mut map = slot.borrow_mut();
                match &self.prev {
                    Prev::Absent => {
                        map.remove(&self.key);
                    }
                    Prev::Recorded(value) => {
                        map.insert(self.key.clone(), value.clone());
                    }
                }
            });
        }
    }

    fn record(key: &OsStr, value: Option<OsString>) -> Guard {
        assert!(
            is_isolated(key),
            "only PATH, HOME, and XDG_* are recorded on the calling thread"
        );
        let owned = key.to_os_string();
        let prev = OVERLAY.with(
            |slot| match slot.borrow_mut().insert(owned.clone(), value) {
                Some(old) => Prev::Recorded(old),
                None => Prev::Absent,
            },
        );
        Guard { key: owned, prev }
    }

    pub(crate) fn set(key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Guard {
        record(key.as_ref(), Some(value.as_ref().to_os_string()))
    }

    /// Drop the record. A later read sees the process environment.
    pub(crate) fn clear(key: &str) {
        OVERLAY.with(|slot| {
            slot.borrow_mut().remove(OsStr::new(key));
        });
    }

    /// `Some` when this thread recorded `key`, including a removal.
    pub(crate) fn recorded(key: &str) -> Option<Option<OsString>> {
        OVERLAY.with(|slot| slot.borrow().get(OsStr::new(key)).cloned())
    }
}

/// Serialises HUD tests that read or write the process environment.
/// Each holder records a private `XDG_RUNTIME_DIR` on this thread. `PATH`,
/// `HOME`, and `XDG_*` are not written back onto the process.
#[cfg(test)]
#[must_use = "the environment is restored when this guard drops"]
pub(crate) struct EnvLock {
    _guard: std::sync::MutexGuard<'static, ()>,
    snapshot: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    _runtime_env: test_env::Guard,
    runtime: tempfile::TempDir,
}

#[cfg(test)]
impl Drop for EnvLock {
    fn drop(&mut self) {
        let keep: std::collections::HashSet<std::ffi::OsString> =
            self.snapshot.iter().map(|(key, _)| key.clone()).collect();
        for (key, value) in &self.snapshot {
            if test_env::is_isolated(key) {
                continue;
            }
            #[allow(unused_unsafe)]
            unsafe {
                std::env::set_var(key, value);
            }
        }
        let added: Vec<std::ffi::OsString> = std::env::vars_os()
            .map(|(key, _)| key)
            .filter(|key| !keep.contains(key))
            .collect();
        for key in added {
            if test_env::is_isolated(&key) {
                continue;
            }
            #[allow(unused_unsafe)]
            unsafe {
                std::env::remove_var(key);
            }
        }
        let _ = self.runtime.path();
    }
}

#[cfg(test)]
pub(crate) fn env_lock() -> EnvLock {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let snapshot: Vec<_> = std::env::vars_os().collect();
    let runtime = tempfile::tempdir().expect("private runtime dir");
    let runtime_env = test_env::set("XDG_RUNTIME_DIR", runtime.path());
    EnvLock {
        _guard: guard,
        snapshot,
        _runtime_env: runtime_env,
        runtime,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dist_targets_are_gnu_and_darwin_only() {
        let cargo = include_str!("../Cargo.toml");
        assert!(cargo.contains("aarch64-apple-darwin"));
        assert!(cargo.contains("x86_64-unknown-linux-gnu"));
        assert!(!cargo.contains("musl"), "HUD must not ship a musl artifact");
    }

    #[test]
    fn release_yml_ships_hud_on_gnu_and_darwin_only() {
        let yml = include_str!("../../../.github/workflows/release.yml");
        assert!(yml.contains("-p ljos-hud"));
        assert!(yml.contains("x86_64-unknown-linux-gnu"));
        assert!(yml.contains("aarch64-apple-darwin"));
        assert!(
            !yml.contains("musl"),
            "release matrix must not build a musl HUD"
        );
    }

    #[test]
    fn ljos_cli_cargo_has_no_iced() {
        let cargo = include_str!("../../ljos-cli/Cargo.toml");
        assert!(
            !cargo.contains("iced"),
            "ljos-cli Cargo.toml must not name iced"
        );
        assert!(
            !cargo.contains("icedtea"),
            "ljos-cli Cargo.toml must not name icedtea"
        );
        assert!(
            !cargo.contains("ljos-hud"),
            "ljos-cli must not depend on the HUD crate"
        );
    }

    #[test]
    fn publish_yml_publishes_hud_after_ljos() {
        let yml = include_str!("../../../.github/workflows/publish.yml");
        let ljos = yml
            .find("-p ljos\n")
            .or_else(|| yml.find("-p ljos "))
            .expect("publish -p ljos");
        let hud = yml.find("-p ljos-hud").expect("publish -p ljos-hud");
        assert!(
            hud > ljos,
            "HUD depends on the ljos crate; publish ljos first"
        );
    }

    #[test]
    fn hud_sources_never_call_write_verbs() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let verbs = [
            "graded(",
            "post_atom",
            "sweep(",
            "fire=true",
            "fire: true",
            "fire:true",
            "due_report",
            "sitting_due_report",
            "WorkGraph::load_dir",
            "complete(",
            "packset_island(cue, true)",
        ];
        let mut hits = Vec::new();
        fn walk(dir: &std::path::Path, verbs: &[&str], hits: &mut Vec<String>) {
            for ent in std::fs::read_dir(dir).unwrap() {
                let path = ent.unwrap().path();
                if path.is_dir() {
                    walk(&path, verbs, hits);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                let prod = text.split("#[cfg(test)]").next().unwrap();
                for verb in verbs {
                    if prod.contains(verb) {
                        hits.push(format!("{}: {verb}", path.display()));
                    }
                }
            }
        }
        walk(&root, &verbs, &mut hits);
        assert!(
            hits.is_empty(),
            "HUD crate is read-only; must not call write verbs: {hits:?}"
        );
    }

    #[test]
    fn hud_island_is_fire_false() {
        let src = include_str!("data.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("packset_island(cue, false)"));
        assert!(prod.contains("packset_search"));
        assert!(!prod.contains("packset_island(cue, true)"));
        assert!(!prod.contains("fire: true"));
    }
}
