//! A tracker write through the server is refused for a seat vissue.toml
//! does not list, once that file lists seats. Reads stay open.

ljos_test_boot::harness!();
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// A server under `root` with a cleared environment, and a way to call it.
fn server(root: &Path) -> impl Fn(&str, serde_json::Value) -> String {
    let tracker = root.join("tracker");
    std::fs::create_dir(&tracker).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"saddle\"]\n").unwrap();
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    let server = root.join("server");
    let mut server_file = std::fs::File::create(&server).unwrap();
    write!(
        server_file,
        "#!/bin/sh\nexec env -i PATH=/usr/bin:/bin HOME={home} \
         XDG_CONFIG_HOME={home}/config XDG_RUNTIME_DIR={home} \
         XDG_STATE_HOME={home}/state XDG_DATA_HOME={home}/data \
         VISSUE_ROOT={tracker} ISSUE_ROOT={tracker} VISSUE_NO_ROUTE=1 \
         LJOS_TRACKER_GIT=off PACKSET_URL=off LJOS_ASK_PACK=off LJOS_MCP_TOOLS=all {binary}\n",
        home = quote(root.to_str().unwrap()),
        tracker = quote(tracker.to_str().unwrap()),
        binary = quote(env!("CARGO_BIN_EXE_ljos-mcp")),
    )
    .unwrap();
    server_file.sync_all().unwrap();
    drop(server_file);
    std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o700)).unwrap();
    move |name: &str, arguments: serde_json::Value| {
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
    }
}

#[test]
fn vissue_toml_keeps_unlisted_seats_off_the_tracker() {
    let root = tempfile::tempdir().unwrap();
    let call = server(root.path());
    let note = |seat: &str| {
        call(
            "ljos_note",
            serde_json::json!({"seat": seat, "issue": "proj-aaaa", "text": "progress"}),
        )
    };

    let refused = note("stranger");
    assert!(refused.contains("\"isError\":true"), "{refused}");
    assert!(refused.contains("no seat named stranger"), "{refused}");
    assert!(
        refused.contains("vissue.toml") || refused.contains("saddle"),
        "{refused}"
    );
    let claim = call(
        "ljos_claim",
        serde_json::json!({"seat": "stranger", "node": "proj-aaaa"}),
    );
    assert!(claim.contains("no seat named stranger"), "{claim}");
    let bound = call(
        "ljos_playbook",
        serde_json::json!({"seat": "stranger", "issue": "proj-aaaa", "name": "sit"}),
    );
    assert!(bound.contains("no seat named stranger"), "{bound}");
    for (tool, args) in [
        (
            "ljos_vote",
            serde_json::json!({"seat": "stranger", "issue": "proj-aaaa", "choice": "accept", "used": "none"}),
        ),
        (
            "ljos_vote",
            serde_json::json!({"seat": "stranger", "issue": "proj-aaaa", "withdraw": true}),
        ),
        (
            "ljos_predict",
            serde_json::json!({"seat": "stranger", "issue": "proj-aaaa", "expect": "accept"}),
        ),
        (
            "ljos_bump_plan",
            serde_json::json!({"seat": "stranger", "bundle": "bundle", "project": "proj", "parent": "proj-aaaa"}),
        ),
    ] {
        let said = call(tool, args);
        assert!(said.contains("no seat named stranger"), "{tool}: {said}");
    }
    let tally = call(
        "ljos_vote",
        serde_json::json!({"seat": "saddle", "issue": "proj-aaaa"}),
    );
    assert!(
        !tally.contains("no seat named"),
        "a tally is a read: {tally}"
    );
    let dry = call(
        "ljos_bump_plan",
        serde_json::json!({"seat": "saddle", "bundle": "bundle", "project": "proj", "parent": "proj-aaaa", "dry_run": true}),
    );
    assert!(!dry.contains("no seat named"), "a dry run is a read: {dry}");

    let listed = note("saddle");
    assert!(!listed.contains("no seat named"), "{listed}");
    let read = call(
        "ljos_deed",
        serde_json::json!({"seat": "saddle", "issue": "proj-aaaa"}),
    );
    assert!(!read.contains("no seat named"), "listing is a read: {read}");
}

#[test]
fn a_listed_seat_cannot_write_as_another_assignee() {
    let root = tempfile::tempdir().unwrap();
    let call = server(root.path());
    for (tool, args) in [
        ("ljos_claim", serde_json::json!({"node": "proj-aaaa"})),
        ("ljos_release", serde_json::json!({"node": "proj-aaaa"})),
        ("ljos_complete", serde_json::json!({"node": "proj-aaaa"})),
        ("ljos_sitting", serde_json::json!({"issue": "proj-aaaa"})),
        (
            "ljos_finish",
            serde_json::json!({"issue": "proj-aaaa", "lesson": "x"}),
        ),
    ] {
        let mut args = args;
        args["seat"] = "saddle".into();
        args["assignee"] = "stranger".into();
        let said = call(tool, args);
        assert!(said.contains("\"isError\":true"), "{tool}: {said}");
        assert!(
            said.contains("stranger is not this seat, saddle"),
            "{tool}: {said}"
        );
    }
}

#[test]
fn a_listed_seat_records_a_persona_ballot() {
    let root = tempfile::tempdir().unwrap();
    let call = server(root.path());
    let seats = root.path().join("config/ljos/seats.toml");
    std::fs::create_dir_all(seats.parent().unwrap()).unwrap();
    std::fs::write(&seats, "seats = [\"saddle\", \"stranger\"]\n").unwrap();
    for args in [
        serde_json::json!({"issue": "proj-aaaa", "choice": "accept", "used": "none"}),
        serde_json::json!({"issue": "proj-aaaa", "withdraw": true}),
    ] {
        let mut args = args;
        args["seat"] = "saddle".into();
        args["as"] = "stranger".into();
        let said = call("ljos_vote", args);
        assert!(!said.contains("not this seat"), "{said}");
    }
}

#[test]
fn a_listed_seat_forecasts_as_a_persona() {
    let root = tempfile::tempdir().unwrap();
    let call = server(root.path());
    let said = call(
        "ljos_predict",
        serde_json::json!({"seat": "saddle", "issue": "proj-aaaa", "expect": "accept", "as": "stranger"}),
    );
    assert!(!said.contains("not this seat"), "{said}");
}
