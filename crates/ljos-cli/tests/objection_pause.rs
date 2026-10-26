//! Past the child cap, and at the descendant limit, every objection is
//! filed or the reading pauses for a person. Nothing is only printed.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

fn vissue_bin() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let bin = dir.join("vissue");
        bin.is_file().then_some(bin)
    })
}

/// vissue 0.20 has to be on PATH. CI builds it. A tree without it
/// skips these tests and says why.
fn vissue_020() -> bool {
    let Some(bin) = vissue_bin() else {
        eprintln!("skip: vissue 0.20 is not on PATH");
        return false;
    };
    let Ok(version) = Command::new(bin).arg("--version").output() else {
        eprintln!("skip: vissue 0.20 is not on PATH");
        return false;
    };
    let text = String::from_utf8_lossy(&version.stdout);
    if !text.contains("0.20") {
        eprintln!("skip: vissue 0.20 is not on PATH (found {text})");
        return false;
    }
    true
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
        let bin = vissue_bin().expect("vissue 0.20");
        let version = Command::new(&bin)
            .arg("--version")
            .output()
            .expect("real vissue");
        let text = String::from_utf8_lossy(&version.stdout);
        assert!(text.contains("0.20"), "{text}");
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
            let dir = vissue_bin()
                .expect("vissue 0.20")
                .parent()
                .unwrap()
                .display()
                .to_string();
            let rest = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
            format!("{dir}:/usr/bin:/bin:{rest}")
        };
        let git_cfg = dir.path().join("gitconfig");
        std::fs::write(&git_cfg, "").unwrap();
        unsafe {
            std::env::set_var("PATH", path);
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
            std::env::set_var("XDG_RUNTIME_DIR", home.join("run"));
            std::env::set_var("VISSUE_ROOT", &root);
            std::env::set_var("VISSUE_NO_ROUTE", "1");
            std::env::set_var("VISSUE_AGENT", "saddle@test");
            std::env::set_var("LJOS_SEAT", "saddle");
            std::env::set_var("LJOS_TRACKER_GIT", "off");
            std::env::set_var("PACKSET_URL", "off");
            std::env::set_var("GIT_CONFIG_GLOBAL", &git_cfg);
            std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
            if let Some(text) = config {
                let path = dir.path().join("ljos.toml");
                std::fs::write(&path, text).unwrap();
                std::env::set_var("LJOS_CONFIG", &path);
            } else {
                std::env::remove_var("LJOS_CONFIG");
            }
        }
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
        let out = Command::new("vissue").args(args).output().unwrap();
        assert!(
            out.status.success(),
            "vissue {args:?}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn show(&self, id: &str) -> serde_json::Value {
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
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

#[test]
fn objections_past_the_child_cap_are_not_dropped() {
    if !vissue_020() {
        return;
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
        card.contains("distinct bug number"),
        "the finding is not on the ticket:\n{card}"
    );
}

#[test]
fn the_descendant_limit_pauses_and_does_not_close() {
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
}

/// Eight dissent children, then each of those reads eight objections.
/// The descendant limit is the whole tree.
#[test]
fn a_nested_split_pauses_at_the_descendant_limit() {
    if !vissue_020() {
        return;
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
}
