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
    assert!(first.contains("#+begin_src dot"), "{first}");
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
Move the cursor position.
composer of music.
llama farm.
Codex Sinaiticus.
Opus 64.
magnum opus.
The Gemini program flew in 1965.
Ask Ai Weiwei.
Use the Mistral wind.
Cursor-based pagination.
The term sonnet.
hello3-mini
xclaude sonnet 4
firmware 1.2.3.4.
ships 1.2.3.4
release 1.2.3.4
2.0.1.0.
section 3.4.5.6.
seat belt.
add::fee.
key=value.
'the token is in the vault' (no value).
Cargo.toml, v1.2.3, e.g., i.e., std::fs::read, =std::fs::read=, Vec::new, 12:30, 1:02:03, TCP/IP.
[[https://example.com][link]]
";
    for line in corpus.lines() {
        let hits = ljos_cli::scrub::scan_author(line, 1);
        assert!(hits.is_empty(), "{line:?}: {hits:?}");
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
        ("db.internal", "host"),
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
        ("cloud agent", "tool or model name"),
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
        ("hello\u{200b}world", "invisible character"),
        ("hello\u{202e}world", "invisible character"),
        ("\u{feff}hello", "invisible character"),
        ("hello\u{2066}world", "invisible character"),
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
    for (prose, class) in cases {
        let line_no = text
            .lines()
            .position(|line| line == prose)
            .unwrap_or_else(|| panic!("missing line {prose:?}"))
            + 1;
        let hit = format!("line {line_no} contains {class}");
        assert!(err.contains(&hit), "missing {hit} for {prose:?}:\n{err}");
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
