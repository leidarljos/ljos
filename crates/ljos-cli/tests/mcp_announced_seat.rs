//! The seat a server is given at connect is the seat for its later calls.
//!
//! The box seat map names this conversation's seat, and the tracker knows
//! only a past claim holder. A write under the mapped name on the
//! connection is sat as the announced seat. A name nobody announced is
//! still refused.

ljos_test_boot::harness!();

#[test]
fn a_write_keeps_the_seat_announced_at_connect() {
    let root = tempfile::tempdir().unwrap();
    let tracker = root.path().join("tracker");
    std::fs::create_dir(&tracker).unwrap();
    let id = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
    let seats = root.path().join("config/ljos/seats.toml");
    std::fs::create_dir_all(seats.parent().unwrap()).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        tracker.join("issues.org"),
        "* TODO old work\n:PROPERTIES:\n:CLAIMED_BY: saddle\n:END:\n",
    )
    .unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&seats, format!("{id:?} = \"paper\"\n")).unwrap();
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    let server = root.path().join("server");
    ljos_cli::install_executable_mode(
        &server,
        format!(
            "#!/bin/sh\nexec env -i PATH=/usr/bin:/bin HOME={home} \
             XDG_CONFIG_HOME={home}/config XDG_RUNTIME_DIR={home} \
             XDG_STATE_HOME={home}/state XDG_DATA_HOME={home}/data \
             VISSUE_ROOT={tracker} ISSUE_ROOT={tracker} VISSUE_NO_ROUTE=1 \
             ACME_THREAD_ID={id} LJOS_TRACKER_GIT=off PACKSET_URL=off \
             LJOS_ASK_PACK=off LJOS_MCP_TOOLS=all {binary}\n",
            home = quote(root.path().to_str().unwrap()),
            tracker = quote(tracker.to_str().unwrap()),
            binary = quote(env!("CARGO_BIN_EXE_ljos-mcp")),
        ),
        0o700,
    )
    .unwrap();
    let note = |seat: &str| {
        ljos_cli::mcp_forward(
            &server,
            "LJOS_MCP_FORWARDED",
            Some(serde_json::json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "some-client", "version": "1"}
            })),
            serde_json::json!({"name": "ljos_note", "arguments":
                {"seat": seat, "issue": "proj-aaaa", "text": "progress"}}),
        )
        .unwrap()
        .to_string()
    };

    let announced = note("paper");
    assert!(
        !announced.contains("no seat named"),
        "the seat announced at connect was refused: {announced}"
    );
    let log = std::fs::read_to_string(root.path().join("ljos/book.log")).unwrap_or_default();
    assert!(
        log.lines().any(|line| {
            line
            == "note: the tracker lists no seat paper; sitting it as the seat this server was given"
        }),
        "the server did not sit the announced seat:\n{log}"
    );

    let stranger = note("stranger");
    assert!(stranger.contains("\"isError\":true"), "{stranger}");
    assert!(stranger.contains("no seat named stranger"), "{stranger}");
}
