//! `ljos consensus` refuses an issue with no ballots, and `--json` prints
//! one JSON document a caller can parse whole. A pack that does not
//! answer leaves the push cite standing on nothing, as at the push gate.
//! `PACKSET_URL=off` is that same unread roster. A document whose cite
//! stands is read against a pack that answers with no personas.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Fakes for the tracker and the model crate. `vissue vote ID --json`
/// answers `ballots`; the settles print what each tool prints.
fn fakes(ballots: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    script(
        dir.path(),
        "vissue",
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  vote) echo '{ballots}' ;;\n  show) echo '{{\"id\":\"'\"$2\"'\"}}' ;;\n  consensus) echo 'consensus: rrf 2 of 2, settled' ;;\n  *) exit 1 ;;\nesac\n"
        ),
    );
    script(
        dir.path(),
        "ljos-consensus",
        "#!/bin/sh\ncase \"$1\" in\n  settle) echo '{\"settled\":true,\"winner\":\"rrf\",\"shares\":{\"rrf\":1.0}}' ;;\n  *) echo '{}' ;;\nesac\n",
    );
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    (dir, home)
}

fn consensus(dir: &Path, home: &Path, args: &[&str]) -> Output {
    consensus_on(dir, home, args, "off")
}

/// [`consensus`] with `PACKSET_URL` set to `pack`.
fn consensus_on(dir: &Path, home: &Path, args: &[&str], pack: &str) -> Output {
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("consensus")
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", pack)
        // A consensus that may file a dissent is a tracker write.
        .env("LJOS_SEAT", "saddle")
        // The stub tracker holds no signature notes; this test reads the
        // shape of the JSON, and signed ballots have their own suite.
        .env("LJOS_UNSIGNED_BALLOTS", "allow")
        .output()
        .unwrap()
}

#[test]
fn an_issue_with_no_ballots_does_not_settle() {
    let (dir, home) = fakes("[]");
    let out = consensus(dir.path(), &home, &["proj-n0v0"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "settled with no ballots: {out:?}");
    assert!(err.contains("holds no ballots"), "{err}");
}

fn one_document(stdout: &[u8]) -> serde_json::Value {
    let text = String::from_utf8(stdout.to_vec()).unwrap();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not one JSON document ({e}): {text}"))
}

/// A writer that answers `GET /v1/atoms` with an empty roster.
fn empty_pack() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 1024];
            loop {
                let n = std::io::Read::read(&mut stream, &mut buf).unwrap_or(0);
                if n == 0 || buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let body = r#"{"atoms":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = std::io::Write::write_all(&mut stream, resp.as_bytes());
        }
    });
    format!("http://{addr}")
}

#[test]
fn json_is_one_document() {
    let (dir, home) = fakes(r#"[{"agent":"alice","choice":"rrf"},{"agent":"bob","choice":"rrf"}]"#);
    let pack = empty_pack();
    let out = consensus_on(dir.path(), &home, &["proj-v0te", "--json"], &pack);
    assert!(out.status.success(), "{out:?}");
    let doc = one_document(&out.stdout);
    assert_eq!(doc["push_cite"]["stands"], true, "{doc}");
    assert_eq!(doc["push_cite"]["choice"], "rrf", "{doc}");
    assert_eq!(doc["settle"]["winner"], "rrf", "{doc}");
    assert!(doc.get("tracker").is_none(), "{doc}");

    let compared = consensus_on(
        dir.path(),
        &home,
        &["proj-v0te", "--json", "--compare"],
        &pack,
    );
    assert!(compared.status.success(), "{compared:?}");
    let doc = one_document(&compared.stdout);
    assert_eq!(doc["settle"]["winner"], "rrf", "{doc}");
    assert_eq!(doc["tracker"], "consensus: rrf 2 of 2, settled", "{doc}");
}

/// Beside the push cite, the reading says whether the issue clears a
/// runner's push onto develop or main, under the quorum the person sets.
#[test]
fn the_reading_says_whether_a_protected_push_clears() {
    let (dir, home) = fakes(r#"[{"agent":"alice","choice":"rrf"},{"agent":"bob","choice":"rrf"}]"#);
    let pack = empty_pack();
    let out = consensus_on(dir.path(), &home, &["proj-v0te", "--json"], &pack);
    assert!(out.status.success(), "{out:?}");
    let doc = one_document(&out.stdout);
    let protected = &doc["protected_push"];
    // Two seats with no recorded family each count as their own, so quorum 2 settles.
    assert_eq!(protected["stands"], true, "{doc}");
    assert!(protected.get("refusal").is_none(), "{doc}");
    assert_eq!(protected["quorum"], 2, "{doc}");
    assert_eq!(
        protected["branches"],
        serde_json::json!(["develop", "main"]),
        "{doc}"
    );

    let config = home.join(".config").join("ljos");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("ljos.toml"), "[gates]\nquorum = 3\n").unwrap();
    let out = consensus_on(dir.path(), &home, &["proj-v0te"], &pack);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains(
            "protected push: proj-v0te does not clear a push onto `develop`, `main`: \
             the sitting has no settled consensus: 2 of the 3 seats"
        ),
        "{text}"
    );
}

#[test]
fn a_pack_that_does_not_answer_leaves_the_push_cite_unread() {
    let (dir, home) = fakes(r#"[{"agent":"alice","choice":"rrf"},{"agent":"bob","choice":"rrf"}]"#);
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let down = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let out = consensus_on(dir.path(), &home, &["proj-v0te", "--json"], &down);
    let doc = one_document(&out.stdout);
    assert_eq!(doc["push_cite"]["stands"], false, "{doc}");
    let refusal = doc["push_cite"]["refusal"].as_str().unwrap_or_default();
    assert!(
        refusal.starts_with("proj-v0te: the roster is not readable"),
        "{doc}"
    );
}
