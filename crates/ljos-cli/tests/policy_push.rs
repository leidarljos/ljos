//! `ljos policy` on a push, with no seat rules and the ask pack switched off.
//! Force, a `+refspec`, deleting a remote branch, and pushing tags ask.
//! A plain branch push stays allowed.

fn policy(line: &str) -> std::process::Output {
    let home = tempfile::tempdir().unwrap();
    ljos_cli::process_env::command(env!("CARGO_BIN_EXE_ljos"))
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
            "a force push rewrites what others may hold",
        ),
        (
            "git push -f origin main",
            "a force push rewrites what others may hold",
        ),
        (
            "git push origin +main",
            "a force push rewrites what others may hold",
        ),
        (
            "git push origin --delete old",
            "deleting a remote branch removes what others may hold",
        ),
        (
            "git push origin :old",
            "deleting a remote branch removes what others may hold",
        ),
        ("git push --tags", "tags and mirrors publish releases"),
        (
            "git push --follow-tags origin main",
            "tags and mirrors publish releases",
        ),
    ];
    for (line, why) in cases {
        let text = stdout_of(line);
        assert!(
            text.contains(&format!("ask: {why}")) && text.contains("built-in: dangerous push"),
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
        "LJOS_CITE=gpr_optim-1yg5 git push upstream develop",
    ] {
        let text = stdout_of(line);
        let heads = verdict_heads(&text);
        assert!(
            heads.iter().all(|head| *head == "allow"),
            "{line} -> {heads:?}\n{text}"
        );
    }
}
