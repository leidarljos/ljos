//! Two search faults a new seat met on its first day, pinned against a fake
//! pack that needs no network and no packsetd.
//!
//! ljos-ahp7: a seat's own open ticket whose title, body or notes carry two
//! query terms ranks above other projects' tickets that share one generic
//! word, and a `ljos remember` of two sentences prints every lesson id it
//! stored.
//!
//! ljos-xsxq: the search page follows the pack's fused rank and not the
//! raw cosine, and a `/health` of `packsetd warming` is a daemon that is up.

#[path = "common/boot.rs"]
mod boot;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const SEAT: &str = "testseat";

struct Request {
    method: String,
    path: String,
    query: String,
    body: String,
}

struct Reply {
    status: u16,
    kind: &'static str,
    body: String,
}

impl Reply {
    fn json(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            kind: "application/json",
            body: body.into(),
        }
    }

    fn text(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            kind: "text/plain",
            body: body.into(),
        }
    }
}

fn read_request(sock: &mut std::net::TcpStream) -> Option<Request> {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while buf.len() < head_end + length {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
    let mut words = head.split_whitespace();
    let method = words.next().unwrap_or("").to_string();
    let target = words.next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Some(Request {
        method,
        path: path.to_string(),
        query: query.to_string(),
        body: String::from_utf8_lossy(&buf[head_end.min(buf.len())..]).into_owned(),
    })
}

fn serve(handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handler = Arc::new(handler);
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let handler = Arc::clone(&handler);
            thread::spawn(move || {
                let mut sock = conn;
                let Some(request) = read_request(&mut sock) else {
                    return;
                };
                let reply = handler(&request);
                let wire = format!(
                    "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.status,
                    reply.kind,
                    reply.body.len(),
                    reply.body
                );
                let _ = sock.write_all(wire.as_bytes());
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

const STATUS: &str = r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#;

struct Seat {
    root: PathBuf,
    url: String,
}

impl Seat {
    fn open(url: String) -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/share",
            "run",
            "work",
            "tracker/Software",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        Self { root, url }
    }

    fn board(&self, project: &str, body: &str) {
        let dir = self.root.join("tracker/Software").join(project);
        std::fs::create_dir_all(&dir).unwrap();
        let head = "#+TITLE: issues\n#+VISSUE: 1\n#+CATEGORY: Software\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n";
        std::fs::write(dir.join("issues.org"), format!("{head}{body}")).unwrap();
    }

    fn command(&self, args: &[&str]) -> Command {
        let home = self.root.join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("USER", "walker")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", SEAT)
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_NO_AUTOSTART", "1")
            .env("LJOS_SEARCH_WAIT", "0")
            .env("LJOS_SEARCH_WARMUP_WAIT", "0")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("NO_COLOR", "1")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args)
            .output()
            .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")))
    }
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn json_field(body: &str, key: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get(key).and_then(|t| t.as_str().map(str::to_string)))
        .unwrap_or_default()
}

fn ticket_hit(id: &str, title: &str, score: u32, rank: u32) -> String {
    format!(
        r#"{{"id":"{id}","kind":"ticket","text":"{title}","score":{score},"cosine":0.91,"dense_rank":{rank},"entities":["issue:{id}"],"ballots":1,"of":1}}"#
    )
}

fn row(title: &str, id: &str, state: &str, claimed_by: &str, log: &str, body: &str) -> String {
    let log = if log.is_empty() {
        String::new()
    } else {
        format!(":LOGBOOK:\n- Note: \"{log}\" [2026-11-17 Tue 09:00]\n:END:\n")
    };
    format!(
        "* {state} [#B] {title}\n:PROPERTIES:\n:ID:         {id}\n:CLAIMED_BY: {claimed_by}\n:END:\n{log}{body}\n\n"
    )
}

fn before(page: &str, earlier: &str, later: &str) {
    let first = page
        .find(earlier)
        .unwrap_or_else(|| panic!("{earlier} is not on the page:\n{page}"));
    let second = page
        .find(later)
        .unwrap_or_else(|| panic!("{later} is not on the page:\n{page}"));
    assert!(first < second, "{earlier} should be above {later}:\n{page}");
}

/// Three own tickets, one per place a query term can sit, and one other
/// project's ticket for each that shares a single generic word.
#[test]
fn ahp7_an_own_ticket_with_two_matching_terms_outranks_other_projects() {
    let others = [
        ("docs-g1g1", "Gantry crane docs page", "gantry torque"),
        ("docs-l1l1", "Ledger export docs page", "quartz ledger"),
        ("docs-r1r1", "Relay docs page", "amber relay"),
    ];
    let own_title = ("mine-t1t1", "Gantry torque calibration");
    let url = serve(move |req| match req.path.as_str() {
        "/health" => Reply::text("packsetd ok"),
        "/v1/status" => Reply::json(STATUS),
        "/v1/search" => {
            let mut hits = vec![
                ticket_hit("docs-g1g1", "Gantry crane docs page", 93, 1),
                ticket_hit("docs-l1l1", "Ledger export docs page", 88, 2),
                ticket_hit("docs-r1r1", "Relay docs page", 84, 3),
            ];
            if req.query.contains("gantry") {
                hits.push(ticket_hit(own_title.0, own_title.1, 61, 4));
            }
            Reply::json(format!(
                r#"{{"hits":[{}],"rank":"relevance"}}"#,
                hits.join(",")
            ))
        }
        "/v1/atoms" => {
            Reply::json(r#"{"atoms":[{"id":"lesson-1","kind":"lesson","text":"kept"}]}"#)
        }
        _ => Reply::json(r#"{"ok":true}"#),
    });
    let seat = Seat::open(url);
    seat.board(
        "mine",
        &[
            row(own_title.1, own_title.0, "STARTED", SEAT, "", ""),
            row(
                "Weekly checklist",
                "mine-b0b0",
                "STARTED",
                SEAT,
                "",
                "Quartz ledger rows must be reconciled by Friday.",
            ),
            row(
                "Standing task",
                "mine-n0n0",
                "STARTED",
                SEAT,
                "Amber relay swap logged today",
                "",
            ),
        ]
        .concat(),
    );
    seat.board(
        "docs",
        &others
            .iter()
            .map(|(id, title, _)| row(title, id, "TODO", "bob", "", ""))
            .collect::<String>(),
    );
    for (own, place, query) in [
        ("mine-t1t1", "title", "gantry torque"),
        ("mine-b0b0", "body", "quartz ledger"),
        ("mine-n0n0", "notes", "amber relay"),
    ] {
        let out = seat.run(&["search", query]);
        let page = said(&out);
        assert!(out.status.success(), "{place}: {page}");
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(
            stdout.contains(own),
            "an own ticket matching {query:?} in its {place} is not on the page:\n{page}"
        );
        for (id, _, term) in others {
            if term == query {
                if let Some(at) = stdout.find(id) {
                    assert!(
                        stdout.find(own).unwrap() < at,
                        "{id} ranked above {own} for {query:?}:\n{page}"
                    );
                }
            }
        }
    }
}

fn counting_pack(posts: Arc<Mutex<Vec<String>>>) -> String {
    serve(move |req| match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/v1/atoms") => {
            let text = json_field(&req.body, "text");
            let mut seen = posts.lock().unwrap();
            let id = format!(
                "{:08x}{}",
                0x1a2b_3c4d_u32 + 0x0101_0101 * seen.len() as u32,
                "0".repeat(24)
            );
            seen.push(text.clone());
            let quoted = serde_json::Value::String(text).to_string();
            Reply::json(format!(
                r#"{{"id":"{id}","kind":"lesson","text":{quoted}}}"#
            ))
        }
        (_, "/v1/status") => Reply::json(STATUS),
        (_, "/health") => Reply::text("packsetd ok"),
        (_, "/v1/atoms") => Reply::json(r#"{"atoms":[]}"#),
        _ => Reply::json(r#"{"ok":true}"#),
    })
}

fn printed_ids(stdout: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in stdout.lines() {
        let Some(rest) = line.trim_start().strip_prefix("id ") else {
            continue;
        };
        let id: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
        if id.len() >= 8 {
            ids.push(id);
        }
    }
    ids
}

#[test]
fn ahp7_a_two_sentence_remember_prints_every_stored_lesson_id() {
    for (text, lessons) in [
        ("Dry the seal before each firing.", 1usize),
        (
            "Wipe the seal with a dry cloth. Check the hinge for rust.",
            1,
        ),
        (
            "Wipe the seal with a dry cloth. Check the hinge for rust. Oil the latch monthly.",
            2,
        ),
    ] {
        let posts = Arc::new(Mutex::new(Vec::new()));
        let seat = Seat::open(counting_pack(Arc::clone(&posts)));
        let out = seat.run(&["remember", text]);
        let page = said(&out);
        assert!(out.status.success(), "{page}");
        let stored = posts.lock().unwrap().clone();
        assert_eq!(stored.len(), lessons, "{text:?} stored {stored:?}\n{page}");
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let printed = printed_ids(&stdout);
        assert_eq!(
            printed.len(),
            stored.len(),
            "the page names {} ids for {} stored lessons:\n{page}",
            printed.len(),
            stored.len()
        );
        for (index, lesson) in stored.iter().enumerate() {
            let id = format!("{:08x}", 0x1a2b_3c4d_u32 + 0x0101_0101 * index as u32);
            assert!(printed.contains(&id), "{id} is not printed:\n{page}");
            assert!(
                stdout.contains(lesson.as_str()),
                "{lesson:?} is not printed:\n{page}"
            );
        }
    }
}

const SLOW: &str = "the kiln schedule is the slow plan";
const MIDDLE: &str = "kiln schedule middle restart";
const QUICK: &str = "the kiln schedule is the quick plan";

/// The pack lists three rows by `score`, the fused order. `cosine` and
/// `dense_rank` say the reverse. Every row is another seat's, so the page
/// order is the pack's and not this seat's.
fn fused_pack() -> String {
    let hits = format!(
        r#"{{"hits":[
            {{"id":"slow","kind":"lesson","text":"{SLOW}","score":90,"cosine":0.21,"dense_rank":3,"entities":["seat:other"],"ballots":2,"of":2}},
            {{"id":"middle","kind":"lesson","text":"{MIDDLE}","score":80,"cosine":0.40,"dense_rank":2,"entities":["seat:other"],"ballots":2,"of":2}},
            {{"id":"quick","kind":"lesson","text":"{QUICK}","score":70,"cosine":0.95,"dense_rank":1,"entities":["seat:other"],"ballots":2,"of":2}}
        ],"rank":"fused"}}"#
    );
    serve(move |req| match req.path.as_str() {
        "/health" => Reply::text("packsetd ok"),
        "/v1/status" => Reply::json(STATUS),
        "/v1/search" => Reply::json(hits.clone()),
        "/v1/atoms" => Reply::json(r#"{"atoms":[]}"#),
        _ => Reply::json(r#"{"ok":true}"#),
    })
}

#[test]
fn xsxq_the_page_follows_the_fused_rank_and_not_the_cosine() {
    let seat = Seat::open(fused_pack());
    let out = seat.run(&["search", "kiln", "schedule"]);
    let page = said(&out);
    assert!(out.status.success(), "{page}");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    before(&stdout, SLOW, MIDDLE);
    before(&stdout, MIDDLE, QUICK);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(ljos_cli::MEANING_AND_WORDS_LINE),
        "the line should name the fused rank:\n{page}"
    );
    assert!(
        !stderr.contains("matched by meaning"),
        "the cosine is not the order:\n{page}"
    );

    let out = seat.run(&["search", "--json", "kiln", "schedule"]);
    let page = said(&out);
    assert!(out.status.success(), "{page}");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    before(&stdout, SLOW, MIDDLE);
    before(&stdout, MIDDLE, QUICK);
}

fn warming_pack(health: &'static str, asked: Arc<Mutex<BTreeMap<String, usize>>>) -> String {
    serve(move |req| {
        *asked.lock().unwrap().entry(req.path.clone()).or_default() += 1;
        match (req.method.as_str(), req.path.as_str()) {
            (_, "/health") => Reply::text(health),
            (_, "/v1/status") => Reply::json(
                r#"{"embedder":{"available":true,"answering":true,"enabled":true},"live":1,"live_cap":0}"#,
            ),
            (_, "/v1/search") => Reply::json(format!(
                r#"{{"hits":[{{"id":"kiln","kind":"lesson","text":"{QUICK}","score":80,"entities":["seat:other"],"ballots":2,"of":2}}],"rank":"fused","warming":["embedder"]}}"#
            )),
            ("POST", "/v1/atoms") => Reply::json(
                r#"{"id":"c0ffee00c0ffee00c0ffee00c0ffee00","kind":"lesson","text":"kept"}"#,
            ),
            (_, "/v1/atoms") => Reply::json(r#"{"atoms":[]}"#),
            (_, "/stats") => Reply::json(r#"{"ok":true,"line":"ok"}"#),
            _ => Reply::json(r#"{"ok":true}"#),
        }
    })
}

#[test]
fn xsxq_a_health_of_packsetd_warming_counts_as_up() {
    const STARTING: &str = "Memory is starting, so try again in a few seconds.";
    for health in [
        "packsetd warming\n",
        "packsetd warming\nloading embedder\neta_s 4.5\n",
    ] {
        let asked = Arc::new(Mutex::new(BTreeMap::new()));
        let seat = Seat::open(warming_pack(health, Arc::clone(&asked)));
        let search = seat.run(&["search", "kiln", "schedule"]);
        let page = said(&search);
        assert!(search.status.success(), "{health:?}: {page}");
        assert!(page.contains(QUICK), "{health:?}: {page}");
        assert!(!page.contains(STARTING), "{health:?}: {page}");
        assert!(
            asked.lock().unwrap().contains_key("/health"),
            "{health:?}: search never read /health"
        );

        let kept = seat.run(&["remember", "Cold metal seats the kiln."]);
        let page = said(&kept);
        assert!(kept.status.success(), "{health:?}: {page}");
        assert!(!page.contains(STARTING), "{health:?}: {page}");

        let doctor = seat.run(&["doctor"]);
        let page = said(&doctor);
        assert!(
            page.contains("warming\tmemory\t"),
            "{health:?}: doctor does not name a warming daemon:\n{page}"
        );
        assert!(
            !page.contains("no\tmemory\t") && !page.contains("packset ensure"),
            "{health:?}: a warming daemon is up and is not started again:\n{page}"
        );
    }
}
