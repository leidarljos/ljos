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
//! Two rules follow, and this module is both of them. A tracker is never
//! made inside a checkout of something else: its files are not that
//! project's files, and the first `git add -A` there would commit them. And
//! every habitat ljos starts is told which tracker it means, so the
//! directory a command happened to run in cannot stand in for it.

use std::path::{Path, PathBuf};

/// The directory under the tracker root that holds one directory per
/// project: `VISSUE_PREFIX`, or vissue's `Software`.
#[must_use]
pub fn prefix() -> String {
    std::env::var("VISSUE_PREFIX")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| vissue_core::DEFAULT_PREFIX.to_string())
}

/// Whether `dir` is the tracker a caller standing in it means: it carries
/// a `vissue.toml`. This is vissue's own test, and the one case where the
/// working directory outranks the seat's configured root.
#[must_use]
pub fn named_tracker(dir: &Path) -> bool {
    dir.join("vissue.toml").is_file()
}

/// Whether `dir` holds a tracker: a `vissue.toml`, or the prefix directory
/// vissue would file projects under. A guessed root passes vissue's
/// `require_tracker` on either, so either is enough to make a directory
/// pass for a tracker.
#[must_use]
pub fn holds_a_tracker(dir: &Path) -> bool {
    named_tracker(dir) || dir.join(prefix()).is_dir()
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
/// root, or one that holds a tracker, is the tracker's own and not a
/// product's.
#[must_use]
pub fn product_repo(root: &Path) -> Option<PathBuf> {
    let top = work_tree(root)?;
    (top != root && !holds_a_tracker(&top)).then_some(top)
}

/// Whether `repo` already keeps `root` out of itself, by a `.gitignore`
/// rule or any other exclude. A tracker under an ignored path is not in
/// that project's files and nothing there will commit it.
///
/// A git that cannot be run, or a path it cannot judge, answers no, so the
/// caller refuses rather than writing into a checkout on a guess.
#[must_use]
pub fn kept_out(repo: &Path, root: &Path) -> bool {
    std::process::Command::new("git")
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
/// child inherits that one, nor when the working directory carries a
/// `vissue.toml`: standing in a tracker means that tracker, and this keeps
/// vissue's own rule. Nothing is set either when the seat configured no
/// root, where there is nothing to name.
pub fn bind_root(cmd: &mut std::process::Command) {
    if named_in_env() {
        return;
    }
    let here = std::env::current_dir().unwrap_or_default();
    if named_tracker(&here) {
        return;
    }
    if let Some(root) = configured() {
        cmd.env("VISSUE_ROOT", root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(at: &Path) {
        std::fs::create_dir_all(at.join(".git")).unwrap();
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
        repo(&under);
        let kept = under.join("tracker");
        std::fs::create_dir_all(kept.join("Software")).unwrap();
        std::fs::create_dir_all(under.join("Software")).unwrap();
        assert_eq!(product_repo(&kept), None, "the checkout holds a tracker");
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
        let out = std::process::Command::new("git")
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
