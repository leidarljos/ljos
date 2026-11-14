//! A writer whose encoder child has died: doctor warns, and recall asks
//! `packset` to stop and start the writer again.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

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

/// A writer that answers, with the encoder child recorded as dead.
fn dead_encoder() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
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
                let body = match path {
                    "/health" => "packsetd ok".to_string(),
                    "/v1/status" => {
                        r#"{"embedder":{"available":false,"answering":false,"enabled":true,"binary":"/usr/bin/packset-embed"},"live":0,"live_cap":0}"#
                            .to_string()
                    }
                    "/v1/atoms" => r#"{"atoms":[]}"#.to_string(),
                    "/v1/search" => r#"{"hits":[],"engine":"linear"}"#.to_string(),
                    _ => r#"{}"#.to_string(),
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

fn run(home: &Path, path: &str, port: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ljos"))
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
        .env("PACKSET_TIMEOUT_MS", "2000")
        .env("LJOS_NO_AUTOSTART", "1")
        .output()
        .expect("ljos")
}

#[test]
fn a_missing_encoder_warns_and_recall_restarts_it() {
    let home = SeatHome::new("encoder-degraded");
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let marker = home.path().join("packset-calls");
    executable(
        &bin.join("packset"),
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\n", marker.display()),
    );
    executable(
        &bin.join("vissue"),
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'vissue 0.23.0' ;;\n  recall) echo recalled ;;\n  vote) echo '--used --confidence' ;;\n  *) echo '{}' ;;\nesac\nexit 0\n",
    );
    let port = dead_encoder();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let doctor = run(home.path(), &path, &port, &["doctor"]);
    let text = String::from_utf8_lossy(&doctor.stdout);
    let err = String::from_utf8_lossy(&doctor.stderr);
    assert!(
        text.contains("warn\tencoder\t"),
        "doctor did not warn\nstdout:\n{text}\nstderr:\n{err}\nstatus: {}",
        doctor.status
    );
    assert!(
        text.contains(
            "matched by words: the encoder is not installed; `ljos doctor --all` for why"
        ),
        "{text}"
    );
    assert!(text.contains(ljos_cli::ENCODER_RESTART_COMMAND), "{text}");
    assert!(
        text.contains("Search matches words"),
        "the warning does not say the search matches words: {text}"
    );
    let before = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(
        !before.lines().any(|line| line.trim() == "stop"),
        "doctor stopped the writer: {before}"
    );

    let recall = run(home.path(), &path, &port, &["recall", "Scratch-1"]);
    let calls = std::fs::read_to_string(&marker).unwrap_or_default();
    let said = String::from_utf8_lossy(&recall.stdout);
    let recall_err = String::from_utf8_lossy(&recall.stderr);
    assert!(
        calls.lines().any(|line| line.trim() == "stop")
            && calls.lines().any(|line| line.trim() == "ensure"),
        "recall did not run `{cmd}`; calls: {calls:?}\nstdout:\n{said}\nstderr:\n{recall_err}",
        cmd = ljos_cli::ENCODER_RESTART_COMMAND
    );
    assert!(
        !said.contains("matched by words"),
        "the mode line belongs on stderr\nstdout:\n{said}"
    );
    assert!(
        recall_err.contains("matched by words: the encoder is not installed"),
        "recall did not name the ranking\nstderr:\n{recall_err}\nstdout:\n{said}"
    );

    let search = run(home.path(), &path, &port, &["search", "fuse"]);
    let search_out = String::from_utf8_lossy(&search.stdout);
    let search_err = String::from_utf8_lossy(&search.stderr);
    assert!(
        search_err.contains("matched by words: the encoder is not installed"),
        "search did not name the ranking\nstderr:\n{search_err}\nstdout:\n{search_out}"
    );
    assert!(
        !search_out.contains("matched by words"),
        "search put the mode on stdout:\n{search_out}"
    );

    let as_json = run(home.path(), &path, &port, &["search", "--json", "fuse"]);
    let json_out = String::from_utf8_lossy(&as_json.stdout);
    let json_err = String::from_utf8_lossy(&as_json.stderr);
    assert!(
        !json_err.contains("matched by words"),
        "json search still printed the mode line:\n{json_err}"
    );
    let doc: serde_json::Value = serde_json::from_str(json_out.trim())
        .unwrap_or_else(|err| panic!("search json did not parse ({err}): {json_out}"));
    assert_eq!(doc["ranking"], "lexical", "{doc}");
    assert_eq!(doc["reason"], "encoder unavailable", "{doc}");

    let recall_json = run(
        home.path(),
        &path,
        &port,
        &["recall", "--json", "Scratch-1"],
    );
    let recall_body = String::from_utf8_lossy(&recall_json.stdout);
    let recall_json_err = String::from_utf8_lossy(&recall_json.stderr);
    assert!(
        !recall_json_err.contains("matched by words"),
        "json recall still printed the mode line:\n{recall_json_err}"
    );
    let recalled: serde_json::Value = serde_json::from_str(recall_body.trim())
        .unwrap_or_else(|err| panic!("recall json did not parse ({err}): {recall_body}"));
    assert_eq!(recalled["ranking"], "lexical", "{recalled}");
    assert_eq!(recalled["reason"], "encoder unavailable", "{recalled}");
}

