//! A tracker git repository whose objects cannot be read.
//!
//! A restore can leave a pack with no index, drop the loose objects, and
//! leave HEAD naming a commit that is not there. Every tracker write then
//! failed with git's raw `fatal: bad object HEAD`, and nothing named the
//! repository or a repair. The write stays in the worktree. This module
//! names the repository and the repair. It only reads: it does not run
//! `index-pack`, `fetch`, `read-tree`, `reset`, or `commit`.

use std::path::{Path, PathBuf};
use std::process::Output;

/// The words the tracker git line and the doctor row share, so the row
/// is a warning rather than an absent tracker.
pub(crate) const MARK: &str = "cannot read its objects";

/// `Some` plain line when `said` is a bad object, a missing object, or a
/// pack git could not open. The line names the repository that holds
/// `dir` and points at `ljos doctor`, which prints the repair. `None`
/// when `said` is some other git failure.
pub(crate) fn plain_failure(dir: &Path, said: &str) -> Option<String> {
    if !git_text_is_object_damage(said) {
        return None;
    }
    let root = crate::tracker_push::repo_root(dir);
    Some(format!(
        "the git repository at {} {MARK}. Run `ljos doctor` for the repair steps",
        root.display()
    ))
}

/// The doctor text when `root` is a git checkout whose objects do not
/// read: a pack with no index, or HEAD naming an object that is not
/// there. The numbered steps are the repair that puts the objects back
/// and then commits the worktree. `None` for a healthy checkout, a
/// checkout that is not git, and a branch that has no commit yet.
///
/// Read-only. The steps are printed. They are not run.
pub(crate) fn doctor_note(root: &Path) -> Option<String> {
    if !inside_work_tree(root) {
        return None;
    }
    let packs = packs_without_index(root);
    let head_bad = head_object_unreadable(root);
    if packs.is_empty() && !head_bad {
        return None;
    }
    Some(format!(
        "the git repository at {} {MARK}\n{}",
        root.display(),
        repair_steps(root, &packs)
    ))
}

/// Whether git's text is a bad object, a missing object, or a pack it
/// could not open for lack of an index.
pub(crate) fn git_text_is_object_damage(said: &str) -> bool {
    let lower = said.to_ascii_lowercase();
    if bad_object(&lower) {
        return true;
    }
    if lower.contains("could not parse head") || lower.contains("could not get object info") {
        return true;
    }
    if lower.contains("missing object")
        || lower.contains("object missing")
        || lower.contains("object corrupt or missing")
        || lower.contains("missing blob ")
        || lower.contains("missing tree ")
    {
        return true;
    }
    if lower.contains("does not point to a valid object") || lower.contains("invalid sha1 pointer")
    {
        return true;
    }
    if lower.contains("unable to open loose object")
        || (lower.contains("loose object") && lower.contains("corrupt"))
        || (lower.contains("object file") && lower.contains("is empty"))
        || lower.contains("unable to read sha1 file")
    {
        return true;
    }
    pack_index_missing(&lower)
}

/// `bad object HEAD` is this failure. `bad object type` and `bad object
/// header` are not: those name a kind git did not expect.
fn bad_object(lower: &str) -> bool {
    lower.match_indices("bad object").any(|(at, _)| {
        let next = lower[at + "bad object".len()..].trim_start();
        !next.starts_with("type") && !next.starts_with("header") && !next.starts_with("id")
    })
}

fn pack_index_missing(lower: &str) -> bool {
    lower.contains("index unavailable")
        || lower.contains("index not opened")
        || lower.contains("failed to load pack-index")
        || lower.contains("cannot open existing pack")
        || lower.contains("broken .idx")
        || (lower.contains("packfile") && lower.contains("cannot be accessed"))
}

fn repair_steps(root: &Path, packs: &[PathBuf]) -> String {
    let quoted = crate::tools::sh_quote(&root.display().to_string());
    let branch = branch_name(root);
    let pack_step = if packs.is_empty() {
        format!("1. `git -C {quoted} index-pack` the pack that has no index")
    } else {
        let cmds: Vec<String> = packs
            .iter()
            .map(|pack| {
                let pack = crate::tools::sh_quote(&pack.display().to_string());
                format!("`git -C {quoted} index-pack {pack}`")
            })
            .collect();
        format!("1. {}", cmds.join(" and "))
    };
    format!(
        "{pack_step}\n\
         2. Point the branch at a valid commit\n\
         3. `git -C {quoted} fetch --refetch origin {branch}`\n\
         4. Set the branch to origin\n\
         5. Rebuild the index with `git -C {quoted} read-tree HEAD`\n\
         6. Commit the worktree. Never reset it: it may hold unpushed tracker changes"
    )
}

