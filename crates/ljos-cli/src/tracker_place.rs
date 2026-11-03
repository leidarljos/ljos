//! Where the tracker lives, and where it must not.
//!
//! vissue resolves its root from the caller, then `ISSUE_ROOT` or
//! `VISSUE_ROOT`, then the working directory when that directory carries a
//! `vissue.toml`, then the seat's own `vissue/config.toml`, and failing all
//! of those the working directory itself. The last two steps are the
//! trouble. A seat run inside a product checkout stood in a directory that
//! was not its tracker, and a checkout holding a directory named like the
//! project prefix passed for one, so tickets were filed into the product's
//! repository instead of the seat's tracker.
//!
//! Three rules follow, and this module is all of them. One function,
//! [`is_tracker`], decides whether a directory is the tracker, from facts a
//! product checkout does not have by accident. A tracker is never made
//! inside a checkout of something else: its files are not that project's
//! files, and the first `git add -A` there would commit them. And every
//! process and habitat ljos starts is told which tracker it means, so the
//! directory a command happened to run in cannot stand in for it.

use std::path::{Path, PathBuf};

/// Whether `dir` is the tracker. Every check that asks goes through here,
/// and it reads only facts a product checkout does not have by accident:
///
/// - `dir` is a root the seat named: `ISSUE_ROOT`, `VISSUE_ROOT`, the
///   `root` of its vissue configuration, or a layout that configuration
///   routes projects to;
/// - `dir` is a checkout whose origin is the remote `LJOS_TRACKER_REMOTE`
///   names;
/// - `dir` holds a `vissue.toml` vissue reads, beside the prefix directory
///   that file or `VISSUE_PREFIX` names, and that `vissue.toml` is
///   committed in the checkout `dir` is in. A directory in no checkout is
///   nobody's files, so there the file alone counts.
///
/// An untracked `vissue.toml`, a bare `Software/`, or the boards a clone of
/// some other project carries do not make a tracker.
#[must_use]
pub fn is_tracker(dir: &Path) -> bool {
    let canon = |path: &Path| std::fs::canonicalize(path).ok();
    let Some(here) = canon(dir) else {
        return false;
    };
    named_roots()
        .iter()
        .any(|root| canon(root).as_ref() == Some(&here))
        || committed_layout(dir)
        || serves_named_remote(dir)
}

/// The tracker roots the seat named, in the environment and in its vissue
/// configuration.
fn named_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["ISSUE_ROOT", "VISSUE_ROOT"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .filter(|raw| !raw.is_empty())
        .map(|raw| crate::expand(&raw))
        .collect();
    roots.extend(configured());
    // An empty default root is no directory, so every layout after it is
    // one the configuration routes to.
    if let Ok(router) = vissue_core::Router::load(vissue_core::Layout::new(PathBuf::new(), "")) {
        roots.extend(
            router
                .unique_layouts()
                .into_iter()
                .skip(1)
                .map(|layout| layout.root().to_path_buf()),
        );
    }
    roots
}

/// The prefix vissue reads under `dir`: `VISSUE_PREFIX`, else the one
/// `dir/vissue.toml` names, else `Software`. `None` when that file is
/// there and does not read.
fn prefix_at(dir: &Path) -> Option<String> {
    let layout = vissue_core::config::layout_at(dir).ok()?;
    Some(
        std::env::var("VISSUE_PREFIX")
            .ok()
            .filter(|raw| !raw.is_empty())
            .unwrap_or_else(|| layout.prefix().to_string()),
    )
}

fn committed_layout(dir: &Path) -> bool {
    if !dir.join("vissue.toml").is_file() {
        return false;
    }
    let Some(prefix) = prefix_at(dir) else {
        return false;
    };
    dir.join(prefix).is_dir()
        && (work_tree(dir).is_none()
            || git_in(dir, &["cat-file", "-e", "HEAD:./vissue.toml"]).is_some())
}

fn serves_named_remote(dir: &Path) -> bool {
    let Some(named) = std::env::var("LJOS_TRACKER_REMOTE")
        .ok()
        .filter(|raw| !raw.trim().is_empty())
    else {
        return false;
    };
    dir.join(".git").exists()
        && git_in(dir, &["config", "--get", "remote.origin.url"])
            .is_some_and(|url| crate::tracker_attach::same_remote(&url, &named))
}

