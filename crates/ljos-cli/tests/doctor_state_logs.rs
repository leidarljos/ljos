//! `ljos doctor` on the guard bypass log and the note queue: it reads
//! both and leaves them as it found them.

use std::io::Read;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// Long enough for a doctor that asks crates.io for each version, short
/// enough that one waiting on a lock fails instead of holding the suite.
const DEADLINE: Duration = Duration::from_secs(60);

/// A home for doctor runs, with the directories both logs live in.
struct Home(tempfile::TempDir);

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("home");
        for sub in ["state/ljos", "data/ljos", "config", "outside"] {
            std::fs::create_dir_all(dir.path().join(sub)).expect("dirs");
        }
        Self(dir)
    }

    fn root(&self) -> &Path {
        self.0.path()
    }

    fn bypass(&self) -> PathBuf {
        self.root().join("data/ljos/bypass.jsonl")
    }

    fn queue(&self) -> PathBuf {
        self.root().join("state/ljos/note-queue.jsonl")
    }

    /// `ljos doctor` with `args`: its exit code and stdout, or `None`
    /// when it was still running at [`DEADLINE`] and had to be killed.
    fn doctor(&self, args: &[&str]) -> Option<(i32, String)> {
        let root = self.root();
        let mut child = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .arg("doctor")
            .args(args)
            .env_clear()
            .env("HOME", root)
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("PATH", "/usr/bin:/bin")
            .env("PACKSET_URL", "off")
            .env("LJOS_NO_AUTOSTART", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ljos doctor");
        let pid = child.id() as libc::pid_t;
        let mut stdout = child.stdout.take().expect("stdout");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut out = String::new();
            let _ = stdout.read_to_string(&mut out);
            let _ = tx.send((child.wait().ok().and_then(|s| s.code()), out));
        });
        match rx.recv_timeout(DEADLINE) {
            Ok((code, out)) => Some((code.unwrap_or(-1), out)),
            Err(_) => {
                // SAFETY: kill takes a pid and a signal; the child is ours.
                unsafe { libc::kill(pid, libc::SIGKILL) };
                None
            }
        }
    }

    /// Every entry in the two ljos directories with what [`on_disk`]
    /// reads of it, so a file doctor added, removed or replaced shows.
    fn tree(&self) -> Vec<(PathBuf, OnDisk)> {
        let mut out = Vec::new();
        for dir in ["state/ljos", "data/ljos"] {
            for entry in std::fs::read_dir(self.root().join(dir)).expect("read dir") {
                let path = entry.expect("entry").path();
                let seen = on_disk(&path);
                out.push((path, seen));
            }
        }
        out.sort();
        out
    }
}

fn set_mtime(path: &Path, at: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open")
        .set_modified(at)
        .expect("set mtime");
}

/// A year ahead of the clock.
fn ahead() -> SystemTime {
    SystemTime::now() + Duration::from_secs(365 * 24 * 3600)
}

/// What lies at a path: its kind, its inode, and the bytes of a regular
/// file or of the file a link names.
type OnDisk = (String, u64, Vec<u8>);

/// [`OnDisk`] for `path`, read without following a link at it and
/// without opening a fifo.
fn on_disk(path: &Path) -> OnDisk {
    let meta = std::fs::symlink_metadata(path).expect("lstat");
    let kind = meta.file_type();
    if kind.is_symlink() {
        let target = std::fs::read_link(path).expect("readlink");
        let named = std::fs::read(&target).unwrap_or_default();
        (format!("link to {}", target.display()), meta.ino(), named)
    } else if kind.is_file() {
        (
            "file".into(),
            meta.ino(),
            std::fs::read(path).expect("read"),
        )
    } else if kind.is_fifo() {
        ("fifo".into(), meta.ino(), Vec::new())
    } else {
        ("other".into(), meta.ino(), Vec::new())
    }
}

/// The doctor line for the row `name`.
fn row<'a>(out: &'a str, name: &str) -> Option<&'a str> {
    out.lines()
        .find(|line| line.split('\t').nth(1) == Some(name))
}

