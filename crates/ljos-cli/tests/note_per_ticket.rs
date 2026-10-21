//! The same sentence noted on different tickets is a different memory.
//! Recall on a ticket finds the note written there.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde_json::Value;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let mut need = 0usize;
        for line in buf[..at].split(|b| *b == b'\n') {
            let line = std::str::from_utf8(line).unwrap_or("").trim();
            if let Some(v) = line
                .split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .map(|(_, v)| v.trim())
            {
                need = v.parse().unwrap_or(0);
            }
        }
        if buf.len().saturating_sub(at + 4) >= need {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn targets(line: &str, method: &str, path: &str) -> bool {
    let Some(rest) = line.strip_prefix(method) else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix(path) else {
        return false;
    };
    rest.starts_with([' ', '?'])
}

fn slot(atom: &Value, key: &str) -> String {
    atom.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

struct Pack {
    /// Compare `issue` as well as text, kind and set.
    honor_issue: bool,
    /// Kind `note` is refused, as a pack from before that kind.
    reject_note: bool,
    atoms: Mutex<Vec<Value>>,
}

impl Pack {
    fn answer(&self, req: &str) -> (u16, String) {
        let line = req.lines().next().unwrap_or("");
        if targets(line, "GET", "/v1/atoms") {
            let atoms = self.atoms.lock().unwrap().clone();
            return (200, serde_json::json!({"atoms": atoms}).to_string());
        }
        if targets(line, "GET", "/v1/search") {
            return (200, r#"{"hits":[]}"#.into());
        }
        if !targets(line, "POST", "/v1/atoms") {
            return (200, "{}".into());
        }
        let body = req.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        let Ok(mut atom) = serde_json::from_str::<Value>(body) else {
            return (400, r#"{"error":"bad json"}"#.into());
        };
        if self.reject_note && slot(&atom, "kind") == "note" {
            return (400, r#"{"error":"unknown atom kind: note"}"#.into());
        }
        let mut atoms = self.atoms.lock().unwrap();
        if let Some(existing) = atoms.iter().find(|kept| self.same(kept, &atom)) {
            return (200, existing.to_string());
        }
        let id = format!("{:08x}{:024x}", atoms.len() as u64 + 1, 0u64);
        atom["id"] = Value::String(id);
        atoms.push(atom.clone());
        (200, atom.to_string())
    }

    fn same(&self, kept: &Value, atom: &Value) -> bool {
        slot(kept, "text") == slot(atom, "text")
            && slot(kept, "kind") == slot(atom, "kind")
            && slot(kept, "set") == slot(atom, "set")
            && (!self.honor_issue || slot(kept, "issue") == slot(atom, "issue"))
    }
}

fn serve(pack: Arc<Pack>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let req = read_request(&mut stream);
            let (code, body) = pack.answer(&req);
            let status = if code == 200 {
                "200 OK"
            } else {
                "400 Bad Request"
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

struct Hab {
    dir: tempfile::TempDir,
}

impl Hab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        script(
            dir.path(),
            "vissue",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\nexit 0\n",
                log.display()
            ),
        );
        script(dir.path(), "claimdag", "#!/bin/sh\nexit 1\n");
        for name in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        Self { dir }
    }

    fn ljos(&self, pack: &str, args: &[&str]) -> std::process::Output {
        let path = format!(
            "{}:{}",
            self.dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", self.dir.path().join("home"))
            .env(
                "XDG_CONFIG_HOME",
                self.dir.path().join("home").join(".config"),
            )
            .env(
                "XDG_STATE_HOME",
                self.dir.path().join("home").join(".state"),
            )
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("VISSUE_ROOT", self.dir.path().join("tracker"))
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_TIMEOUT_MS", "2000")
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_URL", pack)
            .output()
            .unwrap()
    }

    fn note(&self, pack: &str, issue: &str, text: &str) -> std::process::Output {
        self.ljos(pack, &["note", issue, text])
    }

    fn recall(&self, pack: &str, issue: &str) -> String {
        let out = self.ljos(pack, &["recall", issue]);
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "ljos recall {issue} failed: {stdout} {}",
            String::from_utf8_lossy(&out.stderr)
        );
        stdout
    }
}

fn memory_id(stdout: &str) -> String {
    let mut ids = Vec::new();
    for line in stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix("noted in memory (id ") {
            ids.push(rest.trim_end_matches(')').to_string());
        }
    }
    assert_eq!(ids.len(), 1, "{stdout}");
    ids.pop().unwrap()
}

fn notes_stay_apart(pack: &Pack, url: &str) {
    let hab = Hab::new();
    let sentence = "Repro for ljos-l9qq: this exact sentence goes on two tickets.";
    let second = "The second note names the file that failed.";
    let tickets = [
        "ljos-t1aa",
        "ljos-t2bb",
        "ljos-t3cc",
        "ljos-t4dd",
        "9aaa-bbbb",
    ];
    let mut ids = Vec::new();
    for ticket in tickets {
        let out = hab.note(url, ticket, sentence);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "ljos note {ticket} failed: {stdout} {out:?}"
        );
        assert!(
            !stdout.contains("warning:"),
            "the note was not stored: {stdout}"
        );
        ids.push(memory_id(&stdout));
    }
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        tickets.len(),
        "one memory id for different tickets: {ids:?}"
    );
    let again = hab.note(url, tickets[0], sentence);
    let stdout = String::from_utf8_lossy(&again.stdout);
    assert!(again.status.success(), "{stdout}");
    assert_eq!(
        memory_id(&stdout),
        ids[0],
        "the same note on the same ticket stored another memory: {stdout}"
    );

    let extra = hab.note(url, tickets[0], second);
    let stdout = String::from_utf8_lossy(&extra.stdout);
    assert!(extra.status.success(), "{stdout}");
    let extra_id = memory_id(&stdout);
    assert!(
        !ids.contains(&extra_id),
        "a second sentence reused the first memory"
    );

    let stored = pack.atoms.lock().unwrap().clone();
    for ticket in tickets {
        assert!(
            stored
                .iter()
                .any(|atom| { slot(atom, "issue") == ticket && slot(atom, "text") == sentence }),
            "{ticket} has no memory of its note: {stored:?}"
        );
        let said = hab.recall(url, ticket);
        assert!(
            said.contains(sentence),
            "recall on {ticket} missed its note:\n{said}"
        );
    }
    let first = hab.recall(url, tickets[0]);
    assert!(
        first.contains(second),
        "recall missed the second note:\n{first}"
    );
    let other = hab.recall(url, tickets[1]);
    assert!(
        !other.contains(second),
        "recall on {} listed another ticket's note:\n{other}",
        tickets[1]
    );
    let none = hab.recall(url, "ljos-none");
    assert!(
        !none.contains(sentence),
        "recall on a ticket with no note listed one:\n{none}"
    );
}

/// ljos-l9qq: a pack that folds the same text into one memory, and that
/// does not know kind `note`, still keeps each ticket's note.
#[test]
fn the_same_sentence_on_different_tickets_is_a_different_memory() {
    let pack = Arc::new(Pack {
        honor_issue: false,
        reject_note: true,
        atoms: Mutex::new(Vec::new()),
    });
    let url = serve(Arc::clone(&pack));
    notes_stay_apart(&pack, &url);
}

/// A pack that keeps two issues apart stores each note on the first post.
#[test]
fn a_pack_that_reads_issue_stores_each_note() {
    let pack = Arc::new(Pack {
        honor_issue: true,
        reject_note: false,
        atoms: Mutex::new(Vec::new()),
    });
    let url = serve(Arc::clone(&pack));
    notes_stay_apart(&pack, &url);
}
