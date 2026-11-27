//! A slow working directory must not sit in front of the tracker deadline.
//!
//! `ljos file -p` and `ljos sitting` were run from a FUSE mount. The first
//! stat of that directory, or git opening `.git` there, printed nothing
//! for minutes. A named project leaves that directory unread. A stat that
//! does not answer is given up, and a tracker that does not answer fails
//! with nothing on stdout.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
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
    ljos_cli::install_executable(&path, body).unwrap();
}

fn slow_lib(dir: &Path) -> std::path::PathBuf {
    let src = dir.join("slowstat.c");
    let so = dir.join("slowstat.so");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&src, SLOWSTAT).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
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
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
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
        self.run_io(cwd, args, extra, None)
    }

    fn run_io(
        &self,
        cwd: &Path,
        args: &[&str],
        extra: &[(&str, &str)],
        input: Option<&[u8]>,
    ) -> (Output, Duration) {
        let path = format!("{}:/usr/bin:/bin", self.bin.display());
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(cwd)
            .env_clear()
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if input.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        std::fs::create_dir_all(self.dir.path().join("run")).unwrap();
        // An empty value removes the variable, which is how a test clears
        // one Seat set above. `"\u{1}"` sets it to an empty string so a
        // test can leave `TMPDIR` set and empty.
        for (key, value) in extra {
            if *value == "\u{1}" {
                cmd.env(key, "");
            } else if value.is_empty() {
                cmd.env_remove(key);
            } else {
                cmd.env(key, value);
            }
        }
        let started = Instant::now();
        let mut child = cmd.spawn().unwrap();
        if let Some(bytes) = input {
            if let Some(mut pipe) = child.stdin.take() {
                use std::io::Write as _;
                let _ = pipe.write_all(bytes);
            }
        }
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

#[expect(
    clippy::disallowed_methods,
    reason = "mode 0300 cannot be restored by change_mode, which opens the directory for reading"
)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
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
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
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

