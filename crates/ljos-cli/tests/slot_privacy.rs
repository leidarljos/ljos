//! A search reads the tracker's memory slot only for this seat, and only
//! through plain files.
//!
//! The slot is `<tracker>/.ljos/memory`. Its `kept/` and `attempts/`
//! directories are shared by every seat that uses the tracker, so a draft
//! of another seat is not this seat's to find. A file in it that is a link
//! or a pipe, or that every user can write, is not read. `ljos forget` and
//! `ljos memory integrate` read the slot files they rewrite the same way.
//! The writer is down in every test.

ljos_test_boot::harness!();

use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const QUERY: &str = "kiln schedule";

fn slot(root: &Path) -> PathBuf {
    let home = root.join("tracker").join(".ljos").join("memory");
    std::fs::create_dir_all(&home).unwrap();
    home
}

fn kept_row(id: &str, text: &str, seat: &str) -> String {
    serde_json::json!({
        "id": id,
        "text": text,
        "kind": "lesson",
        "seat": seat,
        "entities": [format!("seat:{seat}")],
        "at": "2026-11-01T08:00:00Z",
        "effect": "not-posted",
    })
    .to_string()
}

fn put(dir: &Path, name: &str, body: &str, mode: u32) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

fn search(root: &Path, seat: &str) -> String {
    search_with(root, seat, &[])
}

fn search_with(root: &Path, seat: &str, more: &[&str]) -> String {
    let mut args = vec!["search", QUERY];
    args.extend_from_slice(more);
    ljos(root, seat, &args)
}

/// Run `ljos ARGS` against the slot with the writer down, under a wall
/// clock, so a read that waits on a pipe fails the test.
fn ljos(root: &Path, seat: &str, args: &[&str]) -> String {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", closed.local_addr().unwrap().port());
    drop(closed);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("TMPDIR", root)
        .env("VISSUE_ROOT", root.join("tracker"))
        .env("PACKSET_URL", url)
        .env("PACKSET_WORKSPACE", "ws")
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
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            return format!("TIMED OUT: ljos {} did not end", args.join(" "));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    format!(
        "status {:?}\n{}{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn fifo(path: &Path) {
    let name = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: the name is a valid C string and mkfifo keeps no pointer.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
}

#[test]
fn a_kept_row_of_another_seat_is_not_found_by_this_seat() {
    let root = tempfile::tempdir().unwrap();
    let kept = slot(root.path()).join("kept");
    put(
        &kept,
        "a.json",
        &kept_row("local:a", "ALICE-DRAFT kiln schedule", "alice"),
        0o644,
    );
    put(
        &kept,
        "b.json",
        &kept_row("local:b", "BOB-DRAFT kiln schedule", "bob"),
        0o644,
    );
    let said = search(root.path(), "bob");
    assert!(said.contains("BOB-DRAFT"), "{said}");
    assert!(
        !said.contains("ALICE-DRAFT"),
        "another seat's draft:\n{said}"
    );
}

#[test]
fn an_attempt_row_of_another_seat_is_not_found_by_this_seat() {
    let root = tempfile::tempdir().unwrap();
    let attempts = slot(root.path()).join("attempts");
    put(
        &attempts,
        "a.json",
        &kept_row("attempt:a", "ALICE-ATTEMPT kiln schedule", "alice"),
        0o644,
    );
    put(
        &attempts,
        "b.json",
        &kept_row("attempt:b", "BOB-ATTEMPT kiln schedule", "bob"),
        0o644,
    );
    let said = search_with(root.path(), "bob", &["--attempts"]);
    assert!(said.contains("BOB-ATTEMPT"), "{said}");
    assert!(!said.contains("ALICE-ATTEMPT"), "{said}");
}

#[test]
fn an_attempt_row_as_the_writer_stores_it_is_found_by_its_own_seat_only() {
    let root = tempfile::tempdir().unwrap();
    let attempts = slot(root.path()).join("attempts");
    for (name, seat) in [("a.json", "alice"), ("b.json", "bob")] {
        let row = serde_json::json!({
            "id": format!("attempt:{seat}"),
            "text": format!("{}-FAILED kiln schedule", seat.to_uppercase()),
            "kind": "lesson",
            "seat": seat,
            "at": "2026-11-01T08:00:00Z",
            "effect": "failed",
        })
        .to_string();
        put(&attempts, name, &row, 0o644);
    }
    let said = search_with(root.path(), "bob", &["--attempts"]);
    assert!(said.contains("BOB-FAILED"), "{said}");
    assert!(!said.contains("ALICE-FAILED"), "{said}");
}

#[test]
fn a_kept_row_with_no_seat_is_not_found() {
    let root = tempfile::tempdir().unwrap();
    let kept = slot(root.path()).join("kept");
    let bare = serde_json::json!({"id": "local:n", "text": "NO-SEAT kiln schedule"}).to_string();
    put(&kept, "n.json", &bare, 0o644);
    let said = search(root.path(), "bob");
    assert!(!said.contains("NO-SEAT"), "{said}");
}

#[test]
fn a_kept_file_every_user_can_write_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let kept = slot(root.path()).join("kept");
    put(
        &kept,
        "w.json",
        &kept_row("local:w", "WORLD-WRITABLE kiln schedule", "bob"),
        0o666,
    );
    let said = search(root.path(), "bob");
    assert!(!said.contains("WORLD-WRITABLE"), "{said}");
}