/// What git prints for `args` run in `dir` alone, when it succeeds. A hook
/// exports the repository it runs for, and that would answer instead.
fn git_in(dir: &Path, args: &[&str]) -> Option<String> {
    let out = crate::git_cmd()
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The git work tree `dir` stands in, found by walking up for a `.git`
/// entry. `.git` is a directory in an ordinary checkout and a file in a
/// worktree or a submodule; both count.
#[must_use]
pub fn work_tree(dir: &Path) -> Option<PathBuf> {
    let mut at = Some(dir);
    while let Some(here) = at {
        if here.join(".git").exists() {
            return Some(here.to_path_buf());
        }
        at = here.parent();
    }
    None
}

/// The checkout of something else that a tracker at `root` would sit
/// inside, or `None` when it sits in no checkout, or in its own.
///
/// A tracker is a git repository itself, so the work tree whose top is the
/// root, or one that is a tracker, is the tracker's own and not a
/// product's.
#[must_use]
pub fn product_repo(root: &Path) -> Option<PathBuf> {
    let top = work_tree(root)?;
    (top != root && !is_tracker(&top)).then_some(top)
}

/// Whether `repo` already keeps `root` out of itself, by a `.gitignore`
/// rule or any other exclude. A tracker under an ignored path is not in
/// that project's files and nothing there will commit it.
///
/// A git that cannot be run, or a path it cannot judge, answers no, so the
/// caller refuses rather than writing into a checkout on a guess.
#[must_use]
pub fn kept_out(repo: &Path, root: &Path) -> bool {
    crate::git_cmd()
        .arg("-C")
        .arg(repo)
        .arg("check-ignore")
        .arg("-q")
        .arg("--no-index")
        .arg(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// What onboard says instead of making a tracker inside someone else's
/// checkout.
#[must_use]
pub fn refusal(root: &Path, repo: &Path) -> String {
    format!(
        "{} is inside the checkout at {}, and a tracker is not that project's files; \
         set VISSUE_ROOT=DIR outside it, or name one in {}",
        root.display(),
        repo.display(),
        config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "vissue/config.toml".into())
    )
}

/// The seat's own vissue configuration: `VISSUE_CONFIG`, else
/// `$XDG_CONFIG_HOME/vissue/config.toml`.
#[must_use]
pub fn config_path() -> Option<PathBuf> {
    std::env::var_os("VISSUE_CONFIG")
        .filter(|raw| !raw.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::config_base().map(|b| b.join("vissue").join("config.toml")))
}

/// The `root = "..."` line of a vissue config, which is a top-level key.
/// One inside a `[table]` is something else.
#[must_use]
pub fn root_line(text: &str) -> Option<&str> {
    text.lines()
        .take_while(|line| !line.trim_start().starts_with('['))
        .find(|line| {
            line.split('=')
                .next()
                .is_some_and(|key| key.trim() == "root")
        })
}

/// The tracker root the seat's configuration names, with a leading `~`
/// expanded. `None` when there is no configuration or it names none.
#[must_use]
pub fn configured() -> Option<PathBuf> {
    let text = std::fs::read_to_string(config_path()?).ok()?;
    let value: toml::Value = toml::from_str(root_line(&text)?).ok()?;
    let named = value.get("root")?.as_str()?.trim();
    (!named.is_empty()).then(|| crate::expand(named))
}

/// Whether `ISSUE_ROOT` or `VISSUE_ROOT` already names a tracker, which
/// every child inherits and which outranks everything below it.
#[must_use]
pub fn named_in_env() -> bool {
    ["ISSUE_ROOT", "VISSUE_ROOT"]
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|raw| !raw.is_empty()))
}

