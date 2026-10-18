//! A ticket minted with `vissue q` is an untracked file until the first
//! tracker commit. `persist_tracker_file` adds that one file and commits it;
//! a sibling untracked file stays out of the commit.

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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
    String::from_utf8_lossy(&out.stdout).to_string()
}

const TICKET: &str = "#+TITLE: proj issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
* TODO [#C] fresh ticket\n:PROPERTIES:\n:ID:         proj-nnnn\n:CREATED:    [2026-10-11 Sun]\n:END:\n";

#[test]
fn an_untracked_ticket_is_added_and_committed_alone() {
    let repo = tempfile::tempdir().unwrap();
    let dir = repo.path();
    git(dir, &["init", "-q", "."]);
    git(dir, &["config", "user.email", "seat@example.test"]);
    git(dir, &["config", "user.name", "seat"]);
    let hooks = dir.join("no-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    git(dir, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "base"]);

    let issues = dir.join("Software/proj/issues");
    std::fs::create_dir_all(&issues).unwrap();
    let ticket = issues.join("proj-nnnn.org");
    let sibling = issues.join("proj-oooo.org");
    std::fs::write(&ticket, TICKET).unwrap();
    std::fs::write(&sibling, TICKET.replace("nnnn", "oooo")).unwrap();
    assert!(git(dir, &["ls-files"]).is_empty());

    std::env::set_var("LJOS_TRACKER_GIT", "commit");
    let said = ljos_cli::persist_tracker_file(&ticket, "proj-nnnn", "noted");

    assert!(said.contains("committed"), "{said}");
    assert!(!said.contains("refused"), "{said}");
    let tracked = git(dir, &["ls-files"]);
    assert!(tracked.contains("proj-nnnn.org"), "{tracked}");
    assert!(!tracked.contains("proj-oooo.org"), "{tracked}");
    let subject = git(dir, &["log", "-1", "--format=%s"]);
    assert!(subject.contains("proj-nnnn"), "{subject}");
}

#[test]
fn a_ticket_reached_through_a_symlinked_checkout_is_committed() {
    let real = tempfile::tempdir().unwrap();
    let dir = real.path();
    git(dir, &["init", "-q", "."]);
    git(dir, &["config", "user.email", "seat@example.test"]);
    git(dir, &["config", "user.name", "seat"]);
    let hooks = dir.join("no-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    git(dir, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "base"]);
    std::fs::create_dir_all(dir.join("issues")).unwrap();
    let holder = tempfile::tempdir().unwrap();
    let link = holder.path().join("tracker");
    std::os::unix::fs::symlink(dir, &link).unwrap();
    let ticket = link.join("issues/proj-pppp.org");
    std::fs::write(&ticket, TICKET.replace("nnnn", "pppp")).unwrap();

    std::env::set_var("LJOS_TRACKER_GIT", "commit");
    let said = ljos_cli::persist_tracker_file(&ticket, "proj-pppp", "noted");

    assert!(said.contains("committed"), "{said}");
    assert!(git(dir, &["ls-files"]).contains("proj-pppp.org"));
}
