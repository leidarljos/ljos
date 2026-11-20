//! Verify `ljos hook` preserves late judge replies after the hook process exits.

ljos_test_boot::harness!();

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[path = "common/runner.rs"]
mod runner;

fn read_http(stream: &mut std::net::TcpStream) -> String {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let mut buf = [0u8; 4096];
    let mut out = Vec::new();
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn serve_mock_pack(hits_json: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            std::thread::spawn(move || {
                let req = read_http(&mut s);
                let first = req.lines().next().unwrap_or("").to_string();
                let authed = req.lines().any(|line| {
                    line.split_once(':').is_some_and(|(name, value)| {
                        name.eq_ignore_ascii_case("authorization")
                            && value.trim() == "Bearer test-token"
                    })
                });
                let (status, body) = if !authed {
                    ("401 Unauthorized", r#"{"error":"token"}"#.to_string())
                } else if first.contains("/v1/search") {
                    ("200 OK", format!(r#"{{"hits":{hits_json}}}"#))
                } else if first.starts_with("GET /v1/atoms") {
                    ("200 OK", format!(r#"{{"atoms":{hits_json}}}"#))
                } else {
                    ("404 Not Found", r#"{"error":"not here"}"#.to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });
    url
}

fn clear_runner_env(cmd: &mut Command) {
    for (k, _) in std::env::vars_os() {
        let Some(name) = k.to_str() else { continue };
        if name.starts_with("CURSOR_")
            || name.starts_with("AGENT_")
            || name.starts_with("LJOS")
            || name.starts_with("XDG_")
        {
            cmd.env_remove(k);
        }
    }
    cmd.env_remove("GITHUB_RUN_ID");
}

#[test]
fn late_judge_reply_persists_after_ljos_hook_exits() {
    let hits = r#"[{"id":"p1","text":"Tag a release with git push origin TAG; --follow-tags leaves v-tags behind.","score":2.0,"kind":"preference","ts":"2026-10-10T18:00:00.000Z","entities":[],"origin":"user-declared","source":{"basis":"person"},"ballots":3,"of":3}]"#;
    let url = serve_mock_pack(hits);
    let installed = runner::install_dir();
    let root = tempfile::Builder::new()
        .prefix("jev-late-")
        .tempdir_in(runner::non_temp_homes_root())
        .unwrap();
    let home = root.path().join("home");
    let cfg = root.path().join("cfg");
    let state = root.path().join("state");
    let cache = root.path().join("cache");
    let runtime = root.path().join("run");
    let work = root.path().join("work");
    for d in [&home, &cfg, &state, &cache, &runtime, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::create_dir_all(cfg.join("ljos")).unwrap();
    let started = runtime.join("judge-started");
    let done = runtime.join("judge-done");
    std::fs::write(
        cfg.join("ljos/jev.toml"),
        format!(
            "enabled = true\nbackend = \"command\"\ncache_days = 1\nmin_words = 1\nmin_candidates = 1\n\
             budget_ms = 5000\ncommand = [\"sh\", \"-c\", \"cat >/dev/null; touch '{}'; sleep 1.4; touch '{}'; echo '{{\\\"answers\\\":{{\\\"bears_0\\\":{{\\\"noul\\\":0.5}},\\\"correction\\\":{{\\\"noul\\\":0}},\\\"choice\\\":{{\\\"noul\\\":0.5}}}}}}'\"]\n",
            started.display(),
            done.display(),
        ),
    )
    .unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    clear_runner_env(&mut cmd);
    installed.env(&mut cmd);
    cmd.args(["hook", "--prompt"])
        .current_dir(&work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &cfg)
        .env("XDG_DATA_HOME", root.path().join("share"))
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CACHE_HOME", &cache)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("PACKSET_URL", &url)
        .env("PACKSET_TIMEOUT_MS", "2000")
        .env("PACKSET_TOKEN", "test-token")
        .env("LJOS_SEAT", "inky")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"how should I tag the release?")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "ljos hook failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.contains("Tag a release"),
        "stdout={stdout:?}\nstderr={stderr:?}"
    );
    assert!(
        !done.exists(),
        "ljos hook waited for the late 1.4s judge before exiting"
    );

    let log_path: PathBuf = state.join("ljos/jev-log.jsonl");
    let cost_path: PathBuf = state.join("ljos/jev-cost.toml");
    let cache_dir: PathBuf = cache.join("ljos/jev");
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        let log_ready =
            std::fs::read_to_string(&log_path).is_ok_and(|text| text.contains("\"status\":\"ok\""));
        if log_ready && cost_path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(started.exists(), "detached judge worker never started");
    assert!(done.exists(), "detached judge worker never finished");
    assert!(
        log_path.exists(),
        "late judge reply lost after hook exit: missing {}",
        log_path.display()
    );
    assert!(
        cost_path.exists(),
        "late judge cost lost after hook exit: missing {}",
        cost_path.display()
    );
    let log_text = std::fs::read_to_string(&log_path).unwrap();
    assert!(log_text.contains("\"status\":\"ok\""), "{log_text}");
    let cost_text = std::fs::read_to_string(&cost_path).unwrap();
    assert!(cost_text.contains("-calls = 1.0"), "{cost_text}");
    let cache_entries: Vec<_> = std::fs::read_dir(&cache_dir)
        .map(|rd| rd.filter_map(Result::ok).collect())
        .unwrap_or_default();
    assert_eq!(
        cache_entries.len(),
        1,
        "expected 1 cached entry in {}",
        cache_dir.display()
    );
}

#[test]
fn fast_judge_reply_through_worker_filters_prompt_note() {
    let hits = r#"[{"id":"p1","text":"First unrelated alpha rule for something else.","score":3.0,"kind":"preference","ts":"2026-10-10T18:00:00.000Z","entities":[],"origin":"user-declared","source":{"basis":"person"},"ballots":3,"of":3},{"id":"p2","text":"Second claim release tagging needs annotated tags.","score":1.0,"kind":"preference","ts":"2026-10-10T18:00:00.000Z","entities":[],"origin":"user-declared","source":{"basis":"person"},"ballots":3,"of":3}]"#;
    let url = serve_mock_pack(hits);
    let installed = runner::install_dir();
    let root = tempfile::Builder::new()
        .prefix("jev-fast-")
        .tempdir_in(runner::non_temp_homes_root())
        .unwrap();
    let home = root.path().join("home");
    let cfg = root.path().join("cfg");
    let state = root.path().join("state");
    let cache = root.path().join("cache");
    let runtime = root.path().join("run");
    let work = root.path().join("work");
    for d in [&home, &cfg, &state, &cache, &runtime, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::create_dir_all(cfg.join("ljos")).unwrap();
    std::fs::write(
        cfg.join("ljos/jev.toml"),
        "enabled = true\nbackend = \"command\"\ncache_days = 1\nmin_words = 1\nmin_candidates = 2\n\
         budget_ms = 3000\ncommand = [\"sh\", \"-c\", \"cat >/dev/null; echo '{\\\"answers\\\":{\\\"bears_0\\\":{\\\"noul\\\":0.05},\\\"bears_1\\\":{\\\"noul\\\":0.95},\\\"correction\\\":{\\\"noul\\\":0},\\\"choice\\\":{\\\"noul\\\":0.1}}}'\"]\n",
    )
    .unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    clear_runner_env(&mut cmd);
    installed.env(&mut cmd);
    cmd.args(["hook", "--prompt"])
        .current_dir(&work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &cfg)
        .env("XDG_DATA_HOME", root.path().join("share"))
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CACHE_HOME", &cache)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("PACKSET_URL", &url)
        .env("PACKSET_TIMEOUT_MS", "2000")
        .env("PACKSET_TOKEN", "test-token")
        .env("LJOS_SEAT", "inky")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"how should I tag the release?")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "ljos hook failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Second claim"), "stdout={stdout:?}");
    assert!(
        !stdout.contains("First unrelated alpha"),
        "stdout={stdout:?}"
    );
    let log_text = std::fs::read_to_string(state.join("ljos/jev-log.jsonl")).unwrap();
    assert!(log_text.contains("\"status\":\"ok\""), "{log_text}");
}

#[test]
fn judge_worker_runs_without_ljos_in_hook_and_reads_stdin_payload() {
    let installed = runner::install_dir();
    let root = tempfile::Builder::new()
        .prefix("jev-worker-")
        .tempdir_in(runner::non_temp_homes_root())
        .unwrap();
    let home = root.path().join("home");
    let cfg = root.path().join("cfg");
    let state = root.path().join("state");
    let cache = root.path().join("cache");
    let runtime = root.path().join("run");
    let work = root.path().join("work");
    for d in [&home, &cfg, &state, &cache, &runtime, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    let state_ljos = state.join("ljos");
    let cache_jev = cache.join("ljos/jev");
    let payload = serde_json::json!({
        "cfg": {
            "enabled": true,
            "backend": "command",
            "command": [
                "sh",
                "-c",
                "cat >/dev/null; echo '{\"answers\":{\"bears_0\":{\"noul\":0.9},\"correction\":{\"noul\":0},\"choice\":{\"noul\":0.2}}}'"
            ],
            "budget_ms": 3000,
            "min_words": 1,
            "min_candidates": 1,
            "cache_days": 1
        },
        "cue": "how should I tag the release?",
        "candidates": ["Tag a release with git push origin TAG."],
        "claims": [{"id": "p1", "kind": "preference"}],
        "dirs": {
            "state": state_ljos,
            "cache": cache_jev
        }
    });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    clear_runner_env(&mut cmd);
    installed.env(&mut cmd);
    cmd.args(["hook", "--judge-worker"])
        .current_dir(&work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &cfg)
        .env("XDG_DATA_HOME", root.path().join("share"))
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CACHE_HOME", &cache)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("LJOS_SEAT", "inky")
        .env_remove("LJOS_IN_HOOK")
        .env_remove("LJOS_HOOKD")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "ljos hook --judge-worker failed ({}): stdout={stdout:?} stderr={stderr:?}",
        out.status
    );
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "worker stdout is not valid judgment JSON ({e}): stdout={stdout:?} stderr={stderr:?}"
        )
    });
    assert_eq!(parsed["bears"], serde_json::json!([0.9]), "{parsed}");
    let log_text = std::fs::read_to_string(state_ljos.join("jev-log.jsonl")).unwrap();
    assert!(log_text.contains("\"status\":\"ok\""), "{log_text}");
    let cost_text = std::fs::read_to_string(state_ljos.join("jev-cost.toml")).unwrap();
    assert!(cost_text.contains("-calls = 1.0"), "{cost_text}");
}

#[test]
fn spawned_judge_worker_clears_parent_ljos_in_hook() {
    let hits = r#"[{"id":"p1","text":"First unrelated alpha rule for something else.","score":3.0,"kind":"preference","ts":"2026-10-10T18:00:00.000Z","entities":[],"origin":"user-declared","source":{"basis":"person"},"ballots":3,"of":3},{"id":"p2","text":"Second claim release tagging needs annotated tags.","score":1.0,"kind":"preference","ts":"2026-10-10T18:00:00.000Z","entities":[],"origin":"user-declared","source":{"basis":"person"},"ballots":3,"of":3}]"#;
    let url = serve_mock_pack(hits);
    let installed = runner::install_dir();
    let root = tempfile::Builder::new()
        .prefix("jev-in-hook-")
        .tempdir_in(runner::non_temp_homes_root())
        .unwrap();
    let home = root.path().join("home");
    let cfg = root.path().join("cfg");
    let state = root.path().join("state");
    let cache = root.path().join("cache");
    let runtime = root.path().join("run");
    let work = root.path().join("work");
    for d in [&home, &cfg, &state, &cache, &runtime, &work] {
        std::fs::create_dir_all(d).unwrap();
    }
    let seen_in_hook = runtime.join("worker-in-hook");
    std::fs::create_dir_all(cfg.join("ljos")).unwrap();
    std::fs::write(
        cfg.join("ljos/jev.toml"),
        format!(
            "enabled = true\nbackend = \"command\"\ncache_days = 1\nmin_words = 1\nmin_candidates = 2\n\
             budget_ms = 3000\ncommand = [\"sh\", \"-c\", \"cat >/dev/null; printf '%s' \\\"${{LJOS_IN_HOOK-unset}}\\\" > '{}'; echo '{{\\\"answers\\\":{{\\\"bears_0\\\":{{\\\"noul\\\":0.05}},\\\"bears_1\\\":{{\\\"noul\\\":0.95}},\\\"correction\\\":{{\\\"noul\\\":0}},\\\"choice\\\":{{\\\"noul\\\":0.1}}}}}}'\"]\n",
            seen_in_hook.display()
        ),
    )
    .unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    clear_runner_env(&mut cmd);
    installed.env(&mut cmd);
    cmd.args(["hook", "--prompt"])
        .current_dir(&work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &cfg)
        .env("XDG_DATA_HOME", root.path().join("share"))
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CACHE_HOME", &cache)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("PACKSET_URL", &url)
        .env("PACKSET_TIMEOUT_MS", "2000")
        .env("PACKSET_TOKEN", "test-token")
        .env("LJOS_SEAT", "inky")
        .env("LJOS_IN_HOOK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"how should I tag the release?")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "ljos hook failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let recorded = std::fs::read_to_string(&seen_in_hook).unwrap_or_else(|e| {
        panic!("missing {}: {e}", seen_in_hook.display());
    });
    assert_eq!(
        recorded, "unset",
        "spawned judge worker inherited LJOS_IN_HOOK={recorded:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Second claim"), "stdout={stdout:?}");
}
