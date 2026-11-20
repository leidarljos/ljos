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
use std::time::{Duration, Instant};

const BOARD: &str = "Software/Software/issues.org";
const OTHER: &str = "Software/Other/issues.org";

/// A note written the way vissue writes one, but with a pause between the
/// read and the write while the file's issues lock is held. Not a test:
/// it returns at once unless the wrapper ran it.
#[test]
#[ignore = "run by the vissue wrapper"]
fn vissue_held_note() {
    let (Ok(args), Ok(hold)) = (
        std::env::var("LJOS_SHIM_ARGS"),
        std::env::var("SHIM_HOLD_NOTE_MS"),
    ) else {
        return;
    };
    let args: Vec<&str> = args
        .strip_suffix('\x1f')
        .unwrap_or(&args)
        .split('\x1f')
        .collect();
    let ["note", id, text @ ..] = args.as_slice() else {
        panic!("the held note does not run vissue {args:?}");
    };
    let hold = Duration::from_millis(hold.parse().unwrap());
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    let (_, path, project) = vissue_core::store::find_by_id(&layout, id)
        .unwrap()
        .unwrap();
    vissue_core::store::with_issues_lock(&path, || {
        let mut doc = vissue_core::IssueDoc::parse_file(&project, &path)?;
        if let Ok(mark) = std::env::var("SHIM_HELD_MARK") {
            std::fs::write(mark, "held").unwrap();
        }
        std::thread::sleep(hold);
        let heading = doc.headings.iter_mut().find(|h| h.id == *id).unwrap();
        heading.logbook.insert(
            0,
            vissue_core::model::LogEntry {
                timestamp: vissue_core::model::LogEntry::now(),
                from_state: None,
                to_state: None,
                note: Some(text.join(" ")),
                raw: None,
            },
        );
        doc.write()
    })
    .unwrap();
}

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
    remote: PathBuf,
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
            remote,
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

    fn board_on_remote(&self) -> String {
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(&self.remote)
            .args(["show", &format!("main:{BOARD}")])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    }
}

