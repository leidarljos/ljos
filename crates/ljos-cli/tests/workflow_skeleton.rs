//! A versioned Org skeleton files a ticket DAG, and the sign-off waits.

ljos_test_boot::harness!();

#[path = "common/sibling.rs"]
mod sibling;

use std::path::PathBuf;
use std::process::{Command, Output};

const SKEL: &str = "\
#+title: Release
#+version: 1

* Write the changelog
:PROPERTIES:
:STEP: changelog
:DEPENDS:
:KIND: do
:END:

Notes for {{version}}.

* Bump the version
:PROPERTIES:
:STEP: version
:DEPENDS: changelog
:KIND: do
:END:

* Sign off
:PROPERTIES:
:STEP: signoff
:DEPENDS: version
:KIND: signoff
:QUORUM: 2
:DEED: yes
:END:

Options: ship, hold
";

struct Board {
    dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    store: PathBuf,
    path: String,
}

impl Board {
    fn new() -> Self {
        let vissue = sibling::require("vissue").unwrap_or_else(|line| panic!("{line}"));
        let deedar = sibling::require("deedar").unwrap_or_else(|line| panic!("{line}"));
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        let store = dir.path().join("deeds");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(root.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let mut bins = vec![
            vissue.parent().unwrap().to_path_buf(),
            deedar.parent().unwrap().to_path_buf(),
        ];
        bins.sort();
        bins.dedup();
        bins.push(PathBuf::from("/usr/bin"));
        bins.push(PathBuf::from("/bin"));
        let path = std::env::join_paths(bins)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        Self {
            dir,
            root,
            home,
            store,
            path,
        }
    }

    fn cmd_as(&self, seat: &str, args: &[&str]) -> Command {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", seat)
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_HOOKD", "off")
            .env("DEEDAR_URL", format!("file://{}", self.store.display()))
            .env("DEEDAR_HOST_SIGNING_KEY", "off")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8");
        cmd
    }

    fn ljos_as(&self, seat: &str, args: &[&str]) -> Output {
        self.cmd_as(seat, args).output().unwrap()
    }

    fn ljos(&self, args: &[&str]) -> Output {
        self.ljos_as("saddle", args)
    }

    fn ok(&self, args: &[&str]) -> String {
        self.ok_as("saddle", args)
    }

    fn ok_as(&self, seat: &str, args: &[&str]) -> String {
        let out = self.ljos_as(seat, args);
        assert!(out.status.success(), "{args:?}\n{}", text(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn fail(&self, args: &[&str]) -> String {
        let out = self.ljos(args);
        assert!(!out.status.success(), "exit 0\n{}", text(&out));
        text(&out)
    }

    fn write_skel(&self, name: &str, body: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&path, body).unwrap();
        path
    }

    fn heading(&self, id: &str) -> vissue_core::IssueHeading {
        let layout = vissue_core::Layout::new(&self.root, "Software");
        let doc =
            vissue_core::IssueDoc::parse_file("demo", &layout.project_issues_path("demo")).unwrap();
        doc.headings
            .into_iter()
            .find(|h| h.id == id)
            .unwrap_or_else(|| panic!("{id} missing"))
    }

    fn mark_done(&self, id: &str) {
        let layout = vissue_core::Layout::new(&self.root, "Software");
        vissue_core::ops::update(&layout, id, Some("DONE"), None, None, None).unwrap();
    }
}

fn text(out: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn first_id(text: &str) -> String {
    for line in text.lines() {
        let word = line.split_whitespace().next().unwrap_or("");
        if word.contains('-') && !word.contains(':') {
            return word.to_string();
        }
    }
    panic!("no id in {text}");
}

fn filed(text: &str, key: &str) -> String {
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if words.next() == Some("filed") && words.next() == Some(key) {
            return words.next().unwrap_or("").to_string();
        }
    }
    panic!("no filed {key} in {text}");
}

fn example() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/workflows/release.org")
}

#[test]
fn define_names_the_line_and_a_repeat_is_a_no_op() {
    let board = Board::new();
    let bad = board.write_skel(
        "bad.org",
        "\
#+title: Gap
#+version: 1
* One
:PROPERTIES:
:STEP: one
:DEPENDS:
:KIND: do
:END:
* Two
:PROPERTIES:
:STEP: two
:DEPENDS:
:KIND: do
:END:
* Ship
:PROPERTIES:
:STEP: ship
:DEPENDS: one
:KIND: signoff
:END:
",
    );
    let err = board.fail(&["workflow", "define", bad.to_str().unwrap()]);
    assert!(err.contains("does not depend on two"), "{err}");
    assert!(err.contains("bad.org:"), "{err}");

    let cycle = board.write_skel(
        "cycle.org",
        "\
#+title: Loop
#+version: 1
* One
:PROPERTIES:
:STEP: one
:DEPENDS: two
:KIND: do
:END:
* Two
:PROPERTIES:
:STEP: two
:DEPENDS: one
:KIND: do
:END:
* Ship
:PROPERTIES:
:STEP: ship
:DEPENDS: one two
:KIND: signoff
:END:
",
    );
    let err = board.fail(&["workflow", "define", cycle.to_str().unwrap()]);
    assert!(err.contains("cycle through"), "{err}");
    assert!(err.contains("cycle.org:"), "{err}");

    let path = board.write_skel("release.org", SKEL);
    let stored = board.ok(&["workflow", "define", path.to_str().unwrap()]);
    assert!(stored.contains("stored release version 1"), "{stored}");
    assert!(stored.contains("deed deed-"), "{stored}");
    let again = board.ok(&["workflow", "define", path.to_str().unwrap()]);
    assert!(again.contains("already stored"), "{again}");

    let changed = SKEL.replace(
        "Notes for {{version}}.",
        "Notes for {{version}} were rewritten.",
    );
    let path2 = board.write_skel("release.org", &changed);
    let err = board.fail(&["workflow", "define", path2.to_str().unwrap()]);
    assert!(err.contains("bump #+version"), "{err}");
    assert!(err.contains("release.org:2:"), "{err}");
}

#[test]
fn start_is_idempotent_and_ready_follows_the_edges() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    let missing = board.fail(&["workflow", "--start", "release", &parent]);
    assert!(missing.contains("version"), "{missing}");
    assert!(missing.contains("--var"), "{missing}");

    let started = board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let changelog = filed(&started, "changelog");
    let version = filed(&started, "version");
    let signoff = filed(&started, "signoff");
    assert!(
        started.contains(&format!("{version} waits on {changelog}")),
        "{started}"
    );
    let shown = board.ok(&["show", &changelog]);
    assert!(shown.contains("1.2.0"), "{shown}");
    assert_eq!(
        board.heading(&version).blocked_by(),
        vec![changelog.clone()]
    );
    assert_eq!(board.heading(&signoff).blocked_by(), vec![version.clone()]);

    let again = board.ok(&["workflow", "--start", "release", &parent]);
    assert!(again.contains("already runs release version 1"), "{again}");
    assert!(again.contains("nothing new"), "{again}");
    assert!(!again.contains("filed "), "{again}");

    let ready = board.ok(&["ready", "--all"]);
    assert!(ready.contains(&changelog), "{ready}");
    assert!(!ready.contains(&version), "{ready}");
    board.mark_done(&changelog);
    let ready = board.ok(&["ready", "--all"]);
    assert!(ready.contains(&version), "{ready}");
    assert!(!ready.contains(&signoff), "{ready}");

    let status = board.ok(&["workflow", &parent]);
    assert!(status.contains("release version 1"), "{status}");
    assert!(status.contains("ready: version"), "{status}");
    assert!(status.contains("changelog  do  DONE"), "{status}");
    assert!(status.contains("blockers: none"), "{status}");

    let blocker =
        first_id(&board.ok(&["file", "CI failed on the changelog", "--parent", &changelog]));
    let status = board.ok(&["workflow", &parent]);
    assert!(status.contains(&blocker), "{status}");
    assert!(status.contains("under changelog"), "{status}");
    let refused = board.fail(&["complete", &signoff]);
    assert!(refused.contains("signoff:"), "{refused}");
    assert!(refused.contains("is missing"), "{refused}");
    assert!(refused.contains("is open under changelog"), "{refused}");
    board.mark_done(&blocker);
}

#[test]
fn signoff_refuses_until_the_run_the_votes_and_the_deed_are_in() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    let stored = board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let accession = stored
        .lines()
        .find_map(|line| line.strip_prefix("deed "))
        .unwrap_or("")
        .trim()
        .to_string();
    assert!(accession.starts_with("deed-"), "{stored}");
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    let started = board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let changelog = filed(&started, "changelog");
    let version = filed(&started, "version");
    let signoff = filed(&started, "signoff");

