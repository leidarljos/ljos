//! `ljos claim ID --takeover` takes a ticket from a holder in another home
//! when that holder looks stale, refuses a holder that is running or active,
//! notes the old holder on the ticket, and does the same through `ljos mcp`.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;
#[path = "common/sibling.rs"]
mod sibling;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

const STALE: &str = "[2020-01-01 Wed 00:00]";

struct World {
    _dir: tempfile::TempDir,
    path: String,
    tracker: PathBuf,
    alice_home: PathBuf,
    alice_run: PathBuf,
    bob_home: PathBuf,
    bob_run: PathBuf,
}

impl World {
    fn new(tickets: &[(&str, Option<(&str, &str)>)]) -> World {
        let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
        let claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
        let _deedar = sibling::require("deedar").unwrap_or_else(|e| panic!("{e}"));
        let siblings = vissue.parent().unwrap();
        assert_eq!(claimdag.parent(), Some(siblings));

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
        let mut org = String::from(
            "#+TITLE: demo\n#+VISSUE: 1\n#+CATEGORY: demo\n#+TODO: TODO STARTED | DONE CANCELLED\n\n",
        );
        for (id, held) in tickets {
            match held {
                Some((who, at)) => org.push_str(&format!(
                    "* STARTED [#C] Ticket {id}\n:PROPERTIES:\n:ID:         {id}\n:CLAIMED_BY: {who}\n:CLAIMED_AT: {at}\n:END:\n:LOGBOOK:\n- State \"STARTED\" from \"TODO\" {at}\n:END:\n\n"
                )),
                None => org.push_str(&format!(
                    "* TODO [#C] Ticket {id}\n:PROPERTIES:\n:ID:         {id}\n:END:\n\n"
                )),
            }
        }
        std::fs::write(tracker.join("Software/demo/issues.org"), org).unwrap();
        let path = format!("{}:/usr/bin:/bin", siblings.display());
        World {
            _dir: dir,
            path,
            tracker,
            alice_home,
            alice_run,
            bob_home,
            bob_run,
        }
    }

    fn org(&self) -> String {
        std::fs::read_to_string(self.tracker.join("Software/demo/issues.org")).unwrap()
    }

    fn cmd(&self, home: &Path, runtime: &Path, seat: &str, extra: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.env_clear()
            .env("PATH", &self.path)
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", runtime)
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .current_dir(home)
            .stdin(Stdio::null());
        seat_env::clear(&mut cmd);
        cmd.env("LJOS_SEAT", seat);
        for (key, value) in extra {
            cmd.env(key, value);
        }
        cmd
    }

    fn alice(&self, args: &[&str]) -> Output {
        self.cmd(&self.alice_home, &self.alice_run, "alice", &[])
            .args(args)
            .output()
            .unwrap()
    }

    fn bob(&self, args: &[&str]) -> Output {
        self.bob_with(args, &[])
    }

    fn bob_with(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        self.cmd(&self.bob_home, &self.bob_run, "bob", extra)
            .args(args)
            .output()
            .unwrap()
    }

    fn bob_mcp(&self, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
        cmd.env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.bob_home)
            .env("XDG_CONFIG_HOME", self.bob_home.join(".config"))
            .env("XDG_DATA_HOME", self.bob_home.join(".local/share"))
            .env("XDG_STATE_HOME", self.bob_home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &self.bob_run)
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("PACKSET_URL", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_MCP_TOOLS", "all")
            .current_dir(&self.bob_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        seat_env::clear(&mut cmd);
        cmd.env("LJOS_SEAT", "bob");
        let mut child = cmd.spawn().unwrap();
        let pid = child.id();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(30));
            let _ = Command::new("kill").arg(pid.to_string()).status();
        });
        let mut stdin = child.stdin.take().unwrap();
        for line in [
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "probe", "version": "0"}}
            }),
            serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": name, "arguments": arguments}
            }),
        ] {
            writeln!(stdin, "{line}").unwrap();
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|v| v["id"] == 2)
            .unwrap_or_else(|| panic!("no id=2 in {stdout}"))
    }

    /// A hold record in bob's runtime directory, the way a conversation on
    /// this host leaves one.
    fn hold_record(&self, name: &str, pid: u32, comm: &str, node: &str) {
        let dir = self.bob_run.join("ljos");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("hold-{name}")),
            format!("alice\nalice\n{pid}\n{comm}\n2026-01-01T00:00:00.000Z\n{node}\n-\n\n\n"),
        )
        .unwrap();
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn comm_of(pid: u32) -> String {
    let out = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    text.rsplit('/').next().unwrap_or("").to_string()
}

