//! A stale `ljos-mcp` forwards a tool call to the installed binary through
//! `mcp_forward`; this drives that exchange against the built binary.

ljos_test_boot::harness!();
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Write the script, sync it, and close the handle before anything execs it.
fn write_program(path: &Path, body: &str) {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap();
    file.write_all(body.as_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    if let Some(parent) = path.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn a_child_request_can_share_the_tool_response_id() {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("server");
    write_program(
        &program,
        r#"#!/bin/sh
read -r initialize
read -r initialized
read -r call
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"elicitation/create","params":{"message":"confirm"}}'
read -r answer
printf '{"jsonrpc":"2.0","id":1,"result":%s}\n' "$answer"
"#,
    );
    let params = serde_json::json!({"name": "consent", "arguments": {}});
    let mut requests = 0;
    let answer = ljos_cli::mcp_forward_with_requests(
        &program,
        "TEST_FORWARDED",
        None,
        params.clone(),
        |method, params| {
            requests += 1;
            assert_eq!(method, "elicitation/create");
            assert_eq!(params["message"], "confirm");
            Ok(serde_json::json!({"action":"decline"}))
        },
    )
    .unwrap();
    assert_eq!(requests, 1);
    assert_eq!(answer["result"]["id"], 1);
    assert_eq!(answer["result"]["result"]["action"], "decline");

    let refused = ljos_cli::mcp_forward(&program, "TEST_FORWARDED", None, params).unwrap();
    assert_eq!(refused["result"]["error"]["code"], -32601);
    assert!(refused["result"].get("result").is_none());
}

#[test]
fn a_forwarded_call_is_answered_by_the_fresh_server() {
    let runtime = tempfile::tempdir().unwrap();
    let cards = tempfile::tempdir().unwrap();
    std::fs::write(cards.path().join("USER.md"), "frozen by the test\n").unwrap();
    let tracker = tempfile::tempdir().unwrap();
    std::fs::write(
        tracker.path().join("vissue.toml"),
        "seats = [\"acme-cli\"]\n",
    )
    .unwrap();
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    let program = runtime.path().join("server");
    write_program(
        &program,
        &format!(
            "#!/bin/sh\nexport XDG_RUNTIME_DIR={}\nexport LJOS_CARDS_DIR={}\n\
         export VISSUE_ROOT={}\nexport ISSUE_ROOT={}\nexport LJOS_SEAT=acme-cli\nexec {}\n",
            quote(runtime.path().to_str().unwrap()),
            quote(cards.path().to_str().unwrap()),
            quote(tracker.path().to_str().unwrap()),
            quote(tracker.path().to_str().unwrap()),
            quote(env!("CARGO_BIN_EXE_ljos-mcp")),
        ),
    );
    let params = serde_json::json!({"name": "ljos_cards", "arguments": {}});
    let init = serde_json::json!({"protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "acme-cli", "version": "1"}});
    let answer = ljos_cli::mcp_forward(&program, "LJOS_MCP_FORWARDED", Some(init), params).unwrap();
    assert_eq!(answer["id"], 1, "{answer}");
    assert!(
        answer["result"].to_string().contains("frozen by the test"),
        "{answer}"
    );
}

/// A writer still holds the script. Exec answers ETXTBSY until that handle
/// closes. The forward waits it out and still gets the answer.
#[test]
fn a_script_held_open_for_writing_still_answers() {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("server");
    write_program(
        &program,
        r#"#!/bin/sh
read -r initialize
read -r initialized
read -r call
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"busy-ok"}]}}'
"#,
    );
    let held = std::fs::OpenOptions::new()
        .write(true)
        .open(&program)
        .unwrap();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        drop(held);
    });
    let started = std::time::Instant::now();
    let answer = ljos_cli::mcp_forward(
        &program,
        "TEST_FORWARDED",
        None,
        serde_json::json!({"name": "ping", "arguments": {}}),
    )
    .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "the retry did not finish"
    );
    assert!(answer.to_string().contains("busy-ok"), "{answer}");
    releaser.join().unwrap();
}
