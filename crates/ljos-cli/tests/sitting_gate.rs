//! Under an agent runner, a git verb that writes history waits for a
//! sitting: a live claim this seat holds on an open tracker ticket. The
//! refusal names `ljos sitting ID`. `LJOS_OFF` and a temporary `HOME` do
//! not lift it. A push onto develop or main also waits for the seats on
//! that ticket, and that refusal names `ljos consensus ID --no-file`. The
//! person at their own terminal is never refused.

ljos_test_boot::harness!();
#[path = "common/terminal.rs"]
mod terminal;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SESSION: &str = "5b0c2a71-9d1e-4f3a-8c6b-2e7f1a0d9c43";
const ISSUE: &str = "proj-gate";

fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn text_of(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A writer that answers `GET /v1/atoms` with an empty roster.
fn empty_pack() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 1024];
            loop {
                let n = std::io::Read::read(&mut stream, &mut buf).unwrap_or(0);
                if n == 0 || buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let body = r#"{"atoms":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    format!("http://{addr}")
}

struct Seat {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    home: PathBuf,
    run: PathBuf,
    repo: PathBuf,
    card: PathBuf,
    ballots: PathBuf,
}

impl Seat {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let bin = root.join("bin");
        let home = root.join("home");
        let run = root.join("run");
        let repo = root.join("repo");
        let card = root.join("card.json");
        let ballots = root.join("ballots.json");
        for d in [&bin, &home, &run, &repo] {
            std::fs::create_dir_all(d).unwrap();
        }
        script(
            &bin.join("claimdag"),
            "#!/bin/sh\ncase \"$1\" in\n  claim) echo gen=2 ;;\nesac\nexit 0\n",
        );
        script(
            &bin.join("vissue"),
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  show) if [ \"$2\" = {ISSUE} ] && [ -f '{}' ]; then cat '{}'; else echo '{{}}'; fi ;;\n  vote) cat '{}' 2>/dev/null || echo '[]' ;;\n  claim) echo claimed ;;\nesac\nexit 0\n",
                card.display(),
                card.display(),
                ballots.display()
            ),
        );
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(init.status.success(), "{}", text_of(&init));
        Self {
            _dir: dir,
            bin,
            home,
            run,
            repo,
            card,
            ballots,
        }
    }

    fn command(&self, program: &str, args: &[&str]) -> Command {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("LJOS_SEAT", "inky")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("GROK_SESSION_ID", SESSION)
            .current_dir(&self.repo)
            .stdin(Stdio::null());
        cmd
    }

    /// `ljos ARGS` as a runner's tool call runs it: detached from any
    /// terminal, with the runner's session variable set.
    fn runner(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut all = vec!["-w", env!("CARGO_BIN_EXE_ljos")];
        all.extend_from_slice(args);
        let mut cmd = self.command("setsid", &all);
        cmd.envs(env.iter().copied());
        cmd.output().unwrap()
    }

    fn policy(&self, line: &str, env: &[(&str, &str)]) -> String {
        let out = self.runner(&["policy", line], env);
        assert!(out.status.success(), "{line}\n{}", text_of(&out));
        text_of(&out)
    }

    fn hook(&self, line: &str) -> String {
        self.hook_with(line, &[])
    }

    /// A Codex-shaped hook: `turn_id` means an ask is written down, because
    /// that runner does not stop for one.
    fn hook_turn(&self, line: &str, env: &[(&str, &str)]) -> String {
        let payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "session_id": SESSION,
            "turn_id": "turn-cloud",
            "cwd": self.repo,
            "tool_name": "Bash",
            "tool_input": {"command": line},
        })
        .to_string();
        self.hook_payload(&payload, env)
    }

    fn hook_with(&self, line: &str, env: &[(&str, &str)]) -> String {
        let payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "session_id": SESSION,
            "cwd": self.repo,
            "tool_name": "Bash",
            "tool_input": {"command": line},
        })
        .to_string();
        self.hook_payload(&payload, env)
    }

    fn hook_payload(&self, payload: &str, env: &[(&str, &str)]) -> String {
        let mut cmd = self.command("setsid", &["-w", env!("CARGO_BIN_EXE_ljos"), "hook"]);
        cmd.envs(env.iter().copied());
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{payload}\n{}", text_of(&out));
        text_of(&out)
    }

    /// `ljos git-hook pre-push` under the same runner, with git's ref list
    /// on stdin.
    fn git_hook_push(&self, refs: &str, env: &[(&str, &str)]) -> Output {
        let mut cmd = self.command(
            "setsid",
            &["-w", env!("CARGO_BIN_EXE_ljos"), "git-hook", "pre-push"],
        );
        cmd.envs(env.iter().copied());
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(refs.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn card(&self, state: &str, by: &str) {
        std::fs::write(
            &self.card,
            serde_json::json!({"id": ISSUE, "state": state, "claimed_by": by}).to_string(),
        )
        .unwrap();
    }

    fn vote(&self, ballots: &[(&str, &str)]) {
        let rows: Vec<_> = ballots
            .iter()
            .map(|(agent, choice)| serde_json::json!({"agent": agent, "choice": choice}))
            .collect();
        std::fs::write(&self.ballots, serde_json::Value::from(rows).to_string()).unwrap();
    }

    /// Claim the ticket in this conversation and have the tracker read
    /// it as started and held by this seat.
    fn sit(&self) {
        let out = self.runner(&["claim", ISSUE], &[]);
        assert!(out.status.success(), "{}", text_of(&out));
        self.card(
            "STARTED",
            &format!("inky@{}", ljos_cli::session_tag(SESSION)),
        );
    }
}

fn not_refused(said: &str, line: &str) {
    assert!(
        !said.contains("sitting-gate"),
        "`{line}` was refused:\n{said}"
    );
}

#[test]
fn a_runner_without_a_live_sitting_is_not_refused() {
    let seat = Seat::new();
    for line in [
        "git commit -m x",
        "git merge topic",
        "git rebase main",
        "git cherry-pick abc123",
        "git push origin topic",
        "cd . && git -C . commit --amend --no-edit",
        "git status",
        "git log -1",
        "git push --dry-run origin topic",
    ] {
        not_refused(&seat.policy(line, &[]), line);
    }
    not_refused(
        &seat.policy("git commit -m x", &[("LJOS_OFF", "1")]),
        "git commit with LJOS_OFF",
    );
    not_refused(
        &seat.policy("git commit -m x", &[("LJOS_GUARD", "on")]),
        "git commit with the guard on",
    );
    not_refused(&seat.hook("git commit -m x"), "the hook's git commit");

    let out = seat.runner(&["claim", ISSUE], &[]);
    assert!(out.status.success(), "{}", text_of(&out));
    let holder = format!("inky@{}", ljos_cli::session_tag(SESSION));
    seat.card("STARTED", &holder);
    for line in ["git commit -m x", "git push origin topic"] {
        not_refused(&seat.policy(line, &[]), line);
    }
    seat.card("DONE", &holder);
    not_refused(&seat.policy("git commit -m x", &[]), "a finished ticket");
    seat.card("STARTED", "other@abc");
    not_refused(
        &seat.policy("git commit -m x", &[]),
        "another seat's ticket",
    );
}

fn asks_for_consensus(said: &str, line: &str) {
    assert!(
        said.contains("sitting-gate"),
        "`{line}` was not held:\n{said}"
    );
    assert!(said.contains("settled consensus"), "{line}:\n{said}");
}

#[test]
fn a_runner_pushes_onto_develop_or_main_once_the_seats_clear_it() {
    let seat = Seat::new();
    seat.sit();
    let pack = empty_pack();
    // The stub tracker holds no signature notes; signed ballots have
    // their own suite.
    let env = [
        ("PACKSET_URL", pack.as_str()),
        ("LJOS_UNSIGNED_BALLOTS", "allow"),
    ];
    for line in ["git push origin topic", "git push -u origin HEAD:topic"] {
        not_refused(&seat.policy(line, &env), line);
    }
    let develop = "git push origin develop";
    asks_for_consensus(&seat.policy(develop, &env), develop);
    seat.vote(&[("flint", "approve")]);
    asks_for_consensus(&seat.policy(develop, &env), develop);
    seat.vote(&[("inky", "approve"), ("flint", "approve")]);
    for line in [develop, "git push origin HEAD:main"] {
        asks_for_consensus(&seat.policy(line, &env), line);
    }
    asks_for_consensus(&seat.hook_with(develop, &env), "the hook's push");

    seat.vote(&[("inky", "revise"), ("flint", "revise")]);
    asks_for_consensus(
        &seat.policy("git push origin HEAD:main", &env),
        "a push onto main",
    );
    asks_for_consensus(&seat.hook_with(develop, &env), "the hook's push");

    seat.vote(&[("inky", "approve"), ("flint", "approve")]);
    let config = seat.home.join(".config/ljos");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("ljos.toml"),
        "[gates]\nquorum = 3\nconsensus_branches = [\"release\"]\n",
    )
    .unwrap();
    asks_for_consensus(
        &seat.policy("git push origin release", &env),
        "a push onto release",
    );
    not_refused(&seat.policy(develop, &env), "develop, no longer named");
    std::fs::write(config.join("ljos.toml"), "quorum = 0\n").unwrap();
    let said = seat.policy(develop, &env);
    assert!(said.contains("sitting-gate"), "{said}");
    assert!(said.contains("`quorum`"), "{said}");
}

