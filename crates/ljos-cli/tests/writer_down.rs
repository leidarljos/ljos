//! A down pack writer names what is down and the one command that fixes it.
//!
//! The cases are no writer, a writer on another port, a writer for a
//! different home, and packset missing from PATH. Remember, search, recall,
//! the prompt hook and every MCP tool are walked. A lesson queued while the
//! writer is down is said to be queued and is sent on the next run.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[path = "common/seat_root.rs"]
mod seat_root;

const LESSON: &str = "The kiln fires at dusk on the north bench.";

const STANDIN_C: &str = r#"
#include <netinet/in.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
int main(int argc, char **argv) {
    if (argc < 2) return 2;
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return 1;
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof addr);
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    addr.sin_port = htons(0);
    if (bind(fd, (struct sockaddr *)&addr, sizeof addr) != 0) return 1;
    if (listen(fd, 16) != 0) return 1;
    struct sockaddr_in bound;
    socklen_t len = sizeof bound;
    if (getsockname(fd, (struct sockaddr *)&bound, &len) != 0) return 1;
    FILE *out = fopen(argv[1], "w");
    if (!out) return 1;
    fprintf(out, "%u\n", (unsigned) ntohs(bound.sin_port));
    fclose(out);
    for (;;) pause();
}
"#;

struct Seat {
    root: PathBuf,
}

