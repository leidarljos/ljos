//! Regression for seat and session holder claim dedupe across MCP starts.

ljos_test_boot::harness!();

#[path = "common/sibling.rs"]
mod sibling;

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn result_text(answer: &serde_json::Value) -> String {
    answer["result"]["structuredContent"]["text"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| answer.to_string())
}

fn read_response(stdout: &mut BufReader<std::process::ChildStdout>, id: u64) -> serde_json::Value {
    let mut line = String::new();
    loop {
        line.clear();
        assert_ne!(
            stdout.read_line(&mut line).unwrap(),
            0,
            "no response for {id}"
        );
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value["id"].as_u64() == Some(id) {
            return value;
        }
    }
}

fn started_ids(org: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_started = false;
    for line in org.lines() {
        let line = line.trim_end();
        if line.starts_with("* STARTED ") {
            in_started = true;
            continue;
        }
        if line.starts_with("* ") {
            in_started = false;
            continue;
        }
        if in_started {
            if let Some(id) = line.trim().strip_prefix(":ID:") {
                let id = id.trim();
                if !id.is_empty() {
                    out.push(id.to_string());
                }
            }
        }
    }
    out
}

fn claim_rows(org: &str) -> Vec<String> {
    org.lines()
        .filter_map(|line| line.trim().strip_prefix(":CLAIMED_BY:"))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn claimed_id(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("claimed "))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn issue_file_for(tracker: &std::path::Path, id: &str) -> Option<PathBuf> {
    let mut stack = vec![tracker.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) != Some("issues.org") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if text.contains(&format!(":ID:         {id}")) {
                return Some(path);
            }
        }
    }
    None
}

struct Env {
    _root: tempfile::TempDir,
    path: String,
    home: PathBuf,
    runtime: PathBuf,
    repo: PathBuf,
    tracker: PathBuf,
}

impl Env {
    fn spawn_mcp(&self) -> std::process::Child {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
        cmd.env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .current_dir(&self.repo)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.spawn().unwrap()
    }

    fn claimdag_get(&self, issue: &str) -> String {
        let node = ljos_cli::work_id(issue);
        let out = Command::new("claimdag")
            .arg("get")
            .arg(&node)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .current_dir(&self.repo)
            .output()
            .unwrap();
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }
}

fn setup(case: &str) -> Env {
    let vissue = sibling::require("vissue").unwrap_or_else(|e| panic!("{e}"));
    let claimdag = sibling::require("claimdag").unwrap_or_else(|e| panic!("{e}"));
    let _deedar = sibling::require("deedar").unwrap_or_else(|e| panic!("{e}"));
    let siblings = vissue.parent().unwrap();
    assert_eq!(claimdag.parent(), Some(siblings));
    let path = format!("{}:/usr/bin:/bin", siblings.display());

    let root = tempfile::tempdir().unwrap();
    let home = root.path().join(format!("{case}-home"));
    let repo = root.path().join(format!("{case}-repo"));
    let tracker = root.path().join(format!("{case}-tracker"));
    let runtime = root.path().join(format!("{case}-run"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"saddle\"]\n").unwrap();
    std::fs::write(repo.join("README.md"), "seed\n").unwrap();

    Env {
        _root: root,
        path,
        home,
        runtime,
        repo,
        tracker,
    }
}

fn run_order(case: &str, first_meta: bool) -> (String, String, String, String) {
    let env = setup(case);
    let session = "harness-session-xyz1";
    let mut child = env.spawn_mcp();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "probe", "version": "0"}
            }
        })
    )
    .unwrap();
    let _initialized = read_response(&mut stdout, 1);
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();

    let plain = serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"ljos_start","arguments":{"seat":"saddle","task":"plain start","repo":env.repo.display().to_string()}}
    });
    let meta = serde_json::json!({
        "jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"ljos_start","arguments":{"seat":"saddle","task":"meta start","repo":env.repo.display().to_string()},"_meta":{"session_id":session}}
    });

    let (first_rsp, second_rsp) = if first_meta {
        writeln!(stdin, "{meta}").unwrap();
        let first = read_response(&mut stdout, 3);
        writeln!(stdin, "{plain}").unwrap();
        let second = read_response(&mut stdout, 2);
        (first, second)
    } else {
        writeln!(stdin, "{plain}").unwrap();
        let first = read_response(&mut stdout, 2);
        writeln!(stdin, "{meta}").unwrap();
        let second = read_response(&mut stdout, 3);
        (first, second)
    };
    drop(stdin);
    drop(stdout);
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(!stderr.contains("panicked"), "{stderr}");

    let first_text = result_text(&first_rsp);
    let second_text = result_text(&second_rsp);
    let issue = claimed_id(&second_text)
        .or_else(|| claimed_id(&first_text))
        .unwrap_or_default();
    let org = issue_file_for(&env.tracker, &issue)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let starts = started_ids(&org);
    let claims = claim_rows(&org);
    let expected_holder = if first_meta {
        "saddle".to_string()
    } else {
        ljos_cli::holder_for_session("saddle", session)
    };
    let get = if issue.is_empty() {
        String::new()
    } else {
        env.claimdag_get(&issue)
    };
    let expected_actor = if issue.is_empty() {
        String::new()
    } else {
        ljos_cli::work_id(&ljos_cli::occupancy_assignee(
            Some(&expected_holder),
            &issue,
        ))
    };
    eprintln!(
        "{case}\nfirst:\n{}\nsecond:\n{}\nstderr:\n{stderr}\nstarted={starts:?}\nclaims={claims:?}\nexpected_holder={expected_holder}\nexpected_actor={expected_actor}\nclaimdag_get:\n{get}\nissues.org:\n{org}",
        first_text,
        second_text
    );
    assert_eq!(starts.len(), 1, "{case} started ids mismatch\n{org}");
    assert_eq!(claims.len(), 1, "{case} claim rows mismatch\n{org}");
    assert_eq!(
        claims[0], expected_holder,
        "{case} claim row mismatch\nclaims={claims:?}\norg:\n{org}"
    );
    assert!(
        get.contains("claimed") && get.contains(&format!("assignee={expected_actor}")),
        "{case} claimdag occupancy mismatch\nexpected assignee={expected_actor}\nget:\n{get}\norg:\n{org}"
    );
    (issue, expected_holder, expected_actor, get)
}

#[test]
fn plain_then_session_meta_keeps_one_started_claim() {
    let _ = run_order("plain-then-meta", false);
}

#[test]
fn session_meta_then_plain_keeps_one_started_claim() {
    let _ = run_order("meta-then-plain", true);
}
