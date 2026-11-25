//! Two seats meet on a ticket the claim graph does not hold yet. One reads
//! the graph, finds no node, and mints it; the other has minted and claimed
//! that node in between. The late seat is told who holds the ticket, and
//! never that it could not mint a node. A graph that cannot be read is
//! reported as that, and not as a failed mint, and a node it holds is
//! never minted over.

ljos_test_boot::harness!();

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

#[path = "common/sibling.rs"]
mod sibling;

const TICKET: &str = "demo-t00";

/// The other seat's actor in the deterministic case: a 32-hex id no hold
/// record on this host names.
const OTHER: &str = "0000000000000000000000000000000b";

const SEATS: usize = 4;
const ROUNDS: usize = 12;

struct World {
    root: tempfile::TempDir,
    claimdag: PathBuf,
    siblings: PathBuf,
}

impl World {
    fn new() -> Self {
        let claimdag = sibling::require("claimdag").unwrap_or_else(|line| panic!("{line}"));
        let vissue = sibling::require("vissue").unwrap_or_else(|line| panic!("{line}"));
        let siblings = claimdag.parent().unwrap().to_path_buf();
        assert_eq!(vissue.parent(), Some(siblings.as_path()));
        let root = tempfile::tempdir().unwrap();
        let tracker = root.path().join("tracker");
        std::fs::create_dir_all(tracker.join("Software/demo")).unwrap();
        std::fs::create_dir_all(root.path().join("bin")).unwrap();
        ljos_cli::write_file(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let tickets: String = (0..SEATS)
            .map(|k| {
                format!(
                    "* TODO [#C] Ticket {k}\n:PROPERTIES:\n:ID:         demo-t{k:02}\n:END:\n\n"
                )
            })
            .collect();
        ljos_cli::write_file(
            tracker.join("Software/demo/issues.org"),
            format!(
                "#+TITLE: demo\n#+VISSUE: 1\n#+CATEGORY: demo\n#+TODO: TODO STARTED | DONE CANCELLED\n\n{tickets}"
            ),
        )
        .unwrap();
        Self {
            root,
            claimdag,
            siblings,
        }
    }

    fn graph(&self) -> PathBuf {
        self.root.path().join("claims")
    }

    fn command(&self, seat: &str, args: &[&str]) -> Command {
        let home = self.root.path().join("homes").join(seat);
        std::fs::create_dir_all(home.join("run")).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:{}:/usr/bin:/bin",
                    self.root.path().join("bin").display(),
                    self.siblings.display()
                ),
            )
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("CLAIMDAG_DIR", self.graph())
            .env("VISSUE_ROOT", self.root.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", seat)
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .env("PACKSET_AUTH", "off")
            .current_dir(&home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn node(&self, issue: &str) -> String {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let out = Command::new(&self.claimdag)
            .args(["--dir"])
            .arg(self.graph())
            .args(["get", &ljos_cli::work_id(issue)])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        text(&out)
    }
}

fn text(out: &Output) -> String {
    format!(
        "exit {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_node_minted_and_claimed_between_the_read_and_the_mint_is_held_by_another() {
    let world = World::new();
    let marker = world.root.path().join("interleaved");
    // The first mint goes second: the other seat mints and claims the same
    // node, then this seat's own `upsert` runs against a claimed node.
    ljos_cli::install_executable(
        &world.root.path().join("bin/claimdag"),
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = upsert ] && [ ! -e '{marker}' ]; then\n\
             : > '{marker}'\n\
             '{real}' upsert --id \"$3\" --summary {TICKET} >/dev/null || exit 97\n\
             '{real}' claim \"$3\" --assignee {OTHER} >/dev/null || exit 98\n\
             fi\n\
             exec '{real}' \"$@\"\n",
            marker = marker.display(),
            real = world.claimdag.display(),
        ),
    )
    .unwrap();

    let out = world.command("seat0", &["claim", TICKET]).output().unwrap();
    let said = text(&out);

    assert!(
        marker.exists(),
        "the seat never minted, so the race was not run\n{said}"
    );
    assert!(!said.contains("could not mint"), "{said}");
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("held by another"), "{said}");
    let node = world.node(TICKET);
    assert!(node.contains(&format!("assignee={OTHER}")), "{node}");
}

