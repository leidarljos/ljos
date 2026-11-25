//! Ticket search reuses one loaded tracker corpus and its claim facts.

ljos_test_boot::harness!();

#[cfg(target_os = "linux")]
use packset_client::Hit;
#[cfg(target_os = "linux")]
use std::path::Path;

#[cfg(target_os = "linux")]
fn rchar() -> u64 {
    std::fs::read_to_string("/proc/self/io")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("rchar: ")?.parse().ok())
        .unwrap()
}

#[cfg(target_os = "linux")]
fn tree_bytes(path: &Path) -> u64 {
    let mut bytes = 0;
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            bytes += tree_bytes(&path);
        } else {
            bytes += path.metadata().unwrap().len();
        }
    }
    bytes
}

#[cfg(target_os = "linux")]
#[test]
fn search_reuses_setup_and_does_not_retain_released_claims() {
    const TICKETS: usize = 512;
    const SETUP_BYTES: usize = 64 * 1024;

    let temp = tempfile::tempdir().unwrap();
    let tracker = temp.path().join("tracker");
    let project = tracker.join("Software/bench");
    std::fs::create_dir_all(&project).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        tracker.join("vissue.toml"),
        "prefix = \"Software\"\nseats = [\"alice\"]\n",
    )
    .unwrap();

    let mut setup = String::from("#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n");
    setup.push_str(&"# setup padding\n".repeat(SETUP_BYTES / 16));
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(project.join("setup.org"), setup).unwrap();

    let mut board = String::from(
        "#+TITLE: bench issues\n\
         #+VISSUE: 1\n\
         #+CATEGORY: bench\n\
         #+SETUPFILE: setup.org\n\n",
    );
    for index in 0..TICKETS {
        let (state, title, claim) = if index + 1 == TICKETS {
            ("STARTED", "Claimed sentinel ticket", ":CLAIMED_BY: alice\n")
        } else {
            ("TODO", "Ticket", "")
        };
        board.push_str(&format!(
            "* {state} {title} {index:04}\n\
             :PROPERTIES:\n\
             :ID: Software-{index:04}\n\
             {claim}\
             :END:\n\
             Body {index:04}.\n\n"
        ));
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(project.join("issues.org"), board).unwrap();

    let layout = vissue_core::Layout::new(&tracker, "Software");
    vissue_core::ledger::migrate(&layout, false).unwrap();

    unsafe {
        std::env::set_var("VISSUE_ROOT", &tracker);
        std::env::set_var("VISSUE_NO_ROUTE", "1");
        std::env::set_var("LJOS_SEAT", "alice");
    }
    let corpus_bytes = tree_bytes(&tracker);
    let before = rchar();
    let mut hits = Vec::<Hit>::new();
    let claims = ljos_cli::ticket_search::merge(&mut hits, "Claimed sentinel ticket");
    let hint = ljos_cli::search_finish_hint_with_claims(&hits, Some(&claims));
    let read = rchar() - before;

    assert_eq!(
        hits.first().and_then(|hit| hit.id.as_deref()),
        Some("Software-0511")
    );
    assert!(hint.is_some(), "loaded claim should produce a finish hint");
    assert!(
        read <= corpus_bytes * 3 + 1024 * 1024,
        "search read {read} bytes for a {corpus_bytes}-byte tracker with {TICKETS} tickets"
    );

    vissue_core::ops::release_holder_as(&layout, "alice", None, None, false, "test").unwrap();
    assert_eq!(
        ljos_cli::search_finish_hint(&hits),
        None,
        "a later hint must read the released claim, not retained search state"
    );
}
