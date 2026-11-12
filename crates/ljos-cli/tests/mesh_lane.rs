//! `ljos mesh pull` and `ljos mesh push` over a handoff directory.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn run(root: &Path, home: &Path, pack: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.join("tmp"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("VISSUE_ROOT", root)
        .env("LJOS_SEAT", "box")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_OFF", "1")
        .env("PACKSET_URL", pack)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_TIMEOUT_MS", "2000")
        .stdin(Stdio::null())
        .output()
        .expect("ljos")
}

fn text(out: &std::process::Output) -> String {
    format!(
        "status {}\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn world() -> (tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().expect("home");
    let root = home.path().join("tracker");
    std::fs::create_dir_all(root.join(".ljos/memory/kept")).unwrap();
    std::fs::create_dir_all(home.path().join("tmp")).unwrap();
    (home, root)
}

fn write_lane(dir: &Path, lane: &str, texts: &[&str]) {
    let mut lines = vec![format!(
        r#"{{"format":"ljos-memory-1","record":"export","machine":"{lane}","seat":"{lane}"}}"#
    )];
    for lesson in texts {
        lines.push(format!(
            r#"{{"format":"ljos-memory-1","kind":"lesson","text":"{lesson}"}}"#
        ));
    }
    let mut body = lines.join("\n");
    body.push('\n');
    std::fs::write(dir.join(format!("{lane}.jsonl")), body).unwrap();
}

fn counts(stdout: &str) -> (usize, usize, usize) {
    let mut words = stdout.split_whitespace();
    assert_eq!(words.next(), Some("imported"), "{stdout}");
    let imported = words.next().unwrap().trim_end_matches(',').parse().unwrap();
    assert_eq!(words.next(), Some("unchanged"), "{stdout}");
    let unchanged = words.next().unwrap().trim_end_matches(',').parse().unwrap();
    assert_eq!(words.next(), Some("skipped"), "{stdout}");
    let skipped = words.next().unwrap().parse().unwrap();
    assert_eq!(words.next(), Some("duplicates"), "{stdout}");
    (imported, unchanged, skipped)
}

/// A writer that answers one scripted status for each `POST /v1/atoms`.
/// `retry` answers the first post with 503 and `Retry-After: 0`, then 200.
/// `deadline` answers every post with 408.
fn scripted_pack(mode: &'static str, atoms: Arc<Mutex<Vec<serde_json::Value>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let posts = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = match stream.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&tmp[..n]);
                let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
                let len = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= end + 4 + len {
                    break;
                }
            }
            let req = String::from_utf8_lossy(&buf).into_owned();
            let line = req.lines().next().unwrap_or("");
            let body = req.split_once("\r\n\r\n").map_or("", |(_, rest)| rest);
            let (status, payload, extra) = if line.starts_with("GET /v1/search") {
                let hits: Vec<serde_json::Value> = atoms
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|atom| {
                        serde_json::json!({
                            "id": atom["id"],
                            "text": atom["text"],
                            "score": 1.0,
                            "kind": atom.get("kind").cloned().unwrap_or(serde_json::json!("lesson")),
                            "entities": atom.get("entities").cloned().unwrap_or(serde_json::json!([])),
                        })
                    })
                    .collect();
                (200, serde_json::json!({"hits": hits}).to_string(), "")
            } else if line.starts_with("POST /v1/atoms") {
                let n = posts.fetch_add(1, Ordering::SeqCst);
                if mode == "deadline" || (mode == "retry" && n == 0) {
                    let code = if mode == "deadline" { 408 } else { 503 };
                    let retry = if code == 503 {
                        "Retry-After: 0\r\n"
                    } else {
                        ""
                    };
                    (code, String::new(), retry)
                } else {
                    let mut atom: serde_json::Value =
                        serde_json::from_str(body).unwrap_or(serde_json::json!({}));
                    atom["id"] = serde_json::json!("posted-atom");
                    atoms.lock().unwrap().push(atom.clone());
                    (200, atom.to_string(), "")
                }
            } else {
                (200, r#"{"hits":[],"atoms":[]}"#.to_string(), "")
            };
            let reason = match status {
                200 => "OK",
                408 => "Request Timeout",
                _ => "Service Unavailable",
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
        }
    });
    format!("http://{addr}")
}

