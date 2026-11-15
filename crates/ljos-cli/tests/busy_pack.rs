//! Tracker verbs stay fast when the pack is down or never answers.
//!
//! A closed port and a listener that accepts and then stays quiet both
//! have to leave the org file written, and the command has to return
//! while a claim graph that never answers is still asleep.

ljos_test_boot::harness!();

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

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

struct Seat {
    root: tempfile::TempDir,
}

impl Seat {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let tracker = root.path().join("tracker");
        let project = tracker.join("Software/demo");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(root.path().join("state")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        std::fs::write(
            project.join("issues.org"),
            "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* STARTED [#B] Probe the kiln
:PROPERTIES:
:ID:         demo-busy
:CLAIMED_BY: ada
:CREATED:    [2026-06-01 Mon]
:END:
",
        )
        .unwrap();
        let log = root.path().join("calls.log");
        script(
            root.path(),
            "vissue",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\ncase \"$1\" in\n  identity) printf '%s\\n' 'vissue test' 'root={}' 'prefix=Software' ;;\n  create) printf '%s\\n' 'project: demo' 'demo-new  TODO  [#C]  filed' ;;\n  show) printf '%s\\n' '{{\"id\":\"demo-busy\",\"title\":\"Probe the kiln\",\"state\":\"STARTED\",\"claimed_by\":\"ada\"}}' ;;\nesac\nexit 0\n",
                log.display(),
                tracker.display(),
            ),
        );
        script(root.path(), "claimdag", "#!/bin/sh\nexec sleep 60\n");
        script(root.path(), "deedar", "#!/bin/sh\necho head\nexit 0\n");
        Self { root }
    }

    fn run(&self, pack: &str, args: &[&str]) -> (std::process::Output, Duration) {
        let home = self.root.path().join("home");
        let path = format!(
            "{}:{}",
            self.root.path().display(),
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
        );
        let started = Instant::now();
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.root.path())
            .env("PATH", path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("VISSUE_ROOT", self.root.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", pack)
            .env("PACKSET_AUTH", "off")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (out, started.elapsed())
    }
}

fn text_of(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn assert_fast(what: &str, out: &std::process::Output, took: Duration) {
    let said = text_of(out);
    assert!(
        took < Duration::from_secs(15),
        "{what} waited {took:?}\n{said}"
    );
    assert!(
        out.status.success(),
        "{what} exited {:?}\n{said}",
        out.status.code()
    );
}

#[test]
fn tracker_verbs_return_when_the_pack_port_is_closed() {
    check("http://127.0.0.1:9");
}

#[test]
fn tracker_verbs_return_when_the_pack_never_answers() {
    check(&silent_pack());
}

fn check(pack: &str) {
    let seat = Seat::open();
    let org = seat.root.path().join("tracker/Software/demo/issues.org");
    let before = std::fs::read_to_string(&org).unwrap();

    let (filed, took) = seat.run(pack, &["file", "--id-only", "File the door", "-p", "demo"]);
    assert_fast("file", &filed, took);
    let stdout = String::from_utf8_lossy(&filed.stdout);
    let id = stdout.lines().next().unwrap_or("").trim();
    assert_eq!(id, "demo-new", "{stdout}");
    assert_eq!(
        stdout.lines().filter(|line| !line.is_empty()).count(),
        1,
        "file stdout was more than the id:\n{stdout}"
    );
    let calls = std::fs::read_to_string(seat.root.path().join("calls.log")).unwrap();
    assert!(calls.contains("vissue create"), "{calls}");

    let (sat, took) = seat.run(pack, &["sitting", "demo-busy"]);
    assert_fast("sitting", &sat, took);
    let sat_said = text_of(&sat);
    assert!(
        sat_said.contains(
            "claim graph did not answer; the claim is in the tracker and reconciles later"
        ),
        "{sat_said}"
    );
    let calls = std::fs::read_to_string(seat.root.path().join("calls.log")).unwrap();
    assert!(
        calls.contains("vissue claim demo-busy"),
        "sitting did not stamp the tracker:\n{calls}"
    );

    let (noted, took) = seat.run(
        pack,
        &["note", "demo-busy", "the door seats when it is cold"],
    );
    assert_fast("note", &noted, took);
    let calls = std::fs::read_to_string(seat.root.path().join("calls.log")).unwrap();
    assert!(
        calls.contains("the door seats when it is cold"),
        "note did not write the tracker:\n{calls}"
    );

    let (done, took) = seat.run(
        pack,
        &["finish", "demo-busy", "--lesson", "Cold metal seats."],
    );
    assert_fast("finish", &done, took);
    let done_said = text_of(&done);
    assert!(done_said.contains("finished demo-busy"), "{done_said}");
    assert!(
        done_said.contains("Memory busy: queued 1 write, it will be saved on a later call.")
            || done_said.contains("claim graph did not answer"),
        "{done_said}"
    );
    let queued = std::fs::read_to_string(seat.root.path().join("state/ljos/note-queue.jsonl"))
        .unwrap_or_default();
    assert!(
        queued.contains("Cold metal seats."),
        "the lesson was not queued:\n{queued}\n{done_said}"
    );
    let org_after = std::fs::read_to_string(&org).unwrap();
    assert_ne!(
        org_after, before,
        "finish did not write the tracker:\n{org_after}"
    );

    let (started, took) = seat.run(pack, &["start", "demo-busy"]);
    assert_fast("start", &started, took);
    assert!(
        text_of(&started).contains("claim graph did not answer"),
        "{}",
        text_of(&started)
    );
}
