//! ljos-bslq: the seat replay's order on one checkout. A Write hook claims
//! for the edit, then `ljos_start` over MCP asks for work with no issue.
//! The start keeps the hook's claim. A runner starts its hook command
//! through `sh -c`, and that shell exits with the hook, so the hook's
//! parent is gone before the server's first call.

ljos_test_boot::harness!();
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const SEAT: &str = "free-claude";

struct Habitat {
    _dir: tempfile::TempDir,
    tracker: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    run: PathBuf,
    home: PathBuf,
}

/// git with nothing from the test's environment, so a `GIT_DIR` or
/// `GIT_AUTHOR_*` the caller exported reaches neither checkout.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A tracker that stamps the claim with `VISSUE_AGENT`, as vissue does,
/// so the start has to match the hook's holder to the server's.
const VISSUE: &str = r#"#!/usr/bin/env python3
import os, sys, pathlib, re
root = pathlib.Path(os.environ["VISSUE_ROOT"])
args = sys.argv[1:]
with (root / "vissue.log").open("a") as fh:
    fh.write(" ".join(args) + "\n")
cmd, rest = args[0], args[1:]
board = root / "Software" / "probe" / "issues.org"

def heading(iid):
    return re.compile(r"\* [A-Z]+ (\[#[A-Z]\] [^\n]*)\n:PROPERTIES:\n:ID:\s+" + re.escape(iid) + r"\n(?::CLAIMED_BY:[^\n]*\n)?")

def rewrite(iid, state, holder=None):
    text = board.read_text()
    def put(m):
        out = f"* {state} {m.group(1)}\n:PROPERTIES:\n:ID:         {iid}\n"
        if holder:
            out += f":CLAIMED_BY: {holder}\n"
        return out
    new, n = heading(iid).subn(put, text, count=1)
    if not n:
        sys.exit("no heading for " + iid)
    board.write_text(new)

if cmd == "create":
    proj, title, i = "probe", "work", 0
    while i < len(rest):
        if rest[i] == "-p":
            proj = rest[i + 1]
            i += 2
            continue
        if rest[i] != "-q":
            title = rest[i]
        i += 1
    seq = root / ".seq"
    n = int(seq.read_text()) + 1 if seq.exists() else 1
    seq.write_text(str(n))
    iid = f"{proj}-{n:04x}"
    board.parent.mkdir(parents=True, exist_ok=True)
    with board.open("a") as fh:
        fh.write(f"* TODO [#C] {title}\n:PROPERTIES:\n:ID:         {iid}\n:END:\n")
    print(iid)
elif cmd == "claim":
    rewrite(rest[0], "STARTED", os.environ.get("VISSUE_AGENT", "nobody@host"))
elif cmd == "update":
    rewrite(rest[0], rest[rest.index("-s") + 1])
elif cmd in ("note", "deed", "release"):
    pass
elif cmd == "claims":
    print("[]")
elif cmd == "show":
    print("{}")
else:
    sys.exit("unknown " + cmd)
"#;

fn habitat() -> Habitat {
    let dir = tempfile::tempdir().unwrap();
    let tracker = dir.path().join("tracker");
    let repo = dir.path().join("repo");
    let bin = dir.path().join("bin");
    let run = dir.path().join("run");
    let home = dir.path().join("home");
    for path in [&tracker, &repo, &bin, &run, &home] {
        std::fs::create_dir_all(path).unwrap();
    }
    for root in [&tracker, &repo] {
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "seat@example.invalid"]);
        git(root, &["config", "user.name", "seat"]);
        git(root, &["config", "core.hooksPath", "/dev/null"]);
        git(root, &["config", "commit.gpgsign", "false"]);
    }
    std::fs::write(repo.join("README.md"), "seed\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-q", "-m", "seed"]);
    std::fs::write(
        tracker.join("vissue.toml"),
        format!("seats = [\"{SEAT}\"]\n"),
    )
    .unwrap();
    script(&bin, "vissue", VISSUE);
    Habitat {
        _dir: dir,
        tracker,
        repo,
        bin,
        run,
        home,
    }
}

