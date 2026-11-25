//! Loop verbs return while a sidecar is missing or a pack accepts and
//! stays silent. Each one used to sit on a thirty-second read, or on a
//! helper that never exited.

ljos_test_boot::harness!();

#[path = "common/sibling.rs"]
mod sibling;
#[path = "common/tools.rs"]
mod tools;

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    ljos_cli::install_executable(&path, body).unwrap();
}

/// Accepts and reads one request, then stays silent.
fn silent_pack() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            std::thread::sleep(Duration::from_secs(60));
        }
    });
    format!("http://{addr}")
}

fn home_of(dir: &Path) -> std::path::PathBuf {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    home
}

fn ljos(dir: &Path, pack: &str, args: &[&str]) -> (std::process::Output, Duration) {
    ljos_at(dir, pack, args, false)
}

/// `token` is what lets a search ask the writer. Without one the search
/// reads stored atoms and never waits on a silent pack.
fn ljos_at(dir: &Path, pack: &str, args: &[&str], token: bool) -> (std::process::Output, Duration) {
    let home = home_of(dir);
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
    );
    let started = Instant::now();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .current_dir(dir)
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", pack)
        .env("PACKSET_AUTH", "off")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .stdin(Stdio::null());
    if token {
        cmd.env("PACKSET_TOKEN", "probe-token");
    }
    let out = cmd.output().unwrap();
    (out, started.elapsed())
}

fn text_of(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_silent_pack_does_not_hold_a_sitting() {
    let dir = tempfile::tempdir().unwrap();
    let pack = silent_pack();
    let (out, took) = ljos(dir.path(), &pack, &["sitting", "demo-sit"]);
    let said = text_of(&out);
    assert!(
        took < Duration::from_secs(15),
        "sitting waited {took:?}\n{said}"
    );
    assert!(!out.status.success(), "{said}");
}

#[test]
fn a_sitting_dies_when_the_pack_probe_is_signalled() {
    let dir = tempfile::tempdir().unwrap();
    let home = home_of(dir.path());
    let pack = silent_pack();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["sitting", "demo-sig"])
        .current_dir(dir.path())
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", &pack)
        .env("PACKSET_AUTH", "off")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > Duration::from_secs(2) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("sitting ignored SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success(), "signal status {status}");
}

#[test]
fn a_silent_pack_does_not_hold_a_search() {
    let dir = tempfile::tempdir().unwrap();
    let pack = silent_pack();
    for args in [vec!["search", "32148"], vec!["search", "--rerank", "32148"]] {
        let (out, took) = ljos_at(dir.path(), &pack, &args, true);
        let said = text_of(&out);
        assert!(
            took < Duration::from_secs(8),
            "{} waited {took:?}\n{said}",
            args.join(" ")
        );
        assert_eq!(out.status.code(), Some(1), "{said}");
        assert!(
            said.contains("Memory is busy, so these results are ranked by words. If this persists, run `ljos doctor`."),
            "{said}"
        );
        assert_eq!(said.matches("ranked by words").count(), 1, "{said}");
        assert!(!said.contains("pack busy, try again"), "{said}");
        assert!(
            !said.contains("the pack search did not answer in time"),
            "{said}"
        );
        assert!(!said.contains("The memory writer is busy"), "{said}");
    }
}

#[test]
fn a_silent_pack_does_not_hold_a_remember() {
    let dir = tempfile::tempdir().unwrap();
    let pack = silent_pack();
    let (out, took) = ljos(dir.path(), &pack, &["remember", "tabs not spaces"]);
    let said = text_of(&out);
    assert!(
        took < Duration::from_secs(8),
        "remember waited {took:?}\n{said}"
    );
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("Memory busy: queued 1 write. A later ljos call from any seat will try to save it, and the pack may still refuse it."),
        "{said}"
    );
}

#[test]
fn a_silent_pack_does_not_hold_a_consensus() {
    let dir = tempfile::tempdir().unwrap();
    script(
        dir.path(),
        "vissue",
        "#!/bin/sh\ncase \"$1\" in\n  vote) echo '[{\"agent\":\"ada\",\"choice\":\"ship\"}]' ;;\n  show) echo '{\"id\":\"demo-q\",\"title\":\"ship\"}' ;;\n  children) echo '[]' ;;\n  *) exit 1 ;;\nesac\n",
    );
    script(
        dir.path(),
        "ljos-consensus",
        "#!/bin/sh\necho '{\"options\":[\"ship\"],\"shares\":[1.0],\"settled\":true,\"converged\":true,\"tie\":false,\"agents\":[\"ada\"],\"influence\":[1.0],\"engine\":\"degroot\"}'\n",
    );
    let pack = silent_pack();
    let home = home_of(dir.path());
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
    );
    let started = Instant::now();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["consensus", "demo-q", "--no-file"])
        .current_dir(dir.path())
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", &pack)
        .env("PACKSET_AUTH", "off")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_UNSIGNED_BALLOTS", "allow")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let took = started.elapsed();
    let said = text_of(&out);
    assert!(
        took < Duration::from_secs(8),
        "consensus waited {took:?}\n{said}"
    );
    assert!(out.status.success(), "{said}");
}

