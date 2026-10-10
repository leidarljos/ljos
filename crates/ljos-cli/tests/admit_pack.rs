//! Accept of an agent lesson against packsetd built from develop.
//!
//! Filing posts the atom. The writer holds it. Accept stores that atom,
//! records the acceptance, and a second filing is the same open proposal.
//! `remember` of the same text leaves one live atom and no open proposal.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

struct Daemon {
    child: Child,
    url: String,
    home: tempfile::TempDir,
    log: Arc<Mutex<String>>,
}

impl Daemon {
    fn start() -> Self {
        let bin = packsetd_bin();
        let home = tempfile::tempdir().expect("pack home");
        let port = free_port();
        let log = Arc::new(Mutex::new(String::new()));
        let captured = Arc::clone(&log);
        let mut child = Command::new(&bin)
            .args([
                "--port",
                &port.to_string(),
                "--home",
                home.path().to_str().expect("home is utf-8"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|err| panic!("spawn {}: {err}", bin.display()));
        let stderr = child.stderr.take().expect("stderr");
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match std::io::BufRead::read_line(&mut reader, &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => captured.lock().expect("log").push_str(&line),
                }
            }
        });
        let url = format!("http://127.0.0.1:{port}");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.try_wait().expect("packsetd status") {
                panic!("packsetd exited {status}: {}", log.lock().expect("log"));
            }
            if http_get(&url, "/health")
                .is_ok_and(|(code, body)| code == 200 && body.contains("ok"))
            {
                break;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!(
                    "packsetd did not answer /health: {}",
                    log.lock().expect("log")
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Self {
            child,
            url,
            home,
            log,
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn packsetd_bin() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("packsetd");
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("packsetd is not on PATH; CI builds it from packset develop");
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("free port");
    listener.local_addr().expect("addr").port()
}

fn http_get(base: &str, path: &str) -> Result<(u16, String), String> {
    let addr = base
        .strip_prefix("http://")
        .ok_or_else(|| format!("not an http url: {base}"))?;
    let mut stream = std::net::TcpStream::connect(addr).map_err(|err| err.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| err.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| err.to_string())?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|err| err.to_string())?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|err| err.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("no http body: {text}"))?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("no status: {head}"))?;
    Ok((status, body.to_string()))
}

fn get_json(base: &str, path: &str) -> Value {
    let (code, body) = http_get(base, path).unwrap_or_else(|err| panic!("{path}: {err}"));
    assert_eq!(code, 200, "{path}: {body}");
    serde_json::from_str(&body).unwrap_or_else(|err| panic!("{path}: {err}: {body}"))
}

