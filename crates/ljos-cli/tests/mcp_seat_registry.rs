//! A tracker write through the server is refused for a seat the seats
//! file does not list, once there is a seats file. Reads stay open.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;

#[test]
fn a_seats_file_keeps_unlisted_seats_off_the_tracker() {
    let root = tempfile::tempdir().unwrap();
    let tracker = root.path().join("tracker");
    std::fs::create_dir(&tracker).unwrap();
    std::fs::write(
        tracker.join("vissue.toml"),
        "seats = [\"saddle\", \"stranger\"]\n",
    )
    .unwrap();
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    let server = root.path().join("server");
    let mut server_file = std::fs::File::create(&server).unwrap();
    write!(
        server_file,
        "#!/bin/sh\nexec env -i PATH=/usr/bin:/bin HOME={home} \
         XDG_CONFIG_HOME={home}/config XDG_RUNTIME_DIR={home} \
         XDG_STATE_HOME={home}/state XDG_DATA_HOME={home}/data \
         VISSUE_ROOT={tracker} ISSUE_ROOT={tracker} VISSUE_NO_ROUTE=1 \
         LJOS_TRACKER_GIT=off PACKSET_URL=off LJOS_ASK_PACK=off LJOS_MCP_TOOLS=all {binary}\n",
        home = quote(root.path().to_str().unwrap()),
        tracker = quote(tracker.to_str().unwrap()),
        binary = quote(env!("CARGO_BIN_EXE_ljos-mcp")),
    )
    .unwrap();
    server_file.sync_all().unwrap();
    drop(server_file);
    std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o700)).unwrap();
    let call = |name: &str, arguments: serde_json::Value| {
        ljos_cli::mcp_forward(
            &server,
            "LJOS_MCP_FORWARDED",
            Some(serde_json::json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "some-client", "version": "1"}
            })),
            serde_json::json!({"name": name, "arguments": arguments}),
        )
        .unwrap()
        .to_string()
    };
    let note = |seat: &str| {
        call(
            "ljos_note",
            serde_json::json!({"seat": seat, "issue": "proj-aaaa", "text": "progress"}),
        )
    };

    let open = note("stranger");
    assert!(!open.contains("no seat named"), "no seats file: {open}");

    let seats = root.path().join("config/ljos/seats.toml");
    std::fs::create_dir_all(seats.parent().unwrap()).unwrap();
    std::fs::write(
        &seats,
        "\"0b1c2d3e-0000-4000-8000-000000000000\" = \"saddle\"\n",
    )
    .unwrap();

    let refused = note("stranger");
    assert!(refused.contains("\"isError\":true"), "{refused}");
    assert!(refused.contains("no seat named stranger in"), "{refused}");
    assert!(refused.contains("seats.toml"), "{refused}");
    let claim = call(
        "ljos_claim",
        serde_json::json!({"seat": "stranger", "node": "proj-aaaa"}),
    );
    assert!(claim.contains("no seat named stranger in"), "{claim}");

    let listed = note("saddle");
    assert!(!listed.contains("no seat named"), "{listed}");
    let read = call(
        "ljos_deed",
        serde_json::json!({"seat": "stranger", "issue": "proj-aaaa"}),
    );
    assert!(!read.contains("no seat named"), "listing is a read: {read}");
}
