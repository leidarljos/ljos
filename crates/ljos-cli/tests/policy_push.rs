//! `ljos policy` on a push, with no seat rules and the ask pack switched off.
//! Force, a `+refspec`, a `--force-with-lease` push onto main, master or
//! develop, and a mirror push run with the `git-force-push` warning, and
//! the bypass log keeps the label. Deleting a remote branch or tag runs
//! with the `git-push-delete` warning the same way. A plain branch push, a
//! tag push and a lease push onto a feature branch stay allowed. A push
//! whose `LJOS_CITE` does not stand is denied.

ljos_test_boot::harness!();

#[path = "common/guard_mode.rs"]
mod guard_mode;
use std::path::Path;
use std::process::Command;

fn policy(home: &Path, line: &str) -> std::process::Output {
    guard_mode::command(env!("CARGO_BIN_EXE_ljos"))
        .arg("policy")
        .arg(line)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("PACKSET_URL", "off")
        .env("LJOS_ASK_PACK", "off")
        .env("LJOS_GUARD", "on")
        .env_remove("POLICYD_BIN")
        .env_remove("POLICYD_REQUIRED")
        .env_remove("LJOS_SEAT")
        .output()
        .unwrap()
}

fn stdout_of(line: &str) -> String {
    let home = tempfile::tempdir().unwrap();
    stdout_in(home.path(), line)
}

fn stdout_in(home: &Path, line: &str) -> String {
    let out = policy(home, line);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{line} exited {:?}\nstdout:\n{text}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    text
}

fn verdict_heads(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| {
            let head = line.split([':', '\t']).next().unwrap_or(line).trim();
            matches!(head, "allow" | "ask" | "deny").then_some(head)
        })
        .collect()
}

/// Each line runs with every verdict an allow, prints `said`, and leaves
/// a `warn` row naming `rule` and the line in the bypass log.
fn assert_runs_labelled(rule: &str, said: &str, lines: &[&str]) {
    for line in lines {
        let home = tempfile::tempdir().unwrap();
        let text = stdout_in(home.path(), line);
        assert!(text.contains(said), "{line}\n{text}");
        let heads = verdict_heads(&text);
        assert!(
            heads.iter().all(|head| *head == "allow"),
            "{line} -> {heads:?}\n{text}"
        );
        let log = std::fs::read_to_string(home.path().join(".local/share/ljos/bypass.jsonl"))
            .unwrap_or_default();
        assert!(
            log.lines()
                .filter_map(|row| serde_json::from_str::<serde_json::Value>(row).ok())
                .any(|row| row["kind"] == "warn" && row["rule"] == rule && row["command"] == *line),
            "{line} left no label:\n{log}"
        );
    }
}

#[test]
fn a_force_or_mirror_push_runs_with_its_warning_and_label() {
    assert_runs_labelled(
        "git-force-push",
        "allow\twarn\tgit-force-push\ta force push rewrites a published branch",
        &[
            "git push --force origin main",
            "git push -f origin main",
            "git push origin +main",
            "git push --force-with-lease origin main",
            "git push --force-with-lease=main:abc123 origin main",
            "git push --force-with-lease origin HEAD:develop",
            "git push --force-with-lease origin +feature",
            "git push origin +refs/tags/v1.0.0",
            "git push --mirror origin",
        ],
    );
}

#[test]
fn a_delete_push_runs_with_its_warning_and_label() {
    assert_runs_labelled(
        "git-push-delete",
        "allow\twarn\tgit-push-delete\ta push deletes a remote branch",
        &[
            "git push origin --delete old",
            "git push -d origin old",
            "git push origin :old",
            "git push origin :refs/tags/v1.0.0",
        ],
    );
}

#[test]
fn a_plain_push_stays_allowed() {
    for line in [
        "git push",
        "git push origin main",
        "git push upstream develop",
        "git push -u origin feature/x",
        "git push --force-with-lease origin feature",
        "git push --force-with-lease --force-if-includes origin feature/x",
        "git push --force-with-lease=feature:abc123 origin HEAD:feature",
        "git push --tags",
        "git push --follow-tags",
        "git push --follow-tags origin main",
        "git push origin v1.0.0",
        "git push origin tag v1.0.0",
        "git push origin refs/tags/v1.0.0",
    ] {
        let text = stdout_of(line);
        let heads = verdict_heads(&text);
        assert!(
            heads.iter().all(|head| *head == "allow"),
            "{line} -> {heads:?}\n{text}"
        );
    }
}

#[test]
fn a_push_whose_cite_does_not_stand_is_not_denied_here() {
    let line = "LJOS_CITE=gpr_optim-1yg5 git push upstream develop";
    let text = stdout_of(line);
    assert!(
        !text.contains("built-in push cite"),
        "a cite does not deny the push: {text}"
    );
}
