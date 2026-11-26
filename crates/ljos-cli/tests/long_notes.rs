//! ljos-34dh: long notes are org pages beside the ticket.
//!
//! `ljos note ID --file PATH` writes `notes/LEAF/SLUG.org` next to the
//! board's issue file and puts one stub line on the logbook, an org link to
//! the page. These tests run the real `vissue` and `deedar` on a git
//! tracker in commit mode.

ljos_test_boot::harness!();

#[path = "common/deed_world.rs"]
mod deed_world;

use deed_world::{answer, mcp, said, stderr, stdout, World, ISSUE};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

const OTHER: &str = "demo-c3d4";

fn notes_dir(world: &World) -> PathBuf {
    world.tracker().join("Software/demo/notes").join(ISSUE)
}

fn board(world: &World) -> PathBuf {
    world.tracker().join("Software/demo/issues.org")
}

/// `ljos` with Markdown converted by the converter built into ljos, so a
/// pandoc on the host does not change the page.
fn ljos(world: &World, args: &[&str]) -> Output {
    world
        .command(env!("CARGO_BIN_EXE_ljos"))
        .env("LJOS_PANDOC", "off")
        .args(args)
        .output()
        .unwrap()
}

fn ok(world: &World, args: &[&str]) -> String {
    let out = ljos(world, args);
    assert!(out.status.success(), "ljos {args:?}\n{}", said(&out));
    stdout(&out)
}

/// A second ticket on the board, committed, so a test can watch one
/// ticket's file while it writes another.
fn with_second_ticket(world: &World) {
    let mut text = std::fs::read_to_string(board(world)).unwrap();
    text.push_str(&format!(
        "* TODO [#B] Release notes\n:PROPERTIES:\n:ID:         {OTHER}\n:END:\n"
    ));
    world.write("tracker/Software/demo/issues.org", text);
    world.git(&["add", "."]);
    world.git(&["commit", "-q", "-m", "second ticket"]);
}

/// The logbook line that holds the link to `page`.
fn stub_line(world: &World, page: &str) -> String {
    let text = std::fs::read_to_string(board(world)).unwrap();
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains(&format!("notes/{ISSUE}/{page}]")))
        .collect();
    assert_eq!(lines.len(), 1, "one stub for {page}:\n{text}");
    lines[0].to_string()
}

fn committed_files(world: &World, rev: &str) -> Vec<String> {
    world
        .git(&["show", "--name-only", "--format=", rev])
        .lines()
        .map(str::to_string)
        .filter(|line| !line.is_empty())
        .collect()
}

/// A file passed by a relative path becomes an org page beside the
/// ticket: a header naming the ticket, kind, seat and creation time, the
/// Markdown converted, and one link on the logbook. The page and the stub
/// land in one commit, and the local index stays out of git.
#[test]
fn a_file_becomes_an_org_page_beside_the_ticket_with_a_link_stub() {
    let world = World::new();
    let body = "# Land r3\n\nMerged **green**. See `ci.yml` and [the run](https://example.org/run/7).\n\n* first\n* second\n\n```sh\ncargo test\n```\n";
    world.write("LAND.md", body);
    let before = world.commits();
    let said_out = ok(
        &world,
        &[
            "note", ISSUE, "--file", "LAND.md", "--kind", "land", "--title", "LAND r3",
        ],
    );
    assert!(
        said_out.contains(&format!(
            "noted {} B as Software/demo/notes/demo-a1b2/land-r3.org",
            body.len()
        )),
        "{said_out}"
    );
    let page = std::fs::read_to_string(notes_dir(&world).join("land-r3.org")).unwrap();
    assert!(
        page.starts_with(
            "#+title: LAND r3\n#+ticket: demo-a1b2\n#+kind: land\n#+seat: alice\n#+created: ["
        ),
        "{page}"
    );
    assert!(page.contains("#+source: LAND.md\n"), "{page}");
    assert!(
        page.ends_with(
            "\n* Land r3\n\nMerged *green*. See ~ci.yml~ and [[https://example.org/run/7][the run]].\n\n- first\n- second\n\n#+begin_src sh\ncargo test\n#+end_src\n"
        ),
        "{page}"
    );

    let stub = stub_line(&world, "land-r3.org");
    assert!(
        stub.contains(&format!(
            "long note land: [[file:notes/demo-a1b2/land-r3.org][LAND r3]] {} B 10 lines",
            body.len()
        )),
        "{stub}"
    );
    assert!(stub.len() < 200, "{} bytes: {stub}", stub.len());

    assert_eq!(world.commits(), before + 1);
    let files = committed_files(&world, "HEAD");
    assert!(
        files.contains(&"Software/demo/issues.org".to_string())
            && files.contains(&"Software/demo/notes/demo-a1b2/land-r3.org".to_string()),
        "{files:?}"
    );
    let index = world.tracker().join(".ljos/cache/notes.idx");
    assert!(index.is_file(), "the write did not index the page");
    world.git(&["check-ignore", "-q", ".ljos/cache/notes.idx"]);
    assert!(
        !world.git(&["status", "--porcelain"]).contains(".ljos"),
        "{}",
        world.git(&["status", "--porcelain"])
    );
}

