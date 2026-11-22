//! A slow working directory must not sit in front of the tracker deadline.
//!
//! `ljos file -p` and `ljos sitting` were run from a FUSE mount. The first
//! stat of that directory, or git opening `.git` there, printed nothing
//! for minutes. A named project leaves that directory unread. A stat that
//! does not answer is given up, and a tracker that does not answer fails
//! with nothing on stdout.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const SLOW: &str = "ljos-slow-cwd";

const SLOWSTAT: &str = r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>

static int slow(const char *p) {
    return p && strstr(p, "ljos-slow-cwd") != 0;
}

static void hit(void) {
    const char *path = getenv("LJOS_SLOW_HIT");
    int fd;
    if (!path || !path[0]) return;
    fd = open(path, O_CREAT | O_WRONLY | O_APPEND, 0644);
    if (fd < 0) return;
    if (write(fd, "stat\n", 5) < 0) {
    }
    close(fd);
}

static void block(const char *p) {
    if (!slow(p)) return;
    hit();
    sleep(30);
}

int statx(int d, const char *restrict p, int fl, unsigned m, struct statx *restrict b) {
    static int (*real)(int, const char *restrict, int, unsigned, struct statx *restrict);
    if (!real) real = dlsym(RTLD_NEXT, "statx");
    block(p);
    if (!real) return -1;
    return real(d, p, fl, m, b);
}

int stat(const char *restrict p, struct stat *restrict b) {
    static int (*real)(const char *restrict, struct stat *restrict);
    if (!real) real = dlsym(RTLD_NEXT, "stat");
    block(p);
    if (!real) return -1;
    return real(p, b);
}

int lstat(const char *restrict p, struct stat *restrict b) {
    static int (*real)(const char *restrict, struct stat *restrict);
    if (!real) real = dlsym(RTLD_NEXT, "lstat");
    block(p);
    if (!real) return -1;
    return real(p, b);
}

