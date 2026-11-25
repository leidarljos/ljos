//! `ljos claim` with no id takes the first ready ticket in `ljos ready`
//! order, every time and not only before the claim graph exists. A ticket
//! another seat holds is passed over, a seat that already holds one gets
//! the busy refusal, and a host with no tracker names `ljos onboard`.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;
#[path = "common/sibling.rs"]
mod sibling;

use std::ffi::CString;
use std::io::Read as _;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const NO_READY_TICKET: &str =
    "claim: no ticket is ready on this seat, so file one with `ljos file \"TITLE\" -p PROJECT`";

/// A claim stamp from today, so no held ticket here is long-stale.
fn today_stamp() -> String {
    let out = ljos_cli::program("date")
        .unwrap()
        .arg("+[%Y-%m-%d %a %H:%M]")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// One host: a shared tracker and claim graph, and a home for each seat.
struct World {
    _dir: tempfile::TempDir,
    path: String,
    tracker: PathBuf,
    homes: PathBuf,
    graph: PathBuf,
}

impl World {
    /// `tickets` are ids, each TODO, or STARTED under the named holder.
    fn new(tickets: &[(&str, Option<&str>)]) -> World {
        let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
        let claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
        let siblings = vissue.parent().unwrap();
        assert_eq!(claimdag.parent(), Some(siblings));
        let dir = tempfile::tempdir().unwrap();
        let tracker = dir.path().join("tracker");
        std::fs::create_dir_all(tracker.join("Software/demo")).unwrap();
        ljos_cli::write_file(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let at = today_stamp();
        let mut org = String::from(
            "#+TITLE: demo\n#+VISSUE: 1\n#+CATEGORY: demo\n#+TODO: TODO STARTED | DONE CANCELLED\n\n",
        );
        for (id, held) in tickets {
            match held {
                Some(who) => org.push_str(&format!(
                    "* STARTED [#C] Ticket {id}\n:PROPERTIES:\n:ID:         {id}\n:CLAIMED_BY: {who}\n:CLAIMED_AT: {at}\n:END:\n\n"
                )),
                None => org.push_str(&format!(
                    "* TODO [#C] Ticket {id}\n:PROPERTIES:\n:ID:         {id}\n:END:\n\n"
                )),
            }
        }
        ljos_cli::write_file(tracker.join("Software/demo/issues.org"), org).unwrap();
        World {
            path: format!("{}:/usr/bin:/bin", siblings.display()),
            tracker,
            homes: dir.path().join("homes"),
            graph: dir.path().join("claims"),
            _dir: dir,
        }
    }

    fn cmd(&self, bin: &Path, seat: &str) -> Command {
        let home = self.homes.join(seat);
        let runtime = home.join("run");
        std::fs::create_dir_all(&runtime).unwrap();
        let mut cmd = ljos_cli::program(bin).unwrap();
        cmd.env_clear()
            .env("PATH", &self.path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("CLAIMDAG_DIR", &self.graph)
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .current_dir(&home)
            .stdin(Stdio::null());
        seat_env::clear(&mut cmd);
        cmd.env("LJOS_SEAT", seat);
        cmd
    }

    fn ljos(&self, seat: &str, args: &[&str]) -> Output {
        self.cmd(Path::new(env!("CARGO_BIN_EXE_ljos")), seat)
            .args(args)
            .output()
            .unwrap()
    }

    /// The id `ljos ready` lists first for `seat`.
    fn first_ready(&self, seat: &str) -> String {
        let out = self.ljos(seat, &["ready", "--json"]);
        assert!(out.status.success(), "{}", said(&out));
        let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        doc["rows"][0]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("no ready row: {doc}"))
            .to_string()
    }

    fn close(&self, seat: &str, id: &str) {
        let out = self
            .cmd(Path::new("vissue"), seat)
            .args(["update", id, "--state", "DONE"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", said(&out));
    }

    fn org(&self) -> String {
        std::fs::read_to_string(self.tracker.join("Software/demo/issues.org")).unwrap()
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The id a successful bare claim names first on stdout.
fn claimed(out: &Output) -> String {
    assert!(out.status.success(), "{}", said(out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("nothing claimed: {}", said(out)))
        .to_string()
}

fn fifo(path: &Path) {
    let c_path = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: c_path is a NUL-terminated path and mode has no invalid bits.
    let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    assert_eq!(
        rc,
        0,
        "mkfifo {}: {}",
        path.display(),
        std::io::Error::last_os_error()
    );
}

#[test]
fn simultaneous_bare_claims_pass_over_a_node_claimed_during_mint() {
    let world = World::new(&[("demo-aa01", None), ("demo-bb02", None)]);
    let gate = tempfile::tempdir().unwrap();
    let waiting_fifo = gate.path().join("waiting");
    let release_fifo = gate.path().join("release");
    fifo(&waiting_fifo);
    fifo(&release_fifo);
    let real_claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
    let race_node = ljos_cli::work_id("demo-aa01");
    ljos_cli::install_executable(
        &gate.path().join("claimdag"),
        r#"#!/bin/sh
if [ "${LJOS_SEAT:-}" = seat0 ] && [ "${1:-}" = upsert ]; then
  case " $* " in
    *" --id $RACE_NODE "*)
      printf x > "$WAITING_FIFO"
      IFS= read -r _ < "$RELEASE_FIFO"
      ;;
  esac
fi
"$REAL_CLAIMDAG" "$@"
rc=$?
if [ "${LJOS_SEAT:-}" != seat0 ] &&
   [ "${1:-}" = claim ] &&
   [ "${2:-}" = "$RACE_NODE" ] &&
   [ "$rc" -eq 0 ]; then
  printf x > "$RELEASE_FIFO"
fi
exit "$rc"
"#,
    )
    .unwrap();
    let path = format!("{}:{}", gate.path().display(), world.path);
    let configure = |cmd: &mut Command| {
        cmd.arg("claim")
            .env("PATH", &path)
            .env("REAL_CLAIMDAG", &real_claimdag)
            .env("RACE_NODE", &race_node)
            .env("WAITING_FIFO", &waiting_fifo)
            .env("RELEASE_FIFO", &release_fifo)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    };

    #[expect(
        clippy::disallowed_methods,
        reason = "opens a FIFO read-write so setup does not block before the child starts"
    )]
    let mut waiting = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&waiting_fifo)
        .unwrap();
    let mut slow_cmd = world.cmd(Path::new(env!("CARGO_BIN_EXE_ljos")), "seat0");
    configure(&mut slow_cmd);
    let slow = slow_cmd.spawn().unwrap();
    let mut signal = [0u8; 1];
    waiting.read_exact(&mut signal).unwrap();

    let mut fast_cmd = world.cmd(Path::new(env!("CARGO_BIN_EXE_ljos")), "seat1");
    configure(&mut fast_cmd);
    let fast = fast_cmd.spawn().unwrap();
    let fast = fast.wait_with_output().unwrap();
    let slow = slow.wait_with_output().unwrap();
    assert_eq!(claimed(&fast), "demo-aa01", "{}", said(&fast));
    assert_eq!(claimed(&slow), "demo-bb02", "{}", said(&slow));
}

#[test]
fn another_seat_takes_the_next_ticket() {
    let world = World::new(&[
        ("demo-aa01", None),
        ("demo-bb02", None),
        ("demo-cc03", None),
    ]);
    let first = world.first_ready("alice");
    assert_eq!(first, "demo-aa01");
    assert_eq!(claimed(&world.ljos("alice", &["claim"])), first);
    let next = world.ljos("bob", &["claim"]);
    assert_eq!(claimed(&next), "demo-bb02", "{}", said(&next));
    let org = world.org();
    assert!(org.contains(":CLAIMED_BY: alice"), "{org}");
    assert!(org.contains(":CLAIMED_BY: bob"), "{org}");
}

#[test]
fn the_same_seat_is_refused_while_it_holds_one_then_takes_the_next() {
    let world = World::new(&[
        ("demo-aa01", None),
        ("demo-bb02", None),
        ("demo-cc03", None),
    ]);
    assert_eq!(claimed(&world.ljos("alice", &["claim"])), "demo-aa01");

    let busy = world.ljos("alice", &["claim"]);
    let text = said(&busy);
    assert!(
        text.contains("label claim-conflict:")
            && text.contains("alice already holds a live node")
            && text.contains("holds demo-aa01:")
            && text.contains("`ljos release demo-aa01 --assignee alice`"),
        "{text}"
    );
    assert_eq!(world.org().matches(":CLAIMED_BY: alice").count(), 1);

    let released = world.ljos("alice", &["release", "demo-aa01"]);
    assert!(released.status.success(), "{}", said(&released));
    let first = world.first_ready("alice");
    assert_eq!(claimed(&world.ljos("alice", &["claim"])), first);

    world.close("alice", &first);
    let next = world.ljos("alice", &["claim"]);
    assert_eq!(claimed(&next), "demo-bb02", "{}", said(&next));
}

#[test]
fn every_ticket_held_by_another_seat_gives_one_sentence() {
    let world = World::new(&[("demo-aa01", Some("carol")), ("demo-bb02", Some("carol"))]);
    let out = world.ljos("bob", &["claim"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stdout}{stderr}");
    assert!(stdout.trim().is_empty(), "{stdout}");
    let last = stderr.lines().last().unwrap_or("");
    assert_eq!(last, NO_READY_TICKET, "{stderr}");
    assert!(!world.org().contains(":CLAIMED_BY: bob"), "{}", world.org());
}

#[test]
fn a_home_that_was_never_onboarded_names_ljos_onboard() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("run")).unwrap();
    let mut cmd = ljos_cli::program(env!("CARGO_BIN_EXE_ljos")).unwrap();
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("PACKSET_URL", "off")
        .current_dir(&home)
        .stdin(Stdio::null());
    seat_env::clear(&mut cmd);
    cmd.env("LJOS_SEAT", "alice");
    let out = cmd.arg("claim").output().unwrap();
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("`ljos onboard`"), "{text}");
    assert!(!text.contains("ljos file"), "{text}");
}