/// Name the seat's tracker on a child's environment, so the directory the
/// command ran in cannot stand in for it.
///
/// Nothing is set when the environment already names a root, since the
/// child inherits that one, nor when the working directory is a tracker
/// that carries a `vissue.toml`: standing in a tracker means that tracker,
/// and this keeps vissue's own rule. Nothing is set either when the seat
/// configured no root, where there is nothing to name.
pub fn bind_root(cmd: &mut std::process::Command) {
    if named_in_env() {
        return;
    }
    let here = std::env::current_dir().unwrap_or_default();
    if here.join("vissue.toml").is_file() && is_tracker(&here) {
        return;
    }
    if let Some(root) = configured() {
        cmd.env("VISSUE_ROOT", root);
    }
}

/// The root [`pin_root`] set on this process, so `ljos onboard` can tell
/// it from one the person named.
static PINNED: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Name the tracker on this process when vissue's own order would land on
/// a directory [`is_tracker`] rejects, so every read, write and child after
/// this means the same tracker. vissue takes the working directory when it
/// holds a `vissue.toml`, then the seat's configured root, then the working
/// directory as a guess that a `vissue.toml` or a prefix directory there
/// lets through. A product checkout with either was taken for the tracker,
/// and tickets were written and committed into it.
///
/// The configured root is named instead. With none, the root `ljos
/// onboard` makes is named, so a write that finds no tracker there says
/// that is the one it looked for. Nothing is set when the environment or
/// the seat's env file already names a root, or when vissue's own choice
/// is the tracker. `ljos-mcp` pins before it loads that file, and a root
/// named there is the one it means.
pub fn pin_root() {
    if ["ISSUE_ROOT", "VISSUE_ROOT"]
        .iter()
        .any(|key| crate::seat_setting(key).is_some_and(|raw| !raw.is_empty()))
    {
        return;
    }
    let Ok(here) = std::env::current_dir() else {
        return;
    };
    let standing = here.join("vissue.toml").is_file();
    let configured = configured();
    // vissue reads a configured root only when it is a directory.
    let takes_configured = !standing && configured.as_deref().is_some_and(Path::is_dir);
    let takes_here = standing || prefix_at(&here).is_some_and(|prefix| here.join(prefix).is_dir());
    if takes_configured || !takes_here || is_tracker(&here) {
        return;
    }
    let Some(root) = configured.or_else(default_root) else {
        return;
    };
    std::env::set_var("VISSUE_ROOT", &root);
    *PINNED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(root);
}

/// Whether `value` is the root [`pin_root`] named on this process.
#[must_use]
pub fn pinned(value: &str) -> bool {
    PINNED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_deref()
        .is_some_and(|root| root == Path::new(value))
}