int newfstatat(int d, const char *restrict p, struct stat *restrict b, int fl) {
    static int (*real)(int, const char *restrict, struct stat *restrict, int);
    if (!real) real = dlsym(RTLD_NEXT, "newfstatat");
    block(p);
    if (!real) return -1;
    return real(d, p, b, fl);
}
"#;

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn slow_lib(dir: &Path) -> std::path::PathBuf {
    let src = dir.join("slowstat.c");
    let so = dir.join("slowstat.so");
    std::fs::write(&src, SLOWSTAT).unwrap();
    let out = Command::new("gcc")
        .args(["-shared", "-fPIC", "-o"])
        .arg(&so)
        .arg(&src)
        .output()
        .unwrap_or_else(|err| panic!("gcc: {err}"));
    assert!(
        out.status.success(),
        "gcc slowstat: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    so
}

struct Seat {
    dir: tempfile::TempDir,
    home: std::path::PathBuf,
    tracker: std::path::PathBuf,
    bin: std::path::PathBuf,
    slow: std::path::PathBuf,
    hit: std::path::PathBuf,
    so: std::path::PathBuf,
}

impl Seat {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let tracker = dir.path().join("tracker");
        let bin = dir.path().join("bin");
        let slow = dir.path().join(SLOW);
        let hit = dir.path().join("slow-hit");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(tracker.join("Software")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&slow).unwrap();
        std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let so = slow_lib(dir.path());
        Self {
            dir,
            home,
            tracker,
            bin,
            slow,
            hit,
            so,
        }
    }

    fn run(&self, cwd: &Path, args: &[&str], extra: &[(&str, &str)]) -> (Output, Duration) {
        let path = format!("{}:/usr/bin:/bin", self.bin.display());
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(cwd)
            .env_clear()
            .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("LJOS_SEAT", "ada")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("VISSUE_ROOT", &self.tracker)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        std::fs::create_dir_all(self.dir.path().join("run")).unwrap();
        for (key, value) in extra {
            if value.is_empty() {
                cmd.env_remove(key);
            } else {
                cmd.env(key, value);
            }
        }
        let started = Instant::now();
        let mut child = cmd.spawn().unwrap();
        let limit = Duration::from_secs(6);
        let status = loop {
            if started.elapsed() > limit {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "still running after {limit:?}: cwd {} args {args:?}",
                    cwd.display()
                );
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(30)),
                Err(err) => panic!("{err}"),
            }
        };
        let took = started.elapsed();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        if let Some(mut pipe) = child.stdout.take() {
            use std::io::Read as _;
            let _ = pipe.read_to_end(&mut stdout);
        }
        if let Some(mut pipe) = child.stderr.take() {
            use std::io::Read as _;
            let _ = pipe.read_to_end(&mut stderr);
        }
        let out = Output {
            status,
            stdout,
            stderr,
        };
        (out, took)
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_named_project_does_not_state_a_slow_working_directory() {
    let seat = Seat::new();
    script(
        &seat.bin,
        "vissue",
        "#!/bin/sh\nprintf '%s\\n' 'demo-abcd  TODO  [#C]  a title'\n",
    );
    let (out, took) = seat.run(
        &seat.slow,
        &["file", "-p", "demo", "--top", "a title"],
        &[
            ("LD_PRELOAD", seat.so.to_str().unwrap()),
            ("LJOS_SLOW_HIT", seat.hit.to_str().unwrap()),
        ],
    );
    let said = text(&out);
    assert!(
        took < Duration::from_secs(5),
        "file waited {took:?}\n{said}"
    );
    assert!(!seat.hit.exists(), "the slow directory was stated\n{said}");
    assert!(out.status.success(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "demo-abcd",
        "{said}"
    );
}

#[test]
fn a_slow_stat_of_an_unnamed_directory_is_given_up() {
    let seat = Seat::new();
    script(&seat.bin, "vissue", "#!/bin/sh\nexec sleep 30\n");
    let (out, took) = seat.run(
        &seat.slow,
        &["file", "--top", "a title"],
        &[
            ("LD_PRELOAD", seat.so.to_str().unwrap()),
            ("LJOS_SLOW_HIT", seat.hit.to_str().unwrap()),
            ("VISSUE_ROOT", ""),
        ],
    );
    let said = text(&out);
    assert!(
        took < Duration::from_secs(5),
        "file waited {took:?}\n{said}"
    );
    assert!(seat.hit.exists(), "the probe never ran\n{said}");
}

#[test]
fn a_fifo_git_link_does_not_hold_a_named_project() {
    let seat = Seat::new();
    script(
        &seat.bin,
        "vissue",
        "#!/bin/sh\nprintf '%s\\n' 'demo-abcd  TODO  [#C]  a title'\n",
    );
    let cwd = seat.dir.path().join("fifo-cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let made = Command::new("mkfifo")
        .arg(cwd.join(".git"))
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo");
    let (out, took) = seat.run(&cwd, &["file", "-p", "demo", "--top", "a title"], &[]);
    let said = text(&out);
    assert!(
        took < Duration::from_secs(5),
        "file waited {took:?}\n{said}"
    );
    assert!(out.status.success(), "{said}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "demo-abcd");
}

#[test]
fn a_tracker_that_does_not_answer_prints_nothing_and_fails() {
    let seat = Seat::new();
    script(&seat.bin, "vissue", "#!/bin/sh\nexec sleep 60\n");
    let cwd = seat.dir.path().join("fast");
    std::fs::create_dir_all(&cwd).unwrap();
    let (out, took) = seat.run(
        &cwd,
        &["file", "-p", "demo", "--top", "a title"],
        &[("LJOS_TRACKER_LOCK_WAIT", "1")],
    );
    let said = text(&out);
    assert!(
        took < Duration::from_secs(8),
        "file waited {took:?}\n{said}"
    );
    assert!(!out.status.success(), "{said}");
    assert!(
        String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "stdout was {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        said.contains("tracker did not answer in 1s; the tracker was not written, so run it again"),
        "{said}"
    );
}

#[test]
fn an_umbrella_file_does_not_open_every_child_board() {
    let seat = Seat::new();
    script(
        &seat.bin,
        "vissue",
        "#!/bin/sh\nprintf '%s\\n' 'Software-abcd  TODO  [#C]  a title'\n",
    );
    let nested = seat.tracker.join("Software/alpha/deep");
    std::fs::create_dir_all(&nested).unwrap();
    let made = Command::new("mkfifo")
        .arg(nested.join("issues.org"))
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo");
    let cwd = seat.dir.path().join("fast");
    std::fs::create_dir_all(&cwd).unwrap();
    let (out, took) = seat.run(
        &cwd,
        &["file", "-p", "Software", "--top", "a title"],
        &[("LJOS_TRACKER_GIT", "commit")],
    );
    let said = text(&out);
    assert!(
        took < Duration::from_secs(5),
        "file opened a child board and waited {took:?}\n{said}"
    );
    assert!(out.status.success(), "{said}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "Software-abcd");
}
