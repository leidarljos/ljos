//! The finish tool does not offer a close. The command line flag stays for the person.

#[test]
fn the_loop_does_not_say_it_closes_itself() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    assert!(
        !src.contains("The loop closes itself"),
        "the workflow note still says the loop closes itself"
    );
}

#[test]
fn the_mcp_finish_tool_has_no_close_flag() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/bin/ljos-mcp/server.rs"
    ))
    .unwrap();
    assert!(!src.contains("pub close: bool"));
    assert!(!src.contains("ljos_finish` with close does"));
    assert!(!src.contains("again, A round"));
    assert!(src.contains("This tool does not close the tracker ticket."));
}