#[test]
fn pull_dedups_and_a_second_run_skips_every_row() {
    let (home, root) = world();
    let from = root.join("handoff");
    std::fs::create_dir(&from).unwrap();
    write_lane(&from, "lane-a", &["alpha kiln", "shared kiln"]);
    write_lane(&from, "lane-b", &["shared kiln", "beta kiln"]);
    let pull = run(
        &root,
        home.path(),
        "off",
        &[
            "mesh",
            "pull",
            "--seat",
            "box",
            "--from",
            from.to_str().unwrap(),
        ],
    );
    assert!(pull.status.success(), "{}", text(&pull));
    let stdout = String::from_utf8(pull.stdout).unwrap();
    assert_eq!(counts(&stdout), (3, 0, 1), "{stdout}");
    let seed = std::fs::read(root.join(".ljos/memory/mesh/box/seed.jsonl")).unwrap();
    let again = run(
        &root,
        home.path(),
        "off",
        &[
            "mesh",
            "pull",
            "--seat",
            "box",
            "--from",
            from.to_str().unwrap(),
        ],
    );
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(counts(&String::from_utf8(again.stdout).unwrap()), (0, 4, 0));
    assert_eq!(
        std::fs::read(root.join(".ljos/memory/mesh/box/seed.jsonl")).unwrap(),
        seed
    );
    let body = String::from_utf8(seed).unwrap();
    assert!(body.contains("lane:lane-a"), "{body}");
    assert!(body.contains("lane:lane-b"), "{body}");
}

#[test]
fn push_then_pull_round_trips_a_kept_lesson() {
    let (home, root) = world();
    let row = serde_json::json!({
        "id": "local:lane",
        "text": "the lane keeps its own kiln",
        "kind": "lesson",
        "entities": ["seat:lane-a"],
        "machine": "lane-a",
        "seat": "lane-a",
    });
    std::fs::write(
        root.join(".ljos/memory/kept/lane.json"),
        serde_json::to_vec_pretty(&row).unwrap(),
    )
    .unwrap();
    let dest = root.join("handoff");
    let pushed = run(
        &root,
        home.path(),
        "off",
        &[
            "mesh",
            "push",
            "--seat",
            "lane-a",
            "--lane",
            "lane-a",
            "--to",
            dest.to_str().unwrap(),
        ],
    );
    assert!(pushed.status.success(), "{}", text(&pushed));
    let said = String::from_utf8(pushed.stdout).unwrap();
    assert!(said.starts_with("pushed 1 memories to "), "{said}");
    let file = std::fs::read_to_string(dest.join("lane-a.jsonl")).unwrap();
    assert!(file.contains("the lane keeps its own kiln"), "{file}");
    let pulled = run(
        &root,
        home.path(),
        "off",
        &[
            "mesh",
            "pull",
            "--seat",
            "box",
            "--from",
            dest.to_str().unwrap(),
        ],
    );
    assert!(pulled.status.success(), "{}", text(&pulled));
    assert_eq!(
        counts(&String::from_utf8(pulled.stdout).unwrap()),
        (1, 0, 0)
    );
}