#[test]
fn a_kept_file_that_is_a_link_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let kept = slot(root.path()).join("kept");
    let outside = put(
        root.path(),
        "elsewhere.json",
        &kept_row("local:l", "LINKED-KEPT kiln schedule", "bob"),
        0o600,
    );
    std::fs::create_dir_all(&kept).unwrap();
    symlink(&outside, kept.join("l.json")).unwrap();
    let said = search(root.path(), "bob");
    assert!(!said.contains("LINKED-KEPT"), "{said}");
}

#[test]
fn a_kept_file_that_is_a_pipe_or_a_device_does_not_stop_the_search() {
    let root = tempfile::tempdir().unwrap();
    let kept = slot(root.path()).join("kept");
    put(
        &kept,
        "b.json",
        &kept_row("local:b", "BOB-DRAFT kiln schedule", "bob"),
        0o644,
    );
    fifo(&kept.join("p.json"));
    symlink("/dev/zero", kept.join("z.json")).unwrap();
    let said = search(root.path(), "bob");
    assert!(!said.contains("TIMED OUT"), "{said}");
    assert!(said.contains("BOB-DRAFT"), "{said}");
}

#[test]
fn an_import_index_that_is_a_link_or_a_pipe_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let imports = slot(root.path()).join("imports").join("bob");
    let outside = put(
        root.path(),
        "index.jsonl",
        &format!(
            "{}\n",
            kept_row("imp:1", "LINKED-INDEX kiln schedule", "bob")
        ),
        0o600,
    );
    std::fs::create_dir_all(imports.join("one")).unwrap();
    symlink(&outside, imports.join("one").join("index.jsonl")).unwrap();
    std::fs::create_dir_all(imports.join("two")).unwrap();
    fifo(&imports.join("two").join("index.jsonl"));
    let said = search(root.path(), "bob");
    assert!(!said.contains("TIMED OUT"), "{said}");
    assert!(!said.contains("LINKED-INDEX"), "{said}");
}

#[test]
fn an_import_index_of_this_seat_is_still_found() {
    let root = tempfile::tempdir().unwrap();
    let imports = slot(root.path()).join("imports").join("bob").join("one");
    put(
        &imports,
        "index.jsonl",
        &format!("{}\n", kept_row("imp:1", "IMPORTED kiln schedule", "alice")),
        0o644,
    );
    let said = search(root.path(), "bob");
    assert!(said.contains("IMPORTED"), "{said}");
}