fn wait_for(path: &Path, within: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < within {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
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
            // `stamp_note_seat` names the seat again above a note another
            // seat put on top, so a seat line may repeat across notes.
            if !once.insert(line) && !line.starts_with("- Noted by ") {
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

fn first_line(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

#[test]
fn seats_on_two_hosts_claim_file_and_note_one_board_at_once() {
    if !tools_ready() {
        return;
    }
    let board = Board::new(12);
    let seeded: Vec<String> = (1..=12).map(|n| format!("Software-t{n:03}")).collect();
    let mut must_hold = seeded.clone();

    // A push from host A rebases the board while a note on host A is
    // between its read and its write. The push comes from a write to the
    // other project's file, which takes that file's lock and not this one.
    let filed = board.run(
        "B",
        "stirrup",
        &["file", "Filed on host B", "-p", "Software"],
        &[],
    );
    assert!(filed.ok(), "{}", filed.said());
    must_hold.push(first_line(&filed.out));
    let mark = board.dir.path().join("A/held");
    let mark_s = mark.to_str().unwrap().to_string();
    let held = board.spawn(
        "A",
        "saddle",
        &["note", "Software-t001", "written while a push rebased"],
        &[
            ("SHIM_HOLD_NOTE_MS", "2500"),
            ("SHIM_HELD_MARK", mark_s.as_str()),
            ("LJOS_TRACKER_LOCK_WAIT", "10"),
        ],
    );
    assert!(
        wait_for(&mark, Duration::from_secs(20)),
        "the held note never took the lock"
    );
    let other = board.run(
        "A",
        "bridle",
        &["note", "Other-x001", "a note on the other board"],
        &[],
    );
    assert!(other.ok(), "{}", other.said());
    let held = held.wait_with_output().unwrap();
    let held = Ran {
        host: "A",
        args: vec!["note".into(), "Software-t001".into()],
        out: held,
    };
    assert!(held.ok(), "{}", held.said());
    let mut notes_a = vec![(
        "Software-t001".to_string(),
        "written while a push rebased".to_string(),
    )];

    // eessi on host B claims a ticket whose drawer another ticket quotes.
    let quoted = board.run("B", "eessi", &["claim", "Software-t009"], &[]);
    assert!(quoted.ok(), "{}", quoted.said());

    // Then every seat at once. eessi sits on both hosts.
    let mut jobs: Vec<(&'static str, Vec<String>, Child)> = Vec::new();
    let seats: [(&'static str, &str, &str); 6] = [
        ("A", "eessi", "Software-t002"),
        ("A", "saddle", "Software-t003"),
        ("A", "bridle", "Software-t004"),
        ("B", "eessi", "Software-t005"),
        ("B", "stirrup", "Software-t006"),
        ("B", "girth", "Software-t007"),
    ];
    for (host, seat, own) in seats {
        let lines = [
            vec!["claim".to_string(), own.to_string()],
            vec![
                "note".to_string(),
                own.to_string(),
                format!("{host} {seat} on its own ticket"),
            ],
            vec![
                "file".to_string(),
                format!("Filed by {host} {seat}"),
                "-p".into(),
                "Software".into(),
            ],
        ];
        for args in lines {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            jobs.push((host, args.clone(), board.spawn(host, seat, &refs, &[])));
        }
        if !(host == "B" && seat == "eessi") {
            let args = vec![
                "note".to_string(),
                "Software-t010".to_string(),
                format!("{host} {seat} on the shared ticket"),
            ];
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            jobs.push((host, args.clone(), board.spawn(host, seat, &refs, &[])));
        }
    }

    let mut notes_b = Vec::new();
    let mut refused = Vec::new();
    for (host, args, child) in jobs {
        let ran = Ran {
            host,
            args,
            out: child.wait_with_output().unwrap(),
        };
        if !ran.ok() {
            refused.push(ran.said());
            continue;
        }
        match ran.args[0].as_str() {
            "file" => must_hold.push(first_line(&ran.out)),
            "note" => {
                let note = (ran.args[1].clone(), ran.args[2].clone());
                if host == "A" {
                    notes_a.push(note);
                } else {
                    notes_b.push(note);
                }
            }
            _ => {}
        }
    }

    // Each host pushes what it has left, then reads the remote back.
    for (host, seat) in [("A", "saddle"), ("B", "stirrup"), ("A", "saddle")] {
        let ran = board.run(
            host,
            seat,
            &["note", "Other-x001", &format!("flush from {host} {seat}")],
            &[],
        );
        assert!(ran.ok(), "{}", ran.said());
    }

    let mut faults = Vec::new();
    for line in &refused {
        faults.push(format!("a command said it failed: {line}"));
    }
    let on_a = board.board_on("A");
    for fault in board_faults(&on_a, &must_hold, &notes_a) {
        faults.push(format!("host A: {fault}"));
    }
    let on_b = board.board_on("B");
    for fault in board_faults(&on_b, &must_hold, &notes_b) {
        faults.push(format!("host B: {fault}"));
    }
    let remote = board.board_on_remote();
    let all_notes: Vec<(String, String)> = notes_a.iter().chain(&notes_b).cloned().collect();
    for fault in board_faults(&remote, &must_hold, &all_notes) {
        faults.push(format!("remote: {fault}"));
    }
    assert!(
        faults.is_empty(),
        "{}\n\nremote board:\n{remote}",
        faults.join("\n")
    );
}

/// Host B adds a board host A has never had. While a seat on A keeps that
/// board's issues lock, as a writer between its read and its rename does,
/// A's push waits to replay B's commit instead of writing the board under
/// the writer, and is written once the lock is free.
#[test]
fn a_board_the_remote_adds_is_locked_before_the_replay_writes_it() {
    if !tools_ready() {
        return;
    }
    let board = Board::new(3);
    let fresh = "Software/Fresh/issues.org";
    let b = board.tracker("B");
    std::fs::create_dir_all(b.join("Software/Fresh")).unwrap();
    std::fs::write(
        b.join(fresh),
        seed_board("Fresh", &["Fresh-f001".to_string()], None),
    )
    .unwrap();
    git(&b, &["add", fresh]);
    git(&b, &["commit", "-q", "-m", "a board B added"]);
    git(&b, &["push", "-q", "origin", "HEAD:main"]);

    let board_file = board.tracker("A").join(fresh);
    std::fs::create_dir_all(board_file.parent().unwrap()).unwrap();
    let held =
        ljos_cli::CommitLock::acquire(&PathBuf::from(format!("{}.lock", board_file.display())));
    let note = "written while B's board arrives";
    let child = board.spawn("A", "saddle", &["note", "Software-t001", note], &[]);
    std::thread::sleep(Duration::from_millis(2500));
    let early = board_file.exists();
    drop(held);
    let out = child.wait_with_output().unwrap();
    let said = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !early,
        "the replay wrote {fresh} while a seat on A had its issues lock\n{said}"
    );
    assert!(out.status.success(), "{said}");
    assert!(board_file.exists(), "{said}");
    let remote = board.board_on_remote();
    assert!(remote.contains(note), "{said}\n{remote}");
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

#[test]
fn a_note_whose_id_is_a_sentence_is_refused_by_name() {
    if !tools_ready() {
        return;
    }
    let board = Board::new(3);
    let session = [
        ("LJOS_TRACKER_GIT", "off"),
        ("SEAT_SESSION_ID", "az5k-one-sitting"),
    ];
    let claim = board.run("A", "eessi", &["claim", "Software-t002"], &session);
    assert!(claim.ok(), "{}", claim.said());
    let before = board.board_on("A");
    let sentence = "the claim path rereads the file";
    let ran = board.run("A", "eessi", &["note", sentence, "more words"], &session);
    assert!(!ran.ok(), "{}", ran.said());
    let stderr = String::from_utf8_lossy(&ran.out.stderr);
    assert!(
        stderr.contains(&format!("{sentence:?} is not a ticket id")),
        "{}",
        ran.said()
    );
    assert_eq!(board.board_on("A"), before, "nothing is noted");
}
