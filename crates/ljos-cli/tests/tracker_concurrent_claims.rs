//! Seats on two hosts claim, file and note on one tracker file at once
//! (ljos-az5k). On the box, `- Claimed by eessi` lines appeared in tickets
//! eessi did not hold, some twice, and one ticket lost its properties
//! drawer to a push that rebased the file while a write was between its
//! read and its rename.
//!
//! `vissue` is the real binary behind a small wrapper. The wrapper can
//! make `show` slow, keep a note's process alive after its write, or hold
//! a note inside the file lock; the held note is this test binary run
//! over vissue-core, reading the file, waiting, and writing it back, as
//! every whole-file writer does. Each host is a clone of one bare remote
//! with `LJOS_TRACKER_GIT=push`.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

const BOARD: &str = "Software/Software/issues.org";
const OTHER: &str = "Software/Other/issues.org";

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn tools_ready() -> bool {
    if tools::halt(tools::Tool::Vissue) {
        return false;
    }
    if which("claimdag").is_none() {
        let line = "claimdag is not on PATH, so run `source scripts/sibling-tools.sh`.";
        if std::env::var(tools::REQUIRE_TOOLS).ok().as_deref() == Some("1") {
            panic!("{line}");
        }
        eprintln!("{line}");
        return false;
    }
    true
}

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

fn seed_board(project: &str, ids: &[String], quoted: Option<(&str, &str)>) -> String {
    let mut org = format!(
        "#+TITLE: {project} issues\n#+VISSUE: 1\n#+CATEGORY: {project}\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n"
    );
    for id in ids {
        org.push_str(&format!(
            "* TODO [#C] Ticket {id}\n:PROPERTIES:\n:ID:         {id}\n:CREATED:    [2026-11-19 Thu]\n:END:\n:LOGBOOK:\n- Note: \"seeded\" [2026-11-19 Thu 10:00]\n:END:\n\nBody of {id}.\n"
        ));
        if let Some((_, copied)) = quoted.filter(|(into, _)| into == id) {
            org.push_str(&format!(
                "Copied from {copied}:\n#+begin_example\n:PROPERTIES:\n:ID:         {copied}\n:END:\n#+end_example\n"
            ));
        }
        org.push('\n');
    }
    org
}

struct Board {
    dir: tempfile::TempDir,
    wrapper_bin: PathBuf,
}

struct Ran {
    host: &'static str,
    args: Vec<String>,
    out: Output,
}

impl Ran {
    fn ok(&self) -> bool {
        self.out.status.success()
    }

    fn said(&self) -> String {
        format!(
            "{} {:?} exited {:?}\nstdout:\n{}\nstderr:\n{}",
            self.host,
            self.args,
            self.out.status.code(),
            String::from_utf8_lossy(&self.out.stdout),
            String::from_utf8_lossy(&self.out.stderr)
        )
    }
}

impl Board {
    fn new(tickets: usize) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["claimdag", "deedar"] {
            if let Some(path) = which(name) {
                std::os::unix::fs::symlink(path, bin.join(name)).unwrap();
            }
        }
        let real = which("vissue").unwrap();
        let wrapper = bin.join("vissue");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\n\
case \"$1\" in\n\
show) [ -n \"$SHIM_SLOW_SHOW\" ] && sleep \"$SHIM_SLOW_SHOW\" ;;\n\
note) if [ -n \"$SHIM_HOLD_NOTE_MS\" ]; then\n\
  LJOS_SHIM_ARGS=\"$(printf '%s\\037' \"$@\")\" exec '{test}' --exact vissue_held_note --ignored --test-threads=1 --nocapture >/dev/null 2>>\"$TMPDIR/held-note.log\"\n\
fi ;;\n\
esac\n\
'{real}' \"$@\"\n\
code=$?\n\
if [ \"$1\" = note ] && [ -n \"$SHIM_SLOW_EXIT\" ]; then sleep \"$SHIM_SLOW_EXIT\"; fi\n\
exit $code\n",
                test = std::env::current_exe().unwrap().display(),
                real = real.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();