/// `packset-embed` exits with a loader line. Doctor must not read
/// `GLIBCXX_3.4.34` as the crate version.
#[test]
fn a_loader_error_is_not_the_encoder_version() {
    let (home, path, port) = failing_encoder_seat("encoder-loader");
    let doctor = run(home.path(), &path, &port, &["doctor", "--all"]);
    let text = String::from_utf8_lossy(&doctor.stdout);
    let err = String::from_utf8_lossy(&doctor.stderr);
    assert!(
        doctor.status.success(),
        "a broken encoder must not fail doctor\nstdout:\n{text}\nstderr:\n{err}"
    );
    let row = text
        .lines()
        .find(|line| line.split('\t').nth(1) == Some("packset-embed"))
        .unwrap_or_else(|| panic!("no packset-embed row\n{text}"));
    let mut cols = row.splitn(3, '\t');
    let word = cols.next().unwrap_or("");
    let _name = cols.next();
    let state = cols.next().unwrap_or("");
    assert_eq!(word, "warn", "{row}");
    assert!(
        state.starts_with("not runnable: \""),
        "the encoder was not reported as not runnable: {state}"
    );
    assert!(
        state.contains("version GLIBCXX_3.4.34 not found"),
        "the first error line was not quoted: {state}"
    );
    assert!(
        !state.contains("crates.io") && !state.contains("ahead"),
        "the loader token was read as a version: {state}"
    );
    assert!(
        !text.contains("ahead of crates.io"),
        "doctor compared a loader token with crates.io\n{text}"
    );
}

fn failing_encoder_seat(name: &str) -> (SeatHome, String, String) {
    let home = SeatHome::new(name);
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let stub = r#"#!/bin/sh
name=$(basename "$0")
if [ "$1" = --version ]; then
  echo "$name 99.0.0"
fi
exit 0
"#;
    for bin_name in [
        "ljos",
        "ljos-mcp",
        "ljos-policyd",
        "deedar",
        "claimdag",
        "packset",
        "packsetd",
    ] {
        executable(&bin.join(bin_name), stub);
    }
    executable(&bin.join("curl"), "#!/bin/sh\nexit 1\n");
    executable(
        &bin.join("vissue"),
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'vissue 99.0.0' ;;\n  recall) echo recalled ;;\n  vote) echo '--used --confidence' ;;\n  identity) echo 'vissue 99.0.0 root=/t' ;;\n  *) echo ok ;;\nesac\nexit 0\n",
    );
    // Stdout is version-shaped on purpose. A parser that ignores the exit
    // status, or that reads the loader line, reports 3.4.34.
    executable(
        &bin.join("packset-embed"),
        "#!/bin/sh\nprintf '%s\\n' 'packset-embed 3.4.34'\nprintf '%s\\n' 'version GLIBCXX_3.4.34 not found (required by packset-embed)' >&2\nexit 1\n",
    );
    let port = dead_encoder();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    (home, path, port)
}