fn gate_row(out: &Output) -> String {
    let said = text_of(out);
    said.lines()
        .find(|line| line.contains("\tsitting gate\t"))
        .unwrap_or_else(|| panic!("no sitting gate row:\n{said}"))
        .to_string()
}

/// `ljos doctor` says whether the gate holds this seat. Under a runner it
/// names the seat, the sitting it finds and what a protected push waits
/// for, and warns when the settings do not read. For the person it is
/// `info`: nothing there is gated.
#[test]
fn the_doctor_says_whether_the_gate_holds_this_seat() {
    let seat = Seat::new();
    let line = gate_row(&seat.runner(&["doctor", "--all"], &[]));
    assert!(line.starts_with("ok\t"), "{line}");
    assert!(line.contains("active for inky"), "{line}");
    assert!(line.contains("holds no live sitting"), "{line}");
    assert!(line.contains("`develop`, `main`"), "{line}");

    seat.sit();
    let line = gate_row(&seat.runner(&["doctor"], &[]));
    assert!(line.starts_with("ok\t"), "{line}");
    assert!(line.contains(&format!("sits on {ISSUE}")), "{line}");

    let config = seat.home.join(".config/ljos");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("ljos.toml"), "quorum = 0\n").unwrap();
    let line = gate_row(&seat.runner(&["doctor"], &[]));
    assert!(line.starts_with("warn\t"), "{line}");
    assert!(line.contains("`quorum`"), "{line}");

    let typed = terminal::shape("terminal", env!("CARGO_BIN_EXE_ljos"), &["doctor"]);
    let out = seat.command(terminal::PYTHON, &typed).output().unwrap();
    let line = gate_row(&out);
    assert!(line.starts_with("info\t"), "{line}");
    assert!(line.contains("not active here"), "{line}");
}