#[test]
fn a_parked_working_directory_is_removed_when_the_command_ends() {
    let seat = Seat::new();
    script(
        &seat.bin,
        "vissue",
        "#!/bin/sh\nprintf '%s\\n' 'demo-abcd  TODO  [#C]  a title'\n",
    );
    let cwd = seat.dir.path().join("fast");
    let tmp = seat.dir.path().join("tmp");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    let tmp_env = tmp.to_str().unwrap();
    let (ok_out, _) = seat.run(
        &cwd,
        &["file", "-p", "demo", "--top", "a title"],
        &[("TMPDIR", tmp_env)],
    );
    assert!(ok_out.status.success(), "file: {}", text(&ok_out));
    let (err_out, _) = seat.run(&cwd, &["file", "-p", "demo"], &[("TMPDIR", tmp_env)]);
    assert!(
        !err_out.status.success(),
        "file without a title exited 0: {}",
        text(&err_out)
    );
    let left: Vec<String> = std::fs::read_dir(&tmp)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("ljos-cwd-"))
        .collect();
    assert!(
        left.is_empty(),
        "parked working directories remained: {left:?}"
    );
    let rel = cwd.join("reltmp");
    std::fs::create_dir_all(&rel).unwrap();
    let (rel_out, _) = seat.run(
        &cwd,
        &["file", "-p", "demo", "--top", "a title"],
        &[("TMPDIR", "reltmp")],
    );
    assert!(
        rel_out.status.success()
            && std::fs::read_dir(&rel)
                .unwrap()
                .flatten()
                .all(|entry| { !entry.file_name().to_string_lossy().starts_with("ljos-cwd-") }),
        "relative TMPDIR: {}",
        text(&rel_out)
    );
    let panel = seat.bin.join("panel-bin");
    script(
        &seat.bin,
        "panel-bin",
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > panel-argv\nseen=0\ni=0\nwhile [ \"$i\" -lt 150 ]; do\n  have=0\n  for d in \"$TMPDIR\"/ljos-cwd-*; do\n    if [ -d \"$d\" ]; then\n      have=1\n      seen=1\n    fi\n  done\n  if [ \"$seen\" -eq 1 ] && [ \"$have\" -eq 0 ]; then\n    break\n  fi\n  i=$((i + 1))\n  sleep 0.02\ndone\ntouch child-ran\n",
    );
    let (hook_out, _) = seat.run_io(
        &seat.slow,
        &["hook"],
        &[
            ("TMPDIR", tmp_env),
            ("LJOS_PANEL_BIN", panel.to_str().unwrap()),
            ("LD_PRELOAD", seat.so.to_str().unwrap()),
            ("LJOS_SLOW_HIT", seat.hit.to_str().unwrap()),
        ],
        Some(
            br#"{"hook_event_name":"UserPromptSubmit","prompt":"which is right","cursor_version":"1","conversation_id":"c-park"}"#,
        ),
    );
    let marker = seat.tracker.join("child-ran");
    // The panel script polls for 3s when it never sees the parked
    // directory. This ceiling is not that deadline.
    let marked = Instant::now();
    while !marker.is_file() && marked.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        marker.is_file(),
        "detached child did not run after exit: {}",
        text(&hook_out)
    );
    let argv = std::fs::read_to_string(seat.tracker.join("panel-argv")).unwrap_or_default();
    let args: Vec<&str> = argv.lines().collect();
    assert!(
        !args.iter().any(|arg| arg.contains("ljos-cwd-")),
        "parked directory in panel argv: {argv}"
    );
    let cwd_at = args
        .iter()
        .position(|arg| *arg == "--cwd")
        .expect("open-panel cwd");
    let panel_dir = args.get(cwd_at + 1).copied().unwrap_or("");
    assert!(
        Path::new(panel_dir).is_dir(),
        "panel --cwd is not a directory: {panel_dir}\n{argv}"
    );
    let swap_tmp = seat.dir.path().join("swap-tmp");
    std::fs::create_dir_all(&swap_tmp).unwrap();
    script(
        &seat.bin,
        "vissue",
        "#!/bin/sh\nsleep 0.4\nprintf '%s\\n' 'demo-abcd  TODO  [#C]  a title'\n",
    );
    let swap_watch = swap_tmp.clone();
    let watcher = std::thread::spawn(move || {
        use std::os::unix::fs::MetadataExt as _;
        let start = Instant::now();
        let parked = loop {
            if start.elapsed() > Duration::from_secs(2) {
                return None;
            }
            let found = std::fs::read_dir(&swap_watch).ok().and_then(|rd| {
                rd.flatten()
                    .find(|entry| entry.file_name().to_string_lossy().starts_with("ljos-cwd-"))
            });
            if let Some(entry) = found {
                break entry.path();
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let before = std::fs::symlink_metadata(&parked).ok()?.ino();
        std::fs::remove_dir(&parked).ok()?;
        std::fs::create_dir(&parked).ok()?;
        let after = std::fs::symlink_metadata(&parked).ok()?.ino();
        (before != after).then_some((parked, after))
    });
    let (swap_out, _) = seat.run(
        &cwd,
        &["file", "-p", "demo", "--top", "a title"],
        &[("TMPDIR", swap_tmp.to_str().unwrap())],
    );
    let swapped = watcher.join().expect("swap watcher");
    assert!(swap_out.status.success(), "swap: {}", text(&swap_out));
    let (swapped_path, swapped_ino) = swapped.expect("parked directory was not replaced");
    let swapped_meta =
        std::fs::symlink_metadata(&swapped_path).expect("replaced directory removed");
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&swapped_meta),
        swapped_ino
    );
}

