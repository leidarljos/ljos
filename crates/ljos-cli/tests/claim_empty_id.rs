//! `ljos claim ""` fails at once. It used to read as `ljos claim --next`
//! and wait on `claimdag ready`, which on a seat's graph ran past a
//! minute. Only an omitted id claims the next ready node.

ljos_test_boot::harness!();
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Longer than any answer that does not reach the claim graph, and far
/// shorter than [`STALL`], so a call that waits on the graph cannot pass.
const PROMPT: Duration = Duration::from_secs(10);
/// How long the stand-in claim graph takes over any call.
const STALL: &str = "30";
/// The ticket `ljos file "Pick a codec" -p demo` prints. Filing does not
/// create the claim graph, so this id is only on the tracker.
const FILED_ID: &str = "demo-c0de";
const FILED_TITLE: &str = "Pick a codec";
/// Longer than [`STALL`], so a stalled call fails the timing assert
/// instead of being killed.
const WATCHDOG: Duration = Duration::from_secs(60);

/// Cancels the watchdog, then stops the server. The cancel runs first so
/// the watchdog cannot signal a pid the kernel has given to another test.
struct StopServer {
    child: Option<std::process::Child>,
    cancel: Option<std::sync::mpsc::Sender<()>>,
}

impl Drop for StopServer {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    ljos_cli::install_executable(&path, body).unwrap();
}

struct Hab {
    dir: tempfile::TempDir,
}

impl Hab {
    /// A claim graph that logs each call and answers `ready` with `ready`,
    /// after sleeping `sleep` seconds.
    fn new(sleep: &str, ready: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        script(
            dir.path(),
            "claimdag",
            &format!(
                "#!/bin/sh\necho \"claimdag $*\" >> '{}'\nsleep {sleep}\n[ \"$1\" = ready ] && echo '{ready}'\nexit 0\n",
                log.display()
            ),
        );
        script(dir.path(), "vissue", "#!/bin/sh\nexit 0\n");
        for name in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        Self { dir }
    }

    /// A tracker with no tickets, and a claim graph that has never been created.
    fn empty_tracker() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        let tracker = dir.path().join("tracker");
        std::fs::create_dir_all(tracker.join("Software")).unwrap();
        ljos_cli::write_file(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        script(
            dir.path(),
            "claimdag",
            &format!(
                "#!/bin/sh\necho \"claimdag $*\" >> '{}'\necho 'no work graph at /h/claims: the directory does not exist, so nothing has been claimed on this seat. Set CLAIMDAG_DIR, pass --dir, or claim something to create it' >&2\nexit 1\n",
                log.display()
            ),
        );
        script(dir.path(), "vissue", "#!/bin/sh\nexit 0\n");
        for name in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        Self { dir }
    }

    /// A tracker that can take `ljos file`. The claim graph stays absent
    /// until the first `claimdag claim`, which is what creates it.
    fn files_without_a_graph() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        let tracker = dir.path().join("tracker");
        std::fs::create_dir_all(tracker.join("Software")).unwrap();
        ljos_cli::write_file(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let template = dir.path().join("filed.org");
        ljos_cli::write_file(
            &template,
            format!(
                "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#C] {FILED_TITLE}
:PROPERTIES:
:ID:         {FILED_ID}
:CREATED:    [2026-09-20 Sat]
:END:
"
            ),
        )
        .unwrap();
        let issues = tracker.join("Software/demo/issues.org");
        script(
            dir.path(),
            "claimdag",
            &format!(
                "#!/bin/sh\necho \"claimdag $*\" >> '{}'\ncase \"$1\" in\n  ready|get)\n    echo 'no work graph at /h/claims: the directory does not exist, so nothing has been claimed on this seat. Set CLAIMDAG_DIR, pass --dir, or claim something to create it' >&2\n    exit 1\n    ;;\n  claim)\n    printf '%s\\n' 'gen=1'\n    exit 0\n    ;;\nesac\nexit 0\n",
                log.display()
            ),
        );
        script(
            dir.path(),
            "vissue",
            &format!(
                "#!/bin/sh\necho \"vissue $*\" >> '{}'\nif [ \"$1\" = create ]; then\n  mkdir -p '{}'\n  cp '{}' '{}'\n  printf '%s\\n' 'project: demo' '{}  TODO  [#C]  {}'\n  exit 0\nfi\nexit 0\n",
                log.display(),
                issues.parent().unwrap().display(),
                template.display(),
                issues.display(),
                FILED_ID,
                FILED_TITLE,
            ),
        );
        for name in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        ljos_cli::change_mode(dir.path().join("run"), 0o700).unwrap();
        Self { dir }
    }

