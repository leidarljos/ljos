//! An agent lesson against packsetd built from develop.
//!
//! The lesson is live at once, labelled agent-derived at L0, on a writer
//! that holds agent writes too. Accept raises it to explicit and records
//! the review. A proposal an older ljos left open is stored once.

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

/// packsetd from PATH, auth off, in a temp home, with `env` added.
///
/// Real token support in ljos's client comes with the coordinated packset 0.14 release.
fn writer(env: &[(&str, &str)]) -> ljos_test_boot::Packsetd {
    let mut started = ljos_test_boot::Packsetd::build();
    started.bin(packsetd_bin());
    started
        .command()
        .env("PACKSET_AUTH", "off")
        .envs(env.iter().copied());
    started.spawn()
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

fn lesson(text: &str) -> Value {
    let mut atom = ljos_cli::atom_body("lesson", text, ljos_cli::SEAT_WORKSPACE);
    ljos_cli::add_entities(&mut atom, ["issue:Software-kzdk".to_string()]);
    atom
}

/// One process, one environment. This binary's tests take turns, and each
/// one puts the variables back, including when it returns early.
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|err| err.into_inner())
}

struct SeatEnv {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    _runtime: tempfile::TempDir,
}

impl SeatEnv {
    fn set(pairs: &[(&'static str, &str)]) -> Self {
        let runtime = tempfile::tempdir().expect("runtime");
        let mut keys: Vec<&str> = pairs.iter().map(|(key, _)| *key).collect();
        keys.push("XDG_RUNTIME_DIR");
        let saved = keys
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect();
        unsafe {
            for (key, value) in pairs {
                std::env::set_var(key, value);
            }
            std::env::set_var("XDG_RUNTIME_DIR", runtime.path());
        }
        Self {
            saved,
            _runtime: runtime,
        }
    }
}

impl Drop for SeatEnv {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

/// The pack and the seat one test talks to, with the seat's env put back
/// when it ends.
struct Seat {
    daemon: ljos_test_boot::Packsetd,
    state: tempfile::TempDir,
    _home: tempfile::TempDir,
    _env: SeatEnv,
}

fn seat(daemon_env: &[(&str, &str)]) -> Seat {
    let daemon = writer(daemon_env);
    let state = tempfile::tempdir().expect("state");
    let home = tempfile::tempdir().expect("home");
    // The child packsetd is given its env on the command. The seat in this
    // process reads these.
    let env = SeatEnv::set(&[
        ("HOME", home.path().to_str().expect("home")),
        ("XDG_STATE_HOME", state.path().to_str().expect("state")),
        ("PACKSET_URL", daemon.url()),
        ("PACKSET_WORKSPACE", ljos_cli::SEAT_WORKSPACE),
        ("LJOS_SEAT", "smoke"),
    ]);
    Seat {
        daemon,
        state,
        _home: home,
        _env: env,
    }
}

fn live_with(base: &str, text: &str) -> Vec<Value> {
    live_atoms(base)
        .into_iter()
        .filter(|atom| atom["text"] == text)
        .collect()
}

#[test]
fn an_agent_lesson_is_live_at_once_and_accept_raises_it() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let _lock = env_lock();
    let seat = seat(&[]);
    let url = seat.daemon.url();
    let client = ljos_cli::pack().unwrap_or_else(|err| panic!("{err:#}"));
    let text = "Build the lesson on the cluster fuse.";
    let stored = ljos_cli::admit::store_labelled(&client, lesson(text))
        .unwrap_or_else(|err| panic!("{err:#}\n{}", seat.daemon.log()));
    let id = stored["id"].as_str().expect("atom id").to_string();

    let live = live_with(url, text);
    assert_eq!(live.len(), 1, "stored once: {live:?}");
    assert_eq!(live[0]["id"], id.as_str());
    assert_eq!(live[0]["origin"], "agent-derived", "{live:?}");
    assert_eq!(live[0]["source"]["basis"], "agent", "{live:?}");
    assert_eq!(live[0]["level"], "derived", "{live:?}");
    let entities = live[0]["entities"].to_string();
    assert!(entities.contains("issue:Software-kzdk"), "{entities}");
    assert!(open_proposals(url).is_empty(), "nothing is held");

    let said = ljos_cli::admit::accept(&id[..8])
        .unwrap_or_else(|err| panic!("{err:#}\n{}", seat.daemon.log()));
    assert!(said.contains("origin agent-derived"), "{said}");
    let live = live_with(url, text);
    assert_eq!(live.len(), 1, "accept adds no second atom: {live:?}");
    assert_eq!(live[0]["level"], "explicit", "{live:?}");
    assert_eq!(live[0]["origin"], "agent-derived", "{live:?}");
    let review = ljos_cli::memories::reviews();
    assert_eq!(review.get(&id).map(|r| r.verdict.as_str()), Some("keep"));
}

#[test]
fn a_writer_that_holds_agent_writes_still_stores_the_lesson_as_the_agents() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let _lock = env_lock();
    let seat = seat(&[("PACKSET_HOLD_AGENT_WRITES", "1")]);
    let url = seat.daemon.url();
    let client = ljos_cli::pack().unwrap_or_else(|err| panic!("{err:#}"));
    let text = "Pin the fuse lesson before the rebuild.";
    ljos_cli::admit::store_labelled(&client, lesson(text))
        .unwrap_or_else(|err| panic!("{err:#}\n{}", seat.daemon.log()));
    let live = live_with(url, text);
    assert_eq!(live.len(), 1, "the held write is live: {live:?}");
    assert_eq!(live[0]["origin"], "agent-derived", "{live:?}");
    assert!(
        open_proposals(url).is_empty(),
        "the hold was accepted on the spot: {:?}",
        open_proposals(url)
    );
}

#[test]
fn a_proposal_an_older_ljos_left_open_is_stored_once() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let _lock = env_lock();
    let seat = seat(&[]);
    let url = seat.daemon.url();
    let text = "Drain the fuse queue before a rebuild.";
    let rows = seat.state.path().join("ljos/proposals.jsonl");
    std::fs::create_dir_all(rows.parent().expect("state dir")).expect("state dir");
    let mut atom = lesson(text);
    atom["origin"] = Value::from("agent-derived");
    atom["level"] = Value::from("derived");
    let row = serde_json::json!({
        "id": "7f3e9a1c2b4d",
        "status": "open",
        "remote": false,
        "text": text,
        "kind": "lesson",
        "origin": "agent-derived",
        "atom": atom,
    });
    std::fs::write(&rows, format!("{row}\n")).expect("proposals");

    let first = ljos_cli::admit::import_open(60_000);
    assert_eq!(
        first,
        ljos_cli::admit::Imported { stored: 1, left: 0 },
        "{}",
        seat.daemon.log()
    );
    let live = live_with(url, text);
    assert_eq!(live.len(), 1, "imported once: {live:?}");
    assert_eq!(live[0]["origin"], "agent-derived", "{live:?}");
    assert_eq!(live[0]["level"], "derived", "{live:?}");
    assert_eq!(
        ljos_cli::admit::import_open(60_000),
        ljos_cli::admit::Imported::default()
    );
    assert_eq!(
        live_with(url, text).len(),
        1,
        "a second call stores nothing"
    );

    let said = ljos_cli::admit::accept("7f3e9a1c").expect("accept the old id");
    assert!(said.contains("accepted"), "{said}");
    let live = live_with(url, text);
    assert_eq!(live.len(), 1, "{live:?}");
    assert_eq!(live[0]["level"], "explicit", "{live:?}");
}