fn cloud_mark(seat: &Seat) {
    let dir = seat.home.join(".config/ljos");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("push-allow"), "").unwrap();
}

fn allow_key(path: &Path) -> String {
    let canon = path.canonicalize().unwrap();
    let text = canon.to_string_lossy().to_string();
    let text = text.trim_end_matches('/');
    text.strip_suffix(".git").unwrap_or(text).to_string()
}

fn write_allow(seat: &Seat, origin: &Path) {
    let path = seat.home.join(".config/ljos/push-allow");
    let body = format!(
        "# ljos cloud push allowlist\ncode\t{}\nbranch\tdevelop\nbranch\tmain\n",
        allow_key(origin)
    );
    std::fs::write(path, body).unwrap();
}

fn git_ok(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}\n{}", text_of(&out));
}

/// A cloud seat cannot collect another seat's ballot: the tracker is
/// private to the run. A protected push that is missing only that ballot,
/// with the sitting live and the allowlist already accepting the
/// repository and the branch, asks for that one command. A force, a
/// delete, a cursor ref, another repository, and a push with no sitting
/// stay refused.
#[test]
fn a_cloud_seat_asks_for_one_protected_push_when_no_seat_has_voted() {
    let seat = Seat::new();
    let origin = seat._dir.path().join("origin.git");
    let other = seat._dir.path().join("other.git");
    git_ok(
        seat._dir.path(),
        &["init", "--bare", "-q", origin.to_str().unwrap()],
    );
    git_ok(
        seat._dir.path(),
        &["init", "--bare", "-q", other.to_str().unwrap()],
    );
    let origin = origin.canonicalize().unwrap();
    let other = other.canonicalize().unwrap();
    git_ok(
        &seat.repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git_ok(
        &seat.repo,
        &["remote", "add", "other", other.to_str().unwrap()],
    );
    cloud_mark(&seat);
    write_allow(&seat, &origin);
    let pack = empty_pack();
    let env = [
        ("PACKSET_URL", pack.as_str()),
        ("LJOS_UNSIGNED_BALLOTS", "allow"),
        // A temporary HOME would leave the cloud rules unread. This seat
        // is a cloud seat, so those rules have to run.
        ("LJOS_GUARD", "on"),
    ];
    let develop = "git push origin develop";

    let idle = seat.policy(develop, &env);
    asks_for_consensus(&idle, develop);

    seat.sit();
    let said = seat.policy(develop, &env);
    asks_for_consensus(&said, develop);
    assert!(said.contains("\nask:"), "{said}");
    assert!(!said.contains("\ndeny:"), "{said}");

    let hooked = seat.hook_turn(develop, &env);
    assert!(hooked.contains("ljos approve "), "{hooked}");
    assert!(hooked.contains("ljos_request_approval"), "{hooked}");
    assert!(hooked.contains("Reply `approve "), "{hooked}");
    assert!(
        hooked.contains("\"permissionDecision\":\"deny\""),
        "{hooked}"
    );
    let store = std::fs::read_to_string(seat.run.join("ljos/approvals/requests.json"))
        .expect("the hook wrote a one-use request");
    let command = serde_json::to_string(develop).unwrap();
    assert!(store.contains(&command), "{store}");
    assert_eq!(store.matches("\"approved\":false").count(), 1, "{store}");

    for line in [
        "git push --force origin develop",
        "git push --force-with-lease origin develop",
        "git push origin :develop",
        "git push origin --delete develop",
        "git push origin HEAD:cursor/topic",
    ] {
        let said = seat.policy(line, &env);
        assert!(!said.contains("\nask:"), "{line}\n{said}");
        assert!(
            said.contains("\ndeny:") || said.contains("deny:"),
            "{line}\n{said}"
        );
    }

    let seats = seat.home.join(".config/ljos/seats/inky");
    std::fs::create_dir_all(&seats).unwrap();
    std::fs::write(seats.join("ljos.toml"), "[cloud]\nbranches = [\"main\"]\n").unwrap();
    let main = seat.policy("git push origin main", &env);
    asks_for_consensus(&main, "git push origin main");
    assert!(main.contains("\nask:"), "{main}");
    assert!(!main.contains("\ndeny:"), "{main}");

    seat.vote(&[("inky", "revise"), ("flint", "revise")]);
    let back = seat.policy(develop, &env);
    asks_for_consensus(&back, develop);
}

/// One name on the roster settles a protected push from that seat's vote,
/// for `ljos policy` and for the `pre-push` git hook.
#[test]
fn a_solo_seat_clears_policy_and_the_git_hook() {
    let seat = Seat::new();
    let tracker = seat.home.join("tracker");
    std::fs::create_dir_all(&tracker).unwrap();
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&tracker)
        .output()
        .unwrap();
    assert!(init.status.success(), "{}", text_of(&init));
    std::fs::write(tracker.join("vissue.toml"), "seats = [\"inky\"]\n").unwrap();
    let root = tracker.to_str().unwrap();
    let env = [
        ("VISSUE_ROOT", root),
        ("ISSUE_ROOT", root),
        ("VISSUE_NO_ROUTE", "1"),
        ("LJOS_UNSIGNED_BALLOTS", "allow"),
    ];
    seat.sit();
    let develop = "git push origin develop";
    asks_for_consensus(&seat.policy(develop, &env), develop);
    seat.vote(&[("inky", "approve")]);
    not_refused(&seat.policy(develop, &env), develop);
    let refs = "aaaa bbbb refs/heads/develop refs/heads/develop\n";
    let out = seat.git_hook_push(refs, &env);
    let said = text_of(&out);
    assert!(
        out.status.success(),
        "the git hook refused a solo seat\n{said}"
    );
    assert!(!said.contains("1 of the 2 seats"), "{said}");
}

