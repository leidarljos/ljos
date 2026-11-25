//! Ticket search reuses one loaded tracker corpus and its claim facts.

ljos_test_boot::harness!();

use packset_client::Hit;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());

/// The process environment for one test. It is held for the test and put
/// back as it was when the test returns.
struct Guard {
    saved: Vec<(OsString, OsString)>,
    _lock: MutexGuard<'static, ()>,
}

impl Guard {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let saved = std::env::vars_os().collect();
        Self { saved, _lock: lock }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let now: Vec<OsString> = std::env::vars_os().map(|(key, _)| key).collect();
        unsafe {
            for key in now {
                if !self.saved.iter().any(|(have, _)| *have == key) {
                    std::env::remove_var(&key);
                }
            }
            for (key, value) in &self.saved {
                std::env::set_var(key, value);
            }
        }
    }
}

/// Home and runtime directory in `temp`, so no hold record or runner
/// record names an issue, and no hook is running.
fn isolate(temp: &Path) {
    let home = temp.join("home");
    std::fs::create_dir_all(home.join("run")).unwrap();
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_RUNTIME_DIR", home.join("run"));
        std::env::remove_var("LJOS_IN_HOOK");
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "writes a file that is not a git hook"
)]
fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

/// One `Software/demo` project under `temp` holding `board`. This process
/// sits as `seat` and holds as `hold` when one is given, [`isolate`]d in
/// `temp`.
fn sit_on_tracker(temp: &Path, seat: &str, hold: Option<&str>, board: &str) {
    let tracker = temp.join("tracker");
    let project = tracker.join("Software/demo");
    std::fs::create_dir_all(&project).unwrap();
    isolate(temp);
    write(&tracker.join("vissue.toml"), "prefix = \"Software\"\n");
    write(
        &project.join("issues.org"),
        &format!(
            "#+TITLE: demo issues\n\
             #+VISSUE: 1\n\
             #+CATEGORY: demo\n\
             #+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n{board}"
        ),
    );
    unsafe {
        std::env::set_var("VISSUE_ROOT", &tracker);
        std::env::set_var("VISSUE_NO_ROUTE", "1");
        std::env::set_var("LJOS_SEAT", seat);
        match hold {
            Some(hold) => std::env::set_var("LJOS_HOLD", hold),
            None => std::env::remove_var("LJOS_HOLD"),
        }
    }
}

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

    let _env = Guard::new();
    let temp = tempfile::tempdir().unwrap();
    let tracker = temp.path().join("tracker");
    let project = tracker.join("Software/bench");
    std::fs::create_dir_all(&project).unwrap();
    isolate(temp.path());
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
        std::env::remove_var("LJOS_HOLD");
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

#[test]
fn a_repeated_id_takes_its_claim_from_the_heading_search_kept() {
    let _env = Guard::new();
    let temp = tempfile::tempdir().unwrap();
    sit_on_tracker(
        temp.path(),
        "alice",
        None,
        "* TODO Repeated kiln ticket\n\
         :PROPERTIES:\n\
         :ID: Software-0001\n\
         :END:\n\n\
         * STARTED Repeated kiln ticket copy\n\
         :PROPERTIES:\n\
         :ID: Software-0001\n\
         :CLAIMED_BY: alice\n\
         :END:\n",
    );
    let mut hits = Vec::<Hit>::new();
    let claims = ljos_cli::ticket_search::merge(&mut hits, "Repeated kiln ticket");

    let top = hits.first().expect("the ticket is on the page");
    assert_eq!(top.id.as_deref(), Some("Software-0001"));
    assert_eq!(top.text, "Repeated kiln ticket");
    assert_eq!(
        ljos_cli::search_finish_hint_with_claims(&hits, Some(&claims)),
        None,
        "the kept heading has no claim, so the other heading's claim must not name finish"
    );
}
