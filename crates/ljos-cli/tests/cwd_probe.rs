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
    const char *exact = getenv("SLOW_EXACT");
    if (exact && exact[0])
        return p && strcmp(p, exact) == 0;
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

#include <pthread.h>

struct slow_wrap {
    void *(*fn)(void *);
    void *arg;
    useconds_t delay;
};

static void *slow_start(void *raw) {
    struct slow_wrap *w = raw;
    void *(*fn)(void *) = w->fn;
    void *arg = w->arg;
    useconds_t delay = w->delay;
    free(w);
    if (delay > 0)
        usleep(delay);
    return fn(arg);
}

int pthread_create(pthread_t *thread, const pthread_attr_t *attr,
                    void *(*start)(void *), void *arg) {
    static int (*real)(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);
    const char *ms;
    struct slow_wrap *w;
    if (!real)
        real = dlsym(RTLD_NEXT, "pthread_create");
    if (!real)
        return -1;
    ms = getenv("SLOW_THREAD_MS");
    if (!ms || !ms[0] || strcmp(ms, "0") == 0)
        return real(thread, attr, start, arg);
    w = malloc(sizeof *w);
    if (!w)
        return -1;
    w->fn = start;
    w->arg = arg;
    w->delay = (useconds_t)atoi(ms) * 1000;
    return real(thread, attr, slow_start, w);
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
        self.launch(
            Path::new(env!("CARGO_BIN_EXE_ljos")),
            cwd,
            args,
            extra,
            None,
        )
    }

    /// `program` is the `ljos` this row runs. A sibling `ljos-hud` is the
    /// file beside that binary, so a row that promises the sibling copies
    /// `ljos` into the start directory first.
    fn run_at(
        &self,
        program: &Path,
        cwd: &Path,
        args: &[&str],
        extra: &[(&str, &str)],
    ) -> (Output, Duration) {
        self.launch(program, cwd, args, extra, None)
    }

    fn run_io(
        &self,
        cwd: &Path,
        args: &[&str],
        extra: &[(&str, &str)],
        input: Option<&[u8]>,
    ) -> (Output, Duration) {
        self.launch(
            Path::new(env!("CARGO_BIN_EXE_ljos")),
            cwd,
            args,
            extra,
            input,
        )
    }

    fn launch(
        &self,
        program: &Path,
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
        let mut cmd = Command::new(program);
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
    let rel = cwd.join("reltmp");
    std::fs::create_dir_all(&rel).unwrap();
    // Empty and missing relative values use the platform default, which is
    // `/tmp` on Linux and the confstr user temp on macOS.
    check_unresolved_tmpdir(&seat, &cwd, "reltmp", &rel, Some(&rel));
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
    let seen_tmp = read_trim(&seen_tmp_path);
    let parked = read_trim(&seen_park_path);
    if let Some(expect) = expect_exact {
        assert_eq!(
            Path::new(&seen_tmp),
            expect,
            "TMPDIR {seen_tmp:?} for {tmpdir:?}"
        );
    } else {
        let tmp = Path::new(&seen_tmp);
        assert!(
            tmp.is_absolute() && tmp.is_dir(),
            "fallback TMPDIR {seen_tmp:?}"
        );
        assert!(
            Path::new(&parked).starts_with(tmp),
            "parked {parked} is not under {seen_tmp}"
        );
    }
    assert!(
        !seen_tmp
            .split('/')
            .any(|part| part.starts_with("ljos-cwd-")),
        "runtime temp is inside the parked directory: {seen_tmp}"
    );
    assert!(
        Path::new(&parked)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("ljos-cwd-")),
        "did not park with TMPDIR {tmpdir:?}: {}",
        text(&out)
    );
    assert!(
        !Path::new(&parked).exists(),
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
    let recorded = read_trim(&seat.tracker.join("hud-cwd"));
    assert!(!recorded.is_empty(), "hud did not exec: {}", text(&out));
    assert!(
        !Path::new(&recorded)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("ljos-cwd-")),
        "exec kept the parked directory: {recorded}"
    );
    assert_eq!(
        canon(Path::new(&recorded)),
        canon(&seat.tracker),
        "exec cwd"
    );
    let left: Vec<_> = ljos_names(&tmp)
        .into_iter()
        .filter(|name| name.starts_with("ljos-cwd-"))
        .collect();
    assert!(
        left.is_empty(),
        "parked directories remained: {left:?}\n{}",
        text(&out)
    );
}

