//! For a fixed lesson set and prompt set, the lessons a seat sees in what
//! `ljos hook` prints.
//!
//! `fixtures/hook_relevance.json` is the oracle. Each case names the
//! lessons the hook prints and the rule that picks them, worked by hand.
//! Nothing here asks the pack what it would rank, so the hook and its
//! check cannot share a bug.
//!
//! packsetd runs with its token, as on a seat: with auth off the hook
//! reads the stored atoms and never searches. A stand-in encoder that
//! counts the fixture's word groups gives the pack a dense ballot where
//! the model is not installed.
//!
//! Each way a hook can go wrong has a broken hook here: the real one
//! behind a proxy that changes the pack's search answers, or with its
//! output rewritten. The fixture fails every one.

ljos_test_boot::harness!();
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Fixture {
    encoder: Encoder,
    lessons: Vec<Lesson>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Encoder {
    groups: Vec<Vec<String>>,
}

#[derive(Deserialize)]
struct Lesson {
    name: String,
    id: String,
    ts: String,
    text: String,
}

/// A prompt, or a shell command that failed.
#[derive(Deserialize)]
struct Case {
    name: String,
    prompt: Option<String>,
    failure: Option<Failure>,
    shows: Vec<String>,
}

#[derive(Deserialize)]
struct Failure {
    command: String,
    error: String,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/hook_relevance.json")).expect("fixture")
}

/// packset-embed's line protocol over the groups in the file beside the
/// script. An argument asks for the reranker or another stage this
/// stand-in does not have, so it exits and the pack runs without it.
const ENCODER: &str = r#"#!/usr/bin/env python3
import json, re, sys
if len(sys.argv) > 1:
    sys.exit(1)
groups = json.load(open(sys.argv[0] + ".json"))
def vector(text):
    words = re.findall(r"[a-z0-9]+", text.lower())
    return [float(sum(w in group for w in words)) for group in groups]
for line in sys.stdin:
    asked = json.loads(line)
    if "texts" in asked:
        print(json.dumps({"vs": [vector(t) for t in asked["texts"]]}), flush=True)
    else:
        print(json.dumps({"v": vector(asked.get("text", ""))}), flush=True)
"#;

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Held from the moment this seat starts its writer until that writer is
/// listening, and while a proxy binds, so those binds do not interleave.
static PORTS: Mutex<()> = Mutex::new(());

fn ports() -> MutexGuard<'static, ()> {
    PORTS.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Seat {
    dir: tempfile::TempDir,
    port: u16,
    path: String,
    token: String,
    _daemon: ljos_test_boot::Packsetd,
}

impl Seat {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn get(&self, path: &str) -> Value {
        ureq::get(&self.url(path))
            .set("Authorization", &format!("Bearer {}", self.token))
            .timeout(Duration::from_secs(10))
            .call()
            .unwrap_or_else(|e| panic!("GET {path}: {e}"))
            .into_json()
            .unwrap_or_else(|e| panic!("GET {path}: {e}"))
    }

    fn lesson(&self, lesson: &Lesson) {
        let posted = ureq::post(&self.url("/v1/atoms"))
            .set("Authorization", &format!("Bearer {}", self.token))
            .timeout(Duration::from_secs(10))
            .send_json(serde_json::json!({
                "schema": "inside.atom/v1",
                "kind": "lesson",
                "level": "explicit",
                "text": lesson.text,
                "workspace": ljos_cli::SEAT_WORKSPACE,
                "entities": ["horizon:standing"],
                "ts": lesson.ts,
                "id": lesson.id,
            }));
        assert!(posted.is_ok(), "post {}: {posted:?}", lesson.name);
    }

    /// The context `ljos hook` prints for `case` when it reaches the pack
    /// on `port`. Each case is its own session, so none is told a lesson
    /// was already handed over.
    fn hook(&self, case: &Case, port: u16) -> String {
        let mut payload = serde_json::json!({ "session_id": case.name, "cwd": self.dir.path() });
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        match (&case.prompt, &case.failure) {
            (Some(prompt), None) => {
                cmd.args(["hook", "--prompt"]);
                payload["hook_event_name"] = "UserPromptSubmit".into();
                payload["prompt"] = prompt.as_str().into();
            }
            (None, Some(failure)) => {
                cmd.arg("hook");
                payload["hook_event_name"] = "PostToolUseFailure".into();
                payload["tool_name"] = "Bash".into();
                payload["tool_input"] = serde_json::json!({ "command": failure.command });
                payload["error"] = failure.error.as_str().into();
            }
            _ => panic!("{} names one of a prompt or a failure", case.name),
        }
        env_for(&mut cmd, self.dir.path(), &self.path, port);
        let said = run(cmd, Some(&payload.to_string()));
        assert!(said.status.success(), "{}: {}", case.name, text(&said));
        if said.stdout.iter().all(u8::is_ascii_whitespace) {
            return String::new();
        }
        let out: Value = serde_json::from_slice(&said.stdout)
            .unwrap_or_else(|e| panic!("{e}: {} printed {}", case.name, text(&said)));
        out["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}

fn env_for(cmd: &mut Command, home: &Path, path: &str, port: u16) {
    cmd.env_clear()
        .current_dir(home)
        .env("HOME", home)
        .env("PATH", path)
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.join("tmp"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("PACKSET_PORT", port.to_string())
        .env("LJOS_TRACKER_GIT", "off");
}

fn run(mut cmd: Command, stdin: Option<&str>) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut pipe = child.stdin.take().expect("stdin");
    if let Some(text) = stdin {
        pipe.write_all(text.as_bytes()).expect("write stdin");
    }
    drop(pipe);
    child.wait_with_output().expect("wait")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A fresh home with packsetd on a free port, the stand-in encoder as its
/// model, holding the fixture's lessons. `None` where packsetd is not
/// installed.
fn seat(fixture: &Fixture) -> Option<Seat> {
    let Some(packsetd) = on_path("packsetd") else {
        eprintln!("skipped: packsetd is not installed");
        return None;
    };
    let dir = tempfile::tempdir().expect("home");
    for sub in [
        "tmp",
        "run",
        ".config",
        ".local/share",
        ".local/state",
        ".cache",
    ] {
        std::fs::create_dir_all(dir.path().join(sub)).expect("home dirs");
    }
    let encoder = dir.path().join("packset-embed");
    std::fs::write(&encoder, ENCODER).expect("encoder");
    std::fs::set_permissions(&encoder, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let groups = serde_json::to_string(&fixture.encoder.groups).expect("groups");
    std::fs::write(dir.path().join("packset-embed.json"), groups).expect("groups");
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_ljos"))
        .parent()
        .expect("bin dir");
    let path = std::env::join_paths([
        bin_dir,
        packsetd.parent().expect("packsetd dir"),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .expect("PATH")
    .into_string()
    .expect("utf-8 PATH");
    let ports = ports();
    let pack_home = dir.path().join(".local/share/packset");
    std::fs::create_dir_all(&pack_home).expect("pack home");
    let mut started = ljos_test_boot::Packsetd::build();
    started.home_at(dir.path());
    started.pack_at(&pack_home);
    env_for(started.command(), dir.path(), &path, 0);
    started.command().env("PACKSET_EMBED", &encoder);
    let daemon = started.spawn();
    let port = daemon.port();
    drop(ports);
    let token = std::fs::read_to_string(pack_home.join("token")).expect("packsetd token");
    let token = token.trim().to_string();
    let seat = Seat {
        dir,
        port,
        path,
        token,
        _daemon: daemon,
    };
    for lesson in &fixture.lessons {
        seat.lesson(lesson);
    }
    let listed = seat.get(&format!("/v1/atoms?workspace={}", ljos_cli::SEAT_WORKSPACE));
    let live = listed["atoms"].as_array().map_or(0, Vec::len);
    assert_eq!(
        live,
        fixture.lessons.len(),
        "the pack let one lesson replace another: {listed}"
    );
    let status = seat.get("/v1/status");
    assert_eq!(
        status["embedder"]["answering"], true,
        "the stand-in encoder did not answer: {status}"
    );
    Some(seat)
}

/// Each case where `hook` printed other lessons than the fixture names,
/// in another order, or a text no lesson has, with what it printed.
fn misses(fixture: &Fixture, hook: impl Fn(&Case) -> String) -> Vec<(String, String)> {
    let names: HashMap<&str, &str> = fixture
        .lessons
        .iter()
        .map(|l| (l.text.as_str(), l.name.as_str()))
        .collect();
    let mut misses = Vec::new();
    for case in &fixture.cases {
        let context = hook(case);
        let shown: Vec<&str> = context
            .lines()
            .filter_map(|line| line.strip_prefix("- ["))
            .map(|line| line.split_once("] ").map_or(line, |(_, text)| text))
            .map(|text| names.get(text).copied().unwrap_or("a text no lesson has"))
            .collect();
        if shown != case.shows {
            misses.push((
                case.name.clone(),
                format!("shows {shown:?}, not {:?}:\n{context}", case.shows),
            ));
        }
    }
    misses
}

fn report(misses: &[(String, String)]) -> String {
    misses
        .iter()
        .map(|(case, miss)| format!("{case}: {miss}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The fixture failed the broken hook on each of `cases`.
fn assert_caught(misses: &[(String, String)], cases: &[&str]) {
    for case in cases {
        assert!(
            misses.iter().any(|(name, _)| name == case),
            "the broken hook passed {case}; it missed only:\n{}",
            report(misses)
        );
    }
}

/// What a broken hook does to the pack's search answers.
#[derive(Clone, Copy)]
enum Broken {
    /// Leaves out the encoder's rank and cosine, as if the pack sent no
    /// dense ballot.
    DropsDense,
    /// Answers every search with the first answer it got.
    ReplaysSearch,
    /// Hands over the hits in reverse, scores and all, so where the hook's
    /// own keys tie it falls back on the reverse of the pack's order.
    ReversesTies,
}

impl Broken {
    fn search(self, answer: Vec<u8>, kept: &Mutex<Option<Vec<u8>>>) -> Vec<u8> {
        if let Self::ReplaysSearch = self {
            return kept
                .lock()
                .expect("kept answer")
                .get_or_insert(answer)
                .clone();
        }
        let Some(at) = find(&answer, b"\r\n\r\n") else {
            return answer;
        };
        let Ok(mut body) = serde_json::from_slice::<Value>(&answer[at + 4..]) else {
            return answer;
        };
        let Some(hits) = body["hits"].as_array_mut() else {
            return answer;
        };
        if let Self::DropsDense = self {
            for hit in hits.iter_mut().filter_map(Value::as_object_mut) {
                hit.remove("dense_rank");
                hit.remove("cosine");
            }
        } else {
            hits.reverse();
            for (score, hit) in (1..=hits.len()).rev().zip(hits.iter_mut()) {
                hit["score"] = score.into();
            }
        }
        let body = serde_json::to_vec(&body).expect("search body");
        let mut out: Vec<u8> = String::from_utf8_lossy(&answer[..at])
            .lines()
            .filter(|line| !line.to_ascii_lowercase().starts_with("content-length:"))
            .flat_map(|line| [line, "\r\n"])
            .collect::<String>()
            .into_bytes();
        out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        out.extend_from_slice(&body);
        out
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A port that passes each request to packsetd on `upstream` and changes
/// its search answers as `broken` does. packsetd closes each connection
/// after its answer, so one read to the end is one answer.
fn broken_pack(upstream: u16, broken: Broken) -> u16 {
    let listener = {
        let _ports = ports();
        TcpListener::bind("127.0.0.1:0").expect("proxy port")
    };
    let port = listener.local_addr().expect("addr").port();
    let kept: Arc<Mutex<Option<Vec<u8>>>> = Arc::default();
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let kept = Arc::clone(&kept);
            std::thread::spawn(move || relay(client, upstream, broken, &kept));
        }
    });
    port
}

fn relay(mut client: TcpStream, upstream: u16, broken: Broken, kept: &Mutex<Option<Vec<u8>>>) {
    let Some(request) = read_request(&mut client) else {
        return;
    };
    let Ok(mut pack) = TcpStream::connect(("127.0.0.1", upstream)) else {
        return;
    };
    let mut answer = Vec::new();
    if pack.write_all(&request).is_err() || pack.read_to_end(&mut answer).is_err() {
        return;
    }
    if request.starts_with(b"GET /v1/search") {
        answer = broken.search(answer, kept);
    }
    let _ = client.write_all(&answer);
}

/// One request: its head and the `Content-Length` bytes after it.
fn read_request(client: &mut TcpStream) -> Option<Vec<u8>> {
    let mut request = Vec::new();
    let mut chunk = [0; 4096];
    let head = loop {
        if let Some(at) = find(&request, b"\r\n\r\n") {
            break at + 4;
        }
        let n = client.read(&mut chunk).ok().filter(|n| *n > 0)?;
        request.extend_from_slice(&chunk[..n]);
    };
    let length = String::from_utf8_lossy(&request[..head])
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while request.len() < head + length {
        let n = client.read(&mut chunk).ok().filter(|n| *n > 0)?;
        request.extend_from_slice(&chunk[..n]);
    }
    Some(request)
}

/// `context` with each lesson cut at its first clause, as a hook that
/// shortens what it hands over prints it.
fn first_clauses(context: &str) -> String {
    context
        .lines()
        .map(|line| match line.split_once("; ") {
            Some((clause, _)) if line.starts_with("- [") => format!("{clause}."),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn each_case_shows_the_lessons_the_fixture_names() {
    let fixture = fixture();
    let Some(seat) = seat(&fixture) else {
        return;
    };
    let misses = misses(&fixture, |case| seat.hook(case, seat.port));
    assert!(misses.is_empty(), "{}", report(&misses));
}

/// Without the cosine the paraphrase, which shares no word with any
/// lesson, gets nothing, and the words alone let the signing ceremony in
/// beside the signing key.
#[test]
fn a_hook_that_drops_the_dense_ballot_misses_the_paraphrase() {
    let fixture = fixture();
    let Some(seat) = seat(&fixture) else {
        return;
    };
    let port = broken_pack(seat.port, Broken::DropsDense);
    let misses = misses(&fixture, |case| seat.hook(case, port));
    assert_caught(&misses, &["paraphrase", "dense-band"]);
}

/// Every later search is answered with the first case's, about a rejected
/// token: the paraphrase is handed those lessons, and the dense band gets
/// no cosine.
#[test]
fn a_hook_that_replays_a_cached_search_misses_the_paraphrase() {
    let fixture = fixture();
    let Some(seat) = seat(&fixture) else {
        return;
    };
    let port = broken_pack(seat.port, Broken::ReplaysSearch);
    let misses = misses(&fixture, |case| seat.hook(case, port));
    assert_caught(&misses, &["paraphrase", "dense-band"]);
}

#[test]
fn a_hook_that_rewrites_lesson_text_shows_a_text_no_lesson_has() {
    let fixture = fixture();
    let Some(seat) = seat(&fixture) else {
        return;
    };
    let misses = misses(&fixture, |case| first_clauses(&seat.hook(case, seat.port)));
    assert_caught(
        &misses,
        &[
            "covers-the-prompt",
            "alias-in-a-full-window",
            "failure-note",
        ],
    );
}

/// The lookalikes tie on words and length, so the other order hands over
/// the ones in the window whose ids sort last.
#[test]
fn a_hook_that_breaks_ties_the_other_way_shows_other_lookalikes() {
    let fixture = fixture();
    let Some(seat) = seat(&fixture) else {
        return;
    };
    let port = broken_pack(seat.port, Broken::ReversesTies);
    let misses = misses(&fixture, |case| seat.hook(case, port));
    assert_caught(
        &misses,
        &[
            "covers-the-prompt",
            "alias-in-a-full-window",
            "failure-note",
        ],
    );
}