impl Seat {
    fn new(name: &str) -> Self {
        let root = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp).join(format!(
            "ljos-down-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&root);
        for sub in [
            "home", "bin", "bare", "state", "config", "data", "run", "tmp", "other",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("bin/packset"), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(root.join("bin/vissue"), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(root.join("bare/vissue"), "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in ["bin/packset", "bin/vissue", "bare/vissue"] {
                std::fs::set_permissions(root.join(path), std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
        }
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn other(&self) -> PathBuf {
        self.root.join("other")
    }

    fn path_with_packset(&self) -> String {
        format!("{}:/usr/bin:/bin", self.root.join("bin").display())
    }

    fn path_without_packset(&self) -> String {
        format!("{}:/usr/bin:/bin", self.root.join("bare").display())
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn dead_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

fn standin_bin(root: &Path) -> PathBuf {
    let src = root.join("standin.c");
    let bin = root.join("packsetd");
    if bin.is_file() {
        return bin;
    }
    std::fs::write(&src, STANDIN_C).unwrap();
    let out = Command::new("cc")
        .args(["-O2", "-o"])
        .arg(&bin)
        .arg(&src)
        .output()
        .expect("cc");
    assert!(
        out.status.success(),
        "cc standin: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    bin
}

struct Writer {
    child: std::process::Child,
    port: u16,
}

impl Writer {
    fn spawn(bin: &Path, home: &Path, root: &Path) -> Self {
        let port_file = root.join(format!("port-{}", std::process::id()));
        let _ = std::fs::remove_file(&port_file);
        let child = Command::new(bin)
            .arg(&port_file)
            .env_clear()
            .env("HOME", home)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("standin");
        let start = std::time::Instant::now();
        let port = loop {
            if let Ok(text) = std::fs::read_to_string(&port_file) {
                if let Ok(port) = text.trim().parse::<u16>() {
                    if port != 0 {
                        break port;
                    }
                }
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "standin did not write a port"
            );
            thread::sleep(Duration::from_millis(20));
        };
        Self { child, port }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
struct Case {
    name: &'static str,
    path: String,
    phrase: String,
    command: &'static str,
    retries: bool,
}

fn ljos(seat: &Seat, path: &str, state: &Path, port: u16, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .env_clear()
        .env("HOME", seat.home())
        .env("PATH", path)
        .env("USER", "ada")
        .env("LOGNAME", "ada")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_PORT", port.to_string())
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "400")
        .env("LJOS_PACK_TIMEOUT_MS", "400")
        .env("XDG_STATE_HOME", state)
        .env("XDG_CONFIG_HOME", seat.root.join("config"))
        .env("XDG_DATA_HOME", seat.root.join("data"))
        .env("XDG_RUNTIME_DIR", seat.root.join("run"))
        .env("TMPDIR", seat.root.join("tmp"))
        .env("VISSUE_ROOT", seat.root.join("tracker"))
        .env("VISSUE_NO_ROUTE", "1")
        .current_dir(seat.home())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn text_of(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn stderr_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

fn assert_fault(case: &Case, what: &str, out: &Output) {
    let said = text_of(out);
    let lines = stderr_lines(out);
    assert_eq!(lines.len(), 1, "{what} {case:?} was not one line:\n{said}");
    let line = &lines[0];
    assert!(
        line.contains(&case.phrase),
        "{what} did not say what is down ({})\n{said}",
        case.phrase
    );
    assert!(
        line.contains(case.command),
        "{what} did not name {}\n{said}",
        case.command
    );
    assert!(
        !line.contains("packsetd is unreachable"),
        "{what} kept the old line:\n{said}"
    );
    assert!(!line.contains('\n'), "{what} {line}");
    let code = if case.retries { 75 } else { 1 };
    assert_eq!(out.status.code(), Some(code), "{what}\n{said}");
    if case.retries {
        assert!(
            !line.contains("cargo install"),
            "{what} named two commands:\n{said}"
        );
    } else {
        assert!(
            !line.contains("packset ensure"),
            "{what} named two commands:\n{said}"
        );
    }
}

fn cases(seat: &Seat, dead: u16, other_port: u16, other_home: &Path) -> Vec<Case> {
    vec![
        Case {
            name: "no writer",
            path: seat.path_with_packset(),
            phrase: format!("no pack writer is answering on 127.0.0.1:{dead}"),
            command: "packset ensure",
            retries: true,
        },
        Case {
            name: "other port",
            path: seat.path_with_packset(),
            phrase: format!("listening on 127.0.0.1:{other_port}, not 127.0.0.1:{dead}"),
            command: "packset ensure",
            retries: true,
        },
        Case {
            name: "other home",
            path: seat.path_with_packset(),
            phrase: format!(
                "running for home {}, not {}",
                other_home.display(),
                seat.home().display()
            ),
            command: "packset ensure",
            retries: true,
        },
        Case {
            name: "packset missing",
            path: seat.path_without_packset(),
            phrase: "packset is not on PATH".into(),
            command: "cargo install --locked packset",
            retries: false,
        },
    ]
}

fn walk_commands(seat: &Seat, case: &Case, dead: u16) {
    let state = seat
        .root
        .join(format!("state-{}", case.name.replace(' ', "-")));
    std::fs::create_dir_all(&state).unwrap();
    let search = ljos(seat, &case.path, &state, dead, &["search", "kiln fire"])
        .output()
        .unwrap();
    assert_fault(case, "search", &search);

    let recall = ljos(seat, &case.path, &state, dead, &["recall", "probe-1a2b"])
        .output()
        .unwrap();
    assert_fault(case, "recall", &recall);

    let payload = state.join("prompt.json");
    std::fs::write(
        &payload,
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"down-1","prompt":"where does the kiln fire at dusk"}"#,
    )
    .unwrap();
    let script = format!(
        "{} hook --prompt --text < {}",
        env!("CARGO_BIN_EXE_ljos"),
        payload.display()
    );
    let hook = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .env_clear()
        .env("HOME", seat.home())
        .env("PATH", &case.path)
        .env("USER", "ada")
        .env("LOGNAME", "ada")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_PORT", dead.to_string())
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "400")
        .env("LJOS_PACK_TIMEOUT_MS", "400")
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CONFIG_HOME", seat.root.join("config"))
        .env("XDG_DATA_HOME", seat.root.join("data"))
        .env(
            "XDG_RUNTIME_DIR",
            seat.root
                .join(format!("run-{}", case.name.replace(' ', "-"))),
        )
        .env("TMPDIR", seat.root.join("tmp"))
        .env("VISSUE_ROOT", seat.root.join("tracker"))
        .env("VISSUE_NO_ROUTE", "1")
        .current_dir(seat.home())
        .output()
        .unwrap();
    let said = text_of(&hook);
    assert!(hook.status.success(), "hook\n{said}");
    let joined = format!(
        "{}{}",
        String::from_utf8_lossy(&hook.stdout),
        String::from_utf8_lossy(&hook.stderr)
    );
    assert!(
        joined.contains(&case.phrase) && joined.contains(case.command),
        "hook did not name the fix\n{said}"
    );
    assert!(!joined.contains("packsetd is unreachable"), "hook\n{said}");

    let remember = ljos(seat, &case.path, &state, dead, &["remember", LESSON])
        .output()
        .unwrap();
    assert_fault(case, "remember", &remember);
    let line = stderr_lines(&remember).remove(0);
    assert!(
        line.contains("1 lesson is queued for the next send"),
        "remember did not say the lesson is queued:\n{line}"
    );
    let queued = std::fs::read_to_string(state.join("ljos/note-queue.jsonl")).unwrap_or_default();
    assert!(
        queued.contains(LESSON),
        "the lesson was not queued:\n{queued}"
    );
}

fn mcp_text(value: &serde_json::Value) -> String {
    value["result"]["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| value.to_string())
}

fn walk_mcp(seat: &Seat, case: &Case, dead: u16) {
    let state = seat
        .root
        .join(format!("mcp-{}", case.name.replace(' ', "-")));
    std::fs::create_dir_all(&state).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env("HOME", seat.home())
        .env("PATH", &case.path)
        .env("USER", "ada")
        .env("LOGNAME", "ada")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_MCP_TOOLS", "all")
        .env("PACKSET_PORT", dead.to_string())
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "400")
        .env("LJOS_PACK_TIMEOUT_MS", "400")
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CONFIG_HOME", seat.root.join("config"))
        .env("XDG_DATA_HOME", seat.root.join("data"))
        .env("XDG_RUNTIME_DIR", seat.root.join("run"))
        .env("TMPDIR", seat.root.join("tmp"))
        .env("VISSUE_ROOT", seat.root.join("tracker"))
        .env("VISSUE_NO_ROUTE", "1")
        .current_dir(seat.home())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(90));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let args = serde_json::json!({
        "text": LESSON,
        "query": "kiln fire",
        "issue": "probe-1a2b",
        "id": "atom-kept",
        "pattern": "git push --force*",
        "verdict": "deny",
        "why": "a force push rewrites a published branch",
        "name": "kiln",
        "to": "bee"
    });
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"walk","version":"1"}
        }})
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})
    )
    .unwrap();
    // The list answer is not in hand yet. Ask the memory tools by name,
    // then every name the list returns is checked from the same process
    // after a second session below.
    for (id, name) in [
        (3_u64, "ljos_remember"),
        (4, "ljos_search"),
        (5, "ljos_recall"),
        (6, "ljos_prefer"),
    ] {
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
                "name": name, "arguments": args
            }})
        )
        .unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "mcp {}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        case.name
    );
    let answers: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let by_id = |id: u64| {
        answers
            .iter()
            .find(|answer| answer["id"] == id)
            .unwrap_or_else(|| panic!("no mcp answer {id} for {}\n{stdout}\n{stderr}", case.name))
    };
    let listed = by_id(2)["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("no tool list for {}\n{stdout}", case.name));
    assert!(listed.len() > 4, "tool list for {}: {listed:?}", case.name);
    for (id, name) in [
        (3, "ljos_remember"),
        (4, "ljos_search"),
        (5, "ljos_recall"),
        (6, "ljos_prefer"),
    ] {
        let call = by_id(id);
        let said = mcp_text(call);
        assert!(
            said.contains(&case.phrase) && said.contains(case.command),
            "{name} {}: {said}",
            case.name
        );
        assert!(!said.contains("packsetd is unreachable"), "{name}: {said}");
        let first = call["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(
            !first.contains('\n'),
            "{name} was more than one line: {first}"
        );
        if name == "ljos_remember" {
            assert!(
                first.contains("1 lesson is queued for the next send"),
                "{name}: {first}"
            );
        }
    }
    let queued = std::fs::read_to_string(state.join("ljos/note-queue.jsonl")).unwrap_or_default();
    assert!(
        queued.contains(LESSON),
        "mcp remember did not queue the lesson:\n{queued}"
    );

    // A second server calls every listed tool. The queue is already full,
    // so a pack failure still has to name the same command.
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .env("HOME", seat.home())
        .env("PATH", &case.path)
        .env("USER", "ada")
        .env("LOGNAME", "ada")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_MCP_TOOLS", "all")
        .env("PACKSET_PORT", dead.to_string())
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_TIMEOUT_MS", "400")
        .env("LJOS_PACK_TIMEOUT_MS", "400")
        .env("XDG_STATE_HOME", &state)
        .env("XDG_CONFIG_HOME", seat.root.join("config"))
        .env("XDG_DATA_HOME", seat.root.join("data"))
        .env("XDG_RUNTIME_DIR", seat.root.join("run"))
        .env("TMPDIR", seat.root.join("tmp"))
        .env("VISSUE_ROOT", seat.root.join("tracker"))
        .env("VISSUE_NO_ROUTE", "1")
        .current_dir(seat.home())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(120));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"walk","version":"1"}
        }})
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    for (n, tool) in listed.iter().enumerate() {
        let name = tool["name"].as_str().unwrap_or("");
        let id = n as u64 + 10;
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
                "name": name, "arguments": args
            }})
        )
        .unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "mcp all {}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        case.name
    );
    assert!(
        !stdout.contains("packsetd is unreachable") && !stderr.contains("packsetd is unreachable"),
        "old line in {}\n{stdout}\n{stderr}",
        case.name
    );
    let answers: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    assert!(
        answers.len() >= listed.len(),
        "mcp {} answered {} of {} tools\n{stdout}",
        case.name,
        answers.len(),
        listed.len()
    );
    for answer in &answers {
        let said = mcp_text(answer);
        if said.to_ascii_lowercase().contains("connection refused") {
            assert!(
                said.contains(case.command),
                "tool left connection refused without the command:\n{said}"
            );
        }
    }
}

