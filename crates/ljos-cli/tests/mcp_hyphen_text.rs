//! MCP tracker tools bind hyphen-leading values before invoking real binaries.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Stdio;

fn issue_file(project: &Path, id: &str) {
    std::fs::create_dir_all(project).unwrap();
    ljos_cli::write_file(
        project.join("issues.org"),
        format!(
            "#+TITLE: issues\n#+VISSUE: 1\n#+CATEGORY: demo\n#+TODO: TODO STARTED | DONE CANCELLED\n\n* TODO [#C] Forward values\n:PROPERTIES:\n:ID:         {id}\n:END:\n"
        ),
    )
    .unwrap();
}

#[test]
fn mcp_note_file_options_and_vote_keep_hyphen_leading_values() {
    if tools::halt(tools::Tool::Vissue) || tools::halt(tools::Tool::Packsetd) {
        return;
    }
    let pack = ljos_test_boot::Packsetd::open();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let tracker = root.path().join("tracker");
    for sub in [
        home.join(".config"),
        home.join(".local/state"),
        home.join(".local/share"),
        root.path().join("run"),
        tracker.clone(),
    ] {
        std::fs::create_dir_all(sub).unwrap();
    }
    ljos_cli::write_file(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    issue_file(&tracker.join("Software/demo"), "demo-a1b2");
    issue_file(&tracker.join("Software/--top"), "top-a1b2");

    let target = Path::new(env!("CARGO_BIN_EXE_ljos")).parent().unwrap();
    let path = format!(
        "{}:{}",
        target.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = ljos_cli::program(env!("CARGO_BIN_EXE_ljos-mcp"))
        .unwrap()
        .current_dir(root.path())
        .env_clear()
        .env("PATH", path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "alice")
        .env("LJOS_HARNESS_SESSION", "hyphen-mcp-session")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", pack.url())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let requests = [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"hyphen-text","version":"1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"ljos_note","arguments":{"seat":"alice","issue":"demo-a1b2","text":"-note text"}}}),
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
            "name":"ljos_file","arguments":{"seat":"alice","title":"-title","project":"--top","body":"-body"}}}),
        serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
            "name":"ljos_options","arguments":{"seat":"alice","issue":"demo-a1b2","options":["-left","right"]}}}),
        serde_json::json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
            "name":"ljos_vote","arguments":{"seat":"alice","issue":"demo-a1b2","choice":"-left","used":"none"}}}),
        serde_json::json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{
            "name":"ljos_remember","arguments":{"seat":"alice","text":""}}}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for request in requests {
        writeln!(stdin, "{request}").unwrap();
    }
    drop(stdin);

    let mut answers = std::collections::BTreeMap::new();
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let line = line.unwrap();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(id) = value["id"].as_u64() {
                answers.insert(id, value);
            }
        }
    }
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    for id in 2..=5 {
        let answer = answers
            .get(&id)
            .unwrap_or_else(|| panic!("missing answer {id}: {answers:?}\n{stderr}"));
        assert_ne!(answer["result"]["isError"], true, "{answer}\n{stderr}");
    }
    let empty = answers
        .get(&6)
        .unwrap_or_else(|| panic!("missing answer 6: {answers:?}\n{stderr}"));
    assert_eq!(empty["result"]["isError"], false, "{empty}\n{stderr}");
    assert!(
        empty.to_string().contains("empty-input"),
        "{empty}\n{stderr}"
    );

    let demo = std::fs::read_to_string(tracker.join("Software/demo/issues.org")).unwrap();
    assert!(demo.contains("-note text"), "{demo}");
    assert!(demo.contains("Options: -left, right"), "{demo}");
    assert!(demo.contains("-left"), "{demo}");
    let top = std::fs::read_to_string(tracker.join("Software/--top/issues.org")).unwrap();
    assert!(top.contains("-title"), "{top}");
    assert!(top.contains("-body"), "{top}");
}
