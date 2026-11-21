//! A read that falls back to the listing a hook kept on disk reads only
//! that seat's listing, under the authorization the reader carries now.
//!
//! The writer is down in most tests, so the only rows are the files the
//! tests keep under the data home. The hook tests answer a stamp and
//! count the reads of the atoms.

ljos_test_boot::harness!();

use std::io::{Read as _, Write as _};
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const WORKSPACE: &str = "ws";
const QUERY: &str = "kiln schedule";

fn tag(token: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(token.as_bytes())[..6])
}

fn row(id: &str, text: &str, seat: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "kind": "lesson",
        "text": text,
        "entities": [format!("seat:{seat}")],
        "ts": "2026-11-01T08:00:00Z",
    })
}

fn keep(data: &Path, name: &str, trusted: bool, atoms: Vec<serde_json::Value>) {
    let dir = data.join("ljos");
    std::fs::create_dir_all(&dir).unwrap();
    let doc = serde_json::json!({"stamp": "an-older-write", "trusted": trusted, "atoms": atoms});
    std::fs::write(dir.join(name), doc.to_string()).unwrap();
}

fn search(root: &Path, seat: &str, token: Option<&str>) -> String {
    search_in(root, seat, token, WORKSPACE)
}

fn search_in(root: &Path, seat: &str, token: Option<&str>, workspace: &str) -> String {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", closed.local_addr().unwrap().port());
    drop(closed);
    run(root, seat, token, workspace, &url, false)
}

/// Run `ljos search` with a bounded address space and a wall clock, so a
/// listing that is not a plain file fails the test and does not fill the
/// machine or wait for ever.
fn run(
    root: &Path,
    seat: &str,
    token: Option<&str>,
    workspace: &str,
    url: &str,
    hook: bool,
) -> String {
    run_in(root, &root.join("data"), seat, token, workspace, url, hook)
}

