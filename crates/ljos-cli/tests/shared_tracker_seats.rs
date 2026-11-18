//! Two separate `HOME` directories sharing one `VISSUE_ROOT` tracker do
//! not overwrite each other's claim on `ljos claim`, `ljos sitting`, or
//! `ljos_claim`.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;
#[path = "common/sibling.rs"]
mod sibling;

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

fn run_ljos(
    path: &str,
    home: &Path,
    runtime: &Path,
    tracker: &Path,
    seat: &str,
    args: &[&str],
) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", runtime)
        .env("VISSUE_ROOT", tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", seat)
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .current_dir(home)
        .stdin(Stdio::null());
    seat_env::clear(&mut cmd);
    cmd.env("LJOS_SEAT", seat);
    cmd.output().unwrap()
}

fn call_mcp_one(
    path: &str,
    home: &Path,
    runtime: &Path,
    tracker: &Path,
    seat: &str,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
    cmd.env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", runtime)
        .env("VISSUE_ROOT", tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_MCP_TOOLS", "all")
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    seat_env::clear(&mut cmd);
    cmd.env("LJOS_SEAT", seat);
    let mut child = cmd.spawn().unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "probe", "version": "0"}}
        })
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        })
    )
    .unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|v| v["id"] == 2)
        .unwrap_or_else(|| panic!("no id=2 in {stdout}"))
}

