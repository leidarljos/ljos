//! A connected seat can reach the command guard without a prior tracker claim.

use std::os::unix::fs::PermissionsExt;

#[test]
fn the_connected_seat_reaches_policy_without_borrowing_a_historical_holder() {
    // The guard treats /tmp homes as scratch runs, so this fixture needs
    // an isolated home in the build tree to exercise destructive policy.
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    std::fs::create_dir_all(&target).unwrap();
    let root = tempfile::tempdir_in(target).unwrap();
    let tracker = root.path().join("tracker");
    std::fs::create_dir(&tracker).unwrap();
    std::fs::write(
        tracker.join("vissue.toml"),
        "seats = [\"connected-runner-other\", \"reviewer\"]\n",
    )
    .unwrap();
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    let server = root.path().join("server");
    std::fs::write(
        &server,
        format!(
            "#!/bin/sh\nexec env -i PATH=/usr/bin:/bin HOME={home} \
             XDG_CONFIG_HOME={home}/config XDG_RUNTIME_DIR={home} \
             XDG_STATE_HOME={home}/state XDG_DATA_HOME={home}/data \
             VISSUE_ROOT={tracker} ISSUE_ROOT={tracker} VISSUE_NO_ROUTE=1 \
             PACKSET_URL=off LJOS_ASK_PACK=off LJOS_MCP_TOOLS=all {binary}\n",
            home = quote(root.path().to_str().unwrap()),
            tracker = quote(tracker.to_str().unwrap()),
            binary = quote(env!("CARGO_BIN_EXE_ljos-mcp")),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o700)).unwrap();
    let call = |seat: &str, command: &str| {
        ljos_cli::mcp_forward(
            &server,
            "LJOS_MCP_FORWARDED",
            Some(serde_json::json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "connected-runner", "version": "1"}
            })),
            serde_json::json!({"name": "ljos_guard_check", "arguments": {
                "seat": seat, "command": command
            }}),
        )
        .unwrap()
    };
    let allowed = call("connected-runner", "printf harmless");
    assert_ne!(allowed["result"]["isError"], true, "{allowed}");
    assert_eq!(
        allowed["result"]["structuredContent"]["verdict"], "allow",
        "{allowed}"
    );

    let denied = call("connected-runner", "rm -rf /home/owner/project");
    assert_eq!(denied["result"]["isError"], true, "{denied}");
    assert!(denied.to_string().contains("deny"), "{denied}");
    assert!(!denied.to_string().contains("no seat named"), "{denied}");

    let misspelled = call("connected-runer", "printf harmless");
    assert_eq!(misspelled["result"]["isError"], true, "{misspelled}");
    assert!(
        misspelled
            .to_string()
            .contains("no seat named connected-runer"),
        "{misspelled}"
    );

    let listed = call("reviewer", "printf harmless");
    assert_eq!(
        listed["result"]["structuredContent"]["verdict"], "allow",
        "{listed}"
    );
    assert_eq!(
        std::fs::read_to_string(tracker.join("vissue.toml")).unwrap(),
        "seats = [\"connected-runner-other\", \"reviewer\"]\n"
    );
}
