//! The unchanged import, integrate and export round trip does not call
//! packset, and it does not rewrite the seed.

ljos_test_boot::harness!();

use std::io::Read;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Slow {
    url: String,
    stop: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}

impl Slow {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let stop_flag = Arc::clone(&stop);
        let call_count = Arc::clone(&calls);
        std::thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        call_count.fetch_add(1, Ordering::Relaxed);
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                        let mut buf = [0u8; 1024];
                        let _ = stream.read(&mut buf);
                        let mut waited = Duration::ZERO;
                        while waited < Duration::from_secs(30) && !stop_flag.load(Ordering::Relaxed)
                        {
                            std::thread::sleep(Duration::from_millis(50));
                            waited += Duration::from_millis(50);
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });
        Self { url, stop, calls }
    }
}

impl Drop for Slow {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Seat {
    root: tempfile::TempDir,
    home: std::path::PathBuf,
    tracker: std::path::PathBuf,
    handoff: std::path::PathBuf,
    seed: std::path::PathBuf,
}

impl Seat {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let tracker = root.path().join("tracker");
        let handoff = root.path().join("handoff");
        std::fs::create_dir_all(home.join("tmp")).unwrap();
        std::fs::create_dir_all(&handoff).unwrap();
        std::fs::create_dir_all(&tracker).unwrap();
        Self {
            seed: root.path().join("seed.jsonl"),
            root,
            home,
            tracker,
            handoff,
        }
    }

    fn run(&self, pack: &str, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
            .env("HOME", &self.home)
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", self.home.join("tmp"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("VISSUE_ROOT", &self.tracker)
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_SYNC_HOST", "kiln")
            .env("LJOS_SEARCH_WAIT", "30")
            .env("PACKSET_URL", pack)
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_WORKSPACE", "seat")
            .output()
            .expect("ljos")
    }
}