/// [`run`] with `XDG_DATA_HOME` set to `data`.
fn run_in(
    root: &Path,
    data: &Path,
    seat: &str,
    token: Option<&str>,
    workspace: &str,
    url: &str,
    hook: bool,
) -> String {
    let mut cmd = Command::new("/bin/sh");
    cmd.args([
        "-c",
        "ulimit -v 4000000; exec \"$0\" search \"$1\"",
        env!("CARGO_BIN_EXE_ljos"),
        QUERY,
    ])
    .current_dir(root)
    .env_clear()
    .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
    .env("PATH", "/usr/bin:/bin")
    .env("HOME", root)
    .env("XDG_CONFIG_HOME", root.join("config"))
    .env("XDG_DATA_HOME", data)
    .env("XDG_STATE_HOME", root.join("state"))
    .env("XDG_CACHE_HOME", root.join("cache"))
    .env("XDG_RUNTIME_DIR", root.join("run"))
    .env("TMPDIR", root)
    .env("PACKSET_URL", url)
    .env("PACKSET_WORKSPACE", workspace)
    .env("PACKSET_AUTH", "off")
    .env("LJOS_HOOKD", "off")
    .env("LJOS_TRACKER_GIT", "off")
    .env("LJOS_SEARCH_WARMUP_WAIT", "0")
    .env("LJOS_SEARCH_WAIT", "0")
    .env("LJOS_SEARCH_BUSY_WAIT", "0")
    .env("NO_COLOR", "1")
    .env("LJOS_SEAT", seat)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    if hook {
        cmd.env("LJOS_IN_HOOK", "1");
    }
    if let Some(token) = token {
        cmd.env("PACKSET_TOKEN", token);
    }
    let mut child = cmd.spawn().unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            return "TIMED OUT: the search did not end".to_string();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out: Output = child.wait_with_output().unwrap();
    format!(
        "status {:?}\n{}{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A writer that is busy for a search, names `stamp` as its last write and
/// counts the reads of `/v1/atoms`.
fn busy_writer(stamp: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let atoms = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&atoms);
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let counted = Arc::clone(&counted);
            std::thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 4096];
                let Ok(n) = sock.read(&mut buf) else { return };
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let path = path.split('?').next().unwrap_or("/");
                let (code, body) = match path {
                    "/health" => (200, "packsetd ok".to_string()),
                    "/v1/status" => (
                        200,
                        format!(
                            r#"{{"last_write_ts":"{stamp}","embedder":{{"available":true,"answering":true,"enabled":true}}}}"#
                        ),
                    ),
                    "/v1/atoms" => {
                        counted.fetch_add(1, Ordering::SeqCst);
                        (200, r#"{"atoms":[]}"#.to_string())
                    }
                    _ => (
                        503,
                        r#"{"error":"packsetd is busy: memory pressure; try again"}"#.to_string(),
                    ),
                };
                let resp = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    (url, atoms)
}

fn keep_as(data: &Path, name: &str, stamp: &str, text: &str, mode: u32) {
    let dir = data.join("ljos");
    std::fs::create_dir_all(&dir).unwrap();
    let doc = serde_json::json!({
        "stamp": stamp,
        "seat": "bob",
        "trusted": false,
        "atoms": [row("p", text, "bob")],
    });
    let path = dir.join(name);
    std::fs::write(&path, doc.to_string()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn the_cache_a_token_reader_kept_is_not_printed_to_a_reader_with_none() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    keep(
        &data,
        &format!("listing-{WORKSPACE}-seat.json"),
        true,
        vec![row(
            "old",
            "ALICE-OLD kiln schedule is the private plan",
            "alice",
        )],
    );
    let said = search(root.path(), "bob", None);
    assert!(!said.contains("ALICE-OLD"), "{said}");
}

#[test]
fn an_unauthenticated_reader_gets_only_the_open_listing_of_its_own_seat() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let bob = tag("bob-token");
    keep(
        &data,
        &format!("listing-{WORKSPACE}@alice-open.json"),
        false,
        vec![row("a", "ALICE-OPEN kiln schedule", "alice")],
    );
    keep(
        &data,
        &format!("listing-{WORKSPACE}@bob-t{bob}.json"),
        true,
        vec![row("b", "BOB-TRUSTED kiln schedule", "bob")],
    );
    keep(
        &data,
        &format!("listing-{WORKSPACE}@bob-open.json"),
        false,
        vec![row("c", "BOB-OPEN kiln schedule", "bob")],
    );
    let said = search(root.path(), "bob", None);
    assert!(said.contains("BOB-OPEN"), "{said}");
    assert!(!said.contains("ALICE-OPEN"), "{said}");
    assert!(!said.contains("BOB-TRUSTED"), "{said}");
}

#[test]
fn a_reader_of_another_seat_does_not_get_alices_listing() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let alice = tag("alice-token");
    let bob = tag("bob-token");
    keep(
        &data,
        &format!("listing-{WORKSPACE}@alice-t{alice}.json"),
        true,
        vec![row("a", "ALICE-OWN kiln schedule", "alice")],
    );
    keep(
        &data,
        &format!("listing-{WORKSPACE}@alice-t{bob}.json"),
        true,
        vec![row("a2", "ALICE-UNDER-BOB-TOKEN kiln schedule", "alice")],
    );
    keep(
        &data,
        &format!("listing-{WORKSPACE}@bob-t{alice}.json"),
        true,
        vec![row("b2", "BOB-UNDER-ALICE-TOKEN kiln schedule", "bob")],
    );
    let mut secret = row("s", "SECRET-ROW kiln schedule", "bob");
    secret["source"] = serde_json::json!({"api_key": "abc"});
    keep(
        &data,
        &format!("listing-{WORKSPACE}@bob-t{bob}.json"),
        true,
        vec![
            row("b", "BOB-OWN kiln schedule", "bob"),
            row("ab", "ALICE-IN-BOB-FILE kiln schedule", "alice"),
            secret,
        ],
    );
    let said = search(root.path(), "bob", Some("bob-token"));
    assert!(said.contains("BOB-OWN"), "{said}");
    for left_out in [
        "ALICE-OWN",
        "ALICE-UNDER-BOB-TOKEN",
        "BOB-UNDER-ALICE-TOKEN",
        "ALICE-IN-BOB-FILE",
        "SECRET-ROW",
    ] {
        assert!(!said.contains(left_out), "{left_out} printed:\n{said}");
    }
}

#[test]
fn a_workspace_with_a_dot_does_not_read_the_name_with_the_dot_dropped() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    keep(
        &data,
        "listing-ab@bob-open.json",
        false,
        vec![row("c", "DOT-COLLISION kiln schedule", "bob")],
    );
    let said = search_in(root.path(), "bob", None, "a.b");
    assert!(
        !said.contains("DOT-COLLISION"),
        "workspace a.b read ab:\n{said}"
    );
    keep(
        &data,
        "listing-a.b@bob-open.json",
        false,
        vec![row("d", "DOT-OWN kiln schedule", "bob")],
    );
    let said = search_in(root.path(), "bob", None, "a.b");
    assert!(
        said.contains("DOT-OWN"),
        "workspace a.b lost its own:\n{said}"
    );
}

