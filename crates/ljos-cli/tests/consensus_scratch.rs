//! `ljos consensus` on a scratch tracker the real vissue keeps. CI builds
//! vissue; a tree without it says so and skips. `--no-file` prints the
//! filing reading's settle and writes nothing. Ballots no record labels
//! share one model family in the settle; labelled ones count one family
//! each.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// A reading here takes under a tenth of a second (Intel Xeon, 8 cores,
/// e83534a). One still running after a minute is the hang, not a slow
/// machine, and the test ends it rather than waiting with it.
const HANG: Duration = Duration::from_secs(60);

/// How often a running reading is checked: a small part of one reading.
const POLL: Duration = Duration::from_millis(10);

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(bin).is_file()))
}

struct Scratch {
    dir: tempfile::TempDir,
    id: String,
}

impl Scratch {
    /// One issue holding `ballots` as `(voter, option)`. `None`, said on
    /// stderr, when vissue is not on PATH.
    fn new(ballots: &[(&str, &str)]) -> Option<Self> {
        if !on_path("vissue") {
            eprintln!("skip: vissue is not on PATH; CI builds it from the pinned commit");
            return None;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tracker")).unwrap();
        std::fs::create_dir_all(dir.path().join("home")).unwrap();
        std::fs::write(dir.path().join("gitconfig"), "").unwrap();
        let mut scratch = Self {
            dir,
            id: String::new(),
        };
        let filed = scratch.vissue(&["q", "pick a recall ranker", "-p", "demo"], None);
        scratch.id = String::from_utf8_lossy(&filed.stdout).trim().to_string();
        assert!(!scratch.id.is_empty(), "{filed:?}");
        for (voter, option) in ballots {
            let id = scratch.id.clone();
            scratch.vissue(&["vote", &id, "--for", option], Some(voter));
        }
        Some(scratch)
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn command(&self, bin: &Path) -> Command {
        let home = self.home();
        let mut cmd = Command::new(bin);
        cmd.current_dir(self.dir.path())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", &home)
            .env("GIT_CONFIG_GLOBAL", self.dir.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("VISSUE_ROOT", self.dir.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("PACKSET_URL", "off")
            .env("LJOS_SEAT", "ada")
            // vissue's own votes carry no signature note.
            .env("LJOS_UNSIGNED_BALLOTS", "allow")
            .stdin(Stdio::null());
        cmd
    }

    fn vissue(&self, args: &[&str], voter: Option<&str>) -> Output {
        let mut cmd = self.command(Path::new("vissue"));
        if let Some(voter) = voter {
            cmd.env("VISSUE_AGENT", voter);
        }
        let out = cmd.args(args).output().unwrap();
        assert!(out.status.success(), "vissue {args:?}: {out:?}");
        out
    }

    /// `ljos consensus ID ARGS`, ended and failed if it runs past [`HANG`].
    fn consensus(&self, args: &[&str]) -> (bool, String, String) {
        let out = self.dir.path().join("out");
        let err = self.dir.path().join("err");
        let mut child = self
            .command(Path::new(env!("CARGO_BIN_EXE_ljos")))
            .arg("consensus")
            .arg(&self.id)
            .args(args)
            .stdout(std::fs::File::create(&out).unwrap())
            .stderr(std::fs::File::create(&err).unwrap())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > HANG {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "ljos consensus {args:?} still ran after {HANG:?}:\n{}",
                    std::fs::read_to_string(&out).unwrap_or_default()
                );
            }
            std::thread::sleep(POLL);
        };
        (
            status.success(),
            std::fs::read_to_string(&out).unwrap(),
            std::fs::read_to_string(&err).unwrap(),
        )
    }

    fn json(&self) -> serde_json::Value {
        let (ok, out, err) = self.consensus(&["--no-file", "--json"]);
        assert!(ok, "{out}\n{err}");
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("not one JSON document ({e}): {out}"))
    }

    fn shown(&self) -> String {
        let out = self.vissue(&["show", &self.id, "--json"], None);
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn children(&self) -> String {
        let out = self.vissue(&["children", &self.id], None);
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

#[test]
fn no_file_prints_the_filing_settle_and_writes_nothing() {
    let Some(t) = Scratch::new(&[("ada", "bm25"), ("bob", "bm25"), ("cyd", "rrf")]) else {
        return;
    };
    let card = t.shown();
    let (ok, read, err) = t.consensus(&["--no-file"]);
    assert!(ok, "{read}\n{err}");
    assert!(
        read.contains("0.667") && read.contains("0.333"),
        "no shares: {read}\n{err}"
    );
    assert!(!read.contains("dissent:"), "{read}");
    assert_eq!(t.shown(), card, "--no-file wrote the issue");
    let kids = t.children();
    assert!(!kids.contains("Dissent holds"), "--no-file filed: {kids}");

    let (ok, filed, err) = t.consensus(&[]);
    assert!(ok, "{filed}\n{err}");
    assert!(
        t.children().contains("Dissent holds rrf"),
        "the filing reading filed nothing: {filed}\n{err}"
    );
    let settle: Vec<&str> = filed
        .lines()
        .filter(|line| !line.starts_with("dissent: "))
        .collect();
    assert_eq!(read.lines().collect::<Vec<_>>(), settle, "{filed}");
}

#[test]
fn three_unlabelled_ballots_are_one_family() {
    let Some(t) = Scratch::new(&[("ada", "bm25"), ("bob", "bm25"), ("cyd", "rrf")]) else {
        return;
    };
    let doc = t.json();
    assert_eq!(doc["families"]["voices"], 1, "{doc}");
    assert_eq!(
        doc["families"]["of"]["unknown"],
        serde_json::json!(["ada", "bob", "cyd"]),
        "{doc}"
    );
    let (ok, read, err) = t.consensus(&["--no-file"]);
    assert!(ok, "{read}\n{err}");
    assert!(
        read.contains("model families: 3 ballots from 1 family (unknown 3); "),
        "{read}"
    );
}

#[test]
fn two_labelled_ballots_are_two_families() {
    let labelled = [("judge:model-a", "rrf"), ("judge:model-b", "rrf")];
    let Some(t) = Scratch::new(&labelled) else {
        return;
    };
    let doc = t.json();
    assert_eq!(doc["families"]["voices"], 2, "{doc}");
    assert_eq!(
        doc["families"]["of"]["model-a"],
        serde_json::json!(["judge:model-a"]),
        "{doc}"
    );
    assert_eq!(
        doc["families"]["of"]["model-b"],
        serde_json::json!(["judge:model-b"]),
        "{doc}"
    );
    let (ok, read, err) = t.consensus(&["--no-file"]);
    assert!(ok, "{read}\n{err}");
    assert!(
        !read.contains("model families:"),
        "one ballot per family: {read}"
    );

    let mixed = [
        labelled[0],
        labelled[1],
        ("ada", "bm25"),
        ("bob", "bm25"),
        ("cyd", "bm25"),
    ];
    let Some(t) = Scratch::new(&mixed) else {
        return;
    };
    assert_eq!(t.json()["families"]["voices"], 3);
    let (ok, read, err) = t.consensus(&["--no-file"]);
    assert!(ok, "{read}\n{err}");
    assert!(
        read.contains(
            "model families: 5 ballots from 3 families (model-a 1, model-b 1, unknown 3); "
        ),
        "{read}"
    );
}

/// The model crate's settle weighs each family as one voter, so ten
/// unlabelled ballots do not outvote two from labelled families. The
/// tracker's settle takes no weights, so this needs `ljos-consensus`.
#[test]
fn ten_unlabelled_ballots_do_not_outvote_two_families() {
    if !on_path("ljos-consensus") {
        eprintln!("skip: ljos-consensus is not on PATH");
        return;
    }
    let seats: Vec<String> = (0..10).map(|n| format!("seat{n}")).collect();
    let mut ballots: Vec<(&str, &str)> = seats.iter().map(|s| (s.as_str(), "bm25")).collect();
    ballots.push(("judge:model-a", "rrf"));
    ballots.push(("judge:model-b", "rrf"));
    let Some(t) = Scratch::new(&ballots) else {
        return;
    };
    let doc = t.json();
    let settle = &doc["settle"];
    let share = |option: &str| {
        let at = settle["options"]
            .as_array()
            .and_then(|o| o.iter().position(|x| x == option))
            .unwrap_or_else(|| panic!("no {option} in {settle}"));
        settle["shares"][at].as_f64().unwrap()
    };
    assert!(share("rrf") > share("bm25"), "{settle}");
}