/// Byte-identical bodies on the same ticket write nothing the second time.
/// The same bytes on another ticket are that ticket's own note.
#[test]
fn the_same_bytes_on_the_same_ticket_are_noted_once() {
    let world = World::new();
    with_second_ticket(&world);
    world.write("REPORT.md", "Report: all green.\n");
    ok(&world, &["note", ISSUE, "--file", "REPORT.md"]);
    let commits = world.commits();
    let files = world.tracker_files();
    let again = ok(
        &world,
        &["note", ISSUE, "--file", "REPORT.md", "--title", "other"],
    );
    assert!(
        again.contains(
            "already noted as Software/demo/notes/demo-a1b2/report-md.org; nothing written"
        ),
        "{again}"
    );
    assert_eq!(world.commits(), commits);
    assert_eq!(
        world.tracker_files(),
        files,
        "a duplicate changed the tracker"
    );

    ok(&world, &["note", OTHER, "--file", "REPORT.md"]);
    assert!(world
        .tracker()
        .join("Software/demo/notes")
        .join(OTHER)
        .join("report-md.org")
        .is_file());
}

/// Adding a note inserts its lines into the ticket's logbook and leaves
/// every other byte of the org file where it was: the bytes before the
/// insertion are the old file's prefix, the bytes after are its suffix.
#[test]
fn adding_a_note_does_not_rewrite_the_org_file() {
    let world = World::new();
    with_second_ticket(&world);
    // vissue writes its own header on the first write to a hand-made
    // file. Two plain notes settle it.
    ok(&world, &["note", ISSUE, "settle the header"]);
    ok(&world, &["note", OTHER, "settle the header"]);
    let before = std::fs::read(board(&world)).unwrap();
    world.write(
        "REPORT.md",
        format!("{}\n", "Long report line. ".repeat(500)),
    );
    ok(
        &world,
        &["note", OTHER, "--file", "REPORT.md", "--kind", "report"],
    );
    let after = std::fs::read(board(&world)).unwrap();

    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = before[prefix..]
        .iter()
        .rev()
        .zip(after[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    assert_eq!(
        prefix + suffix,
        before.len(),
        "the note changed bytes outside one insertion:\n{}",
        String::from_utf8_lossy(&after)
    );
    let inserted = &after[prefix..after.len() - suffix];
    assert!(
        inserted.len() < 400,
        "{}",
        String::from_utf8_lossy(inserted)
    );
    let other_at = String::from_utf8_lossy(&before).find(OTHER).unwrap();
    assert!(prefix > other_at, "the first ticket's bytes moved");
    assert!(
        String::from_utf8_lossy(inserted)
            .contains("[[file:notes/demo-c3d4/report-md.org][REPORT.md]]"),
        "{}",
        String::from_utf8_lossy(inserted)
    );
}

/// The stub stays under 200 bytes on the logbook however long the title.
#[test]
fn a_long_title_keeps_the_stub_under_200_bytes() {
    let world = World::new();
    world.write("gate.md", "Gate notes.\n");
    let title =
        "Adversarial gate on the tracker growth kit, second round, with every finding ".repeat(3);
    ok(
        &world,
        &[
            "note", ISSUE, "--file", "gate.md", "--kind", "gate", "--title", &title,
        ],
    );
    let names: Vec<String> = std::fs::read_dir(notes_dir(&world))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].len() <= 44, "{names:?}");
    let stub = stub_line(&world, &names[0]);
    assert!(stub.len() < 200, "{} bytes: {stub}", stub.len());
    assert!(stub.contains("...]] 12 B 1 lines"), "{stub}");
}

