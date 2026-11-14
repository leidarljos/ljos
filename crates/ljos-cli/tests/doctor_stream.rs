//! `ljos doctor` prints each row when that check finishes. A later probe
//! that sits there does not hold the earlier rows. The ready line follows
//! the short report's rows. `--all` is the same rows, with no ready line.

ljos_test_boot::harness!();

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn seat(root: &Path) -> PathBuf {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for sub in ["home", "run", "config", "data", "state", "cache"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    script(
        &bin,
        "ljos",
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo ljos 0.0.0; fi\nexit 0\n",
    );
    // `--version` answers at once. `log head` is the deed-store probe,
    // which doctor waits out. The earlier rows must already be printed.
    script(
        &bin,
        "deedar",
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo deedar 0.0.0; exit 0; fi\nsleep 2\nexit 0\n",
    );
    script(&bin, "curl", "#!/bin/sh\nexit 1\n");
    bin
}

struct Run {
    child: std::process::Child,
    rx: mpsc::Receiver<String>,
    latest: String,
}

impl Run {
    fn start(root: &Path, bin: &Path, args: &[&str]) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.arg("doctor")
            .args(args)
            .env_clear()
            .env("HOME", root.join("home"))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("PACKSET_URL", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_NO_AUTOSTART", "1")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut tmp = [0u8; 512];
            loop {
                match stdout.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
                    }
                }
            }
        });
        Self {
            child,
            rx,
            latest: String::new(),
        }
    }

    fn pump(&mut self, wait: Duration) {
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline {
            match self.rx.recv_timeout(Duration::from_millis(30)) {
                Ok(text) => self.latest = text,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    /// Read until `needle` is in stdout, or `limit` passes.
    fn until(&mut self, needle: &str, limit: Duration) -> &str {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            match self.rx.recv_timeout(Duration::from_millis(30)) {
                Ok(text) => self.latest = text,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if self.latest.contains(needle) {
                break;
            }
        }
        &self.latest
    }

    fn finish(&mut self) -> &str {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            match self.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(text) => self.latest = text,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if self.child.try_wait().ok().flatten().is_some() {
                self.pump(Duration::from_millis(200));
                break;
            }
        }
        let _ = self.child.wait();
        self.pump(Duration::from_millis(200));
        &self.latest
    }
}

#[test]
fn doctor_prints_an_early_row_before_a_slow_later_check() {
    let root = tempfile::tempdir().unwrap();
    let bin = seat(root.path());
    let mut run = Run::start(root.path(), &bin, &["--all"]);
    let early = run.until("\tljos\t", Duration::from_secs(8)).to_string();
    assert!(
        early.contains("\tljos\t"),
        "the ljos row was not printed before the slow check finished:\n{early}"
    );
    // The deed store is asked after the binary rows. Its line is `deeds`.
    // A report printed in one piece has that line in the same bytes as ljos.
    assert!(
        !early.contains("deeds"),
        "the deed-store row was held back with the earlier rows:\n{early}"
    );
    let full = run.finish().to_string();
    let ljos_at = full.find("\tljos\t").unwrap_or_else(|| panic!("{full}"));
    let store_at = full.find("deeds").unwrap_or_else(|| panic!("{full}"));
    assert!(ljos_at < store_at, "{full}");
    assert!(
        !full.contains('\r'),
        "a pipe got a progress rewrite:\n{full}"
    );
}

#[test]
fn the_short_doctor_streams_rows_and_prints_the_ready_line_after_them() {
    let root = tempfile::tempdir().unwrap();
    let bin = seat(root.path());
    let mut run = Run::start(root.path(), &bin, &[]);
    let early = run
        .until("\tljos-mcp\t", Duration::from_secs(8))
        .to_string();
    assert!(
        early.contains("\tljos-mcp\t"),
        "a row that needs action did not appear before the slow check:\n{early}"
    );
    // The ready line is printed after every check, including the deed store.
    assert!(
        !early.contains("ljos is ") && !early.contains("ljos runs"),
        "the ready line was printed with the first row:\n{early}"
    );
    let full = run.finish().to_string();
    let row_at = full
        .find("\tljos-mcp\t")
        .unwrap_or_else(|| panic!("no ljos-mcp row:\n{full}"));
    let ready_at = full
        .find("ljos is ")
        .or_else(|| full.find("ljos runs"))
        .unwrap_or_else(|| panic!("no ready line:\n{full}"));
    assert!(
        row_at < ready_at,
        "the ready line was printed before the rows:\n{full}"
    );
    assert!(
        !full.contains('\r'),
        "a pipe got a progress rewrite:\n{full}"
    );
}
