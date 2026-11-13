//! Adoption follow-ups: the box seat map, seat exit status, attempt
//! search, and the deed line `deedar create` prints.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

fn pack() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf);
            let line = req.lines().next().unwrap_or("");
            let body = if line.contains("/v1/atoms") {
                r#"{"atoms":[]}"#
            } else if line.contains("/v1/search") {
                r#"{"hits":[]}"#
            } else {
                "{}"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    port
}

struct World {
    _dir: tempfile::TempDir,
    home: std::path::PathBuf,
    run: std::path::PathBuf,
    path: String,
    pack: String,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let run = dir.path().join("run");
        std::fs::create_dir_all(home.join(".config/ljos")).unwrap();
        std::fs::create_dir_all(&run).unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let deedar = bin.join("deedar");
        std::fs::write(&deedar, "#!/bin/sh\necho \"no such deed $2\" >&2\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&deedar, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let port = pack();
        Self {
            _dir: dir,
            home,
            run,
            path,
            pack: format!("http://127.0.0.1:{port}"),
        }
    }

    fn ljos(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("USER", "probe-login")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", &self.pack);
        seat_env::clear(&mut cmd);
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
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

#[test]
fn a_mapped_main_conversation_sits_and_an_unresolved_one_exits_2() {
    let world = World::new();
    let id = "bc-1234abcd-rest-main";
    std::fs::write(
        world.home.join(".config/ljos/seats.toml"),
        format!("{id:?} = \"saddle\"\n"),
    )
    .unwrap();
    let seat = world.ljos(&["seat"], &[("CURSOR_CONVERSATION_ID", id)]);
    let said = text(&seat);
    assert!(seat.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&seat.stdout);
    assert!(stdout.contains("seat\tsaddle\n"), "{said}");
    assert!(stdout.contains("seats.toml"), "{said}");
    assert!(!stdout.contains("sand-sub"), "{said}");

    let sub = "sand-subagent-0099";
    let open = world.ljos(&["seat"], &[("CURSOR_CONVERSATION_ID", sub)]);
    let said = text(&open);
    assert!(open.status.success(), "{said}");
    let both = format!(
        "{}{}",
        String::from_utf8_lossy(&open.stdout),
        String::from_utf8_lossy(&open.stderr)
    );
    assert!(!both.contains("label seat-unresolved"), "{said}");
    assert!(
        both.contains("reading every seat") || both.contains("every-seat"),
        "{said}"
    );
    assert!(!both.contains("this conversation's seat is seat"), "{said}");
}

#[test]
fn an_unlisted_seat_exits_2() {
    let world = World::new();
    let tracker = world.home.join("tracker");
    std::fs::create_dir_all(&tracker).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"saddle\"]\n").unwrap();
    let out = world.ljos(
        &["note", "proj-aaaa", "progress"],
        &[
            ("LJOS_SEAT", "stranger"),
            ("VISSUE_ROOT", tracker.to_str().unwrap()),
        ],
    );
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    let both = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(both.contains("label seat-unlisted"), "{said}");
    assert!(both.contains("stranger"), "{said}");
}

#[test]
fn a_failed_command_is_left_out_of_search_until_asked() {
    let world = World::new();
    let missing = world.home.join("zircon-missing.jsonl");
    let failed = world.ljos(
        &["memory", "import", missing.to_str().unwrap()],
        &[("LJOS_SEAT", "saddle")],
    );
    assert!(!failed.status.success(), "{}", text(&failed));
    let plain = world.ljos(&["search", "zircon-missing"], &[("LJOS_SEAT", "saddle")]);
    let said = text(&plain);
    assert!(plain.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&plain.stdout);
    assert!(
        stdout.contains("No hits for zircon-missing, so run `ljos remember \"TEXT\"`."),
        "{said}"
    );
    assert!(!stdout.contains("attempt,"), "{said}");
    let asked = world.ljos(
        &["search", "--attempts", "zircon-missing"],
        &[("LJOS_SEAT", "saddle")],
    );
    let said = text(&asked);
    assert!(asked.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&asked.stdout);
    assert!(stdout.contains("zircon-missing"), "{said}");
    // A missing import file is the caller's input, so the label is input-warning.
    assert!(stdout.contains("attempt, input-warning"), "{said}");
}

#[test]
fn a_missing_deed_names_the_id_line_and_exits_nonzero() {
    let world = World::new();
    let out = world.ljos(
        &["deed", "proj-aaaa", "--add", "deed-missing"],
        &[("LJOS_SEAT", "saddle")],
    );
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert_ne!(out.status.code(), Some(0), "{said}");
    let both = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(both.contains("deed-missing"), "{said}");
    assert!(both.contains("`id=`"), "{said}");
}

fn search_pack() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = r#"{"hits":[
        {"id":"note-hour","kind":"note","text":"Saddle hourly keep-moving note","score":9.0,"entities":["seat:saddle"]},
        {"id":"log-poke","kind":"log","text":"disk-saver poke log from the night","score":8.0,"entities":["seat:disk-saver"]},
        {"id":"attempt:help","kind":"lesson","text":"memory --help showed the flags","score":7.0,"entities":["seat:saddle"]},
        {"id":"glaze-other","kind":"lesson","text":"The glaze on the other seat is thick.","score":9.0,"entities":["seat:saddle"]},
        {"id":"glaze-own","kind":"lesson","text":"The glaze fires hotter than the clay body.","score":1.0,"entities":["seat:ada"]},
        {"id":"glaze-pref","kind":"preference","text":"Keep the glaze schedule written down.","score":2.0,"entities":["seat:ada"]}
    ]}"#;
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf);
            let line = req.lines().next().unwrap_or("");
            let body = if line.contains("/v1/search") {
                hits
            } else if line.contains("/v1/atoms") {
                r#"{"atoms":[]}"#
            } else {
                "{}"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    port
}

