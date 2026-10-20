//! `ljos policy` on a push, with no seat rules and the ask pack switched off.
//! Force, a `+refspec`, a `--force-with-lease` push onto main, master or
//! develop, deleting a remote branch or tag, and a mirror push ask. A plain
//! branch push, a tag push and a lease push onto a feature branch stay
//! allowed.

use std::process::Command;

fn policy(line: &str) -> std::process::Output {
    let home = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("policy")
        .arg(line)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("PACKSET_URL", "off")
        .env("LJOS_ASK_PACK", "off")
        .env_remove("POLICYD_BIN")
        .env_remove("POLICYD_REQUIRED")
        .env_remove("LJOS_SEAT")
        .output()
        .unwrap()
}

fn stdout_of(line: &str) -> String {
    let out = policy(line);
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

#[test]
fn a_dangerous_push_asks_with_no_seat_rule() {
    let cases = [
        (
            "git push --force origin main",
            "a force push rewrites shared history",
        ),
        (
            "git push -f origin main",
            "a force push rewrites shared history",
        ),
        (
            "git push origin +main",
            "a force push rewrites shared history",
        ),
        (
            "git push --force-with-lease origin main",
            "a force push rewrites shared history",
        ),
        (
            "git push --force-with-lease=main:abc123 origin main",
            "a force push rewrites shared history",
        ),
        (
            "git push --force-with-lease origin HEAD:develop",
            "a force push rewrites shared history",
        ),
        (
            "git push --force-with-lease origin +feature",
            "a force push rewrites shared history",
        ),
        (
            "git push origin --delete old",
            "deleting a remote branch removes shared work",
        ),
        (
            "git push origin :old",
            "deleting a remote branch removes shared work",
        ),
        (
            "git push origin :refs/tags/v1.0.0",
            "deleting a remote branch removes shared work",
        ),
        (
            "git push origin +refs/tags/v1.0.0",
            "a force push rewrites shared history",
        ),
        (
            "git push --mirror origin",
            "a mirror push overwrites and deletes every ref on the remote",
        ),
    ];
    for (line, why) in cases {
        let text = stdout_of(line);
        assert!(
            text.contains(&format!("ask: {why}")) && text.contains("(rule: built-in push)"),
            "{line}\n{text}"
        );
        assert!(
            !verdict_heads(&text).contains(&"allow"),
            "{line} still allowed\n{text}"
        );
    }
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
        "LJOS_CITE=gpr_optim-1yg5 git push upstream develop",
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