fn mkfifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).expect("path");
    // SAFETY: mkfifo takes a NUL-terminated path that lives until it
    // returns, and a mode.
    let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    assert_eq!(rc, 0, "mkfifo: {}", std::io::Error::last_os_error());
}

const BYPASS_ROW: &[u8] =
    b"{\"kind\":\"switch\",\"command\":\"forged\",\"how\":\"h\",\"rule\":\"r\",\"ts\":\"t\"}\n";
const QUEUE_ROW: &[u8] =
    b"{\"issue\":\"ljos-old\",\"text\":\"kept\",\"workspace\":\"w\",\"at\":1,\"kind\":\"note\"}\n";

/// A fifo or a symlink at either log is left as doctor found it: the
/// fifo stays a fifo, the link and the file it names keep their bytes,
/// nothing lands beside them, and doctor returns and names the log
/// damaged in its summary and in a row the short doctor shows: `warn`
/// for the bypass log, `no` for the note queue.
#[test]
fn doctor_leaves_a_fifo_or_a_symlink_at_either_log_as_it_found_it() {
    for kind in ["fifo", "symlink"] {
        for (name, row_bytes) in [("guard bypass", BYPASS_ROW), ("note queue", QUEUE_ROW)] {
            let home = Home::new();
            let log = if name == "guard bypass" {
                home.bypass()
            } else {
                home.queue()
            };
            if kind == "fifo" {
                mkfifo(&log);
            } else {
                let target = home.root().join("outside/named.jsonl");
                std::fs::write(&target, row_bytes).expect("target");
                std::os::unix::fs::symlink(&target, &log).expect("symlink");
            }
            let before = home.tree();
            let (_, out) = home
                .doctor(&[])
                .unwrap_or_else(|| panic!("doctor hung on a {kind} at the {name}"));
            assert_eq!(home.tree(), before, "{kind} at the {name}:\n{out}");
            let (word, log) = if name == "guard bypass" {
                ("warn", "the guard bypass log")
            } else {
                ("no", "the note queue")
            };
            let line = row(&out, name).unwrap_or_else(|| panic!("no {name} row:\n{out}"));
            assert!(
                line.starts_with(word) && line.contains("damaged"),
                "{kind}: {line}"
            );
            let summary = out.lines().next().unwrap();
            assert!(summary.contains(&format!("{log} is damaged")), "{summary}");
        }
    }
}