fn text(out: &std::process::Output) -> String {
    format!(
        "status {}\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn lane(seat: &Seat, name: &str, rows: &[(usize, &str)]) {
    let mut body = String::new();
    body.push_str(
        &serde_json::json!({
            "format": "ljos-memory-1",
            "record": "export",
            "machine": "host-a",
            "seat": "donor",
            "exported_at": "2026-11-01T00:00:00Z",
        })
        .to_string(),
    );
    body.push('\n');
    for (n, lesson) in rows {
        body.push_str(
            &serde_json::json!({
                "format": "ljos-memory-1",
                "kind": "lesson",
                "text": lesson,
                "origin": "agent-derived",
                "atom_id": format!("{name}-{n}"),
                "exported_at": "2026-11-01T00:00:00Z",
                "source": {"machine": "host-a", "seat": "donor"},
            })
            .to_string(),
        );
        body.push('\n');
    }
    std::fs::write(seat.handoff.join(format!("{name}.jsonl")), body).unwrap();
}

fn lesson(k: usize) -> String {
    format!("Seed lesson {k:02} stands alone on bench {k:02}.")
}

/// Seven lanes, 281 rows, 61 distinct lessons.
fn plant(seat: &Seat) -> Vec<String> {
    let sizes = [38usize, 40, 41, 39, 42, 40, 41];
    assert_eq!(sizes.iter().sum::<usize>(), 281);
    let lanes = ["aa", "bb", "cc", "dd", "ee", "ff", "gg"];
    let mut n = 0usize;
    let mut names = Vec::new();
    for (name, count) in lanes.into_iter().zip(sizes) {
        let mut rows = Vec::new();
        for _ in 0..count {
            let text = lesson(n % 61);
            rows.push((n, text));
            n += 1;
        }
        let owned: Vec<(usize, &str)> = rows.iter().map(|(i, text)| (*i, text.as_str())).collect();
        lane(seat, name, &owned);
        names.push(name.to_string());
    }
    assert_eq!(n, 281);
    names
}

fn round_trip(seat: &Seat, pack: &str, names: &[String]) -> std::process::Output {
    for name in names {
        let path = seat.handoff.join(format!("{name}.jsonl"));
        let out = seat.run(pack, &["memory", "import", path.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
    }
    let integrated = seat.run(pack, &["memory", "integrate", "--all"]);
    assert!(integrated.status.success(), "{}", text(&integrated));
    seat.run(
        pack,
        &[
            "memory",
            "export",
            "--out",
            seat.seed.to_str().unwrap(),
            "--machine",
            "kiln",
        ],
    )
}

#[test]
fn an_unchanged_round_trip_does_not_wait_on_a_slow_pack() {
    let slow = Slow::start();
    let seat = Seat::new();
    let names = plant(&seat);
    let first = round_trip(&seat, "off", &names);
    assert!(first.status.success(), "{}", text(&first));
    let said = String::from_utf8(first.stdout.clone()).unwrap();
    assert!(said.starts_with("exported "), "{said}");
    let bytes = std::fs::read(&seat.seed).unwrap();
    let written = std::fs::metadata(&seat.seed).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    let calls_before = slow.calls.load(Ordering::Relaxed);
    let started = Instant::now();
    for name in &names {
        let path = seat.handoff.join(format!("{name}.jsonl"));
        let out = seat.run(&slow.url, &["memory", "import", path.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        let line = String::from_utf8(out.stdout).unwrap();
        assert!(line.starts_with("nothing new:"), "{line}");
    }
    let integrated = seat.run(&slow.url, &["memory", "integrate", "--all"]);
    assert!(integrated.status.success(), "{}", text(&integrated));
    assert!(
        String::from_utf8(integrated.stdout.clone())
            .unwrap()
            .starts_with("nothing new:"),
        "{}",
        text(&integrated)
    );
    let again = seat.run(
        &slow.url,
        &[
            "memory",
            "export",
            "--out",
            seat.seed.to_str().unwrap(),
            "--machine",
            "kiln",
        ],
    );
    let took = started.elapsed();
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(
        String::from_utf8(again.stdout).unwrap(),
        "seed unchanged: 61 memories\n"
    );
    assert_eq!(std::fs::read(&seat.seed).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&seat.seed).unwrap().modified().unwrap(),
        written
    );
    assert_eq!(
        slow.calls.load(Ordering::Relaxed),
        calls_before,
        "unchanged round trip called packset"
    );
    assert!(
        took < Duration::from_secs(5),
        "unchanged round trip took {took:?}"
    );
    let json = seat.run(
        &slow.url,
        &[
            "memory",
            "export",
            "--out",
            seat.seed.to_str().unwrap(),
            "--machine",
            "kiln",
            "--json",
        ],
    );
    assert!(json.status.success(), "{}", text(&json));
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["memories"], 61);
    assert_eq!(value["unchanged"], true);
    assert_eq!(
        std::fs::metadata(&seat.seed).unwrap().modified().unwrap(),
        written
    );

    let queue = seat.home.join(".local/state/ljos");
    std::fs::create_dir_all(&queue).unwrap();
    let queued = queue.join("note-queue.jsonl");
    std::fs::write(
        &queued,
        "{\"issue\":\"\",\"text\":\"a queued lesson that must stay\",\"workspace\":\"seat\",\"at\":1,\"kind\":\"lesson\",\"seat\":\"ada\"}\n",
    )
    .unwrap();
    let before = std::fs::read(&queued).unwrap();
    let calls = slow.calls.load(Ordering::Relaxed);
    let imported = seat.run(
        &slow.url,
        &[
            "memory",
            "import",
            seat.handoff.join("aa.jsonl").to_str().unwrap(),
        ],
    );
    assert!(imported.status.success(), "{}", text(&imported));
    assert_eq!(std::fs::read(&queued).unwrap(), before);
    assert_eq!(slow.calls.load(Ordering::Relaxed), calls);

    let fresh = "A new kiln stands on the west bench.";
    lane(&seat, "hh", &[(0, fresh)]);
    let brought = seat.run(
        "off",
        &[
            "memory",
            "import",
            seat.handoff.join("hh.jsonl").to_str().unwrap(),
        ],
    );
    assert!(brought.status.success(), "{}", text(&brought));
    let mapped = seat.run("off", &["memory", "integrate", "--all"]);
    assert!(mapped.status.success(), "{}", text(&mapped));
    let wrote = seat.run(
        "off",
        &[
            "memory",
            "export",
            "--out",
            seat.seed.to_str().unwrap(),
            "--machine",
            "kiln",
        ],
    );
    assert!(wrote.status.success(), "{}", text(&wrote));
    let body = String::from_utf8(std::fs::read(&seat.seed).unwrap()).unwrap();
    assert!(body.contains(fresh), "{body}");
    assert_ne!(body.as_bytes(), bytes.as_slice());
    let _ = seat.root;
}
