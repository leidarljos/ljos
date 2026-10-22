//! A runner that detaches is still a runner.
//!
//! `setsid -f`, a double fork, `script` and `tmux new-session -d` drop the
//! runner from the process tree and can clear its variables. The cgroup
//! stays. Keeping a preference and stamping a person basis from any of
//! those must fail the same way a live `claude` parent fails.

#[path = "common/terminal.rs"]
mod terminal;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

struct Daemon {
    child: std::process::Child,
    url: String,
    _home: tempfile::TempDir,
}

impl Daemon {
    fn start() -> Self {
        let home = tempfile::tempdir().unwrap();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let dir = home.path().to_str().unwrap().to_string();
        let mut child = Command::new("packsetd")
            .args(["--port", &port.to_string(), "--home", &dir])
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_HOME", &dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("packsetd is not on PATH; CI builds it from packset develop");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                let mut err = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut err);
                }
                panic!("packsetd exited {status}: {err}");
            }
            if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                let _ = stream.write_all(b"GET /health HTTP/1.1\r\nConnection: close\r\n\r\n");
                let mut said = String::new();
                let _ = stream.read_to_string(&mut said);
                if said.starts_with("HTTP/1.1 200") {
                    break;
                }
            }
            assert!(Instant::now() < deadline, "packsetd did not answer");
            std::thread::sleep(Duration::from_millis(50));
        }
        Self {
            child,
            url: format!("http://127.0.0.1:{port}"),
            _home: home,
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Seat {
    home: tempfile::TempDir,
    run: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn on(url: String) -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            run: tempfile::tempdir().unwrap(),
            url,
        }
    }

    fn command(&self, program: &str, args: &[&str]) -> Command {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(self.home.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home.path())
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.run.path())
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_CAPTURE", "save")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }
}

fn ljos() -> &'static str {
    env!("CARGO_BIN_EXE_ljos")
}

fn text_of(out: &std::process::Output) -> String {
    format!(
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn atoms(url: &str) -> Vec<Value> {
    let got: Value = ureq::get(&format!("{url}/v1/atoms"))
        .query("workspace", "seat")
        .query("embedding", "omit")
        .call()
        .expect("GET /v1/atoms")
        .into_json()
        .expect("atoms json");
    got["atoms"].as_array().cloned().unwrap_or_default()
}

fn by_text<'a>(atoms: &'a [Value], needle: &str) -> &'a Value {
    atoms
        .iter()
        .find(|atom| {
            atom.get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains(needle))
        })
        .unwrap_or_else(|| panic!("no atom containing {needle}"))
}

fn basis(atom: &Value) -> Option<&str> {
    atom.pointer("/source/basis")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|b| !b.is_empty())
}

/// The suite may already sit in a runner scope (`cursor-agent`). Otherwise
/// move this process into a child cgroup whose name is a runner, so a
/// detached child inherits a scope the check can see.
fn ensure_runner_scope() {
    let text = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    if cgroup_names_runner(&text) {
        return;
    }
    let path = text
        .lines()
        .find_map(|line| line.rsplit_once(':').map(|(_, path)| path))
        .filter(|path| !path.is_empty())
        .unwrap_or("/");
    let dir = PathBuf::from("/sys/fs/cgroup")
        .join(path.trim_start_matches('/'))
        .join("claude");
    // A hosted runner keeps this process in a service cgroup the test
    // user cannot extend. Root creates the child scope and moves the
    // process; detached children inherit that scope.
    if std::fs::create_dir_all(&dir).is_err() {
        let made = Command::new("sudo")
            .args(["-n", "mkdir", "-p"])
            .arg(&dir)
            .output()
            .unwrap_or_else(|err| panic!("sudo mkdir {dir:?} failed to start: {err}"));
        assert!(
            made.status.success(),
            "could not create {dir:?} for the detach test\n{}",
            text_of(&made)
        );
    }
    let procs = dir.join("cgroup.procs");
    let pid = std::process::id().to_string();
    if std::fs::write(&procs, &pid).is_err() {
        let mut tee = Command::new("sudo");
        tee.args(["-n", "tee"])
            .arg(&procs)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = tee
            .spawn()
            .unwrap_or_else(|err| panic!("sudo tee {procs:?} failed to start: {err}"));
        child
            .stdin
            .take()
            .expect("tee stdin")
            .write_all(pid.as_bytes())
            .unwrap_or_else(|err| panic!("could not write {pid} into {procs:?}: {err}"));
        let done = child
            .wait_with_output()
            .unwrap_or_else(|err| panic!("sudo tee {procs:?} failed: {err}"));
        assert!(
            done.status.success(),
            "could not move into {procs:?}; cgroup was {text}\n{}",
            String::from_utf8_lossy(&done.stderr)
        );
    }
    let moved = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    assert!(
        cgroup_names_runner(&moved),
        "cgroup {moved:?} still names no runner after moving into {dir:?}"
    );
}