fn serve_pack() -> (u16, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(String::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf);
                            let len = head.lines().find_map(|line| {
                                line.split_once(':')
                                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                            });
                            let have = buf.len() - (at + 4);
                            if len.is_none_or(|len| have >= len) {
                                break;
                            }
                        }
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf);
            log.lock().unwrap().push_str(&req);
            let body = if req.contains("POST /v1/atoms") {
                r#"{"id":"atom-kept","kind":"lesson","text":"kept"}"#
            } else if req.contains("/v1/search") {
                r#"{"hits":[]}"#
            } else if req.contains("/v1/atoms") {
                r#"{"atoms":[]}"#
            } else {
                "{}"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    (port, seen)
}

#[test]
fn every_surface_names_the_down_writer_and_one_command() {
    let seat = Seat::new("walk");
    let dead = dead_port();
    let bin = standin_bin(&seat.root);
    let same = Writer::spawn(&bin, &seat.home(), &seat.root);
    let other = Writer::spawn(&bin, &seat.other(), &seat.root);
    let cases = cases(&seat, dead, same.port, &seat.other());
    // The two writers stay up for the whole walk. The "no writer" and
    // "missing" cases still see them, so those phrases have to win on
    // their own facts: no writer means we point at `dead` and the same-home
    // writer is the other port. Run "no writer" with no standin of this home.
    drop(same);
    drop(other);
    let none = &cases[0];
    walk_commands(&seat, none, dead);
    walk_mcp(&seat, none, dead);

    let same = Writer::spawn(&bin, &seat.home(), &seat.root);
    let other_only = &cases[1];
    // Rebuild the phrase with the port this writer actually bound.
    let other_port = Case {
        phrase: format!("listening on 127.0.0.1:{}, not 127.0.0.1:{dead}", same.port),
        ..other_only.clone_case()
    };
    walk_commands(&seat, &other_port, dead);
    walk_mcp(&seat, &other_port, dead);
    drop(same);

    let other = Writer::spawn(&bin, &seat.other(), &seat.root);
    let home_case = &cases[2];
    walk_commands(&seat, home_case, dead);
    walk_mcp(&seat, home_case, dead);
    drop(other);

    let missing = &cases[3];
    walk_commands(&seat, missing, dead);
    walk_mcp(&seat, missing, dead);
}