#[test]
fn a_retargeted_tmpdir_symlink_leaves_the_foreign_directory() {
    let seat = Seat::new();
    script(
        &seat.bin,
        "vissue",
        "#!/bin/sh\nsleep 0.6\nprintf '%s\\n' 'demo-abcd  TODO  [#C]  a title'\n",
    );
    let cwd = seat.dir.path().join("fast");
    let dir_a = seat.dir.path().join("A");
    let dir_b = seat.dir.path().join("B");
    let link = seat.dir.path().join("tmp-link");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "links a directory that is not a git hook"
    )]
    std::os::unix::fs::symlink(&dir_a, &link).unwrap();
    let watch_a = dir_a.clone();
    let watch_b = dir_b.clone();
    let watch_link = link.clone();
    let watcher = std::thread::spawn(move || {
        use std::os::unix::fs::MetadataExt as _;
        let start = Instant::now();
        let parked = loop {
            if start.elapsed() > Duration::from_secs(2) {
                return None;
            }
            let found = std::fs::read_dir(&watch_a).ok().and_then(|rd| {
                rd.flatten()
                    .find(|entry| entry.file_name().to_string_lossy().starts_with("ljos-cwd-"))
            });
            if let Some(entry) = found {
                break entry.path();
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let name = parked.file_name()?.to_os_string();
        let original_ino = std::fs::symlink_metadata(&parked).ok()?.ino();
        let moved = watch_b.join(&name);
        #[expect(
            clippy::disallowed_methods,
            reason = "moves a directory that is not a git hook"
        )]
        std::fs::rename(&parked, &moved).ok()?;
        std::fs::create_dir(&parked).ok()?;
        let foreign_ino = std::fs::symlink_metadata(&parked).ok()?.ino();
        if foreign_ino == original_ino {
            return None;
        }
        std::fs::remove_file(&watch_link).ok()?;
        #[expect(
            clippy::disallowed_methods,
            reason = "links a directory that is not a git hook"
        )]
        std::os::unix::fs::symlink(&watch_b, &watch_link).ok()?;
        Some((name, foreign_ino, original_ino, moved))
    });
    let (out, _) = seat.run(
        &cwd,
        &["file", "-p", "demo", "--top", "a title"],
        &[("TMPDIR", link.to_str().unwrap())],
    );
    let swapped = watcher.join().expect("retarget watcher");
    assert!(out.status.success(), "retarget: {}", text(&out));
    let (name, foreign_ino, original_ino, moved) =
        swapped.expect("parked directory was not retargeted");
    let foreign = dir_a.join(&name);
    let foreign_meta = std::fs::symlink_metadata(&foreign).expect("foreign directory removed");
    assert!(foreign_meta.is_dir(), "foreign entry is not a directory");
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&foreign_meta),
        foreign_ino
    );
    let original_meta = std::fs::symlink_metadata(&moved).expect("original directory removed");
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&original_meta),
        original_ino
    );
}