/// The tracker `ljos onboard` makes when nothing names one:
/// `$XDG_DATA_HOME/vissue/tracker`.
#[must_use]
pub fn default_root() -> Option<PathBuf> {
    crate::data_base().map(|base| base.join("vissue").join("tracker"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(at: &Path) {
        std::fs::create_dir_all(at.join(".git")).unwrap();
    }

    fn git(at: &Path, args: &[&str]) {
        std::fs::create_dir_all(at).unwrap();
        let out = crate::git_cmd()
            .arg("-C")
            .arg(at)
            .args(["-c", "user.name=seat", "-c", "user.email=seat@example.test"])
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    /// A checkout with `vissue.toml` committed beside its prefix directory.
    fn committed_tracker(at: &Path, toml: &str, prefix: &str) {
        git(at, &["init", "-q"]);
        std::fs::create_dir_all(at.join(prefix)).unwrap();
        std::fs::write(at.join("vissue.toml"), toml).unwrap();
        git(at, &["add", "vissue.toml"]);
        git(at, &["commit", "-q", "-m", "tracker"]);
    }

    /// The complaint: a tracker made under a home directory that was
    /// itself inside a product checkout.
    #[test]
    fn a_tracker_under_a_product_checkout_names_that_checkout() {
        let dir = tempfile::TempDir::new().unwrap();
        let product = dir.path().join("myapp");
        repo(&product);
        let root = product.join(".local/share/vissue/tracker");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(product_repo(&root).as_deref(), Some(product.as_path()));
        let said = refusal(&root, &product);
        assert!(said.contains("myapp"), "{said}");
        assert!(said.contains("VISSUE_ROOT"), "{said}");
    }

    /// A tracker is a git repository of its own. Its own checkout is not a
    /// product's, and neither is one that holds it.
    #[test]
    fn a_trackers_own_repository_is_not_a_product_checkout() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("tracker");
        repo(&root);
        std::fs::write(root.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        assert_eq!(product_repo(&root), None);

        let under = dir.path().join("vault");
        git(&under, &["init", "-q"]);
        let kept = under.join("tracker");
        std::fs::create_dir_all(kept.join("Software")).unwrap();
        std::fs::create_dir_all(under.join("Software")).unwrap();
        assert_eq!(
            product_repo(&kept).as_deref(),
            Some(under.as_path()),
            "an empty Software/ is any project's"
        );
        committed_tracker(&under, "", "Software");
        assert_eq!(product_repo(&kept), None, "the checkout is a tracker");
    }

    /// The accidents that made a product checkout pass for the tracker.
    #[test]
    fn a_product_checkout_is_not_the_tracker_by_accident() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let untracked = dir.path().join("untracked");
        git(&untracked, &["init", "-q"]);
        std::fs::create_dir_all(untracked.join("Software")).unwrap();
        std::fs::write(untracked.join("vissue.toml"), "").unwrap();
        assert!(!is_tracker(&untracked), "an untracked vissue.toml");

        let bare = dir.path().join("bare");
        git(&bare, &["init", "-q"]);
        std::fs::create_dir_all(bare.join("Software")).unwrap();
        assert!(!is_tracker(&bare), "an empty Software/");

        let boards = dir.path().join("boards");
        git(&boards, &["init", "-q"]);
        std::fs::create_dir_all(boards.join("Software/app")).unwrap();
        std::fs::write(boards.join("Software/app/issues.org"), "* TODO x\n").unwrap();
        git(&boards, &["add", "."]);
        git(&boards, &["commit", "-q", "-m", "fixtures"]);
        assert!(!is_tracker(&boards), "boards a clone carries");

        let real = dir.path().join("real");
        committed_tracker(&real, "", "Software");
        assert!(is_tracker(&real));
        let plain = dir.path().join("plain");
        std::fs::create_dir_all(plain.join("Software")).unwrap();
        std::fs::write(plain.join("vissue.toml"), "").unwrap();
        assert!(is_tracker(&plain), "a directory in no checkout is nobody's");
    }

    /// `VISSUE_PREFIX` names the prefix directory the tracker must hold, as
    /// it does for vissue.
    #[test]
    fn a_tracker_under_vissue_prefix_is_the_tracker() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("work");
        committed_tracker(&root, "", "Work");
        assert!(!is_tracker(&root), "no Software/ and no VISSUE_PREFIX");
        let before = std::env::var_os("VISSUE_PREFIX");
        std::env::set_var("VISSUE_PREFIX", "Work");
        let taken = is_tracker(&root);
        match before {
            Some(v) => std::env::set_var("VISSUE_PREFIX", v),
            None => std::env::remove_var("VISSUE_PREFIX"),
        }
        assert!(taken);
    }

    #[test]
    fn a_tracker_in_no_checkout_at_all_is_nobody_s_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("share/vissue/tracker");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(product_repo(&root), None);
    }

    /// A checkout that already ignores the path is not written into by
    /// anything there, so the tracker may stay.
    #[test]
    fn a_path_the_checkout_ignores_is_not_its_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let product = dir.path().join("myapp");
        std::fs::create_dir_all(&product).unwrap();
        let out = crate::git_cmd()
            .args(["init", "-q"])
            .current_dir(&product)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        std::fs::write(product.join(".gitignore"), ".local/\n").unwrap();
        let root = product.join(".local/share/vissue/tracker");
        std::fs::create_dir_all(&root).unwrap();
        assert!(kept_out(&product, &root));
        assert!(!kept_out(&product, &product.join("src/tracker")));
    }

    #[test]
    fn the_root_line_is_the_top_level_one() {
        assert_eq!(
            root_line("root = \"/srv/tracker\"\n[routes]\n"),
            Some("root = \"/srv/tracker\"")
        );
        assert_eq!(root_line("[routes]\nroot = \"/srv/elsewhere\"\n"), None);
        assert_eq!(root_line("prefix = \"Work\"\n"), None);
    }
}