#[test]
fn a_note_returns_when_claimdag_never_answers() {
    let dir = tempfile::tempdir().unwrap();
    let noted = dir.path().join("noted");
    script(
        dir.path(),
        "vissue",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = show ]; then echo '{{\"id\":\"demo-n\",\"logbook\":[]}}'; exit 0; fi\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            noted.display()
        ),
    );
    script(dir.path(), "claimdag", "#!/bin/sh\nexec sleep 60\n");
    let (out, took) = ljos(
        dir.path(),
        "http://127.0.0.1:9",
        &["note", "demo-n", "hello"],
    );
    let said = text_of(&out);
    assert!(
        took < Duration::from_secs(8),
        "note waited {took:?}\n{said}"
    );
    assert!(out.status.success(), "{said}");
    let wrote = std::fs::read_to_string(&noted).unwrap_or_default();
    assert!(wrote.contains("note"), "{wrote}");
}

#[test]
fn a_note_returns_when_the_tracker_does_not_answer() {
    let dir = tempfile::tempdir().unwrap();
    script(
        dir.path(),
        "vissue",
        "#!/bin/sh\nif [ \"$1\" = show ]; then echo '{\"id\":\"demo-n\",\"logbook\":[]}' ; exit 0; fi\nexec sleep 60\n",
    );
    let home = home_of(dir.path());
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
    );
    let started = Instant::now();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["note", "demo-n", "hello"])
        .current_dir(dir.path())
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", "http://127.0.0.1:9")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_TRACKER_LOCK_WAIT", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let took = started.elapsed();
    let said = text_of(&out);
    assert!(
        took < Duration::from_secs(8),
        "note waited {took:?}\n{said}"
    );
    assert!(!out.status.success(), "{said}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(out.status.code(), Some(ljos_cli::NOTE_WRITE_UNCERTAIN_EXIT));
    assert_eq!(
        lines,
        ["the note on demo-n may or may not have been written; run `ljos show demo-n` to check"],
        "{said}"
    );
}

fn timed_note(mode: &str, timeout: u64, real: &Path) -> (std::process::Output, String, usize) {
    let dir = tempfile::tempdir().unwrap();
    let tracker = dir.path().join("tracker");
    let layout = vissue_core::Layout::new(&tracker, "Software");
    std::fs::create_dir_all(layout.projects_dir()).unwrap();
    let issue = vissue_core::ops::create(
        &layout,
        "demo",
        "A slow note",
        vissue_core::CreateOpts {
            quiet: true,
            ..Default::default()
        },
    )
    .unwrap();
    let issue = issue.trim();
    if mode == "old" {
        let seeded = ljos_cli::program(real)
            .unwrap()
            .args(["note", issue, "--", "hello"])
            .env("VISSUE_ROOT", &tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("VISSUE_AGENT", "ada")
            .output()
            .unwrap();
        assert!(
            seeded.status.success(),
            "{}",
            String::from_utf8_lossy(&seeded.stderr)
        );
    }
    let noted = dir.path().join("noted");
    script(
        dir.path(),
        "vissue",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = note ]; then\n  printf '%s\\n' \"$*\" >> '{}'\n  if [ '{}' = landed ]; then '{}' \"$@\"; fi\n  sleep 60\n  exit 0\nfi\nexec '{}' \"$@\"\n",
            noted.display(),
            mode,
            real.display(),
            real.display()
        ),
    );
    let home = home_of(dir.path());
    let path = format!(
        "{}:{}:{}",
        dir.path().display(),
        real.parent().unwrap().display(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
    );
    let started = Instant::now();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["note", issue, "hello"])
        .current_dir(dir.path())
        .env("PATH", &path)
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", "http://127.0.0.1:9")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_TRACKER_LOCK_WAIT", "4")
        .env("LJOS_CALL_TIMEOUT", timeout.to_string())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let _took = started.elapsed();
    let wrote = std::fs::read_to_string(&noted).unwrap_or_default();
    let doc =
        vissue_core::IssueDoc::parse_file("demo", &layout.project_issues_path("demo")).unwrap();
    let card = doc
        .headings
        .iter()
        .find(|heading| heading.id == issue)
        .unwrap();
    let hello = card
        .logbook
        .iter()
        .filter_map(|entry| entry.note.as_deref())
        .filter(|note| *note == "hello")
        .count();
    (out, wrote, hello)
}

#[test]
fn a_timed_out_note_is_checked_once_and_never_sent_again() {
    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let real = sibling::require("vissue").unwrap_or_else(|line| panic!("{line}"));
    for timeout in 1..=4 {
        let (landed, calls, hello) = timed_note("landed", timeout, &real);
        let said = text_of(&landed);
        assert!(landed.status.success(), "timeout {timeout}: {said}");
        assert!(
            said.contains("the note is in the logbook"),
            "timeout {timeout}: {said}"
        );
        assert!(!said.contains("run it again"), "timeout {timeout}: {said}");
        assert_eq!(calls.lines().count(), 1, "timeout {timeout}: {calls}");
        assert_eq!(hello, 1, "timeout {timeout}");
    }

    let (uncertain, calls, hello) = timed_note("missing", 1, &real);
    let said = text_of(&uncertain);
    assert_eq!(
        uncertain.status.code(),
        Some(ljos_cli::NOTE_WRITE_UNCERTAIN_EXIT),
        "{said}"
    );
    assert!(said.contains("may or may not have been written"), "{said}");
    assert!(said.contains("`ljos show "), "{said}");
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert_eq!(hello, 0);

    let (uncertain, calls, hello) = timed_note("old", 1, &real);
    let said = text_of(&uncertain);
    assert_eq!(
        uncertain.status.code(),
        Some(ljos_cli::NOTE_WRITE_UNCERTAIN_EXIT),
        "{said}"
    );
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert_eq!(hello, 1, "the old matching note remains");
}