#[test]
fn two_pulls_at_once_keep_one_copy_of_each_text() {
    let (home, root) = world();
    let from = root.join("handoff");
    std::fs::create_dir(&from).unwrap();
    write_lane(&from, "lane-a", &["alpha kiln", "shared kiln"]);
    write_lane(&from, "lane-b", &["shared kiln", "beta kiln"]);
    let args = [
        "mesh",
        "pull",
        "--seat",
        "box",
        "--from",
        from.to_str().unwrap(),
    ];
    let spawn = || {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(&root)
            .env_clear()
            .env("HOME", home.path())
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", home.path().join("tmp"))
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .env("XDG_DATA_HOME", home.path().join(".local/share"))
            .env("XDG_STATE_HOME", home.path().join(".local/state"))
            .env("XDG_CACHE_HOME", home.path().join(".cache"))
            .env("VISSUE_ROOT", &root)
            .env("LJOS_SEAT", "box")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_OFF", "1")
            .env("PACKSET_URL", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let left = spawn();
    let right = spawn();
    let left = left.wait_with_output().unwrap();
    let right = right.wait_with_output().unwrap();
    assert!(left.status.success(), "{}", text(&left));
    assert!(right.status.success(), "{}", text(&right));
    let (li, lu, ls) = counts(&String::from_utf8(left.stdout).unwrap());
    let (ri, ru, rs) = counts(&String::from_utf8(right.stdout).unwrap());
    assert_eq!(li + ri, 3, "imported {li} and {ri}");
    assert_eq!(
        lu + ru + ls + rs,
        5,
        "unchanged {lu}+{ru} skipped {ls}+{rs}"
    );
    let body = std::fs::read_to_string(root.join(".ljos/memory/mesh/box/seed.jsonl")).unwrap();
    for lesson in ["alpha kiln", "shared kiln", "beta kiln"] {
        assert_eq!(body.matches(lesson).count(), 1, "{lesson} in {body}");
    }
}

#[test]
fn a_busy_writer_is_retried_and_a_deadline_stops_the_pull() {
    let (home, root) = world();
    let from = root.join("handoff");
    std::fs::create_dir(&from).unwrap();
    write_lane(&from, "lane-a", &["alpha kiln"]);
    let atoms = Arc::new(Mutex::new(Vec::new()));
    let url = scripted_pack("retry", Arc::clone(&atoms));
    let pulled = run(
        &root,
        home.path(),
        &url,
        &[
            "mesh",
            "pull",
            "--seat",
            "box",
            "--from",
            from.to_str().unwrap(),
        ],
    );
    assert!(pulled.status.success(), "{}", text(&pulled));
    assert_eq!(
        counts(&String::from_utf8(pulled.stdout).unwrap()),
        (1, 0, 0)
    );
    let stored = atoms.lock().unwrap();
    assert_eq!(stored.len(), 1, "{stored:?}");
    let tags = stored[0]["entities"].to_string();
    assert!(tags.contains("lane:lane-a"), "{stored:?}");
    drop(stored);

    let late = root.join("late");
    std::fs::create_dir(&late).unwrap();
    write_lane(&late, "lane-c", &["gamma kiln"]);
    let missed = Arc::new(Mutex::new(Vec::new()));
    let deadline = scripted_pack("deadline", Arc::clone(&missed));
    let stopped = run(
        &root,
        home.path(),
        &deadline,
        &[
            "mesh",
            "pull",
            "--seat",
            "late",
            "--from",
            late.to_str().unwrap(),
        ],
    );
    assert_eq!(stopped.status.code(), Some(1), "{}", text(&stopped));
    let err = String::from_utf8_lossy(&stopped.stderr);
    assert!(
        err.contains("the deadline passed before this request ran"),
        "{err}"
    );
    let seed =
        std::fs::read_to_string(root.join(".ljos/memory/mesh/late/seed.jsonl")).unwrap_or_default();
    assert!(!seed.contains("gamma kiln"), "{seed}");
    assert!(missed.lock().unwrap().is_empty());
}

#[test]
fn pull_integrates_a_lane_and_a_second_pull_imports_nothing() {
    let (home, root) = world();
    let from = root.join("handoff");
    std::fs::create_dir(&from).unwrap();
    write_lane(&from, "brine", &["the brine lane keeps a zircon shelf"]);
    let atoms = Arc::new(Mutex::new(Vec::new()));
    let url = scripted_pack("ok", Arc::clone(&atoms));
    let args = [
        "mesh",
        "pull",
        "--seat",
        "box",
        "--from",
        from.to_str().unwrap(),
    ];
    let pulled = run(&root, home.path(), &url, &args);
    assert!(pulled.status.success(), "{}", text(&pulled));
    assert_eq!(
        counts(&String::from_utf8(pulled.stdout).unwrap()),
        (1, 0, 0)
    );
    let index =
        std::fs::read_to_string(root.join(".ljos/memory/imports/box/mesh/index.jsonl")).unwrap();
    let row: serde_json::Value = serde_json::from_str(index.lines().next().unwrap()).unwrap();
    let labels = row["labels"].to_string();
    assert!(labels.contains("imported"), "{row}");
    assert!(labels.contains("integrated"), "{row}");
    assert!(!labels.contains("unintegrated"), "{row}");
    assert!(row["entities"].to_string().contains("lane:brine"), "{row}");
    assert!(row["entities"].to_string().contains("seat:box"), "{row}");
    let found = run(&root, home.path(), &url, &["search", "zircon", "shelf"]);
    assert!(found.status.success(), "{}", text(&found));
    let said = text(&found);
    assert!(said.contains("zircon shelf"), "{said}");
    let again = run(&root, home.path(), &url, &args);
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(counts(&String::from_utf8(again.stdout).unwrap()), (0, 1, 0));
    assert_eq!(atoms.lock().unwrap().len(), 1, "a second pull posted again");
}

#[test]
fn help_names_pull_and_push() {
    let (home, root) = world();
    let pull = run(&root, home.path(), "off", &["mesh", "pull", "--help"]);
    assert!(pull.status.success(), "{}", text(&pull));
    let help = String::from_utf8(pull.stdout).unwrap();
    assert!(help.contains("--seat"), "{help}");
    assert!(help.contains("--from"), "{help}");
    let push = run(&root, home.path(), "off", &["mesh", "push", "--help"]);
    assert!(push.status.success(), "{}", text(&push));
    let help = String::from_utf8(push.stdout).unwrap();
    assert!(help.contains("--to"), "{help}");
    assert!(help.contains("--lane"), "{help}");
}
