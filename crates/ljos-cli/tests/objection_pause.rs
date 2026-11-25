//! Past the child cap, and at the descendant limit, every objection is
//! filed or the reading pauses for a person. Nothing is only printed.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

#[path = "common/sibling.rs"]
mod sibling;
#[path = "common/tools.rs"]
mod tools;

static LOCK: Mutex<()> = Mutex::new(());

fn vissue_bin() -> PathBuf {
    sibling::require("vissue").unwrap_or_else(|line| panic!("{line}"))
}

struct Tracker {
    _dir: tempfile::TempDir,
    _saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl Tracker {
    fn new(config: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(home.join("run")).unwrap();
        let _bin = vissue_bin();
        let keys = [
            "PATH",
            "HOME",
            "XDG_CONFIG_HOME",
            "XDG_RUNTIME_DIR",
            "VISSUE_ROOT",
            "VISSUE_NO_ROUTE",
            "VISSUE_AGENT",
            "LJOS_SEAT",
            "LJOS_TRACKER_GIT",
            "LJOS_CONFIG",
            "PACKSET_URL",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_NOSYSTEM",
        ];
        let saved = keys.into_iter().map(|k| (k, std::env::var_os(k))).collect();
        let path = {
            let dir = vissue_bin().parent().unwrap().display().to_string();
            let rest = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
            format!("{dir}:/usr/bin:/bin:{rest}")
        };
        let git_cfg = dir.path().join("gitconfig");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&git_cfg, "").unwrap();
        unsafe {
            ljos_cli::test_env_guards::set_var_unchecked("PATH", path);
            ljos_cli::test_env_guards::set_var_unchecked("HOME", &home);
            ljos_cli::test_env_guards::set_var_unchecked("XDG_CONFIG_HOME", home.join(".config"));
            ljos_cli::test_env_guards::set_var_unchecked("XDG_RUNTIME_DIR", home.join("run"));
            ljos_cli::test_env_guards::set_var_unchecked("VISSUE_ROOT", &root);
            ljos_cli::test_env_guards::set_var_unchecked("VISSUE_NO_ROUTE", "1");
            ljos_cli::test_env_guards::set_var_unchecked("VISSUE_AGENT", "saddle@test");
            ljos_cli::test_env_guards::set_var_unchecked("LJOS_SEAT", "saddle");
            ljos_cli::test_env_guards::set_var_unchecked("LJOS_TRACKER_GIT", "off");
            ljos_cli::test_env_guards::set_var_unchecked("PACKSET_URL", "off");
            ljos_cli::test_env_guards::set_var_unchecked("GIT_CONFIG_GLOBAL", &git_cfg);
            ljos_cli::test_env_guards::set_var_unchecked("GIT_CONFIG_NOSYSTEM", "1");
            if let Some(text) = config {
                let path = dir.path().join("ljos.toml");
                #[expect(
                    clippy::disallowed_methods,
                    reason = "writes a file that is not a git hook"
                )]
                std::fs::write(&path, text).unwrap();
                ljos_cli::test_env_guards::set_var_unchecked("LJOS_CONFIG", &path);
            } else {
                ljos_cli::test_env_guards::remove_var_unchecked("LJOS_CONFIG");
            }
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let _ = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "-q"])
            .output();
        Self {
            _dir: dir,
            _saved: saved,
        }
    }

    fn vissue(&self, args: &[&str]) -> String {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let out = Command::new("vissue").args(args).output().unwrap();
        assert!(
            out.status.success(),
            "vissue {args:?}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn show(&self, id: &str) -> serde_json::Value {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let out = Command::new("vissue")
            .args(["show", id, "--json"])
            .output()
            .unwrap();
        serde_json::from_slice(&out.stdout).expect("show json")
    }
}

impl Drop for Tracker {
    fn drop(&mut self) {
        for (key, value) in &self._saved {
            unsafe {
                match value {
                    Some(v) => ljos_cli::test_env_guards::set_var_unchecked(key, v),
                    None => ljos_cli::test_env_guards::remove_var_unchecked(key),
                }
            }
        }
    }
}

