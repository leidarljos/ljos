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
    // A pin inside the checkout this command was run from is how a later
    // commit landed in that product. Leave the root unset and let
    // [`aim_tracker`] name a tracker outside it, or refuse.
    if product_checkout(&root).is_some() {
        return;
    }
    std::env::set_var("VISSUE_ROOT", &root);
    *PINNED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(root);
}

/// The checkout a commit of `dir` would land in, when that checkout is not
/// a tracker. A directory in no checkout, a tracker checkout, and a path
/// the checkout ignores are `None`.
///
/// A checkout counts as the tracker when `VISSUE_ROOT` or `ISSUE_ROOT`
/// names it, when it is the seat's tracker, when [`is_tracker`] holds, or
/// when it already holds tracker issues. An empty prefix directory is
/// none of those. A board file with no issue id is not a tracker issue.
#[must_use]
pub fn product_checkout(dir: &Path) -> Option<PathBuf> {
    let top = work_tree(dir)?;
    if intrinsic_tracker(&top) {
        return None;
    }
    if top != dir && kept_out(&top, dir) {
        return None;
    }
    Some(top)
}

/// Whether `dir` is the tracker rather than a product checkout.
///
/// [`is_tracker`] is a `vissue.toml` at `HEAD`, a root the seat named, or
/// `LJOS_TRACKER_REMOTE`. `VISSUE_ROOT` and `ISSUE_ROOT` are named roots,
/// so a checkout they name is the tracker even when this process pinned
/// the value. The directory `ljos onboard` makes is the seat's tracker
/// before that layout is committed. Issue files already in the checkout
/// are the tracker a commit is writing.
fn intrinsic_tracker(dir: &Path) -> bool {
    is_tracker(dir) || seat_root_is(dir) || holds_tracker_issues(dir)
}

/// Whether `dir` is the directory `ljos onboard` makes for this seat.
fn seat_root_is(dir: &Path) -> bool {
    default_root().is_some_and(|root| same_path(dir, &root))
}

fn same_path(dir: &Path, root: &Path) -> bool {
    let Some(here) = std::fs::canonicalize(dir).ok() else {
        return false;
    };
    std::fs::canonicalize(root).ok().as_ref() == Some(&here)
}

/// Prefix directories that can hold a project's issues. `Software` is the
/// default. `VISSUE_PREFIX` and `vissue.toml` name another when set.
fn tracker_prefixes(dir: &Path) -> Vec<String> {
    let mut prefixes = Vec::new();
    if let Ok(prefix) = std::env::var("VISSUE_PREFIX") {
        let prefix = prefix.trim();
        if !prefix.is_empty() {
            prefixes.push(prefix.to_string());
        }
    }
    if dir.join("vissue.toml").is_file() {
        if let Some(prefix) = prefix_at(dir) {
            if !prefixes.iter().any(|have| have == &prefix) {
                prefixes.push(prefix);
            }
        }
    }
    if !prefixes.iter().any(|have| have == "Software") {
        prefixes.push("Software".to_string());
    }
    prefixes
}

/// Whether `dir` already holds a tracker issue. The file carries an
/// `:ID:` property. A heading with no id is a board a clone can carry,
/// and that board does not make the checkout the tracker.
fn holds_tracker_issues(dir: &Path) -> bool {
    if issue_file(&dir.join("issues.org")) || issue_dir(&dir.join("issues")) {
        return true;
    }
    tracker_prefixes(dir).iter().any(|prefix| {
        let Ok(projects) = std::fs::read_dir(dir.join(prefix)) else {
            return false;
        };
        projects.flatten().any(|entry| {
            let path = entry.path();
            issue_file(&path.join("issues.org")) || issue_dir(&path.join("issues"))
        })
    })
}

fn issue_dir(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| issue_file(&entry.path()))
}

/// An org file whose properties name an issue id.
fn issue_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("org"))
        && std::fs::read_to_string(path).is_ok_and(|text| {
            text.lines().any(|line| {
                let line = line.trim_start();
                let Some(rest) = line.strip_prefix(":ID:") else {
                    return false;
                };
                rest.is_empty() || rest.starts_with(char::is_whitespace)
            })
        })
}

