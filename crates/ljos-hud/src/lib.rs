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
mod process_env;
pub mod summon;
pub mod theme;
pub mod tray;
pub mod view;
pub mod wlactivate;

pub use cli::run_cli;
pub use summon::{parse_request, sanitize_token, SummonAction, SummonRequest};

/// Serialises HUD tests that read or write the process environment.
/// Each holder gets a private `XDG_RUNTIME_DIR`, and the environment it
/// found is put back while the mutex is still held.
#[cfg(test)]
#[must_use = "the environment is restored when this guard drops"]
pub(crate) struct EnvLock {
    _hold: crate::process_env::Hold,
    snapshot: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    runtime: tempfile::TempDir,
}

#[cfg(test)]
impl Drop for EnvLock {
    fn drop(&mut self) {
        let keep: std::collections::HashSet<std::ffi::OsString> =
            self.snapshot.iter().map(|(key, _)| key.clone()).collect();
        for (key, value) in &self.snapshot {
            crate::process_env::set_var(key, value);
        }
        let added: Vec<std::ffi::OsString> = crate::process_env::vars_os()
            .map(|(key, _)| key)
            .filter(|key| !keep.contains(key))
            .collect();
        for key in added {
            crate::process_env::remove_var(key);
        }
        let _ = self.runtime.path();
    }
}

#[cfg(test)]
pub(crate) fn env_lock() -> EnvLock {
    let hold = crate::process_env::hold();
    let snapshot: Vec<_> = crate::process_env::vars_os().collect();
    let runtime = tempfile::tempdir().expect("private runtime dir");
    crate::process_env::set_var("XDG_RUNTIME_DIR", runtime.path());
    EnvLock {
        _hold: hold,
        snapshot,
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
