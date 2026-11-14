//! What a person reads from the commands that list memories and tickets.
//!
//! A temporary pack and a temporary tracker hold short claims, a long
//! claim, a claim with Icelandic letters, a claim that is one long word,
//! a claim that is a long URL, enough hits to pass a small limit, and a
//! query that matches nothing. The files under `listing_pages/` are that
//! output with the clock, the fence nonce and this directory folded out,
//! so the shape stays put when the day changes.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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
        std::fs::write(
            root.join("tracker/vissue.toml"),
            "prefix = \"Software\"\n",
        )
        .unwrap();
        std::fs::write(root.join("tracker/Software/demo/issues.org"), ISSUES).unwrap();
        let bin = root.join("home/.local/bin/vissue");
        std::fs::write(&bin, VISSUE_STUB).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || serve(listener, atoms));
        Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        let home = self.root.join("home");
        let path = format!("{}:/usr/bin:/bin", home.join(".local/bin").display());
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
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
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")))
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
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(expected, actual, "{name} drifted\n{actual}");
}

fn atoms() -> Value {
    let today = chrono::Utc::now().format("%Y-%m-%dT12:00:00.000Z").to_string();
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
    ];
    for (name, args) in cases {
        let out = home.run(args);
        let text = page(&out, &home.root);
        assert!(
            out.status.success(),
            "{name} failed\n{text}"
        );
        snap(name, &text);
    }
}