#[test]
fn default_search_keeps_this_seats_topic_and_drops_routine_logs() {
    let world = World::new();
    let port = search_pack();
    let url = format!("http://127.0.0.1:{port}");
    let on_topic = world.ljos(
        &["search", "glaze"],
        &[
            ("LJOS_SEAT", "ada"),
            ("PACKSET_URL", &url),
            ("PACKSET_AUTH", "off"),
            ("PACKSET_TOKEN", "probe-token"),
        ],
    );
    let said = text(&on_topic);
    assert!(on_topic.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&on_topic.stdout);
    assert!(!stdout.contains("keep-moving"), "{said}");
    assert!(!stdout.contains("poke log"), "{said}");
    assert!(!stdout.contains("memory --help"), "{said}");
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| line.contains("glaze"))
        .collect();
    assert!(lines.len() >= 2, "{said}");
    assert!(
        lines[0].contains("seat:ada") || lines[0].contains("schedule"),
        "{said}"
    );
    assert!(
        lines[0].contains("schedule") || lines[0].contains("fires"),
        "{said}"
    );
    assert!(
        lines.iter().any(|line| line.contains("other seat")),
        "{said}"
    );
    let own_at = lines
        .iter()
        .position(|line| line.contains("fires hotter"))
        .expect(&said);
    let other_at = lines
        .iter()
        .position(|line| line.contains("other seat"))
        .expect(&said);
    assert!(own_at < other_at, "{said}");

    let hourly = world.ljos(
        &["search", "hourly"],
        &[
            ("LJOS_SEAT", "ada"),
            ("PACKSET_URL", &url),
            ("PACKSET_AUTH", "off"),
            ("PACKSET_TOKEN", "probe-token"),
        ],
    );
    let said = text(&hourly);
    assert!(hourly.status.success(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&hourly.stdout).trim(),
        "No hits for hourly, so run `ljos remember \"TEXT\"`.",
        "{said}"
    );

    let stop = world.ljos(
        &["search", "the"],
        &[
            ("LJOS_SEAT", "ada"),
            ("PACKSET_URL", &url),
            ("PACKSET_AUTH", "off"),
            ("PACKSET_TOKEN", "probe-token"),
        ],
    );
    let said = text(&stop);
    assert!(stop.status.success(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&stop.stdout).trim(),
        "No hits for the, so run `ljos remember \"TEXT\"`.",
        "{said}"
    );
}