        let seed = dir.path().join("seed");
        std::fs::create_dir_all(seed.join("Software/Software")).unwrap();
        std::fs::create_dir_all(seed.join("Software/Other")).unwrap();
        std::fs::write(seed.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let ids: Vec<String> = (1..=tickets).map(|n| format!("Software-t{n:03}")).collect();
        std::fs::write(
            seed.join(BOARD),
            seed_board("Software", &ids, Some(("Software-t008", "Software-t009"))),
        )
        .unwrap();
        std::fs::write(
            seed.join(OTHER),
            seed_board("Other", &["Other-x001".to_string()], None),
        )
        .unwrap();
        git(&seed, &["init", "-q", "-b", "main"]);
        git(&seed, &["config", "user.email", "seat@example.invalid"]);
        git(&seed, &["config", "user.name", "seat"]);
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-q", "-m", "seed"]);
        let remote = dir.path().join("remote.git");
        git(
            dir.path(),
            &[
                "clone",
                "-q",
                "--bare",
                seed.to_str().unwrap(),
                remote.to_str().unwrap(),
            ],
        );
        for host in ["A", "B"] {
            let root = dir.path().join(host);
            std::fs::create_dir_all(&root).unwrap();
            git(&root, &["clone", "-q", remote.to_str().unwrap(), "tracker"]);
            let tracker = root.join("tracker");
            git(&tracker, &["config", "user.email", "seat@example.invalid"]);
            git(&tracker, &["config", "user.name", "seat"]);
            git(&tracker, &["config", "core.hooksPath", "/dev/null"]);
        }
        Self {
            dir,
            wrapper_bin: bin,
        }
    }

    fn tracker(&self, host: &str) -> PathBuf {
        self.dir.path().join(host).join("tracker")
    }