/// Past 256 KiB a note is kept whole with a warning. Past 1 MiB the page
/// keeps the first and last 128 KiB and a line naming the full size and
/// sha256; the full body is a deed in the local store, cited on the
/// ticket, and reads back byte for byte. `--refuse-trim` refuses instead.
#[test]
fn a_body_past_the_caps_warns_then_trims_into_the_deed_store() {
    let world = World::new();
    let line = "2026-11-25T22:00:00Z build step finished with status ok\n";
    let mid = line.repeat(300 * 1024 / line.len());
    world.write("mid.log", &mid);
    let out = ljos(&world, &["note", ISSUE, "--file", "mid.log"]);
    assert!(out.status.success(), "{}", said(&out));
    assert!(
        stderr(&out).contains("past the 262,144 B soft cap"),
        "{}",
        said(&out)
    );
    let page = std::fs::read_to_string(notes_dir(&world).join("mid-log.org")).unwrap();
    assert!(page.len() > mid.len() && !page.contains("ljos: trimmed"));
    assert!(page.contains("#+begin_example\n") && page.ends_with("#+end_example\n"));

    let big = line.repeat(2 * 1024 * 1024 / line.len());
    let path = world.write("big.log", &big);
    let commits = world.commits();
    let out = ljos(&world, &["note", ISSUE, "--file", &path, "--refuse-trim"]);
    assert!(!out.status.success(), "{}", said(&out));
    assert!(
        stderr(&out).contains("--refuse-trim keeps it whole"),
        "{}",
        said(&out)
    );
    assert!(!notes_dir(&world).join("big-log.org").exists());
    assert_eq!(world.commits(), commits);

    let out = ljos(&world, &["note", ISSUE, "--file", &path, "--kind", "log"]);
    assert!(out.status.success(), "{}", said(&out));
    let page = std::fs::read_to_string(notes_dir(&world).join("big-log.org")).unwrap();
    assert!(page.len() < 2 * 128 * 1024 + 4096, "{} bytes", page.len());
    let digest = {
        use sha2::Digest as _;
        hex::encode(sha2::Sha256::digest(big.as_bytes()))
    };
    let elision = page
        .lines()
        .find(|l| l.starts_with("[ljos: trimmed:"))
        .unwrap_or_else(|| panic!("no elision line"));
    assert!(
        elision.contains(&format!("{} B, sha256:{digest}, deed ", grouped(big.len()))),
        "{elision}"
    );
    assert!(
        elision.ends_with("; this note keeps the first and last 131,072 B]"),
        "{elision}"
    );
    assert!(
        page.contains(&format!("#+begin_example\n{line}")),
        "the head is gone"
    );
    assert!(
        page.ends_with(&format!("{line}#+end_example\n")),
        "the tail is gone"
    );
    let accession = elision
        .split("deed ")
        .nth(1)
        .and_then(|rest| rest.split(';').next())
        .unwrap()
        .to_string();
    let cited = ok(&world, &["deed", ISSUE]);
    assert!(cited.contains(&accession), "{cited}");
    let full = ljos(&world, &["deed", "--show", &accession]);
    assert!(full.status.success(), "{}", said(&full));
    assert!(
        full.stdout == big.as_bytes(),
        "the deed is not the full body"
    );
    let stub = stub_line(&world, "big-log.org");
    assert!(
        stub.contains(&format!("[big.log]] {} B ", grouped(big.len())))
            && stub.contains(" lines trimmed"),
        "{stub}"
    );
    assert!(stub.len() < 200, "{} bytes: {stub}", stub.len());
}

fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (at, ch) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Binary or encoded bytes are not a note. Each refusal exits non-zero,
/// names `deedar create file`, and leaves the tracker as it was.
#[test]
fn binary_and_base64_bodies_are_refused_with_the_deed_route() {
    let world = World::new();
    let files = world.tracker_files();
    let commits = world.commits();
    world.write("blob.dat", b"PK\x03\x04\x00\x00binary");
    world.write("latin.txt", [0x63, 0x61, 0x66, 0xe9, 0x0a]);
    world.write(
        "snap.txt",
        format!("snapshot\n{}\n", "QUJDRA".repeat(1_000)),
    );
    for (file, why) in [
        ("blob.dat", "holds a NUL byte"),
        ("latin.txt", "is not UTF-8 text"),
        (
            "snap.txt",
            "has a run of 6,000 bytes with no space on line 2",
        ),
    ] {
        let out = ljos(&world, &["note", ISSUE, "--file", file]);
        assert!(!out.status.success(), "{file}: {}", said(&out));
        let err = stderr(&out);
        assert!(err.contains(why), "{file}: {err}");
        assert!(err.contains("deedar create file"), "{file}: {err}");
        assert!(err.contains("Nothing was noted"), "{file}: {err}");
    }
    let empty = world.write("empty.md", "\n\n");
    let out = ljos(&world, &["note", ISSUE, "--file", &empty]);
    assert!(
        !out.status.success() && stderr(&out).contains("is empty"),
        "{}",
        said(&out)
    );
    assert_eq!(world.tracker_files(), files);
    assert_eq!(world.commits(), commits);
}

/// A short note past either cap is never refused or dropped: the text is
/// kept whole as a long note, the logbook gets only the stub, and the
/// command says so and exits 0.
#[test]
fn a_short_note_past_its_caps_becomes_a_long_note() {
    let world = World::new();
    let paragraph = format!("{}\n", "the proxy answered 429 again ".repeat(12));
    let big = format!("Retry findings\n\n{}", paragraph.repeat(14));
    let one_line = "the second run timed out at the same step ".repeat(60);
    assert!(big.len() > 4096 && big.lines().all(|l| l.len() < 2048));
    assert!(one_line.len() < 4096 && one_line.len() > 2048);
    for (text, why, page) in [
        (
            big.as_str(),
            format!("the note is {} bytes, past the 4096-byte cap", big.len()),
            "retry-findings.org",
        ),
        (
            one_line.as_str(),
            format!(
                "line 1 of the note is {} bytes, past the 2048-byte cap",
                one_line.len()
            ),
            "the-second-run-timed-out-at-the-same.org",
        ),
    ] {
        let commits = world.commits();
        let out = ljos(&world, &["note", ISSUE, text]);
        assert!(out.status.success(), "{}", said(&out));
        assert!(stderr(&out).contains(&why), "{}", said(&out));
        assert!(
            stderr(&out).contains("so it is kept as a long note"),
            "{}",
            said(&out)
        );
        assert!(
            stdout(&out).contains(&format!("as Software/demo/notes/demo-a1b2/{page}")),
            "{}",
            said(&out)
        );
        let kept = std::fs::read_to_string(notes_dir(&world).join(page)).unwrap();
        assert!(kept.contains("#+ticket: demo-a1b2\n"), "{kept}");
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            assert!(
                kept.contains(line.trim()),
                "the page lost {line:?}:\n{kept}"
            );
        }
        let stub = stub_line(&world, page);
        assert!(stub.len() < 200, "{} bytes: {stub}", stub.len());
        let board = std::fs::read_to_string(board(&world)).unwrap();
        assert!(
            board.lines().all(|l| l.len() < 300),
            "the text reached the logbook:\n{board}"
        );
        assert_eq!(world.commits(), commits + 1);
    }

    let files = world.tracker_files();
    let again = ljos(&world, &["note", ISSUE, &big]);
    assert!(again.status.success(), "{}", said(&again));
    assert!(
        stdout(&again)
            .contains("already noted as Software/demo/notes/demo-a1b2/retry-findings.org"),
        "{}",
        said(&again)
    );
    assert_eq!(world.tracker_files(), files);

    let encoded = format!("snapshot b64z:{}", "QUJD".repeat(5_000));
    let out = ljos(&world, &["note", ISSUE, &encoded]);
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(
        stderr(&out).contains("Nothing was noted") && stderr(&out).contains("deedar create file"),
        "{}",
        said(&out)
    );
    assert_eq!(world.tracker_files(), files);
}

