//! Whether the tracker checkout still has work the other hosts cannot see.
//!
//! Unpushed commits and uncommitted issue edits are one line, with the
//! command that publishes them. Finish pushes the commits when the tracker
//! config allows a push. Stray edits are left for that command.

use std::path::Path;

/// The line a digest and a sitting print when the tracker has drifted.
/// `None` when it is caught up, or when it is not a checkout.
#[must_use]
pub fn tracker_sync_line() -> Option<String> {
    let layout = vissue_core::Layout::resolve(None, None).ok()?;
    let root = layout.root();
    let unpushed = unpushed_count(root).unwrap_or(0);
    let edits = org_edits(root);
    if unpushed == 0 && edits == 0 {
        return None;
    }
    let phrase = counts_phrase(unpushed, edits);
    let fix = fix_command(root, unpushed, edits);
    Some(format!("tracker: {phrase}; fix: {fix}\n"))
}

/// Push tracker commits a finish did not already push.
///
/// Empty when the config does not push, nothing is waiting, a push is
/// already running, or the checkout has no remote. Edits are not committed.
#[must_use]
pub fn push_leftover() -> String {
    if !allows_push() {
        return String::new();
    }
    let Ok(layout) = vissue_core::Layout::resolve(None, None) else {
        return String::new();
    };
    let root = layout.root().to_path_buf();
    if crate::tracker_push_logs().0 || !has_remote(&root) {
        return String::new();
    }
    let Some(waiting) = unpushed_count(&root).filter(|n| *n > 0) else {
        return String::new();
    };
    match crate::tracker_push::publish(&root) {
        Ok(()) => {
            crate::clear_push_refusal();
            format!("tracker git: pushed {waiting}\n")
        }
        Err(why) => {
            crate::record_push_refusal(&why);
            format!("tracker git: push refused: {why}\n")
        }
    }
}

fn allows_push() -> bool {
    let mode = std::env::var("LJOS_TRACKER_GIT").unwrap_or_default();
    !matches!(mode.as_str(), "off" | "0" | "false" | "commit")
}

fn git_leaves_edits() -> bool {
    matches!(
        std::env::var("LJOS_TRACKER_GIT")
            .unwrap_or_default()
            .as_str(),
        "off" | "0" | "false"
    )
}

fn unpushed_count(root: &Path) -> Option<u64> {
    let up = crate::tracker_upstream(root)?;
    let range = format!("{up}..HEAD");
    crate::git_ok_stdout(root, &["rev-list", "--count", &range])?
        .trim()
        .parse()
        .ok()
}

fn has_remote(root: &Path) -> bool {
    crate::git_ok_stdout(root, &["remote"])
        .is_some_and(|text| text.lines().any(|line| !line.trim().is_empty()))
}

fn org_edits(root: &Path) -> usize {
    if git_leaves_edits() {
        return 0;
    }
    let Some(status) = crate::git_ok_stdout(
        root,
        &["status", "--porcelain", "--untracked-files=all", "--", "."],
    ) else {
        return 0;
    };
    status
        .lines()
        .filter_map(|line| line.get(3..))
        .map(|path| path.rsplit(" -> ").next().unwrap_or(path).trim_matches('"'))
        .filter(|path| path.ends_with(".org"))
        .count()
}

fn counts_phrase(unpushed: u64, edits: usize) -> String {
    let mut parts = Vec::new();
    if unpushed == 1 {
        parts.push("1 unpushed".to_string());
    } else if unpushed > 1 {
        parts.push(format!("{unpushed} unpushed"));
    }
    if edits == 1 {
        parts.push("1 uncommitted edit".to_string());
    } else if edits > 1 {
        parts.push(format!("{edits} uncommitted edits"));
    }
    parts.join(", ")
}