#[test]
fn names_that_differ_in_a_character_the_old_name_dropped_keep_their_own_listing() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    for (name, text) in [
        ("listing-a%2Fb@bob-open.json", "SLASH-OWN kiln schedule"),
        ("listing-ab@bob-open.json", "PLAIN-OWN kiln schedule"),
    ] {
        keep(&data, name, false, vec![row("c", text, "bob")]);
    }
    let said = search_in(root.path(), "bob", None, "a/b");
    assert!(said.contains("SLASH-OWN"), "{said}");
    assert!(!said.contains("PLAIN-OWN"), "{said}");
}

#[test]
fn a_hook_does_not_read_a_group_writable_listing() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    keep_as(
        &data,
        "listing-ws@bob-open.json",
        "stamp-9",
        "HOOK-POISON kiln schedule",
        0o666,
    );
    let (url, atoms) = busy_writer("stamp-9");
    let said = run(root.path(), "bob", None, WORKSPACE, &url, true);
    assert!(
        !said.contains("HOOK-POISON"),
        "hook read a 0666 listing:\n{said}"
    );
    assert!(
        atoms.load(Ordering::SeqCst) > 0,
        "the hook never asked for the atoms"
    );
}

#[test]
fn a_hook_reads_a_listing_only_its_user_can_write() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    keep_as(
        &data,
        "listing-ws@bob-open.json",
        "stamp-9",
        "HOOK-OWN kiln schedule",
        0o600,
    );
    let (url, atoms) = busy_writer("stamp-9");
    let said = run(root.path(), "bob", None, WORKSPACE, &url, true);
    assert!(said.contains("HOOK-OWN"), "{said}");
    assert_eq!(
        atoms.load(Ordering::SeqCst),
        0,
        "the kept listing was not used"
    );
}

fn plant_symlink_to_alices_listing(root: &Path) {
    let data = root.join("data");
    let dir = data.join("ljos");
    std::fs::create_dir_all(&dir).unwrap();
    let target = root.join("alice-listing.json");
    let doc = serde_json::json!({
        "stamp": "stamp-9",
        "trusted": false,
        "atoms": [
            row("t", "ALICE-TAGGED kiln schedule", "alice"),
            {"id": "u", "kind": "lesson", "text": "UNTAGGED-LEAK kiln schedule", "ts": "2026-11-01T08:00:00Z"},
        ],
    });
    std::fs::write(&target, doc.to_string()).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, dir.join("listing-ws@bob-open.json")).unwrap();
}

fn said_with(root: &Path, hook: bool) -> String {
    if hook {
        let (url, _) = busy_writer("stamp-9");
        run(root, "bob", None, WORKSPACE, &url, true)
    } else {
        search(root, "bob", None)
    }
}

#[test]
fn a_symlink_to_another_listing_is_not_read_by_a_search() {
    let root = tempfile::tempdir().unwrap();
    plant_symlink_to_alices_listing(root.path());
    let said = said_with(root.path(), false);
    assert!(
        !said.contains("UNTAGGED-LEAK"),
        "symlink was followed:\n{said}"
    );
    assert!(!said.contains("ALICE-TAGGED"), "{said}");
}

#[test]
fn a_symlink_to_another_listing_is_not_read_by_a_hook() {
    let root = tempfile::tempdir().unwrap();
    plant_symlink_to_alices_listing(root.path());
    let said = said_with(root.path(), true);
    assert!(
        !said.contains("UNTAGGED-LEAK"),
        "symlink was followed:\n{said}"
    );
}

fn plant(root: &Path, make: impl Fn(&Path)) {
    let dir = root.join("data").join("ljos");
    std::fs::create_dir_all(&dir).unwrap();
    make(&dir.join("listing-ws@bob-open.json"));
}

#[test]
fn a_listing_that_is_a_fifo_is_left_alone() {
    for hook in [false, true] {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), |path| {
            let fifo = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap();
            // SAFETY: the path is a valid C string and mkfifo keeps no pointer.
            assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        });
        let said = said_with(root.path(), hook);
        assert!(
            !said.contains("TIMED OUT"),
            "hook {hook} waited on a fifo:\n{said}"
        );
        assert!(said.contains("status Some("), "hook {hook}:\n{said}");
    }
}