#[test]
fn separate_homes_sharing_one_tracker_do_not_steal_claims() {
    let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
    let claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
    let _deedar = sibling::require("deedar").unwrap_or_else(|e| panic!("{e}"));

    let dir = tempfile::tempdir().unwrap();
    let alice_home = dir.path().join("alice");
    let alice_run = alice_home.join("run");
    let bob_home = dir.path().join("bob");
    let bob_run = bob_home.join("run");
    let tracker = dir.path().join("tracker");
    for d in [
        &alice_home,
        &alice_run,
        &bob_home,
        &bob_run,
        &tracker.join("Software/demo"),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    let issues = tracker.join("Software/demo/issues.org");
    std::fs::write(
        &issues,
        "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#C] Shared claim ticket
:PROPERTIES:
:ID:         demo-shr1
:END:
",
    )
    .unwrap();

    let siblings = vissue.parent().unwrap();
    assert_eq!(claimdag.parent(), Some(siblings));
    let path = format!("{}:/usr/bin:/bin", siblings.display());

    let onboard = run_ljos(&path, &bob_home, &bob_run, &tracker, "bob", &["onboard"]);
    assert!(
        onboard.status.success(),
        "bob onboard failed: {}",
        String::from_utf8_lossy(&onboard.stderr)
    );

    let alice_claim = run_ljos(
        &path,
        &alice_home,
        &alice_run,
        &tracker,
        "alice",
        &["claim", "demo-shr1"],
    );
    assert!(
        alice_claim.status.success(),
        "alice claim failed: {}",
        String::from_utf8_lossy(&alice_claim.stderr)
    );

    // Bob in a separate HOME must not steal alice's claim on ljos claim.
    let bob_claim = run_ljos(
        &path,
        &bob_home,
        &bob_run,
        &tracker,
        "bob",
        &["claim", "demo-shr1"],
    );
    let claim_out = format!(
        "{}{}",
        String::from_utf8_lossy(&bob_claim.stdout),
        String::from_utf8_lossy(&bob_claim.stderr)
    );
    assert!(!bob_claim.status.success(), "{claim_out}");
    assert!(
        claim_out.contains("claim: demo-shr1 is held by another")
            && claim_out.contains("ljos send alice TEXT"),
        "bob stole or did not name alice on claim:\n{claim_out}"
    );

    // Bob must not steal alice's claim via ljos sitting either.
    let bob_sitting = run_ljos(
        &path,
        &bob_home,
        &bob_run,
        &tracker,
        "bob",
        &["sitting", "demo-shr1"],
    );
    let sit_out = format!(
        "{}{}",
        String::from_utf8_lossy(&bob_sitting.stdout),
        String::from_utf8_lossy(&bob_sitting.stderr)
    );
    assert!(
        sit_out.contains("ljos send alice TEXT"),
        "bob stole or did not name alice on sitting:\n{sit_out}"
    );

    // Bob must not steal alice's claim via ljos_claim over MCP either.
    let mcp_claim = call_mcp_one(
        &path,
        &bob_home,
        &bob_run,
        &tracker,
        "bob",
        "ljos_claim",
        serde_json::json!({"seat": "bob", "node": "demo-shr1"}),
    );
    let mcp_text = mcp_claim.to_string();
    assert!(
        mcp_text.contains("claim-conflict") && mcp_text.contains("ljos send alice TEXT"),
        "bob stole or did not name alice on ljos_claim:\n{mcp_text}"
    );

    // The shared tracker still shows alice as the holder.
    let org = std::fs::read_to_string(&issues).unwrap();
    assert!(org.contains(":CLAIMED_BY: alice"), "{org}");
    assert!(!org.contains(":CLAIMED_BY: bob"), "{org}");
}

#[test]
fn separate_homes_report_holder_and_unclaimed_state_on_release_and_finish() {
    let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
    let claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
    let _deedar = sibling::require("deedar").unwrap_or_else(|e| panic!("{e}"));

    let dir = tempfile::tempdir().unwrap();
    let alice_home = dir.path().join("alice");
    let alice_run = alice_home.join("run");
    let bob_home = dir.path().join("bob");
    let bob_run = bob_home.join("run");
    let tracker = dir.path().join("tracker");
    for d in [
        &alice_home,
        &alice_run,
        &bob_home,
        &bob_run,
        &tracker.join("Software/demo"),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    let issues = tracker.join("Software/demo/issues.org");
    std::fs::write(
        &issues,
        "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#C] Shared release ticket
:PROPERTIES:
:ID:         demo-rel1
:END:
",
    )
    .unwrap();

    let siblings = vissue.parent().unwrap();
    assert_eq!(claimdag.parent(), Some(siblings));
    let path = format!("{}:/usr/bin:/bin", siblings.display());

    let alice_claim = run_ljos(
        &path,
        &alice_home,
        &alice_run,
        &tracker,
        "alice",
        &["claim", "demo-rel1"],
    );
    assert!(
        alice_claim.status.success(),
        "{}",
        String::from_utf8_lossy(&alice_claim.stderr)
    );

    // Bob releasing or finishing alice's held ticket must name alice and `ljos send`.
    let bob_release = run_ljos(
        &path,
        &bob_home,
        &bob_run,
        &tracker,
        "bob",
        &["release", "demo-rel1"],
    );
    assert!(!bob_release.status.success(), "{bob_release:?}");
    let rel_text = format!(
        "{}{}",
        String::from_utf8_lossy(&bob_release.stdout),
        String::from_utf8_lossy(&bob_release.stderr)
    );
    assert!(
        rel_text.contains("ljos send alice TEXT") && !rel_text.contains("claimdag failed"),
        "bob release:\n{rel_text}"
    );

    let mcp_rel = call_mcp_one(
        &path,
        &bob_home,
        &bob_run,
        &tracker,
        "bob",
        "ljos_release",
        serde_json::json!({"seat": "bob", "node": "demo-rel1"}),
    );
    let mcp_text = mcp_rel.to_string();
    assert_eq!(mcp_rel["result"]["isError"], true, "{mcp_rel}");
    assert!(
        mcp_text.contains("ljos send alice TEXT") && !mcp_text.contains("claimdag failed"),
        "bob ljos_release:\n{mcp_text}"
    );

    let bob_finish = run_ljos(
        &path,
        &bob_home,
        &bob_run,
        &tracker,
        "bob",
        &["finish", "demo-rel1", "--lesson", "bob note"],
    );
    assert!(!bob_finish.status.success(), "{bob_finish:?}");
    let fin_text = format!(
        "{}{}",
        String::from_utf8_lossy(&bob_finish.stdout),
        String::from_utf8_lossy(&bob_finish.stderr)
    );
    assert!(
        fin_text.contains("ljos send alice TEXT") && !fin_text.contains("claimdag failed"),
        "bob finish:\n{fin_text}"
    );

    // Alice releases the ticket cleanly.
    let alice_release = run_ljos(
        &path,
        &alice_home,
        &alice_run,
        &tracker,
        "alice",
        &["release", "demo-rel1"],
    );
    assert!(
        alice_release.status.success(),
        "{}",
        String::from_utf8_lossy(&alice_release.stderr)
    );

    // Releasing an already unclaimed ticket prints a clear message naming `ljos claim`.
    let again = run_ljos(
        &path,
        &alice_home,
        &alice_run,
        &tracker,
        "alice",
        &["release", "demo-rel1"],
    );
    assert!(!again.status.success(), "{again:?}");
    let again_text = format!(
        "{}{}",
        String::from_utf8_lossy(&again.stdout),
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(
        again_text.contains("is not claimed")
            && again_text.contains("ljos claim demo-rel1")
            && !again_text.contains("claimdag failed"),
        "unclaimed release:\n{again_text}"
    );
}

#[test]
fn separate_homes_print_assist_line_when_noting_another_seats_ticket() {
    let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
    let claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
    let _deedar = sibling::require("deedar").unwrap_or_else(|e| panic!("{e}"));

    let dir = tempfile::tempdir().unwrap();
    let alice_home = dir.path().join("alice");
    let alice_run = alice_home.join("run");
    let bob_home = dir.path().join("bob");
    let bob_run = bob_home.join("run");
    let tracker = dir.path().join("tracker");
    for d in [
        &alice_home,
        &alice_run,
        &bob_home,
        &bob_run,
        &tracker.join("Software/demo"),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(
        tracker.join("Software/demo/issues.org"),
        "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#C] Assist note ticket
:PROPERTIES:
:ID:         demo-not1
:END:
",
    )
    .unwrap();

    let siblings = vissue.parent().unwrap();
    assert_eq!(claimdag.parent(), Some(siblings));
    let path = format!("{}:/usr/bin:/bin", siblings.display());

    let alice_claim = run_ljos(
        &path,
        &alice_home,
        &alice_run,
        &tracker,
        "alice",
        &["claim", "demo-not1"],
    );
    assert!(
        alice_claim.status.success(),
        "{}",
        String::from_utf8_lossy(&alice_claim.stderr)
    );

    let bob_note = run_ljos(
        &path,
        &bob_home,
        &bob_run,
        &tracker,
        "bob",
        &["note", "demo-not1", "bob assist note"],
    );
    assert!(
        bob_note.status.success(),
        "{}",
        String::from_utf8_lossy(&bob_note.stderr)
    );
    let note_out = format!(
        "{}{}",
        String::from_utf8_lossy(&bob_note.stdout),
        String::from_utf8_lossy(&bob_note.stderr)
    );
    assert!(
        note_out.contains("alice") && note_out.contains("ljos send alice TEXT"),
        "bob note missed assist line:\n{note_out}"
    );
}

/// Two homes on one tracker with one open ticket, and both onboarded.
struct Shared {
    _dir: tempfile::TempDir,
    path: String,
    tracker: std::path::PathBuf,
    alice: (std::path::PathBuf, std::path::PathBuf),
    bob: (std::path::PathBuf, std::path::PathBuf),
}

impl Shared {
    fn new() -> Self {
        let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
        let claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
        let _deedar = sibling::require("deedar").unwrap_or_else(|e| panic!("{e}"));
        let dir = tempfile::tempdir().unwrap();
        let tracker = dir.path().join("tracker");
        let mut homes = Vec::new();
        for name in ["alice", "bob"] {
            let home = dir.path().join(name);
            let run = home.join("run");
            std::fs::create_dir_all(&run).unwrap();
            homes.push((home, run));
        }
        std::fs::create_dir_all(tracker.join("Software/demo")).unwrap();
        std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        std::fs::write(
            tracker.join("Software/demo/issues.org"),
            "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#C] Shared claim ticket
:PROPERTIES:
:ID:         demo-shr1
:END:
",
        )
        .unwrap();
        let siblings = vissue.parent().unwrap();
        assert_eq!(claimdag.parent(), Some(siblings));
        let path = format!("{}:/usr/bin:/bin", siblings.display());
        let bob = homes.pop().unwrap();
        let alice = homes.pop().unwrap();
        for (seat, (home, run)) in [("alice", &alice), ("bob", &bob)] {
            let onboard = run_ljos(&path, home, run, &tracker, seat, &["onboard"]);
            assert!(
                onboard.status.success(),
                "{seat} onboard failed: {}",
                String::from_utf8_lossy(&onboard.stderr)
            );
        }
        Self {
            _dir: dir,
            path,
            tracker,
            alice,
            bob,
        }
    }

    fn bob_ljos(&self, args: &[&str]) -> Output {
        run_ljos(
            &self.path,
            &self.bob.0,
            &self.bob.1,
            &self.tracker,
            "bob",
            args,
        )
    }

    fn alice_ljos(&self, args: &[&str]) -> Output {
        run_ljos(
            &self.path,
            &self.alice.0,
            &self.alice.1,
            &self.tracker,
            "alice",
            args,
        )
    }

    fn bob_mcp(&self, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        call_mcp_one(
            &self.path,
            &self.bob.0,
            &self.bob.1,
            &self.tracker,
            "bob",
            name,
            arguments,
        )
    }

    /// Every node in bob's claim graph, as `claimdag list --all` prints it.
    fn bob_graph(&self) -> String {
        let out = Command::new("claimdag")
            .arg("--dir")
            .arg(self.bob.1.join("claimdag"))
            .args(["list", "--all"])
            .env("PATH", &self.path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("no work graph") {
            return String::new();
        }
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).to_string()
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_claim_graph_id_is_claimed_without_a_ticket_of_that_name() {
    let world = Shared::new();
    let minted = Command::new("claimdag")
        .arg("--dir")
        .arg(world.bob.1.join("claimdag"))
        .args(["upsert", "--summary", "scratch work"])
        .env("PATH", &world.path)
        .output()
        .unwrap();
    assert!(minted.status.success(), "{minted:?}");
    let id = String::from_utf8_lossy(&minted.stdout)
        .split_whitespace()
        .find(|word| word.len() == 32 && word.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| panic!("no node id in {minted:?}"))
        .to_string();

    let claimed = world.bob_ljos(&["claim", &id]);
    assert!(claimed.status.success(), "{}", said(&claimed));
    assert!(!said(&claimed).contains("not found"), "{}", said(&claimed));
}
#[test]
fn ljos_mcp_refuses_a_ticket_that_does_not_exist_as_the_cli_does() {
    let world = Shared::new();
    let cli = world.bob_ljos(&["claim", "demo-nope"]);
    assert!(!cli.status.success(), "{}", said(&cli));
    assert!(said(&cli).contains("not found"), "{}", said(&cli));

    for (tool, arguments) in [
        (
            "ljos_claim",
            serde_json::json!({"seat": "bob", "node": "demo-nope"}),
        ),
        (
            "ljos_release",
            serde_json::json!({"seat": "bob", "node": "demo-nope"}),
        ),
        (
            "ljos_complete",
            serde_json::json!({"seat": "bob", "node": "demo-nope"}),
        ),
    ] {
        let answer = world.bob_mcp(tool, arguments);
        let text = answer.to_string();
        assert_eq!(answer["result"]["isError"], true, "{tool}: {text}");
        assert!(
            text.contains("demo-nope") && text.contains("not found"),
            "{tool}: {text}"
        );
        assert!(
            !text.contains("ljos claim demo-nope"),
            "{tool} sends the caller to claim a ticket that does not exist: {text}"
        );
    }
    assert!(
        !world.bob_graph().contains("demo-nope"),
        "a missing ticket got a node:\n{}",
        world.bob_graph()
    );
}
#[test]
fn a_claim_another_seat_refuses_leaves_no_node_in_the_refused_seats_graph() {
    let world = Shared::new();
    let taken = world.alice_ljos(&["claim", "demo-shr1"]);
    assert!(taken.status.success(), "{}", said(&taken));

    let refused = world.bob_ljos(&["claim", "demo-shr1"]);
    assert!(!refused.status.success(), "{}", said(&refused));
    assert!(
        said(&refused).contains("ljos send alice TEXT"),
        "{}",
        said(&refused)
    );
    assert!(
        !world.bob_graph().contains("demo-shr1"),
        "a refused claim left a node behind:\n{}",
        world.bob_graph()
    );

    let over_mcp = world.bob_mcp(
        "ljos_claim",
        serde_json::json!({"seat": "bob", "node": "demo-shr1"}),
    );
    assert!(
        over_mcp.to_string().contains("claim-conflict"),
        "{over_mcp}"
    );
    assert!(
        !world.bob_graph().contains("demo-shr1"),
        "a refused ljos_claim left a node behind:\n{}",
        world.bob_graph()
    );
}

#[test]
fn a_refused_sitting_writes_no_playbook_note_or_local_record() {
    let world = Shared::new();
    let taken = world.alice_ljos(&["claim", "demo-shr1"]);
    assert!(taken.status.success(), "{}", said(&taken));

    let issues = world.tracker.join("Software/demo/issues.org");
    let before_org = std::fs::read_to_string(&issues).unwrap();

    let refused = world.bob_ljos(&["sitting", "demo-shr1"]);
    assert!(!refused.status.success(), "{}", said(&refused));
    assert!(
        said(&refused).contains("alice"),
        "refused sitting did not name alice:\n{}",
        said(&refused)
    );

    let after_org = std::fs::read_to_string(&issues).unwrap();
    assert_eq!(
        after_org, before_org,
        "a refused sitting wrote to the tracker:\n{after_org}"
    );
    assert!(
        !after_org.contains("playbook:"),
        "a refused sitting wrote a playbook note:\n{after_org}"
    );
    assert!(
        !world.bob.1.join("playbook-demo-shr1").exists(),
        "a refused sitting wrote a local playbook record"
    );
    assert!(
        !world.bob_graph().contains("demo-shr1"),
        "a refused sitting minted a node:\n{}",
        world.bob_graph()
    );

    let over_mcp = world.bob_mcp(
        "ljos_sitting",
        serde_json::json!({"seat": "bob", "issue": "demo-shr1"}),
    );
    assert!(
        over_mcp.to_string().contains("claim-conflict") && over_mcp.to_string().contains("alice"),
        "{over_mcp}"
    );
    assert_eq!(
        std::fs::read_to_string(&issues).unwrap(),
        before_org,
        "a refused ljos_sitting wrote to the tracker"
    );
    assert!(
        !world.bob.1.join("playbook-demo-shr1").exists(),
        "a refused ljos_sitting wrote a local playbook record"
    );
}

#[test]
fn complete_on_another_seats_ticket_refuses_without_changing_state() {
    let world = Shared::new();
    let taken = world.alice_ljos(&["claim", "demo-shr1"]);
    assert!(taken.status.success(), "{}", said(&taken));

    let issues = world.tracker.join("Software/demo/issues.org");
    let before_org = std::fs::read_to_string(&issues).unwrap();

    let refused = world.bob_ljos(&["complete", "demo-shr1"]);
    let out_text = said(&refused);
    assert!(
        !refused.status.success(),
        "complete on another seat's ticket exited 0:\n{out_text}"
    );
    assert!(
        refused.stdout.is_empty(),
        "expected empty stdout on refusal:\n{out_text}"
    );
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(
        stderr.lines().count(),
        1,
        "expected one plain line on stderr:\n{stderr}"
    );
    assert!(stderr.contains("alice"), "did not name holder:\n{stderr}");
    assert!(
        !stderr.contains("stays claimed by"),
        "printed stays claimed by:\n{stderr}"
    );
    assert!(
        !out_text.contains("label "),
        "expected plain refusal line:\n{out_text}"
    );
    assert_eq!(
        std::fs::read_to_string(&issues).unwrap(),
        before_org,
        "complete on another seat's ticket changed tracker state"
    );
    assert!(
        !world.bob_graph().contains("demo-shr1"),
        "complete on another seat's ticket minted or marked a node:\n{}",
        world.bob_graph()
    );

    // Even if bob's own claim graph already has a node for demo-shr1,
    // completing it while alice owns the ticket must refuse and keep the node unchanged.
    let id = ljos_cli::work_id("demo-shr1");
    let upsert = Command::new("claimdag")
        .arg("--dir")
        .arg(world.bob.1.join("claimdag"))
        .args(["upsert", "--id", &id, "--summary", "demo-shr1"])
        .env("PATH", &world.path)
        .output()
        .unwrap();
    assert!(upsert.status.success(), "{upsert:?}");
    let before_graph = world.bob_graph();

    let refused_existing = world.bob_ljos(&["complete", "demo-shr1"]);
    assert!(
        !refused_existing.status.success(),
        "complete marked bob's existing node done:\n{}",
        said(&refused_existing)
    );
    assert_eq!(world.bob_graph(), before_graph);
    assert_eq!(std::fs::read_to_string(&issues).unwrap(), before_org);

    let over_mcp = world.bob_mcp(
        "ljos_complete",
        serde_json::json!({"seat": "bob", "node": "demo-shr1"}),
    );
    assert_eq!(over_mcp["result"]["isError"], true, "{over_mcp}");
    assert!(
        over_mcp.to_string().contains("alice"),
        "ljos_complete did not name alice: {over_mcp}"
    );
    assert_eq!(world.bob_graph(), before_graph);
    assert_eq!(std::fs::read_to_string(&issues).unwrap(), before_org);
}

#[test]
fn release_of_another_seats_ticket_mints_no_node() {
    let world = Shared::new();
    let taken = world.alice_ljos(&["claim", "demo-shr1"]);
    assert!(taken.status.success(), "{}", said(&taken));
    let issues = world.tracker.join("Software/demo/issues.org");
    let before_org = std::fs::read_to_string(&issues).unwrap();

    let refused = world.bob_ljos(&["release", "demo-shr1"]);
    let text = said(&refused);
    assert!(!refused.status.success(), "{text}");
    assert!(
        text.contains("ljos send alice TEXT"),
        "release did not name alice:\n{text}"
    );
    assert_eq!(std::fs::read_to_string(&issues).unwrap(), before_org);
    assert!(
        !world.bob_graph().contains("demo-shr1"),
        "a refused release minted a node:\n{}",
        world.bob_graph()
    );

    let finished = world.bob_ljos(&["finish", "demo-shr1"]);
    let fin = said(&finished);
    assert!(!finished.status.success(), "{fin}");
    assert!(
        fin.contains("ljos send alice TEXT"),
        "finish did not name alice:\n{fin}"
    );
    assert_eq!(std::fs::read_to_string(&issues).unwrap(), before_org);
    assert!(
        !world.bob_graph().contains("demo-shr1"),
        "a refused finish minted a node:\n{}",
        world.bob_graph()
    );

    let over_mcp = world.bob_mcp(
        "ljos_release",
        serde_json::json!({"seat": "bob", "node": "demo-shr1"}),
    );
    assert_eq!(over_mcp["result"]["isError"], true, "{over_mcp}");
    assert!(
        over_mcp.to_string().contains("ljos send alice TEXT"),
        "{over_mcp}"
    );
    assert_eq!(std::fs::read_to_string(&issues).unwrap(), before_org);
    assert!(
        !world.bob_graph().contains("demo-shr1"),
        "a refused ljos_release minted a node:\n{}",
        world.bob_graph()
    );
}