#[test]
fn a_queued_lesson_reaches_the_pack_on_the_next_run() {
    let seat = Seat::new("again");
    let dead = dead_port();
    let state = seat.root.join("state");
    let remember = ljos(
        &seat,
        &seat.path_with_packset(),
        &state,
        dead,
        &["remember", LESSON],
    )
    .output()
    .unwrap();
    assert_fault(
        &Case {
            name: "queue",
            path: seat.path_with_packset(),
            phrase: format!("no pack writer is answering on 127.0.0.1:{dead}"),
            command: "packset ensure",
            retries: true,
        },
        "remember",
        &remember,
    );
    let queue = state.join("ljos/note-queue.jsonl");
    let queued = std::fs::read_to_string(&queue).unwrap_or_default();
    assert!(queued.contains(LESSON), "{queued}");

    let (port, seen) = serve_pack();
    let url = format!("http://127.0.0.1:{port}");
    let mut search = ljos(
        &seat,
        &seat.path_with_packset(),
        &state,
        dead,
        &["search", "kiln"],
    );
    search.env("PACKSET_URL", &url);
    search.env_remove("PACKSET_PORT");
    let search = search.output().unwrap();
    let said = text_of(&search);
    let body = seen.lock().unwrap().clone();
    assert!(
        body.contains(LESSON),
        "the next run did not send the lesson\n{body}\n{said}"
    );
    let left = std::fs::read_to_string(&queue).unwrap_or_default();
    assert!(
        !left.contains(LESSON),
        "the lesson stayed queued\n{left}\n{said}"
    );
}

