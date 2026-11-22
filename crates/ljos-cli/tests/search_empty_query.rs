//! `ljos search` with no words is a usage error that names the command to
//! run. It used to print `Nothing matched this query.` and exit 0, as if a
//! search had run and found nothing.

#[path = "common/boot.rs"]
mod boot;

use std::process::{Command, Output, Stdio};

fn search(args: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    return Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("search")
        .args(args)
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", dir.path().join("run"))
        .env("TMPDIR", dir.path())
        .env("LJOS_SEAT", "testseat")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_NO_AUTOSTART", "1")
        .env("PACKSET_URL", "http://127.0.0.1:9")
        .env("PACKSET_AUTH", "off")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
}

#[test]
fn a_search_with_no_words_names_the_command_to_run() {
    for args in [&[][..], &["  "][..], &["--json"][..]] {
        let out = search(args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {stderr}");
        assert_eq!(
            stderr.trim(),
            "search: the query is empty, so run `ljos search \"QUERY\"`.",
            "{args:?}"
        );
        assert!(out.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn a_search_with_a_source_filter_and_no_words_is_not_refused() {
    let out = search(&["--source-seat", "alice"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("the query is empty"), "{stderr}");
}

#[test]
fn empty_store_search_help_and_docs_name_lesson_placeholder() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut sock in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut buf = [0u8; 2048];
                let n = sock.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]);
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let body = if path.starts_with("/v1/status") {
                    r#"{"embedder":{"enabled":true,"binary":null,"available":false,"answering":null},"live":0,"live_cap":0}"#
                } else if path.starts_with("/v1/search") {
                    r#"{"hits":[],"rank":"fused"}"#
                } else if path.starts_with("/v1/atoms") {
                    r#"{"atoms":[]}"#
                } else {
                    "ok"
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["search", "lexical default"])
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", dir.path().join("run"))
        .env("TMPDIR", dir.path())
        .env("LJOS_SEAT", "testseat")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_NO_AUTOSTART", "1")
        .env("LJOS_SEARCH_WAIT", "0")
        .env("PACKSET_URL", format!("http://127.0.0.1:{port}"))
        .env("PACKSET_AUTH", "off")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}{stderr}");
    assert!(
        stdout.contains("ljos remember \"LESSON\""),
        "empty store search did not print placeholder:\n{stdout}{stderr}"
    );

    let help = search(&["--help"]);
    let help_out = String::from_utf8_lossy(&help.stdout);
    assert!(help.status.success(), "{help_out}");
    assert!(
        help_out.contains("`ljos remember \"LESSON\"`")
            && !help_out.contains("with the query in quotes"),
        "search --help drifted:\n{help_out}"
    );

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for rel in [
        "README.md",
        "docs/orgmode/reference-mcp.org",
        "docs/source/reference-mcp.rst",
    ] {
        let body = std::fs::read_to_string(root.join(rel)).unwrap();
        assert!(
            !body.contains("with the query in quotes") && body.contains("LESSON"),
            "{rel} still says remember with the query in quotes"
        );
    }
}

#[test]
fn an_empty_search_stays_one_line_even_with_a_queued_lesson() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let state = home.join(".local/state/ljos");
    std::fs::create_dir_all(&state).unwrap();
    #[expect(clippy::disallowed_methods, reason = "writes a file that is not a git hook")] std::fs::write(
        state.join("note-queue.jsonl"),
        "{\"issue\":\"demo-kiln\",\"text\":\"The kiln fires at dusk.\",\"workspace\":\"ws\",\"at\":1}\n",
    )
    .unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["search"])
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", dir.path().join("run"))
        .env("TMPDIR", dir.path())
        .env("LJOS_SEAT", "testseat")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_NO_AUTOSTART", "1")
        .env("PACKSET_URL", "http://127.0.0.1:9")
        .env("PACKSET_AUTH", "off")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert_eq!(
        stderr.trim(),
        "search: the query is empty, so run `ljos search \"QUERY\"`."
    );
    assert!(out.stdout.is_empty(), "{stderr}");
}