#[test]
fn a_cwd_named_ljos_cwd_still_uses_a_relative_tmpdir() {
    let seat = Seat::new();
    script(
        &seat.bin,
        "vissue",
        "#!/bin/sh\nsleep 0.4\nprintf '%s\\n' 'demo-abcd  TODO  [#C]  a title'\n",
    );
    let cwd = seat.dir.path().join("ljos-cwd-work");
    let rel = cwd.join("reltmp");
    std::fs::create_dir_all(&rel).unwrap();
    let watch = rel.clone();
    let watcher = std::thread::spawn(move || {
        let start = Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(2) {
                return None;
            }
            let found = std::fs::read_dir(&watch).ok().and_then(|rd| {
                rd.flatten()
                    .find(|entry| entry.file_name().to_string_lossy().starts_with("ljos-cwd-"))
            });
            if let Some(entry) = found {
                return Some(entry.path());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let (out, _) = seat.run(
        &cwd,
        &["file", "-p", "demo", "--top", "a title"],
        &[("TMPDIR", "reltmp")],
    );
    let seen = watcher.join().expect("relative watcher");
    assert!(out.status.success(), "named cwd: {}", text(&out));
    let parked = seen.expect("relative TMPDIR was not used");
    assert!(
        !parked.exists(),
        "parked directory remained: {}",
        parked.display()
    );
}

#[test]
fn an_unreadable_tmpdir_does_not_keep_the_parked_directory() {
    let seat = Seat::new();
    script(
        &seat.bin,
        "vissue",
        "#!/bin/sh\nprintf '%s\\n' 'demo-abcd  TODO  [#C]  a title'\n",
    );
    let cwd = seat.dir.path().join("fast");
    let tmp = seat.dir.path().join("tmp-0300");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    set_mode(&tmp, 0o300);
    let (out, _) = seat.run(
        &cwd,
        &["file", "-p", "demo", "--top", "a title"],
        &[("TMPDIR", tmp.to_str().unwrap())],
    );
    set_mode(&tmp, 0o700);
    assert!(out.status.success(), "mode 0300: {}", text(&out));
    let left: Vec<String> = std::fs::read_dir(&tmp)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("ljos-cwd-"))
        .collect();
    assert!(
        left.is_empty(),
        "parked working directories remained: {left:?}"
    );
}

/// `TMPDIR` that is relative, empty, or a missing relative path, with
/// `XDG_RUNTIME_DIR` unset. The runtime directory must not be created
/// inside the parked directory, and that directory must be gone on exit.
#[test]
fn an_unresolved_tmpdir_does_not_keep_the_parked_directory() {
    let seat = Seat::new();
    script(
        &seat.bin,
        "vissue",
        r#"#!/bin/sh
printf '%s\n' "$TMPDIR" > "$VISSUE_ROOT/seen-tmpdir"
name="ljos-cwd-$PPID"
for base in "$TMPDIR" "$LJOS_PARK_WATCH" /tmp; do
  if [ -n "$base" ] && [ -d "$base/$name" ]; then
    printf '%s\n' "$base/$name" > "$VISSUE_ROOT/seen-park"
    break
  fi
done
printf '%s\n' 'demo-abcd  TODO  [#C]  a title'
"#,
    );
    let cwd = seat.dir.path().join("fast");
    std::fs::create_dir_all(&cwd).unwrap();
    let rel = cwd.join("reltmp");
    std::fs::create_dir_all(&rel).unwrap();
    check_unresolved_tmpdir(&seat, &cwd, "reltmp", &rel, Some(&rel));
    // Empty and missing relative values use the platform default, which is
    // `/tmp` on Linux and the confstr user temp on macOS.
    check_unresolved_tmpdir(&seat, &cwd, "\u{1}", Path::new("/tmp"), None);
    check_unresolved_tmpdir(&seat, &cwd, "missing-rel", Path::new("/tmp"), None);
}

fn check_unresolved_tmpdir(
    seat: &Seat,
    cwd: &Path,
    tmpdir: &str,
    watch: &Path,
    expect_exact: Option<&Path>,
) {
    let seen_tmp_path = seat.tracker.join("seen-tmpdir");
    let seen_park_path = seat.tracker.join("seen-park");
    let _ = std::fs::remove_file(&seen_tmp_path);
    let _ = std::fs::remove_file(&seen_park_path);
    let (out, _) = seat.run(
        cwd,
        &["file", "-p", "demo", "--top", "a title"],
        &[
            ("XDG_RUNTIME_DIR", ""),
            ("TMPDIR", tmpdir),
            ("LJOS_PARK_WATCH", watch.to_str().unwrap()),
        ],
    );
    assert!(out.status.success(), "tmpdir {tmpdir:?}: {}", text(&out));
    let seen_tmp = std::fs::read_to_string(&seen_tmp_path).unwrap_or_default();
    let seen_tmp = seen_tmp.trim();
    let parked = std::fs::read_to_string(&seen_park_path).unwrap_or_default();
    let parked = parked.trim();
    if let Some(expect) = expect_exact {
        assert_eq!(
            Path::new(seen_tmp),
            expect,
            "TMPDIR left as {seen_tmp:?} for {tmpdir:?}"
        );
    } else {
        let tmp = Path::new(seen_tmp);
        assert!(
            tmp.is_absolute() && tmp.is_dir(),
            "fallback TMPDIR {seen_tmp:?} for {tmpdir:?}"
        );
        assert!(
            Path::new(parked).starts_with(tmp),
            "parked {parked} is not under the platform default {seen_tmp}"
        );
    }
    assert!(
        !seen_tmp
            .split('/')
            .any(|part| part.starts_with("ljos-cwd-")),
        "runtime temp is inside the parked directory: {seen_tmp}"
    );
    assert!(
        Path::new(parked)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("ljos-cwd-")),
        "did not park with TMPDIR {tmpdir:?}: {}",
        text(&out)
    );
    assert!(
        !Path::new(parked).exists(),
        "parked directory remained: {parked}"
    );
}

/// `ljos hud` parks and then execs, which skips atexit. The parked
/// directory must already be gone, and the exec'd process must be in the
/// stable directory the other background children use.
#[test]
fn hud_exec_drops_the_parked_directory() {
    let seat = Seat::new();
    let hud = seat.bin.join("ljos-hud");
    script(
        &seat.bin,
        "ljos-hud",
        "#!/bin/sh\npwd -P > \"$VISSUE_ROOT/hud-cwd\"\nexit 0\n",
    );
    let tmp = seat.dir.path().join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let (out, _) = seat.run(
        &seat.slow,
        &["hud", "--foreground"],
        &[
            ("TMPDIR", tmp.to_str().unwrap()),
            ("LJOS_HUD_BIN", hud.to_str().unwrap()),
            ("LD_PRELOAD", seat.so.to_str().unwrap()),
            ("LJOS_SLOW_HIT", seat.hit.to_str().unwrap()),
        ],
    );
    let marked = seat.tracker.join("hud-cwd");
    let recorded = std::fs::read_to_string(&marked).unwrap_or_default();
    assert!(
        marked.is_file(),
        "hud did not exec: {}\n{recorded}",
        text(&out)
    );
    let cwd = recorded.trim();
    assert!(
        !Path::new(cwd)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("ljos-cwd-")),
        "exec kept the parked directory: {cwd}"
    );
    let got = std::fs::canonicalize(cwd).unwrap_or_else(|err| panic!("exec cwd {cwd}: {err}"));
    let expect = std::fs::canonicalize(&seat.tracker).unwrap();
    assert_eq!(got, expect, "exec cwd is not the stable directory");
    let left: Vec<String> = std::fs::read_dir(&tmp)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("ljos-cwd-"))
        .collect();
    assert!(
        left.is_empty(),
        "parked working directories remained: {left:?}\n{}",
        text(&out)
    );
}