/// `ljos notes` lists the pages; `ljos show --long` prints the newest, or
/// the one named, as it is on disk; stdin is read with `--file -`.
#[test]
fn notes_lists_and_show_long_prints_a_page() {
    let world = World::new();
    world.write("LAND.md", "Landed.\n");
    ok(
        &world,
        &["note", ISSUE, "--file", "LAND.md", "--kind", "land"],
    );
    let out = world
        .command(env!("CARGO_BIN_EXE_ljos"))
        .args([
            "note",
            ISSUE,
            "--file",
            "-",
            "--kind",
            "log",
            "--title",
            "smoke log",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"* step one\nstep two\n")?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(out.status.success(), "{}", said(&out));
    let log = std::fs::read_to_string(notes_dir(&world).join("smoke-log.org")).unwrap();
    assert!(
        log.ends_with("#+begin_example\n,* step one\nstep two\n#+end_example\n"),
        "{log}"
    );
    assert!(!log.contains("#+source:"), "{log}");

    let listed = ok(&world, &["notes", ISSUE]);
    assert!(listed.starts_with("demo-a1b2: 2 long notes\n"), "{listed}");
    assert!(
        listed.contains("LAND.md\n  land, ") && listed.contains("smoke log\n  log, "),
        "{listed}"
    );
    assert!(
        listed.contains("  Software/demo/notes/demo-a1b2/land-md.org\n"),
        "{listed}"
    );
    let rows: serde_json::Value =
        serde_json::from_str(&ok(&world, &["notes", ISSUE, "--json"])).unwrap();
    assert_eq!(rows.as_array().map(Vec::len), Some(2), "{rows}");
    assert_eq!(rows[0]["kind"], "land");
    assert_eq!(rows[0]["seat"], "alice");

    let page = std::fs::read_to_string(notes_dir(&world).join("land-md.org")).unwrap();
    assert_eq!(ok(&world, &["show", ISSUE, "--long", "land-md"]), page);
    assert_eq!(ok(&world, &["show", ISSUE, "--long", "land-md.org"]), page);
    assert_eq!(
        ok(&world, &["show", ISSUE, "--long"]),
        log,
        "the newest is the log"
    );
    let out = ljos(&world, &["show", ISSUE, "--long", "nope"]);
    assert!(
        !out.status.success() && stderr(&out).contains("no long note is nope"),
        "{}",
        said(&out)
    );

    // A page is plain org: an edit outside ljos is what show prints next.
    let edited = format!("{page}Edited by hand.\n");
    world.write("tracker/Software/demo/notes/demo-a1b2/land-md.org", &edited);
    assert_eq!(ok(&world, &["show", ISSUE, "--long", "land-md"]), edited);
}

/// An import notes each `.md`, `.txt` and `.log` file once, in one commit,
/// and says what it did with every file. A dry run writes nothing, and a
/// second run notes nothing.
#[test]
fn import_notes_kit_files_once_and_skips_the_rest() {
    let world = World::new();
    let kit = world.dir().join("kit");
    std::fs::create_dir_all(kit.join("sub")).unwrap();
    for (name, text) in [
        ("kit/LAND.md", "# Land\nok\n"),
        ("kit/REPORT.md", "# Report\nfine\n"),
        ("kit/build.log", "step 1\nstep 2\n"),
        ("kit/notes.txt", "plain text\n"),
        ("kit/kit.bundle", "not a note"),
        ("kit/0001-fix.patch", "diff --git"),
        ("kit/sub/inner.md", "below\n"),
    ] {
        world.write(name, text);
    }
    let files = world.tracker_files();
    let commits = world.commits();
    let dry = ok(
        &world,
        &["notes", "import", "kit", "--ticket", ISSUE, "--dry-run"],
    );
    assert_eq!(world.tracker_files(), files, "the dry run wrote");
    assert!(
        dry.contains("would note LAND.md as Software/demo/notes/demo-a1b2/land-md.org"),
        "{dry}"
    );
    assert!(
        dry.contains("skipped kit.bundle: not .md, .txt or .log"),
        "{dry}"
    );
    assert!(dry.contains("skipped 0001-fix.patch"), "{dry}");
    assert!(
        dry.contains("import: would note 4, already noted 0, skipped 2, refused 0"),
        "{dry}"
    );

    let done = ok(&world, &["notes", "import", "kit", "--ticket", ISSUE]);
    assert!(
        done.contains("import: noted 4, already noted 0, skipped 2, refused 0"),
        "{done}"
    );
    assert_eq!(world.commits(), commits + 1, "{done}");
    let committed = committed_files(&world, "HEAD");
    assert_eq!(committed.len(), 5, "{committed:?}");
    let land = std::fs::read_to_string(notes_dir(&world).join("land-md.org")).unwrap();
    assert!(land.contains("#+kind: land\n"), "{land}");
    let log = std::fs::read_to_string(notes_dir(&world).join("build-log.org")).unwrap();
    assert!(log.contains("#+kind: log\n#+seat: alice\n"), "{log}");
    assert!(!notes_dir(&world).join("inner-md.org").exists());

    let files = world.tracker_files();
    let again = ok(&world, &["notes", "import", "kit", "--ticket", ISSUE]);
    assert!(
        again.contains("import: noted 0, already noted 4, skipped 2, refused 0"),
        "{again}"
    );
    assert!(
        again.contains("already noted REPORT.md as Software/demo/notes/demo-a1b2/report-md.org"),
        "{again}"
    );
    assert_eq!(world.commits(), commits + 1);
    assert_eq!(world.tracker_files(), files);
}

/// An import with a file that is not a note notes the others and exits 1.
#[test]
fn import_names_a_refused_file_and_exits_1() {
    let world = World::new();
    std::fs::create_dir_all(world.dir().join("kit")).unwrap();
    world.write("kit/a.md", "fine\n");
    world.write("kit/b.txt", b"bad\0bytes");
    let out = ljos(&world, &["notes", "import", "kit", "--ticket", ISSUE]);
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    let text = stdout(&out);
    assert!(text.contains("noted a.md as"), "{text}");
    assert!(
        text.contains("refused b.txt: note:") && text.contains("NUL byte"),
        "{text}"
    );
    assert!(notes_dir(&world).join("a-md.org").is_file());
}

/// Run `args`, killing it after `secs`. `None` when it had to be killed.
fn bounded(world: &World, args: &[&str], secs: u64) -> Option<Output> {
    let mut child = world
        .command(env!("CARGO_BIN_EXE_ljos"))
        .env("LJOS_PANDOC", "off")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            return Some(child.wait_with_output().unwrap());
        }
        if start.elapsed() > Duration::from_secs(secs) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn mkfifo(path: &Path) {
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o644) }, 0, "mkfifo");
}