/// The seat's environment, the same for the hook and the server: the seat
/// named, and no conversation id, as the replay ran them.
fn seat_env(cmd: &mut Command, hab: &Habitat) {
    cmd.env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", hab.bin.display()))
        .env("HOME", &hab.home)
        .env("USER", SEAT)
        .env("XDG_CONFIG_HOME", hab.home.join(".config"))
        .env("XDG_STATE_HOME", hab.home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &hab.run)
        .env("VISSUE_ROOT", &hab.tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_TRACKER_GIT", "commit")
        .env("LJOS_PROJECT", "probe")
        .env("LJOS_SEAT", SEAT)
        .env("LJOS_TRACK_WORK", "on")
        .env("PACKSET_URL", "off")
        .current_dir(&hab.repo);
}

/// `ljos hook` under `sh -c`, as a runner starts it. The command after it
/// keeps the shell from exec'ing the hook, so the hook's parent is a shell
/// that exits as soon as the hook does.
fn hook_through_a_shell(hab: &Habitat, payload: &str) {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "\"$0\" hook; exit $?", env!("CARGO_BIN_EXE_ljos")]);
    seat_env(&mut cmd, hab);
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
    assert!(
        out.status.success(),
        "ljos hook: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    drain(hab);
}

/// Apply the book the hook queued, so the test reads the book after it.
fn drain(hab: &Habitat) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(["hook", "--drain"]);
    seat_env(&mut cmd, hab);
    assert!(cmd.status().unwrap().success(), "ljos hook --drain");
}

/// A process named `name` that runs `body` and then stays up until its
/// stdin closes, the way a terminal or a multiplexer stays up after the
/// command typed in it. The kernel names a script's process after the
/// script, cut to fifteen bytes. `$PAYLOAD` is the hook's input.
fn long_lived(hab: &Habitat, name: &str, body: &str, payload: &str) -> Child {
    let procs = hab.home.with_file_name("procs");
    std::fs::create_dir_all(&procs).unwrap();
    std::fs::write(procs.join("payload.json"), payload).unwrap();
    script(&procs, name, &format!("#!/bin/sh\n{body}\nread _\n"));
    let mut cmd = Command::new(procs.join(name));
    seat_env(&mut cmd, hab);
    cmd.env("LJOS_BIN", env!("CARGO_BIN_EXE_ljos"))
        .env("PAYLOAD", procs.join("payload.json"))
        .env("DONE", procs.join("done"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Another test's fork holds the script's write descriptor until its
    // exec, and until then the kernel refuses to run the script.
    for _ in 0..100 {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(20));
            }
            spawned => return spawned.unwrap(),
        }
    }
    cmd.spawn().unwrap()
}

/// The hook through `sh -c`, as [`hook_through_a_shell`] runs it, from a
/// script; `$DONE` holds its exit status once the shell has exited.
const HOOK_IN_A_SHELL: &str = r#"/bin/sh -c '"$0" hook < "$1"; exit $?' "$LJOS_BIN" "$PAYLOAD""#;

