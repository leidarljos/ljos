//! A stale `ljos-mcp` forwards a tool call to the installed binary through
//! `mcp_forward`; this drives that exchange against the built binary.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[test]
fn a_child_request_can_share_the_tool_response_id() {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("server");
    std::fs::write(&program, r#"#!/bin/sh
read -r initialize
read -r initialized
read -r call
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"elicitation/create","params":{"message":"confirm"}}'
read -r answer
printf '{"jsonrpc":"2.0","id":1,"result":%s}\n' "$answer"
"#).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
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
    // Safety: this test binary runs one test; the child inherits both.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", runtime.path());
        std::env::set_var("LJOS_CARDS_DIR", cards.path());
    }
    let params = serde_json::json!({"name": "ljos_cards", "arguments": {}});
    let init = serde_json::json!({"protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "acme-cli", "version": "1"}});
    let answer = ljos_cli::mcp_forward(
        Path::new(env!("CARGO_BIN_EXE_ljos-mcp")),
        "LJOS_MCP_FORWARDED",
        Some(init),
        params,
    )
    .unwrap();
    assert_eq!(answer["id"], 1, "{answer}");
    assert!(
        answer["result"].to_string().contains("frozen by the test"),
        "{answer}"
    );
}