fn manifest(slot: &Path) {
    put(slot, "manifest.json", r#"{"import_id":"imp1"}"#, 0o644);
}

#[test]
fn forget_and_integrate_do_not_wait_on_an_index_that_is_a_pipe() {
    let root = tempfile::tempdir().unwrap();
    let slot = slot(root.path()).join("imports").join("bob").join("imp1");
    manifest(&slot);
    fifo(&slot.join("index.jsonl"));
    let said = ljos(root.path(), "bob", &["forget", "imp:1"]);
    assert!(!said.contains("TIMED OUT"), "{said}");
    let said = ljos(root.path(), "bob", &["memory", "integrate", "imp1"]);
    assert!(!said.contains("TIMED OUT"), "{said}");
}

#[test]
fn forget_does_not_wait_on_a_kept_file_that_is_a_pipe() {
    let root = tempfile::tempdir().unwrap();
    let kept = slot(root.path()).join("kept");
    std::fs::create_dir_all(&kept).unwrap();
    fifo(&kept.join("p.json"));
    let said = ljos(root.path(), "bob", &["forget", "local:p"]);
    assert!(!said.contains("TIMED OUT"), "{said}");
}

#[test]
fn forget_does_not_parse_an_index_that_is_a_link() {
    let root = tempfile::tempdir().unwrap();
    let slot = slot(root.path()).join("imports").join("bob").join("imp1");
    manifest(&slot);
    let body = format!(
        "{}\n",
        kept_row("imp:1", "LINKED-INDEX kiln schedule", "bob")
    );
    let outside = put(root.path(), "outside.jsonl", &body, 0o600);
    symlink(&outside, slot.join("index.jsonl")).unwrap();
    let said = ljos(root.path(), "bob", &["forget", "imp:1"]);
    assert!(
        !said.contains("is a symlink"),
        "the link was read and matched:\n{said}"
    );
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), body);
}

#[test]
fn forget_does_not_parse_an_index_with_another_name() {
    let root = tempfile::tempdir().unwrap();
    let slot = slot(root.path()).join("imports").join("bob").join("imp1");
    manifest(&slot);
    let body = format!(
        "{}\n",
        kept_row("imp:1", "LINKED-INDEX kiln schedule", "bob")
    );
    let outside = put(root.path(), "outside.jsonl", &body, 0o600);
    std::fs::hard_link(&outside, slot.join("index.jsonl")).unwrap();
    let said = ljos(root.path(), "bob", &["forget", "imp:1"]);
    assert!(!said.contains("TIMED OUT"), "{said}");
    let index = std::fs::read_to_string(slot.join("index.jsonl")).unwrap();
    assert!(
        !index.contains("retired"),
        "a hard-linked index was rewritten:\n{index}\n{said}"
    );
}

#[test]
fn forget_retires_a_row_in_a_slot_its_group_can_write() {
    let root = tempfile::tempdir().unwrap();
    let home = slot(root.path());
    let slot = home.join("imports").join("bob").join("imp1");
    manifest(&slot);
    let index = put(
        &slot,
        "index.jsonl",
        &format!("{}\n", kept_row("imp:1", "GROUP-SLOT kiln schedule", "bob")),
        0o664,
    );
    for dir in [
        home.as_path(),
        &home.join("imports"),
        &home.join("imports/bob"),
        &slot,
    ] {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o775)).unwrap();
    }
    let said = ljos(root.path(), "bob", &["forget", "imp:1"]);
    let text = std::fs::read_to_string(&index).unwrap();
    assert!(text.contains("\"retired\":true"), "{said}\n{text}");
}