/// Wait for the hook a [`long_lived`] process ran to exit. The shell
/// creates `$DONE` before it writes the status, so an empty file is a
/// hook not yet done.
fn hook_done(hab: &Habitat) {
    let done = hab.home.with_file_name("procs").join("done");
    for _ in 0..600 {
        let status = std::fs::read_to_string(&done).unwrap_or_default();
        if !status.trim().is_empty() {
            assert_eq!(status.trim(), "0", "ljos hook failed");
            drain(hab);
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the hook did not finish");
}

fn stop(mut child: Child) {
    drop(child.stdin.take());
    let _ = child.wait();
}

/// The pid the hook's book records as the runner that owns its sitting.
fn book_owner(hab: &Habitat) -> u32 {
    let books: Vec<serde_json::Value> = std::fs::read_dir(hab.run.join("ljos"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("book-"))
        .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .filter(|b: &serde_json::Value| b["repos"].as_object().is_some_and(|r| !r.is_empty()))
        .collect();
    assert_eq!(books.len(), 1, "{books:?}");
    u32::try_from(books[0]["owner_pid"].as_u64().unwrap()).unwrap()
}

/// One `ljos_start` call to a fresh `ljos-mcp`, and the text it answered.
fn mcp_start(hab: &Habitat, task: &str) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
    seat_env(&mut cmd, hab);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut stdin = child.stdin.take().unwrap();
    for line in [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "claude-code", "version": "2"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "ljos_start", "arguments": {"task": task, "repo": hab.repo}}}),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    stdin.flush().unwrap();
    let mut answer = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line.unwrap()) else {
            continue;
        };
        if value["id"] == 2 {
            answer = Some(value);
            break;
        }
    }
    drop(stdin);
    let _ = child.wait();
    let answer = answer.expect("ljos-mcp answered the start");
    assert_ne!(answer["result"]["isError"], true, "{answer}");
    answer["result"]["structuredContent"]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn state_of(hab: &Habitat, issue: &str) -> String {
    let org = std::fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    let at = org.find(&format!(":ID:         {issue}\n")).unwrap();
    let head = org[..at].rfind("* ").unwrap();
    org[head + 2..]
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn holder_of(hab: &Habitat, issue: &str) -> String {
    let org = std::fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    let at = org.find(&format!(":ID:         {issue}\n")).unwrap();
    org[at..]
        .lines()
        .nth(1)
        .and_then(|line| line.strip_prefix(":CLAIMED_BY:"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn creates(hab: &Habitat) -> usize {
    std::fs::read_to_string(hab.tracker.join("vissue.log"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("create "))
        .count()
}

/// The Write of `slug.py`, and the hook payload a runner sends after it.
fn write_slug(hab: &Habitat) -> String {
    std::fs::write(hab.repo.join("slug.py"), "import re\n").unwrap();
    serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": "6b3c9a2e-1f0d-4c55-9a51-2f6f3c1e8d70",
        "cwd": hab.repo,
        "tool_name": "Write",
        "tool_input": {"file_path": hab.repo.join("slug.py"), "content": "import re\n"},
    })
    .to_string()
}

#[test]
fn ljos_start_after_a_write_keeps_the_hooks_claim() {
    let hab = habitat();
    hook_through_a_shell(&hab, &write_slug(&hab));
    assert_eq!(creates(&hab), 1, "the write filed its claim");
    let held = "probe-0001";
    assert_eq!(state_of(&hab, held), "STARTED");
    let hook_holder = holder_of(&hab, held);
    assert!(
        hook_holder.starts_with(&format!("{SEAT}@")),
        "the hook holds under the seat and a tag: {hook_holder}"
    );
    let said = mcp_start(&hab, "add a slug helper");
    let log = std::fs::read_to_string(hab.run.join("ljos/book.log")).unwrap_or_default();
    assert_eq!(
        said.lines().next(),
        Some(format!("claimed {held}").as_str()),
        "{said}\nbook.log:\n{log}"
    );
    assert_eq!(creates(&hab), 1, "ljos_start filed a second issue");
    assert_eq!(
        state_of(&hab, held),
        "STARTED",
        "the hook's claim was closed"
    );
    assert!(
        log.contains(&format!("start: kept {held}")),
        "book.log:\n{log}"
    );
}

/// A terminal or a multiplexer that ran the hook's shell outlives every
/// sitting under it, so it does not own the hook's. Here nothing but that
/// shell ran the hook, the shell exits with it, and the next call finds
/// the sitting orphaned. `gnome-terminal-server` runs as `gnome-terminal-`.
#[test]
fn a_session_above_the_hooks_shell_does_not_own_its_sitting() {
    for name in ["tmux", "gnome-terminal-server"] {
        let hab = habitat();
        let body = format!("{HOOK_IN_A_SHELL}\necho $? > \"$DONE\"");
        let session = long_lived(&hab, name, &body, &write_slug(&hab));
        hook_done(&hab);
        assert_eq!(state_of(&hab, "probe-0001"), "STARTED", "{name}");
        assert_ne!(book_owner(&hab), session.id(), "{name} owns the sitting");
        let said = mcp_start(&hab, "add a slug helper");
        let log = std::fs::read_to_string(hab.run.join("ljos/book.log")).unwrap_or_default();
        let context = format!("{name}: {said}\nbook.log:\n{log}");
        assert_eq!(state_of(&hab, "probe-0001"), "DONE", "{context}");
        assert_eq!(said.lines().next(), Some("claimed probe-0002"), "{context}");
        stop(session);
    }
}

/// A `uv`, `npx` or `node` that only starts the hook's shell and exits
/// with it does not own the sitting while a harness above it runs on, so
/// the start keeps the Write's claim.
#[test]
fn a_runtime_that_only_wraps_the_hook_does_not_own_its_sitting() {
    for runtime in ["uv", "npx", "node"] {
        let hab = habitat();
        let procs = hab.home.with_file_name("procs");
        std::fs::create_dir_all(&procs).unwrap();
        script(
            &procs,
            runtime,
            &format!("#!/bin/sh\n{HOOK_IN_A_SHELL}\nexit $?\n"),
        );
        let body = format!("\"$(dirname \"$0\")/{runtime}\"\necho $? > \"$DONE\"");
        let harness = long_lived(&hab, "claude", &body, &write_slug(&hab));
        hook_done(&hab);
        assert_eq!(book_owner(&hab), harness.id(), "{runtime} owns the sitting");
        let said = mcp_start(&hab, "add a slug helper");
        let log = std::fs::read_to_string(hab.run.join("ljos/book.log")).unwrap_or_default();
        let context = format!("{runtime}: {said}\nbook.log:\n{log}");
        assert_eq!(said.lines().next(), Some("claimed probe-0001"), "{context}");
        assert_eq!(creates(&hab), 1, "{context}");
        assert_eq!(state_of(&hab, "probe-0001"), "STARTED", "{context}");
        stop(harness);
    }
}
