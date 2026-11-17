//! Ownership on a search page comes from the tracker, not from a missing tag.
//!
//! Seat readcon-paper noted `readcon-6zv0` and had filed it, and filed
//! `readcon-4ms0`, neither claimed. A search for the release notes put other
//! seats' untagged tickets first. Seat bounty works in `d-SEAMS`. A search
//! for that video work put OmniPotentRPC docs tickets, which only share
//! generic words, above the d-SEAMS rows.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

const READCON: &str = "readcon 0.23.3 release scaling records";
const SEAMS: &str = "d-SEAMS migrate video docs embed";

struct Seat {
    root: PathBuf,
    url: String,
    name: String,
}

impl Seat {
    fn open(name: &str, files: &[(&str, &str)], hits: String) -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        for (project, body) in files {
            let dir = root.join("tracker/Software").join(project);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("issues.org"), body).unwrap();
        }
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/share",
            "run",
            "work",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || serve(listener, hits));
        Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
            name: name.to_string(),
        }
    }

    fn search(&self, args: &[&str]) -> Output {
        let home = self.root.join("home");
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", &self.name)
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_SEARCH_WAIT", "0")
            .env("LJOS_SEARCH_WARMUP_WAIT", "0")
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

fn ticket(id: &str, title: &str, score: &str) -> String {
    format!(
        r#"{{"id":"{id}","kind":"ticket","text":"{text}","score":{score},"cosine":0.91,"dense_rank":1,"entities":["issue:{id}"],"ballots":1,"of":1}}"#,
        text = title.replace('"', "'"),
    )
}

fn serve(listener: TcpListener, hits: String) {
    let body = format!(r#"{{"hits":[{hits}],"rank":"relevance"}}"#);
    let status = r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#;
    for conn in listener.incoming().flatten() {
        let body = body.clone();
        let status = status.to_string();
        thread::spawn(move || {
            let mut sock = conn;
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            while !buf.windows(4).any(|window| window == b"\r\n\r\n") {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                }
            }
            let head_end = buf
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap()
                + 4;
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let request = head.split_whitespace().nth(1).unwrap_or("/");
            let path = request.split('?').next().unwrap_or("/");
            let payload = if path == "/v1/search" {
                body
            } else if path == "/v1/status" {
                status
            } else if path == "/v1/atoms" {
                r#"{"atoms":[{"id":"lesson-1","kind":"lesson","text":"kept"}]}"#.to_string()
            } else {
                r#"{"ok":true}"#.to_string()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        });
    }
}

fn text(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn heading(title: &str, id: &str, log: &str, body: &str) -> String {
    format!(
        "\
* TODO [#B] {title}
:PROPERTIES:
:ID:         {id}
:END:
:LOGBOOK:
{log}\
:END:
{body}
"
    )
}

fn before(page: &str, earlier: &str, later: &str) {
    let at = page
        .find(earlier)
        .unwrap_or_else(|| panic!("{earlier} missing:\n{page}"));
    let other = page
        .find(later)
        .unwrap_or_else(|| panic!("{later} missing:\n{page}"));
    assert!(at < other, "{earlier} should come before {later}:\n{page}");
}

#[test]
fn a_filed_unclaimed_ticket_with_a_fresh_note_outranks_untagged_generic_rows() {
    let phrase = READCON;
    let noted = heading(
        "Archive the seat notes",
        "readcon-6zv0",
        "\
- Noted by readcon-paper [2026-11-16 Mon 18:30]
- Note: \"readcon 0.23.3 release scaling records were checked\" [2026-11-16 Mon 18:29]
- Filed by readcon-paper [2026-11-01 Sun 09:00]
",
        "The readcon 0.23.3 release scaling records are in the note.\n",
    );
    let filed = heading(
        "Keep the older archive",
        "readcon-4ms0",
        "- Filed by readcon-paper [2026-11-01 Sun 09:00]\n",
        "readcon 0.23.3 release scaling records from the earlier pass.\n",
    );
    let foreign = [
        ("HaoZeke-6il2", "Release records index"),
        ("HaoZeke-c64s", "Release scaling notes"),
        ("OmniPotentRPC-1fs8", "Docs release video"),
        ("HaoZeke-aa01", "Video release records"),
        ("HaoZeke-aa02", "Embed the release docs"),
        ("HaoZeke-aa03", "Records for the release video"),
    ];
    let exact = heading(
        phrase,
        "HaoZeke-0ex1",
        "",
        "Another seat wrote this title.\n",
    );
    let readcon = format!(
        "\
#+TITLE: readcon issues
#+VISSUE: 1
#+CATEGORY: readcon
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

{noted}
{filed}"
    );
    let mut hao = String::from(
        "\
#+TITLE: HaoZeke issues
#+VISSUE: 1
#+CATEGORY: HaoZeke
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

",
    );
    hao.push_str(&exact);
    let mut omni = String::from(
        "\
#+TITLE: OmniPotentRPC issues
#+VISSUE: 1
#+CATEGORY: OmniPotentRPC
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

",
    );
    for (id, title) in foreign {
        let block = heading(title, id, "", "A generic page.\n");
        if id.starts_with("Omni") {
            omni.push_str(&block);
        } else {
            hao.push_str(&block);
        }
    }
    let mut hits = vec![ticket("HaoZeke-0ex1", phrase, "0.99")];
    for (index, (id, title)) in foreign.iter().enumerate() {
        let score = format!("{:.2}", 0.95 - index as f64 * 0.02);
        hits.push(ticket(id, title, &score));
    }
    let seat = Seat::open(
        "readcon-paper",
        &[
            ("readcon", &readcon),
            ("HaoZeke", &hao),
            ("OmniPotentRPC", &omni),
        ],
        hits.join(","),
    );
    let out = seat.search(&["search", phrase, "-n", "6"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(ljos_cli::FUSED_RANK_LINE),
        "the page was not ranked by meaning:\n{said}"
    );
    assert!(
        stdout.contains("This seat's rows come before another seat's."),
        "the header does not say what the order does:\n{said}"
    );
    before(&stdout, "HaoZeke-0ex1", "readcon-6zv0");
    before(&stdout, "readcon-6zv0", "readcon-4ms0");
    for id in ["HaoZeke-6il2", "HaoZeke-c64s", "OmniPotentRPC-1fs8"] {
        if stdout.contains(id) {
            before(&stdout, "readcon-4ms0", id);
        }
    }
    assert!(
        stdout.contains("HaoZeke-6il2")
            || stdout.contains("HaoZeke-c64s")
            || stdout.contains("OmniPotentRPC-1fs8"),
        "an untagged ticket from another project should stay on the page, below:\n{said}"
    );
}

#[test]
fn a_project_this_seat_works_in_outranks_generic_docs_tickets() {
    let noted = heading(
        "Capture the walk",
        "d-SEAMS-vid1",
        "\
- Noted by bounty [2026-11-16 Mon 18:40]
- Note: \"d-SEAMS migrate video docs embed is the row to cut\" [2026-11-16 Mon 18:40]
- Filed by bounty [2026-10-02 Fri 11:00]
",
        "d-SEAMS migrate video docs embed stays with this seat.\n",
    );
    let sibling = heading(
        "Show the d-SEAMS video",
        "d-SEAMS-vid2",
        "",
        "The walk is the video for the project.\n",
    );
    let seams = format!(
        "\
#+TITLE: d-SEAMS issues
#+VISSUE: 1
#+CATEGORY: d-SEAMS
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

{noted}
{sibling}"
    );
    let omni_rows = [
        ("cpmdc-u6t1", "Embed the docs video"),
        ("rgmin-js40", "Write the docs for the video"),
        ("cpmdc-u6t2", "Release docs video page"),
        ("rgmin-js41", "Docs video embed notes"),
    ];
    let mut omni = String::from(
        "\
#+TITLE: OmniPotentRPC issues
#+VISSUE: 1
#+CATEGORY: OmniPotentRPC
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

",
    );
    let mut hits = Vec::new();
    for (index, (id, title)) in omni_rows.iter().enumerate() {
        omni.push_str(&heading(title, id, "", "Docs for another project.\n"));
        omni.push('\n');
        let score = format!("{:.2}", 0.96 - index as f64 * 0.02);
        hits.push(ticket(id, title, &score));
    }
    let seat = Seat::open(
        "bounty",
        &[("d-SEAMS", &seams), ("OmniPotentRPC", &omni)],
        hits.join(","),
    );
    let out = seat.search(&["search", SEAMS, "-n", "6"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(ljos_cli::FUSED_RANK_LINE), "{said}");
    assert!(
        stdout.contains("This seat's rows come before another seat's."),
        "{said}"
    );
    before(&stdout, "d-SEAMS-vid1", "d-SEAMS-vid2");
    before(&stdout, "d-SEAMS-vid2", "cpmdc-u6t1");
    before(&stdout, "d-SEAMS-vid2", "rgmin-js40");
}