#[test]
fn mesh_push_names_a_queued_lesson_instead_of_zero_memories() {
    let seat = Seat::new("mesh");
    let state = seat.root.join("state");
    std::fs::create_dir_all(state.join("ljos")).unwrap();
    std::fs::create_dir_all(seat.root.join("tracker/.ljos/memory")).unwrap();
    let row = serde_json::json!({
        "issue": "",
        "text": LESSON,
        "workspace": "seat",
        "at": 1,
        "kind": "lesson",
        "seat": "ada"
    });
    std::fs::write(state.join("ljos/note-queue.jsonl"), format!("{row}\n")).unwrap();
    let dest = seat.root.join("handoff");
    let mut cmd = ljos(
        &seat,
        &seat.path_without_packset(),
        &state,
        dead_port(),
        &[
            "mesh",
            "push",
            "--seat",
            "ada",
            "--to",
            dest.to_str().unwrap(),
        ],
    );
    cmd.env("LJOS_OFF", "1");
    let out = cmd.output().unwrap();
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("1 lesson queued, not pushed"), "{said}");
    assert!(!stdout.contains("pushed 0 memories"), "{said}");
}

impl Case {
    fn clone_case(&self) -> Self {
        Self {
            name: self.name,
            path: self.path.clone(),
            phrase: self.phrase.clone(),
            command: self.command,
            retries: self.retries,
        }
    }
}