/// A fast `ljos hud` stays in the directory it started in. A relative
/// `TMPDIR`, `VISSUE_ROOT`, or `LJOS_HUD_BIN` still names that directory,
/// and a tracker that cannot be searched is not entered.
#[test]
fn hud_keeps_the_start_directory_unless_it_parked() {
    let seat = Seat::new();
    let work = seat.dir.path().join("work");
    let reltmp = work.join("reltmp");
    let reltracker = work.join("reltracker");
    let mark = seat.dir.path().join("mark");
    for path in [&reltmp, &reltracker, &mark] {
        std::fs::create_dir_all(path).unwrap();
    }
    write_file(&reltracker.join("vissue.toml"), "prefix = \"Software\"\n");
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
    let mark_s = mark.to_str().unwrap();
    let (out, _) = seat.run(
        &work,
        &["hud", "--foreground"],
        &[
            ("XDG_RUNTIME_DIR", ""),
            ("TMPDIR", "reltmp"),
            ("VISSUE_ROOT", "reltracker"),
            ("LJOS_HUD_BIN", "./hud-rel"),
            ("LJOS_MARK", mark_s),
        ],
    );
    assert!(out.status.success(), "relative hud: {}", text(&out));
    let expect = canon(&work);
    assert_eq!(
        read_trim(&mark.join("which")),
        "FROM_START",
        "ran the tracker binary"
    );
    assert_eq!(
        canon(Path::new(&read_trim(&mark.join("pwd")))),
        expect,
        "hud left the start directory"
    );
    assert_eq!(read_trim(&mark.join("root")), "reltracker");
    assert_eq!(read_trim(&mark.join("root-kind")), "dir");
    assert_eq!(read_trim(&mark.join("tmp")), "reltmp");
    assert!(
        reltmp.join("hud-marker").is_file(),
        "marker left the start dir"
    );
    assert!(
        !seat.tracker.join("reltmp/hud-marker").exists(),
        "marker landed in the tracker"
    );

    for name in ["which", "pwd", "tmp", "root", "root-kind"] {
        let _ = std::fs::remove_file(mark.join(name));
    }
    let _ = std::fs::remove_file(reltmp.join("hud-marker"));
    let nox = seat.dir.path().join("nox");
    std::fs::create_dir_all(&nox).unwrap();
    set_mode(&nox, 0o644);
    let (out, _) = seat.run(
        &work,
        &["hud", "--foreground"],
        &[
            ("XDG_RUNTIME_DIR", ""),
            ("TMPDIR", mark_s),
            ("VISSUE_ROOT", nox.to_str().unwrap()),
            ("LJOS_HUD_BIN", work.join("hud-rel").to_str().unwrap()),
            ("LJOS_MARK", mark_s),
        ],
    );
    set_mode(&nox, 0o755);
    assert!(out.status.success(), "mode 644 tracker: {}", text(&out));
    assert_eq!(read_trim(&mark.join("which")), "FROM_START");
    assert_eq!(
        canon(Path::new(&read_trim(&mark.join("pwd")))),
        expect,
        "hud entered an unsearchable directory"
    );
}

/// A home inside a relative `TMPDIR` is not a sandbox after a slow start
/// makes that value absolute. The directory must sit outside the fixed
/// temp roots, or this promise is not what the run measured.
#[test]
fn remember_keeps_a_home_under_a_relative_tmpdir() {
    let root = guard_root();
    let _keep = RmDir(&root);
    let proj = root.join("proj");
    let tracker = root.join("tracker");
    std::fs::create_dir_all(proj.join("reltmp/home")).unwrap();
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    write_file(&tracker.join("vissue.toml"), "prefix = \"Software\"\n");
    let so = slow_lib(&root);
    let proj = canon(&proj);
    let home = proj.join("reltmp/home");
    std::fs::create_dir_all(&home).unwrap();
    let tracker = canon(&tracker);
    for (home, tmpdir) in [(&proj, "."), (&home, "reltmp")] {
        remember_once(&proj, home, tmpdir, &tracker, &so);
    }
}

fn remember_once(cwd: &Path, home: &Path, tmpdir: &str, tracker: &Path, so: &Path) {
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
        .env("SLOW_EXACT", cwd)
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
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stderr.take() {
        use std::io::Read as _;
        let _ = pipe.read_to_end(&mut stderr);
    }
    let err = String::from_utf8_lossy(&stderr);
    assert!(status.is_some(), "remember did not finish: {err}");
    assert!(
        !err.contains("memory and the command guard are off"),
        "relative TMPDIR became a sandbox ({status:?}): {err}"
    );
}

fn guard_root() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/cwdleak-guard");
    std::fs::create_dir_all(&base).unwrap();
    let base = canon(&base);
    for root in ["/tmp", "/var/tmp", "/private/tmp", "/dev/shm"] {
        assert!(
            !base.starts_with(root),
            "sandbox {} is under {root}",
            base.display()
        );
    }
    let path = base.join(std::process::id().to_string());
    std::fs::create_dir_all(&path).unwrap();
    canon(&path)
}

struct RmDir<'a>(&'a Path);

impl Drop for RmDir<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

fn ljos_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("ljos-"))
        .collect()
}

fn refused(out: &Output, key: &str) -> bool {
    !out.status.success() && text(out).contains("did not resolve") && text(out).contains(key)
}

fn canon(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn read_trim(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn write_file(path: &Path, body: &str) {
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(path, body).unwrap();
}

fn link_dir(target: &Path, path: &Path) {
    #[expect(
        clippy::disallowed_methods,
        reason = "links a directory that is not a git hook"
    )]
    std::os::unix::fs::symlink(target, path).unwrap();
}