#[test]
fn a_graph_that_cannot_be_read_is_reported_and_not_a_failed_mint() {
    let world = World::new();
    ljos_cli::install_executable(
        &world.root.path().join("bin/claimdag"),
        "#!/bin/sh\necho \"$1: snapshot checksum mismatch\" >&2\nexit 1\n",
    )
    .unwrap();

    let out = world.command("seat0", &["claim", TICKET]).output().unwrap();
    let said = text(&out);

    assert!(!out.status.success(), "{said}");
    assert!(said.contains("could not read the node"), "{said}");
    assert!(said.contains("get: snapshot checksum mismatch"), "{said}");
    assert!(!said.contains("could not mint"), "{said}");
}

/// A node the graph already holds, with a kind, status and summary a mint
/// would overwrite, and a claimdag on PATH that logs every call and answers
/// `get` with a checksum failure: every `get`, or only the first.
fn seeded_world_with_failing_reads(only_first: bool) -> (World, PathBuf) {
    let world = World::new();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let seeded = Command::new(&world.claimdag)
        .args(["--dir"])
        .arg(world.graph())
        .args(["upsert", "--id", &ljos_cli::work_id(TICKET)])
        .args(["--kind", "goal", "--status", "ready"])
        .args(["--summary", "preserve-this-summary"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(seeded.status.success(), "{}", text(&seeded));
    let log = world.root.path().join("claimdag.log");
    let marker = world.root.path().join("read-failed");
    let fail_when = if only_first {
        format!("[ \"$1\" = get ] && [ ! -e '{}' ]", marker.display())
    } else {
        "[ \"$1\" = get ]".to_string()
    };
    ljos_cli::install_executable(
        &world.root.path().join("bin/claimdag"),
        format!(
            "#!/bin/sh\n\
             echo \"$*\" >> '{log}'\n\
             if {fail_when}; then\n\
             : > '{marker}'\n\
             echo 'snapshot checksum mismatch' >&2\n\
             exit 1\n\
             fi\n\
             exec '{real}' \"$@\"\n",
            log = log.display(),
            marker = marker.display(),
            real = world.claimdag.display(),
        ),
    )
    .unwrap();
    (world, log)
}

#[test]
fn a_read_that_fails_for_another_reason_never_rewrites_the_node() {
    let (world, log) = seeded_world_with_failing_reads(false);
    let before = world.node(TICKET);
    assert!(before.contains("preserve-this-summary"), "{before}");

    let out = world.command("seat0", &["claim", TICKET]).output().unwrap();
    let said = text(&out);
    let calls = std::fs::read_to_string(&log).unwrap_or_default();

    assert!(!out.status.success(), "{said}");
    assert!(said.contains("could not read the node"), "{said}");
    assert!(said.contains("snapshot checksum mismatch"), "{said}");
    assert!(!said.contains("could not mint"), "{said}");
    assert!(
        !calls.lines().any(|l| l.starts_with("upsert")),
        "{calls}\n{said}"
    );
    assert_eq!(world.node(TICKET), before, "{said}");
}

#[test]
fn a_read_that_fails_once_is_asked_again_and_not_minted_over() {
    let (world, log) = seeded_world_with_failing_reads(true);

    let out = world.command("seat0", &["claim", TICKET]).output().unwrap();
    let said = text(&out);
    let calls = std::fs::read_to_string(&log).unwrap_or_default();

    assert!(
        calls.lines().any(|l| l.starts_with("get")),
        "{calls}\n{said}"
    );
    assert!(
        !calls.lines().any(|l| l.starts_with("upsert")),
        "{calls}\n{said}"
    );
    let node = world.node(TICKET);
    assert!(node.contains("goal"), "{node}\n{said}");
    assert!(node.contains("preserve-this-summary"), "{node}\n{said}");
}

#[test]
fn seats_on_a_fresh_graph_never_fail_to_mint() {
    for round in 0..ROUNDS {
        let world = World::new();
        let seats: Vec<(String, Vec<&str>)> = (0..SEATS)
            .map(|k| {
                let args = if k % 2 == 0 {
                    vec!["claim", TICKET]
                } else {
                    vec!["claim"]
                };
                (format!("seat{k}"), args)
            })
            .collect();
        let running: Vec<_> = seats
            .iter()
            .map(|(seat, args)| (seat, world.command(seat, args).spawn().unwrap()))
            .collect();
        let outs: Vec<(String, String)> = running
            .into_iter()
            .map(|(seat, child)| (seat.clone(), text(&child.wait_with_output().unwrap())))
            .collect();
        let all = outs
            .iter()
            .map(|(seat, said)| format!("[{seat}] {said}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !all.contains("could not mint"),
            "round {round}: a seat could not mint\n{all}"
        );
        let node = world.node(TICKET);
        assert!(
            node.contains(" claimed "),
            "round {round}: nobody holds {TICKET}\n{node}\n{all}"
        );
    }
}