fn cgroup_names_runner(text: &str) -> bool {
    let runners = ["cursor", "claude", "codex"];
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.len() >= 4 && runners.contains(&word))
}

fn keep_refused(out: &std::process::Output, how: &str) {
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("a runner cannot keep"),
        "{how} was not refused as a runner\n{}",
        text_of(out)
    );
}

fn prefer_as_agent(seat: &Seat, out: &std::process::Output, needle: &str, how: &str) {
    assert!(
        out.status.success(),
        "{how} prefer failed\n{}",
        text_of(out)
    );
    let stored = atoms(&seat.url);
    let stored = by_text(&stored, needle);
    assert_eq!(basis(stored), Some("agent"), "{how}: {stored}");
    assert_ne!(stored["origin"], "user-declared", "{how}: {stored}");
}

/// Run `body` under `setsid -f`, so the shell's parent is init and the
/// runner is not an ancestor. `-w` would keep `setsid` and this test in
/// the tree, which is not the bypass.
fn detached(seat: &Seat, tag: &str, body: &str) -> std::process::Output {
    let dir = seat.home.path().join(tag);
    std::fs::create_dir_all(&dir).unwrap();
    let stdout = dir.join("stdout");
    let stderr = dir.join("stderr");
    let done = dir.join("done");
    let line = format!(
        "{body} >'{}' 2>'{}'; echo $? > '{}'",
        stdout.display(),
        stderr.display(),
        done.display()
    );
    let launch = seat
        .command("setsid", &["-f", "sh", "-c", &line])
        .output()
        .unwrap_or_else(|err| panic!("setsid {tag}: {err}"));
    assert!(
        launch.status.success(),
        "setsid {tag} did not start\n{}",
        text_of(&launch)
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done.is_file() {
        assert!(
            Instant::now() < deadline,
            "{tag} did not finish\n{}",
            text_of(&launch)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let code: i32 = std::fs::read_to_string(&done)
        .unwrap()
        .trim()
        .parse()
        .unwrap_or(1);
    let mut out = Command::new("true").output().unwrap();
    out.stdout = std::fs::read(&stdout).unwrap_or_default();
    out.stderr = std::fs::read(&stderr).unwrap_or_default();
    if code != 0 {
        out.status = std::os::unix::process::ExitStatusExt::from_raw(code << 8);
    }
    out
}

fn setsid_keep(seat: &Seat) -> std::process::Output {
    let bin = ljos();
    detached(
        seat,
        "setsid",
        &format!("{bin} prefer --review --keep deadbeef"),
    )
}

fn setsid_prefer(seat: &Seat, text: &str) -> std::process::Output {
    let bin = ljos();
    detached(seat, "prefer", &format!("{bin} prefer '{text}'"))
}

fn script_keep(seat: &Seat) -> std::process::Output {
    let log = seat.home.path().join("script.log");
    let bin = ljos();
    let mut out = detached(
        seat,
        "script",
        &format!(
            "script -q -e -c '{bin} prefer --review --keep deadbeef' '{log}'",
            log = log.display()
        ),
    );
    // script keeps the child's terminal on the typescript, not on stderr.
    if let Ok(logged) = std::fs::read(&log) {
        out.stderr.extend(logged);
    }
    out
}

fn env_i_keep(seat: &Seat) -> std::process::Output {
    let bin = ljos();
    let home = seat.home.path().display().to_string();
    let runtime = seat.run.path().display().to_string();
    let body = format!(
        "env -i PATH=/usr/bin:/bin HOME='{home}' USER=person \
         XDG_CONFIG_HOME='{home}/.config' XDG_STATE_HOME='{home}/state' \
         XDG_RUNTIME_DIR='{runtime}' PACKSET_URL='{url}' PACKSET_WORKSPACE=seat \
         PACKSET_AUTH=off PACKSET_TIMEOUT_MS=5000 LJOS_USER_RULES_SYNC=off \
         {bin} prefer --review --keep deadbeef",
        url = seat.url
    );
    detached(seat, "env-i", &body)
}

fn double_fork_keep(seat: &Seat) -> std::process::Output {
    let captured = seat.home.path().join("fork.stderr");
    let bin = ljos();
    // The process that execs ljos is the second child, reparented to init.
    // A third fork would leave python as the parent and the tree would
    // still name a program.
    let script = r#"
import os, sys
if os.fork() > 0:
    os._exit(0)
os.setsid()
if os.fork() > 0:
    os._exit(0)
fd = os.open(sys.argv[2], os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
os.dup2(fd, 2)
os.close(fd)
os.execv(sys.argv[1], [sys.argv[1], "prefer", "--review", "--keep", "deadbeef"])
"#;
    let out = seat
        .command(
            "python3",
            &["-c", script, bin, &captured.display().to_string()],
        )
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "double fork launcher\n{}",
        text_of(&out)
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let stderr = loop {
        let got = std::fs::read(&captured).unwrap_or_default();
        if !got.is_empty() {
            break got;
        }
        assert!(
            Instant::now() < deadline,
            "double fork produced no stderr\n{}",
            text_of(&out)
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut built = Command::new("true").output().unwrap();
    built.stderr = stderr;
    built
}

fn tmux_keep(seat: &Seat) -> std::process::Output {
    let sock = seat.home.path().join("tmux.sock");
    let captured = seat.home.path().join("tmux.err");
    let marker = seat.home.path().join("tmux.done");
    let bin = ljos();
    let line = format!(
        "{bin} prefer --review --keep deadbeef >{out} 2>{err}; echo $? > {marker}",
        bin = bin,
        out = seat.home.path().join("tmux.out").display(),
        err = captured.display(),
        marker = marker.display(),
    );
    let out = seat
        .command(
            "tmux",
            &[
                "-S",
                &sock.display().to_string(),
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                &line,
            ],
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "tmux\n{}", text_of(&out));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !marker.is_file() {
        assert!(
            Instant::now() < deadline,
            "tmux session did not finish\n{}",
            text_of(&out)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = Command::new("tmux")
        .args(["-S", &sock.display().to_string(), "kill-server"])
        .output();
    let stderr = std::fs::read(&captured).unwrap_or_default();
    let mut built = Command::new("true").output().unwrap();
    built.stderr = stderr;
    built.status = std::os::unix::process::ExitStatusExt::from_raw(1 << 8);
    built
}

fn hook_payload() -> String {
    json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "forged-hook-1",
        "prompt": "From now on always use bun, never yarn.",
    })
    .to_string()
}

fn forged_hook(seat: &Seat) -> std::process::Output {
    let payload = seat.home.path().join("hook.json");
    std::fs::write(&payload, hook_payload()).unwrap();
    let bin = ljos();
    detached(
        seat,
        "hook",
        &format!("{bin} hook < '{}'", payload.display()),
    )
}

/// Detaching does not make a keep, a prefer, or a prompt hook the person's.
#[test]
fn detaching_does_not_make_this_process_the_person() {
    ensure_runner_scope();
    let daemon = Daemon::start();
    let seat = Seat::on(daemon.url.clone());

    let direct = seat
        .command(ljos(), &["prefer", "--review", "--keep", "deadbeef"])
        .output()
        .unwrap();
    let direct_err = String::from_utf8_lossy(&direct.stderr);
    assert!(
        direct_err.contains("needs a terminal"),
        "a direct keep should miss the terminal, not the runner\n{}",
        text_of(&direct)
    );
    assert!(
        !direct_err.contains("a runner cannot keep"),
        "the test process itself was taken for a runner\n{}",
        text_of(&direct)
    );

    let shaped = terminal::shape(
        "terminal",
        ljos(),
        &["prefer", "Keep the release notes in one file."],
    );
    let typed = seat.command(terminal::PYTHON, &shaped).output().unwrap();
    assert!(
        typed.status.success(),
        "terminal prefer\n{}",
        text_of(&typed)
    );
    let stored = atoms(&seat.url);
    let stored = by_text(&stored, "release notes");
    assert_eq!(basis(stored), Some("person"), "{stored}");

    keep_refused(&setsid_keep(&seat), "setsid");
    keep_refused(&env_i_keep(&seat), "setsid env -i");
    keep_refused(&script_keep(&seat), "script");
    keep_refused(&double_fork_keep(&seat), "double fork");
    keep_refused(&tmux_keep(&seat), "tmux");

    let detached = setsid_prefer(&seat, "Name release branches after the month.");
    prefer_as_agent(&seat, &detached, "release branches", "setsid prefer");

    let hook = forged_hook(&seat);
    assert!(hook.status.success(), "hook\n{}", text_of(&hook));
    let err = String::from_utf8_lossy(&hook.stderr);
    assert!(
        err.contains("not kept as the person's"),
        "a detached hook was kept\n{}",
        text_of(&hook)
    );
    let saved = atoms(&seat.url);
    let saved = saved.iter().find(|atom| {
        atom.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("use bun"))
    });
    assert!(saved.is_none(), "forged hook stored {saved:?}");
}

/// `tmux send-keys` into a session that already exists types into a real
/// pane. The pane has a terminal. The server was started in the runner's
/// cgroup, so the command is still the runner's.
#[test]
fn tmux_send_keys_into_an_existing_session_is_not_the_person() {
    ensure_runner_scope();
    let daemon = Daemon::start();
    let seat = Seat::on(daemon.url.clone());
    let sock = seat.home.path().join("send.sock");
    let out_path = seat.home.path().join("send.out");
    let err_path = seat.home.path().join("send.err");
    let done = seat.home.path().join("send.done");
    let sock_s = sock.display().to_string();
    let started = seat
        .command(
            "tmux",
            &[
                "-S",
                &sock_s,
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-s",
                "stay",
            ],
        )
        .output()
        .unwrap();
    assert!(started.status.success(), "tmux\n{}", text_of(&started));
    std::thread::sleep(Duration::from_millis(300));
    let line = format!(
        "{} prefer 'Use tabs in every Makefile.' >'{}' 2>'{}'; echo $? > '{}'",
        ljos(),
        out_path.display(),
        err_path.display(),
        done.display()
    );
    let sent = seat
        .command(
            "tmux",
            &["-S", &sock_s, "send-keys", "-t", "stay", &line, "Enter"],
        )
        .output()
        .unwrap();
    assert!(sent.status.success(), "send-keys\n{}", text_of(&sent));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done.is_file() {
        assert!(
            Instant::now() < deadline,
            "send-keys did not run\n{}",
            text_of(&sent)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = Command::new("tmux")
        .args(["-S", &sock_s, "kill-server"])
        .output();
    let mut built = Command::new("true").output().unwrap();
    built.stdout = std::fs::read(&out_path).unwrap_or_default();
    built.stderr = std::fs::read(&err_path).unwrap_or_default();
    if std::fs::read_to_string(&done).unwrap_or_default().trim() != "0" {
        built.status = std::os::unix::process::ExitStatusExt::from_raw(1 << 8);
    }
    prefer_as_agent(&seat, &built, "every Makefile", "tmux send-keys");
}

/// `systemd-run --user --scope` when the user bus answers, and otherwise
/// the `run-*.scope` cgroup that command creates. A pty is attached, which
/// is the shape that used to pass the terminal check after the environment
/// was cleared.
#[test]
fn a_transient_scope_is_not_the_person() {
    let daemon = Daemon::start();
    let seat = Seat::on(daemon.url.clone());
    let out = if user_bus_answers() {
        systemd_run_prefer(&seat, "Keep release notes in one file.")
    } else {
        scope_prefer(&seat, "Keep release notes in one file.")
    };
    prefer_as_agent(&seat, &out, "release notes", "transient scope");
}

fn user_bus_answers() -> bool {
    Command::new("systemd-run")
        .args(["--user", "--scope", "--collect", "--quiet", "--", "true"])
        .output()
        .is_ok_and(|out| out.status.success())
}

fn systemd_run_prefer(seat: &Seat, text: &str) -> std::process::Output {
    seat.command(
        "systemd-run",
        &[
            "--user",
            "--scope",
            "--collect",
            "--pty",
            "--",
            ljos(),
            "prefer",
            text,
        ],
    )
    .output()
    .unwrap_or_else(|err| panic!("systemd-run: {err}"))
}

/// Move a detached `script` into a transient scope and run `ljos prefer`
/// on the pty `script` opens. The cgroup name is what `systemd-run
/// --scope` uses, and it does not contain a runner's name.
fn scope_prefer(seat: &Seat, text: &str) -> std::process::Output {
    let cg = PathBuf::from("/sys/fs/cgroup/system.slice/run-u1000-ljostest.scope");
    if !cg.is_dir() {
        let made = Command::new("sudo")
            .args(["-n", "mkdir", "-p"])
            .arg(&cg)
            .output()
            .unwrap_or_else(|err| panic!("sudo mkdir {cg:?}: {err}"));
        assert!(
            made.status.success(),
            "could not create {cg:?}\n{}",
            text_of(&made)
        );
    }
    let log = seat.home.path().join("scope-script.log");
    let bin = ljos();
    let procs = cg.join("cgroup.procs");
    let body = format!(
        "echo $$ | sudo -n tee '{procs}' >/dev/null && script -q -e -c \"{bin} prefer '{text}'\" '{log}'",
        procs = procs.display(),
        log = log.display(),
    );
    let mut out = detached(seat, "scope", &body);
    if let Ok(logged) = std::fs::read(&log) {
        out.stderr.extend(logged.iter().copied());
        out.stdout.extend(logged);
    }
    out
}