const TRACKER_STUB: &str = r#"#!/bin/sh
root="$VISSUE_ROOT"
if [ -z "$root" ]; then root="$ISSUE_ROOT"; fi
printf '%s\n' "$root" > "$LJOS_MARK/root"
mkdir -p -- "$root/Software"
printf x > "$root/Software/landed"
printf '%s\n' 'demo-abcd  TODO  [#C]  a title'
"#;

/// A relative or `.` `TMPDIR` on a slow start is not stated without a
/// deadline, and it is not replaced with another directory.
#[test]
fn a_slow_relative_tmpdir_is_refused() {
    let seat = Seat::new();
    let (mark, _tmp) = scratch(&seat);
    std::fs::create_dir_all(seat.slow.join("reltmp")).unwrap();
    let hud = seat.bin.join("ljos-hud");
    script(
        &seat.bin,
        "ljos-hud",
        "#!/bin/sh\nprintf FROM_START > \"$LJOS_MARK/which\"\nexit 0\n",
    );
    for value in [".", "reltmp"] {
        let _ = std::fs::remove_file(mark.join("which"));
        let (out, took) = seat.run(
            &seat.slow,
            &["hud", "--foreground"],
            &[
                ("XDG_RUNTIME_DIR", ""),
                ("TMPDIR", value),
                ("LJOS_HUD_BIN", hud.to_str().unwrap()),
                ("LJOS_MARK", mark.to_str().unwrap()),
                ("LD_PRELOAD", seat.so.to_str().unwrap()),
                ("LJOS_SLOW_HIT", seat.hit.to_str().unwrap()),
            ],
        );
        assert!(
            took < Duration::from_secs(4),
            "{value} took {took:?}: {}",
            text(&out)
        );
        assert!(refused(&out, "TMPDIR"), "{value}: {}", text(&out));
        assert!(!mark.join("which").is_file(), "{value} ran the hud");
        assert!(
            ljos_names(&seat.slow)
                .iter()
                .all(|name| !name.starts_with("ljos-cwd-")),
            "{value} left {:?}",
            ljos_names(&seat.slow)
        );
    }
}

/// A relative `LJOS_HUD_BIN` in a parked start directory is refused for
/// `hud --foreground` and `hud --hide`. `--hide` does not report success
/// without running it, and the parked directory is not left behind.
#[test]
fn a_slow_relative_hud_bin_is_refused() {
    let seat = Seat::new();
    let (mark, tmp) = scratch(&seat);
    script(
        &seat.slow,
        "hud-rel",
        "#!/bin/sh\nprintf FROM_START > \"$LJOS_MARK/which\"\nexit 0\n",
    );
    for args in [["hud", "--foreground"], ["hud", "--hide"]] {
        let _ = std::fs::remove_file(mark.join("which"));
        let (out, took) = seat.run(
            &seat.slow,
            &args,
            &[
                ("TMPDIR", tmp.to_str().unwrap()),
                ("LJOS_HUD_BIN", "./hud-rel"),
                ("LJOS_MARK", mark.to_str().unwrap()),
                ("LD_PRELOAD", seat.so.to_str().unwrap()),
                ("LJOS_SLOW_HIT", seat.hit.to_str().unwrap()),
            ],
        );
        assert!(
            took < Duration::from_secs(4),
            "{args:?} took {took:?}: {}",
            text(&out)
        );
        assert!(refused(&out, "LJOS_HUD_BIN"), "{args:?}: {}", text(&out));
        assert!(out.stdout.is_empty(), "{args:?} stdout: {}", text(&out));
        assert!(!mark.join("which").is_file(), "{args:?} ran the hud");
        assert!(
            ljos_names(&tmp).is_empty(),
            "{args:?} left {:?}",
            ljos_names(&tmp)
        );
    }
}

/// `..` after a symlink is that symlink's parent. `ISSUE_ROOT` uses the
/// same walk as `VISSUE_ROOT`. The lexical path is a different directory.
#[test]
fn a_dotdot_tracker_follows_the_kernel() {
    let seat = Seat::new();
    let work = seat.dir.path().join("work");
    let elsewhere = seat.dir.path().join("elsewhere");
    let real = seat.dir.path().join("tracker");
    let decoy = work.join("tracker");
    let (mark, tmp) = scratch(&seat);
    for path in [
        &elsewhere,
        &real.join("Software"),
        &decoy.join("Software"),
        &work,
    ] {
        std::fs::create_dir_all(path).unwrap();
    }
    write_file(&real.join("vissue.toml"), "prefix = \"Software\"\n");
    write_file(&decoy.join("vissue.toml"), "prefix = \"Software\"\n");
    link_dir(&elsewhere, &work.join("via"));
    script(&seat.bin, "vissue", TRACKER_STUB);
    let (out, _) = seat.run(
        &work,
        &["file", "-p", "demo", "--top", "a title"],
        &[
            ("TMPDIR", tmp.to_str().unwrap()),
            ("LJOS_MARK", mark.to_str().unwrap()),
            ("VISSUE_ROOT", "via/../tracker"),
        ],
    );
    assert!(out.status.success(), "vissue root: {}", text(&out));
    assert_eq!(
        canon(Path::new(&read_trim(&mark.join("root")))),
        canon(&real),
        "lexical path"
    );
    assert!(
        real.join("Software/landed").is_file(),
        "missed the kernel tracker"
    );
    assert!(
        !decoy.join("Software/landed").exists(),
        "wrote the lexical tracker"
    );
}

