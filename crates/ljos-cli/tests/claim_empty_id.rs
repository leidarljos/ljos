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
fn an_omitted_id_still_claims_the_next_ready_node() {
    let hab = Hab::new("0", "[]");
    let (out, _) = hab.claim(&[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("nothing in the claim graph is ready"),
        "{stderr}"
    );
    assert!(hab.calls().contains("claimdag ready"), "{}", hab.calls());
}

#[test]
fn the_mcp_claim_tool_answers_a_blank_id_at_once() {
    let hab = Hab::new(STALL, "[]");
    let mut child = hab
        .command(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env("LJOS_MCP_TOOLS", "all")
        .env("GROK_SESSION_ID", "0e610c5a-32e0-7dab-eb9b-ab7d13ce6556")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut stdin = child.stdin.take().unwrap();
    for line in [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "probe", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    lines.next().unwrap().unwrap();
    let started = Instant::now();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "ljos_claim", "arguments": {"node": ""}}})
    )
    .unwrap();
    let answer = lines
        .map(Result::unwrap)
        .find(|l| l.contains("\"id\":2"))
        .expect("no answer to the claim");
    let took = started.elapsed();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    assert!(took < PROMPT, "ljos_claim took {took:?}: {answer}");
    assert!(answer.contains("the id is empty"), "{answer}");
    assert_eq!(hab.calls(), "", "the claim graph was asked");
}