const GUARD_SLOW: &str = r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>

static int slow(const char *p) {
    const char *want = getenv("SLOW_STAT");
    return p && want && strcmp(p, want) == 0;
}

static void pause_if(const char *p) {
    const char *hit;
    int fd;
    if (!slow(p)) return;
    hit = getenv("SLOW_HIT");
    if (hit && hit[0]) {
        fd = open(hit, O_CREAT | O_WRONLY | O_TRUNC, 0644);
        if (fd >= 0) close(fd);
    }
    usleep(900000);
}

int statx(int d, const char *restrict p, int fl, unsigned m, struct statx *restrict b) {
    static int (*real)(int, const char *restrict, int, unsigned, struct statx *restrict);
    if (!real) real = dlsym(RTLD_NEXT, "statx");
    pause_if(p);
    if (!real) return -1;
    return real(d, p, fl, m, b);
}

int stat(const char *restrict p, struct stat *restrict b) {
    static int (*real)(const char *restrict, struct stat *restrict);
    if (!real) real = dlsym(RTLD_NEXT, "stat");
    pause_if(p);
    if (!real) return -1;
    return real(p, b);
}

int lstat(const char *restrict p, struct stat *restrict b) {
    static int (*real)(const char *restrict, struct stat *restrict);
    if (!real) real = dlsym(RTLD_NEXT, "lstat");
    pause_if(p);
    if (!real) return -1;
    return real(p, b);
}