fn branch_name(root: &Path) -> String {
    let Some(out) = git(root, &["symbolic-ref", "--short", "HEAD"]) else {
        return "<branch>".into();
    };
    if !out.status.success() {
        return "<branch>".into();
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let plain = !name.is_empty()
        && !name.starts_with('-')
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'));
    if plain {
        name
    } else {
        "<branch>".into()
    }
}

/// `git cat-file -e HEAD` exits 1 when the name resolves and the object
/// does not read. An unborn branch exits 128 and is not this failure.
fn head_object_unreadable(root: &Path) -> bool {
    let Some(out) = git(root, &["cat-file", "-e", "HEAD"]) else {
        return false;
    };
    if out.status.success() {
        return false;
    }
    if out.status.code() == Some(1) {
        return true;
    }
    let said = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    git_text_is_object_damage(&said)
}

fn packs_without_index(root: &Path) -> Vec<PathBuf> {
    let Some(common) = common_dir(root) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(common.join("objects/pack")) else {
        return Vec::new();
    };
    let mut missing = Vec::new();
    for ent in entries.flatten() {
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("pack-") || !name.ends_with(".pack") {
            continue;
        }
        if !path.with_extension("idx").is_file() {
            missing.push(path);
        }
    }
    missing.sort();
    missing
}

fn common_dir(root: &Path) -> Option<PathBuf> {
    let out = git(root, &["rev-parse", "--git-common-dir"])?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    let path = PathBuf::from(&text);
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    path.is_dir().then_some(path)
}

fn inside_work_tree(root: &Path) -> bool {
    let Some(out) = git(root, &["rev-parse", "--is-inside-work-tree"]) else {
        return false;
    };
    out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "true"
}

/// English object errors, including on a seat whose locale translates git.
fn git(dir: &Path, args: &[&str]) -> Option<Output> {
    let mut cmd = crate::git_at(dir);
    cmd.args(args)
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null());
    crate::git_hooks::hook_child_env(&mut cmd);
    crate::bounded_output(&mut cmd).ok()
}

