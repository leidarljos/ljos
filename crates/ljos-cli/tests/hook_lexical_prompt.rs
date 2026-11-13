//! Lexical-only prompt recall. The encoder is down, so the hook ranks the
//! fixture store by words. A lesson is handed over only when it shares
//! enough distinctive terms with the prompt. The three notes that share
//! `push`, `box` and `release` stay out. The note that names `GH_RELAY_PAT`
//! is handed over.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

#[path = "common/seat_root.rs"]
mod seat_root;

struct SeatHome(PathBuf);

impl SeatHome {
    fn new(name: &str) -> Self {
        let dir = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp)
            .join(format!("ljos-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("seat home");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for SeatHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/hook_lexical_prompt.json")).expect("fixture")
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let rest = line
                .strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))?;
            rest.trim().parse().ok()
        })
        .unwrap_or(0)
}

/// A writer that answers, with the encoder recorded down, holding the
/// fixture lessons and no cosine.
fn lexical_store(lessons: Value) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let lessons = lessons.clone();
            std::thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or("/");
                let cl = content_length(&head);
                while buf.len() < head_end + cl {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let hits: Vec<Value> = lessons
                    .as_array()
                    .expect("lessons")
                    .iter()
                    .map(|lesson| {
                        json!({
                            "id": lesson["id"],
                            "text": lesson["text"],
                            "kind": "lesson",
                            "score": 1.0,
                            "ts": lesson["ts"],
                            "entities": ["horizon:standing"],
                            "ballots": 2,
                            "of": 3
                        })
                    })
                    .collect();
                let body = match path {
                    "/health" => "packsetd ok".to_string(),
                    "/v1/status" => {
                        r#"{"embedder":{"available":false,"answering":false,"enabled":true},"live":0,"live_cap":0}"#
                            .to_string()
                    }
                    "/v1/atoms" => json!({"atoms": hits}).to_string(),
                    "/v1/search" => json!({"hits": hits, "engine": "linear"}).to_string(),
                    _ => "{}".to_string(),
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    port.to_string()
}

fn executable(path: &Path, text: &str) {
    std::fs::write(path, text).expect("script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

/// `echo PROMPT | ljos hook --prompt` against the fixture store.
fn hook_argv(home: &Path, path: &str, port: &str, line: &str) -> std::process::Output {
    hook_with(home, path, port, line, &["hook"])
}

fn hook_prompt(home: &Path, path: &str, port: &str, prompt: &str) -> std::process::Output {
    hook_with(home, path, port, prompt, &["hook", "--prompt"])
}

fn hook_with(
    home: &Path,
    path: &str,
    port: &str,
    line: &str,
    args: &[&str],
) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("PATH", path)
        .env("PACKSET_PORT", port)
        .env("PACKSET_HOME", home.join("pack"))
        .env("PACKSET_TOKEN", "fixture-token-fixture-token-fixture")
        .env("PACKSET_TIMEOUT_MS", "2000")
        .env("LJOS_NO_AUTOSTART", "1")
        .env("LJOS_TRACKER_GIT", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("ljos");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(line.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait")
}

#[test]
fn a_lexical_prompt_keeps_the_distinctive_lesson_and_drops_the_generic_ones() {
    let fixture = fixture();
    let home = SeatHome::new("hook-lexical-prompt");
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    executable(
        &bin.join("vissue"),
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'vissue 0.23.0' ;;\n  *) echo '{}' ;;\nesac\nexit 0\n",
    );
    executable(&bin.join("packset"), "#!/bin/sh\nexit 0\n");
    let port = lexical_store(fixture["lessons"].clone());
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let prompt = fixture["prompt"].as_str().expect("prompt");
    let out = hook_prompt(home.path(), &path, &port, prompt);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "the label was a refusal\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let show = fixture["show"].as_str().expect("show");
    assert!(
        stdout.contains(show),
        "the related lesson was not injected\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    for hidden in fixture["hide"].as_array().expect("hide") {
        let text = hidden.as_str().expect("hide text");
        assert!(
            !stdout.contains(text),
            "an unrelated lesson was injected: {text}\nstdout:\n{stdout}"
        );
    }
    assert_eq!(
        stderr.matches("label prompt-origin:").count(),
        1,
        "the label should be printed once\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("not stored as the person's words"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("not a known runner"), "stderr:\n{stderr}");
    assert!(
        !stderr.contains("not a runner's own program"),
        "stderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("label prompt-origin:"),
        "the label belongs after the lessons, not in them\nstdout:\n{stdout}"
    );
    let lesson_at = stdout.find(show).expect("lesson");
    let combined = format!("{stdout}{stderr}");
    let label_at = combined.find("label prompt-origin:").expect("label");
    assert!(
        lesson_at < label_at,
        "lessons should stay first\n{combined}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("refus"),
        "stderr:\n{stderr}"
    );
}

/// A bare command on stdin is an argv. The guard is unchanged, and the
/// lessons that bear on the line are printed, as they are for a prompt.
#[test]
fn a_bare_command_on_stdin_prints_the_lesson_it_bears_on() {
    let fixture = fixture();
    let home = SeatHome::new("hook-argv-recall");
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    executable(
        &bin.join("vissue"),
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'vissue 0.23.0' ;;\n  *) echo '{}' ;;\nesac\nexit 0\n",
    );
    executable(&bin.join("packset"), "#!/bin/sh\nexit 0\n");
    let port = lexical_store(fixture["lessons"].clone());
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let line = fixture["prompt"].as_str().expect("prompt");
    let out = hook_argv(home.path(), &path, &port, line);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    let show = fixture["show"].as_str().expect("show");
    assert!(
        stdout.contains(show),
        "the related lesson was not printed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    for hidden in fixture["hide"].as_array().expect("hide") {
        let text = hidden.as_str().expect("hide text");
        assert!(
            !stdout.contains(text),
            "an unrelated lesson was printed: {text}\nstdout:\n{stdout}"
        );
    }
}
