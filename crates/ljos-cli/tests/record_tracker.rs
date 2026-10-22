//! `ljos record check --tracker ROOT` on a git tracker. Each ballot in a
//! `:VOTES:` drawer is held to its voter's newest `ballot-sig:` note and
//! the pin file, a ballot that does not stand is named with its line,
//! and every run leaves the checkout and HOME as it found them.

use ed25519_dalek::SigningKey;
use ljos_cli::ballot_sig;
use std::path::{Path, PathBuf};
use std::process::Command;

const BOARD: &str = "Software/probe/issues.org";
const ISSUE: &str = "probe-r001";

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn git(dir: &Path, args: &[&str], date: Option<&str>) -> String {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(["-c", "commit.gpgsign=false"])
        .args(args);
    if let Some(date) = date {
        cmd.env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

struct Tracker {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Tracker {
    /// A committed board holding `issues`, none with a ballot yet.
    fn new(issues: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        std::fs::create_dir_all(root.join("Software/probe")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&root, &["init", "-q"], None);
        git(
            &root,
            &["config", "user.email", "seat@example.invalid"],
            None,
        );
        git(&root, &["config", "user.name", "seat"], None);
        git(&root, &["config", "core.hooksPath", "/dev/null"], None);
        let mut org = String::from("#+TITLE: probe issues\n#+TODO: TODO STARTED | DONE\n\n");
        for id in issues {
            org.push_str(&format!(
                "* TODO [#C] Pick a rail for {id}\n:PROPERTIES:\n:ID:         {id}\n:END:\n"
            ));
        }
        std::fs::write(root.join(BOARD), org).unwrap();
        std::fs::write(root.join(".gitignore"), "*.lock\n").unwrap();
        let tracker = Self {
            _dir: dir,
            root,
            home,
        };
        tracker.commit(None);
        tracker
    }

    fn layout(&self) -> vissue_core::Layout {
        vissue_core::Layout::new(self.root.clone(), "Software")
    }

    fn vote(&self, issue: &str, voter: &str, choice: &str) {
        vissue_core::ops::vote_with(
            &self.layout(),
            issue,
            Some(choice),
            voter,
            Some("none"),
            None,
        )
        .unwrap();
    }

    fn note(&self, issue: &str, text: &str) {
        vissue_core::ops::note(&self.layout(), issue, text).unwrap();
    }

    /// `voter` casts `choice` on `issue` and signs it with `key`.
    fn cast(&self, issue: &str, voter: &str, choice: &str, key: &SigningKey) {
        self.vote(issue, voter, choice);
        let session = format!("sess-{voter}-0001");
        self.note(
            issue,
            &ballot_sig::sign_bound(key, issue, voter, choice, &session, "-"),
        );
    }

    fn pin(&self, rows: &[(&str, &SigningKey)]) {
        let text: String = rows
            .iter()
            .map(|(voter, key)| {
                format!("{voter} {}\n", hex::encode(key.verifying_key().to_bytes()))
            })
            .collect();
        std::fs::write(self.root.join(ballot_sig::PIN_FILE), text).unwrap();
    }

    fn commit(&self, date: Option<&str>) {
        git(&self.root, &["add", "-A"], None);
        git(
            &self.root,
            &["commit", "-q", "--allow-empty", "-m", "tracker"],
            date,
        );
    }

    /// The board line `voter`'s ballot sits on.
    fn line_of(&self, voter: &str) -> usize {
        let text = std::fs::read_to_string(self.root.join(BOARD)).unwrap();
        let needle = format!("] {voter}: ");
        text.lines()
            .position(|l| l.trim_start().starts_with('[') && l.contains(&needle))
            .unwrap_or_else(|| panic!("no ballot from {voter}:\n{text}"))
            + 1
    }

    /// Whether the check passed, and what it said.
    fn check(&self) -> (bool, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["record", "check", "--tracker"])
            .arg(&self.root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("PACKSET_URL", "off")
            .output()
            .unwrap();
        let said = format!(
            "status {:?}\n{}{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let dirty = git(&self.root, &["status", "--porcelain"], None);
        assert!(dirty.is_empty(), "the check changed the tracker:\n{dirty}");
        let wrote: Vec<_> = std::fs::read_dir(&self.home).unwrap().flatten().collect();
        assert!(wrote.is_empty(), "the check wrote under HOME: {wrote:?}");
        (out.status.success(), said)
    }
}

#[test]
fn a_tracker_whose_ballots_are_signed_and_pinned_passes() {
    let tracker = Tracker::new(&[ISSUE, "probe-r002"]);
    tracker.cast(ISSUE, "inky", "rail", &key(1));
    tracker.cast(ISSUE, "bob", "bus", &key(2));
    tracker.cast("probe-r002", "inky", "bus", &key(1));
    tracker.pin(&[("inky", &key(1)), ("bob", &key(2))]);
    tracker.commit(None);
    let (ok, said) = tracker.check();
    assert!(ok, "{said}");
    assert!(
        said.contains("record check: 3 ballots on 2 issues stand"),
        "{said}"
    );
}

/// The voter whose ballot does not stand, why, and how it was cast.
type Case = (&'static str, &'static str, fn(&Tracker));

#[test]
fn each_ballot_that_does_not_stand_fails_with_its_reason() {
    let cases: [Case; 5] = [
        ("quiet", "quiet's ballot is unsigned", |t| {
            t.vote(ISSUE, "quiet", "rail");
            t.pin(&[("inky", &key(1))]);
        }),
        ("stray", "stray has no pinned key on this tracker", |t| {
            t.cast(ISSUE, "stray", "rail", &key(5));
            t.pin(&[("inky", &key(1))]);
        }),
        (
            "mallory",
            "mallory's ballot is signed by a key other than its pinned one",
            |t| {
                t.cast(ISSUE, "mallory", "rail", &key(4));
                t.pin(&[("inky", &key(1)), ("mallory", &key(3))]);
            },
        ),
        ("forger", "forger's signature does not verify", |t| {
            t.vote(ISSUE, "forger", "rail");
            let lifted = ballot_sig::sign_bound(&key(6), "probe-r002", "forger", "rail", "s", "-");
            t.note(ISSUE, &lifted);
            t.pin(&[("inky", &key(1)), ("forger", &key(6))]);
        }),
        (
            "edit",
            "edit's signature is for rail and the ballot says bus",
            |t| {
                t.cast(ISSUE, "edit", "rail", &key(7));
                t.vote(ISSUE, "edit", "bus");
                t.pin(&[("inky", &key(1)), ("edit", &key(7))]);
            },
        ),
    ];
    for (voter, reason, setup) in cases {
        let tracker = Tracker::new(&[ISSUE]);
        tracker.cast(ISSUE, "inky", "rail", &key(1));
        setup(&tracker);
        tracker.commit(None);
        let (ok, said) = tracker.check();
        assert!(!ok, "{voter}: {said}");
        let want = format!("{BOARD}:{}: {ISSUE}: {reason}\n", tracker.line_of(voter));
        assert!(said.contains(&want), "{voter}: want {want}{said}");
        assert!(
            said.contains("record check: 1 of 2 ballots on 1 issues do not stand"),
            "{voter}: {said}"
        );
    }
}

#[test]
fn one_key_pinned_for_two_voters_stands_for_neither() {
    let shared = |voter: &str, other: &str| {
        format!("{voter}'s key is also pinned for {other}, so one key counts as several voters")
    };
    let tracker = Tracker::new(&[ISSUE]);
    tracker.cast(ISSUE, "inky", "rail", &key(1));
    tracker.cast(ISSUE, "twin-a", "bus", &key(8));
    tracker.cast(ISSUE, "twin-b", "bus", &key(8));
    tracker.pin(&[("inky", &key(1)), ("twin-a", &key(8)), ("twin-b", &key(8))]);
    tracker.commit(None);
    let (ok, said) = tracker.check();
    assert!(!ok, "{said}");
    for (voter, other) in [("twin-a", "twin-b"), ("twin-b", "twin-a")] {
        let line = tracker.line_of(voter);
        let want = format!("{BOARD}:{line}: {ISSUE}: {}\n", shared(voter, other));
        assert!(said.contains(&want), "want {want}{said}");
    }
    assert!(
        said.contains("record check: 2 of 3 ballots on 1 issues do not stand"),
        "{said}"
    );

    // A pin alone makes the key a second voter's, in either hex case.
    let tracker = Tracker::new(&[ISSUE]);
    tracker.cast(ISSUE, "twin-a", "bus", &key(8));
    let hex = hex::encode(key(8).verifying_key().to_bytes());
    let pins = format!("twin-a {hex}\ntwin-b {}\n", hex.to_uppercase());
    std::fs::write(tracker.root.join(ballot_sig::PIN_FILE), pins).unwrap();
    tracker.commit(None);
    let (ok, said) = tracker.check();
    assert!(!ok, "{said}");
    assert!(said.contains(&shared("twin-a", "twin-b")), "{said}");
}

#[test]
fn an_unsigned_ballot_committed_before_signing_began_is_legacy() {
    let tracker = Tracker::new(&[ISSUE]);
    tracker.vote(ISSUE, "elder", "rail");
    tracker.commit(Some("2026-10-01T00:00:00Z"));
    let (ok, said) = tracker.check();
    assert!(ok, "{said}");
    assert!(
        said.contains("legacy unsigned, committed before signing began: probe-r001 elder"),
        "{said}"
    );
}