    fn command(&self, bin: &str) -> Command {
        let home = self.dir.path().join("home");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new(bin);
        cmd.env_clear()
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.dir.path().display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .current_dir(self.dir.path());
        let tracker = self.dir.path().join("tracker");
        if tracker.join("vissue.toml").is_file() {
            cmd.env("VISSUE_ROOT", &tracker)
                .env("VISSUE_NO_ROUTE", "1")
                .env("LJOS_ASK_PACK", "off")
                .env("LJOS_USER_RULES_SYNC", "off");
        }
        cmd
    }

    fn ljos(&self, args: &[&str]) -> std::process::Output {
        self.command(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .output()
            .unwrap()
    }

    fn claim(&self, args: &[&str]) -> (std::process::Output, Duration) {
        let started = Instant::now();
        let out = self
            .command(env!("CARGO_BIN_EXE_ljos"))
            .arg("claim")
            .args(args)
            .output()
            .unwrap();
        (out, started.elapsed())
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("calls.log")).unwrap_or_default()
    }
}

#[test]
fn a_blank_id_fails_at_once_and_never_asks_the_claim_graph() {
    let hab = Hab::new(STALL, "[]");
    for blank in ["", "   "] {
        let (out, took) = hab.claim(&[blank]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "ljos claim {blank:?} succeeded");
        assert!(
            took < PROMPT,
            "ljos claim {blank:?} took {took:?}: {stderr}"
        );
        assert!(stderr.contains("the id is empty"), "{stderr}");
    }
    assert_eq!(hab.calls(), "", "the claim graph was asked");
}

