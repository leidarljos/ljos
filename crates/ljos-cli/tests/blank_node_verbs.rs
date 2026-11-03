//! `ljos release ""` and `ljos complete ""` fail at once. They used to
//! hash the blank id and ask the claim graph about that node, so a
//! release reported success for a node nobody named, after waiting as
//! long as the graph took.

ljos_test_boot::harness!();
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// Kills the server when the test ends, including on a failed assertion.
struct Stop(Child);

impl Drop for Stop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Longer than any answer that does not reach the claim graph, and far
/// shorter than [`STALL`], so a call that waits on the graph cannot pass.
/// Ten seconds is the acceptance bound for this check.
const PROMPT: Duration = Duration::from_secs(10);
/// Seconds the stand-in sleeps on every call. One claim-graph call on this
/// bug took thirty seconds, and a release makes two, so a miss here runs
/// about a minute and cannot finish inside [`PROMPT`].
const STALL: &str = "30";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Hab {
    dir: tempfile::TempDir,
}

impl Hab {
    /// A claim graph that logs each call and then sleeps `sleep` seconds.
    fn new(sleep: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        script(
            dir.path(),
            "claimdag",
            &format!(
                "#!/bin/sh\necho \"claimdag $*\" >> '{}'\nsleep {sleep}\nexit 0\n",
                log.display()
            ),
        );
        script(dir.path(), "vissue", "#!/bin/sh\nexit 0\n");
        for name in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        Self { dir }
    }

    fn command(&self, bin: &str) -> Command {
        let home = self.dir.path().join("home");
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
        cmd
    }

    fn verb(&self, verb: &str, args: &[&str]) -> (std::process::Output, Duration) {
        let started = Instant::now();
        let out = self
            .command(env!("CARGO_BIN_EXE_ljos"))
            .arg(verb)
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
    let hab = Hab::new(STALL);
    for verb in ["release", "complete"] {
        for blank in ["", "   "] {
            let (out, took) = hab.verb(verb, &[blank]);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                !out.status.success(),
                "ljos {verb} {blank:?} succeeded: {stderr}"
            );
            assert!(
                took < PROMPT,
                "ljos {verb} {blank:?} took {took:?}: {stderr}"
            );
            assert!(
                stderr.contains(&format!("{verb}: the id is empty")),
                "{stderr}"
            );
        }
    }
    assert_eq!(
        hab.calls(),
        "",
        "the claim graph was asked: {}",
        hab.calls()
    );
}

/// The next JSON-RPC line whose id is `id`, or why it did not arrive in time.
fn line_for(rx: &Receiver<String>, id: u64, limit: Duration) -> Result<String, String> {
    let deadline = Instant::now() + limit;
    let needle = format!("\"id\":{id}");
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(format!("no answer with {needle} within {limit:?}"));
        }
        match rx.recv_timeout(left) {
            Ok(line) if line.contains(&needle) => return Ok(line),
            Ok(_) => {}
            Err(err) => return Err(format!("no answer with {needle}: {err}")),
        }
    }
}

#[test]
fn the_mcp_release_and_complete_tools_answer_a_blank_id_at_once() {
    let hab = Hab::new(STALL);
    let child = hab
        .command(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env("LJOS_MCP_TOOLS", "all")
        .env("LJOS_SESSION_ID", "0e610c5a-32e0-7dab-eb9b-ab7d13ce6556")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = Stop(child);
    let mut stdin = child.0.stdin.take().unwrap();
    let stdout = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    for line in [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    stdin.flush().unwrap();
    line_for(&rx, 1, PROMPT).expect("the server did not initialize");
    for (id, tool) in [(2, "ljos_release"), (3, "ljos_complete")] {
        let started = Instant::now();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {
                "name": tool, "arguments": {"node": ""}}})
        )
        .unwrap();
        stdin.flush().unwrap();
        let answer = line_for(&rx, id, PROMPT).unwrap_or_else(|why| panic!("{tool}: {why}"));
        let took = started.elapsed();
        let verb = tool.trim_start_matches("ljos_");
        assert!(took < PROMPT, "{tool} took {took:?}: {answer}");
        assert!(
            answer.contains(&format!("{verb}: the id is empty")),
            "{answer}"
        );
    }
    drop(stdin);
    drop(child);
    assert_eq!(
        hab.calls(),
        "",
        "the claim graph was asked: {}",
        hab.calls()
    );
}
