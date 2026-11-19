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

    fn ljos_as(&self, seat: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
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
            .env("LC_ALL", "C.UTF-8")
            .output()
            .unwrap()
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
fn render_drops_versioned_models_and_seat_names() {
    let board = Board::new();
    let body = SKEL.replace(
        "Notes for {{version}}.",
        "Notes for {{version}}.\nReviewed by GPT-4 and seat saddle.\nPacked by saddle.\n",
    );
    let path = board.write_skel("leak.org", &body);
    let out = board.dir.path().join("page.org");
    let out_s = out.to_str().unwrap();
    board.ok(&[
        "workflow",
        "render",
        "leak",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out_s,
    ]);
    let page = std::fs::read_to_string(&out).unwrap();
    assert!(page.contains("Notes for {{version}}."), "{page}");
    assert!(!page.contains("GPT-4"), "{page}");
    assert!(!page.to_ascii_lowercase().contains("saddle"), "{page}");
    assert!(!page.to_ascii_lowercase().contains("seat saddle"), "{page}");

    std::fs::write(&out, format!("{page}Reviewed by GPT-4 and seat saddle.\n")).unwrap();
    let err = board.fail(&[
        "workflow",
        "render",
        "leak",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out_s,
        "--check",
    ]);
    assert!(
        err.contains("model") || err.contains("seat") || err.contains("differs"),
        "{err}"
    );
    assert!(!err.contains("matches"), "{err}");
}

#[test]
fn render_scrubs_all_adversarial_classes_and_passes_check_and_org_lint() {
    let board = Board::new();
    let adv = concat!("\
#+title: \u{ff27}\u{ff30}\u{ff34}4 Release
#+version: 1

* Check Cl\u{200b}aude and [[[file:/etc/passwd][x]]]
:PROPERTIES:
:STEP: check
:DEPENDS:
:KIND: do
:ROLE: seat saddle
:END:

Update Cargo.toml to v1.2.3, e.g. the manifest, i.e. the version.
Cl\u{200b}aude wrote this note.
\u{ff27}\u{ff30}\u{ff34} drafted this line.
GPT4 checked it.
Claude3 verified it.
ChatGPT, Anthropic, OpenAI, Gemini, Grok, Copilot, Llama, Mistral, Codex, Composer, Cursor reviewed it.
Run a subagent, chatbot, LLM, and AI agent on it.
Read file:///home/x and /home/x/repo today.
Export GITHUB_TOKEN=zzz and KEY=abc with password: x before running.
Slack x", "ox", "b-12345-abcde and gh", "p_1234567890abcdef and github", "_pat_11ABC_xyz and sk", "-proj-123456 and AKI", "AIOSFODNN7EXAMPLE.
Connect 10.0.0.1 and 2001:db8::1 and [::1]:8761 on box.tail1234.ts.net or db.internal or nas.local.
Mail alice@example.com or seat:carol or saddle about it.
Click [[[file:/etc/passwd][x]]] and [[file:/home/x][x]] and [[https://example.com][link]] and [fn:1].

* Sign off
:PROPERTIES:
:STEP: signoff
:DEPENDS: check
:KIND: signoff
:END:

Options: ship, hold
");
    let path = board.write_skel("adv.org", adv);
    let out = board.dir.path().join("adv-public.org");
    let out_s = out.to_str().unwrap();
    board.ok(&[
        "workflow",
        "render",
        "adv",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out_s,
    ]);
    let page = std::fs::read_to_string(&out).unwrap();
    assert!(page.contains("Cargo.toml"), "{page}");
    assert!(page.contains("v1.2.3"), "{page}");
    assert!(page.contains("e.g."), "{page}");
    assert!(page.contains("i.e."), "{page}");
    assert!(page.contains("* check"), "{page}");
    assert!(page.contains("#+title: Workflow"), "{page}");
    for leaked in [
        "Claude",
        "claude",
        "GPT",
        "gpt",
        "ChatGPT",
        "Anthropic",
        "OpenAI",
        "Gemini",
        "Grok",
        "Copilot",
        "Llama",
        "Mistral",
        "Codex",
        "Composer",
        "Cursor",
        "subagent",
        "chatbot",
        "LLM",
        "AI agent",
        "file://",
        "file:/",
        "/home/x",
        "/etc/passwd",
        "GITHUB_TOKEN",
        "zzz",
        "password:",
        concat!("x", "oxb-"),
        concat!("gh", "p_"),
        concat!("githu", "b_pat_"),
        concat!("s", "k-proj"),
        concat!("AK", "IA"),
        "10.0.0.1",
        "2001:db8::1",
        "::1",
        "ts.net",
        "db.internal",
        "nas.local",
        "alice@example.com",
        "saddle",
        "carol",
        "[[",
        "]]",
        "[fn:1]",
        "\u{200b}",
    ] {
        assert!(!page.contains(leaked), "leaked {leaked:?} in:\n{page}");
    }

    let checked = board.ok(&[
        "workflow",
        "render",
        "adv",
        "--file",
        path.to_str().unwrap(),
        "--out",
        out_s,
        "--check",
    ]);
    assert!(checked.contains("matches"), "{checked}");

    if std::path::Path::new("/usr/bin/emacs").is_file() {
        let lint = Command::new("/usr/bin/emacs")
            .args([
                "--batch",
                "-Q",
                "--eval",
                "(progn (require (quote org)) (require (quote org-lint)) (find-file (car command-line-args-left)) (let ((issues (org-lint))) (when issues (dolist (i issues) (message \"%S\" i)) (kill-emacs 1))))",
                out_s,
            ])
            .output()
            .unwrap();
        assert!(lint.status.success(), "org-lint failed:\n{}", text(&lint));
    }

    for dirty_line in [
        "Cl\u{200b}aude\n",
        "\u{ff27}\u{ff30}\u{ff34}\n",
        "GPT4\n",
        "Claude3\n",
        "subagent\n",
        "file:///home/x\n",
        "GITHUB_TOKEN=zzz\n",
        "password: x\n",
        concat!("x", "ox", "b-12345-abcde\n"),
        concat!("github", "_pat_11ABC_xyz\n"),
        "10.0.0.1\n",
        "2001:db8::1\n",
        "box.ts.net\n",
        "alice@example.com\n",
        "[[[file:/etc/passwd][x]]]\n",
    ] {
        std::fs::write(&out, format!("{page}{dirty_line}")).unwrap();
        let err = board.fail(&[
            "workflow",
            "render",
            "adv",
            "--file",
            path.to_str().unwrap(),
            "--out",
            out_s,
            "--check",
        ]);
        assert!(!err.contains("matches"), "accepted {dirty_line:?}: {err}");
    }
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