    let early = board.fail(&["finish", &signoff]);
    assert!(early.contains("changelog is TODO"), "{early}");
    assert!(early.contains("consensus has 0 of 2 ballots"), "{early}");
    assert!(early.contains("no deed is named"), "{early}");
    assert!(!early.contains("a person closes"), "{early}");

    board.mark_done(&changelog);
    board.mark_done(&version);
    board.ok_as(
        "alice",
        &["vote", &signoff, "--for", "hold", "--used", "none"],
    );

    let held = board.fail(&["complete", &signoff]);
    assert!(
        held.contains("settled hold, not ship") || held.contains("consensus has 1 of 2"),
        "{held}"
    );
}

#[test]
fn two_ship_votes_and_a_deed_let_the_signoff_past_the_check() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    let stored = board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let accession = stored
        .lines()
        .find_map(|line| line.strip_prefix("deed "))
        .unwrap()
        .trim()
        .to_string();
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    let started = board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let changelog = filed(&started, "changelog");
    let version = filed(&started, "version");
    let signoff = filed(&started, "signoff");
    board.mark_done(&changelog);
    board.mark_done(&version);

    board.ok_as(
        "alice",
        &["vote", &signoff, "--for", "ship", "--used", "none"],
    );
    let one = board.fail(&["complete", &signoff]);
    assert!(one.contains("consensus has 1 of 2 ballots"), "{one}");

    board.ok_as(
        "bob",
        &["vote", &signoff, "--for", "ship", "--used", "none"],
    );
    let nodeed = board.fail(&["complete", &signoff]);
    assert!(nodeed.contains("no deed is named"), "{nodeed}");
    assert!(!nodeed.contains("consensus has"), "{nodeed}");

    board.ok(&["deed", &signoff, "--add", &accession]);
    let evidence = board.ok(&["evidence", &accession]);
    assert!(!evidence.is_empty(), "{evidence}");

    let finished = board.fail(&["finish", &signoff]);
    assert!(
        finished.contains("is not claimed and stays TODO"),
        "{finished}"
    );
    assert!(!finished.contains("is missing"), "{finished}");
    assert!(!finished.contains("no deed is named"), "{finished}");
}

#[test]
fn revise_stores_the_next_version_after_the_proposal_settles() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    board.ok(&[
        "note",
        &parent,
        "the changelog step wanted a link to the tag",
    ]);
    let filed = board.ok(&["workflow", "revise", "release", "--from", &parent]);
    assert!(filed.contains("Options: keep, revise"), "{filed}");
    let proposal = filed
        .lines()
        .find_map(|line| line.strip_prefix("filed proposal "))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let early = board.fail(&[
        "workflow",
        "revise",
        "release",
        "--apply",
        path.to_str().unwrap(),
    ]);
    assert!(early.contains("0 of 2"), "{early}");

    board.ok_as(
        "alice",
        &["vote", &proposal, "--for", "revise", "--used", "none"],
    );
    board.ok_as(
        "bob",
        &["vote", &proposal, "--for", "revise", "--used", "none"],
    );
    let applied = board.ok(&[
        "workflow",
        "revise",
        "release",
        "--apply",
        path.to_str().unwrap(),
    ]);
    assert!(applied.contains("stored release version 2"), "{applied}");
    assert!(applied.contains(&proposal), "{applied}");
    let status = board.ok(&["workflow", &parent]);
    assert!(
        status.contains("version 2"),
        "revise left the run on the old version:\n{status}"
    );
    let again = board.ok(&["workflow", "--start", "release", &parent]);
    assert!(
        again.contains("version 2"),
        "start did not follow the version revise stored:\n{again}"
    );
    let disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        disk.contains("#+version: 1"),
        "apply rewrites the stored copy, not the file\n{disk}"
    );

    let second = board.ok(&[
        "workflow",
        "revise",
        "release",
        "--apply",
        path.to_str().unwrap(),
    ]);
    assert!(
        second.contains("already applied"),
        "a second apply stored another version:\n{second}"
    );
    assert!(
        !second.contains("version 3"),
        "a second apply bumped the version:\n{second}"
    );
    let status = board.ok(&["workflow", &parent]);
    assert!(status.contains("version 2"), "{status}");
    assert!(!status.contains("version 3"), "{status}");
}