    fn spawn(&self, host: &'static str, seat: &str, args: &[&str], env: &[(&str, &str)]) -> Child {
        use std::os::unix::fs::PermissionsExt;
        let root = self.dir.path().join(host);
        let home = root.join(format!("home-{seat}"));
        let run = root.join(format!("run-{seat}"));
        let store = home.join(".local/share/deedar/store");
        for d in [
            &home.join(".config"),
            &home.join(".local/state"),
            &store,
            &run,
        ] {
            std::fs::create_dir_all(d).unwrap();
        }
        for d in [&run, &store] {
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        static SESSION: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let session = SESSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tracker = self.tracker(host);
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(&tracker)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.wrapper_bin.display()),
            )
            .env("HOME", &home)
            .env("USER", seat)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &run)
            .env("TMPDIR", &root)
            .env("VISSUE_ROOT", &tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", seat)
            .env("LJOS_TRACKER_GIT", "push")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_NO_AUTOSTART", "1")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("NO_COLOR", "1")
            .env("LANG", "C.UTF-8")
            .env("SEAT_SESSION_ID", format!("az5k-{host}-{seat}-{session}"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.spawn().unwrap()
    }

    fn run(&self, host: &'static str, seat: &str, args: &[&str], env: &[(&str, &str)]) -> Ran {
        let out = self
            .spawn(host, seat, args, env)
            .wait_with_output()
            .unwrap();
        Ran {
            host,
            args: args.iter().map(|a| (*a).to_string()).collect(),
            out,
        }
    }

    fn board_on(&self, host: &str) -> String {
        std::fs::read_to_string(self.tracker(host).join(BOARD)).unwrap()
    }
}

/// Every way a board read back can be wrong. Empty when it is right.
fn board_faults(text: &str, must_hold: &[String], notes: &[(String, String)]) -> Vec<String> {
    let mut faults = Vec::new();
    let doc = match vissue_core::IssueDoc::parse("Software", PathBuf::from(BOARD), text) {
        Ok(doc) => doc,
        Err(e) => return vec![format!("the board does not parse: {e}")],
    };
    let headlines = text.lines().filter(|l| l.starts_with("* ")).count();
    if headlines != doc.headings.len() {
        faults.push(format!(
            "{headlines} headlines but {} issues with a properties drawer",
            doc.headings.len()
        ));
    }
    let mut seen = BTreeMap::<&str, usize>::new();
    for heading in &doc.headings {
        *seen.entry(heading.id.as_str()).or_default() += 1;
    }
    for (id, n) in &seen {
        if *n > 1 {
            faults.push(format!("{id} is on {n} headings"));
        }
    }
    for id in must_hold {
        if !seen.contains_key(id.as_str()) {
            faults.push(format!("{id} is gone"));
        }
    }
    for heading in &doc.headings {
        let lines: Vec<String> = heading.logbook.iter().map(|e| e.render()).collect();
        let mut once = BTreeSet::new();
        for line in &lines {
            if !once.insert(line) {
                faults.push(format!("{} has {line:?} twice", heading.id));
            }
            let Some(rest) = line.strip_prefix("- Claimed by ") else {
                continue;
            };
            let seat = rest.split_whitespace().next().unwrap_or("");
            let holder = heading
                .claimed_by()
                .map(|h| h.split('@').next().unwrap_or(h).to_lowercase());
            if holder.as_deref() != Some(seat) {
                faults.push(format!(
                    "{} has {line:?} but :CLAIMED_BY: is {:?}",
                    heading.id,
                    heading.claimed_by()
                ));
            }
        }
    }
    for (id, note) in notes {
        let Some(heading) = doc.headings.iter().find(|h| &h.id == id) else {
            continue;
        };
        let n = heading
            .logbook
            .iter()
            .filter(|e| e.note.as_deref() == Some(note.as_str()))
            .count();
        if n != 1 {
            faults.push(format!("{id} has the note {note:?} {n} times"));
        }
    }
    faults
}

/// A claim the tracker deferred is retried by the seat's next note, under
/// a two-second wall. A `vissue show` slower than that is not an answer.
#[test]
fn a_pending_claim_retried_while_the_tracker_stalls_is_not_stamped_as_held() {
    use std::os::unix::fs::PermissionsExt;
    if !tools_ready() {
        return;
    }
    let board = Board::new(3);
    let run = board.dir.path().join("A/run-eessi");
    let pending = run.join("ljos/claim-pending/Software-t002");
    std::fs::create_dir_all(pending.parent().unwrap()).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(&pending, "eessi\nvissue lock busy\n").unwrap();
    let ran = board.run(
        "A",
        "eessi",
        &["note", "Software-t002", "a note while the claim waits"],
        &[("SHIM_SLOW_SHOW", "3"), ("LJOS_TRACKER_GIT", "commit")],
    );
    assert!(ran.ok(), "{}", ran.said());
    let text = board.board_on("A");
    let faults = board_faults(&text, &[], &[]);
    assert!(
        faults.is_empty(),
        "{}\n{}\n{text}",
        faults.join("\n"),
        ran.said()
    );
    let doc = vissue_core::IssueDoc::parse("Software", PathBuf::from(BOARD), &text).unwrap();
    let heading = doc
        .headings
        .iter()
        .find(|h| h.id == "Software-t002")
        .unwrap();
    assert!(
        heading.claimed_by().is_some() || pending.exists(),
        "the pending claim was dropped and the tracker has no claim\n{}\n{text}",
        ran.said()
    );
}

#[test]
fn notes_whose_tracker_exits_late_are_each_written_once() {
    if !tools_ready() {
        return;
    }
    let board = Board::new(3);
    let seats = ["eessi", "saddle", "bridle", "stirrup", "girth", "cinch"];
    let children: Vec<(String, Child)> = seats
        .iter()
        .map(|seat| {
            let text = format!("{seat} was here");
            let child = board.spawn(
                "A",
                seat,
                &["note", "Software-t002", &text],
                &[
                    ("SHIM_SLOW_EXIT", "3"),
                    ("LJOS_TRACKER_LOCK_WAIT", "1"),
                    ("LJOS_TRACKER_GIT", "commit"),
                ],
            );
            (text, child)
        })
        .collect();
    let mut notes = Vec::new();
    let mut refused = Vec::new();
    for (text, child) in children {
        let out = child.wait_with_output().unwrap();
        if out.status.success() {
            notes.push(("Software-t002".to_string(), text));
        } else {
            refused.push(format!("{text}: {}", String::from_utf8_lossy(&out.stderr)));
        }
    }
    let on_a = board.board_on("A");
    let mut faults = board_faults(&on_a, &[], &notes);
    for line in refused {
        faults.push(format!("a note said it failed: {line}"));
    }
    assert!(faults.is_empty(), "{}\n{on_a}", faults.join("\n"));
}
