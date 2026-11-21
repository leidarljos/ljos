//! A healthy packsetd ranks by meaning, even when the seat's local snapshot
//! is full of rows that share the query. A writer that is busy with other
//! seats says why and names the command to run next.

#![allow(
    clippy::disallowed_macros,
    reason = "a test writes its notes to the test harness, not to a person"
)]

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const LESSON: &str = "The ferry timetable lists Oak Tree at dawn.";

fn packsetd_bin() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join("packsetd"))
        .find(|candidate| candidate.is_file())
}

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

struct Seat {
    home: PathBuf,
    pack: PathBuf,
    url: String,
    path: String,
}

impl Seat {
    fn open(daemon: &ljos_test_boot::Packsetd) -> Self {
        let bin_dir = Path::new(env!("CARGO_BIN_EXE_ljos"))
            .parent()
            .expect("bin dir");
        let path = std::env::join_paths([bin_dir, Path::new("/usr/bin"), Path::new("/bin")])
            .expect("PATH")
            .into_string()
            .expect("utf-8 PATH");
        Self {
            home: daemon.home().to_path_buf(),
            pack: daemon.pack_home().to_path_buf(),
            url: daemon.url().to_string(),
            path,
        }
    }

    fn run(&self, seat: &str, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", &self.path)
            .env("LANG", "C.UTF-8")
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_HOME", &self.pack)
            .env("LJOS_SEAT", seat)
            .env("LJOS_TRACKER_GIT", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("ljos")
    }
}

fn embed_stub(path: &Path, delay: &str) {
    // An empty marker sleeps on every encode. A marker sleeps only when
    // that text is in the request, so earlier encodes can bring the
    // encoder up. A sleep on the first encode is reported as the encoder
    // still loading, not as a busy writer.
    write_embed_stub(path, delay, "");
}

fn write_embed_stub(path: &Path, delay: &str, hold_marker: &str) {
    let flag = path.with_file_name("embed-held");
    let body = format!(
        "#!/usr/bin/env python3\n\
         import json, sys, time\n\
         delay = {delay}\n\
         marker = {hold_marker:?}\n\
         flag = {flag:?}\n\
         for line in sys.stdin:\n\
         \tline = line.strip()\n\
         \tif not line:\n\
         \t\tcontinue\n\
         \tif marker and marker in line:\n\
         \t\topen(flag, \"w\").close()\n\
         \t\ttime.sleep(delay)\n\
         \telif delay and not marker:\n\
         \t\ttime.sleep(delay)\n\
         \ttry:\n\
         \t\tasked = json.loads(line)\n\
         \texcept json.JSONDecodeError:\n\
         \t\tcontinue\n\
         \tif \"candidates\" in asked or \"q\" in asked:\n\
         \t\tn = len(asked.get(\"candidates\") or asked.get(\"d\") or [])\n\
         \t\tprint(json.dumps({{\"scores\": [0.8] * max(n, 1)}}), flush=True)\n\
         \telse:\n\
         \t\tprint(json.dumps({{\"v\": [0.2, 0.4, 0.1]}}), flush=True)\n"
    );
    ljos_cli::install_executable(path, body).unwrap();
}

fn local_rows(home: &Path, n: usize) {
    let kept = home.join(".local/state/ljos/memory/kept");
    std::fs::create_dir_all(&kept).unwrap();
    for i in 0..n {
        let text = format!("ferry timetable note {i} about the docks and the oak");
        let row = serde_json::json!({
            "id": format!("local:note-{i}"),
            "text": text,
            "kind": "lesson",
            "labels": ["stored-local"],
            "entities": ["seat:ada"],
            "machine": "box",
            "seat": "ada",
            "at": "2026-11-01T00:00:00Z",
            "retired": false,
        });
        std::fs::write(
            kept.join(format!("{i}.json")),
            serde_json::to_vec_pretty(&row).unwrap(),
        )
        .unwrap();
    }
}

/// One pack home, a token file, several seats, and a local snapshot that
/// shares the query. The writer's hit stays, and the ranking is semantic.
#[test]
fn a_healthy_writer_ranks_by_meaning() {
    let Some(bin) = packsetd_bin() else {
        eprintln!("skipped: packsetd is not on PATH");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    for sub in [
        "tmp",
        "run",
        ".config",
        ".local/state",
        ".local/share/packset",
    ] {
        std::fs::create_dir_all(home.path().join(sub)).unwrap();
    }
    let stub = home.path().join("packset-embed");
    embed_stub(&stub, "0");
    let mut started = ljos_test_boot::Packsetd::build();
    started.bin(&bin);
    started.home_at(home.path());
    started.pack_at(home.path().join(".local/share/packset"));
    started.command().env("PACKSET_EMBED", &stub);
    let daemon = started.spawn();
    let seat = Seat::open(&daemon);
    assert!(
        seat.pack.join("token").is_file(),
        "the writer did not leave a token file"
    );
    for name in ["ada", "bob", "cara"] {
        let remembered = seat.run(
            name,
            &["remember", &format!("Seat {name} keeps a note: {LESSON}")],
        );
        assert!(
            remembered.status.success(),
            "{name} remember: {}",
            text(&remembered)
        );
    }
    local_rows(home.path(), 24);
    let search = seat.run("ada", &["search", "ferry timetable Oak Tree"]);
    let said = text(&search);
    assert!(search.status.success(), "{said}");
    let stderr = String::from_utf8_lossy(&search.stderr);
    let first = stderr.lines().find(|line| !line.trim().is_empty());
    assert_eq!(first, Some(ljos_cli::FUSED_RANK_LINE), "{said}");
    assert!(
        String::from_utf8_lossy(&search.stdout).contains(LESSON),
        "the pack lesson was dropped for the local snapshot:\n{said}"
    );
    let as_json = seat.run("ada", &["search", "--json", "ferry timetable Oak Tree"]);
    let json_out = String::from_utf8_lossy(&as_json.stdout);
    let json_err = String::from_utf8_lossy(&as_json.stderr);
    assert!(as_json.status.success(), "{json_out}{json_err}");
    assert!(
        json_err.trim().is_empty(),
        "json search printed a mode line: {json_err}"
    );
    let doc: serde_json::Value = serde_json::from_str(json_out.trim())
        .unwrap_or_else(|err| panic!("search json ({err}): {json_out}"));
    assert_eq!(doc["ranking"], "semantic", "{doc}");
    let hits = doc["hits"].as_array().expect("hits");
    assert!(
        hits.iter().any(|hit| {
            hit["text"]
                .as_str()
                .is_some_and(|text| text.contains(LESSON))
                && hit["id"]
                    .as_str()
                    .is_some_and(|id| !id.starts_with("local:"))
        }),
        "semantic page had no pack hit: {doc}"
    );
}

/// Several seats write at once and one request holds the only worker.
/// Search falls back, and the first line names the wait and the command.
#[test]
fn a_busy_writer_names_the_reason_and_the_next_command() {
    let Some(bin) = packsetd_bin() else {
        eprintln!("skipped: packsetd is not on PATH");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    for sub in [
        "tmp",
        "run",
        ".config",
        ".local/state",
        ".local/share/packset",
    ] {
        std::fs::create_dir_all(home.path().join(sub)).unwrap();
    }
    let stub = home.path().join("packset-embed");
    write_embed_stub(&stub, "12", "hold the encoder");
    let mut started = ljos_test_boot::Packsetd::build();
    started.bin(&bin);
    started.home_at(home.path());
    started.pack_at(home.path().join(".local/share/packset"));
    started.command().env("PACKSET_EMBED", &stub);
    started.command().env("PACKSET_WORKERS", "1");
    started.command().env("PACKSET_QUEUE", "1");
    started.command().env("PACKSET_AUTH", "off");
    let daemon = started.spawn();
    let seat = Seat::open(&daemon);
    let url = seat.url.clone();
    // One fast encode, then wait until status says the encoder is answering.
    // Until that, search reports the encoder as loading and never asks the
    // writer for rows.
    let warmed = Command::new("curl")
        .args([
            "--max-time",
            "5",
            "--silent",
            "--get",
            "--data-urlencode",
            "workspace=seat",
            "--data-urlencode",
            "q=warmup the encoder",
            "--data-urlencode",
            "limit=1",
            &format!("{url}/v1/search"),
        ])
        .output()
        .expect("warmup search");
    assert!(
        warmed.status.success(),
        "warmup: {}",
        String::from_utf8_lossy(&warmed.stderr)
    );
    let status_url = format!("{url}/v1/status");
    let ready_at = Instant::now();
    loop {
        let status = Command::new("curl")
            .args(["--max-time", "2", "--silent", &status_url])
            .output()
            .expect("status");
        let body = String::from_utf8_lossy(&status.stdout);
        if body.contains("\"answering\":true") || body.contains("\"answering\": true") {
            break;
        }
        assert!(
            ready_at.elapsed() < Duration::from_secs(8),
            "encoder did not start answering: {body}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let held = stub.with_file_name("embed-held");
    let hold = std::thread::spawn(move || {
        let _ = Command::new("curl")
            .args([
                "--max-time",
                "20",
                "--silent",
                "--get",
                "--data-urlencode",
                "workspace=seat",
                "--data-urlencode",
                "q=hold the encoder",
                "--data-urlencode",
                "limit=1",
                &format!("{url}/v1/search"),
            ])
            .output();
    });
    let held_at = Instant::now();
    while !held.is_file() {
        assert!(
            held_at.elapsed() < Duration::from_secs(8),
            "the hold request never reached the encoder"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut writers = Vec::new();
    for name in ["ada", "bob", "cara", "dee"] {
        let home = seat.home.clone();
        let path = seat.path.clone();
        let url = seat.url.clone();
        let pack = seat.pack.clone();
        writers.push(std::thread::spawn(move || {
            let _ = Command::new(env!("CARGO_BIN_EXE_ljos"))
                .args(["remember", &format!("{name} notes the kiln schedule")])
                .env_clear()
                .env("HOME", &home)
                .env("PATH", &path)
                .env("XDG_CONFIG_HOME", home.join(".config"))
                .env("XDG_STATE_HOME", home.join(".local/state"))
                .env("XDG_RUNTIME_DIR", home.join("run"))
                .env("PACKSET_URL", &url)
                .env("PACKSET_HOME", &pack)
                .env("PACKSET_AUTH", "off")
                .env("LJOS_SEAT", name)
                .env("LJOS_TRACKER_GIT", "off")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output();
        }));
    }
    let search = seat.run("ada", &["search", "kiln schedule"]);
    let said = text(&search);
    let stderr = String::from_utf8_lossy(&search.stderr);
    let stdout = String::from_utf8_lossy(&search.stdout);
    let first = stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let rows = "Memory is busy, so these results are ranked by words. If this persists, run `ljos doctor`.";
    let miss = rows;
    if stdout.trim().is_empty() {
        assert_eq!(search.status.code(), Some(1), "{said}");
        assert_eq!(first, miss, "first line: {first}\n{said}");
    } else {
        assert!(search.status.success(), "{said}");
        assert_eq!(first, rows, "first line: {first}\n{said}");
    }
    assert!(!said.contains("pack busy, try again"), "{said}");
    assert!(
        !said.contains("the pack search did not answer in time"),
        "{said}"
    );
    for writer in writers {
        let _ = writer.join();
    }
    let _ = hold.join();
}