/// The search hot path never opens a long note and never touches the
/// index. A FIFO named like a page blocks any reader that opens it, so a
/// search that opened the notes would hang. The stub on the logbook is
/// still found. `--long` reads the index, refreshes a page edited outside
/// ljos, and leaves the FIFO alone.
#[test]
fn search_never_reads_note_bodies() {
    let world = World::new();
    world.write("LAND.md", "The quokka migration landed.\n");
    ok(
        &world,
        &[
            "note",
            ISSUE,
            "--file",
            "LAND.md",
            "--title",
            "Quokka landing",
        ],
    );
    let index = world.tracker().join(".ljos/cache/notes.idx");
    std::fs::remove_file(&index).unwrap();
    mkfifo(&notes_dir(&world).join("trap.org"));

    let out = bounded(&world, &["search", "Quokka landing"], 60)
        .expect("ljos search hung: it opened a note page");
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        text.contains(ISSUE),
        "the stub did not find the ticket:\n{text}"
    );
    let out = bounded(&world, &["search", "migration"], 60)
        .expect("ljos search hung: it opened a note page");
    assert!(!stdout(&out).contains("quokka migration"), "{}", said(&out));
    assert!(!index.exists(), "a plain search wrote the note index");

    let out = bounded(&world, &["search", "--long", "quokka", "migration"], 60)
        .expect("ljos search --long hung on the FIFO");
    assert!(out.status.success(), "{}", said(&out));
    assert!(
        stdout(&out).contains(
            "demo-a1b2  Quokka landing\n  Software/demo/notes/demo-a1b2/quokka-landing.org\n"
        ),
        "{}",
        said(&out)
    );
    assert!(index.is_file());

    let page = notes_dir(&world).join("quokka-landing.org");
    let mut text = std::fs::read_to_string(&page).unwrap();
    text.push_str("A wombat followed.\n");
    world.write(
        "tracker/Software/demo/notes/demo-a1b2/quokka-landing.org",
        &text,
    );
    let held = std::fs::read_to_string(&index).unwrap();
    bounded(&world, &["search", "wombat"], 60).expect("ljos search hung");
    assert_eq!(
        std::fs::read_to_string(&index).unwrap(),
        held,
        "a plain search refreshed the index"
    );
    let out = bounded(&world, &["search", "--long", "wombat"], 60).expect("hung");
    assert!(
        stdout(&out).contains("A wombat followed."),
        "{}",
        said(&out)
    );
    let out = bounded(&world, &["search", "--long", "platypus"], 60).expect("hung");
    assert!(
        stdout(&out).starts_with("No long note holds"),
        "{}",
        said(&out)
    );
}

