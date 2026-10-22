//! `ljos doctor` on the guard bypass log and the note queue: it reads
//! both and leaves them as it found them.

use std::io::Read;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

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
/// and doctor returns and names the log damaged.
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
            let before = on_disk(&log);
            let (_, out) = home
                .doctor(&["--all"])
                .unwrap_or_else(|| panic!("doctor hung on a {kind} at the {name}"));
            assert_eq!(on_disk(&log), before, "{kind} at the {name}:\n{out}");
            let line = row(&out, name).unwrap_or_else(|| panic!("no {name} row:\n{out}"));
            assert!(line.contains("damaged"), "{kind}: {line}");
        }
    }
}
