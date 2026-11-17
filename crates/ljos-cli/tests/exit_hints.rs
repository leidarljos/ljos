//! Exit codes and the next command, for the misses a new seat hits first.
//!
//! An omitted claim, an unknown accept, a memory prefix, a reopen of a
//! missing ticket, a policy flag that belongs to the command, a missing
//! `landscape`, an empty workflow, a one character ticket typo, and the
//! next step on an open ticket.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

const ORG: &str = "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#B] Fire the kiln
:PROPERTIES:
:ID:         demo-4b1s
:CREATED:    [2026-06-01 Mon]
:END:

* DONE [#C] The glaze cooled
:PROPERTIES:
:ID:         demo-9done
:CREATED:    [2026-06-01 Mon]
:END:
";

struct Home {
    root: PathBuf,
    pack: String,
}

impl Home {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/share",
            "run",
            "bin",
            "tracker/Software/demo",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        std::fs::write(root.join("tracker/Software/demo/issues.org"), ORG).unwrap();
        let bin = root.join("bin");
        std::fs::write(bin.join("curl"), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(bin.join("curl"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            bin.join("claimdag"),
            "#!/bin/sh\necho \"claimdag $*\" >> \"$LJOS_CALLS\"\n[ \"$1\" = ready ] && echo '[]'\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(bin.join("claimdag"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        Self { root, pack: pack() }
    }

    fn run(&self, args: &[&str], path: &str, pack: Option<&str>) -> Output {
        let home = self.root.join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.root.join("home"))
            .env_clear()
            .env("PATH", path)
            .env("HOME", &home)
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_OFF", "1")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_CALLS", self.root.join("calls.log"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(std::process::Stdio::null());
        match pack {
            Some(url) => {
                cmd.env("PACKSET_URL", url)
                    .env("PACKSET_WORKSPACE", "seat")
                    .env("PACKSET_AUTH", "off")
                    .env("PACKSET_TIMEOUT_MS", "4000");
            }
            None => {
                cmd.env("PACKSET_URL", "off");
            }
        }
        cmd.output().unwrap()
    }

    fn bare(&self, args: &[&str]) -> Output {
        let path = format!("{}:/usr/bin:/bin", self.root.join("bin").display());
        self.run(args, &path, None)
    }
}

fn pack() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf);
            let body = if req.contains("/v1/atoms") {
                r#"{"atoms":[]}"#
            } else {
                "{}"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    url
}

fn text(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn claim_with_no_id_exits_2_unless_next_is_set() {
    let home = Home::new();
    let out = home.bare(&["claim"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("`ljos claim demo-2f5j`") && stderr.contains("`--next`"),
        "{said}"
    );
    assert!(out.stdout.is_empty(), "{said}");
    let calls = std::fs::read_to_string(home.root.join("calls.log")).unwrap_or_default();
    assert!(calls.is_empty(), "claimdag was asked:\n{calls}");

    let next = home.bare(&["claim", "--next"]);
    let said = text(&next);
    assert!(next.status.success(), "{said}");
    let calls = std::fs::read_to_string(home.root.join("calls.log")).unwrap_or_default();
    assert!(calls.contains("claimdag ready"), "{calls}");
}

#[test]
fn an_unknown_accept_exits_1_and_names_proposals() {
    let home = Home::new();
    let path = format!("{}:/usr/bin:/bin", home.root.join("bin").display());
    let out = home.run(&["accept", "deadbeef"], &path, Some(&home.pack));
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "not found deadbeef, so run `ljos proposals`."
    );
}

#[test]
fn an_unknown_review_prefix_exits_1() {
    let home = Home::new();
    let path = format!("{}:/usr/bin:/bin", home.root.join("bin").display());
    let out = home.run(
        &["memories", "review", "--keep", "abcd"],
        &path,
        Some(&home.pack),
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("label review-input-warning:") && stdout.contains("no memory starts with"),
        "{said}"
    );
}

#[test]
fn reopen_names_a_missing_ticket_before_the_person_warning() {
    let home = Home::new();
    let missing = home.bare(&["reopen", "demo-9999"]);
    let said = text(&missing);
    assert_eq!(missing.status.code(), Some(1), "{said}");
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("demo-9999") && stderr.contains("ljos ready"),
        "{said}"
    );
    assert!(!said.contains("a person reopens"), "{said}");

    let open = home.bare(&["reopen", "demo-4b1s"]);
    let said = text(&open);
    assert!(open.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&open.stdout);
    assert!(
        stdout.contains("label reopen-warning:") && stdout.contains("a person reopens a ticket"),
        "{said}"
    );
    assert!(!said.contains("not found"), "{said}");
}

#[test]
fn policy_tells_the_person_to_put_a_separator_before_the_command() {
    let home = Home::new();
    let out = home.bare(&["policy", "git", "push", "--force"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Put `--` before the command")
            && stderr.contains("`ljos policy -- git push --force`"),
        "{said}"
    );

    let separated = home.bare(&["policy", "--", "git", "push", "--force"]);
    let said = text(&separated);
    assert!(
        !said.contains("has no flag"),
        "the separator was still read as a policy flag:\n{said}"
    );

    let typo = home.bare(&["policy", "--fail-on-den"]);
    let said = text(&typo);
    assert!(
        said.contains("--fail-on-deny") && !said.contains("Put `--` before the command"),
        "{said}"
    );
}

#[test]
fn conflicts_names_landscape_and_doctor_lists_it() {
    let home = Home::new();
    let only = home.root.join("bin").display().to_string();
    let out = home.run(&["conflicts"], &only, None);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("landscape is not on PATH"), "{said}");
    assert!(!stderr.contains("A required program"), "{said}");

    let doctor = home.bare(&["doctor", "--all"]);
    let page = format!(
        "{}{}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );
    assert!(
        page.contains("\tlandscape\t"),
        "doctor --all did not list landscape:\n{page}"
    );
}

#[test]
fn a_ticket_with_no_workflow_exits_0() {
    let home = Home::new();
    let out = home.bare(&["workflow", "demo-4b1s"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("demo-4b1s") && stdout.contains("--start"),
        "{said}"
    );
    assert!(out.stderr.is_empty(), "{said}");

    let missing = home.bare(&["workflow", "demo-9999"]);
    let said = text(&missing);
    assert_eq!(missing.status.code(), Some(1), "{said}");
}

#[test]
fn a_one_character_ticket_typo_names_the_closest_id() {
    let home = Home::new();
    let line =
        "issue demo-4b1a not found, and the closest is demo-4b1s, so run `ljos show demo-4b1s`.";
    for args in [
        &["show", "demo-4b1a"][..],
        &["recall", "demo-4b1a"][..],
        &["finish", "demo-4b1a"][..],
    ] {
        let out = home.bare(args);
        let said = text(&out);
        assert_eq!(out.status.code(), Some(1), "{args:?}\n{said}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr).trim(),
            line,
            "{args:?}\n{said}"
        );
    }

    let far = home.bare(&["show", "demo-9999"]);
    let said = text(&far);
    assert_eq!(far.status.code(), Some(1), "{said}");
    assert!(!said.contains("closest"), "{said}");
    assert!(said.contains("ljos ready"), "{said}");
}

#[test]
fn show_of_an_open_ticket_names_sitting() {
    let home = Home::new();
    let open = home.bare(&["show", "demo-4b1s"]);
    let said = text(&open);
    assert!(open.status.success(), "{said}");
    let page = String::from_utf8_lossy(&open.stdout);
    assert!(page.contains("next: `ljos sitting demo-4b1s`"), "{said}");

    let done = home.bare(&["show", "demo-9done"]);
    let said = text(&done);
    assert!(done.status.success(), "{said}");
    let page = String::from_utf8_lossy(&done.stdout);
    assert!(!page.contains("ljos sitting"), "{said}");
}
