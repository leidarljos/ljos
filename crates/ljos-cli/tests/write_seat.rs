//! A tracker write needs a seat somebody named, as a ballot does.
//! Subagent shells with no `LJOS_SEAT` wrote 314 notes as `exec-daemon`
//! and 74 as `cursor`, the programs above them.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Bench {
    _dir: tempfile::TempDir,
    root: PathBuf,
    log: PathBuf,
    path: String,
}

impl Bench {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let log = root.join("calls.log");
        script(
            &root,
            "vissue",
            &format!(
                "#!/bin/sh\necho \"vissue $* VISSUE_AGENT=${{VISSUE_AGENT:-}}\" >> '{}'\ncase \"$1\" in\n  show) echo '{{}}' ;;\n  list|board) echo '[]' ;;\nesac\nexit 0\n",
                log.display()
            ),
        );
        for sub in ["home", "run"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        let path = format!(
            "{}:{}",
            root.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Self {
            _dir: dir,
            root,
            log,
            path,
        }
    }

    fn ljos(&self, seat: Option<&str>, args: &[&str]) -> Output {
        let env: Vec<(&str, &str)> = seat.map(|s| ("LJOS_SEAT", s)).into_iter().collect();
        self.ljos_env(&env, args)
    }

    fn ljos_env(&self, env: &[(&str, &str)], args: &[&str]) -> Output {
        let home = self.root.join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("GROK_SESSION_ID", "2c12969d-0000-4000-8000-000000000000")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .envs(env.iter().copied());
        cmd.output().unwrap()
    }

    fn seats(&self, body: &str) -> PathBuf {
        let dir = self.root.join("home/.config/ljos");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("seats.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

const WRITES: &[&[&str]] = &[
    &["note", "proj-aaaa", "progress"],
    &["claim", "proj-aaaa"],
    &["sitting", "proj-aaaa"],
    &["finish", "proj-aaaa", "--lesson", "x"],
    &["release", "proj-aaaa"],
    &["file", "a new bug", "--top", "-p", "proj"],
    &["options", "proj-aaaa", "a,b"],
    &["deed", "proj-aaaa", "--add", "d-0000"],
    &["deed", "proj-aaaa", "--remove", "d-0000"],
    &["workflow", "proj-aaaa", "--start", "feature"],
    &["workflow", "proj-aaaa", "--round"],
    &["consensus", "proj-aaaa"],
    &["playbook", "proj-aaaa", "sit"],
    &["reap", "--idle", "6h", "--apply"],
    &["vote", "proj-aaaa", "--for", "accept", "--used", "none"],
    &["vote", "proj-aaaa", "--withdraw"],
    &["vote", "proj-aaaa", "--as", "skeptic", "--jev"],
    &["predict", "proj-aaaa", "--expect", "accept"],
    &[
        "slot",
        "proj-aaaa",
        "runner",
        "--model",
        "m",
        "--run",
        "r1",
        "--key",
        "00",
    ],
    &[
        "bump-plan",
        "bundle",
        "--project",
        "proj",
        "--parent",
        "proj-aaaa",
    ],
];

#[test]
fn every_tracker_write_from_a_shell_with_no_seat_is_refused() {
    let b = Bench::new();
    for args in WRITES {
        let out = b.ljos(None, args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} went through: {out:?}");
        assert!(err.contains("no seat is named"), "{args:?}: {err}");
        // Which derived seat it is depends on what runs above the test.
        assert!(
            err.contains("the program above this shell")
                || err.contains("the login user of a shell under an agent runner"),
            "{args:?}: {err}"
        );
        assert!(err.contains("LJOS_SEAT"), "{args:?} names the fix: {err}");
        assert!(
            err.contains("seats.toml"),
            "{args:?} names the seats file: {err}"
        );
    }
    assert_eq!(b.calls(), "", "nothing reached the tracker");
}

#[test]
fn a_named_seat_writes_and_a_read_needs_no_seat() {
    let b = Bench::new();
    let note = b.ljos(Some("saddle"), &["note", "proj-aaaa", "progress"]);
    let err = String::from_utf8_lossy(&note.stderr);
    assert!(!err.contains("no seat is named"), "{err}");
    assert!(b.calls().contains("vissue note proj-aaaa"), "{}", b.calls());

    let read = b.ljos(None, &["deed", "proj-aaaa"]);
    let err = String::from_utf8_lossy(&read.stderr);
    assert!(
        !err.contains("no seat is named"),
        "listing is a read: {err}"
    );
}

#[test]
fn with_a_seats_file_only_a_listed_seat_writes() {
    let b = Bench::new();
    let note = ["note", "proj-aaaa", "progress"];
    let open = b.ljos(Some("stranger"), &note);
    assert!(
        !String::from_utf8_lossy(&open.stderr).contains("no seat named"),
        "with no seats file any named seat writes: {open:?}"
    );
    assert!(b.calls().contains("vissue note"), "{}", b.calls());

    let b = Bench::new();
    let path = b.seats("\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n");
    let refused = b.ljos(Some("stranger"), &note);
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(err.contains("no seat named stranger"), "{err}");
    assert!(err.contains(&path.display().to_string()), "{err}");
    assert!(err.contains("saddle"), "names the seats it lists: {err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");

    let listed = b.ljos(Some("saddle"), &note);
    assert!(
        !String::from_utf8_lossy(&listed.stderr).contains("no seat named"),
        "{listed:?}"
    );
    assert!(b.calls().contains("vissue note"), "{}", b.calls());
}

#[test]
fn a_conversation_the_seats_file_maps_writes_as_that_seat() {
    let b = Bench::new();
    b.seats("\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n");
    let out = b.ljos_env(
        &[(
            "CURSOR_CONVERSATION_ID",
            "0b1c2d3e-0000-4000-8000-000000000000",
        )],
        &["note", "proj-aaaa", "progress"],
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("no seat is named"), "{err}");
    assert!(!err.contains("no seat named"), "{err}");
    assert!(b.calls().contains("vissue note proj-aaaa"), "{}", b.calls());
}

#[test]
fn a_write_records_the_seat_the_seats_file_maps() {
    let b = Bench::new();
    b.seats("\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n");
    b.ljos_env(
        &[(
            "CURSOR_CONVERSATION_ID",
            "0b1c2d3e-0000-4000-8000-000000000000",
        )],
        &["note", "proj-aaaa", "progress"],
    );
    let calls = b.calls();
    let note = calls
        .lines()
        .find(|l| l.starts_with("vissue note proj-aaaa"))
        .unwrap_or_else(|| panic!("no note: {calls}"));
    assert!(note.ends_with("VISSUE_AGENT=saddle"), "{note}");
}

#[test]
fn a_filed_issue_is_created_as_the_seat_the_check_named() {
    let b = Bench::new();
    b.seats("seats = [\"saddle\", \"inky\"]\n");
    b.ljos_env(
        &[("LJOS_SEAT", "saddle"), ("VISSUE_AGENT", "inky")],
        &["file", "a new bug", "--top", "-p", "proj"],
    );
    let calls = b.calls();
    let create = calls
        .lines()
        .find(|l| l.starts_with("vissue create"))
        .unwrap_or_else(|| panic!("no create: {calls}"));
    assert!(create.ends_with("VISSUE_AGENT=saddle"), "{create}");
}

fn open_panel(b: &Bench, env: &[(&str, &str)]) -> Output {
    let prompt = b.root.join("prompt.txt");
    std::fs::write(&prompt, "so what do we think? is this the right answer?").unwrap();
    let log = b.root.join("run/panel-open.log");
    let mut env = env.to_vec();
    env.push(("LJOS_PROJECT", "proj"));
    b.ljos_env(
        &env,
        &[
            "open-panel",
            "--prompt-file",
            prompt.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
        ],
    )
}

#[test]
fn a_panel_opened_by_an_unnamed_or_unlisted_seat_files_nothing() {
    let b = Bench::new();
    let unnamed = open_panel(&b, &[]);
    let err = String::from_utf8_lossy(&unnamed.stderr);
    assert!(!unnamed.status.success(), "{unnamed:?}");
    assert!(err.contains("open-panel: no seat is named"), "{err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");

    b.seats("seats = [\"saddle\"]\n");
    let unlisted = open_panel(&b, &[("LJOS_SEAT", "stranger")]);
    let err = String::from_utf8_lossy(&unlisted.stderr);
    assert!(!unlisted.status.success(), "{unlisted:?}");
    assert!(err.contains("no seat named stranger"), "{err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");
}

#[test]
fn a_panel_decision_is_filed_as_the_seat_the_check_named() {
    let b = Bench::new();
    b.seats("seats = [\"saddle\", \"inky\"]\n");
    open_panel(&b, &[("LJOS_SEAT", "saddle"), ("VISSUE_AGENT", "inky")]);
    let calls = b.calls();
    // The body spans lines, so the agent is on the record's last line.
    let create = calls
        .split_once("vissue create")
        .and_then(|(_, rest)| rest.split_once(" VISSUE_AGENT="))
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_else(|| panic!("no create: {calls}"));
    assert_eq!(create, "saddle", "{calls}");
}

/// A bundle of one module, so a plan makes one issue.
fn one_module_bundle(b: &Bench) -> String {
    let bundle = b.root.join("bundle");
    std::fs::create_dir_all(bundle.join("locks")).unwrap();
    std::fs::write(
        bundle.join("locks/default.lock.json"),
        r#"{"package":"eOn","version":"2.17.10","toolchain":{"name":"foss","version":"2026.1"},"dependencies":[]}"#,
    )
    .unwrap();
    std::fs::write(
        bundle.join("package.sbom.cdx.json"),
        r#"{"dependencies":[]}"#,
    )
    .unwrap();
    bundle.display().to_string()
}

#[test]
fn a_bump_plan_is_filed_only_as_a_listed_seat() {
    let b = Bench::new();
    let bundle = one_module_bundle(&b);
    // No issue is on the tracker yet, so the plan creates it.
    script(
        &b.root,
        "vissue",
        &format!(
            "#!/bin/sh\necho \"vissue $* VISSUE_AGENT=${{VISSUE_AGENT:-}}\" >> '{}'\n[ \"$1\" = show ] && exit 1\nexit 0\n",
            b.log.display()
        ),
    );
    b.seats("seats = [\"saddle\", \"inky\"]\n");
    let plan = [
        "bump-plan",
        &bundle,
        "--project",
        "proj",
        "--parent",
        "proj-aaaa",
    ];

    let dry = b.ljos(None, &[&plan[..], &["--dry-run"][..]].concat());
    assert!(dry.status.success(), "a dry run is a read: {dry:?}");
    assert_eq!(b.calls(), "", "a dry run writes nothing");

    let unlisted = b.ljos(Some("stranger"), &plan);
    let err = String::from_utf8_lossy(&unlisted.stderr);
    assert!(!unlisted.status.success(), "{unlisted:?}");
    assert!(err.contains("no seat named stranger"), "{err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");

    b.ljos_env(&[("LJOS_SEAT", "saddle"), ("VISSUE_AGENT", "inky")], &plan);
    let calls = b.calls();
    let create = calls
        .lines()
        .find(|l| l.starts_with("vissue create"))
        .unwrap_or_else(|| panic!("no create: {calls}"));
    assert!(create.ends_with("VISSUE_AGENT=saddle"), "{create}");
}

#[test]
fn a_conversation_the_seats_file_maps_votes_as_that_seat() {
    let b = Bench::new();
    b.seats("\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n");
    let out = b.ljos_env(
        &[(
            "CURSOR_CONVERSATION_ID",
            "0b1c2d3e-0000-4000-8000-000000000000",
        )],
        &["vote", "proj-aaaa", "--for", "accept", "--used", "none"],
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("no seat is named"), "{err}");
    let calls = b.calls();
    assert!(
        calls.contains("vissue vote proj-aaaa --for accept --used none VISSUE_AGENT=saddle"),
        "{calls}"
    );
}

#[test]
fn a_ballot_or_a_withdrawal_as_another_seat_is_refused() {
    let b = Bench::new();
    b.seats("seats = [\"saddle\", \"inky\"]\n");
    for args in [
        &[
            "vote",
            "proj-aaaa",
            "--for",
            "accept",
            "--used",
            "none",
            "--as",
            "inky",
        ][..],
        &["vote", "proj-aaaa", "--withdraw", "--as", "inky"],
    ] {
        let out = b.ljos(Some("saddle"), args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} went through: {out:?}");
        assert!(
            err.contains("--as inky is not this seat, saddle"),
            "{args:?}: {err}"
        );
    }
    assert_eq!(b.calls(), "", "nothing reached the tracker");

    b.ljos(
        Some("saddle"),
        &["vote", "proj-aaaa", "--withdraw", "--as", "saddle"],
    );
    let calls = b.calls();
    assert!(
        calls.contains("vissue vote proj-aaaa --withdraw VISSUE_AGENT=saddle"),
        "{calls}"
    );
}

#[test]
fn an_assignee_that_is_another_seat_is_refused() {
    let b = Bench::new();
    for args in [
        &["claim", "proj-aaaa", "--assignee", "stranger"][..],
        &["sitting", "proj-aaaa", "--assignee", "stranger"],
        &[
            "finish",
            "proj-aaaa",
            "--lesson",
            "x",
            "--assignee",
            "stranger",
        ],
        &["complete", "proj-aaaa", "--assignee", "stranger"],
    ] {
        let out = b.ljos(Some("saddle"), args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} went through: {out:?}");
        assert!(
            err.contains("stranger is not this seat, saddle"),
            "{args:?}: {err}"
        );
    }
    assert_eq!(b.calls(), "", "nothing reached the tracker");
}

#[test]
fn release_hands_back_only_to_a_seat_the_seats_file_lists() {
    let b = Bench::new();
    b.seats(
        "\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n\
         \"1c2d3e4f-0000-4000-8000-000000000000\" = \"helper\"\n",
    );
    let refused = b.ljos(
        Some("saddle"),
        &["release", "proj-aaaa", "--assignee", "stranger"],
    );
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(err.contains("no seat named stranger"), "{err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");

    let handed = b.ljos(
        Some("saddle"),
        &["release", "proj-aaaa", "--assignee", "helper"],
    );
    let err = String::from_utf8_lossy(&handed.stderr);
    assert!(!err.contains("no seat named"), "{err}");
    assert!(!err.contains("is not this seat"), "{err}");
}

#[test]
fn a_seats_array_is_the_list_of_seats() {
    let b = Bench::new();
    let note = ["note", "proj-aaaa", "progress"];
    b.seats("seats = [\"saddle\"]\n");
    let refused = b.ljos(Some("stranger"), &note);
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(err.contains("no seat named stranger"), "{err}");
    assert!(err.contains("which lists saddle"), "{err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");

    b.ljos(Some("saddle"), &note);
    assert!(b.calls().contains("vissue note"), "{}", b.calls());
}

#[test]
fn a_seats_file_that_lists_no_seat_refuses_every_write() {
    let b = Bench::new();
    let path = b.seats("# seats go here\n[owner]\nname = 1\n");
    let out = b.ljos(Some("saddle"), &["note", "proj-aaaa", "progress"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{out:?}");
    assert!(err.contains("lists no seat"), "{err}");
    assert!(err.contains(&path.display().to_string()), "{err}");
    assert!(err.contains("seats = [\"saddle\"]"), "names the fix: {err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");
}

#[test]
fn a_playbook_binding_records_the_seat_the_seats_file_maps() {
    let b = Bench::new();
    b.seats("\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n");
    b.ljos_env(
        &[(
            "CURSOR_CONVERSATION_ID",
            "0b1c2d3e-0000-4000-8000-000000000000",
        )],
        &["playbook", "proj-aaaa", "sit"],
    );
    let calls = b.calls();
    let note = calls
        .lines()
        .find(|l| l.starts_with("vissue note proj-aaaa playbook: sit"))
        .unwrap_or_else(|| panic!("no binding: {calls}"));
    assert!(note.ends_with("VISSUE_AGENT=saddle"), "{note}");
}

#[test]
fn a_slot_is_signed_as_the_seat_the_check_named() {
    let b = Bench::new();
    b.seats("seats = [\"saddle\"]\n");
    let key: String = ed25519_dalek::SigningKey::from_bytes(&[7; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let out = b.ljos_env(
        &[("LJOS_SEAT", "saddle"), ("VISSUE_AGENT", "stranger")],
        &[
            "slot",
            "proj-aaaa",
            "runner",
            "--model",
            "m",
            "--run",
            "r1",
            "--key",
            &key,
        ],
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{out:?}");
    assert!(err.contains("slot: saddle has no pinned key"), "{err}");
    assert!(!err.contains("stranger"), "{err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");
}

/// A pack that answers each request with the body it was sent.
fn echo_pack() -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let body = loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break String::new();
                }
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf);
                let Some((head, body)) = text.split_once("\r\n\r\n") else {
                    continue;
                };
                let need = head
                    .lines()
                    .filter_map(|l| l.split_once(':'))
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if body.len() >= need {
                    break body.to_string();
                }
            };
            let body = if body.is_empty() { "{}".into() } else { body };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

#[test]
fn a_forecast_is_recorded_as_the_seat_the_check_named() {
    let b = Bench::new();
    b.seats("seats = [\"saddle\", \"inky\"]\n");
    let pack = echo_pack();
    let out = b.ljos_env(
        &[
            ("LJOS_SEAT", "saddle"),
            ("VISSUE_AGENT", "inky"),
            ("PACKSET_URL", &pack),
        ],
        &["predict", "proj-aaaa", "--expect", "accept"],
    );
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{out:?}");
    assert!(said.contains("\"agent\": \"saddle\""), "{said}");

    let refused = b.ljos_env(
        &[("LJOS_SEAT", "saddle"), ("PACKSET_URL", &pack)],
        &["predict", "proj-aaaa", "--expect", "accept", "--as", "inky"],
    );
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{refused:?}");
    assert!(err.contains("--as inky is not this seat, saddle"), "{err}");
}

#[test]
fn a_seats_file_that_is_not_toml_refuses_every_write() {
    let b = Bench::new();
    let path = b.seats("seats = [\"saddle\"\n");
    for args in WRITES {
        let out = b.ljos(Some("saddle"), args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?} went through: {out:?}");
        assert!(
            err.contains(&format!("the seats file {} is not TOML", path.display())),
            "{args:?}: {err}"
        );
    }
    assert_eq!(b.calls(), "", "nothing reached the tracker");
}

#[test]
fn ljos_seats_does_not_point_the_check_at_another_file() {
    let b = Bench::new();
    let path = b.seats("seats = [\"saddle\"]\n");
    let own = b.root.join("mine.toml");
    std::fs::write(&own, "seats = [\"stranger\"]\n").unwrap();
    let out = b.ljos_env(
        &[
            ("LJOS_SEAT", "stranger"),
            ("LJOS_SEATS", own.to_str().unwrap()),
        ],
        &["note", "proj-aaaa", "progress"],
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{out:?}");
    assert!(err.contains("no seat named stranger"), "{err}");
    assert!(err.contains(&path.display().to_string()), "{err}");
    assert_eq!(b.calls(), "", "nothing reached the tracker");
}

#[test]
fn a_seats_file_that_links_to_nothing_refuses_every_write() {
    let b = Bench::new();
    let path = b.seats("");
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(b.root.join("gone.toml"), &path).unwrap();
    for seat in ["saddle", "rohit"] {
        for args in WRITES {
            let out = b.ljos(Some(seat), args);
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(
                !out.status.success(),
                "{seat} {args:?} went through: {out:?}"
            );
            assert!(
                err.contains(&format!(
                    "the seats file {} links to a file that is not there",
                    path.display()
                )),
                "{seat} {args:?}: {err}"
            );
        }
    }
    assert_eq!(b.calls(), "", "nothing reached the tracker");

    std::fs::remove_file(&path).unwrap();
    b.ljos(Some("rohit"), &["note", "proj-aaaa", "progress"]);
    let calls = b.calls();
    let note = calls
        .lines()
        .find(|l| l.starts_with("vissue note proj-aaaa"))
        .unwrap_or_else(|| panic!("rohit with no seats file wrote nothing: {calls}"));
    assert!(note.ends_with("VISSUE_AGENT=rohit"), "{note}");
}
