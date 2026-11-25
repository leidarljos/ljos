//! What a person reads from the commands that list memories and tickets.
//!
//! A temporary pack and a temporary tracker hold short claims, a long
//! claim, a claim with Icelandic letters, a claim that is one long word,
//! a claim that is a long URL, enough hits to pass a small limit, and a
//! query that matches nothing. The files under `listing_pages/` are that
//! output with the clock, the fence nonce and this directory folded out,
//! so the shape stays put when the day changes.

ljos_test_boot::harness!();

#[path = "common/pty.rs"]
mod pty;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

const SNAP_DIR: &str = "tests/listing_pages";

struct Home {
    root: PathBuf,
    url: String,
}

impl Home {
    fn open(atoms: Value) -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/share",
            "home/.local/bin",
            "run",
            "work",
            "tracker/Software/demo",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(root.join("tracker/Software/demo/issues.org"), ISSUES).unwrap();
        let bin = root.join("home/.local/bin/vissue");
        ljos_cli::install_executable(&bin, VISSUE_STUB).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || serve(listener, atoms));
        Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, None)
    }

    fn run_with(&self, args: &[&str], columns: Option<&str>) -> Output {
        self.command(args, columns)
            .output()
            .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")))
    }

    fn command(&self, args: &[&str], columns: Option<&str>) -> Command {
        let home = self.root.join("home");
        let path = format!("{}:/usr/bin:/bin", home.join(".local/bin").display());
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("NO_COLOR", "1")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .stdin(Stdio::null());
        if let Some(cols) = columns {
            cmd.env("COLUMNS", cols);
        }
        cmd
    }
}