/// The review's bypass log: a row, then one torn off two hours ago.
/// Doctor warns about it in the short output and leaves it alone.
#[test]
fn doctor_warns_about_an_aged_torn_bypass_log() {
    let home = Home::new();
    let mut torn = BYPASS_ROW.to_vec();
    torn.extend_from_slice(br#"{"kind":"switch","command":"torn"#);
    std::fs::write(home.bypass(), &torn).expect("bypass log");
    set_mtime(
        &home.bypass(),
        SystemTime::now() - Duration::from_secs(2 * 3600),
    );
    let before = home.tree();
    let (_, out) = home.doctor(&[]).expect("doctor hung");
    assert_eq!(home.tree(), before, "{out}");
    let line = row(&out, "guard bypass").unwrap_or_else(|| panic!("no bypass row:\n{out}"));
    assert!(
        line.starts_with("warn\tguard bypass\t") && line.contains("an unfinished last row"),
        "{line}"
    );
    let summary = out.lines().next().unwrap();
    assert!(
        summary.contains("the guard bypass log is damaged"),
        "{summary}"
    );
}

/// A note, then a whole note still missing its newline: 151 bytes.
const KEPT_THEN_UNFINISHED: &[u8] = concat!(
    r#"{"issue":"ljos-old","text":"kept","workspace":"w","at":1,"kind":"note"}"#,
    "\n",
    r#"{"issue":"ljos-new","text":"just written","workspace":"w","at":2,"kind":"note"}"#,
)
.as_bytes();

/// Doctor reads the note queue and writes none of it. A last row
/// without its newline, touched a moment ago or dated ahead of the
/// clock, is not taken for one being written and then lost to a rewrite
/// of the rows that parse. Touched a moment ago it may be one being
/// written; a year ahead it is not, and doctor calls the queue damaged.
#[test]
fn doctor_keeps_every_byte_of_the_note_queue() {
    for (when, at, word) in [
        ("just now", SystemTime::now(), "ok"),
        ("ahead", ahead(), "no"),
    ] {
        let home = Home::new();
        std::fs::write(home.queue(), KEPT_THEN_UNFINISHED).expect("queue");
        set_mtime(&home.queue(), at);
        let before = home.tree();
        let (_, out) = home
            .doctor(&["--all"])
            .unwrap_or_else(|| panic!("doctor hung on a queue touched {when}"));
        assert_eq!(home.tree(), before, "touched {when}:\n{out}");
        assert_eq!(std::fs::read(home.queue()).unwrap(), KEPT_THEN_UNFINISHED);
        let line = row(&out, "note queue").unwrap_or_else(|| panic!("no queue row:\n{out}"));
        assert!(line.starts_with(&format!("{word}\t")), "{when}: {line}");
    }
}

/// A row cut short and then ended with a newline, which is what a writer
/// that caps a torn tail leaves, is damage in either log, alone or after
/// a whole row: the short doctor shows it and the bytes stay.
#[test]
fn doctor_names_a_cut_row_with_its_newline_damaged() {
    for (name, whole) in [("guard bypass", BYPASS_ROW), ("note queue", QUEUE_ROW)] {
        for first in [&b""[..], whole] {
            let home = Home::new();
            let log = if name == "guard bypass" {
                home.bypass()
            } else {
                home.queue()
            };
            let mut bytes = first.to_vec();
            bytes.extend_from_slice(b"{\"issue\":\"ljos-new\",\"te\n");
            std::fs::write(&log, &bytes).expect("log");
            let before = home.tree();
            let (_, out) = home.doctor(&[]).expect("doctor hung");
            assert_eq!(home.tree(), before, "{name}:\n{out}");
            let word = if name == "guard bypass" { "warn" } else { "no" };
            let line = row(&out, name).unwrap_or_else(|| panic!("no {name} row:\n{out}"));
            assert!(
                line.starts_with(word) && line.contains("damaged"),
                "{name}: {line}"
            );
        }
    }
}

/// Another process holding the note-queue lock does not hold doctor: it
/// tries the lock once, says the queue is busy in a row the short doctor
/// shows, and says so no longer once the lock is let go. A queue dated a
/// year ahead is damaged, and that is what the row says.
#[test]
fn doctor_says_busy_while_the_note_queue_lock_is_held() {
    use std::os::unix::io::AsRawFd;
    for (when, at, said) in [
        ("just now", SystemTime::now(), "warn\tnote queue\tbusy: "),
        ("ahead", ahead(), "no\tnote queue\t"),
    ] {
        let home = Home::new();
        std::fs::write(home.queue(), KEPT_THEN_UNFINISHED).expect("queue");
        set_mtime(&home.queue(), at);
        let lock = std::fs::File::create(home.root().join("state/ljos/note-queue.lock"))
            .expect("lock file");
        // SAFETY: flock on a descriptor this test keeps open to the end.
        assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0);
        let before = home.tree();
        let (_, out) = home
            .doctor(&[])
            .unwrap_or_else(|| panic!("doctor waited on the held queue lock ({when})"));
        assert_eq!(home.tree(), before, "touched {when}:\n{out}");
        let line = row(&out, "note queue").unwrap_or_else(|| panic!("no queue row:\n{out}"));
        assert!(line.starts_with(said), "{when}: {line}");
        drop(lock);
        let (_, out) = home.doctor(&["--all"]).expect("doctor hung");
        let line = row(&out, "note queue").unwrap_or_else(|| panic!("no queue row:\n{out}"));
        assert!(!line.contains("busy"), "{when}, let go: {line}");
    }
}