/// `missing/..` is not the start directory. `vartmp/..` through a symlink
/// is that symlink's parent. A missing relative path with no `..` is the
/// platform default, which the unresolved-tmpdir test already locks.
#[test]
fn an_unresolvable_tmpdir_is_not_the_start_directory() {
    let seat = Seat::new();
    let work = seat.dir.path().join("work");
    let elsewhere = seat.dir.path().join("elsewhere");
    let (mark, _tmp) = scratch(&seat);
    std::fs::create_dir_all(elsewhere.join("child")).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    link_dir(&elsewhere.join("child"), &work.join("vartmp"));
    script(
        &seat.bin,
        "vissue",
        r#"#!/bin/sh
printf '%s\n' "$TMPDIR" > "$LJOS_MARK/tmp"
name="ljos-repro-marker-$$"
printf '%s\n' "$name" > "$LJOS_MARK/marker-name"
if [ -n "$TMPDIR" ]; then
  : > "$TMPDIR/$name"
fi
printf '%s\n' 'demo-abcd  TODO  [#C]  a title'
"#,
    );
    let args = ["file", "-p", "demo", "--top", "a title"];
    let base = [
        ("XDG_RUNTIME_DIR", ""),
        ("LJOS_MARK", mark.to_str().unwrap()),
    ];
    for (value, ok) in [("missing/..", false), ("vartmp/..", true)] {
        let (out, _) = seat.run(&work, &args, &[base[0], base[1], ("TMPDIR", value)]);
        assert!(
            ljos_names(&work).is_empty(),
            "{value} left {:?}",
            ljos_names(&work)
        );
        if ok {
            assert!(out.status.success(), "{value}: {}", text(&out));
            assert_eq!(
                canon(Path::new(&read_trim(&mark.join("tmp")))),
                canon(&elsewhere),
                "{value}"
            );
            let marker = read_trim(&mark.join("marker-name"));
            assert!(
                elsewhere.join(marker).is_file(),
                "marker missed the symlink parent"
            );
        } else {
            assert!(refused(&out, "TMPDIR"), "{value}: {}", text(&out));
        }
    }
}

/// `VISSUE_ROOT=missing/..` and `ISSUE_ROOT=missing/..` are not the start
/// directory, so `file` does not write a tracker there.
#[test]
fn an_unresolvable_root_is_not_the_start_directory() {
    let seat = Seat::new();
    let work = seat.dir.path().join("work");
    let (mark, tmp) = scratch(&seat);
    std::fs::create_dir_all(&work).unwrap();
    script(&seat.bin, "vissue", TRACKER_STUB);
    let leaked = work.join("Software/leaked.org");
    let args = ["file", "-p", "demo", "--top", "a title"];
    let tmp_s = tmp.to_str().unwrap();
    let mark_s = mark.to_str().unwrap();
    for (key, extra) in [
        (
            "VISSUE_ROOT",
            vec![
                ("TMPDIR", tmp_s),
                ("VISSUE_ROOT", "missing/.."),
                ("LJOS_MARK", mark_s),
            ],
        ),
        (
            "ISSUE_ROOT",
            vec![
                ("TMPDIR", tmp_s),
                ("VISSUE_ROOT", ""),
                ("ISSUE_ROOT", "missing/.."),
                ("LJOS_MARK", mark_s),
            ],
        ),
    ] {
        let (out, _) = seat.run(&work, &args, &extra);
        assert!(refused(&out, key), "{key}: {}", text(&out));
        assert!(
            !leaked.exists() && !work.join("Software/landed").exists(),
            "{key} wrote the start directory"
        );
    }
}