/// Sitting, recall and the ticket page show the stub, never the body.
#[test]
fn sitting_recall_and_show_print_the_stub_not_the_body() {
    let world = World::new();
    world.write("REPORT.md", "Marker body text kangaroo-7f3a.\n");
    ok(
        &world,
        &["note", ISSUE, "--file", "REPORT.md", "--kind", "report"],
    );
    let page = ok(&world, &["show", ISSUE]);
    assert!(
        page.contains("long note report:") && page.contains("report-md.org"),
        "{page}"
    );
    assert!(!page.contains("kangaroo-7f3a"), "{page}");
    for args in [&["sitting", ISSUE][..], &["recall", ISSUE][..]] {
        let out = ljos(&world, args);
        let text = format!("{}{}", stdout(&out), stderr(&out));
        assert!(
            !text.contains("kangaroo-7f3a"),
            "{args:?} printed the body:\n{text}"
        );
    }
}

/// The MCP verbs: `ljos_note` with `file`, `ljos_notes` and `ljos_show_long`.
#[test]
fn mcp_notes_a_file_lists_and_reads_it() {
    let world = World::new();
    let path = world.write("GATE.md", "Gate passed.\n");
    let call = |id: u64, name: &str, arguments: serde_json::Value| {
        serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": name, "arguments": arguments}})
    };
    // The server answers calls concurrently, so the reads go in a second
    // session once the write has landed.
    let wrote = mcp(
        &world,
        &[call(
            1,
            "ljos_note",
            serde_json::json!({"issue": ISSUE, "file": path, "kind": "gate", "seat": "alice"}),
        )],
    );
    let (failed, text) = tool_text(&wrote, 1);
    assert!(
        !failed && text.contains("noted 13 B as Software/demo/notes/demo-a1b2/gate-md.org"),
        "{text}"
    );
    let read = mcp(
        &world,
        &[
            call(2, "ljos_notes", serde_json::json!({"issue": ISSUE})),
            call(
                3,
                "ljos_show_long",
                serde_json::json!({"issue": ISSUE, "note": "gate-md"}),
            ),
            call(
                4,
                "ljos_show_long",
                serde_json::json!({"issue": ISSUE, "note": "missing"}),
            ),
        ],
    );
    let (failed, text) = tool_text(&read, 2);
    assert!(!failed && text.contains("GATE.md\n  gate, "), "{text}");
    let (failed, text) = tool_text(&read, 3);
    assert!(
        !failed && text.contains("#+kind: gate\n") && text.contains("Gate passed."),
        "{text}"
    );
    let (failed, text) = tool_text(&read, 4);
    assert!(failed && text.contains("no long note is missing"), "{text}");
}