#[test]
fn next_with_nothing_ready_names_a_bare_claim() {
    let hab = Hab::new("0", "[]");
    let (out, _) = hab.claim(&["--next"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!("{stdout}{stderr}");
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        said.contains("label claim-conflict:")
            && said.contains("nothing in the claim graph is ready")
            && said.contains("so run `ljos claim`."),
        "{said}"
    );
    assert!(hab.calls().contains("claimdag ready"), "{}", hab.calls());
}

#[test]
fn next_with_no_claim_graph_names_a_bare_claim() {
    let hab = Hab::empty_tracker();
    let (out, _) = hab.claim(&["--next"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stdout}{stderr}");
    assert!(stdout.trim().is_empty(), "{stdout}");
    assert_eq!(
        stderr.trim(),
        "claim --next: nothing has been claimed on this seat yet, so run `ljos claim`."
    );
}

#[test]
fn role_and_slack_need_next() {
    let hab = Hab::new(STALL, "[]");
    for args in [&["--role", "explore"][..], &["--slack", "2"][..]] {
        let (out, took) = hab.claim(args);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{stdout}{stderr}");
        assert!(took < PROMPT, "{args:?} took {took:?}");
        assert!(stdout.trim().is_empty(), "{stdout}");
        assert!(stderr.contains("--next"), "{stderr}");
    }
    assert_eq!(hab.calls(), "", "the claim graph was asked");
}

/// One sentence, and only that sentence, when the tracker has nothing ready.
const NO_READY_TICKET: &str =
    "claim: no ticket is ready on this seat, so file one with `ljos file \"TITLE\" -p PROJECT`";

#[test]
fn an_empty_tracker_names_one_sentence() {
    let hab = Hab::empty_tracker();
    let (out, _) = hab.claim(&[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "bare claim succeeded\n{stdout}{stderr}"
    );
    assert_eq!(stderr.trim(), NO_READY_TICKET, "stdout:\n{stdout}");
    assert!(stdout.trim().is_empty(), "{stdout}");
    assert_eq!(hab.calls(), "", "an empty tracker asked the claim graph");
}

#[test]
fn filing_then_a_bare_claim_claims_the_new_ticket() {
    // The README ends on `ljos file "Pick a codec" -p demo`. Filing leaves
    // the claim graph uncreated. A bare `ljos claim` claims the first
    // ticket `ljos ready` lists, and that claim creates the graph.
    let hab = Hab::files_without_a_graph();
    let filed = hab.ljos(&["file", "Pick a codec", "-p", "demo"]);
    let stdout = String::from_utf8_lossy(&filed.stdout);
    let stderr = String::from_utf8_lossy(&filed.stderr);
    assert!(
        filed.status.success(),
        "file failed\n{stdout}{stderr}\n{}",
        hab.calls()
    );
    let id = stdout.lines().next().unwrap_or("").trim();
    assert_eq!(id, FILED_ID, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let ready = hab.ljos(&["ready"]);
    let ready_out = String::from_utf8_lossy(&ready.stdout);
    let ready_err = String::from_utf8_lossy(&ready.stderr);
    assert!(
        ready.status.success() && ready_out.contains(FILED_ID) && ready_out.contains(FILED_TITLE),
        "ready did not list the filed ticket\n{ready_out}{ready_err}"
    );
    assert!(
        !hab.calls().contains("claimdag claim"),
        "filing created the claim graph: {}",
        hab.calls()
    );
    let (out, _) = hab.claim(&[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let said = format!("{stdout}{stderr}");
    assert!(out.status.success(), "{said}");
    assert!(
        stdout.contains(FILED_ID),
        "bare claim did not take {FILED_ID}: {said}"
    );
    let calls = hab.calls();
    assert!(
        calls.lines().any(|line| line.starts_with("vissue create")),
        "file did not create a ticket: {calls}"
    );
    assert!(
        calls.contains("claimdag claim"),
        "the claim did not go through claimdag claim: {calls}"
    );
}

#[test]
fn the_mcp_claim_tool_answers_a_blank_id_at_once() {
    let hab = Hab::new(STALL, "[]");
    let child = hab
        .command(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env("LJOS_MCP_TOOLS", "all")
        .env("LJOS_SESSION_ID", "0e610c5a-32e0-7dab-eb9b-ab7d13ce6556")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let (cancel, cancelled) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if matches!(
            cancelled.recv_timeout(WATCHDOG),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) {
            #[expect(
                clippy::disallowed_methods,
                reason = "starts a program outside git hook code"
            )]
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
    });
    let mut server = StopServer {
        child: Some(child),
        cancel: Some(cancel),
    };
    let (mut stdin, stdout) = {
        let held = server.child.as_mut().unwrap();
        (held.stdin.take().unwrap(), held.stdout.take().unwrap())
    };
    for line in [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    let mut lines = BufReader::new(stdout).lines();
    lines.next().unwrap().unwrap();
    let started = Instant::now();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "ljos_claim", "arguments": {"seat": "saddle", "node": ""}}})
    )
    .unwrap();
    let answer = lines
        .map(Result::unwrap)
        .find(|l| l.contains("\"id\":2"))
        .expect("no answer to the claim");
    let took = started.elapsed();
    drop(stdin);
    drop(server);
    assert!(took < PROMPT, "ljos_claim took {took:?}: {answer}");
    assert!(answer.contains("the id is empty"), "{answer}");
    assert_eq!(hab.calls(), "", "the claim graph was asked");
}
