//! A connected seat reaches command policy without a prior tracker claim.
//!
//! The guard's verdict does not depend on which seat the call names. A
//! write on the announced connection keeps that seat. A name the call
//! made up is still refused on a writer, in one short line.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;

#[test]
fn the_connected_seat_reaches_policy_without_borrowing_a_historical_holder() {
    // A temp home is a scratch run, so the server gets the explicit
    // guard-on setting to judge destructive commands there.
    let root = tempfile::tempdir().unwrap();
    let tracker = root.path().join("tracker");
    std::fs::create_dir(&tracker).unwrap();
    std::fs::write(
        tracker.join("vissue.toml"),
        "seats = [\"connected-runner-other\", \"reviewer\"]\n",
    )
    .unwrap();
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    let server = root.path().join("server");
    let mut server_file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&server)
        .unwrap();
    write!(
        server_file,
        "#!/bin/sh\nexec env -i PATH=/usr/bin:/bin HOME={home} \
         XDG_CONFIG_HOME={home}/config XDG_RUNTIME_DIR={home} \
         XDG_STATE_HOME={home}/state XDG_DATA_HOME={home}/data \
         VISSUE_ROOT={tracker} ISSUE_ROOT={tracker} VISSUE_NO_ROUTE=1 \
         PACKSET_URL=off LJOS_ASK_PACK=off LJOS_MCP_TOOLS=all LJOS_GUARD=on {binary}\n",
        home = quote(root.path().to_str().unwrap()),
        tracker = quote(tracker.to_str().unwrap()),
        binary = quote(env!("CARGO_BIN_EXE_ljos-mcp")),
    )
    .unwrap();
    server_file.sync_all().unwrap();
    drop(server_file);
    if let Ok(dir) = std::fs::File::open(root.path()) {
        let _ = dir.sync_all();
    }
    std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o700)).unwrap();
    let call = |name: &str, arguments: serde_json::Value| {
        ljos_cli::mcp_forward(
            &server,
            "LJOS_MCP_FORWARDED",
            Some(serde_json::json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "connected-runner", "version": "1"}
            })),
            serde_json::json!({"name": name, "arguments": arguments}),
        )
        .unwrap()
    };
    let guard = |seat: &str, command: &str| {
        call(
            "ljos_guard_check",
            serde_json::json!({"seat": seat, "command": command}),
        )
    };
    let allowed = guard("connected-runner", "printf harmless");
    assert_ne!(allowed["result"]["isError"], true, "{allowed}");
    assert_eq!(
        allowed["result"]["structuredContent"]["verdict"], "allow",
        "{allowed}"
    );

    let denied = guard("connected-runner", "rm -rf /home/owner/project");
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    assert!(denied.to_string().contains("deny"), "{denied}");
    assert!(!denied.to_string().contains("no seat named"), "{denied}");

    // A misspelled seat does not change the guard's verdict.
    let misspelled = guard("connected-runer", "printf harmless");
    assert_ne!(misspelled["result"]["isError"], true, "{misspelled}");
    assert_eq!(
        misspelled["result"]["structuredContent"]["verdict"], "allow",
        "{misspelled}"
    );
    assert!(
        !misspelled.to_string().contains("no seat named"),
        "{misspelled}"
    );

    let listed = guard("reviewer", "printf harmless");
    assert_eq!(
        listed["result"]["structuredContent"]["verdict"], "allow",
        "{listed}"
    );

    // The announced connection is the seat for a write. A made-up name is
    // refused, with the short hint and none of the historical holder ids.
    let wrote = call(
        "ljos_claim",
        serde_json::json!({"seat": "connected-runner", "node": "probe-abcd"}),
    );
    assert!(
        !wrote.to_string().contains("no seat named"),
        "the announced seat was refused: {wrote}"
    );
    let missed_write = call(
        "ljos_claim",
        serde_json::json!({"seat": "connected-runer", "node": "probe-abcd"}),
    );
    assert_eq!(missed_write["result"]["isError"], true, "{missed_write}");
    let said = missed_write.to_string();
    assert!(
        said.contains("no seat named connected-runer"),
        "{missed_write}"
    );
    assert!(
        said.contains("Seats here: connected-runner-other, reviewer."),
        "{missed_write}"
    );
    assert!(said.contains("LJOS_SEAT"), "{missed_write}");
    assert!(!said.contains("01a0dab3"), "{missed_write}");

    assert_eq!(
        std::fs::read_to_string(tracker.join("vissue.toml")).unwrap(),
        "seats = [\"connected-runner-other\", \"reviewer\"]\n"
    );
}
