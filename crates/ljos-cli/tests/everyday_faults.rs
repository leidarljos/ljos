//! Everyday failures: a mistyped command, a ticket id, an empty or huge
//! remember, a search with nothing, a recall of an empty pack, a missing
//! seat, a seat the tracker does not list, two seats on one ticket, and a
//! local file that was left half-written.
//!
//! Each line says what happened and the next command. The exit is 2 when
//! the invocation is wrong, and 1 when the ticket is not one id.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::thread;

struct Home {
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/share",
            "run",
            "work",
            "tracker/Software/demo",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        Self { root }
    }

    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let home = self.root.join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_OFF", "1")
            .env("PACKSET_URL", "off")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(std::process::Stdio::null());
        for (key, value) in extra {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }
}

fn text(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn one_line(bytes: &[u8]) -> String {
    let lines: Vec<_> = String::from_utf8_lossy(bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    lines.into_iter().next().unwrap()
}

fn issues(home: &Home, body: &str) {
    std::fs::write(home.root.join("tracker/Software/demo/issues.org"), body).unwrap();
}

const ORG: &str = "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#B] Fire the kiln
:PROPERTIES:
:ID:         demo-kiln
:CREATED:    [2026-06-01 Mon]
:END:

* TODO [#B] Glaze the tile
:PROPERTIES:
:ID:         demo-tile
:CREATED:    [2026-06-01 Mon]
:END:
";

#[test]
fn a_mistyped_command_names_the_closest_and_exits_2() {
    let home = Home::new();
    let out = home.run(&["serach", "fix", "the", "kiln"], &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    let line = one_line(&out.stderr);
    assert_eq!(
        line,
        "ljos: no command serach. The closest is search. Run `ljos search fix the kiln`."
    );
    assert!(out.stdout.is_empty(), "{said}");
}

#[test]
fn a_mistyped_flag_names_the_closest_and_exits_2() {
    let home = Home::new();
    let out = home.run(&["search", "--limt", "5", "kiln"], &[]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    let line = one_line(&out.stderr);
    assert_eq!(
        line,
        "ljos: search has no flag --limt. The closest is --limit. Run `ljos search --limit 5 kiln`."
    );
}

#[test]
fn an_unknown_ticket_names_ready_and_exits_1() {
    let home = Home::new();
    issues(&home, ORG);
    let out = home.run(&["show", "demo-none"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    let line = one_line(&out.stderr);
    assert_eq!(line, "issue demo-none not found, so run `ljos ready`");
    assert!(out.stdout.is_empty(), "{said}");
}

#[test]
fn an_ambiguous_ticket_names_one_show_and_exits_1() {
    let home = Home::new();
    issues(&home, ORG);
    let out = home.run(&["show", "demo-"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    let line = one_line(&out.stderr);
    assert_eq!(
        line,
        "issue demo- matches demo-kiln and demo-tile, so run `ljos show demo-kiln`"
    );
}

#[test]
fn a_ticket_in_two_trackers_names_ready_and_exits_1() {
    let home = Home::new();
    issues(&home, ORG);
    let other = home.root.join("other");
    std::fs::create_dir_all(other.join("Software/demo")).unwrap();
    std::fs::write(other.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(other.join("Software/demo/issues.org"), ORG).unwrap();
    let config = home.root.join("home/.config/vissue");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("config.toml"),
        format!(
            "[layouts.other]\nroot = \"{}\"\nprefix = \"Software\"\n\n[routes.demo]\nlayout = \"other\"\nproject_dir = \"demo\"\n",
            other.display()
        ),
    )
    .unwrap();
    let out = home.run(
        &["show", "demo-kiln"],
        &[("LJOS_SEAT", "alice"), ("VISSUE_NO_ROUTE", "")],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    let line = one_line(&out.stderr);
    assert_eq!(
        line,
        "issue demo-kiln is in more than one tracker, so run `ljos ready` with VISSUE_ROOT set to one of them"
    );
}

#[test]
fn an_empty_remember_names_the_command_and_exits_2() {
    let home = Home::new();
    let out = home.run(&["remember"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    let line = one_line(&out.stderr);
    assert_eq!(
        line,
        "remember: the text is empty, so run `ljos remember \"TEXT\"`."
    );
}

#[test]
fn a_very_long_remember_names_the_command_and_exits_2() {
    let home = Home::new();
    let text_in = "word ".repeat(250);
    let out = home.run(&["remember", text_in.trim()], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    let line = one_line(&out.stderr);
    let chars = text_in.trim().chars().count();
    assert!(chars > 1000, "{chars}");
    assert_eq!(
        line,
        format!(
            "remember: the text is {chars} characters, past 1000, so run `ljos remember` with one or two sentences."
        )
    );
}

fn empty_pack() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(_) => continue,
            };
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let body: &[u8] = if buf.windows(10).any(|w| w == b"/v1/search") {
                br#"{"hits":[],"rank":"lexical"}"#
            } else if buf.windows(9).any(|w| w == b"/v1/atoms") {
                br#"{"atoms":[]}"#
            } else {
                b"{}"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn a_search_with_no_hits_names_the_next_command() {
    let home = Home::new();
    let url = empty_pack();
    let out = home.run(
        &["search", "which fusion method"],
        &[
            ("LJOS_SEAT", "alice"),
            ("PACKSET_URL", &url),
            ("PACKSET_AUTH", "off"),
            ("PACKSET_TOKEN", "probe-token"),
        ],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "No hits for \"which fusion method\", so run `ljos search fusion`."
    );
}

#[test]
fn a_recall_of_an_empty_pack_names_remember() {
    let home = Home::new();
    issues(&home, ORG);
    let bin = home.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("vissue"),
        "#!/bin/sh\nif [ \"$1\" = recall ]; then echo tracker; fi\nexit 0\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("vissue"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    let url = empty_pack();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let out = home.run(
        &["recall", "demo-kiln"],
        &[
            ("PATH", path.as_str()),
            ("LJOS_SEAT", "alice"),
            ("PACKSET_URL", &url),
            ("PACKSET_AUTH", "off"),
            ("PACKSET_TOKEN", "probe-token"),
        ],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .contains("The pack has no lesson on demo-kiln, so run `ljos remember \"TEXT\"`."),
        "{said}"
    );
}

#[test]
fn a_write_with_no_seat_names_the_export() {
    let home = Home::new();
    let url = empty_pack();
    let out = home.run(
        &["remember", "The kiln fires at dusk."],
        &[
            ("PACKSET_URL", &url),
            ("PACKSET_AUTH", "off"),
            ("PACKSET_TOKEN", "probe-token"),
        ],
    );
    let said = text(&out);
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("ljos: no seat was named, so this write uses unknown-"),
        "{said}"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Run `export LJOS_SEAT=NAME`."),
        "{said}"
    );
}

#[test]
fn a_missing_seat_name_names_seat_add_and_exits_2() {
    let home = Home::new();
    std::fs::write(
        home.root.join("tracker/vissue.toml"),
        "prefix = \"Software\"\nseats = [\"alice\"]\n",
    )
    .unwrap();
    let out = home.run(&["note", "the glaze crawled"], &[("LJOS_SEAT", "stranger")]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    let line = one_line(&out.stdout);
    assert!(
        line.contains("label seat-unlisted: no seat named stranger in "),
        "{line}"
    );
    assert!(
        line.contains(
            ", which lists alice, and nothing was written, so run `ljos seat add stranger`"
        ),
        "{line}"
    );
    assert!(out.stderr.is_empty(), "{said}");
}

#[test]
fn two_seats_on_one_ticket_name_send() {
    let hold = ljos_cli::Hold {
        assignee: "acme".into(),
        seat: "acme".into(),
        pid: 1,
        comm: "ljos".into(),
        since: "2026-01-01T00:00:00.000Z".into(),
    };
    let said = ljos_cli::held_by_another_message("demo-aaaa", "brio", &hold, "still running");
    assert_eq!(
        said,
        "claim: demo-aaaa is held by another (acme, seat acme, still running, since 2026-01-01T00:00:00.000Z), not by brio, so run `ljos send acme TEXT`"
    );
    assert!(!said.contains('\n'));
}

#[test]
fn a_half_written_user_rules_file_names_prefer() {
    let home = Home::new();
    std::fs::write(
        home.root.join("home/.config/ljos/user-rules.json"),
        "{\"rules\":[{\"id\":\"r1\",\"text\":",
    )
    .unwrap();
    let out = home.run(&["doctor"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    let joined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        joined.contains("is half-written or does not parse"),
        "{said}"
    );
    assert!(
        joined.contains("so run `ljos prefer --user --why REASON TEXT` to write it again"),
        "{said}"
    );
}

#[test]
fn a_half_written_note_queue_names_finish() {
    let home = Home::new();
    std::fs::write(
        home.root.join("home/.local/state/ljos/note-queue.jsonl"),
        "{\"issue\":\"demo-kiln\",\"te\n",
    )
    .unwrap();
    let out = home.run(&["doctor"], &[("LJOS_SEAT", "alice")]);
    let said = text(&out);
    let joined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(joined.contains("is damaged:"), "{said}");
    assert!(
        joined.contains("so run `ljos finish` to move the bad rows aside"),
        "{said}"
    );
}