#[test]
fn render_refuses_a_secret_split_across_lines() {
    let board = Board::new();
    let skel = SKEL.replace("Notes for {{version}}.", "gh\np_1234567890abcdef1234");
    let path = board.write_skel("split-secret.org", &skel);
    let out = board.dir.path().join("split-secret-public.org");
    let err = board.fail(&[
        "workflow",
        "render",
        "split-secret",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(!out.exists(), "render wrote a page:\n{err}");
    assert!(err.contains("secret"), "{err}");
    assert!(err.contains("line "), "{err}");
}

#[test]
fn render_refuses_an_email_split_across_lines() {
    let board = Board::new();
    let skel = SKEL.replace("Notes for {{version}}.", "ada (at)\nexample.com");
    let path = board.write_skel("split-mail.org", &skel);
    let out = board.dir.path().join("split-mail-public.org");
    let err = board.fail(&[
        "workflow",
        "render",
        "split-mail",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(!out.exists(), "render wrote a page:\n{err}");
    assert!(err.contains("email"), "{err}");
    assert!(err.contains("line "), "{err}");
}

#[test]
fn render_emits_no_babel_source_block() {
    let board = Board::new();
    let path = board.write_skel("plain.org", SKEL);
    let out = board.dir.path().join("plain-public.org");
    board.ok(&[
        "workflow",
        "render",
        "plain",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    let page = std::fs::read_to_string(&out).unwrap();
    let lower = page.to_ascii_lowercase();
    assert!(!lower.contains("#+begin_src"), "{page}");
    assert!(!lower.contains("src_"), "{page}");
    assert!(!lower.contains("#+call"), "{page}");
    assert!(!lower.contains("#+include"), "{page}");
    assert!(!lower.contains(":results"), "{page}");
    assert!(page.contains("#+begin_example"), "{page}");
    assert!(page.contains("changelog -> version"), "{page}");
}

#[test]
fn render_is_deterministic_and_check_rejects_a_drift() {
    let board = Board::new();
    let src = example();
    let out = board.dir.path().join("page.org");
    let out_s = out.to_str().unwrap();
    let src_s = src.to_str().unwrap();
    board.ok(&[
        "workflow", "render", "release", "--file", src_s, "--out", out_s,
    ]);
    let first = std::fs::read_to_string(&out).unwrap();
    board.ok(&[
        "workflow", "render", "release", "--file", src_s, "--out", out_s,
    ]);
    let second = std::fs::read_to_string(&out).unwrap();
    assert_eq!(first, second);
    assert!(first.contains("#+title: Release"), "{first}");
    assert!(
        !first.to_ascii_lowercase().contains("#+begin_src"),
        "{first}"
    );
    assert!(first.contains("#+begin_example"), "{first}");
    assert!(first.contains("changelog -> version"), "{first}");
    assert!(first.contains("{{version}}"), "{first}");
    assert!(
        first.find("* Write the changelog").unwrap() < first.find("* Tag the release").unwrap()
    );
    let checked = board.ok(&[
        "workflow", "render", "release", "--file", src_s, "--out", out_s, "--check",
    ]);
    assert!(checked.contains("matches"), "{checked}");

    let mut dirty = first.clone();
    dirty.push_str("Reviewed by Claude in /home/alice/src.\n");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&out, &dirty).unwrap();
    let err = board.fail(&[
        "workflow", "render", "release", "--file", src_s, "--out", out_s, "--check",
    ]);
    assert!(
        err.contains("differs") || err.contains("local path"),
        "{err}"
    );
}

#[test]
fn an_open_child_or_blocker_keeps_the_step_off_ready() {
    let board = Board::new();
    let path = board.write_skel(
        "tiny.org",
        "\
#+title: Tiny
#+version: 1

* Do the root
:PROPERTIES:
:STEP: root
:DEPENDS:
:KIND: do
:END:

* Sign off
:PROPERTIES:
:STEP: sign
:DEPENDS: root
:KIND: signoff
:END:
",
    );
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Tiny run", "--top", "-p", "demo"]));
    let started = board.ok(&["workflow", "--start", "tiny", &parent]);
    let root = filed(&started, "root");
    let status = board.ok(&["workflow", &parent]);
    assert!(status.contains("ready: root"), "{status}");

    let child = first_id(&board.ok(&["file", "CI failed on the root step", "--parent", &root]));
    let status = board.ok(&["workflow", &parent]);
    assert!(
        !status.contains("ready: root"),
        "an open child still counts as ready:\n{status}"
    );
    assert!(status.contains("ready: none"), "{status}");
    board.mark_done(&child);
    let status = board.ok(&["workflow", &parent]);
    assert!(status.contains("ready: root"), "{status}");

    let other = first_id(&board.ok(&["file", "Waiting on legal", "--top", "-p", "demo"]));
    board.ok(&["block", &root, "--on", &other]);
    let status = board.ok(&["workflow", &parent]);
    assert!(
        !status.contains("ready: root"),
        "an open blocker still counts as ready:\n{status}"
    );
    assert!(status.contains("ready: none"), "{status}");
    board.mark_done(&other);
    let status = board.ok(&["workflow", &parent]);
    assert!(status.contains("ready: root"), "{status}");
}

#[test]
fn start_restores_a_missing_edge() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    let started = board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let changelog = filed(&started, "changelog");
    let version = filed(&started, "version");
    board.ok(&["block", &version, "--off", &changelog]);
    assert!(
        board.heading(&version).blocked_by().is_empty(),
        "the edge is still present"
    );

    let again = board.ok(&["workflow", "--start", "release", &parent]);
    assert!(
        again.contains("repaired"),
        "a second start said nothing about the missing edge:\n{again}"
    );
    assert!(again.contains(&version), "{again}");
    assert!(again.contains(&changelog), "{again}");
    assert!(!again.contains("nothing new"), "{again}");
    assert_eq!(
        board.heading(&version).blocked_by(),
        vec![changelog.clone()]
    );

    let third = board.ok(&["workflow", "--start", "release", &parent]);
    assert!(third.contains("nothing new"), "{third}");
    assert!(!third.contains("repaired"), "{third}");
    assert_eq!(board.heading(&version).blocked_by(), vec![changelog]);
}

#[test]
fn start_keeps_the_pinned_version_after_a_newer_define() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let v2 = SKEL.replace("#+version: 1", "#+version: 2").replace(
        "Notes for {{version}}.",
        "Notes for {{version}}.\nA second pass.",
    );
    board.write_skel("release.org", &v2);
    let stored = board.ok(&["workflow", "define", path.to_str().unwrap()]);
    assert!(stored.contains("stored release version 2"), "{stored}");

    let again = board.ok(&["workflow", "--start", "release", &parent]);
    assert!(
        again.contains("version 1"),
        "start moved off the pinned version:\n{again}"
    );
    assert!(
        !again.contains("version 2"),
        "start followed the newest stored skeleton:\n{again}"
    );
    let status = board.ok(&["workflow", &parent]);
    assert!(status.contains("version 1"), "{status}");
    assert!(!status.contains("version 2"), "{status}");
}

#[test]
fn render_keeps_the_precision_corpus_byte_for_byte() {
    let board = Board::new();
    let corpus = "\
hello3-mini
xclaude is a word
firmware 1.2.3.4.
ships 1.2.3.4
release 1.2.3.4
section 3.4.5.6.
Requires Rust 1.89
Python 3.12.3
0.5
median 139.1 ms
seat belt.
add::fee.
key=value.
'the token is in the vault' (no value).
Cargo.toml, v1.2.3, e.g., i.e., std::fs::read, =std::fs::read=, Vec::new, 12:30, 1:02:03, TCP/IP.
[[https://example.com][link]]
/dev/null
$npm_package_version
pass: all tests green
";
    for line in corpus.lines() {
        let hits = ljos_cli::scrub::scan_author(line, 1);
        assert!(hits.is_empty(), "{line:?}: {hits:?}");
    }
    for line in ["کتاب\u{200c}ها", "क्\u{200d}ष"] {
        let hits = ljos_cli::scrub::scan_author(line, 1);
        assert_eq!(
            hits,
            vec!["line 1 contains character outside the public set"],
            "{line:?}"
        );
    }
    let body = SKEL.replace("Notes for {{version}}.", corpus.trim_end());
    let path = board.write_skel("prose.org", &body);
    let out = board.dir.path().join("prose-public.org");
    board.ok_as(
        "changelog",
        &[
            "workflow",
            "render",
            "prose",
            "--file",
            path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    let page = std::fs::read_to_string(out).unwrap();
    assert!(page.contains(corpus), "prose changed:\n{page}");
}

#[test]
fn render_refuses_the_export_and_credential_corpus() {
    let leaks = [
        ("{{{author}}}", "Org structure"),
        ("@@html:<b>x</b>@@", "Org structure"),
        ("src_sh{whoami}", "Org structure"),
        ("call_whoami()", "Org structure"),
        ("#+begin_export html", "Org structure"),
        ("#+call: whoami()", "Org structure"),
        ("#+include: secrets.org", "Org structure"),
        ("#+setupfile: local.org", "Org structure"),
        ("[[elisp:(message \"hi\")][go]]", "Org structure"),
        ("[[shell:whoami][go]]", "Org structure"),
        ("[[file:/etc/passwd][go]]", "private path"),
        ("<img src=x onerror=alert(1)>", "Org structure"),
        ("Claude Opus 4.1", "tool or model name"),
        ("Sonnet 4.5", "tool or model name"),
        ("Cursor", "tool or model name"),
        ("Copilot", "tool or model name"),
        ("ChatGPT", "tool or model name"),
        ("Anthropic", "tool or model name"),
        (concat!("xap", "p-1-ABCDEFGHIJ"), "secret"),
        (
            concat!("https://hooks.slack.com", "/services/T00/B00/secretvalue"),
            "secret",
        ),
        (concat!("glpat", "-abcdefghij"), "secret"),
        (concat!("AccountKey=", "abcDEF123456"), "secret"),
        (concat!("\"apiKey\": \"", "abcDEF123456\""), "secret"),
        (concat!("clientSecret=", "abcDEF123456"), "secret"),
        (concat!("PGPASSWORD=", "abcDEF123456"), "secret"),
        ("curl -u ada:s3cret", "secret"),
        ("0123456789abcdef0123456789abcdef", "secret"),
        ("abcdefghijklmnopqrstuvwxyz0123456789ABCD", "secret"),
        (
            concat!("ey", "JhbGciOiJub25lIn0.ey", "JzdWIiOiIxIn0"),
            "secret",
        ),
        ("ada (at) example.com", "email"),
        ("ada [at] example.com", "email"),
        ("ping 127.1", "address"),
        ("ip 0x7f000001", "address"),
        ("$HOME/.ssh/id_rsa", "private path"),
        ("~ada/.ssh/id_rsa", "private path"),
        (
            "\u{0563}hp_abcdefghijklmnop",
            "character outside the public set",
        ),
        ("\u{0581}hp_abcdefghijklmnop", "secret"),
    ];
    let board = Board::new();
    let body = leaks
        .iter()
        .map(|(line, _)| *line)
        .collect::<Vec<_>>()
        .join("\n");
    let skel = SKEL.replace("Notes for {{version}}.", &body);
    let path = board.write_skel("hostile.org", &skel);
    let out = board.dir.path().join("hostile-public.org");
    let err = board.fail(&[
        "workflow",
        "render",
        "hostile",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(!out.exists(), "render wrote a page:\n{err}");
    for (offset, (line, class)) in leaks.iter().enumerate() {
        let number = 11 + offset;
        let needle = format!("line {number} contains {class}");
        assert!(err.contains(&needle), "{line:?} missing {needle}:\n{err}");
    }
}

#[test]
fn a_skeleton_name_cannot_leave_the_store() {
    let board = Board::new();
    let outside = board.dir.path().join("outside.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&outside, "{\"name\":\"escaped\"}").unwrap();
    let err = board.fail(&["workflow", "render", "../outside"]);
    assert!(err.contains("is not a skeleton name"), "{err}");
    assert_eq!(
        std::fs::read_to_string(&outside).unwrap(),
        "{\"name\":\"escaped\"}"
    );
}

#[test]
fn a_lost_applied_marker_does_not_store_the_same_body() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let filed = board.ok(&["workflow", "revise", "release", "--from", &parent]);
    let proposal = filed
        .lines()
        .find_map(|line| line.strip_prefix("filed proposal "))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    board.ok_as(
        "alice",
        &["vote", &proposal, "--for", "revise", "--used", "none"],
    );
    board.ok_as(
        "bob",
        &["vote", &proposal, "--for", "revise", "--used", "none"],
    );
    board.ok(&[
        "workflow",
        "revise",
        "release",
        "--apply",
        path.to_str().unwrap(),
    ]);
    let index = board
        .home
        .join(".state")
        .join("ljos")
        .join("workflows")
        .join("release.json");
    let mut stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index).unwrap()).unwrap();
    assert_eq!(stored["version"], 2);
    stored["applied"] = serde_json::Value::Null;
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&index, serde_json::to_string_pretty(&stored).unwrap()).unwrap();
    let again = board.ok(&[
        "workflow",
        "revise",
        "release",
        "--apply",
        path.to_str().unwrap(),
    ]);
    assert!(again.contains("already applied"), "{again}");
    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index).unwrap()).unwrap();
    assert_eq!(stored["version"], 2, "{stored}");
}

#[test]
fn an_applied_proposal_is_closed_so_the_run_can_revise_again() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let filed = board.ok(&["workflow", "revise", "release", "--from", &parent]);
    let proposal = filed
        .lines()
        .find_map(|line| line.strip_prefix("filed proposal "))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    board.ok_as(
        "alice",
        &["vote", &proposal, "--for", "revise", "--used", "none"],
    );
    board.ok_as(
        "bob",
        &["vote", &proposal, "--for", "revise", "--used", "none"],
    );
    board.ok(&[
        "workflow",
        "revise",
        "release",
        "--apply",
        path.to_str().unwrap(),
    ]);
    assert_eq!(board.heading(&proposal).state, "DONE");
    let next = SKEL.replace(
        "Notes for {{version}}.",
        "Notes for {{version}} after the first revise.",
    );
    let next_path = board.write_skel("release-next.org", &next);
    let filed = board.ok(&["workflow", "revise", "release", "--from", &parent]);
    assert!(
        filed.contains("filed proposal"),
        "the closed proposal still blocked revise:\n{filed}"
    );
    let proposal = filed
        .lines()
        .find_map(|line| line.strip_prefix("filed proposal "))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    board.ok_as(
        "alice",
        &["vote", &proposal, "--for", "revise", "--used", "none"],
    );
    board.ok_as(
        "bob",
        &["vote", &proposal, "--for", "revise", "--used", "none"],
    );
    let applied = board.ok(&[
        "workflow",
        "revise",
        "release",
        "--apply",
        next_path.to_str().unwrap(),
    ]);
    assert!(applied.contains("stored release version 3"), "{applied}");
}

#[test]
fn concurrent_define_and_apply_keep_a_readable_index() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    let path_s = path.to_str().unwrap().to_string();
    for round in 1..=3 {
        let body = SKEL.replace("#+version: 1", &format!("#+version: {round}"));
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&path, &body).unwrap();
        let mut kids = Vec::new();
        for _ in 0..8 {
            kids.push(
                board
                    .cmd_as("saddle", &["workflow", "define", &path_s])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
        }
        for kid in kids {
            let out = kid.wait_with_output().unwrap();
            let said = text(&out);
            assert!(out.status.success(), "define round {round} failed:\n{said}");
            assert!(!said.contains("EOF while parsing"), "{said}");
            assert!(!said.to_ascii_lowercase().contains("deedar"), "{said}");
        }
    }
    let index = board
        .home
        .join(".state")
        .join("ljos")
        .join("workflows")
        .join("release.json");
    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index).unwrap()).unwrap();
    assert_eq!(stored["version"], 3);

    let parent = first_id(&board.ok(&["file", "Concurrent revise", "--top", "-p", "demo"]));
    board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    for round in 1..=3 {
        let body = SKEL.replace("#+version: 1", "#+version: 3").replace(
            "Notes for {{version}}.",
            &format!("Notes for round {round}."),
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&path, &body).unwrap();
        let filed = board.ok(&["workflow", "revise", "release", "--from", &parent]);
        assert!(
            filed.contains("filed proposal"),
            "round {round} could not file:\n{filed}"
        );
        let proposal = filed
            .lines()
            .find_map(|line| line.strip_prefix("filed proposal "))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        board.ok_as(
            "alice",
            &["vote", &proposal, "--for", "revise", "--used", "none"],
        );
        board.ok_as(
            "bob",
            &["vote", &proposal, "--for", "revise", "--used", "none"],
        );
        let mut kids = Vec::new();
        for _ in 0..8 {
            kids.push(
                board
                    .cmd_as(
                        "saddle",
                        &["workflow", "revise", "release", "--apply", &path_s],
                    )
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
        }
        for kid in kids {
            let out = kid.wait_with_output().unwrap();
            let said = text(&out);
            assert!(out.status.success(), "apply round {round} failed:\n{said}");
            assert!(!said.contains("EOF while parsing"), "{said}");
        }
        let stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&index).unwrap()).unwrap();
        assert!(stored["version"].as_i64().unwrap() >= 3, "{stored}");
    }
}