fn sleeper() -> Child {
    Command::new("sleep")
        .arg("120")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn a_stale_holder_is_named_as_stale_and_taken_over_with_a_note() {
    let world = World::new(&[("demo-tk01", Some(("alice", STALE)))]);
    let before = world.org();

    let refused = world.bob(&["claim", "demo-tk01"]);
    let text = said(&refused);
    assert!(!refused.status.success(), "{text}");
    assert!(
        text.contains("claim: demo-tk01 is held by another (alice, seat alice)"),
        "{text}"
    );
    assert!(text.contains("The holder looks stale"), "{text}");
    assert!(text.contains("`ljos claim demo-tk01 --takeover`"), "{text}");
    assert_eq!(world.org(), before, "a refused claim wrote the tracker");

    let taken = world.bob(&["claim", "demo-tk01", "--takeover"]);
    let text = said(&taken);
    assert!(taken.status.success(), "{text}");
    assert!(text.contains("took over demo-tk01 from alice"), "{text}");
    let org = world.org();
    assert!(org.contains(":CLAIMED_BY: bob"), "{org}");
    assert!(!org.contains(":CLAIMED_BY: alice"), "{org}");
    assert!(
        org.contains("took over from alice (seat alice)"),
        "the ticket carries no note naming alice:\n{org}"
    );
}

#[test]
fn a_dead_recorded_holder_on_this_host_is_taken_over_too() {
    let world = World::new(&[("demo-tk02", Some(("alice", STALE)))]);
    let mut gone = Command::new("true").spawn().unwrap();
    let pid = gone.id();
    let comm = "true";
    gone.wait().unwrap();
    world.hold_record("deadalice", pid, comm, "demo-tk02");

    let taken = world.bob(&["claim", "demo-tk02", "--takeover"]);
    let text = said(&taken);
    assert!(taken.status.success(), "{text}");
    let org = world.org();
    assert!(org.contains(":CLAIMED_BY: bob"), "{org}");
    assert!(org.contains("took over from alice"), "{org}");
}

#[test]
fn a_takeover_through_ljos_mcp_notes_the_old_holder() {
    let world = World::new(&[("demo-tk03", Some(("alice", STALE)))]);

    let refused = world.bob_mcp(
        "ljos_claim",
        serde_json::json!({"seat": "bob", "node": "demo-tk03"}),
    );
    let text = refused.to_string();
    assert!(text.contains("label claim-conflict"), "{text}");
    assert!(text.contains("The holder looks stale"), "{text}");
    assert!(text.contains("ljos claim demo-tk03 --takeover"), "{text}");
    assert!(world.org().contains(":CLAIMED_BY: alice"));

    let taken = world.bob_mcp(
        "ljos_claim",
        serde_json::json!({"seat": "bob", "node": "demo-tk03", "takeover": true}),
    );
    let text = taken.to_string();
    assert_ne!(taken["result"]["isError"], true, "{text}");
    assert!(text.contains("took over demo-tk03 from alice"), "{text}");
    let org = world.org();
    assert!(org.contains(":CLAIMED_BY: bob"), "{org}");
    assert!(org.contains("took over from alice (seat alice)"), "{org}");
}

#[test]
fn a_holder_active_inside_the_lease_is_refused_and_nothing_changes() {
    let world = World::new(&[("demo-tk04", None)]);
    let claimed = world.alice(&["claim", "demo-tk04"]);
    assert!(claimed.status.success(), "{}", said(&claimed));
    let before = world.org();
    assert!(before.contains(":CLAIMED_BY: alice"), "{before}");

    let refused = world.bob(&["claim", "demo-tk04", "--takeover"]);
    let text = said(&refused);
    assert!(!refused.status.success(), "{text}");
    assert!(
        text.contains("inside the 900s lease") && text.contains("changes nothing"),
        "{text}"
    );
    assert!(text.contains("`ljos send alice TEXT`"), "{text}");
    assert_eq!(world.org(), before, "a refused takeover wrote the tracker");

    let mcp = world.bob_mcp(
        "ljos_claim",
        serde_json::json!({"seat": "bob", "node": "demo-tk04", "takeover": true}),
    );
    let text = mcp.to_string();
    assert!(text.contains("label claim-conflict"), "{text}");
    assert!(text.contains("changes nothing"), "{text}");
    assert_eq!(
        world.org(),
        before,
        "a refused ljos mcp takeover wrote the tracker"
    );
}

#[test]
fn a_running_conversation_on_this_host_is_refused_even_when_the_claim_is_old() {
    let world = World::new(&[("demo-tk05", Some(("alice", STALE)))]);
    let mut running = sleeper();
    world.hold_record(
        "livealice",
        running.id(),
        &comm_of(running.id()),
        "demo-tk05",
    );
    let before = world.org();

    let refused = world.bob(&["claim", "demo-tk05", "--takeover"]);
    let text = said(&refused);
    let _ = running.kill();
    let _ = running.wait();
    assert!(!refused.status.success(), "{text}");
    assert!(
        text.contains("still running") && text.contains("changes nothing"),
        "{text}"
    );
    assert_eq!(world.org(), before, "a refused takeover wrote the tracker");
}

#[test]
fn a_lease_of_zero_turns_the_takeover_off() {
    let world = World::new(&[("demo-tk06", Some(("alice", STALE)))]);
    let before = world.org();

    let refused = world.bob_with(
        &["claim", "demo-tk06", "--takeover"],
        &[("LJOS_LEASE_SECS", "0")],
    );
    let text = said(&refused);
    assert!(!refused.status.success(), "{text}");
    assert!(text.contains("LJOS_LEASE_SECS"), "{text}");
    assert_eq!(world.org(), before);

    let plain = world.bob_with(&["claim", "demo-tk06"], &[("LJOS_LEASE_SECS", "0")]);
    let text = said(&plain);
    assert!(!plain.status.success(), "{text}");
    assert!(
        !text.contains("looks stale") && text.contains("`ljos send alice TEXT`"),
        "{text}"
    );
}

#[test]
fn a_takeover_of_a_ticket_nobody_has_is_an_ordinary_claim() {
    let world = World::new(&[("demo-tk07", None)]);
    let taken = world.bob(&["claim", "demo-tk07", "--takeover"]);
    let text = said(&taken);
    assert!(taken.status.success(), "{text}");
    let org = world.org();
    assert!(org.contains(":CLAIMED_BY: bob"), "{org}");
    assert!(!org.contains("took over"), "{org}");
}

#[test]
fn the_takeover_flag_needs_an_id_and_does_not_mix_with_next() {
    let world = World::new(&[("demo-tk08", None)]);
    let bare = world.bob(&["claim", "--takeover"]);
    assert!(!bare.status.success(), "{}", said(&bare));
    let next = world.bob(&["claim", "demo-tk08", "--takeover", "--next"]);
    assert!(!next.status.success(), "{}", said(&next));
    assert!(world.org().contains("* TODO"));
}

#[test]
fn a_sitting_on_a_stale_ticket_names_the_takeover_and_then_opens() {
    let world = World::new(&[("demo-tk09", Some(("alice", STALE)))]);
    let onboard = world.bob(&["onboard"]);
    assert!(onboard.status.success(), "{}", said(&onboard));
    let before = world.org();

    let refused = world.bob(&["sitting", "demo-tk09"]);
    let text = said(&refused);
    assert!(!refused.status.success(), "{text}");
    assert!(text.contains("The holder looks stale"), "{text}");
    assert!(text.contains("`ljos claim demo-tk09 --takeover`"), "{text}");
    assert_eq!(world.org(), before, "a refused sitting wrote the tracker");

    let taken = world.bob(&["claim", "demo-tk09", "--takeover"]);
    assert!(taken.status.success(), "{}", said(&taken));
    let sat = world.bob(&["sitting", "demo-tk09"]);
    assert!(sat.status.success(), "{}", said(&sat));
    assert!(world.org().contains(":CLAIMED_BY: bob"));
}
