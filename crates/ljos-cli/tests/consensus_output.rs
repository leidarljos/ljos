//! `ljos consensus` refuses an issue with no ballots, and `--json` prints
//! one JSON document a caller can parse whole.

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
            "#!/bin/sh\ncase \"$1\" in\n  vote) echo '{ballots}' ;;\n  consensus) echo 'consensus: rrf 2 of 2, settled' ;;\n  *) exit 1 ;;\nesac\n"
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
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("consensus")
        .args(args)
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", "off")
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

#[test]
fn json_is_one_document() {
    let (dir, home) = fakes(r#"[{"agent":"alice","choice":"rrf"},{"agent":"bob","choice":"rrf"}]"#);
    let out = consensus(dir.path(), &home, &["proj-v0te", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let doc = one_document(&out.stdout);
    assert_eq!(doc["push_cite"]["stands"], true, "{doc}");
    assert_eq!(doc["push_cite"]["choice"], "rrf", "{doc}");
    assert_eq!(doc["settle"]["winner"], "rrf", "{doc}");
    assert!(doc.get("tracker").is_none(), "{doc}");

    let compared = consensus(dir.path(), &home, &["proj-v0te", "--json", "--compare"]);
    assert!(compared.status.success(), "{compared:?}");
    let doc = one_document(&compared.stdout);
    assert_eq!(doc["settle"]["winner"], "rrf", "{doc}");
    assert_eq!(doc["tracker"], "consensus: rrf 2 of 2, settled", "{doc}");
}