#[test]
fn a_listing_that_is_a_symlink_to_dev_zero_is_left_alone() {
    for hook in [false, true] {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), |path| symlink("/dev/zero", path).unwrap());
        let said = said_with(root.path(), hook);
        assert!(
            !said.contains("TIMED OUT"),
            "hook {hook} read /dev/zero:\n{said}"
        );
        assert!(said.contains("status Some("), "hook {hook}:\n{said}");
    }
}

fn alices_two_rows() -> serde_json::Value {
    serde_json::json!({
        "stamp": "stamp-9",
        "seat": "bob",
        "trusted": false,
        "atoms": [
            row("t", "ALICE-TAGGED kiln schedule", "alice"),
            {"id": "u", "kind": "lesson", "text": "ALICE-UNTAGGED kiln schedule", "ts": "2026-11-01T08:00:00Z"},
        ],
    })
}

fn plant_hardlink_to_alices_listing(root: &Path) {
    let dir = root.join("data").join("ljos");
    std::fs::create_dir_all(&dir).unwrap();
    let alice = dir.join("listing-ws@alice-open.json");
    std::fs::write(&alice, alices_two_rows().to_string()).unwrap();
    std::fs::set_permissions(&alice, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::hard_link(&alice, dir.join("listing-ws@bob-open.json")).unwrap();
}

#[test]
fn a_hardlink_to_another_seats_listing_is_not_read_by_a_search() {
    let root = tempfile::tempdir().unwrap();
    plant_hardlink_to_alices_listing(root.path());
    let said = said_with(root.path(), false);
    assert!(!said.contains("ALICE-"), "hardlink was read:\n{said}");
}

#[test]
fn a_hardlink_to_another_seats_listing_is_not_read_by_a_hook() {
    let root = tempfile::tempdir().unwrap();
    plant_hardlink_to_alices_listing(root.path());
    let said = said_with(root.path(), true);
    assert!(!said.contains("ALICE-"), "hardlink was read:\n{said}");
}

fn plant_for(root: &Path, header_seat: Option<&str>, rows: Vec<serde_json::Value>) {
    let dir = root.join("data").join("ljos");
    std::fs::create_dir_all(&dir).unwrap();
    let mut doc = serde_json::json!({"stamp": "stamp-9", "trusted": false, "atoms": rows});
    if let Some(seat) = header_seat {
        doc["seat"] = seat.into();
    }
    let path = dir.join("listing-ws@bob-open.json");
    std::fs::write(&path, doc.to_string()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn a_hook_reads_the_listing_it_kept_for_its_own_seat() {
    let root = tempfile::tempdir().unwrap();
    plant_for(
        root.path(),
        Some("bob"),
        vec![row("b", "BOB-OWN kiln schedule", "bob")],
    );
    let (url, atoms) = busy_writer("stamp-9");
    let said = run(root.path(), "bob", None, WORKSPACE, &url, true);
    assert!(said.contains("BOB-OWN"), "{said}");
    assert_eq!(
        atoms.load(Ordering::SeqCst),
        0,
        "the kept listing was not used"
    );
}

#[test]
fn a_hook_does_not_read_a_listing_that_names_another_seat() {
    for header in [Some("alice"), None] {
        let root = tempfile::tempdir().unwrap();
        plant_for(
            root.path(),
            header,
            vec![row("a", "ALICE-IN-BOB-FILE kiln schedule", "alice")],
        );
        let (url, atoms) = busy_writer("stamp-9");
        let said = run(root.path(), "bob", None, WORKSPACE, &url, true);
        assert!(
            atoms.load(Ordering::SeqCst) > 0,
            "header {header:?}: the kept listing was used:\n{said}"
        );
    }
}

fn plant_listing_in(dir: &Path, text: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("listing-ws@bob-open.json");
    let doc = serde_json::json!({
        "stamp": "stamp-9",
        "seat": "bob",
        "trusted": false,
        "atoms": [row("p", text, "bob")],
    });
    std::fs::write(&path, doc.to_string()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn a_listing_in_a_directory_others_can_write_is_not_read() {
    for mode in [0o777, 0o770, 0o707] {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("data").join("ljos");
        plant_listing_in(&dir, "DIR-WRITABLE-ROW kiln schedule");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
        let said = said_with(root.path(), false);
        assert!(
            !said.contains("DIR-WRITABLE-ROW"),
            "directory {mode:o} was read:\n{said}"
        );
    }
}

#[test]
fn a_listing_in_a_directory_that_is_a_link_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("elsewhere");
    plant_listing_in(&real, "LINKED-DIR-ROW kiln schedule");
    let data = root.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    symlink(&real, data.join("ljos")).unwrap();
    let said = said_with(root.path(), false);
    assert!(!said.contains("LINKED-DIR-ROW"), "{said}");
    let said = said_with(root.path(), true);
    assert!(!said.contains("LINKED-DIR-ROW"), "{said}");
}

#[test]
fn a_hook_takes_group_and_other_write_off_the_directory_it_keeps_a_listing_in() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("data").join("ljos");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    let (url, atoms) = busy_writer("stamp-9");
    let said = run(root.path(), "bob", None, WORKSPACE, &url, true);
    assert!(
        atoms.load(Ordering::SeqCst) > 0,
        "no listing was read:\n{said}"
    );
    let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode & 0o022, 0, "directory mode {mode:o}");
    assert!(
        dir.join("listing-ws@bob-open.json").is_file(),
        "no listing kept"
    );
}

#[test]
fn a_hook_keeps_no_listing_in_a_directory_that_is_a_link() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("elsewhere");
    std::fs::create_dir_all(&real).unwrap();
    let data = root.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    symlink(&real, data.join("ljos")).unwrap();
    let (url, atoms) = busy_writer("stamp-9");
    let said = run(root.path(), "bob", None, WORKSPACE, &url, true);
    assert!(
        atoms.load(Ordering::SeqCst) > 0,
        "no listing was read:\n{said}"
    );
    let kept: Vec<_> = std::fs::read_dir(&real)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.contains("listing") || name.contains("tokens"))
        .collect();
    assert!(kept.is_empty(), "wrote through the link: {kept:?}");
}