fn serve(listener: TcpListener, atoms: Value) {
    for conn in listener.incoming().flatten() {
        let atoms = atoms.clone();
        thread::spawn(move || {
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
            let request = head.split_whitespace().nth(1).unwrap_or("/");
            let path = request.split('?').next().unwrap_or("/");
            let query = request.split_once('?').map(|(_, q)| q).unwrap_or("");
            let body = if path == "/v1/search" {
                search_body(&atoms, &form_get(query, "q"), &form_get(query, "limit"))
            } else if path == "/v1/atoms" {
                atoms.to_string()
            } else if path == "/v1/status" {
                r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#
                    .to_string()
            } else if path == "/v1/hubs" {
                hubs_body(&atoms)
            } else if path == "/v1/activate" {
                island_body(&atoms, &form_get(query, "q"))
            } else if path == "/v1/sweep" {
                r#"{"lapsed":0,"forgotten":0}"#.to_string()
            } else if path == "/health" || path == "/v1/health" || path == "/v1/identity" {
                r#"{"ok":true,"workspace":"seat"}"#.to_string()
            } else {
                r#"{"ok":true}"#.to_string()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        });
    }
}

fn form_get(query: &str, key: &str) -> String {
    for pair in query.split('&') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        if name == key {
            return decode(value);
        }
    }
    String::new()
}

fn decode(raw: &str) -> String {
    let mut bytes = Vec::new();
    let raw_bytes = raw.as_bytes();
    let mut i = 0;
    while i < raw_bytes.len() {
        match raw_bytes[i] {
            b'+' => bytes.push(b' '),
            b'%' if i + 2 < raw_bytes.len() => {
                let hex = &raw[i + 1..i + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    bytes.push(byte);
                    i += 3;
                    continue;
                }
                bytes.push(b'%');
            }
            byte => bytes.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn search_body(atoms: &Value, query: &str, limit: &str) -> String {
    let words = words_of(query);
    let mut hits: Vec<&Value> = atoms["atoms"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|atom| {
            let text = atom["text"].as_str().unwrap_or("").to_lowercase();
            words.iter().any(|word| text.contains(word))
        })
        .collect();
    hits.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
    });
    let limit = limit.parse::<usize>().unwrap_or(10);
    hits.truncate(limit);
    let hits: Vec<Value> = hits.iter().map(|atom| (*atom).clone()).collect();
    json!({"hits": hits, "rank": "relevance"}).to_string()
}

fn hubs_body(atoms: &Value) -> String {
    let hubs: Vec<Value> = atoms["atoms"]
        .as_array()
        .into_iter()
        .flatten()
        .take(4)
        .enumerate()
        .map(|(index, atom)| {
            json!({
                "id": atom["id"],
                "text": atom["text"],
                "kind": atom["kind"],
                "score": 1.0 - index as f64 * 0.1,
                "links": 4 - index,
                "ts": atom["ts"],
                "entities": atom["entities"],
                "origin": atom["origin"],
            })
        })
        .collect();
    json!({"hubs": hubs}).to_string()
}

fn island_body(atoms: &Value, query: &str) -> String {
    let words = words_of(query);
    let island: Vec<Value> = atoms["atoms"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|atom| {
            let text = atom["text"].as_str().unwrap_or("").to_lowercase();
            words.iter().any(|word| text.contains(word))
        })
        .take(4)
        .enumerate()
        .map(|(index, atom)| {
            json!({
                "id": atom["id"],
                "text": atom["text"],
                "kind": atom["kind"],
                "activation": 1.0 - index as f64 * 0.15,
                "seed": index < 2,
                "ts": atom["ts"],
                "entities": atom["entities"],
                "origin": atom["origin"],
                "source": atom["source"],
            })
        })
        .collect();
    json!({"island": island, "fired": 0}).to_string()
}

fn words_of(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| word.len() >= 3)
        .collect()
}

fn page(out: &Output, root: &Path) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let text = format!(
        "status {}\nstderr:\n{stderr}stdout:\n{stdout}",
        out.status.code().unwrap_or(127)
    );
    freeze(&text, root)
}

fn freeze(text: &str, root: &Path) -> String {
    let text = text.replace(&root.display().to_string(), "<root>");
    let mut out = String::new();
    let mut rest = text.as_str();
    while !rest.is_empty() {
        if let Some((marker, next)) = skip_nonce(rest) {
            out.push_str(marker);
            rest = next;
            continue;
        }
        if let Some(len) = rfc3339_len(rest) {
            out.push_str("<time>");
            rest = &rest[len..];
            continue;
        }
        if let Some(len) = org_stamp_len(rest) {
            out.push_str("<stamp>");
            rest = &rest[len..];
            continue;
        }
        if let Some(len) = date_len(rest) {
            out.push_str("<date>");
            rest = &rest[len..];
            continue;
        }
        if let Some(len) = age_len(rest) {
            out.push_str("<age>");
            rest = &rest[len..];
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

fn skip_nonce(text: &str) -> Option<(&'static str, &str)> {
    for (prefix, marker) in [
        ("[unverified ", "[unverified <nonce>]"),
        ("[fence ", "[fence <nonce>]"),
        ("[end ", "[end <nonce>]"),
    ] {
        if let Some(body) = text.strip_prefix(prefix) {
            let hex = body.chars().take_while(|ch| ch.is_ascii_hexdigit()).count();
            if hex == 16 && body[hex..].starts_with(']') {
                let keep = prefix.len() + hex + 1;
                return Some((marker, &text[keep..]));
            }
        }
    }
    None
}

fn rfc3339_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || !date_at(bytes) || bytes[10] != b'T' {
        return None;
    }
    if !bytes[11..13].iter().all(u8::is_ascii_digit)
        || bytes[13] != b':'
        || !bytes[14..16].iter().all(u8::is_ascii_digit)
        || bytes[16] != b':'
        || !bytes[17..19].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    let mut len = 19;
    if bytes.get(len) == Some(&b'.') {
        len += 1;
        while bytes.get(len).is_some_and(u8::is_ascii_digit) {
            len += 1;
        }
    }
    if bytes.get(len) == Some(&b'Z') {
        return Some(len + 1);
    }
    None
}

fn org_stamp_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'[') || !date_at(bytes.get(1..)?) {
        return None;
    }
    text.find(']').map(|end| end + 1)
}

fn date_len(text: &str) -> Option<usize> {
    date_at(text.as_bytes()).then_some(10)
}

fn date_at(bytes: &[u8]) -> bool {
    bytes.len() >= 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

fn age_len(text: &str) -> Option<usize> {
    let digits = text.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let rest = &text[digits..];
    for unit in [
        " years ago",
        " year ago",
        " months ago",
        " month ago",
        " weeks ago",
        " week ago",
        " days ago",
        " day ago",
    ] {
        if rest.starts_with(unit) {
            return Some(digits + unit.len());
        }
    }
    None
}

fn snap(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(SNAP_DIR)
        .join(format!("{name}.txt"));
    if std::env::var_os("LJOS_UPDATE_LISTING").is_some() || !path.is_file() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(expected, actual, "{name} drifted\n{actual}");
}

fn atoms() -> Value {
    let today = chrono::Utc::now()
        .format("%Y-%m-%dT12:00:00.000Z")
        .to_string();
    let yesterday = (chrono::Utc::now().date_naive() - chrono::Duration::days(1))
        .format("%Y-%m-%dT12:00:00.000Z")
        .to_string();
    let mut rows = vec![
        atom(
            "idkiln01aaaaaaaaaaaaaaaaaaaaaaaa",
            "lesson",
            "The damper sticks when the kiln is cold.",
            &today,
            "alice",
            Some("demo-kiln"),
            0.96,
            Some((2, 3)),
        ),
        atom(
            "idkiln02bbbbbbbbbbbbbbbbbbbbbbbb",
            "lesson",
            "The damper sticks.\nOpen the flue before the kiln reaches temperature.\nÞetta er íslenskt: ofninn í Reykjavík þarf nýja þéttingu.",
            &yesterday,
            "alice",
            Some("demo-kiln"),
            0.91,
            Some((3, 3)),
        ),
        atom(
            "idkiln03cccccccccccccccccccccccc",
            "lesson",
            "The damper log is https://kiln.example/logs/2026/november/reykjavik/damper-and-thermocouple-readings/line-4/workshop-north/session-that-is-one-long-url-so-a-person-can-see-where-it-breaks",
            &today,
            "bob",
            Some("demo-tile"),
            0.88,
            Some((2, 3)),
        ),
        atom(
            "idkiln04dddddddddddddddddddddddd",
            "note",
            "SupercalifragilisticexpialidociousKilnDamperReadingThatExceedsEightyColumnsAndKeepsGoingFurtherStillUntilTheWordItselfPassesTheMark",
            &today,
            "alice",
            None,
            0.4,
            None,
        ),
    ];
    for n in 1..=12 {
        rows.push(atom(
            &format!("idglaze{n:02}eeeeeeeeeeeeeeeeeeeeeeee"),
            "lesson",
            &format!("Glaze batch {n} waits until the kiln is cool enough to open."),
            &today,
            "alice",
            Some("demo-tile"),
            0.8 - f64::from(n) * 0.01,
            Some((2, 2)),
        ));
    }
    rows.push(atom(
        "idother01ffffffffffffffffffffffff",
        "preference",
        "A ferry timetable is not a kiln schedule.",
        &today,
        "carol",
        None,
        0.2,
        None,
    ));
    json!({"atoms": rows})
}

#[allow(clippy::too_many_arguments)]
fn atom(
    id: &str,
    kind: &str,
    text: &str,
    ts: &str,
    seat: &str,
    issue: Option<&str>,
    score: f64,
    ballots: Option<(u32, u32)>,
) -> Value {
    let mut entities = vec![format!("seat:{seat}")];
    if let Some(issue) = issue {
        entities.push(format!("issue:{issue}"));
    }
    let mut row = json!({
        "id": id,
        "kind": kind,
        "text": text,
        "score": score,
        "ts": ts,
        "entities": entities,
        "origin": "agent-derived",
        "source": {"seat": seat, "harness": "cursor", "session": "sess-listing-pages", "at": ts, "basis": "agent"},
        "dense_rank": 1,
        "cosine": 0.84,
    });
    if let Some((hit, of)) = ballots {
        row["ballots"] = json!(hit);
        row["of"] = json!(of);
    }
    row
}

const ISSUES: &str = "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#B] Fire the kiln
:PROPERTIES:
:ID:         demo-kiln
:CREATED:    [2026-09-01 Tue]
:END:
:LOGBOOK:
- Note: \"The damper sticks when the morning is cold. Þéttingin er gömul.\" [2026-09-02 Wed 09:14]
:END:

* STARTED [#A] Glaze the tile after reading https://kiln.example/logs/2026/november/reykjavik/damper-and-thermocouple-readings/line-4/workshop-north/session-that-is-one-long-url
:PROPERTIES:
:ID:         demo-tile
:CREATED:    [2026-09-03 Thu]
:END:

* TODO [#C] Wait for the kiln
:PROPERTIES:
:ID:         demo-wait
:BLOCKED_BY: demo-kiln
:CREATED:    [2026-09-04 Fri]
:END:

* DONE [#B] Sweep the hearth
:PROPERTIES:
:ID:         demo-done
:CREATED:    [2026-08-01 Sat]
:END:
";

const VISSUE_STUB: &str = "\
#!/bin/sh
if [ \"$1\" = \"recall\" ]; then
  printf '%s\\n' \"* TODO Fire the kiln\" \"The damper sticks when the morning is cold.\"
  exit 0
fi
exit 0
";

#[test]
fn listing_commands_match_the_saved_pages() {
    let home = Home::open(atoms());
    let cases = [
        ("search-kiln", &["search", "kiln"][..]),
        ("search-glaze", &["search", "glaze", "-n", "3"][..]),
        ("search-none", &["search", "zzzznotaword"][..]),
        ("search-icelandic", &["search", "Reykjavík"][..]),
        ("memories-list", &["memories", "list", "--all"][..]),
        ("search-other-seat", &["search", "thermocouple"][..]),
        ("show-kiln", &["show", "demo-kiln"][..]),
        ("show-tile", &["show", "demo-tile"][..]),
        ("ready", &["ready"][..]),
        ("due", &["due"][..]),
        ("timeline", &["timeline", "demo-kiln"][..]),
        ("hubs", &["hubs", "-n", "4"][..]),
        ("island", &["island", "kiln"][..]),
        ("recall", &["recall", "demo-kiln"][..]),
        ("search-json", &["search", "thermocouple", "--json"][..]),
        ("recall-json", &["recall", "demo-kiln", "--json"][..]),
        (
            "memories-json",
            &["memories", "list", "--all", "--json"][..],
        ),
        ("ready-json", &["ready", "--json"][..]),
        ("show-json", &["show", "demo-kiln", "--json"][..]),
        ("due-json", &["due", "--json"][..]),
        ("timeline-json", &["timeline", "demo-kiln", "--json"][..]),
        ("hubs-json", &["hubs", "-n", "4", "--json"][..]),
        ("island-json", &["island", "kiln", "--json"][..]),
    ];
    for (name, args) in cases {
        let out = home.run(args);
        let text = page(&out, &home.root);
        assert!(out.status.success(), "{name} failed\n{text}");
        if !name.ends_with("-json") {
            let stdout = text
                .split_once("stdout:\n")
                .map(|(_, rest)| rest)
                .unwrap_or(&text);
            for line in stdout.lines() {
                assert!(
                    line.chars().count() <= 80,
                    "{name} is wider than 80 columns: {line}"
                );
            }
        }
        snap(name, &text);
        if name == "due" {
            assert!(
                text.contains("ljos graded ID") && text.contains("ljos forget ID"),
                "due output missing graded/forget hint:\n{text}"
            );
        }
    }
}

#[test]
fn non_tty_pages_match_saved_base_bytes_under_columns_variants() {
    let columns = [None, Some("60"), Some("0"), Some("garbage"), Some("999999")];
    let sequence: [(&str, &[&str]); 16] = [
        ("search-kiln", &["search", "kiln"][..]),
        ("search-glaze", &["search", "glaze", "-n", "3"][..]),
        ("search-none", &["search", "zzzznotaword"][..]),
        ("search-icelandic", &["search", "Reykjavík"][..]),
        ("memories-list", &["memories", "list", "--all"][..]),
        ("search-other-seat", &["search", "thermocouple"][..]),
        ("show-kiln", &["show", "demo-kiln"][..]),
        ("show-tile", &["show", "demo-tile"][..]),
        ("ready", &["ready"][..]),
        ("due", &["due"][..]),
        ("timeline", &["timeline", "demo-kiln"][..]),
        ("hubs", &["hubs", "-n", "4"][..]),
        ("island", &["island", "kiln"][..]),
        ("recall", &["recall", "demo-kiln"][..]),
        ("search-json", &["search", "thermocouple", "--json"][..]),
        ("recall-json", &["recall", "demo-kiln", "--json"][..]),
    ];
    let targets = [
        "search-kiln",
        "memories-list",
        "ready",
        "show-kiln",
        "timeline",
        "recall",
        "recall-json",
    ];
    for name in targets {
        let upto = sequence
            .iter()
            .position(|(n, _)| *n == name)
            .expect("target command missing from sequence");
        let want = std::fs::read_to_string(format!("{SNAP_DIR}/{name}.txt")).unwrap();
        for cols in columns {
            let home = Home::open(atoms());
            let mut out = None;
            for (_, args) in sequence.iter().take(upto + 1) {
                out = Some(home.run_with(args, cols));
            }
            let out = out.expect("target output missing");
            let got = page(&out, &home.root);
            assert_eq!(got, want, "name={name} columns={cols:?}");
        }
    }
}

#[test]
fn the_token_miss_on_stderr_wraps_to_the_stderr_terminal() {
    let miss = ljos_cli::SEARCH_TOKEN_MISS;
    let fixed = ljos_cli::scan::wrap_for(miss, ljos_cli::scan::Width::FIXED);
    let narrow = ljos_cli::output_wrap::wrap_prose(miss, Some(40));
    assert_ne!(fixed, narrow);

    let home = Home::open(atoms());
    let term = pty::Pty::open();
    let mut cmd = home.command(&["recall", "demo-kiln"], Some("40"));
    cmd.stdout(Stdio::piped());
    term.attach_stderr(&mut cmd);
    let child = term.spawn(&mut cmd);
    let out = term.wait_output(child, 20);
    term.finish_reading();
    let shown = term.shown().replace("\r\n", "\n");
    assert!(out.status.success(), "{shown}");
    assert!(shown.contains(&narrow), "{shown}");

    let home = Home::open(atoms());
    let term = pty::Pty::open();
    let mut cmd = home.command(&["recall", "demo-kiln"], Some("40"));
    cmd.stderr(Stdio::piped());
    term.attach_stdout(&mut cmd);
    let child = term.spawn(&mut cmd);
    let out = term.wait_output(child, 20);
    term.finish_reading();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains(&fixed), "{stderr}");
}

#[test]
fn a_fault_line_on_stderr_wraps_to_the_stderr_terminal() {
    let args = ["show", "demo-nosuchticket"];
    let home = Home::open(atoms());
    let piped = home.run(&args);
    assert!(!piped.status.success());
    let line = String::from_utf8_lossy(&piped.stderr)
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    let cols = 30;
    let once = ljos_cli::output_wrap::wrap_prose(&line, Some(cols));
    assert!(once.lines().count() > 1, "{line}");

    let term = pty::Pty::open();
    let mut cmd = home.command(&args, Some(&cols.to_string()));
    cmd.stdout(Stdio::piped());
    term.attach_stderr(&mut cmd);
    let child = term.spawn(&mut cmd);
    let out = term.wait_output(child, 20);
    term.finish_reading();
    let shown = term.shown().replace("\r\n", "\n");
    assert!(!out.status.success(), "{shown}");
    assert!(shown.starts_with(&format!("{once}\n")), "{line}\n{shown}");
}

#[test]
fn a_told_line_on_stdout_wraps_to_the_stdout_terminal() {
    let home = Home::open(atoms());
    let empty = tempfile::tempdir().unwrap();
    let dir = empty.path().display().to_string();
    let args = ["cards", "--dir", dir.as_str()];
    let piped = home.run(&args);
    let line = String::from_utf8_lossy(&piped.stdout).into_owned();
    assert!(line.starts_with("no USER.md or MEMORY.md"), "{line}");
    let cols = 30;
    let once = ljos_cli::output_wrap::wrap_prose(&line, Some(cols));
    assert!(once.lines().count() > 2, "{line}");

    let term = pty::Pty::open();
    let mut cmd = home.command(&args, Some(&cols.to_string()));
    cmd.stderr(Stdio::piped());
    term.attach_stdout(&mut cmd);
    let child = term.spawn(&mut cmd);
    let out = term.wait_output(child, 20);
    term.finish_reading();
    let shown = term.shown().replace("\r\n", "\n");
    assert!(out.status.success(), "{shown}");
    assert_eq!(shown, once);
}

/// A line that may run past the width: one URL, path, code span or fence
/// marker that the wrapper keeps whole, under its indent and quote mark.
fn unbreakable(line: &str) -> bool {
    let body = line.trim_start();
    let body = body.strip_prefix("| ").unwrap_or(body);
    let body = body.strip_prefix("- ").unwrap_or(body);
    if body.starts_with("[unverified ") || body.starts_with("[end ") {
        return true;
    }
    let body = body.trim_end_matches(['.', ',', ':', ';']);
    !body.contains(char::is_whitespace)
        || (body.starts_with('`')
            && body.ends_with('`')
            && body[1..body.len() - 1].find('`').is_none())
}

/// The listing commands a person runs at a terminal.
const TERMINAL_CASES: [&[&str]; 14] = [
    &["search", "kiln"],
    &["search", "glaze", "-n", "3"],
    &["search", "zzzznotaword"],
    &["search", "Reykjavík"],
    &["memories", "list", "--all"],
    &["search", "thermocouple"],
    &["show", "demo-kiln"],
    &["show", "demo-tile"],
    &["ready"],
    &["due"],
    &["timeline", "demo-kiln"],
    &["hubs", "-n", "4"],
    &["island", "kiln"],
    &["recall", "demo-kiln"],
];

/// What `args` prints with stdout on a terminal `cols` columns wide.
fn on_terminal(home: &Home, args: &[&str], cols: usize) -> String {
    let term = pty::Pty::open();
    let mut cmd = home.command(args, Some(&cols.to_string()));
    cmd.stderr(Stdio::piped());
    term.attach_stdout(&mut cmd);
    let child = term.spawn(&mut cmd);
    let out = term.wait_output(child, 20);
    term.finish_reading();
    let shown = term.shown().replace("\r\n", "\n");
    assert!(out.status.success(), "{args:?}\n{shown}");
    shown
}

#[test]
fn listing_pages_on_a_terminal_fit_the_terminal_width() {
    let home = Home::open(atoms());
    let mut over = Vec::new();
    for cols in [40usize, 24] {
        for args in TERMINAL_CASES {
            let shown = on_terminal(&home, args, cols);
            for line in shown.lines() {
                if ljos_cli::output_wrap::prose_display_width(line) > cols && !unbreakable(line) {
                    over.push(format!("{args:?} at {cols}: {line:?}"));
                }
            }
        }
    }
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// Each fence nonce as sixteen zeros, even where a wrap split it from its
/// marker. A nonce is fresh on each run, and the zeros keep its width.
fn zero_nonces(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |b: &u8| b.is_ascii_digit() || (b'a'..=b'f').contains(b);
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let bounded = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if bounded
            && bytes.len() >= i + 16
            && bytes[i..i + 16].iter().all(hex)
            && bytes.get(i + 16).is_none_or(|b| !b.is_ascii_alphanumeric())
        {
            out.push_str(&"0".repeat(16));
            i += 16;
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// The indent and quote mark a wrapped line repeats on its continuations.
fn line_prefix(line: &str) -> &str {
    let spaces = line.len() - line.trim_start_matches(' ').len();
    if line[spaces..].starts_with("| ") {
        &line[..spaces + 2]
    } else {
        &line[..spaces]
    }
}

fn visible(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

/// The fence markers a timeline row keeps whole, each with its nonce.
const MARKERS: [&str; 2] = ["[unverified ", "[end "];

/// `line` wrapped once at `cols` under `first` and `cont`. A line with a
/// fence marker is wrapped greedily word by word, the marker one word.
fn wrap_once(line: &str, cols: usize, first: &str, cont: &str) -> String {
    use ljos_cli::output_wrap::{prose_display_width as width, wrap_hanging_prose};
    if !MARKERS.iter().any(|marker| line.contains(marker)) {
        return wrap_hanging_prose(line, cols, first, cont);
    }
    let mut words: Vec<String> = Vec::new();
    for word in line.split_whitespace() {
        match words.last_mut() {
            Some(last) if MARKERS.iter().any(|m| m.trim_end() == last) => {
                last.push(' ');
                last.push_str(word);
            }
            _ => words.push(word.to_string()),
        }
    }
    let mut lines = vec![first.to_string()];
    let mut bare = true;
    for word in words {
        let fits = width(lines.last().unwrap()) + 1 + width(&word) <= cols;
        if !bare && !fits {
            lines.push(cont.to_string());
            bare = true;
        }
        let current = lines.last_mut().unwrap();
        if !bare {
            current.push(' ');
        }
        current.push_str(&word);
        bare = false;
    }
    lines.join("\n")
}

/// The other layout a builder picks when the one-line form of `line` does
/// not fit `cols`, wrapped once at `cols`. A count line puts its command on
/// the next line. A ticket's head line puts the title under it.
fn other_layout(line: &str, cols: usize) -> Option<String> {
    if let Some((counts, rest)) = line.split_once(". Run ") {
        if counts.starts_with("shown ") {
            let (cmd, tail) = rest.rsplit_once(" to see ")?;
            let lead = format!("{counts}. To see {}, run:", tail.trim_end_matches('.'));
            return Some(format!(
                "{}\n{cmd}",
                ljos_cli::output_wrap::wrap_prose(&lead, Some(cols))
            ));
        }
    }
    let parts: Vec<&str> = line.splitn(4, "  ").collect();
    let state = parts.get(1)?;
    if parts[0].contains(' ') || !state.chars().all(|ch| ch.is_ascii_uppercase()) {
        return None;
    }
    let (head, title) = match parts.get(2) {
        Some(prio) if prio.starts_with("[#") => {
            (parts[..3].join("  "), parts.get(3).copied().unwrap_or(""))
        }
        _ => (
            parts[..2].join("  "),
            &line[parts[0].len() + state.len() + 4..],
        ),
    };
    Some(format!(
        "{}\n{}",
        ljos_cli::output_wrap::wrap_prose(&head, Some(cols)),
        wrap_once(title, cols, "  ", "  ")
    ))
}

/// Whether `narrow` is `wide` with each line wrapped once at `cols`.
///
/// A line of `wide` is a line of the page before it is wrapped. It maps
/// to the run of `narrow` lines that hold the same visible text, and that
/// run must be the line wrapped once at `cols`, under the line's own prefix
/// and the continuation's prefix, or [`other_layout`] of the line. A page
/// wrapped at 80 columns and again at `cols` breaks where the first wrap
/// did, so its runs are not one wrap of the line.
fn wrapped_once(wide: &str, narrow: &str, cols: usize) -> Result<(), String> {
    let lines: Vec<&str> = narrow.lines().collect();
    let mut at = 0;
    for line in wide.lines() {
        if let Some(other) = other_layout(line, cols) {
            let other: Vec<&str> = other.lines().collect();
            if lines.get(at..at + other.len()) == Some(&other[..]) {
                at += other.len();
                continue;
            }
        }
        let want = visible(line);
        let mut run: Vec<&str> = Vec::new();
        let mut got = String::new();
        while run.is_empty() || got.len() < want.len() {
            let Some(next) = lines.get(at) else {
                return Err(format!("the page ends before {line:?}"));
            };
            at += 1;
            got.push_str(&visible(if run.is_empty() {
                next
            } else {
                &next[line_prefix(next).len()..]
            }));
            run.push(next);
        }
        if got != want {
            return Err(format!("{line:?} is not the text of {run:?}"));
        }
        if run.len() == 1 {
            if run[0].trim_end() != line.trim_end() {
                return Err(format!("{line:?} became {:?}", run[0]));
            }
            continue;
        }
        let first = line_prefix(line);
        let once = wrap_once(&line[first.len()..], cols, first, line_prefix(run[1]));
        let shown = run.join("\n");
        if once != shown {
            return Err(format!(
                "{line:?} wrapped once at {cols} is\n{once}\nbut the page has\n{shown}"
            ));
        }
    }
    match lines[at..].iter().find(|line| !line.trim().is_empty()) {
        Some(line) => Err(format!("{line:?} is past the end of the wide page")),
        None => Ok(()),
    }
}

#[test]
fn the_wrap_once_check_refuses_a_page_wrapped_twice() {
    let wide = "Glaze batch 1 waits until the kiln is cool enough to open, and the damper log says the flue was shut for the night.\n";
    for cols in [24usize, 30] {
        let once = ljos_cli::output_wrap::wrap_prose(wide, Some(cols));
        assert_eq!(wrapped_once(wide, &once, cols), Ok(()), "{once}");
        let fixed = ljos_cli::scan::wrap_for(wide, ljos_cli::scan::Width::FIXED);
        let twice = ljos_cli::output_wrap::wrap_prose(&fixed, Some(cols));
        assert_ne!(once, twice);
        assert!(wrapped_once(wide, &twice, cols).is_err(), "{twice}");
    }
}

/// The cases whose fixtures hold a line of prose longer than [`ljos_cli::scan::COLS`].
const LONG_LINE_CASES: [&[&str]; 8] = [
    &["memories", "list", "--all"],
    &["search", "thermocouple"],
    &["show", "demo-tile"],
    &["ready"],
    &["due"],
    &["timeline", "demo-kiln"],
    &["hubs", "-n", "4"],
    &["island", "kiln"],
];

#[test]
fn listing_pages_on_a_terminal_are_wrapped_once_at_its_width() {
    let home = Home::open(atoms());
    let mut faults = Vec::new();
    for args in TERMINAL_CASES {
        let wide = zero_nonces(&on_terminal(&home, args, ljos_cli::output_wrap::MAX_COLS));
        let longest = wide
            .lines()
            .filter(|line| line.split_whitespace().count() > 2)
            .map(ljos_cli::output_wrap::prose_display_width)
            .max()
            .unwrap_or(0);
        if LONG_LINE_CASES.contains(&args) && longest <= ljos_cli::scan::COLS {
            faults.push(format!(
                "{args:?} wide page is wrapped at 80 columns:\n{wide}"
            ));
        }
        for cols in [24usize, 40] {
            let narrow = zero_nonces(&on_terminal(&home, args, cols));
            if let Err(why) = wrapped_once(&wide, &narrow, cols) {
                faults.push(format!("{args:?} at {cols}: {why}"));
            }
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n\n"));
}
