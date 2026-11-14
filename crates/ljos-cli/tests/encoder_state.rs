//! Each encoder state, from a writer that only answers HTTP.
//!
//! Plain `ljos doctor` names a load, a failure, or a missing encoder.
//! A ready encoder stays off that list. `ljos search` says the same thing
//! on one stderr line, and a reported remaining time does not extend
//! `LJOS_SEARCH_WAIT`.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Seat {
    url: String,
    home: tempfile::TempDir,
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let rest = line
                .strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))?;
            rest.trim().parse().ok()
        })
        .unwrap_or(0)
}

fn serve(embedder: &str) -> Seat {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let embedder = embedder.to_string();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let embedder = embedder.clone();
            std::thread::spawn(move || {
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
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or("/");
                let cl = content_length(&head);
                while buf.len() < head_end + cl {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let body = if path == "/v1/status" {
                    format!(r#"{{"embedder":{embedder},"live":0,"live_cap":0}}"#)
                } else if path == "/v1/search" {
                    r#"{"hits":[],"rank":"fused"}"#.to_string()
                } else if path == "/v1/atoms" {
                    r#"{"atoms":[]}"#.to_string()
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
    });
    Seat {
        url: format!("http://127.0.0.1:{port}"),
        home,
    }
}

fn run(seat: &Seat, args: &[&str]) -> (i32, String, String, Duration) {
    let home = seat.home.path();
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env_clear()
        .env("PATH", home)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TMPDIR", home)
        .env("PACKSET_URL", &seat.url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("LJOS_NO_AUTOSTART", "1")
        .env("LJOS_SEARCH_WAIT", "0")
        .env("LJOS_SEAT", "travel")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        started.elapsed(),
    )
}

fn encoder_row(text: &str) -> Option<(&str, &str)> {
    text.lines().find_map(|line| {
        let mut cols = line.splitn(3, '\t');
        let word = cols.next()?;
        let name = cols.next()?;
        let state = cols.next()?;
        (name == "encoder").then_some((word, state))
    })
}

/// `want` is the whole stderr sentence, or the ready line `matched by meaning`.
fn assert_search_line(stderr: &str, want: &str, state: &str) {
    let line = stderr
        .lines()
        .find(|line| line.contains(want))
        .unwrap_or_else(|| panic!("{state}: search stderr lacked {want:?}\n{stderr}"));
    if want != "matched by meaning" {
        assert_eq!(line, want, "{state}: search stderr line");
        assert_eq!(
            line.matches('.').count(),
            1,
            "{state}: the search line is not one sentence: {line}"
        );
    }
    assert!(
        !line.contains("encoder is still loading"),
        "{state}: the old loading token was printed: {line}"
    );
}

#[test]
fn each_encoder_state_names_itself_on_doctor_and_on_search() {
    let rerun = "ljos search kiln";
    let install = ljos_cli::ENCODER_INSTALL_COMMAND;
    let restart = ljos_cli::ENCODER_RESTART_COMMAND;
    let cases: &[(&str, &str)] = &[
        (
            "ready",
            r#"{"available":true,"answering":true,"enabled":true}"#,
        ),
        (
            "loading with remaining_ms",
            r#"{"available":true,"answering":null,"enabled":true,"remaining_ms":120000}"#,
        ),
        (
            "loading without remaining_ms",
            r#"{"available":true,"answering":null,"enabled":true}"#,
        ),
        (
            "failed",
            r#"{"available":true,"answering":false,"enabled":true,"error":"the runtime is missing"}"#,
        ),
        (
            "not installed",
            r#"{"available":false,"answering":null,"enabled":true}"#,
        ),
    ];
    for (name, embedder) in cases {
        let doctor_row = match *name {
            "ready" => None,
            "loading with remaining_ms" => Some((
                "warn",
                "loading, about 120 s left. Search matches words until then.".to_string(),
            )),
            "loading without remaining_ms" => Some((
                "warn",
                "loading. Search matches words until it is ready.".to_string(),
            )),
            "failed" => Some((
                "warn",
                format!(
                    "failed: the runtime is missing. Search matches words, so run `{restart}`."
                ),
            )),
            "not installed" => Some((
                "info",
                format!("not installed. Search matches words, so run `{install}`."),
            )),
            _ => unreachable!(),
        };
        let search_line = match *name {
            "ready" => "matched by meaning".to_string(),
            "loading with remaining_ms" => format!(
                "The encoder is loading and is ready in about 120 s, so rerun `{rerun}`."
            ),
            "loading without remaining_ms" => {
                format!("The encoder is loading, so rerun `{rerun}` when it is ready.")
            }
            "failed" => format!(
                "The encoder failed (the runtime is missing), so these rows match words; run `{restart}`."
            ),
            "not installed" => {
                format!("matched by words (encoder not installed), so run `{install}`.")
            }
            _ => unreachable!(),
        };
        let seat = serve(embedder);
        let (code, doctor, doctor_err, _) = run(&seat, &["doctor"]);
        let said = format!("{doctor}{doctor_err}");
        assert!(
            code == 0
                || doctor.starts_with("ljos is ready")
                || doctor.starts_with("ljos is not ready")
                || doctor.starts_with("ljos runs"),
            "{name}: doctor exited {code}\n{said}"
        );
        match doctor_row {
            None => {
                assert!(
                    encoder_row(&doctor).is_none(),
                    "{name}: a ready encoder was printed on plain doctor\n{doctor}"
                );
                let (_, all, all_err, _) = run(&seat, &["doctor", "--all"]);
                let (word, state) = encoder_row(&all).unwrap_or_else(|| {
                    panic!("{name}: --all had no encoder row\n{all}\n{all_err}")
                });
                assert_eq!(word, "ok", "{name}: {all}");
                assert_eq!(state, "loaded", "{name}: {all}");
            }
            Some((word, state)) => {
                let row = encoder_row(&doctor)
                    .unwrap_or_else(|| panic!("{name}: plain doctor had no encoder row\n{said}"));
                assert_eq!(row, (word, state.as_str()), "{name}");
            }
        }
        let (search_code, search_out, search_err, elapsed) = run(&seat, &["search", "kiln"]);
        assert!(
            search_code == 0,
            "{name}: search exited {search_code}\n{search_out}{search_err}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "{name}: search took {elapsed:?}, past LJOS_SEARCH_WAIT\n{search_err}"
        );
        assert_search_line(&search_err, &search_line, name);
        if !name.starts_with("loading") {
            assert!(
                !search_err.contains("is loading"),
                "{name}: search said the encoder was loading\n{search_err}"
            );
        }
    }
}