#[test]
fn objections_past_the_child_cap_are_not_dropped() -> Result<(), String> {
    if tools::halt(tools::Tool::Vissue) {
        return Ok(());
    }
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let t = Tracker::new(None);
    let parent = t.vissue(&["q", "-p", "probe", "split reading"]);
    for n in 1..=9 {
        t.vissue(&[
            "note",
            &parent,
            &format!("finding: carol: blocking: a real failure :: distinct bug number {n}"),
        ]);
    }
    let lines = ljos_cli::converge::record_dissents(&parent);
    let text = lines.join("\n");
    let state = t.show(&parent)["state"].as_str().unwrap().to_string();
    assert_ne!(state, "DONE", "{text}");
    assert_ne!(state, "CANCELLED", "{text}");
    assert!(
        text.contains("paused for a person"),
        "an objection past the cap was only printed:\n{text}"
    );
    assert!(
        text.contains("was not filed"),
        "the unfiled objection text is missing:\n{text}"
    );
    let kids = t.vissue(&["children", &parent, "--json"]);
    let filed = kids.matches("distinct bug").count();
    assert_eq!(filed, 8, "filed {filed}, not the cap:\n{kids}\n{text}");
    let card = t.show(&parent).to_string();
    assert!(
        card.contains("was not filed") && card.contains("distinct bug number 9"),
        "the unfiled objection is not on the ticket:\n{card}"
    );
    let once = card.matches("was not filed").count();
    let _ = ljos_cli::converge::record_dissents(&parent);
    let again = t.show(&parent).to_string();
    assert_eq!(
        again.matches("was not filed").count(),
        once,
        "the pause was written again:\n{again}"
    );
    Ok(())
}

#[test]
fn the_descendant_limit_pauses_and_does_not_close() -> Result<(), String> {
    if tools::halt(tools::Tool::Vissue) {
        return Ok(());
    }
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let t = Tracker::new(Some("[panel]\nmax_descendants = 1\n"));
    let parent = t.vissue(&["q", "-p", "probe", "wide tree"]);
    let child = t.vissue(&["q", "-p", "probe", "--parent", &parent, "already filed"]);
    let before = t.vissue(&["children", &parent, "--json"]);
    assert!(before.contains(&child), "{before}");
    t.vissue(&[
        "note",
        &parent,
        "finding: carol: blocking: a real failure :: one more bug",
    ]);
    let lines = ljos_cli::converge::record_dissents(&parent);
    let text = lines.join("\n");
    assert!(
        text.contains("paused for a person"),
        "the limit did not pause:\n{text}"
    );
    assert!(text.contains("one more bug"), "{text}");
    let state = t.show(&parent)["state"].as_str().unwrap().to_string();
    assert_ne!(state, "DONE", "{text}");
    assert_ne!(state, "CANCELLED", "{text}");
    let kids = t.vissue(&["children", &parent, "--json"]);
    assert!(kids.contains(&child), "{kids}");
    assert!(
        !kids.contains("one more bug"),
        "filed past the limit:\n{kids}"
    );
    let card = t.show(&parent).to_string();
    assert!(
        card.contains("was not filed") && card.contains("one more bug"),
        "the pause is not on the ticket:\n{card}"
    );
    Ok(())
}

/// Eight dissent children, then each of those reads eight objections.
/// The descendant limit is the whole tree.
#[test]
fn a_nested_split_pauses_at_the_descendant_limit() -> Result<(), String> {
    if tools::halt(tools::Tool::Vissue) {
        return Ok(());
    }
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let t = Tracker::new(None);
    let limits = ljos_cli::converge::ScopeBudget::panel();
    let root = t.vissue(&["q", "-p", "probe", "nested split"]);
    for n in 1..=8 {
        t.vissue(&[
            "note",
            &root,
            &format!("finding: carol: blocking: a real failure :: root bug {n}"),
        ]);
    }
    let _ = ljos_cli::converge::record_dissents(&root);
    let kids: Vec<serde_json::Value> =
        serde_json::from_str(&t.vissue(&["children", &root, "--json"])).unwrap();
    let ids: Vec<String> = kids
        .iter()
        .filter_map(|row| row["id"].as_str().map(str::to_string))
        .collect();
    assert_eq!(ids.len(), 8, "{ids:?}");
    for (i, kid) in ids.iter().enumerate() {
        for n in 1..=8 {
            t.vissue(&[
                "note",
                kid,
                &format!("finding: dave: blocking: a real failure :: child {i} bug {n}"),
            ]);
        }
        let _ = ljos_cli::converge::record_dissents(kid);
    }
    let total = ljos_cli::converge::descendant_count(&root);
    assert!(
        total <= limits.max_descendants,
        "the tree holds {total}, limit {}",
        limits.max_descendants
    );
    Ok(())
}
