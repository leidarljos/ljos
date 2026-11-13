//! `ljos finish` on a ticket another seat holds is one sentence.
//!
//! The sentence names that seat and `ljos send`. It does not talk about a
//! session node, an env file, or `--assignee`.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn finish_of_another_seats_ticket_says_send() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = dir.path().join("tracker");
    let project = tracker.join("Software/demo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(
        project.join("issues.org"),
        "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* STARTED [#C] Pick a codec
:PROPERTIES:
:ID:         demo-uqy6
:CLAIMED_BY: alice
:END:
",
    )
    .unwrap();
    script(
        dir.path(),
        "claimdag",
        "#!/bin/sh\ncase \"$1\" in\n  get) echo 'gen=2 assignee=0123456789abcdef0123456789abcdef state=claimed' ;;\n  complete) echo 'not assignee' >&2; exit 1 ;;\n  *) exit 0 ;;\nesac\n",
    );
    script(dir.path(), "vissue", "#!/bin/sh\nexit 0\n");
    script(dir.path(), "packset", "#!/bin/sh\nexit 1\n");
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("run")).unwrap();
    let path = format!("{}:/usr/bin:/bin", dir.path().display());
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["finish", "demo-uqy6"])
        .env_clear()
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("VISSUE_ROOT", &tracker)
        .env("LJOS_SEAT", "bob")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .env("LJOS_TEST_HOSTNAME", "laptop")
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains(
            "demo-uqy6 is held by another (alice), not by bob, so run `ljos send alice TEXT`."
        ),
        "{text}"
    );
    assert!(!text.contains("session node"), "{text}");
    assert!(!text.contains("LJOS_SESSION_ID"), "{text}");
    assert!(!text.contains("--assignee"), "{text}");
}