fn open_proposals(base: &str) -> Vec<Value> {
    get_json(base, "/v1/proposals?workspace=seat")["proposals"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn live_atoms(base: &str) -> Vec<Value> {
    get_json(base, "/v1/atoms?workspace=seat")["atoms"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn latest_proposals(home: &Path) -> BTreeMap<String, Value> {
    let mut latest = BTreeMap::new();
    let mut dirs = vec![home.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.file_name().and_then(|name| name.to_str()) != Some("proposals.jsonl") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            for line in text.lines() {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                let Some(id) = value["id"].as_str() else {
                    continue;
                };
                latest.insert(id.to_string(), value);
            }
        }
    }
    latest
}

fn lesson(text: &str) -> Value {
    let mut atom = ljos_cli::atom_body("lesson", text, ljos_cli::SEAT_WORKSPACE);
    ljos_cli::add_entities(&mut atom, ["issue:Software-kzdk".to_string()]);
    atom
}

#[test]
fn accept_makes_a_held_lesson_live_and_remember_closes_the_same_text() {
    let daemon = Daemon::start();
    let state = tempfile::tempdir().expect("state");
    let home = tempfile::tempdir().expect("home");
    // Safety: this test binary runs one test. The child packsetd does not
    // read these; the seat does.
    unsafe {
        std::env::set_var("HOME", home.path());
        std::env::set_var("XDG_STATE_HOME", state.path());
        std::env::set_var("PACKSET_URL", &daemon.url);
        std::env::set_var("PACKSET_WORKSPACE", ljos_cli::SEAT_WORKSPACE);
        std::env::set_var("LJOS_SEAT", "smoke");
    }

    let client = ljos_cli::pack().unwrap_or_else(|err| panic!("{err:#}"));
    let text = "Build the lesson on the cluster fuse.";
    let filed = ljos_cli::admit::propose_atom(&client, lesson(text))
        .unwrap_or_else(|err| panic!("{err:#}\n{}", daemon.log.lock().expect("log")));
    assert!(filed.remote, "the writer held the lesson");

    let open = open_proposals(&daemon.url);
    assert_eq!(open.len(), 1, "one filing is one proposal: {open:?}");
    if open[0].get("atom").is_none() {
        // A writer built before packset kept the held atom on the proposal
        // (7730739, released in 0.13.0) has nothing for accept to store.
        // That is the writer on PATH, not this crate; CI builds develop.
        eprintln!(
            "skipped: {} predates proposals that keep their atom; \
             install a released packset, 0.13.0 or later, to run this test",
            packsetd_bin().display()
        );
        return;
    }
    assert_eq!(open[0]["origin"], "agent-derived");
    assert_eq!(open[0]["status"], "open");
    assert_eq!(open[0]["atom"]["text"], text);
    assert_eq!(open[0]["atom"]["kind"], "lesson");
    assert_eq!(open[0]["atom"]["level"], "explicit");
    let entities = open[0]["atom"]["entities"].to_string();
    assert!(entities.contains("issue:Software-kzdk"), "{entities}");
    assert!(open[0]["atom"]["source"].is_object(), "{open:?}");
    let remote_id = open[0]["id"].as_str().expect("proposal id").to_string();
    assert!(
        live_atoms(&daemon.url)
            .iter()
            .all(|atom| atom["text"] != text),
        "a held lesson is not live"
    );

    let local = state.path().join("ljos/proposals.jsonl");
    std::fs::remove_file(&local).expect("local proposals");
    let again = ljos_cli::admit::propose_atom(&client, lesson(text)).expect("refile");
    assert!(again.remote);
    assert_eq!(again.id, filed.id);
    let open = open_proposals(&daemon.url);
    assert_eq!(
        open.len(),
        1,
        "a second filing is the open proposal already on file: {open:?}"
    );
    assert_eq!(open[0]["id"], remote_id);

    let said = ljos_cli::admit::accept(&filed.id)
        .unwrap_or_else(|err| panic!("{err:#}\n{}", daemon.log.lock().expect("log")));
    assert!(said.contains("user-declared"), "{said}");
    let live: Vec<_> = live_atoms(&daemon.url)
        .into_iter()
        .filter(|atom| atom["text"] == text)
        .collect();
    assert_eq!(live.len(), 1, "accept writes one lesson: {live:?}");
    assert_eq!(live[0]["kind"], "lesson");
    assert_eq!(live[0]["level"], "explicit");
    assert_eq!(live[0]["origin"], "user-declared");
    let entities = live[0]["entities"].to_string();
    assert!(entities.contains("issue:Software-kzdk"), "{entities}");
    assert!(live[0]["source"].is_object(), "{live:?}");
    let atom_id = live[0]["id"].as_str().expect("atom id").to_string();
    assert!(
        open_proposals(&daemon.url).is_empty(),
        "accept left a held proposal: {:?}",
        open_proposals(&daemon.url)
    );
    let recorded = latest_proposals(daemon.home.path());
    let row = recorded.get(&remote_id).expect("proposal log");
    assert_eq!(row["status"], "accepted", "{row}");
    assert_eq!(row["atom_id"], atom_id);

    let second = ljos_cli::admit::accept(&filed.id).expect("second accept");
    assert!(second.contains("already in the pack"), "{second}");
    assert_eq!(
        live_atoms(&daemon.url)
            .iter()
            .filter(|atom| atom["text"] == text)
            .count(),
        1
    );
    assert!(open_proposals(&daemon.url).is_empty());

    let typed = "Type the same words to close the hold.";
    let open_id = ljos_cli::admit::propose_atom(&client, lesson(typed))
        .expect("typed filing")
        .id;
    assert_eq!(open_proposals(&daemon.url).len(), 1);
    let wrote = ljos_cli::packset_write("Remember", typed)
        .unwrap_or_else(|err| panic!("{err:#}\n{}", daemon.log.lock().expect("log")));
    assert_eq!(wrote["origin"], "user-declared");
    assert_eq!(wrote["kind"], "lesson");
    let live: Vec<_> = live_atoms(&daemon.url)
        .into_iter()
        .filter(|atom| atom["text"] == typed)
        .collect();
    assert_eq!(
        live.len(),
        1,
        "remember does not add a second atom: {live:?}"
    );
    assert_eq!(live[0]["origin"], "user-declared");
    assert!(
        open_proposals(&daemon.url).is_empty(),
        "remember left the hold open: {:?}",
        open_proposals(&daemon.url)
    );
    let said = ljos_cli::admit::accept(&open_id).expect("accept after remember");
    assert!(
        said.contains("already written by remember or prefer"),
        "{said}"
    );
    assert_eq!(
        live_atoms(&daemon.url)
            .iter()
            .filter(|atom| atom["text"] == typed)
            .count(),
        1
    );
    assert!(open_proposals(&daemon.url).is_empty());
}