/// Drop a pin that sits in a product checkout, so vissue does not write
/// there because this process named it.
fn drop_pin_inside_product() {
    let pinned = PINNED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let Some(root) = pinned else {
        return;
    };
    if product_checkout(&root).is_none() {
        return;
    }
    let text = root.display().to_string();
    for key in ["ISSUE_ROOT", "VISSUE_ROOT"] {
        if std::env::var(key).ok().as_deref() == Some(text.as_str()) {
            std::env::remove_var(key);
        }
    }
    *PINNED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// The root vissue would use from `here`, and whether that root is only a
/// guess. The order is vissue's: the environment, a `vissue.toml` in
/// `here`, the seat's configured root, then `here`.
fn root_vissue_would_use(here: &Path) -> (PathBuf, bool) {
    for key in ["ISSUE_ROOT", "VISSUE_ROOT"] {
        if let Ok(raw) = std::env::var(key) {
            if !raw.is_empty() {
                return (crate::expand(&raw), false);
            }
        }
    }
    if here.join("vissue.toml").is_file() {
        return (here.to_path_buf(), false);
    }
    if let Some(root) = configured().filter(|path| path.is_dir()) {
        return (root, false);
    }
    (here.to_path_buf(), true)
}

/// Whether vissue would create or edit issues under `root`. A missing
/// directory is not written. A guess is written only when it already
/// looks like a tracker; a named root that exists is written.
fn accepts_writes(root: &Path, guessed: bool) -> bool {
    if !root.is_dir() {
        return false;
    }
    if !guessed {
        return true;
    }
    root.join("vissue.toml").is_file()
        || prefix_at(root).is_some_and(|prefix| root.join(prefix).is_dir())
}

/// A tracker the seat already has, outside `product`. The configured root
/// comes first, then the directory `ljos onboard` makes.
fn seat_outside(product: &Path) -> Option<PathBuf> {
    let mut found = Vec::new();
    if let Some(root) = configured() {
        found.push(root);
    }
    if let Some(root) = default_root() {
        found.push(root);
    }
    found.into_iter().find(|root| {
        root.is_dir()
            && !rooted_in(product, root)
            && product_checkout(root).is_none()
            && (intrinsic_tracker(root)
                || root.join("vissue.toml").is_file()
                || root.join("Software").is_dir())
    })
}

/// Whether `root` is `product` or a directory inside it.
fn rooted_in(product: &Path, root: &Path) -> bool {
    let canon = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let product = canon(product);
    let root = canon(root);
    root == product || root.starts_with(&product)
}

/// One line a tracker verb prints instead of writing `repo`. It names the
/// setting that chooses the tracker.
#[must_use]
pub fn product_write_refusal(repo: &Path) -> String {
    format!(
        "set VISSUE_ROOT=DIR to name the tracker; {} is a product checkout and this command will not write there",
        repo.display()
    )
}

/// Point a tracker verb at the seat's tracker, or refuse when the write
/// would land in the product checkout it was run from.
///
/// A root the person named is kept. A pin inside that checkout is dropped.
/// The seat's configured tracker, then the onboard directory, is used when
/// it sits outside the checkout. With neither, the verb stops before vissue
/// runs. A directory vissue would not write is left alone, so a command
/// standing in an ordinary checkout still runs.
///
/// # Errors
///
/// The write would enter a product checkout and the seat has no tracker
/// outside it. The error is one line.
pub fn aim_tracker() -> Result<(), String> {
    let here = std::env::current_dir().map_err(|err| err.to_string())?;
    aim_tracker_from(&here)
}

/// [`aim_tracker`] for a command whose working directory is `here`.
///
/// # Errors
///
/// As [`aim_tracker`].
pub fn aim_tracker_from(here: &Path) -> Result<(), String> {
    drop_pin_inside_product();
    let (root, guessed) = root_vissue_would_use(here);
    if !accepts_writes(&root, guessed) {
        return Ok(());
    }
    let Some(product) = product_checkout(&root) else {
        return Ok(());
    };
    if person_named_root().is_none() {
        if let Some(seat) = seat_outside(&product) {
            std::env::set_var("VISSUE_ROOT", &seat);
            *PINNED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(seat);
            return Ok(());
        }
    }
    Err(product_write_refusal(&product))
}

/// `ISSUE_ROOT` or `VISSUE_ROOT` when the person set it. A pin is not that.
fn person_named_root() -> Option<PathBuf> {
    for key in ["ISSUE_ROOT", "VISSUE_ROOT"] {
        let Ok(raw) = std::env::var(key) else {
            continue;
        };
        if raw.is_empty() || pinned(&raw) {
            continue;
        }
        return Some(crate::expand(&raw));
    }
    None
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

    /// ljos-76mj: a product checkout is not where a tracker verb writes
    /// when nothing names a tracker outside it.
    #[test]
    fn a_verb_in_a_product_checkout_refuses_instead_of_using_it() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let product = dir.path().join("myapp");
        git(&product, &["init", "-q"]);
        std::fs::create_dir_all(product.join("Software")).unwrap();
        let inside = product.join("data/vissue/tracker");
        std::fs::create_dir_all(inside.join("Software")).unwrap();
        let saved = (
            std::env::var_os("XDG_DATA_HOME"),
            std::env::var_os("VISSUE_ROOT"),
            std::env::var_os("ISSUE_ROOT"),
            std::env::var_os("VISSUE_CONFIG"),
        );
        std::env::set_var("XDG_DATA_HOME", product.join("data"));
        std::env::remove_var("VISSUE_ROOT");
        std::env::remove_var("ISSUE_ROOT");
        std::env::remove_var("VISSUE_CONFIG");
        let err = aim_tracker_from(&product).unwrap_err();
        let named = std::env::var("VISSUE_ROOT").ok();
        let restore = |key: &str, value: Option<std::ffi::OsString>| match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        };
        restore("XDG_DATA_HOME", saved.0);
        restore("VISSUE_ROOT", saved.1);
        restore("ISSUE_ROOT", saved.2);
        restore("VISSUE_CONFIG", saved.3);
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.contains("set VISSUE_ROOT=DIR"), "{err}");
        assert!(err.contains("myapp"), "{err}");
        assert!(
            named.is_none_or(|root| !root.contains("myapp")),
            "the product was named as the tracker"
        );
    }

    /// The seat's tracker, outside the checkout, is the one a verb uses.
    #[test]
    fn a_verb_in_a_product_checkout_uses_the_seat_tracker() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let product = dir.path().join("myapp");
        git(&product, &["init", "-q"]);
        std::fs::create_dir_all(product.join("Software")).unwrap();
        // vissue takes this checkout before the seat file, because
        // `vissue.toml` is here. The verb has to name the seat's tracker.
        std::fs::write(product.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let tracker = dir.path().join("vault");
        std::fs::create_dir_all(tracker.join("Software")).unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(
            &cfg,
            format!(
                "root = {}\n",
                toml::Value::String(tracker.display().to_string())
            ),
        )
        .unwrap();
        let saved = (
            std::env::var_os("VISSUE_ROOT"),
            std::env::var_os("ISSUE_ROOT"),
            std::env::var_os("VISSUE_CONFIG"),
        );
        std::env::remove_var("VISSUE_ROOT");
        std::env::remove_var("ISSUE_ROOT");
        std::env::set_var("VISSUE_CONFIG", &cfg);
        let aimed = aim_tracker_from(&product);
        let set = std::env::var("VISSUE_ROOT").ok();
        match saved.0 {
            Some(value) => std::env::set_var("VISSUE_ROOT", value),
            None => std::env::remove_var("VISSUE_ROOT"),
        }
        match saved.1 {
            Some(value) => std::env::set_var("ISSUE_ROOT", value),
            None => std::env::remove_var("ISSUE_ROOT"),
        }
        match saved.2 {
            Some(value) => std::env::set_var("VISSUE_CONFIG", value),
            None => std::env::remove_var("VISSUE_CONFIG"),
        }
        assert!(aimed.is_ok(), "{aimed:?}");
        assert_eq!(set.as_deref(), Some(tracker.to_str().unwrap()));
    }

    fn saved_tracker_env() -> [(&'static str, Option<std::ffi::OsString>); 5] {
        [
            ("VISSUE_ROOT", std::env::var_os("VISSUE_ROOT")),
            ("ISSUE_ROOT", std::env::var_os("ISSUE_ROOT")),
            ("VISSUE_CONFIG", std::env::var_os("VISSUE_CONFIG")),
            ("VISSUE_PREFIX", std::env::var_os("VISSUE_PREFIX")),
            (
                "LJOS_TRACKER_REMOTE",
                std::env::var_os("LJOS_TRACKER_REMOTE"),
            ),
        ]
    }

    fn restore_tracker_env(saved: [(&'static str, Option<std::ffi::OsString>); 5]) {
        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    fn clear_tracker_env() {
        for key in [
            "VISSUE_ROOT",
            "ISSUE_ROOT",
            "VISSUE_CONFIG",
            "VISSUE_PREFIX",
            "LJOS_TRACKER_REMOTE",
        ] {
            std::env::remove_var(key);
        }
    }

    /// `VISSUE_ROOT` names the checkout. It is the tracker with no
    /// `vissue.toml` and no issue file.
    #[test]
    fn a_checkout_named_by_vissue_root_is_not_a_product() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("vault");
        git(&root, &["init", "-q"]);
        std::fs::write(root.join("README"), "tracker\n").unwrap();
        let saved = saved_tracker_env();
        clear_tracker_env();
        std::env::set_var("VISSUE_ROOT", &root);
        let found = product_checkout(&root);
        restore_tracker_env(saved);
        assert_eq!(found, None);
    }

    /// The directory `ljos onboard` makes is the seat's tracker, including
    /// when it is its own git checkout and nothing else names it.
    #[test]
    fn the_seat_tracker_directory_is_not_a_product_checkout() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let data = dir.path().join("data");
        let root = data.join("vissue/tracker");
        git(&root, &["init", "-q"]);
        let saved_data = std::env::var_os("XDG_DATA_HOME");
        let saved = saved_tracker_env();
        clear_tracker_env();
        std::env::set_var("XDG_DATA_HOME", &data);
        let found = product_checkout(&root);
        restore_tracker_env(saved);
        match saved_data {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        assert_eq!(found, None);
    }

    /// A checkout that already holds an issue id is the tracker the commit
    /// writes. The path may be a project board or a ledger file, and it
    /// may be reached through a symlink. A heading with no id is a board
    /// a clone carries, and that checkout stays a product.
    #[test]
    fn a_checkout_holding_tracker_issues_is_not_a_product() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("tracker");
        git(&root, &["init", "-q"]);
        std::fs::create_dir_all(root.join("Software/probe")).unwrap();
        std::fs::write(
            root.join("Software/probe/issues.org"),
            "* TODO [#C] Probe\n:PROPERTIES:\n:ID:         probe-a1b2\n:END:\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("Software/proj/issues")).unwrap();
        std::fs::write(
            root.join("Software/proj/issues/proj-nnnn.org"),
            "* TODO [#C] Fresh\n:PROPERTIES:\n:ID:         proj-nnnn\n:END:\n",
        )
        .unwrap();
        let saved = saved_tracker_env();
        clear_tracker_env();
        let from_board = product_checkout(&root.join("Software/probe"));
        let from_ledger = product_checkout(&root.join("Software/proj/issues"));
        let link_holder = dir.path().join("link-holder");
        std::fs::create_dir_all(&link_holder).unwrap();
        let link = link_holder.join("tracker");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        std::fs::create_dir_all(root.join("issues")).unwrap();
        std::fs::write(
            root.join("issues/proj-pppp.org"),
            "* TODO [#C] Linked\n:PROPERTIES:\n:ID:         proj-pppp\n:END:\n",
        )
        .unwrap();
        let from_link = product_checkout(&link.join("issues"));

        let boards = dir.path().join("boards");
        git(&boards, &["init", "-q"]);
        std::fs::create_dir_all(boards.join("Software/app")).unwrap();
        std::fs::write(boards.join("Software/app/issues.org"), "* TODO x\n").unwrap();
        let board_only = product_checkout(&boards);
        restore_tracker_env(saved);
        assert_eq!(from_board, None);
        assert_eq!(from_ledger, None);
        assert_eq!(from_link, None);
        assert_eq!(board_only.as_deref(), Some(boards.as_path()));
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