/// A gates file that does not parse asks. The git hook does not answer
/// with the two-seat quorum line.
#[test]
fn a_gates_file_that_does_not_parse_asks_the_git_hook() {
    let seat = Seat::new();
    std::fs::create_dir_all(seat.repo.join(".ljos")).unwrap();
    std::fs::write(seat.repo.join(".ljos/gates.toml"), "quorum = [\n").unwrap();
    let develop = "git push origin develop";
    let said = seat.policy(develop, &[]);
    assert!(said.contains("do not read"), "{said}");
    assert!(!said.contains("1 of the 2 seats"), "{said}");
    assert!(
        said.contains("ask"),
        "a malformed gates file was not an ask\n{said}"
    );
    let refs = "aaaa bbbb refs/heads/develop refs/heads/develop\n";
    let out = seat.git_hook_push(refs, &[]);
    let hooked = text_of(&out);
    assert!(
        !out.status.success(),
        "a malformed gates file let the hook through\n{hooked}"
    );
    assert!(hooked.contains("do not read"), "{hooked}");
    assert!(!hooked.contains("1 of the 2 seats"), "{hooked}");
}

/// One commit check returns while the adopt lock is held and the tracker
/// program does not answer. A guard git on the search list is not the git
/// the check starts.
#[test]
fn a_commit_check_finishes_with_a_stuck_lock_and_a_slow_vissue() {
    use std::os::unix::io::AsRawFd;
    use std::time::{Duration, Instant};
    let seat = Seat::new();
    let share = seat.home.join(".local/share");
    let guard = share.join("ljos/guard-bin");
    std::fs::create_dir_all(&guard).unwrap();
    script(&guard.join("git"), "#!/bin/sh\nsleep 30\nexit 0\n");
    script(
        &seat.bin.join("vissue"),
        "#!/bin/sh\nsleep 30\nprintf '%s\\n' '{\"id\":\"proj-gate\",\"state\":\"STARTED\",\"claimed_by\":\"inky\"}'\n",
    );
    let runtime = seat.run.join("ljos");
    std::fs::create_dir_all(&runtime).unwrap();
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(runtime.join("hold-adopt.lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX) },
        0
    );
    let fake = seat.bin.join("ljos-mcp");
    std::os::unix::fs::symlink("/bin/sleep", &fake).unwrap();
    let mut sleeper = Command::new(&fake).arg("300").spawn().unwrap();
    let bare = ljos_cli::work_id(&format!("inky:{ISSUE}"));
    std::fs::write(
        runtime.join(format!("hold-{bare}")),
        format!(
            "inky\ninky\n{}\nljos-mcp\n2026-10-25T00:00:00Z\n{ISSUE}\n-\n",
            sleeper.id()
        ),
    )
    .unwrap();
    let path = format!("{}:{}:/usr/bin:/bin", guard.display(), seat.bin.display());
    let extra = [("CURSOR_CONVERSATION_ID", SESSION)];
    let run = |args: &[&str], stdin: Option<&str>| -> Duration {
        let mut cmd = seat.command(env!("CARGO_BIN_EXE_ljos"), args);
        cmd.envs(extra.iter().copied())
            .env("PATH", &path)
            .env("XDG_DATA_HOME", &share)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if args.first() == Some(&"hook") {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().unwrap();
        if let Some(body) = stdin {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(body.as_bytes())
                .unwrap();
        }
        let pid = child.id();
        let started = Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        let out = rx.recv_timeout(Duration::from_secs(3)).unwrap_or_else(|_| {
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            panic!("commit check hung");
        });
        let out = out.unwrap();
        let took = started.elapsed();
        assert!(
            out.status.success(),
            "commit check failed after {took:?}\n{}",
            text_of(&out)
        );
        assert!(
            took < Duration::from_secs(2),
            "commit check took {took:?}\n{}",
            text_of(&out)
        );
        took
    };
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": SESSION,
        "conversation_id": SESSION,
        "cwd": seat.repo,
        "tool_name": "Bash",
        "tool_input": {"command": "git commit -m x"},
    })
    .to_string();
    let hook_took = run(&["hook"], Some(&payload));
    let pre_took = run(&["git-hook", "pre-commit"], None);
    let _ = sleeper.kill();
    let _ = sleeper.wait();
    drop(lock_file);
    eprintln!("commit check hook {hook_took:?} pre-commit {pre_took:?}");
}

