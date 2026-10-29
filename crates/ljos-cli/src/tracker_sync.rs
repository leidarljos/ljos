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
    if unpushed == 0 && edits.count == 0 {
        return None;
    }
    let phrase = counts_phrase(unpushed, edits.count);
    let fix = fix_command(root, unpushed, &edits);
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
    // Resolve what an implicit push would update before doing any network
    // write. Only the primary remote's default branch is published.
    let Some(destination) = safe_push_destination(&root) else {
        return String::new();
    };
    match crate::tracker_push::publish_explicit(&root, &destination.remote, &destination.branch) {
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

struct PushDestination {
    remote: String,
    branch: String,
}

/// The destination Git's configured implicit push would use, but only
/// when it is exactly `origin`'s default branch.
fn safe_push_destination(root: &Path) -> Option<PushDestination> {
    let push = crate::git_ok_stdout(root, &["rev-parse", "--symbolic-full-name", "@{push}"])?;
    let push = push.trim();
    let default = remote_default_branch(root, "origin")?;
    let expected = format!("refs/remotes/origin/{default}");
    if push != expected {
        return None;
    }
    Some(PushDestination {
        remote: "origin".to_string(),
        branch: default,
    })
}

fn remote_default_branch(root: &Path, remote: &str) -> Option<String> {
    let remote_head = format!("refs/remotes/{remote}/HEAD");
    if let Some(target) = crate::git_ok_stdout(root, &["symbolic-ref", "--quiet", &remote_head]) {
        let prefix = format!("refs/remotes/{remote}/");
        if let Some(branch) = target.trim().strip_prefix(&prefix) {
            if !branch.is_empty() {
                return Some(branch.to_string());
            }
        }
    }
    let out = crate::git_ok_stdout(root, &["ls-remote", "--symref", remote, "HEAD"])?;
    out.lines().find_map(|line| {
        let target = line.strip_prefix("ref: refs/heads/")?;
        let (branch, name) = target.split_once('\t')?;
        (name == "HEAD" && !branch.is_empty()).then(|| branch.to_string())
    })
}

struct OrgEdits {
    count: usize,
    untracked: Vec<String>,
}

fn org_edits(root: &Path) -> OrgEdits {
    if git_leaves_edits() {
        return OrgEdits {
            count: 0,
            untracked: Vec::new(),
        };
    }
    let Some(status) = crate::git_ok_stdout(
        root,
        &["status", "--porcelain", "--untracked-files=all", "--", "."],
    ) else {
        return OrgEdits {
            count: 0,
            untracked: Vec::new(),
        };
    };
    let mut edits = OrgEdits {
        count: 0,
        untracked: Vec::new(),
    };
    for line in status.lines() {
        let Some(path) = line.get(3..) else {
            continue;
        };
        let path = path.rsplit(" -> ").next().unwrap_or(path).trim_matches('"');
        if !path.ends_with(".org") {
            continue;
        }
        edits.count += 1;
        if line.starts_with("??") {
            edits.untracked.push(path.to_string());
        }
    }
    edits
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

fn fix_command(root: &Path, unpushed: u64, edits: &OrgEdits) -> String {
    let quoted = crate::tools::sh_quote(&root.display().to_string());
    let mut parts = Vec::new();
    if edits.count > 0 {
        let message = crate::tools::sh_quote("tracker: commit issue edits");
        parts.push(format!("git -C {quoted} add -u -- ."));
        if !edits.untracked.is_empty() {
            let paths = edits
                .untracked
                .iter()
                .map(|path| crate::tools::sh_quote(path))
                .collect::<Vec<_>>()
                .join(" ");
            parts.push(format!("git -C {quoted} add -- {paths}"));
        }
        parts.push(format!("git -C {quoted} commit -m {message}"));
    }
    // A commit of the edits is itself unpushed, so the fix publishes
    // only when the configured destination is the permitted default.
    if unpushed > 0 || (edits.count > 0 && has_remote(root)) {
        if let Some(destination) = safe_push_destination(root) {
            let remote = crate::tools::sh_quote(&destination.remote);
            let spec = crate::tools::sh_quote(&format!("HEAD:refs/heads/{}", destination.branch));
            parts.push(format!("git -C {quoted} push {remote} {spec}"));
        }
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
        assert!(
            line.contains("push 'origin' 'HEAD:refs/heads/main'"),
            "{line}"
        );
        assert!(!line.contains("uncommitted"), "{line}");

        std::fs::write(root.join("Software/probe/issues.org"), "* DONE [#C] a\n").unwrap();
        std::fs::create_dir_all(root.join("Software/probe/issues")).unwrap();
        std::fs::write(root.join("Software/probe/issues/extra.org"), "* TODO b\n").unwrap();
        let both = super::tracker_sync_line().unwrap();
        assert!(both.contains("1 unpushed, 2 uncommitted edits"), "{both}");
        assert!(both.contains("add -u -- ."), "{both}");
        assert!(
            both.contains("add -- 'Software/probe/issues/extra.org'"),
            "{both}"
        );
        assert!(!both.contains("add -- ."), "{both}");
        assert!(both.contains("commit -m "), "{both}");
        assert!(
            both.contains("push 'origin' 'HEAD:refs/heads/main'"),
            "{both}"
        );

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

    fn review_heads(origin: &Path) -> String {
        let out = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(origin)
            .args([
                "for-each-ref",
                "--format=%(refname) %(objectname:short)",
                "refs/heads",
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[test]
    fn review_repro_finish_pushes_a_new_branch_that_is_not_the_default() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, origin) = checkout(dir.path());
        isolate(&root);
        git(&root, &["remote", "set-head", "origin", "main"]);
        git(&root, &["checkout", "-q", "-b", "wip"]);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "on wip"]);
        let said = super::push_leftover();
        let heads = review_heads(&origin);
        assert!(!heads.contains("refs/heads/wip"), "said={said:?}\n{heads}");
        assert!(said.is_empty(), "{said}");
    }

    #[test]
    fn review_repro_finish_pushes_a_tracked_feature_branch() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, origin) = checkout(dir.path());
        isolate(&root);
        git(&root, &["checkout", "-q", "-b", "feature"]);
        git(&root, &["push", "-q", "-u", "origin", "feature"]);
        let before = review_heads(&origin);
        git(
            &root,
            &["commit", "-q", "--allow-empty", "-m", "feature work"],
        );
        let said = super::push_leftover();
        let after = review_heads(&origin);
        assert_eq!(before, after, "said={said:?}");
        assert!(said.is_empty(), "{said}");
    }

    #[test]
    fn review_repro_the_suggested_fix_leaves_a_new_issue_file() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, _) = checkout(dir.path());
        isolate(&root);
        std::fs::create_dir_all(root.join("Software/probe/issues")).unwrap();
        std::fs::write(root.join("Software/probe/issues/extra.org"), "* TODO b\n").unwrap();
        let line = super::tracker_sync_line().unwrap();
        let fix = line.split_once("fix: ").unwrap().1.trim().to_string();
        let ran = std::process::Command::new("sh")
            .arg("-c")
            .arg(&fix)
            .output()
            .unwrap();
        let again = super::tracker_sync_line();
        assert!(
            again.is_none(),
            "fix={fix}\nexit={:?} {}\nstill: {again:?}",
            ran.status.code(),
            String::from_utf8_lossy(&ran.stderr)
        );
    }

    #[test]
    fn review_repro_a_feature_branch_with_no_upstream_counts_as_unpushed() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, origin) = checkout(dir.path());
        isolate(&root);
        git(&root, &["remote", "set-head", "origin", "main"]);
        git(&root, &["checkout", "-q", "-b", "feature"]);
        git(
            &root,
            &["commit", "-q", "--allow-empty", "-m", "local only"],
        );
        let line = super::tracker_sync_line().unwrap();
        assert!(line.contains("1 unpushed"), "{line}");
        let said = super::push_leftover();
        assert!(said.is_empty(), "{said}");
        let heads = review_heads(&origin);
        assert!(!heads.contains("refs/heads/feature"), "{heads}");
    }

    #[test]
    fn review2_main_tracking_a_feature_branch_pushes_the_feature() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, origin) = checkout(dir.path());
        isolate(&root);
        git(&root, &["remote", "set-head", "origin", "main"]);
        git(&root, &["push", "-q", "origin", "main:feature"]);
        git(&root, &["fetch", "-q", "origin"]);
        git(&root, &["branch", "-q", "-u", "origin/feature"]);
        git(&root, &["config", "push.default", "upstream"]);
        let before = review_heads(&origin);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "on main"]);
        let said = super::push_leftover();
        let after = review_heads(&origin);
        eprintln!("REVIEW2 said={said:?}\nbefore={before}\nafter={after}");
        assert_eq!(before, after, "said={said:?}");
    }

    #[test]
    fn review2_main_tracking_a_feature_branch_default_push_config() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, origin) = checkout(dir.path());
        isolate(&root);
        git(&root, &["remote", "set-head", "origin", "main"]);
        git(&root, &["push", "-q", "origin", "main:feature"]);
        git(&root, &["fetch", "-q", "origin"]);
        git(&root, &["branch", "-q", "-u", "origin/feature"]);
        let before = review_heads(&origin);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "on main"]);
        let said = super::push_leftover();
        let after = review_heads(&origin);
        eprintln!("REVIEW2 said={said:?}\nbefore={before}\nafter={after}");
        assert_eq!(before, after, "said={said:?}");
        assert!(said.is_empty(), "{said}");
    }

    #[test]
    fn review2_main_tracking_another_remote_pushes_there() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, origin) = checkout(dir.path());
        isolate(&root);
        let fork = dir.path().join("fork.git");
        git(
            dir.path(),
            &["init", "-q", "--bare", "-b", "main", fork.to_str().unwrap()],
        );
        git(&root, &["remote", "add", "fork", fork.to_str().unwrap()]);
        git(&root, &["remote", "set-head", "origin", "main"]);
        git(&root, &["push", "-q", "-u", "fork", "main"]);
        let before_fork = review_heads(&fork);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "on main"]);
        let said = super::push_leftover();
        let after_fork = review_heads(&fork);
        let after_origin = review_heads(&origin);
        eprintln!("REVIEW2 said={said:?}\nfork before={before_fork}\nfork after={after_fork}\norigin={after_origin}");
        assert_eq!(before_fork, after_fork, "said={said:?}");
    }

    #[test]
    fn review2_the_fix_commits_and_pushes_a_stray_file() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let (root, origin) = checkout(dir.path());
        isolate(&root);
        std::fs::write(root.join("Software/probe/issues.org"), "* DONE [#C] a\n").unwrap();
        std::fs::write(root.join("scratch.env"), "GITHUB_TOKEN=ghp_notreal\n").unwrap();
        let line = super::tracker_sync_line().unwrap();
        eprintln!("REVIEW2 line={line}");
        let fix = line.split_once("fix: ").unwrap().1.trim().to_string();
        let ran = std::process::Command::new("sh")
            .arg("-c")
            .arg(&fix)
            .output()
            .unwrap();
        let tree = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&origin)
            .args(["ls-tree", "-r", "--name-only", "main"])
            .output()
            .unwrap();
        let tree = String::from_utf8_lossy(&tree.stdout).into_owned();
        eprintln!(
            "REVIEW2 exit={:?} origin main tree:\n{tree}",
            ran.status.code()
        );
        assert!(!tree.contains("scratch.env"), "{tree}");
    }
}