fn fix_command(root: &Path, unpushed: u64, edits: usize) -> String {
    let quoted = crate::tools::sh_quote(&root.display().to_string());
    let mut parts = Vec::new();
    if edits > 0 {
        let message = crate::tools::sh_quote("tracker: commit issue edits");
        parts.push(format!("git -C {quoted} add -u -- ."));
        parts.push(format!("git -C {quoted} commit -m {message}"));
    }
    if unpushed > 0 {
        parts.push(format!("git -C {quoted} push"));
    }
    parts.join(" && ")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn checkout(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let origin = dir.join("origin.git");
        let root = dir.join("tracker");
        git(
            dir,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        );
        std::fs::create_dir_all(root.join("Software/probe")).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        for (key, value) in [
            ("user.email", "seat@example.invalid"),
            ("user.name", "seat"),
            ("commit.gpgsign", "false"),
            ("core.hooksPath", "/dev/null"),
        ] {
            git(&root, &["config", key, value]);
        }
        std::fs::write(
            root.join("Software/probe/issues.org"),
            "* TODO [#C] a\n:PROPERTIES:\n:ID:         probe-a\n:END:\n",
        )
        .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "seed"]);
        git(
            &root,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&root, &["push", "-q", "-u", "origin", "HEAD"]);
        (root, origin)
    }

    fn isolate(root: &Path) {
        unsafe {
            std::env::set_var("VISSUE_ROOT", root);
            std::env::remove_var("ISSUE_ROOT");
            std::env::remove_var("LJOS_TRACKER_GIT");
            std::env::set_var("PACKSET_URL", "off");
        }
    }

    #[test]
    fn the_sync_line_names_the_count_and_the_fix() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, _) = checkout(dir.path());
        isolate(&root);
        assert!(super::tracker_sync_line().is_none());

        git(&root, &["commit", "-q", "--allow-empty", "-m", "again"]);
        let line = super::tracker_sync_line().unwrap();
        assert!(line.starts_with("tracker: 1 unpushed; fix: "), "{line}");
        assert!(line.contains("git -C "), "{line}");
        assert!(line.contains(" push\n"), "{line}");
        assert!(!line.contains("uncommitted"), "{line}");

        std::fs::write(root.join("Software/probe/issues.org"), "* DONE [#C] a\n").unwrap();
        std::fs::create_dir_all(root.join("Software/probe/issues")).unwrap();
        std::fs::write(root.join("Software/probe/issues/extra.org"), "* TODO b\n").unwrap();
        let both = super::tracker_sync_line().unwrap();
        assert!(both.contains("1 unpushed, 2 uncommitted edits"), "{both}");
        assert!(both.contains("add -u -- ."), "{both}");
        assert!(both.contains("commit -m "), "{both}");
        assert!(both.contains(" push\n"), "{both}");

        unsafe { std::env::set_var("LJOS_TRACKER_GIT", "off") };
        let off = super::tracker_sync_line().unwrap();
        assert!(off.contains("1 unpushed"), "{off}");
        assert!(!off.contains("uncommitted"), "{off}");

        unsafe { std::env::remove_var("LJOS_TRACKER_GIT") };
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "edits"]);
        git(&root, &["push", "-q"]);
        assert!(super::tracker_sync_line().is_none());

        let full =
            "tracker: 1 unpushed; fix: git -C 'x' push\n== doctor\nok\tpack\tanswers\n".to_string();
        let short = crate::compact_sitting(&full, "probe-a");
        assert!(
            short.starts_with("tracker: 1 unpushed; fix: git -C 'x' push\n"),
            "{short}"
        );
    }

    #[test]
    fn finish_pushes_when_the_config_allows_it() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, origin) = checkout(dir.path());
        isolate(&root);
        git(
            &root,
            &["commit", "-q", "--allow-empty", "-m", "local only"],
        );
        let said = super::push_leftover();
        assert!(said.contains("pushed 1"), "{said}");
        let ahead = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-list", "--count", "origin/main..HEAD"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&ahead.stdout).trim(), "0");
        let _ = origin;

        git(&root, &["commit", "-q", "--allow-empty", "-m", "held"]);
        unsafe { std::env::set_var("LJOS_TRACKER_GIT", "commit") };
        assert!(super::push_leftover().is_empty());
        let still = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-list", "--count", "origin/main..HEAD"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&still.stdout).trim(), "1");
    }
}