fn kept_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .filter(|name| name.contains("listing") || name.contains("tokens"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_listing_under_a_parent_others_can_write_is_not_read_or_kept() {
    for mode in [0o777, 0o770] {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let dir = data.join("ljos");
        plant_listing_in(&dir, "OPEN-PARENT-ROW kiln schedule");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(mode)).unwrap();
        for hook in [false, true] {
            let said = said_with(root.path(), hook);
            assert!(
                !said.contains("OPEN-PARENT-ROW"),
                "parent {mode:o}, hook {hook}, was read:\n{said}"
            );
        }

        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(mode)).unwrap();
        let (url, _) = busy_writer("stamp-9");
        let said = run(root.path(), "bob", None, WORKSPACE, &url, true);
        let kept = kept_files(&data.join("ljos"));
        assert!(kept.is_empty(), "parent {mode:o}, kept {kept:?}:\n{said}");
    }
}

#[test]
fn a_listing_reached_through_a_linked_data_home_is_read_only_through_safe_directories() {
    let root = tempfile::tempdir().unwrap();
    let open = root.path().join("open");
    plant_listing_in(
        &open.join("data").join("ljos"),
        "BEHIND-OPEN-ROW kiln schedule",
    );
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
    symlink(open.join("data"), root.path().join("data")).unwrap();
    let said = said_with(root.path(), false);
    assert!(
        !said.contains("BEHIND-OPEN-ROW"),
        "a link to a data home under a 777 directory was followed:\n{said}"
    );

    let good = root.path().join("good");
    plant_listing_in(&good.join("ljos"), "SAFE-ROW kiln schedule");
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", closed.local_addr().unwrap().port());
    drop(closed);
    symlink(&good, open.join("link")).unwrap();
    let said = run_in(
        root.path(),
        &open.join("link"),
        "bob",
        None,
        WORKSPACE,
        &url,
        false,
    );
    assert!(
        !said.contains("SAFE-ROW"),
        "a link in a 777 directory was followed:\n{said}"
    );

    symlink(&good, root.path().join("mine")).unwrap();
    let said = run_in(
        root.path(),
        &root.path().join("mine"),
        "bob",
        None,
        WORKSPACE,
        &url,
        false,
    );
    assert!(
        said.contains("SAFE-ROW"),
        "a link this user made to a safe directory was not followed:\n{said}"
    );
}
