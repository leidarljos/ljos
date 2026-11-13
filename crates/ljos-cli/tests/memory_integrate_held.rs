//! `ljos memory integrate --dry-run` names a text the pack already holds
//! the way an apply that posts nothing does.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

struct Pack {
    url: String,
    posts: Arc<AtomicUsize>,
}

fn pack(text: &str) -> Pack {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let posts = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&posts);
    let text = text.to_string();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let counted = Arc::clone(&counted);
            let text = text.clone();
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
                let cl = content_length(&head);
                while buf.len() < head_end + cl {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or("/");
                let held = serde_json::json!({
                    "id": "atom-kiln",
                    "kind": "lesson",
                    "text": text,
                    "origin": "agent-derived",
                    "level": "explicit",
                    "entities": ["seat:ada", "horizon:standing"],
                    "source": {"seat": "ada", "basis": "agent"},
                });
                let body = if head.starts_with("POST") {
                    counted.fetch_add(1, Ordering::SeqCst);
                    let raw = String::from_utf8_lossy(&buf[head_end..]);
                    let mut atom: serde_json::Value =
                        serde_json::from_str(&raw).unwrap_or(held.clone());
                    if atom
                        .get("id")
                        .and_then(|id| id.as_str())
                        .unwrap_or("")
                        .is_empty()
                    {
                        atom["id"] = serde_json::Value::String("atom-posted".into());
                    }
                    atom.to_string()
                } else if path == "/v1/atoms" {
                    serde_json::json!({"atoms": [held]}).to_string()
                } else {
                    "{}".to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    Pack {
        url: format!("http://127.0.0.1:{port}"),
        posts,
    }
}

fn ljos(home: &Path, root: &Path, url: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("TMPDIR", home.join("tmp"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("VISSUE_ROOT", root)
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_URL", url)
        .env("PACKSET_WORKSPACE", "seat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("ljos")
}

fn said(out: &std::process::Output) -> String {
    format!(
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_dry_run_of_texts_the_pack_holds_says_held() {
    let text = "The kiln fires at dusk on the north bench.";
    let pack = pack(text);
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("tracker");
    for dir in [".config", ".local/state", ".local/share", "run", "tmp"] {
        std::fs::create_dir_all(home.path().join(dir)).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("mem.jsonl");
    let exported = ljos(
        home.path(),
        &root,
        &pack.url,
        &["memory", "export", "--out", file.to_str().unwrap()],
    );
    assert!(exported.status.success(), "{}", said(&exported));
    assert!(
        std::fs::read_to_string(&file).unwrap().contains(text),
        "the export left the pack text out"
    );
    let imported = ljos(
        home.path(),
        &root,
        &pack.url,
        &["memory", "import", file.to_str().unwrap()],
    );
    assert!(imported.status.success(), "{}", said(&imported));
    assert_eq!(pack.posts.load(Ordering::SeqCst), 0, "import posted a copy");

    let dry = ljos(
        home.path(),
        &root,
        &pack.url,
        &["memory", "integrate", "--all", "--dry-run"],
    );
    let report = String::from_utf8_lossy(&dry.stdout);
    assert!(dry.status.success(), "{}", said(&dry));
    assert!(report.contains("held"), "{report}");
    assert!(report.contains("already in pack"), "{report}");
    assert!(
        !report.contains(" new"),
        "a text the pack holds was labelled new:\n{report}"
    );
    assert!(report.contains(text), "{report}");
    assert_eq!(
        pack.posts.load(Ordering::SeqCst),
        0,
        "a dry run posted a copy"
    );

    let apply = ljos(
        home.path(),
        &root,
        &pack.url,
        &["memory", "integrate", "--all"],
    );
    assert!(apply.status.success(), "{}", said(&apply));
    assert_eq!(
        pack.posts.load(Ordering::SeqCst),
        0,
        "apply posted a text the pack already holds\n{}",
        said(&apply)
    );
    let applied = String::from_utf8_lossy(&apply.stdout);
    assert!(
        applied.contains("held") && applied.contains("already in pack"),
        "{applied}"
    );
}
