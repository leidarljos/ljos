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
    serve_hits(
        embedder,
        r#"{"hits":[{"id":"k1","kind":"lesson","text":"the kiln fires at dusk","score":0.8,"ts":"2026-01-01T00:00:00.000Z","entities":["horizon:standing","seat:travel"],"ballots":2,"of":2}],"rank":"fused"}"#,
    )
}

fn serve_hits(embedder: &str, search_body: &str) -> Seat {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let embedder = embedder.to_string();
    let search_body = search_body.to_string();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let embedder = embedder.clone();
            let search_body = search_body.clone();
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
                    search_body
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
    // cargo is on PATH and cargo-binstall is not, so doctor names
    // `cargo install` rather than `cargo binstall`.
    ljos_cli::install_executable(&home.join("cargo"), "#!/bin/sh\nexit 0\n").unwrap();
    let started = Instant::now();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
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

/// `want` is the whole stderr sentence.
fn assert_search_line(stderr: &str, want: &str, state: &str) {
    let line = stderr
        .lines()
        .find(|line| line.contains(want))
        .unwrap_or_else(|| panic!("{state}: search stderr lacked {want:?}\n{stderr}"));
    assert_eq!(line, want, "{state}: search stderr line");
    assert_eq!(
        line.matches('.').count(),
        1,
        "{state}: the search line is not one sentence: {line}"
    );
    assert!(
        !line.contains("encoder is still loading"),
        "{state}: the old loading token was printed: {line}"
    );
}

#[test]
fn each_encoder_state_names_itself_on_doctor_and_on_search() {
    let install = ljos_cli::encoder_install_command();
    let doctor_install = "cargo install --locked packset-embed";
    let restart = ljos_cli::ENCODER_RESTART_COMMAND;
    let cases: &[(&str, &str)] = &[
        (
            "ready",
            r#"{"available":true,"answering":true,"enabled":true}"#,
        ),
        (
            "loading with remaining_ms",
            r#"{"binary":"/usr/bin/packset-embed","available":true,"answering":null,"enabled":true,"remaining_ms":120000}"#,
        ),
        (
            "loading without remaining_ms",
            r#"{"binary":"/usr/bin/packset-embed","available":true,"answering":null,"enabled":true}"#,
        ),
        (
            "failed",
            r#"{"available":true,"answering":false,"enabled":true,"error":"the runtime is missing"}"#,
        ),
        (
            "not installed",
            r#"{"available":false,"answering":null,"enabled":true}"#,
        ),
        (
            "not installed with stale availability",
            r#"{"binary":null,"available":true,"answering":null,"enabled":true}"#,
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
            "not installed" | "not installed with stale availability" => Some((
                "info",
                format!("not installed. Search matches words, so run `{doctor_install}`."),
            )),
            _ => unreachable!(),
        };
        let search_line = match *name {
            "ready" => ljos_cli::WORDS_LINE.to_string(),
            "loading with remaining_ms" => {
                "These are word matches; the encoder is loading (about 120 s left).".to_string()
            }
            "loading without remaining_ms" => {
                "These are word matches; the encoder has not answered yet.".to_string()
            }
            "failed" => format!(
                "The encoder failed (the runtime is missing), so these rows match words; run `{restart}`."
            ),
            "not installed" | "not installed with stale availability" => {
                if install.starts_with("curl ") || install.starts_with("wget ") {
                    format!(
                        "matched by words (encoder not installed), so read {} or run `ljos doctor`.",
                        ljos_cli::INSTALL_PAGE
                    )
                } else {
                    format!("matched by words (encoder not installed), so run `{install}`.")
                }
            }
            _ => unreachable!(),
        };
        let seat = serve(embedder);
        let (code, doctor, doctor_err, _) = run(&seat, &["doctor"]);
        let said = format!("{doctor}{doctor_err}");
        assert!(
            code == 0
                || doctor.lines().any(|line| {
                    line.starts_with("ljos is ready")
                        || line.starts_with("ljos is not ready")
                        || line.starts_with("ljos runs")
                }),
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
        if name.starts_with("loading") {
            assert!(
                search_out.contains("the kiln fires at dusk"),
                "{name}: the word-ranked row was not printed\n{search_out}"
            );
            assert!(
                !search_err.contains("rerun"),
                "{name}: a page with rows said to rerun\n{search_err}"
            );
        } else {
            assert!(
                !search_err.contains("is loading"),
                "{name}: search said the encoder was loading\n{search_err}"
            );
        }
    }
}

/// Rows printed while the encoder is loading are word matches, on the
/// plain page and in `--json`. A remaining time does not add a rerun.
#[test]
fn a_loading_search_with_rows_says_they_are_word_matches() {
    let cases = [
        (
            "with remaining_ms",
            r#"{"available":true,"answering":null,"enabled":true,"remaining_ms":120000}"#,
            "These are word matches; the encoder is loading (about 120 s left).",
        ),
        (
            "loading flag without remaining_ms",
            r#"{"available":true,"answering":null,"enabled":true,"loading":true}"#,
            "These are word matches; the encoder is loading.",
        ),
        (
            "without remaining_ms",
            r#"{"available":true,"answering":null,"enabled":true}"#,
            "These are word matches; the encoder has not answered yet.",
        ),
    ];
    for (name, embedder, line) in cases {
        let seat = serve(embedder);
        let (code, stdout, stderr, _) = run(&seat, &["search", "kiln"]);
        assert!(code == 0, "{name}: {stdout}{stderr}");
        assert!(
            stdout.contains("the kiln fires at dusk"),
            "{name}: {stdout}"
        );
        assert_search_line(&stderr, line, name);
        assert!(
            !stderr.contains("rerun") && !stderr.contains("shortly"),
            "{stderr}"
        );
        let (json_code, json_out, json_err, _) = run(&seat, &["search", "--json", "kiln"]);
        assert!(json_code == 0, "{name}: {json_out}{json_err}");
        assert!(
            !json_err.contains("rerun") && !json_err.contains("word-match"),
            "{name}: json printed the plain line: {json_err}"
        );
        let doc: serde_json::Value = serde_json::from_str(json_out.trim())
            .unwrap_or_else(|err| panic!("{name}: {err}: {json_out}"));
        assert_eq!(doc["ranking"], "lexical", "{name}: {doc}");
        assert_eq!(doc["reason"], line, "{name}: {doc}");
        assert!(
            doc["hits"].as_array().is_some_and(|rows| !rows.is_empty()),
            "{name}: {doc}"
        );
    }
}

/// An empty page while the encoder is loading still names the command to
/// run again, with the remaining time when the writer sends one.
#[test]
fn a_loading_search_with_no_rows_says_to_rerun() {
    let empty = r#"{"hits":[],"rank":"fused"}"#;
    let cases = [
        (
            "with remaining_ms",
            r#"{"available":true,"answering":null,"enabled":true,"remaining_ms":120000}"#,
            "The encoder is loading and is ready in about 120 s, so rerun `ljos search kiln`.",
            "The encoder is loading and is ready in about 120 s, so rerun `ljos search --json kiln`.",
        ),
        (
            "without remaining_ms",
            r#"{"available":true,"answering":null,"enabled":true}"#,
            "The encoder is loading, so rerun `ljos search kiln` when it is ready.",
            "The encoder is loading, so rerun `ljos search --json kiln` when it is ready.",
        ),
    ];
    for (name, embedder, plain, json_reason) in cases {
        let seat = serve_hits(embedder, empty);
        let (code, stdout, stderr, _) = run(&seat, &["search", "kiln"]);
        assert!(code == 0, "{name}: {stdout}{stderr}");
        assert!(
            stdout.contains("Nothing matched") && !stdout.contains("the kiln fires at dusk"),
            "{name}: {stdout}"
        );
        assert_search_line(&stderr, plain, name);
        assert!(!stderr.contains("word-match"), "{name}: {stderr}");
        let (json_code, json_out, json_err, _) = run(&seat, &["search", "--json", "kiln"]);
        assert!(json_code == 0, "{name}: {json_out}{json_err}");
        assert!(
            !json_err.contains("rerun") && !json_err.contains("word-match"),
            "{name}: json printed the plain line: {json_err}"
        );
        let doc: serde_json::Value = serde_json::from_str(json_out.trim())
            .unwrap_or_else(|err| panic!("{name}: {err}: {json_out}"));
        assert_eq!(doc["ranking"], "lexical", "{name}: {doc}");
        assert_eq!(doc["reason"], json_reason, "{name}: {doc}");
        assert!(
            doc["hits"].as_array().is_some_and(|rows| rows.is_empty()),
            "{name}: {doc}"
        );
    }
}

fn exe(dir: &std::path::Path, name: &str) {
    ljos_cli::install_executable(&dir.join(name), "#!/bin/sh\nexit 1\n").unwrap();
}

fn packset_embed_line(path: &str, cwd: &std::path::Path) -> String {
    let home = tempfile::tempdir().unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["doctor", "--all"])
        .current_dir(cwd)
        .env_clear()
        .env("PATH", path)
        .env("HOME", home.path())
        .env("PACKSET_URL", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_OFF", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    text.lines()
        .find(|line| line.split('\t').nth(1) == Some("packset-embed"))
        .unwrap_or_else(|| panic!("no packset-embed row:\n{text}"))
        .to_string()
}

/// `ljos doctor` names an encoder install command whose program is on this
/// machine. `cargo binstall` is that command only when cargo-binstall is
/// installed. A `scripts/install.sh` in the working directory is not read,
/// so the hint is the same wherever `ljos mcp` started.
#[test]
fn doctor_names_an_encoder_install_command_that_runs_here() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    exe(&bin, "curl");
    exe(&bin, "cargo");
    exe(&bin, "cargo-binstall");
    let path = bin.display().to_string();
    let cargo_only = root.path().join("cargo-only");
    std::fs::create_dir_all(&cargo_only).unwrap();
    exe(&cargo_only, "curl");
    exe(&cargo_only, "cargo");
    let curl_only = root.path().join("curl-only");
    std::fs::create_dir_all(&curl_only).unwrap();
    exe(&curl_only, "curl");
    let script_home = root.path().join("checkout");
    std::fs::create_dir_all(script_home.join("scripts")).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        script_home.join("scripts/install.sh"),
        "#!/bin/sh\nexit 0\n",
    )
    .unwrap();
    let bare = root.path().join("bare");
    std::fs::create_dir_all(&bare).unwrap();

    let binstall = packset_embed_line(&path, &bare);
    assert!(
        binstall.contains("so run `cargo binstall --locked packset-embed`."),
        "{binstall}"
    );
    let cargo = packset_embed_line(&cargo_only.display().to_string(), &bare);
    assert!(
        cargo.contains("so run `cargo install --locked packset-embed`."),
        "{cargo}"
    );
    assert!(!cargo.contains("cargo binstall"), "{cargo}");
    let from_checkout = packset_embed_line(&curl_only.display().to_string(), &script_home);
    assert!(
        from_checkout.contains("so run `curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh`."),
        "a checkout in the working directory changes nothing: {from_checkout}"
    );
    let release = packset_embed_line(&curl_only.display().to_string(), &bare);
    assert!(
        release.contains(
            "so run `curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh`."
        ),
        "{release}"
    );
    assert!(!release.contains("cargo binstall"), "{release}");
}