fn busy_then_ready() -> (u16, u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let busy_port = busy.local_addr().unwrap().port();
    let posts = std::sync::Arc::new(AtomicUsize::new(0));
    let count = std::sync::Arc::clone(&posts);
    thread::spawn(move || {
        for stream in busy.incoming() {
            let Ok(mut sock) = stream else { break };
            count.fetch_add(1, Ordering::SeqCst);
            let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            let body = br#"{"error":"the pack is shedding writes"}"#;
            let resp = format!(
                "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
            let _ = sock.write_all(body);
        }
    });
    let ready = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ready_port = ready.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in ready.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf);
            let line = req.lines().next().unwrap_or("");
            let body = if line.contains("POST") {
                r#"{"id":"atom-queued","kind":"lesson","text":"kept"}"#
            } else if line.contains("/v1/atoms") {
                r#"{"atoms":[]}"#
            } else {
                "{}"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    (busy_port, ready_port, posts)
}

#[test]
fn a_busy_remember_retries_queues_and_the_next_call_sends_it() {
    let world = World::new();
    let (busy_port, ready_port, posts) = busy_then_ready();
    let lesson = "The kiln dries slowly. Fire it at dusk.";
    let busy_url = format!("http://127.0.0.1:{busy_port}");
    let started = std::time::Instant::now();
    let out = world.ljos(
        &["remember", lesson],
        &[
            ("LJOS_SEAT", "ada"),
            ("PACKSET_URL", &busy_url),
            ("PACKSET_AUTH", "off"),
        ],
    );
    let took = started.elapsed();
    let said = text(&out);
    assert_eq!(out.status.code(), Some(75), "{said}");
    assert!(took < std::time::Duration::from_secs(3), "{took:?} {said}");
    let tries = posts.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        (1..=12).contains(&tries),
        "remember tried {tries} times\n{said}"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("the pack is shedding writes"), "{said}");
    assert!(err.contains("queued"), "{said}");
    let queue = world.home.join(".state/ljos/note-queue.jsonl");
    let queued = std::fs::read_to_string(&queue).unwrap_or_default();
    assert!(queued.contains("kiln dries slowly"), "{queued}");

    let ready_url = format!("http://127.0.0.1:{ready_port}");
    let next = world.ljos(
        &["seat"],
        &[
            ("LJOS_SEAT", "ada"),
            ("PACKSET_URL", &ready_url),
            ("PACKSET_AUTH", "off"),
        ],
    );
    let said = text(&next);
    assert!(next.status.success(), "{said}");
    let err = String::from_utf8_lossy(&next.stderr);
    assert!(err.contains("sent") && err.contains("queued"), "{said}");
    let left = std::fs::read_to_string(&queue).unwrap_or_default();
    assert!(!left.contains("kiln dries slowly"), "{left}");
}

#[test]
fn a_refused_pack_search_is_one_line_and_exits_75() {
    let world = World::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let url = format!("http://127.0.0.1:{port}");
    let out = world.ljos(
        &["search", "glaze"],
        &[
            ("LJOS_SEAT", "ada"),
            ("PACKSET_URL", &url),
            ("PACKSET_AUTH", "off"),
        ],
    );
    let said = text(&out);
    assert!(out.stdout.is_empty(), "{said}");
    let err = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = err.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines.len(), 1, "{said}");
    assert!(
        lines[0].contains("packset ensure") || lines[0].contains("cargo install --locked packset"),
        "{said}"
    );
    assert!(!lines[0].contains("packsetd is unreachable"), "{said}");
    let code = if lines[0].contains("packset ensure") {
        75
    } else {
        1
    };
    assert_eq!(out.status.code(), Some(code), "{said}");
    assert!(!err.contains("Caused by"), "{said}");
}