/// The answer's text, unwrapped from the `{"text": ...}` block a habitat
/// verb returns.
fn tool_text(answers: &[serde_json::Value], id: u64) -> (bool, String) {
    let (failed, text) = answer(answers, id);
    let inner = serde_json::Deserializer::from_str(&text)
        .into_iter::<serde_json::Value>()
        .next()
        .and_then(Result::ok)
        .and_then(|v| v["text"].as_str().map(str::to_string));
    (failed, inner.unwrap_or(text))
}

/// `ljos_note` with text past a tracker note's caps keeps it as a long
/// note, and says so. Encoded text is a tool error, never a success that
/// wrote nothing.
#[test]
fn mcp_text_past_the_caps_is_kept_as_a_long_note() {
    let world = World::new();
    let text = format!(
        "MCP findings\n\n{}",
        "the cache warmed after the third try\n".repeat(150)
    );
    assert!(text.len() > 4096);
    let call = |id: u64, text: &str| {
        serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "ljos_note", "arguments": {"issue": ISSUE, "text": text, "seat": "alice"}}})
    };
    let answers = mcp(&world, &[call(1, &text)]);
    let (failed, said_text) = tool_text(&answers, 1);
    assert!(!failed, "{said_text}");
    assert!(
        said_text.contains("so it is kept as a long note"),
        "{said_text}"
    );
    assert!(
        said_text.contains("as Software/demo/notes/demo-a1b2/mcp-findings.org"),
        "{said_text}"
    );
    let page = std::fs::read_to_string(notes_dir(&world).join("mcp-findings.org")).unwrap();
    assert_eq!(
        page.matches("the cache warmed after the third try").count(),
        150,
        "{page}"
    );
    assert!(stub_line(&world, "mcp-findings.org").len() < 200);

    let files = world.tracker_files();
    let encoded = format!("snapshot b64z:{}", "QUJD".repeat(5_000));
    let answers = mcp(&world, &[call(2, &encoded)]);
    let (failed, said_text) = tool_text(&answers, 2);
    assert!(failed, "an encoded note read as noted: {said_text}");
    assert!(said_text.contains("Nothing was noted"), "{said_text}");
    assert_eq!(world.tracker_files(), files);
}