int newfstatat(int d, const char *restrict p, struct stat *restrict b, int fl) {
    static int (*real)(int, const char *restrict, struct stat *restrict, int);
    if (!real) real = dlsym(RTLD_NEXT, "newfstatat");
    pause_if(p);
    if (!real) return -1;
    return real(d, p, b, fl);
}
"#;

/// A fast `ljos hud` stays in the directory it started in. A relative
/// `TMPDIR`, `VISSUE_ROOT`, or `LJOS_HUD_BIN` still names that directory,
/// and a tracker that cannot be searched is not entered. When a slow
/// directory does park, a relative `TMPDIR` is made absolute and is not a
/// new temp root, so `remember` does not treat a home inside it as a sandbox.
#[test]
fn hud_keeps_the_start_directory_unless_it_parked() {
    let seat = Seat::new();
    let work = seat.dir.path().join("work");
    let reltmp = work.join("reltmp");
    let reltracker = work.join("reltracker");
    let mark = seat.dir.path().join("mark");
    std::fs::create_dir_all(&reltmp).unwrap();
    std::fs::create_dir_all(&reltracker).unwrap();
    std::fs::create_dir_all(&mark).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(reltracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    script(
        &work,
        "hud-rel",
        r#"#!/bin/sh
printf '%s\n' FROM_START > "$LJOS_MARK/which"
pwd -P > "$LJOS_MARK/pwd"
printf '%s\n' "$TMPDIR" > "$LJOS_MARK/tmp"
printf '%s\n' "$VISSUE_ROOT" > "$LJOS_MARK/root"
if [ -d "$VISSUE_ROOT" ]; then printf dir > "$LJOS_MARK/root-kind"
else printf missing > "$LJOS_MARK/root-kind"; fi
if [ -n "$TMPDIR" ]; then
  mkdir -p -- "$TMPDIR"
  : > "$TMPDIR/hud-marker" || true
fi
exit 0
"#,
    );
    script(
        &seat.tracker,
        "hud-rel",
        "#!/bin/sh\nprintf '%s\\n' FROM_TRACKER > \"$LJOS_MARK/which\"\nexit 0\n",
    );
    let (out, _) = seat.run(
        &work,
        &["hud", "--foreground"],
        &[
            ("XDG_RUNTIME_DIR", ""),
            ("TMPDIR", "reltmp"),
            ("VISSUE_ROOT", "reltracker"),
            ("LJOS_HUD_BIN", "./hud-rel"),
            ("LJOS_MARK", mark.to_str().unwrap()),
        ],
    );
    assert!(out.status.success(), "relative hud: {}", text(&out));
    let which = std::fs::read_to_string(mark.join("which")).unwrap_or_default();
    assert_eq!(which.trim(), "FROM_START", "ran the tracker binary");
    let pwd = std::fs::read_to_string(mark.join("pwd")).unwrap_or_default();
    let got = std::fs::canonicalize(pwd.trim()).unwrap_or_else(|err| panic!("pwd: {err}"));
    let expect = std::fs::canonicalize(&work).unwrap();
    assert_eq!(got, expect, "hud left the start directory");
    let root = std::fs::read_to_string(mark.join("root")).unwrap_or_default();
    assert_eq!(root.trim(), "reltracker");
    let kind = std::fs::read_to_string(mark.join("root-kind")).unwrap_or_default();
    assert_eq!(kind.trim(), "dir");
    let tmp = std::fs::read_to_string(mark.join("tmp")).unwrap_or_default();
    assert_eq!(tmp.trim(), "reltmp");
    assert!(
        reltmp.join("hud-marker").is_file(),
        "marker left the start dir"
    );
    assert!(
        !seat.tracker.join("reltmp").join("hud-marker").exists(),
        "marker landed in the tracker"
    );

    for name in ["which", "pwd", "tmp", "root", "root-kind"] {
        let _ = std::fs::remove_file(mark.join(name));
    }
    let _ = std::fs::remove_file(reltmp.join("hud-marker"));
    let nox = seat.dir.path().join("nox");
    std::fs::create_dir_all(&nox).unwrap();
    set_mode(&nox, 0o644);
    let hud = work.join("hud-rel");
    let (out, _) = seat.run(
        &work,
        &["hud", "--foreground"],
        &[
            ("XDG_RUNTIME_DIR", ""),
            ("TMPDIR", mark.to_str().unwrap()),
            ("VISSUE_ROOT", nox.to_str().unwrap()),
            ("LJOS_HUD_BIN", hud.to_str().unwrap()),
            ("LJOS_MARK", mark.to_str().unwrap()),
        ],
    );
    set_mode(&nox, 0o755);
    assert!(out.status.success(), "mode 644 tracker: {}", text(&out));
    let which = std::fs::read_to_string(mark.join("which")).unwrap_or_default();
    assert_eq!(which.trim(), "FROM_START");
    let pwd = std::fs::read_to_string(mark.join("pwd")).unwrap_or_default();
    let got = std::fs::canonicalize(pwd.trim()).unwrap_or_else(|err| panic!("pwd: {err}"));
    assert_eq!(got, expect, "hud entered a directory it could not search");

    remember_keeps_a_home_under_a_relative_tmpdir();
}

fn remember_keeps_a_home_under_a_relative_tmpdir() {
    let root = guard_root();
    if !outside_fixed_temp(&root) {
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    let _keep = RmDir(&root);
    let proj = root.join("proj");
    let tracker = root.join("tracker");
    let hit = root.join("slow-hit");
    std::fs::create_dir_all(proj.join("reltmp/home")).unwrap();
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    let so = guard_slow_lib(&root);
    let proj = std::fs::canonicalize(&proj).unwrap();
    let home = proj.join("reltmp/home");
    std::fs::create_dir_all(&home).unwrap();
    remember_once(&proj, &proj, ".", &tracker, &so, &hit);
    let _ = std::fs::remove_file(&hit);
    remember_once(&proj, &home, "reltmp", &tracker, &so, &hit);
}

fn remember_once(cwd: &Path, home: &Path, tmpdir: &str, tracker: &Path, so: &Path, hit: &Path) {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("remember")
        .arg("probe")
        .current_dir(cwd)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("TMPDIR", tmpdir)
        .env("VISSUE_ROOT", tracker)
        .env("LD_PRELOAD", so)
        .env("SLOW_STAT", cwd)
        .env("SLOW_HIT", hit)
        .env("LJOS_SEAT", "ada")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = cmd.spawn().unwrap();
    let status = loop {
        if started.elapsed() > Duration::from_secs(4) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(err) => panic!("{err}"),
        }
    };
    let took = started.elapsed();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stderr.take() {
        use std::io::Read as _;
        let _ = pipe.read_to_end(&mut stderr);
    }
    let err = String::from_utf8_lossy(&stderr);
    assert!(
        hit.is_file(),
        "stat of the start directory was not delayed: {err}"
    );
    assert!(
        took >= Duration::from_millis(450),
        "remember did not wait out the cwd probe ({took:?}): {err}"
    );
    assert!(
        !err.contains("memory and the command guard are off"),
        "relative TMPDIR became a sandbox ({status:?}): {err}"
    );
}

fn guard_root() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/cwdleak-guard");
    let path = base.join(std::process::id().to_string());
    std::fs::create_dir_all(&path).unwrap();
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn outside_fixed_temp(path: &Path) -> bool {
    !["/tmp", "/var/tmp", "/private/tmp", "/dev/shm"]
        .iter()
        .any(|root| path.starts_with(root))
}

fn guard_slow_lib(dir: &Path) -> PathBuf {
    let src = dir.join("guardstat.c");
    let so = dir.join("guardstat.so");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&src, GUARD_SLOW).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new("gcc")
        .args(["-shared", "-fPIC", "-o"])
        .arg(&so)
        .arg(&src)
        .output()
        .unwrap_or_else(|err| panic!("gcc: {err}"));
    assert!(
        out.status.success(),
        "gcc guardstat: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    so
}

struct RmDir<'a>(&'a Path);

impl Drop for RmDir<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}