#[cfg(test)]
mod tests {
    use super::{doctor_note, git_text_is_object_damage, plain_failure, MARK};
    use std::path::Path;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        // `GIT_DIR` overrides `-C`. A sibling test exports it for a moment.
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("LC_ALL", "C")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_NAMESPACE")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn seeded(root: &Path) {
        std::fs::create_dir_all(root.join("Software")).unwrap();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "seat@example.invalid"]);
        git(root, &["config", "user.name", "seat"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        git(root, &["config", "core.hooksPath", "/dev/null"]);
        std::fs::write(root.join("Software/issues.org"), "* TODO a\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "seed"]);
    }

    fn drop_pack_index(root: &Path) {
        git(root, &["repack", "-Ad"]);
        let pack = root.join(".git/objects/pack");
        let mut saw_pack = false;
        for ent in std::fs::read_dir(&pack).unwrap() {
            let path = ent.unwrap().path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("pack") {
                saw_pack = true;
            }
            if matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("idx") | Some("rev")
            ) {
                std::fs::remove_file(&path).unwrap();
            }
        }
        assert!(saw_pack, "repack left no pack to break");
        for ent in std::fs::read_dir(root.join(".git/objects")).unwrap() {
            let path = ent.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if name.len() == 2 && name.chars().all(|c| c.is_ascii_hexdigit()) {
                std::fs::remove_dir_all(&path).unwrap();
            }
        }
    }

    fn point_head_at_missing(root: &Path) {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["symbolic-ref", "HEAD"])
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_NAMESPACE")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let rel = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let path = root.join(".git").join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n").unwrap();
    }

    #[test]
    fn object_phrases_are_damage_and_other_git_errors_are_not() {
        for said in [
            "fatal: bad object HEAD",
            "fatal: bad object refs/heads/master",
            "fatal: could not parse HEAD",
            "fatal: git cat-file: could not get object info",
            "missing object abcdef for refs/heads/main",
            "error: object file .git/objects/aa/bb is empty",
            "error: loose object aabbcc is corrupt",
            "error: packfile .git/objects/pack/pack-abc.pack index unavailable",
            "error: packfile .git/objects/pack/pack-abc.pack cannot be accessed",
            "fatal: Cannot open existing pack file '.git/objects/pack/pack-abc.idx'",
            "missing blob abcdef",
            "HEAD does not point to a valid object!",
        ] {
            assert!(git_text_is_object_damage(said), "{said}");
        }
        for said in [
            "Author identity unknown",
            "fatal: Authentication failed for 'https://example.invalid/'",
            "fatal: Not a valid object name HEAD",
            "fatal: Unable to create '/t/.git/index.lock': File exists",
            "Branch main is missing commits.",
            "fatal: bad object type",
            "bad object header",
            "fatal: Could not resolve host: example.invalid",
            "the remote was not reached",
        ] {
            assert!(!git_text_is_object_damage(said), "{said}");
        }
    }

    #[test]
    fn a_healthy_checkout_and_an_unborn_branch_say_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        seeded(&root);
        assert!(doctor_note(&root).is_none());
        git(&root, &["repack", "-Ad"]);
        assert!(
            doctor_note(&root).is_none(),
            "a pack that has its index is not this failure"
        );

        let fresh = dir.path().join("fresh");
        std::fs::create_dir_all(&fresh).unwrap();
        git(&fresh, &["init", "-q"]);
        assert!(
            doctor_note(&fresh).is_none(),
            "an unborn branch is not broken"
        );
        assert!(doctor_note(dir.path()).is_none());
    }

    #[test]
    fn a_pack_without_an_index_names_the_repair_and_does_not_run_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        seeded(&root);
        drop_pack_index(&root);
        let index = std::fs::read(root.join(".git/index")).ok();
        let note = doctor_note(&root).unwrap_or_else(|| panic!("no note"));
        assert!(note.contains(&root.display().to_string()), "{note}");
        assert!(note.contains(MARK), "{note}");
        assert_repair(&note, &root);
        assert!(
            std::fs::read_dir(root.join(".git/objects/pack"))
                .unwrap()
                .flatten()
                .all(|ent| ent.path().extension().and_then(|ext| ext.to_str()) != Some("idx")),
            "doctor wrote an index"
        );
        assert_eq!(std::fs::read(root.join(".git/index")).ok(), index);
        let said = plain_failure(&root.join("Software"), "fatal: bad object HEAD").unwrap();
        assert!(said.contains(&root.display().to_string()), "{said}");
        assert!(said.contains("`ljos doctor`"), "{said}");
        assert!(!said.contains("fatal:"), "{said}");
        assert!(!said.to_ascii_lowercase().contains("bad object"), "{said}");
    }

    #[test]
    fn a_missing_head_commit_names_the_same_repair() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        seeded(&root);
        point_head_at_missing(&root);
        let note = doctor_note(&root).unwrap_or_else(|| panic!("no note"));
        assert!(note.contains(&root.display().to_string()), "{note}");
        assert_repair(&note, &root);
        let head = std::fs::read_to_string(root.join(".git/HEAD")).unwrap();
        assert!(head.contains("refs/heads/"), "doctor moved HEAD: {head}");
        let said = plain_failure(&root, "fatal: could not parse HEAD").unwrap();
        assert!(said.contains("`ljos doctor`"), "{said}");
        assert!(plain_failure(&root, "Author identity unknown").is_none());
    }

    fn assert_repair(note: &str, root: &Path) {
        for step in ["\n1. ", "\n2. ", "\n3. ", "\n4. ", "\n5. ", "\n6. "] {
            assert!(note.contains(step), "{note}");
        }
        assert!(note.contains("index-pack"), "{note}");
        assert!(
            note.contains("Point the branch at a valid commit"),
            "{note}"
        );
        assert!(note.contains("fetch --refetch origin"), "{note}");
        assert!(note.contains("Set the branch to origin"), "{note}");
        assert!(note.contains("read-tree HEAD"), "{note}");
        assert!(
            note.contains("Never reset it: it may hold unpushed tracker changes"),
            "{note}"
        );
        assert!(note.contains(&root.display().to_string()), "{note}");
        let branch = std::fs::read_to_string(root.join(".git/HEAD")).unwrap();
        let branch = branch.trim().rsplit('/').next().unwrap();
        assert!(
            note.contains(&format!("fetch --refetch origin {branch}")),
            "{note}"
        );
    }
}
