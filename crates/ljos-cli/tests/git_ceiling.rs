//! A test's git call cannot reach the checkout's `.git`.
//!
//! A scratch repository whose `.git` is missing or half made is not a
//! repository, so git walks up from it. From the checkout's `target` that
//! walk ends in the ljos checkout, and a `git config` there rewrites its
//! `.git/config`.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::Output;

#[path = "common/seat_root.rs"]
mod seat_root;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn local_config(checkout: &Path) -> Option<Vec<u8>> {
    std::fs::read(checkout.join(".git/config")).ok()
}

/// A directory with no `.git` inside the checkout's `crates/target` is
/// not a repository to a test's git.
#[test]
fn a_test_git_in_a_dir_without_git_does_not_reach_the_checkout() {
    let checkout = ljos_test_boot::checkout();
    let scratch = Scratch(ljos_test_boot::fresh_dir(
        &ljos_test_boot::checkout_seats(),
        "ljos-git-ceiling",
    ));
    let nested = scratch.0.join("repo/sub");
    std::fs::create_dir_all(&nested).unwrap();
    let before = local_config(&checkout);

    let open = ljos_test_boot::git()
        .args(["rev-parse", "--absolute-git-dir"])
        .current_dir(&nested)
        .env_remove(ljos_test_boot::GIT_CEILING)
        .output()
        .unwrap();
    let found = String::from_utf8_lossy(&open.stdout).trim().to_string();
    if !open.status.success() || !Path::new(&found).starts_with(&checkout) {
        eprintln!(
            "skip: {} is not inside a git checkout\n{}",
            nested.display(),
            said(&open)
        );
        return;
    }

    let fenced = ljos_test_boot::git()
        .args(["rev-parse", "--absolute-git-dir"])
        .current_dir(&nested)
        .output()
        .unwrap();
    assert!(
        !fenced.status.success(),
        "git from {} found {}\n{}",
        nested.display(),
        String::from_utf8_lossy(&fenced.stdout).trim(),
        said(&fenced)
    );
    assert!(
        String::from_utf8_lossy(&fenced.stderr).contains("not a git repository"),
        "{}",
        said(&fenced)
    );

    let cleared = ljos_test_boot::git_at("/usr/bin/git")
        .args(["rev-parse", "--absolute-git-dir"])
        .current_dir(&nested)
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(!cleared.status.success(), "{}", said(&cleared));
    assert_eq!(
        local_config(&checkout),
        before,
        "the checkout config changed"
    );
}

/// A `HOME` with no room for seat homes is refused. No seat goes under
/// the checkout's `crates/target` in its place.
#[test]
fn a_seat_root_never_falls_back_to_the_checkout() {
    let is_temp = ljos_cli::bookkeep::path_is_temp;
    let temp = std::env::temp_dir().join("ljos-test-seats");
    let long = PathBuf::from(format!("/{}/.cache/ljos-test-seats", "h".repeat(40)));
    for cache in [
        None,
        Some(temp),
        Some(long),
        Some(ljos_test_boot::checkout_seats()),
    ] {
        let picked = seat_root::pick_seat_root(cache.clone(), is_temp);
        assert!(picked.is_err(), "{cache:?} gave {picked:?}");
    }
    let fine = PathBuf::from("/home/u/.cache/ljos-test-seats");
    assert_eq!(
        seat_root::pick_seat_root(Some(fine.clone()), |_| false),
        Ok(fine)
    );
}

/// Seat homes sit outside the checkout.
#[test]
fn a_seat_root_sits_outside_the_checkout() {
    let is_temp = ljos_cli::bookkeep::path_is_temp;
    let checkout = ljos_test_boot::checkout();
    let root = seat_root::seat_root(is_temp);
    assert!(
        !root.starts_with(&checkout),
        "seat root {} is inside the checkout {}",
        root.display(),
        checkout.display()
    );
    let dir = Scratch(seat_root::seat_dir(is_temp, "ljos-git-ceiling"));
    assert!(!dir.0.starts_with(&checkout), "{}", dir.0.display());
}

/// Two tests asking for the same name at once get two directories, and
/// neither removes the other's.
#[test]
fn scratch_dirs_with_one_name_never_collide() {
    let root = Scratch(ljos_test_boot::fresh_dir(
        &std::env::temp_dir(),
        "ljos-git-ceiling",
    ));
    let made: Vec<PathBuf> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..16)
            .map(|_| s.spawn(|| ljos_test_boot::fresh_dir(&root.0, "same")))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut names = made.clone();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), made.len(), "{made:?}");
    assert!(made.iter().all(|dir| dir.is_dir()), "{made:?}");
}

/// No test runs git without the ceiling: every git command is built by
/// `ljos_test_boot::git` or `git_at`, and a chain that clears the
/// environment sets the ceiling again after it.
#[test]
fn every_test_git_call_carries_the_ceiling() {
    let checkout = ljos_test_boot::checkout();
    let raw = ["Command::new(\"git\")", "Command::new(\"/usr/bin/git\")"];
    let mut files = Vec::new();
    collect(&checkout.join("crates"), &mut files);
    let mut found = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let rel = file.strip_prefix(&checkout).unwrap_or(&file).display();
        for needle in raw {
            for (at, _) in text.match_indices(needle) {
                found.push(format!("{rel}:{}: {needle}", line_of(&text, at)));
            }
        }
        for (at, _) in text.match_indices("ljos_test_boot::git") {
            let chain = &text[at..text[at..].find(';').map_or(text.len(), |end| at + end)];
            if let Some(clear) = chain.find(".env_clear()") {
                if !chain[clear..].contains("git_ceiling()") {
                    found.push(format!(
                        "{rel}:{}: env_clear drops the ceiling",
                        line_of(&text, at)
                    ));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "build test git calls with ljos_test_boot::git or git_at:\n{}",
        found.join("\n")
    );
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// Test sources: `crates/*/tests` trees and the test-boot crate.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name == "target" || name == "fixtures" {
                continue;
            }
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && path
                .components()
                .any(|c| c.as_os_str() == "tests" || c.as_os_str() == "ljos-test-boot")
        {
            out.push(path);
        }
    }
}