#[test]
fn integrate_does_not_wait_on_a_pipe_index_when_a_kept_row_names_the_import() {
    let root = tempfile::tempdir().unwrap();
    let home = slot(root.path());
    let slot = home.join("imports").join("bob").join("imp1");
    manifest(&slot);
    fifo(&slot.join("index.jsonl"));
    let row = serde_json::json!({
        "id": "imp:1",
        "text": "KEPT-FOR-IMPORT kiln schedule",
        "seat": "bob",
        "import_id": "imp1",
    });
    put(&home.join("kept"), "k.json", &row.to_string(), 0o644);
    let said = ljos(root.path(), "bob", &["memory", "integrate", "imp1"]);
    assert!(!said.contains("TIMED OUT"), "{said}");
}

#[test]
fn a_pipe_where_a_slot_directory_belongs_does_not_stop_search_forget_or_integrate() {
    let root = tempfile::tempdir().unwrap();
    let home = slot(root.path());
    let seat = home.join("imports").join("bob");
    put(
        &seat.join("imp1"),
        "index.jsonl",
        &format!("{}\n", kept_row("imp:1", "IMPORTED kiln schedule", "bob")),
        0o644,
    );
    manifest(&seat.join("imp1"));
    fifo(&seat.join("hole"));
    let said = search(root.path(), "bob");
    assert!(!said.contains("TIMED OUT"), "{said}");
    assert!(said.contains("IMPORTED"), "{said}");
    let said = ljos(root.path(), "bob", &["memory", "integrate", "imp1"]);
    assert!(!said.contains("TIMED OUT"), "{said}");
    let said = ljos(root.path(), "bob", &["show", "imp:1"]);
    assert!(!said.contains("TIMED OUT"), "{said}");
    let said = ljos(root.path(), "bob", &["forget", "imp:1"]);
    assert!(!said.contains("TIMED OUT"), "{said}");
}

#[test]
fn an_import_index_or_kept_row_with_another_name_is_not_found() {
    let root = tempfile::tempdir().unwrap();
    let home = slot(root.path());
    let body = format!(
        "{}\n",
        kept_row("imp:1", "LINKED-INDEX kiln schedule", "bob")
    );
    let outside = put(root.path(), "outside.jsonl", &body, 0o644);
    let one = home.join("imports").join("bob").join("one");
    std::fs::create_dir_all(&one).unwrap();
    std::fs::hard_link(&outside, one.join("index.jsonl")).unwrap();
    let kept = home.join("kept");
    let draft = put(
        root.path(),
        "draft.json",
        &kept_row("local:k", "LINKED-KEPT kiln schedule", "bob"),
        0o644,
    );
    std::fs::create_dir_all(&kept).unwrap();
    std::fs::hard_link(&draft, kept.join("k.json")).unwrap();
    let said = search(root.path(), "bob");
    assert!(!said.contains("LINKED-INDEX"), "{said}");
    assert!(!said.contains("LINKED-KEPT"), "{said}");
    let said = ljos(root.path(), "bob", &["show", "imp:1"]);
    assert!(!said.contains("LINKED-INDEX"), "{said}");
}

#[test]
fn forget_does_not_follow_a_link_above_the_slot() {
    let root = tempfile::tempdir().unwrap();
    let home = slot(root.path());
    let elsewhere = root.path().join("elsewhere");
    let body = format!(
        "{}\n",
        kept_row("imp:1", "LINKED-SEAT kiln schedule", "bob")
    );
    let index = put(&elsewhere.join("imp1"), "index.jsonl", &body, 0o644);
    manifest(&elsewhere.join("imp1"));
    std::fs::create_dir_all(home.join("imports")).unwrap();
    symlink(&elsewhere, home.join("imports").join("bob")).unwrap();
    let said = ljos(root.path(), "bob", &["forget", "imp:1"]);
    assert!(!said.contains("TIMED OUT"), "{said}");
    assert_eq!(
        std::fs::read_to_string(&index).unwrap(),
        body,
        "a slot behind a linked seat directory was rewritten:\n{said}"
    );
}