/// One push check returns while the adopt lock is held and the tracker
/// program does not answer. The check does not refuse, and it returns
/// inside the hang guard.
#[test]
fn a_push_check_finishes_with_a_stuck_lock_and_a_slow_vissue() {
    use std::os::unix::io::AsRawFd;
    use std::time::{Duration, Instant};
    let seat = Seat::new();
    let share = seat.home.join(".local/share");
    let guard = share.join("ljos/guard-bin");
    std::fs::create_dir_all(&guard).unwrap();
    script(&guard.join("git"), "#!/bin/sh\nsleep 30\nexit 0\n");
    script(
        &seat.bin.join("vissue"),
        "#!/bin/sh\nsleep 30\nprintf '%s\\n' '{\"id\":\"proj-gate\",\"state\":\"STARTED\",\"claimed_by\":\"inky\"}'\n",
    );
    let runtime = seat.run.join("ljos");
    std::fs::create_dir_all(&runtime).unwrap();
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(runtime.join("hold-adopt.lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX) },
        0
    );
    let fake = seat.bin.join("ljos-mcp");
    std::os::unix::fs::symlink("/bin/sleep", &fake).unwrap();
    let mut sleeper = Command::new(&fake)
        .arg("300")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let bare = ljos_cli::work_id(&format!("inky:{ISSUE}"));
    std::fs::write(
        runtime.join(format!("hold-{bare}")),
        format!(
            "inky\ninky\n{}\nljos-mcp\n2026-10-25T00:00:00Z\n{ISSUE}\n-\n",
            sleeper.id()
        ),
    )
    .unwrap();
    let path = format!("{}:{}:/usr/bin:/bin", guard.display(), seat.bin.display());
    let mut cmd = seat.command(env!("CARGO_BIN_EXE_ljos"), &["git-hook", "pre-push"]);
    cmd.env("PATH", &path)
        .env("XDG_DATA_HOME", &share)
        .env("CURSOR_CONVERSATION_ID", SESSION)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn().unwrap();
    let pid = child.id();
    let started = Instant::now();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let out = rx.recv_timeout(Duration::from_secs(3)).unwrap_or_else(|_| {
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        panic!("push check hung");
    });
    let out = out.unwrap();
    let took = started.elapsed();
    let said = text_of(&out);
    assert!(
        out.status.success(),
        "push check was refused after {took:?}\n{said}"
    );
    assert!(
        said.contains("the tracker or lock was slow"),
        "push check after {took:?}\n{said}"
    );
    assert!(
        !said.contains("this push is refused"),
        "push check was refused after {took:?}\n{said}"
    );
    assert!(
        took < Duration::from_secs(3),
        "push check took {took:?}\n{said}"
    );
    let _ = sleeper.kill();
    let _ = sleeper.wait();
    drop(lock_file);
    eprintln!("push check {took:?}");
}

#[test]
fn the_person_at_a_terminal_is_not_refused() {
    let seat = Seat::new();
    let ljos = env!("CARGO_BIN_EXE_ljos");
    for line in ["git commit -m x", "git push origin develop"] {
        let typed = terminal::shape("terminal", ljos, &["policy", line]);
        let out = seat.command(terminal::PYTHON, &typed).output().unwrap();
        assert!(out.status.success(), "{line}\n{}", text_of(&out));
        not_refused(&text_of(&out), line);
    }
}