/// An absolute program inside a slow start directory is stated before the
/// park, so `hud --hide` and `hud --foreground` run it. A tracker root
/// whose last component is missing is that parent plus the new name.
/// `file -p` and `note` both use that walk. A parent that is not there is
/// still refused. An absolute missing root is left for vissue.
#[test]
fn an_absolute_program_and_a_missing_root_resolve_before_the_park() {
    let seat = Seat::new();
    let (mark, tmp) = scratch(&seat);
    let slow = canon(&seat.slow);
    script(
        &slow,
        "hud-abs",
        "#!/bin/sh\nprintf '%s\\n' \"$1\" > \"$LJOS_MARK/which\"\nexit 0\n",
    );
    let mark_s = mark.to_str().unwrap();
    let tmp_s = tmp.to_str().unwrap();
    let bin = slow.join("hud-abs");
    for flag in ["--hide", "--foreground"] {
        let _ = std::fs::remove_file(mark.join("which"));
        let (out, took) = seat.run(
            &slow,
            &["hud", flag],
            &[
                ("TMPDIR", tmp_s),
                ("LJOS_HUD_BIN", bin.to_str().unwrap()),
                ("LJOS_MARK", mark_s),
                ("LD_PRELOAD", seat.so.to_str().unwrap()),
                ("SLOW_EXACT", slow.to_str().unwrap()),
            ],
        );
        assert!(
            took < Duration::from_secs(4) && out.status.success(),
            "{flag} {took:?}: {}",
            text(&out)
        );
        assert_eq!(
            read_trim(&mark.join("which")),
            flag,
            "{flag} did not run: {}",
            text(&out)
        );
    }

    let work = seat.dir.path().join("work");
    std::fs::create_dir_all(work.join("parent")).unwrap();
    script(&seat.bin, "vissue", TRACKER_STUB);
    let abs = work.join("abs-missing");
    let file_args = ["file", "-p", "demo", "--top", "a title"];
    let check = |key: &str, value: &str, ok: bool, args: &[&str]| {
        let expect = if Path::new(value).is_absolute() {
            PathBuf::from(value)
        } else {
            work.join(value)
        };
        let _ = std::fs::remove_dir_all(expect.join("Software"));
        let mut extra = vec![("TMPDIR", tmp_s), ("LJOS_MARK", mark_s), (key, value)];
        if key == "ISSUE_ROOT" {
            extra.insert(0, ("VISSUE_ROOT", ""));
        }
        let (out, _) = seat.run(&work, args, &extra);
        if ok {
            assert!(out.status.success(), "{key}={value}: {}", text(&out));
            assert!(
                expect.join("Software/landed").is_file(),
                "{key}={value} did not land"
            );
            assert!(
                !work.join("Software/landed").exists(),
                "{key}={value} wrote the start directory"
            );
        } else {
            assert!(refused(&out, key), "{key}={value}: {}", text(&out));
            assert!(!expect.exists(), "{key}={value} was created");
        }
    };
    for (key, value, ok) in [
        ("VISSUE_ROOT", "brand-new", true),
        ("VISSUE_ROOT", "parent/newchild", true),
        ("ISSUE_ROOT", "issue-new", true),
        ("VISSUE_ROOT", "have/not-yet", false),
    ] {
        check(key, value, ok, &file_args);
    }
    check("VISSUE_ROOT", abs.to_str().unwrap(), true, &file_args);
    let (note, _) = seat.run(
        &work,
        &["note", "demo-1", "a note"],
        &[
            ("TMPDIR", tmp_s),
            ("VISSUE_ROOT", "note-new"),
            ("LJOS_MARK", mark_s),
        ],
    );
    assert!(
        !text(&note).contains("did not resolve"),
        "note: {}",
        text(&note)
    );
    assert!(
        work.join("note-new/Software/landed").is_file(),
        "note did not reach vissue: {}",
        text(&note)
    );

    // One row per parked-start miss: a missing tracker leaf, a HUD on
    // PATH, the sibling HUD, a HUD path with a trailing space, and a PATH
    // stat that never answers.
    let neutral = seat.dir.path().join("exe");
    std::fs::create_dir_all(&neutral).unwrap();
    let copied = neutral.join("ljos");
    ljos_cli::copy_executable(Path::new(env!("CARGO_BIN_EXE_ljos")), &copied).unwrap();
    let beside = slow.join("ljos");
    ljos_cli::copy_executable(Path::new(env!("CARGO_BIN_EXE_ljos")), &beside).unwrap();
    script(
        &slow,
        "ljos-hud",
        "#!/bin/sh\nprintf '%s\\n' \"$1\" > \"$LJOS_MARK/which\"\nexit 0\n",
    );
    let slow_s = slow.to_str().unwrap();
    let so_s = seat.so.to_str().unwrap();
    let path_in = format!("{slow_s}:/usr/bin:/bin");
    let spaced = format!("{slow_s}/hud-abs ");
    let mut missed = Vec::new();
    for row in [
        "missing-root",
        "path-hud",
        "sibling-hud",
        "hud-space",
        "path-hang",
        "link-root",
        "path-out",
        "hud-hang",
        "hud-late",
        "path-rel",
        "sib-fifo",
        "hud-bytes",
        "issue-wins",
    ] {
        let _ = std::fs::remove_file(mark.join("which"));
        let err = match row {
            "missing-root" => {
                let root = slow.join("brand-new");
                let _ = std::fs::remove_dir_all(&root);
                let (out, took) = seat.run(
                    &slow,
                    &["file", "-p", "demo", "--top", "a title"],
                    &[
                        ("TMPDIR", tmp_s),
                        ("VISSUE_ROOT", "brand-new"),
                        ("LJOS_MARK", mark_s),
                        ("LD_PRELOAD", so_s),
                        ("SLOW_EXACT", slow_s),
                    ],
                );
                parked_ok(
                    took < Duration::from_secs(4) && out.status.success(),
                    root.join("Software/landed").is_file(),
                    &format!("{row} {took:?}: {}", text(&out)),
                )
            }
            "path-hud" | "sibling-hud" | "hud-space" => {
                let program = if row == "sibling-hud" {
                    &beside
                } else {
                    &copied
                };
                let (out, took) = seat.run_at(
                    program,
                    &slow,
                    &["hud", "--foreground"],
                    &[
                        ("TMPDIR", tmp_s),
                        ("LJOS_MARK", mark_s),
                        ("LD_PRELOAD", so_s),
                        ("SLOW_EXACT", slow_s),
                        (
                            "PATH",
                            if row == "path-hud" {
                                path_in.as_str()
                            } else {
                                "/usr/bin:/bin"
                            },
                        ),
                        (
                            "LJOS_HUD_BIN",
                            if row == "hud-space" {
                                spaced.as_str()
                            } else {
                                ""
                            },
                        ),
                    ],
                );
                parked_ok(
                    took < Duration::from_secs(4) && out.status.success(),
                    read_trim(&mark.join("which")) == "--foreground",
                    &format!("{row} {took:?}: {}", text(&out)),
                )
            }
            "path-hang" => {
                // The unrepaired lookup states this path on the main thread.
                // The harness kills that at 6s. Catch it so the other rows
                // still report.
                let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    seat.run_at(
                        &copied,
                        &slow,
                        &["hud", "--foreground"],
                        &[
                            ("TMPDIR", tmp_s),
                            ("LJOS_MARK", mark_s),
                            ("LD_PRELOAD", so_s),
                            ("PATH", path_in.as_str()),
                            ("LJOS_HUD_BIN", ""),
                        ],
                    )
                }));
                let Ok((out, took)) = caught else {
                    missed.push(format!("{row}: still running"));
                    continue;
                };
                let gone = ljos_names(&tmp)
                    .iter()
                    .all(|name| !name.starts_with("ljos-cwd-"));
                parked_ok(
                    took < Duration::from_secs(4)
                        && !out.status.success()
                        && text(&out).contains("did not resolve")
                        && out.stdout.is_empty()
                        && gone
                        && !mark.join("which").is_file(),
                    true,
                    &format!("{row} {took:?}: {}", text(&out)),
                )
            }
            "link-root" => {
                let real = seat.dir.path().join("real");
                let link = seat.dir.path().join("via-link");
                std::fs::create_dir_all(&real).unwrap();
                if !link.exists() {
                    link_dir(&real, &link);
                }
                let spelling = link.join("leaf-abs");
                let landed = real.join("leaf-abs/Software/landed");
                let _ = std::fs::remove_dir_all(real.join("leaf-abs"));
                let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    seat.run(
                        &real,
                        &["file", "-p", "demo", "--top", "a title"],
                        &[
                            ("TMPDIR", tmp_s),
                            ("VISSUE_ROOT", spelling.to_str().unwrap()),
                            ("LJOS_MARK", mark_s),
                            ("LD_PRELOAD", so_s),
                            ("SLOW_EXACT", spelling.to_str().unwrap()),
                        ],
                    )
                }));
                let Ok((out, took)) = caught else {
                    missed.push(format!("{row}: still running"));
                    continue;
                };
                parked_ok(
                    took < Duration::from_secs(4) && out.status.success() && landed.is_file(),
                    true,
                    &format!("{row} {took:?}: {}", text(&out)),
                )
            }
            "path-out" => {
                let hang = seat.dir.path().join("hangout");
                std::fs::create_dir_all(&hang).unwrap();
                script(&hang, "ljos-hud", "#!/bin/sh\nexit 0\n");
                let hang_hud = hang.join("ljos-hud");
                let path = format!("{}:{}:/usr/bin:/bin", hang.display(), seat.bin.display());
                let landed = seat.tracker.join("Software/landed");
                let _ = std::fs::remove_file(&landed);
                let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    seat.run_at(
                        &copied,
                        &work,
                        &["file", "-p", "demo", "--top", "a title"],
                        &[
                            ("TMPDIR", tmp_s),
                            ("VISSUE_ROOT", seat.tracker.to_str().unwrap()),
                            ("LJOS_MARK", mark_s),
                            ("LD_PRELOAD", so_s),
                            ("SLOW_EXACT", hang_hud.to_str().unwrap()),
                            ("PATH", path.as_str()),
                            ("LJOS_HUD_BIN", ""),
                        ],
                    )
                }));
                let Ok((out, took)) = caught else {
                    missed.push(format!("{row}: still running"));
                    continue;
                };
                parked_ok(
                    took < Duration::from_secs(4) && out.status.success() && landed.is_file(),
                    true,
                    &format!("{row} {took:?}: {}", text(&out)),
                )
            }
            "hud-hang" => {
                let hang = seat.dir.path().join("hudhang");
                let good = seat.dir.path().join("hudgood");
                let fast = seat.dir.path().join("fasthud");
                std::fs::create_dir_all(&hang).unwrap();
                std::fs::create_dir_all(&good).unwrap();
                std::fs::create_dir_all(&fast).unwrap();
                script(
                    &hang,
                    "ljos-hud",
                    "#!/bin/sh\nprintf hung > \"$LJOS_MARK/which\"\nexit 0\n",
                );
                script(
                    &good,
                    "ljos-hud",
                    "#!/bin/sh\nprintf ran > \"$LJOS_MARK/which\"\nexit 0\n",
                );
                let path = format!("{}:{}:/usr/bin:/bin", hang.display(), good.display());
                let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    seat.run_at(
                        &copied,
                        &fast,
                        &["hud", "--foreground"],
                        &[
                            ("TMPDIR", tmp_s),
                            ("LJOS_MARK", mark_s),
                            ("LD_PRELOAD", so_s),
                            ("SLOW_EXACT", hang.join("ljos-hud").to_str().unwrap()),
                            ("PATH", path.as_str()),
                            ("LJOS_HUD_BIN", ""),
                            ("VISSUE_ROOT", ""),
                        ],
                    )
                }));
                let Ok((out, took)) = caught else {
                    missed.push(format!("{row}: still running"));
                    continue;
                };
                parked_ok(
                    took < Duration::from_secs(4)
                        && out.status.success()
                        && read_trim(&mark.join("which")) == "ran",
                    true,
                    &format!("{row} {took:?}: {}", text(&out)),
                )
            }
            "hud-late" => {
                let late = work.join("fast-hud");
                script(
                    &work,
                    "fast-hud",
                    "#!/bin/sh\nprintf late > \"$LJOS_MARK/which\"\nexit 0\n",
                );
                let landed = seat.tracker.join("Software/landed");
                let _ = std::fs::remove_file(&landed);
                let (out, took) = seat.run(
                    &work,
                    &["file", "-p", "demo", "--top", "a title"],
                    &[
                        ("TMPDIR", tmp_s),
                        ("VISSUE_ROOT", seat.tracker.to_str().unwrap()),
                        ("LJOS_HUD_BIN", late.to_str().unwrap()),
                        ("LJOS_MARK", mark_s),
                        ("LD_PRELOAD", so_s),
                        ("SLOW_THREAD_MS", "800"),
                    ],
                );
                parked_ok(
                    took < Duration::from_secs(4)
                        && out.status.success()
                        && landed.is_file()
                        && !text(&out).contains("did not resolve"),
                    true,
                    &format!("{row} {took:?}: {}", text(&out)),
                )
            }
            "path-rel" => {
                let outside = seat.dir.path().join("outside");
                std::fs::create_dir_all(&outside).unwrap();
                std::fs::create_dir_all(slow.join("bin")).unwrap();
                script(
                    &slow,
                    "ljos-hud",
                    "#!/bin/sh\nprintf start > \"$LJOS_MARK/which\"\nexit 0\n",
                );
                script(
                    &slow.join("bin"),
                    "ljos-hud",
                    "#!/bin/sh\nprintf relbin > \"$LJOS_MARK/which\"\nexit 0\n",
                );
                script(
                    &outside,
                    "ljos-hud",
                    "#!/bin/sh\nprintf outside > \"$LJOS_MARK/which\"\nexit 0\n",
                );
                let outside_path = format!("{}:/usr/bin:/bin", outside.display());
                let mut bad = None;
                for (label, path) in [
                    ("dot", format!(".:{outside_path}")),
                    ("empty", format!(":{outside_path}")),
                    ("bin", format!("bin:{outside_path}")),
                ] {
                    let _ = std::fs::remove_file(mark.join("which"));
                    let (out, took) = seat.run_at(
                        &copied,
                        &slow,
                        &["hud", "--foreground"],
                        &[
                            ("TMPDIR", tmp_s),
                            ("LJOS_MARK", mark_s),
                            ("LD_PRELOAD", so_s),
                            ("SLOW_EXACT", slow_s),
                            ("PATH", path.as_str()),
                            ("LJOS_HUD_BIN", ""),
                            ("VISSUE_ROOT", ""),
                        ],
                    );
                    if !(took < Duration::from_secs(4)
                        && out.status.success()
                        && read_trim(&mark.join("which")) == "outside")
                    {
                        bad = Some(format!(
                            "{label} {took:?} marker {:?}: {}",
                            read_trim(&mark.join("which")),
                            text(&out)
                        ));
                        break;
                    }
                }
                match bad {
                    Some(why) => Err(format!("{row} {why}")),
                    None => Ok(()),
                }
            }
            "sib-fifo" => {
                let outside = seat.dir.path().join("outside");
                std::fs::create_dir_all(&outside).unwrap();
                script(
                    &outside,
                    "ljos-hud",
                    "#!/bin/sh\nprintf outside > \"$LJOS_MARK/which\"\nexit 0\n",
                );
                let fifo = slow.join("ljos-hud");
                let _ = std::fs::remove_file(&fifo);
                #[expect(
                    clippy::disallowed_methods,
                    reason = "starts a program outside git hook code"
                )]
                let made = Command::new("mkfifo").arg(&fifo).output().unwrap();
                assert!(made.status.success(), "mkfifo");
                let path = format!("{}:/usr/bin:/bin", outside.display());
                let (out, took) = seat.run_at(
                    &beside,
                    &slow,
                    &["hud", "--foreground"],
                    &[
                        ("TMPDIR", tmp_s),
                        ("LJOS_MARK", mark_s),
                        ("LD_PRELOAD", so_s),
                        ("SLOW_EXACT", slow_s),
                        ("PATH", path.as_str()),
                        ("LJOS_HUD_BIN", ""),
                        ("VISSUE_ROOT", ""),
                    ],
                );
                parked_ok(
                    took < Duration::from_secs(4)
                        && out.status.success()
                        && read_trim(&mark.join("which")) == "outside",
                    true,
                    &format!("{row} {took:?}: {}", text(&out)),
                )
            }
            "hud-bytes" => {
                let spaced = slow.join("plain ");
                script(
                    &slow,
                    "plain",
                    "#!/bin/sh\nprintf 'PLAIN\\n%s' \"$LJOS_HUD_BIN\" > \"$LJOS_MARK/which\"\nexit 0\n",
                );
                script(
                    &slow,
                    "plain ",
                    "#!/bin/sh\nprintf SPACED > \"$LJOS_MARK/which\"\nexit 0\n",
                );
                let _ = std::fs::remove_file(mark.join("which"));
                let (out, took) = seat.run_at(
                    &copied,
                    &slow,
                    &["hud", "--foreground"],
                    &[
                        ("TMPDIR", tmp_s),
                        ("LJOS_MARK", mark_s),
                        ("LD_PRELOAD", so_s),
                        ("SLOW_EXACT", slow_s),
                        ("PATH", "/usr/bin:/bin"),
                        ("LJOS_HUD_BIN", spaced.to_str().unwrap()),
                        ("VISSUE_ROOT", ""),
                    ],
                );
                let body = std::fs::read_to_string(mark.join("which")).unwrap_or_default();
                let want = format!("PLAIN\n{}", spaced.display());
                let hud_ok = took < Duration::from_secs(4) && out.status.success() && body == want;
                let spaced_root = slow.join("rname ");
                let plain_root = slow.join("rname");
                std::fs::create_dir_all(&spaced_root).unwrap();
                std::fs::create_dir_all(&plain_root).unwrap();
                let _ = std::fs::remove_dir_all(spaced_root.join("Software"));
                let _ = std::fs::remove_dir_all(plain_root.join("Software"));
                let (root_out, root_took) = seat.run(
                    &slow,
                    &["file", "-p", "demo", "--top", "a title"],
                    &[
                        ("TMPDIR", tmp_s),
                        ("VISSUE_ROOT", "rname "),
                        ("LJOS_MARK", mark_s),
                        ("LD_PRELOAD", so_s),
                        ("SLOW_EXACT", slow_s),
                    ],
                );
                let root_ok = root_took < Duration::from_secs(4)
                    && root_out.status.success()
                    && spaced_root.join("Software/landed").is_file()
                    && !plain_root.join("Software/landed").exists();
                parked_ok(
                    hud_ok && root_ok,
                    true,
                    &format!(
                        "{row} hud {took:?} body {body:?}: {} root {root_took:?}: {}",
                        text(&out),
                        text(&root_out)
                    ),
                )
            }
            "issue-wins" => {
                script(
                    &seat.bin,
                    "vissue",
                    r#"#!/bin/sh
root="$ISSUE_ROOT"
if [ -z "$root" ]; then root="$VISSUE_ROOT"; fi
printf '%s\n' "$root" > "$LJOS_MARK/root"
mkdir -p -- "$root/Software"
printf x > "$root/Software/landed"
printf '%s\n' 'demo-abcd  TODO  [#C]  a title'
"#,
                );
                let issue = seat.dir.path().join("issue-tracker");
                std::fs::create_dir_all(issue.join("Software")).unwrap();
                write_file(&issue.join("vissue.toml"), "prefix = \"Software\"\n");
                let landed = issue.join("Software/landed");
                let _ = std::fs::remove_file(&landed);
                let (out, took) = seat.run(
                    &work,
                    &["file", "-p", "demo", "--top", "a title"],
                    &[
                        ("TMPDIR", tmp_s),
                        ("VISSUE_ROOT", "missing/not-yet"),
                        ("ISSUE_ROOT", issue.to_str().unwrap()),
                        ("LJOS_MARK", mark_s),
                    ],
                );
                parked_ok(
                    took < Duration::from_secs(4)
                        && out.status.success()
                        && landed.is_file()
                        && !work.join("missing").exists(),
                    true,
                    &format!("{row} {took:?}: {}", text(&out)),
                )
            }
            _ => unreachable!(),
        };
        if let Err(why) = err {
            missed.push(why);
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

fn parked_ok(passed: bool, marker: bool, why: &str) -> Result<(), String> {
    if passed && marker {
        Ok(())
    } else {
        Err(why.to_string())
    }
}

fn scratch(seat: &Seat) -> (PathBuf, PathBuf) {
    let mark = seat.dir.path().join("mark");
    let tmp = seat.dir.path().join("tmp");
    std::fs::create_dir_all(&mark).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    (mark, tmp)
}