#[test]
fn render_refuses_each_sensitive_class_and_names_the_line() {
    let cases = [
        ("GITHUB_TOKEN = zzz", "secret"),
        ("export PASSWORD = hunter2", "secret"),
        ("pass: hunter2", "secret"),
        ("Authorization: token abc123def456", "secret"),
        ("https://user:pw@example.com/x", "secret"),
        ("file:~/notes.org", "private path"),
        ("~/x", "private path"),
        (r"C:\Users\alice\notes.org", "private path"),
        ("[[file:../secret.org][notes]]", "private path"),
        ("connect 203.0.113.8", "address"),
        ("10.0.0.1", "address"),
        ("2001:db8::1", "address"),
        ("db.internal", "address"),
        ("a@exämple.com", "email"),
        ("GPT-4", "tool or model name"),
        ("gpt-5.6", "tool or model name"),
        ("Claude 3", "tool or model name"),
        ("Claude Sonnet 4", "tool or model name"),
        ("Gemini 2.5", "tool or model name"),
        ("o3-mini", "tool or model name"),
        ("Qwen2", "tool or model name"),
        ("DeepSeek-V3", "tool or model name"),
        ("Llama 3", "tool or model name"),
        ("AI-generated", "tool or model name"),
        ("subagent", "tool or model name"),
        ("Co-authored-by", "tool or model name"),
    ];
    for (prose, class) in cases {
        let board = Board::new();
        let body = SKEL.replace("Notes for {{version}}.", prose);
        let path = board.write_skel("leak.org", &body);
        let out = board.dir.path().join("leak-public.org");
        let result = board.ljos(&[
            "workflow",
            "render",
            "leak",
            "--file",
            path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        assert!(
            !result.status.success(),
            "{prose:?} was accepted:\n{}",
            text(&result)
        );
        let err = text(&result);
        assert!(
            err.contains("line "),
            "{prose:?} did not name a line:\n{err}"
        );
        assert!(
            err.contains(class),
            "{prose:?} did not name {class:?}:\n{err}"
        );
        assert!(!out.exists(), "{prose:?} wrote an output");
    }
}

#[test]
fn render_refuses_the_leak_corpus_and_names_each_line() {
    let cases = [
        (
            concat!("token hf", "_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789"),
            "secret",
        ),
        (concat!("sk", "_live_51Habcdefghijklmnopqrstuv"), "secret"),
        (concat!("sk", "_test_51Habcdefghijklmnopqrstuv"), "secret"),
        (concat!("rk", "_live_51Habcdefghijklmnopqrstuv"), "secret"),
        (
            concat!(
                "ey",
                "JhbGciOiJIUzI1NiJ9.ey",
                "JzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N"
            ),
            "secret",
        ),
        (concat!("-----BEGIN RSA PRIVATE", " KEY-----"), "secret"),
        ("-----BEGIN CERTIFICATE-----", "secret"),
        ("see https://user@example.com/x today", "secret"),
        ("redis://:pw@cache/0", "secret"),
        ("postgres://alice:secret@db/app", "secret"),
        ("mysql://alice@db/app", "secret"),
        ("mongodb+srv://alice:pw@cluster/db", "secret"),
        ("amqp://alice:pw@mq/vhost", "secret"),
        ("http://alice@203.0.113.5/x", "secret"),
        ("X-API-Key: v", "secret"),
        ("X-Auth-Token: abcdef", "secret"),
        ("Proxy-Authorization: Basic dXNlcjpw", "secret"),
        ("Authorization: Bearer abcdef", "secret"),
        ("Authorization: token abcdef", "secret"),
        ("Authorization: Basic X", "secret"),
        ("see file:secret.org now", "private path"),
        ("see [[file:secret.org][notes]] now", "private path"),
        ("[[elisp:(message \"hi\")][go]]", "private path"),
        ("[[shell:uname][go]]", "private path"),
        ("[[help:org][h]]", "private path"),
        ("[[info:emacs][i]]", "private path"),
        ("see ~alice/.ssh/id_rsa now", "private path"),
        (
            "hello\u{200b}world",
            "character U+200B outside the public set",
        ),
        (
            "hello\u{202e}world",
            "character U+202E outside the public set",
        ),
        ("\u{feff}hello", "character U+FEFF outside the public set"),
        (
            "hello\u{2066}world",
            "character U+2066 outside the public set",
        ),
        (
            "Reviewed by G\u{0420}\u{0422}-4 today",
            "tool or model name",
        ),
        ("\u{0422}\u{041e}KEN=hunter2", "secret"),
        ("\u{0410}KIAIOSFODNN7EXAMPLE", "secret"),
        ("ann@exampl\u{0301}e.com", "email"),
        ("ann@example.xn--p1ai", "email"),
        ("ping 127.0.0.1 now", "address"),
        ("ping 169.254.1.1 now", "address"),
        ("server at 1.2.3.4", "address"),
        ("GPT4 wrote the note", "tool or model name"),
        ("Claude3 wrote the note", "tool or model name"),
        ("Gemini2 wrote the note", "tool or model name"),
        ("Llama3 wrote the note", "tool or model name"),
        (concat!("gh", "p_1234567890abcdef"), "secret"),
        (concat!("AKI", "AIOSFODNN7EXAMPLE"), "secret"),
    ];
    let board = Board::new();
    let body = cases
        .iter()
        .map(|(line, _)| *line)
        .collect::<Vec<_>>()
        .join("\n");
    let org = SKEL.replace("Notes for {{version}}.", &body);
    let path = board.write_skel("leaks.org", &org);
    let text = std::fs::read_to_string(&path).unwrap();
    let out = board.dir.path().join("leaks-public.org");
    let err = board.fail(&[
        "workflow",
        "render",
        "leaks",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(err.contains("public render refused"), "{err}");
    assert!(!out.exists(), "refused render wrote {}", out.display());
    for (prose, _) in cases {
        let line_no = text
            .lines()
            .position(|line| line == prose)
            .unwrap_or_else(|| panic!("missing line {prose:?}"))
            + 1;
        let hit = format!("line {line_no} contains");
        assert!(err.contains(&hit), "missing {hit} for {prose:?}:\n{err}");
    }
}

#[test]
fn render_refuses_a_path_or_address_written_with_look_alike_marks() {
    let cases = [
        (
            "see \u{2215}home\u{2215}alice\u{2215}notes.org now",
            "private path",
        ),
        (
            "see \u{2044}home\u{2044}alice\u{2044}notes.org now",
            "private path",
        ),
        (
            "see \u{29f8}home\u{29f8}alice\u{29f8}notes.org now",
            "private path",
        ),
        (
            "ping 10\u{3002}0\u{3002}0\u{3002}5 now",
            "character outside the public set",
        ),
        (
            "ping 192\u{ff61}168\u{ff61}1\u{ff61}20 now",
            "character outside the public set",
        ),
        ("ping 10\u{0660}0\u{0701}0\u{0660}5 now", "address"),
        (
            "see \u{2571}home\u{30ce}alice\u{2216}notes.org now",
            "private path",
        ),
        ("see %2Fhome%2Falice%2Fnotes.org now", "private path"),
    ];
    for (prose, class) in cases {
        let board = Board::new();
        let body = SKEL.replace("Notes for {{version}}.", prose);
        let path = board.write_skel("marks.org", &body);
        let text = std::fs::read_to_string(&path).unwrap();
        let out = board.dir.path().join("marks-public.org");
        let err = board.fail(&[
            "workflow",
            "render",
            "marks",
            "--file",
            path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        let line_no = text
            .lines()
            .position(|line| line == prose)
            .unwrap_or_else(|| panic!("missing line {prose:?}"))
            + 1;
        let hit = format!("line {line_no} contains {class}");
        assert!(err.contains(&hit), "missing {hit} for {prose:?}:\n{err}");
        assert!(!out.exists(), "{prose:?} wrote an output");
    }
}

#[test]
fn render_refuses_var_breaks_and_org_titles() {
    let board = Board::new();
    let titled = SKEL
        .replace("* Write the changelog", "* Ship {{version}}")
        .replace("Notes for {{version}}.", "Notes only.");
    let path = board.write_skel("var.org", &titled);
    let out = board.dir.path().join("var-public.org");
    let err = board.fail(&[
        "workflow",
        "render",
        "var",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--var",
        "version=1.2.0\n* Injected heading",
    ]);
    assert!(err.contains("line break or control character"), "{err}");
    assert!(!out.exists(), "a broken `--var` wrote {}", out.display());
    let err = board.fail(&[
        "workflow",
        "render",
        "var",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--var",
        "version=1.2.0\u{0001}bad",
    ]);
    assert!(err.contains("line break or control character"), "{err}");
    assert!(!out.exists());

    let org_title = SKEL.replace("* Write the changelog", "* #+begin_src shell");
    let org_path = board.write_skel("title.org", &org_title);
    let err = board.fail(&[
        "workflow",
        "render",
        "title",
        "--file",
        org_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(err.contains("Org structure"), "{err}");
    assert!(!out.exists());
}

#[test]
fn render_refuses_a_role_that_names_a_seat() {
    let board = Board::new();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        board.root.join("vissue.toml"),
        "prefix = \"Software\"\nseats = [\"saddle\"]\n",
    )
    .unwrap();
    let body = SKEL.replace(":KIND: do\n:END:", ":KIND: do\n:ROLE: saddle\n:END:");
    let path = board.write_skel("role.org", &body);
    let out = board.dir.path().join("role-public.org");
    let err = board.fail(&[
        "workflow",
        "render",
        "role",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(err.contains("seat name"), "{err}");
    assert!(err.contains("line "), "{err}");
    assert!(!out.exists());

    let author = SKEL.replace(":KIND: do\n:END:", ":KIND: do\n:ROLE: author\n:END:");
    let author_path = board.write_skel("author.org", &author);
    let author_out = board.dir.path().join("author-public.org");
    board.ok(&[
        "workflow",
        "render",
        "author",
        "--file",
        author_path.to_str().unwrap(),
        "--out",
        author_out.to_str().unwrap(),
    ]);
    let page = std::fs::read_to_string(&author_out).unwrap();
    assert!(page.contains(":ROLE: author\n"), "{page}");
}

#[test]
fn check_reports_a_secret_on_a_generated_line() {
    let board = Board::new();
    let path = board.write_skel("clean.org", SKEL);
    let out = board.dir.path().join("clean-public.org");
    let out_s = out.to_str().unwrap();
    board.ok(&[
        "workflow",
        "render",
        "clean",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out_s,
    ]);
    let page = std::fs::read_to_string(&out).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        &out,
        format!("{page}:STEP: {}\n", concat!("gh", "p_1234567890abcdef")),
    )
    .unwrap();
    let err = board.fail(&[
        "workflow",
        "render",
        "clean",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out_s,
        "--check",
    ]);
    assert!(err.contains("secret"), "{err}");
    assert!(err.contains("line "), "{err}");
}

#[test]
fn render_uses_only_declared_seats() {
    let board = Board::new();
    let body = SKEL.replace(
        "Notes for {{version}}.",
        "Write the changelog under LJOS_SEAT=changelog.",
    );
    let path = board.write_skel("seat-independent.org", &body);
    let out = board.dir.path().join("seat-independent-public.org");
    board.ok_as(
        "changelog",
        &[
            "workflow",
            "render",
            "seat-independent",
            "--file",
            path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    let first = std::fs::read_to_string(&out).unwrap();
    assert!(first.contains("LJOS_SEAT=changelog"), "{first}");

    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        board.root.join("vissue.toml"),
        "prefix = \"Software\"\nseats = [\"saddle\"]\n",
    )
    .unwrap();
    let named = SKEL.replace("Notes for {{version}}.", "Reviewed by saddle.");
    let named_path = board.write_skel("seat-declared.org", &named);
    let err = board.fail(&[
        "workflow",
        "render",
        "seat-declared",
        "--file",
        named_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(err.contains("seat name"), "{err}");
}

#[test]
fn normalized_org_injection_is_refused_and_safe_output_lints() {
    let board = Board::new();
    let safe = board.write_skel("safe.org", SKEL);
    let out = board.dir.path().join("safe-public.org");
    board.ok(&[
        "workflow",
        "render",
        "safe",
        "--file",
        safe.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let lint = Command::new("/usr/bin/emacs")
        .args([
            "--batch",
            "-Q",
            "--eval",
            "(progn (require (quote org)) (require (quote org-lint)) (find-file (car command-line-args-left)) (let ((issues (org-lint))) (when issues (dolist (i issues) (message \"%S\" i)) (kill-emacs 1))))",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(lint.status.success(), "org-lint failed:\n{}", text(&lint));

    for injection in [
        "\u{200b}* Hidden heading",
        "\u{ff03}\u{ff0b}begin_src shell",
        "\u{ff1a}PROPERTIES\u{ff1a}",
    ] {
        let body = SKEL.replace("Notes for {{version}}.", injection);
        let path = board.write_skel("inject.org", &body);
        let err = board.fail(&[
            "workflow",
            "render",
            "inject",
            "--file",
            path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        assert!(err.contains("Org structure"), "{injection:?}:\n{err}");
    }
}

#[test]
fn concurrent_start_files_one_ticket_per_step() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Concurrent run", "--top", "-p", "demo"]));
    let args = [
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ];
    let first = board
        .cmd_as("saddle", &args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let second = board
        .cmd_as("saddle", &args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let out1 = first.wait_with_output().unwrap();
    let out2 = second.wait_with_output().unwrap();
    assert!(out1.status.success(), "first failed:\n{}", text(&out1));
    assert!(out2.status.success(), "second failed:\n{}", text(&out2));

    let layout = vissue_core::Layout::new(&board.root, "Software");
    let doc =
        vissue_core::IssueDoc::parse_file("demo", &layout.project_issues_path("demo")).unwrap();
    let children: Vec<_> = doc
        .headings
        .iter()
        .filter(|h| h.parent().map(str::trim) == Some(parent.as_str()))
        .collect();
    assert_eq!(
        children.len(),
        3,
        "expected 3 steps under {parent}, got {}:\nstdout1:\n{}\nstdout2:\n{}",
        children.len(),
        String::from_utf8_lossy(&out1.stdout),
        String::from_utf8_lossy(&out2.stdout)
    );
    let mut step_tags: Vec<String> = children
        .iter()
        .filter_map(|h| {
            h.tags()
                .iter()
                .find(|t| t.starts_with("step-"))
                .map(|t| t.to_string())
        })
        .collect();
    step_tags.sort();
    assert_eq!(
        step_tags,
        vec![
            "step-changelog".to_string(),
            "step-signoff".to_string(),
            "step-version".to_string()
        ]
    );
}

#[test]
fn start_lock_times_out_with_a_clear_error() {
    use std::os::fd::AsRawFd;

    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Locked run", "--top", "-p", "demo"]));
    let lock_dir = board.root.join(".ljos-locks");
    std::fs::create_dir_all(&lock_dir).unwrap();
    let lock_path = lock_dir.join(format!("workflow-start-{parent}.lock"));
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let held = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&lock_path)
        .unwrap();
    // SAFETY: `held` owns this descriptor through the command.
    assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0);
    let started = std::time::Instant::now();
    let out = board
        .cmd_as(
            "saddle",
            &[
                "workflow",
                "--start",
                "release",
                &parent,
                "--var",
                "version=1.2.0",
            ],
        )
        .env("LJOS_WORKFLOW_LOCK_WAIT", "1")
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", text(&out));
    let err = text(&out);
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    assert!(err.contains("timed out waiting for the run lock"), "{err}");
    assert!(err.contains(&parent), "{err}");
    assert!(err.contains(lock_path.to_str().unwrap()), "{err}");
}

#[test]
fn the_start_lock_follows_the_tracker_git_dir_not_an_exported_one() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let common = board.dir.path().join("tracker-git");
    let decoy = board.dir.path().join("decoy");
    for args in [
        vec![
            "init".to_string(),
            "-q".to_string(),
            format!("--separate-git-dir={}", common.display()),
            board.root.display().to_string(),
        ],
        vec![
            "init".to_string(),
            "-q".to_string(),
            decoy.display().to_string(),
        ],
    ] {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let out = Command::new("git")
            .args(&args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("GIT_DIR")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}\n{}", text(&out));
    }
    assert!(board.root.join(".git").is_file());
    let parent = first_id(&board.ok(&["file", "Worktree run", "--top", "-p", "demo"]));
    let out = board
        .cmd_as(
            "saddle",
            &[
                "workflow",
                "--start",
                "release",
                &parent,
                "--var",
                "version=1.2.0",
            ],
        )
        .env("GIT_DIR", decoy.join(".git"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let lock = format!("workflow-start-{parent}.lock");
    assert!(
        common.join("ljos-locks").join(&lock).is_file(),
        "no lock under {}\n{}",
        common.display(),
        text(&out)
    );
    assert!(
        !decoy.join(".git/ljos-locks").join(&lock).exists(),
        "the lock followed the exported GIT_DIR"
    );
}

#[test]
fn revise_reads_and_writes_the_index_under_the_skeleton_lock() {
    use std::os::fd::AsRawFd;

    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Locked revise", "--top", "-p", "demo"]));
    board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let workflows = board.home.join(".state/ljos/workflows");
    let index = workflows.join("release.json");
    let lock_path = workflows.join("release.lock");
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let held = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&lock_path)
        .unwrap();
    // SAFETY: `held` owns this descriptor until it is dropped below.
    assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0);
    let before = std::fs::read_to_string(&index).unwrap();
    let mut revise = board
        .cmd_as(
            "saddle",
            &["workflow", "revise", "release", "--from", &parent],
        )
        .env("LJOS_WORKFLOW_LOCK_WAIT", "60")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    if let Some(status) = revise.try_wait().unwrap() {
        let out = revise.wait_with_output().unwrap();
        panic!(
            "revise ran while the skeleton lock was held ({status}):\n{}",
            text(&out)
        );
    }
    assert_eq!(std::fs::read_to_string(&index).unwrap(), before);

    let mut doc: serde_json::Value = serde_json::from_str(&before).unwrap();
    doc["applied"] = serde_json::Value::String("Software-held".into());
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&index, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    drop(held);

    let out = revise.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index).unwrap()).unwrap();
    assert_eq!(
        after["applied"].as_str(),
        Some("Software-held"),
        "revise wrote back a copy it read before the lock:\n{after}"
    );
    assert!(after["proposal"].as_str().is_some(), "{after}");
}

#[test]
fn start_refuses_an_unreadable_lock_and_files_nothing() {
    use std::os::unix::fs::PermissionsExt;

    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Unreadable lock", "--top", "-p", "demo"]));
    let lock_dir = board.root.join(".ljos-locks");
    std::fs::create_dir_all(&lock_dir).unwrap();
    let lock_path = lock_dir.join(format!("workflow-start-{parent}.lock"));
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&lock_path, b"").unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "sets the mode of a path that is not a git hook"
    )]
    std::fs::set_permissions(&lock_path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let args = [
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ];
    let first = board
        .cmd_as("saddle", &args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let second = board
        .cmd_as("saddle", &args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let out1 = first.wait_with_output().unwrap();
    let out2 = second.wait_with_output().unwrap();
    let err1 = text(&out1);
    let err2 = text(&out2);
    assert!(!out1.status.success(), "{err1}");
    assert!(!out2.status.success(), "{err2}");
    assert!(err1.contains("cannot lock"), "{err1}");
    assert!(err2.contains("cannot lock"), "{err2}");
    assert!(err1.contains(&parent), "{err1}");
    assert!(err1.contains(lock_path.to_str().unwrap()), "{err1}");

    let layout = vissue_core::Layout::new(&board.root, "Software");
    let doc =
        vissue_core::IssueDoc::parse_file("demo", &layout.project_issues_path("demo")).unwrap();
    let children: Vec<_> = doc
        .headings
        .iter()
        .filter(|h| h.parent().map(str::trim) == Some(parent.as_str()))
        .collect();
    assert!(
        children.is_empty(),
        "unreadable lock filed {} steps:\n{err1}\n{err2}",
        children.len()
    );
}

#[test]
fn define_does_not_follow_a_symlink_at_the_deed_temp_path() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    let scratch = board.dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let victim = board.dir.path().join("victim.txt");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&victim, "PRECIOUS\n").unwrap();
    let pid = std::process::id();
    for n in 0..8192u32 {
        let link = scratch.join(format!(
            "ljos-workflow-release-1-{}.org",
            pid.wrapping_add(n)
        ));
        #[expect(
            clippy::disallowed_methods,
            reason = "links a path that is not a git hook"
        )]
        std::os::unix::fs::symlink(&victim, &link).unwrap();
    }
    let out = board
        .cmd_as("saddle", &["workflow", "define", path.to_str().unwrap()])
        .env("TMPDIR", &scratch)
        .output()
        .unwrap();
    let after = std::fs::read_to_string(&victim).unwrap();
    assert_eq!(
        after,
        "PRECIOUS\n",
        "deed temp write followed the symlink:\n{after}\n{}",
        text(&out)
    );
    assert!(
        out.status.success(),
        "define failed after leaving the target:\n{}",
        text(&out)
    );
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("stored release version 1"), "{said}");
}

#[test]
fn external_blocked_by_edges_appear_in_status_and_block_signoff() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    let stored = board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let accession = stored
        .lines()
        .find_map(|line| line.strip_prefix("deed "))
        .unwrap()
        .trim()
        .to_string();
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    let started = board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let changelog = filed(&started, "changelog");
    let version = filed(&started, "version");
    let signoff = filed(&started, "signoff");
    board.mark_done(&changelog);
    board.mark_done(&version);
    board.ok_as(
        "alice",
        &["vote", &signoff, "--for", "ship", "--used", "none"],
    );
    board.ok_as(
        "bob",
        &["vote", &signoff, "--for", "ship", "--used", "none"],
    );
    board.ok(&["deed", &signoff, "--add", &accession]);

    let ext = first_id(&board.ok(&["file", "Waiting on legal", "--top", "-p", "demo"]));
    board.ok(&["block", &version, "--on", &ext]);
    let status = board.ok(&["workflow", &parent]);
    assert!(!status.contains("blockers: none"), "{status}");
    assert!(status.contains(&ext), "{status}");
    assert!(status.contains("Waiting on legal"), "{status}");
    let refused = board.fail(&["complete", &signoff]);
    assert!(refused.contains(&ext), "{refused}");
    let refused_finish = board.fail(&["finish", &signoff]);
    assert!(refused_finish.contains(&ext), "{refused_finish}");

    board.mark_done(&ext);
    let status = board.ok(&["workflow", &parent]);
    assert!(status.contains("blockers: none"), "{status}");

    let ext2 = first_id(&board.ok(&["file", "External security hold", "--top", "-p", "demo"]));
    board.ok(&["block", &signoff, "--on", &ext2]);
    let sib = first_id(&board.ok(&["file", "Extra check under parent", "--parent", &parent]));
    board.ok(&["block", &changelog, "--on", &sib]);

    let status = board.ok(&["workflow", &parent]);
    assert!(!status.contains("blockers: none"), "{status}");
    assert!(status.contains(&ext2), "{status}");
    assert!(status.contains(&sib), "{status}");
    let refused = board.fail(&["complete", &signoff]);
    assert!(refused.contains(&ext2), "{refused}");
    assert!(refused.contains(&sib), "{refused}");
    let refused_finish = board.fail(&["finish", &signoff]);
    assert!(refused_finish.contains(&ext2), "{refused_finish}");

    board.mark_done(&ext2);
    board.mark_done(&sib);
    let status = board.ok(&["workflow", &parent]);
    assert!(status.contains("blockers: none"), "{status}");
    let finished = board.fail(&["finish", &signoff]);
    assert!(
        finished.contains("is not claimed and stays TODO"),
        "{finished}"
    );
    assert!(!finished.contains("is missing"), "{finished}");
}

#[test]
fn a_dangling_blocker_is_missing_and_matches_ready() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Dangling run", "--top", "-p", "demo"]));
    let started = board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let changelog = filed(&started, "changelog");
    let version = filed(&started, "version");
    let signoff = filed(&started, "signoff");
    board.mark_done(&changelog);

    let layout = vissue_core::Layout::new(&board.root, "Software");
    let issues = layout.project_issues_path("demo");
    let text = std::fs::read_to_string(&issues).unwrap();
    let mut changed = String::new();
    let mut target = false;
    let mut replaced = false;
    for line in text.lines() {
        if line.trim_start().starts_with(":ID:") {
            target = line.ends_with(&version);
        }
        if target && line.starts_with(":BLOCKED_BY:") {
            changed.push_str(":BLOCKED_BY: demo-nosuch\n");
            replaced = true;
            continue;
        }
        changed.push_str(line);
        changed.push('\n');
    }
    assert!(
        replaced,
        "could not replace the edge on {version} in\n{text}"
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&issues, &changed).unwrap();
    assert!(
        board
            .heading(&version)
            .blocked_by()
            .contains(&"demo-nosuch".to_string()),
        "dangling edge was not parsed:\n{changed}"
    );

    let ready = board.ok(&["ready", "--all"]);
    assert!(ready.contains(&version), "{ready}");
    let status = board.ok(&["workflow", &parent]);
    assert!(
        status.contains("demo-nosuch missing blocking version"),
        "{status}"
    );
    assert!(status.contains("ready: version"), "{status}");
    let refused = board.fail(&["complete", &signoff]);
    assert!(!refused.contains("demo-nosuch"), "{refused}");
}

#[test]
fn the_feature_workflow_still_files_its_design_child() {
    let board = Board::new();
    let parent = first_id(&board.ok(&["file", "Add a codec", "--top", "-p", "demo"]));
    let started = board.ok(&["workflow", &parent, "--start", "feature"]);
    assert!(
        started.contains("design child") || started.contains("filed the design child"),
        "{started}"
    );
    let again = board.ok(&["workflow", &parent, "--start", "feature"]);
    assert!(again.contains("already runs"), "{again}");
    assert!(!again.contains("filed the design child"), "{again}");
    let status = board.ok(&["workflow", &parent]);
    assert!(status.starts_with("feature on "), "{status}");
    assert!(status.contains("stage"), "{status}");
}

#[test]
fn feature_start_refuses_a_parent_that_runs_a_skeleton() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let refused = text(&board.ljos(&["workflow", "--start", "feature", &parent]));
    assert!(refused.contains("already runs"), "{refused}");
    assert!(refused.contains("skeleton"), "{refused}");
    assert!(refused.contains("nothing was written"), "{refused}");
    let status = board.ok(&["workflow", &parent]);
    assert!(status.contains("release version 1"), "{status}");
    assert!(!status.contains("workflow: feature"), "{status}");
    let shown = board.ok(&["show", &parent]);
    assert!(!shown.contains("workflow: feature"), "{shown}");
}

#[test]
fn skeleton_start_refuses_a_parent_that_runs_the_feature_workflow() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Add a codec", "--top", "-p", "demo"]));
    board.ok(&["workflow", &parent, "--start", "feature"]);
    let refused = board.fail(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    assert!(
        refused.contains(&format!(
            "workflow: {parent} already runs the feature workflow"
        )),
        "{refused}"
    );
    let status = board.ok(&["workflow", &parent]);
    assert!(status.starts_with("feature on "), "{status}");
    assert!(!status.contains("release version"), "{status}");
}

#[test]
fn a_kind_note_does_not_let_a_signoff_complete() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    let started = board.ok(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    let signoff = filed(&started, "signoff");
    board.ok(&["note", &signoff, "kind: do"]);
    let refused = board.fail(&["complete", &signoff]);
    assert!(refused.contains("signoff:"), "{refused}");
    assert!(refused.contains("is missing"), "{refused}");
}

#[test]
fn a_duplicate_dependency_is_refused_before_the_parent_is_bound() {
    let board = Board::new();
    let dup = "\
#+title: Build
#+version: 1
* Build it
:PROPERTIES:
:STEP: build
:DEPENDS:
:KIND: do
:END:
* Ship it
:PROPERTIES:
:STEP: ship
:DEPENDS: build build
:KIND: signoff
:END:
";
    let path = board.write_skel("build.org", dup);
    let err = board.fail(&["workflow", "define", path.to_str().unwrap()]);
    assert!(err.contains("twice"), "{err}");
    assert!(err.contains("build"), "{err}");

    let path = board.write_skel("release.org", SKEL);
    board.ok(&["workflow", "define", path.to_str().unwrap()]);
    let index = board.home.join(".state/ljos/workflows/release.json");
    let body = std::fs::read_to_string(&index).unwrap();
    let broken = body.replacen(":DEPENDS: version", ":DEPENDS: version version", 1);
    assert_ne!(body, broken);
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&index, broken).unwrap();
    let parent = first_id(&board.ok(&["file", "Release 1.2.0", "--top", "-p", "demo"]));
    let err = board.fail(&[
        "workflow",
        "--start",
        "release",
        &parent,
        "--var",
        "version=1.2.0",
    ]);
    assert!(err.contains("twice"), "{err}");
    assert!(!err.contains("cycle"), "{err}");
    let status = board.fail(&["workflow", &parent]);
    assert!(status.contains("carries no workflow"), "{status}");
    let shown = board.ok(&["show", &parent]);
    assert!(!shown.contains("skeleton:"), "{shown}");
}

#[test]
fn render_refuses_a_hidden_quad_and_a_var_that_is_not_a_pair() {
    let board = Board::new();
    let path = board.write_skel("release.org", SKEL);
    let out = board.dir.path().join("out.org");
    let err = board.fail(&[
        "workflow",
        "render",
        "release",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--var",
        "version=x10.0.0.1",
    ]);
    assert!(err.contains("address"), "{err}");
    assert!(!out.exists(), "a refused quad wrote {}", out.display());
    let err = board.fail(&[
        "workflow",
        "render",
        "release",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--var",
        "hunter2",
    ]);
    assert!(err.contains("key=value"), "{err}");
    assert!(!err.contains("hunter2"), "{err}");
    let err = board.fail(&[
        "workflow",
        "render",
        "release",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--var",
        "=hunter2",
    ]);
    assert!(err.contains("empty variable name"), "{err}");
    assert!(!err.contains("hunter2"), "{err}");
}
