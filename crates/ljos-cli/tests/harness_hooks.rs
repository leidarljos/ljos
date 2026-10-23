//! The five real-harness fixes, each driven through `ljos hook` (the ballot
//! case through `ljos vote`, which is the process that casts and tallies).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[path = "common/seat_root.rs"]
mod seat_root;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn record_evidence(repo: &Path) {
    std::fs::create_dir_all(repo.join(".ljos")).unwrap();
    std::fs::write(repo.join(".ljos/gates.toml"), "test = \"true\"\n").unwrap();
    let sha = head(repo);
    let dir = repo.join(".git/ljos-evidence");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{sha}.json")),
        format!(r#"{{"command":"true","exit":0,"sha":"{sha}","ts":"2026-01-01T00:00:00Z"}}"#),
    )
    .unwrap();
}

fn head(dir: &Path) -> String {
    let out = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

struct Habitat {
    root: PathBuf,
    tracker: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    run: PathBuf,
    home: PathBuf,
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn habitat() -> (Scratch, Habitat) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp)
        .join(format!("ljos-harness-hooks-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let dir = Scratch(dir);
    let tracker = dir.0.join("tracker");
    let repo = dir.0.join("repo");
    let bin = dir.0.join("bin");
    let run = dir.0.join("run");
    let home = dir.0.join("home");
    for path in [&tracker, &repo, &bin, &run, &home] {
        std::fs::create_dir_all(path).unwrap();
    }
    for root in [&tracker, &repo] {
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "seat@example.invalid"]);
        git(root, &["config", "user.name", "seat"]);
        git(root, &["config", "core.hooksPath", "/dev/null"]);
    }
    git(&repo, &["config", "user.name", "Rohit Goswami"]);
    git(&repo, &["config", "user.email", "rgoswami@ieee.org"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/acme/widgets.git",
        ],
    );
    git(&repo, &["branch", "-M", "main"]);
    std::fs::write(repo.join("README.md"), "seed\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-q", "-m", "seed"]);
    script(
        &bin,
        "vissue",
        r#"#!/usr/bin/env python3
import os, sys, pathlib, re
root = pathlib.Path(os.environ["VISSUE_ROOT"])
log = root / "vissue.log"
args = sys.argv[1:]
with log.open("a") as fh:
    fh.write(" ".join(args) + "\n")
cmd, rest = args[0], args[1:]

def files():
    return list(root.rglob("issues.org"))

def rewrite(iid, state):
    pat = re.compile(r"(\* )[A-Z]+( \[#[A-Z]\] [^\n]*\n:PROPERTIES:\n:ID:\s+" + re.escape(iid) + r"\n)")
    for f in files():
        text = f.read_text()
        new, n = pat.subn(r"\1" + state + r"\2", text, count=1)
        if n:
            f.write_text(new)
            return
    sys.exit("no heading for " + iid)

if cmd == "create":
    proj, title, i = "agent", "agent work", 0
    while i < len(rest):
        if rest[i] == "-p" and i + 1 < len(rest):
            proj = rest[i + 1]
            i += 2
            continue
        if rest[i] == "-q":
            i += 1
            continue
        title = rest[i]
        i += 1
    seq = root / ".seq"
    n = int(seq.read_text()) + 1 if seq.exists() else 1
    seq.write_text(str(n))
    iid = f"{proj}-{n:04x}"
    dest = root / "Software" / proj
    dest.mkdir(parents=True, exist_ok=True)
    with (dest / "issues.org").open("a") as fh:
        fh.write(f"* TODO [#C] {title}\n:PROPERTIES:\n:ID:         {iid}\n:END:\n")
    print(iid)
elif cmd == "claim":
    rewrite(rest[0], "STARTED")
    needle = f":ID:         {rest[0]}\n"
    holder = os.environ.get("VISSUE_AGENT", "")
    for f in files():
        text = f.read_text()
        if holder and needle in text:
            f.write_text(text.replace(needle, needle + f":CLAIMED_BY: {holder}\n", 1))
            break
elif cmd == "update":
    state = rest[rest.index("-s") + 1]
    rewrite(rest[0], state)
elif cmd == "deed":
    iid, acc = rest[0], rest[rest.index("--add") + 1]
    needle = f":ID:         {iid}\n"
    for f in files():
        text = f.read_text()
        at = text.find(needle)
        if at >= 0:
            end = text.index(":END:\n", at) + len(":END:\n")
            f.write_text(text[:end] + f"cited {acc}\n" + text[end:])
            sys.exit(0)
    sys.exit("no issue " + iid)
elif cmd == "claims":
    print("[]")
elif cmd == "show":
    print("{}")
else:
    sys.exit("unknown " + cmd)
"#,
    );
    script(
        &bin,
        "deedar",
        r#"#!/usr/bin/env python3
import os, pathlib, sys
root = pathlib.Path(os.environ["VISSUE_ROOT"])
args = sys.argv[1:]
(root / "deedar.log").open("a").write(" ".join(args) + "\n")
ident = None
i = 0
while i < len(args):
    if args[i] == "--id" and i + 1 < len(args):
        ident = args[i + 1]
    if args[i] == "--path" and i + 1 < len(args):
        (root / "deedar-body.log").open("a").write(pathlib.Path(args[i + 1]).read_text())
    i += 1
if not ident:
    sys.exit("deedar was not given --id")
print(f"id={ident}")
"#,
    );
    let hab = Habitat {
        root: dir.0.clone(),
        tracker,
        repo,
        bin,
        run,
        home,
    };
    (dir, hab)
}

fn ljos(hab: &Habitat, payload: &str, extra: &[(&str, &str)]) -> std::process::Output {
    ljos_cmd(hab, &["hook"], payload, extra)
}

/// A sitting for the habitat's holder: an open ticket the tracker has
/// claimed by `book-holder`, and a hold whose conversation is this test,
/// the parent of every `ljos hook` it runs.
fn sit(hab: &Habitat) {
    let project = hab.tracker.join("Software/probe");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(project.join("issues.org"))
        .unwrap()
        .write_all(
            b"* STARTED [#C] probe work\n:PROPERTIES:\n:ID:         probe-0001\n:CLAIMED_BY: book-holder\n:END:\n",
        )
        .unwrap();
    let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
    std::fs::create_dir_all(hab.run.join("ljos")).unwrap();
    std::fs::write(
        hab.run.join("ljos/hold-probe-sitting"),
        format!(
            "book-holder\nprobe-seat\n{}\n{}\n2026-01-01T00:00:00Z\nprobe-0001\n-\n",
            std::process::id(),
            comm.trim()
        ),
    )
    .unwrap();
}

/// `ljos ARGS` in the habitat's repo. The command must succeed.
fn ljos_cmd(
    hab: &Habitat,
    args: &[&str],
    payload: &str,
    extra: &[(&str, &str)],
) -> std::process::Output {
    let out = run_ljos(hab, args, payload, &hab.repo, extra);
    assert!(
        out.status.success(),
        "ljos hook failed\nstdout:\n{}\nstderr:\n{}\npayload:\n{payload}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn hook_in(
    hab: &Habitat,
    payload: &str,
    dir: &Path,
    fail_on_deny: bool,
    extra: &[(&str, &str)],
) -> std::process::Output {
    let mut args = vec!["hook"];
    if fail_on_deny {
        args.push("--fail-on-deny");
    }
    run_ljos(hab, &args, payload, dir, extra)
}

fn run_ljos(
    hab: &Habitat,
    args: &[&str],
    payload: &str,
    dir: &Path,
    extra: &[(&str, &str)],
) -> std::process::Output {
    let path = format!("{}:/usr/bin:/bin", hab.bin.display());
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .env_clear()
        .env("PATH", path)
        .env("HOME", &hab.home)
        .env("XDG_CONFIG_HOME", hab.home.join(".config"))
        .env("XDG_DATA_HOME", hab.home.join(".local/share"))
        .env("XDG_CACHE_HOME", hab.home.join(".cache"))
        .env("XDG_STATE_HOME", hab.home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &hab.run)
        .env("VISSUE_ROOT", &hab.tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_TRACKER_GIT", "commit")
        .env("LJOS_PROJECT", "probe")
        .env("LJOS_SEAT", "probe-seat")
        .env("LJOS_HOLD", "book-holder")
        .env("PACKSET_URL", "off")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in extra {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn claude(event: &str, session: &str, cwd: &Path, extra: serde_json::Value) -> String {
    let mut body = serde_json::json!({
        "hook_event_name": event,
        "session_id": session,
        "cwd": cwd,
    });
    if let (serde_json::Value::Object(dst), serde_json::Value::Object(src)) = (&mut body, extra) {
        for (k, v) in src {
            dst.insert(k, v);
        }
    }
    body.to_string()
}

fn commit_file(repo: &Path, name: &str) -> String {
    std::fs::write(repo.join(name), "x\n").unwrap();
    git(repo, &["add", name]);
    git(repo, &["commit", "-q", "-m", name]);
    head(repo)
}

#[test]
fn the_hook_cites_a_commit_only_after_it_exits_zero() {
    let (_dir, hab) = habitat();
    std::fs::create_dir_all(hab.run.join("ljos")).unwrap();
    std::fs::write(
        hab.run.join("ljos/hold-book-holder"),
        "book-holder\nprobe-seat\n999999\nbash\n2026-01-01T00:00:00Z\nother-9999\n-\n",
    )
    .unwrap();
    let session = "deed-session";
    let cwd = hab.repo.clone();
    ljos(
        &hab,
        &claude(
            "UserPromptSubmit",
            session,
            &cwd,
            serde_json::json!({"prompt": "Add a readme line"}),
        ),
        &[],
    );
    ljos(
        &hab,
        &claude(
            "PreToolUse",
            session,
            &cwd,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "README.md"}}),
        ),
        &[],
    );
    let claimed = std::fs::read_to_string(hab.tracker.join("vissue.log")).unwrap();
    assert!(claimed.contains("create"), "{claimed}");
    assert!(!claimed.contains("other-9999"), "{claimed}");
    ljos(
        &hab,
        &claude(
            "PreToolUse",
            session,
            &cwd,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git commit -m one"}}),
        ),
        &[],
    );
    assert!(
        !hab.tracker.join("deedar.log").exists(),
        "the pre-hook cited a deed before the commit ran"
    );
    let first = commit_file(&hab.repo, "one.txt");
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "git commit -m one"},
                "tool_response": {"exit_code": 0}
            }),
        ),
        &[],
    );
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "git commit -m boom"},
                "tool_response": {"exit_code": 1}
            }),
        ),
        &[],
    );
    let after_fail = std::fs::read_to_string(hab.tracker.join("deedar.log")).unwrap();
    assert_eq!(after_fail.lines().count(), 1, "{after_fail}");
    let second = commit_file(&hab.repo, "two.txt");
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "git commit -m two"}
            }),
        ),
        &[],
    );
    let deeds = std::fs::read_to_string(hab.tracker.join("deedar.log")).unwrap();
    assert!(
        deeds.contains(&format!("--id deed-file-commit-{first}")),
        "{deeds}"
    );
    assert!(
        deeds.contains(&format!("--id deed-file-commit-{second}")),
        "{deeds}"
    );
    assert_eq!(deeds.lines().count(), 2, "{deeds}");
    let body = std::fs::read_to_string(hab.tracker.join("deedar-body.log")).unwrap();
    assert!(body.contains(&first) && body.contains(&second), "{body}");
    assert!(!body.contains("git commit"), "{body}");
    let org = std::fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(
        org.contains(&format!("cited deed-file-commit-{first}")),
        "{org}"
    );
    assert!(
        org.contains(&format!("cited deed-file-commit-{second}")),
        "{org}"
    );
    assert!(!org.contains("other-9999"), "{org}");
}

#[test]
fn the_hook_claims_on_the_first_write_and_on_a_redirect() {
    let (_dir, hab) = habitat();
    let cwd = hab.repo.clone();
    let tools = [
        (
            "Write",
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "README.md"}}),
        ),
        (
            "Edit",
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "README.md"}}),
        ),
        (
            "MultiEdit",
            serde_json::json!({"tool_name": "MultiEdit", "tool_input": {"file_path": "README.md"}}),
        ),
        (
            "apply_patch",
            serde_json::json!({"tool_name": "apply_patch", "tool_input": {"file_path": "README.md"}}),
        ),
        (
            "redirect",
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "echo hi > README.md"}}),
        ),
    ];
    for (name, extra) in tools {
        ljos(
            &hab,
            &claude("PreToolUse", &format!("claim-{name}"), &cwd, extra),
            &[],
        );
    }
    ljos(
        &hab,
        &claude(
            "PreToolUse",
            "claim-new-file",
            &cwd,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "echo hi > brand-new.txt"}}),
        ),
        &[],
    );
    let log = std::fs::read_to_string(hab.tracker.join("vissue.log")).unwrap();
    let creates = log.matches("create").count();
    // Write, Edit, MultiEdit, apply_patch, a tracked redirect, and a
    // redirect that creates a file git does not track yet.
    assert_eq!(creates, 6, "{log}");
}

#[test]
fn a_hook_books_nothing_for_a_seat_nobody_named_or_the_seats_file_omits() {
    let (_dir, hab) = habitat();
    let write = |session: &str, extra: &[(&str, &str)]| {
        let payload = claude(
            "PreToolUse",
            session,
            &hab.repo,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "README.md"}}),
        );
        let out = hook_in(&hab, &payload, &hab.repo, false, extra);
        assert!(out.status.success(), "{out:?}");
    };
    let log = || std::fs::read_to_string(hab.tracker.join("vissue.log")).unwrap_or_default();
    // An empty LJOS_SEAT is no seat. The session id puts the shell under a
    // runner, so the seat left is derived whatever process ran this test.
    write(
        "unseated",
        &[
            ("LJOS_SEAT", ""),
            ("LJOS_HOLD", ""),
            ("GROK_SESSION_ID", "2c12969d-0000-4000-8000-000000000000"),
        ],
    );
    assert!(!log().contains("create"), "{}", log());

    let seats = hab.home.join(".config/ljos/seats.toml");
    std::fs::create_dir_all(seats.parent().unwrap()).unwrap();
    std::fs::write(&seats, "seats = [\"saddle\"]\n").unwrap();
    write("unlisted", &[]);
    assert!(!log().contains("create"), "{}", log());

    std::fs::write(&seats, "seats = [\"probe-seat\"]\n").unwrap();
    write("listed", &[]);
    assert!(log().contains("create"), "a listed seat claims: {}", log());
}

fn context(stdout: &[u8]) -> String {
    let text = String::from_utf8_lossy(stdout);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return text.into_owned();
    };
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

#[test]
fn stop_writes_a_lesson_and_the_next_sessions_first_prompt_recalls_it() {
    let (_dir, hab) = habitat();
    let store = hab.root.join("atoms.json");
    let pack = hab.root.join("pack.py");
    std::fs::write(
        &pack,
        r#"#!/usr/bin/env python3
import json, os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
store = os.environ["ATOM_STORE"]
atoms = []
class H(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/v1/status":
            body = {"embedder": {"available": True, "answering": None}}
        elif path == "/v1/atoms":
            body = {"atoms": atoms}
        else:
            body = {"hits": []}
        self._send(body)
    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(n)
        try:
            atom = json.loads(raw)
        except Exception:
            atom = {}
        if isinstance(atom, dict) and atom.get("text"):
            atom.setdefault("id", "atom-%d" % (len(atoms) + 1))
            atoms.append(atom)
            open(store, "w").write(json.dumps(atoms))
        self._send(atom if isinstance(atom, dict) else {})
    def _send(self, body):
        raw = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)
server = ThreadingHTTPServer(("127.0.0.1", 0), H)
print(server.server_address[1], flush=True)
server.serve_forever()
"#,
    )
    .unwrap();
    let mut pack_proc = Command::new("python3")
        .arg(&pack)
        .env("ATOM_STORE", &store)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut port = String::new();
    let mut stdout = pack_proc.stdout.take().unwrap();
    let mut byte = [0u8; 1];
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if stdout.read(&mut byte).unwrap() == 0 {
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        port.push(byte[0] as char);
    }
    let port: u16 = port.trim().parse().expect("pack port");
    let url = format!("http://127.0.0.1:{port}");
    let cwd = hab.repo.clone();
    let first = ljos(
        &hab,
        &claude(
            "UserPromptSubmit",
            "memory-1",
            &cwd,
            serde_json::json!({"prompt": "Add a readme line"}),
        ),
        &[("PACKSET_URL", url.as_str())],
    );
    let warming = context(&first.stdout);
    assert!(
        warming.contains("pack is loading its search model"),
        "{warming}"
    );
    assert!(!warming.contains("Remembered, oldest first"), "{warming}");
    ljos(
        &hab,
        &claude(
            "PreToolUse",
            "memory-1",
            &cwd,
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "README.md"}}),
        ),
        &[("PACKSET_URL", url.as_str())],
    );
    ljos(
        &hab,
        &claude(
            "PostToolUseFailure",
            "memory-1",
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "cargo test"},
                "tool_response": {"error": "readme test failed"}
            }),
        ),
        &[("PACKSET_URL", url.as_str())],
    );
    record_evidence(&hab.repo);
    sit(&hab);
    let asked = ljos(
        &hab,
        &claude(
            "PreToolUse",
            "memory-1",
            &cwd,
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "git push --force origin feature"}}),
        ),
        &[("PACKSET_URL", url.as_str())],
    );
    let asked = String::from_utf8_lossy(&asked.stdout);
    assert!(asked.contains("\"permissionDecision\":\"ask\""), "{asked}");
    ljos(
        &hab,
        &claude("Stop", "memory-1", &cwd, serde_json::json!({})),
        &[("PACKSET_URL", url.as_str())],
    );
    let saved = std::fs::read_to_string(&store).unwrap_or_default();
    assert!(saved.contains("agent-derived"), "{saved}");
    assert!(saved.contains("Add a readme line"), "{saved}");
    assert!(saved.contains("horizon:standing"), "{saved}");
    assert!(
        saved.contains("probe-seat") || saved.contains("\"seat\""),
        "{saved}"
    );
    let start_line = ljos(
        &hab,
        &claude("SessionStart", "memory-2", &cwd, serde_json::json!({})),
        &[("PACKSET_URL", url.as_str())],
    );
    let start_line = context(&start_line.stdout);
    assert!(!start_line.contains("no memories here yet"), "{start_line}");
    assert!(start_line.contains("memor"), "{start_line}");
    let recalled = ljos(
        &hab,
        &claude(
            "UserPromptSubmit",
            "memory-2",
            &cwd,
            serde_json::json!({"prompt": "Add a readme line to the notes"}),
        ),
        &[("PACKSET_URL", url.as_str())],
    );
    let recalled = context(&recalled.stdout);
    assert!(recalled.contains("Remembered, oldest first"), "{recalled}");
    assert!(recalled.contains("readme"), "{recalled}");
    let _ = pack_proc.kill();
    let _ = pack_proc.wait();
}

#[test]
fn an_ask_is_rendered_for_each_harness() {
    let (_dir, hab) = habitat();
    record_evidence(&hab.repo);
    sit(&hab);
    let policyd = hab.bin.join("ljos-policyd");
    script(
        &hab.bin,
        "ljos-policyd",
        "#!/bin/sh\nprintf 'deny\\tgit-force-push\\n'\nexit 2\n",
    );
    let cwd = hab.repo.clone();
    let env = [("POLICYD_BIN", policyd.to_str().unwrap())];
    let command = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "git push --force origin feature"}
    });
    // Claude Code and opencode stop on permissionDecision ask. opencode's
    // plugin throws before the tool runs; the payload it sends has no
    // turn_id, so the seat answers in the ask shape.
    for name in ["claude", "opencode"] {
        let out = ljos(
            &hab,
            &claude(
                "PreToolUse",
                &format!("force-{name}"),
                &cwd,
                command.clone(),
            ),
            &env,
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("\"permissionDecision\":\"ask\""),
            "{name}: {text}"
        );
        assert!(text.contains("git-force-push"), "{name}: {text}");
        assert!(
            !text.contains("\"permissionDecision\":\"deny\""),
            "{name}: {text}"
        );
    }
    let plugin = include_str!("../assets/opencode/ljos.ts");
    let plugin_v2 = include_str!("../assets/opencode/ljos-v2.ts");
    for source in [plugin, plugin_v2] {
        assert!(source.contains("verdict === \"ask\""));
        assert!(source.contains("needs the person's approval"));
    }
    // Codex PreToolUse does not honour ask: the hook is marked failed and
    // the push runs. The seat denies, and the reason says how to grant it.
    let mut codex_body = command.clone();
    codex_body["turn_id"] = serde_json::json!("turn-1");
    let codex = ljos(
        &hab,
        &claude("PreToolUse", "force-codex", &cwd, codex_body),
        &env,
    );
    let codex = String::from_utf8_lossy(&codex.stdout);
    assert!(codex.contains("\"permissionDecision\":\"deny\""), "{codex}");
    assert!(!codex.contains("\"permissionDecision\":\"ask\""), "{codex}");
    assert!(codex.contains("ljos approve "), "{codex}");
    assert!(codex.contains("ljos_request_approval"), "{codex}");
    assert!(codex.contains("git-force-push"), "{codex}");
    assert!(!codex.contains("leave it for them"), "{codex}");
    // agy asks on decision ask.
    let agy = ljos_cmd(
        &hab,
        &["hook", "--event", "PreToolUse"],
        &serde_json::json!({
            "conversationId": "force-agy",
            "cwd": cwd,
            "toolCall": {
                "name": "run_command",
                "args": {"CommandLine": "git push --force origin feature"}
            }
        })
        .to_string(),
        &env,
    );
    let agy = String::from_utf8_lossy(&agy.stdout);
    assert!(agy.contains("\"decision\":\"ask\""), "{agy}");
    assert!(!agy.contains("\"decision\":\"deny\""), "{agy}");
    assert!(agy.contains("git-force-push"), "{agy}");
    // Cursor beforeShellExecution enforces permission ask.
    let shell = ljos(
        &hab,
        &serde_json::json!({
            "cursor_version": "2.4.0",
            "conversation_id": "force-cursor-shell",
            "hook_event_name": "beforeShellExecution",
            "command": "git push --force origin feature",
            "cwd": cwd,
        })
        .to_string(),
        &env,
    );
    let shell = String::from_utf8_lossy(&shell.stdout);
    assert!(shell.contains("\"permission\":\"ask\""), "{shell}");
    assert!(!shell.contains("\"permission\":\"deny\""), "{shell}");
    assert!(shell.contains("git-force-push"), "{shell}");
    // Cursor preToolUse accepts ask and does not enforce it.
    let tool = ljos(
        &hab,
        &serde_json::json!({
            "cursor_version": "2.4.0",
            "conversation_id": "force-cursor-tool",
            "hook_event_name": "preToolUse",
            "tool_name": "Shell",
            "tool_input": {"command": "git push --force origin feature"},
            "cwd": cwd,
        })
        .to_string(),
        &env,
    );
    let tool = String::from_utf8_lossy(&tool.stdout);
    assert!(tool.contains("\"permission\":\"deny\""), "{tool}");
    assert!(!tool.contains("\"permission\":\"ask\""), "{tool}");
    assert!(tool.contains("ljos approve "), "{tool}");
    assert!(tool.contains("ljos_request_approval"), "{tool}");
    assert!(!tool.contains("leave it for them"), "{tool}");
    // Gemini honours decision deny (and, in current source, ask). The
    // seat sends deny, including when the event is still BeforeTool.
    let gemini = ljos_cmd(
        &hab,
        &["hook", "--runner", "gemini"],
        &serde_json::json!({
            "hook_event_name": "BeforeTool",
            "session_id": "force-gemini",
            "cwd": cwd,
            "tool_name": "run_shell_command",
            "tool_input": {"command": "git push --force origin feature"}
        })
        .to_string(),
        &env,
    );
    let gemini = String::from_utf8_lossy(&gemini.stdout);
    assert!(gemini.contains("\"decision\":\"deny\""), "{gemini}");
    assert!(!gemini.contains("\"ask\""), "{gemini}");
    assert!(gemini.contains("git-force-push"), "{gemini}");
    assert!(gemini.contains("cannot ask"), "{gemini}");
}

#[test]
fn persona_ballots_from_one_holder_count_once() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let home = dir.path().join("home");
    let state = dir.path().join("state");
    let root = dir.path().join("tracker");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "seat@example.invalid"]);
    git(&root, &["config", "user.name", "seat"]);
    let state_s = state.display().to_string();
    script(
        &bin,
        "vissue",
        &format!(
            r#"#!/usr/bin/env python3
import json, os, sys, pathlib
root = pathlib.Path({state_s:?})
root.mkdir(parents=True, exist_ok=True)
ballots = root / "ballots.json"
notes = root / "notes.json"
hide = root / "hide-notes"
def load(path):
    return json.loads(path.read_text()) if path.exists() else {{}}
def save(path, value):
    path.write_text(json.dumps(value))
args = sys.argv[1:]
cmd = args[0] if args else ""
issue = args[1] if len(args) > 1 else ""
if cmd == "vote" and "--json" in args:
    print(json.dumps(load(ballots).get(issue, [])))
elif cmd == "vote" and "--for" in args:
    choice = args[args.index("--for") + 1]
    rows = load(ballots)
    rows.setdefault(issue, []).append({{"agent": os.environ.get("VISSUE_AGENT", ""), "choice": choice}})
    save(ballots, rows)
    print("ok")
elif cmd == "show":
    kept = [] if hide.exists() else load(notes).get(issue, [])
    logbook = [{{"note": note}} for note in reversed(kept)]
    print(json.dumps({{"id": issue, "title": "rig", "logbook": logbook}}))
elif cmd == "note":
    rows = load(notes)
    rows.setdefault(issue, []).append(" ".join(args[2:]))
    save(notes, rows)
    print("noted")
else:
    print("ok")
"#
        ),
    );
    let consensus_log = dir.path().join("consensus.log");
    script(
        &bin,
        "ljos-consensus",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nprintf '%s\\n' '{{\"options\":[\"A\"],\"shares\":[1.0],\"settled\":true}}'\n",
            consensus_log.display()
        ),
    );
    std::fs::write(state.join("hide-notes"), "1").unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let vote = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", dir.path().join("run"))
            .env("VISSUE_ROOT", &root)
            .env("VISSUE_STATE", &state)
            .env("VISSUE_NO_ROUTE", "1")
            .env("VISSUE_AGENT", "sess-1")
            .env("LJOS_SEAT", "sess-1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let personas = ["", "alpha", "beta", "gamma", "delta"];
    for (i, persona) in personas.iter().enumerate() {
        let choice = if i < 3 { "A" } else { "B" };
        let mut args = vec!["vote", "Rig-uw1n", "--for", choice, "--used", "none"];
        if !persona.is_empty() {
            args.push("--as");
            args.push(persona);
        }
        let (ok, stdout, stderr) = vote(&args);
        assert!(ok, "vote {persona}: {stdout}\n{stderr}");
    }
    std::fs::remove_file(state.join("hide-notes")).unwrap();
    let (ok, tally, stderr) = vote(&["vote", "Rig-uw1n"]);
    assert!(ok, "{tally}\n{stderr}");
    assert!(tally.contains("1 ballot counts"), "{tally}");
    assert!(!tally.contains("5 ballots"), "{tally}");
    let (ok, said, stderr) = vote(&["consensus", "Rig-uw1n", "--no-file", "--json"]);
    assert!(ok, "{said}\n{stderr}");
    let logged = std::fs::read_to_string(&consensus_log).unwrap_or_default();
    assert!(logged.contains("--ballots"), "{logged}");
    let ballots = logged
        .split("--ballots")
        .nth(1)
        .unwrap_or("")
        .trim()
        .lines()
        .next()
        .unwrap_or("");
    let rows: Vec<serde_json::Value> = serde_json::from_str(ballots.trim()).unwrap_or_default();
    assert_eq!(rows.len(), 1, "{logged}");
    let (ok, stdout, stderr) = vote(&["vote", "Rig-uw2n", "--for", "A", "--used", "none"]);
    assert!(ok, "{stdout}\n{stderr}");
    let (ok, stdout, stderr) = vote(&[
        "vote", "Rig-uw2n", "--for", "B", "--used", "none", "--as", "alpha",
    ]);
    assert!(!ok, "a second persona was counted\n{stdout}\n{stderr}");
    let refused = format!("{stdout}\n{stderr}");
    assert!(refused.contains("already cast"), "{refused}");
}

fn issue_id(tracker: &Path) -> String {
    let org = std::fs::read_to_string(tracker.join("Software/probe/issues.org")).unwrap();
    org.lines()
        .find_map(|line| line.trim().strip_prefix(":ID:"))
        .expect("an issue id")
        .trim()
        .to_string()
}

#[test]
fn the_session_start_line_names_the_claimed_issue() {
    let (_dir, hab) = habitat();
    let cwd = hab.repo.clone();
    let session = "start-issue";
    ljos(
        &hab,
        &claude(
            "PreToolUse",
            session,
            &cwd,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "README.md"}}),
        ),
        &[],
    );
    let id = issue_id(&hab.tracker);
    let started = ljos(
        &hab,
        &claude("SessionStart", session, &cwd, serde_json::json!({})),
        &[],
    );
    let line = context(&started.stdout);
    assert!(
        line.contains(&format!("This session's issue is {id}.")),
        "{line}"
    );
}

/// willma-20261020f w4, w8 and w12: agy 1.4.3 wrote `slug.py` with
/// `write_file`, the after hook answered `{}`, and no claim was filed until a
/// later `edit_file` or shell command. This is that payload, with no `cwd`
/// field, as agy sends it; the write itself files the claim.
#[test]
fn an_agy_write_file_files_the_claim() {
    let (_dir, hab) = habitat();
    let cwd = hab.repo.clone();
    let file = cwd.join("slug.py");
    std::fs::write(&file, "import re\n").unwrap();
    let payload = serde_json::json!({
        "artifactDirectoryPath": hab.home.join(".gemini/antigravity-cli/brain/agy-write"),
        "conversationId": "agy-write",
        "error": "",
        "modelName": "gemini-3.1-pro-low",
        "stepIdx": 8,
        "toolCall": {
            "name": "write_file",
            "args": {"Content": "import re\n", "Overwrite": false, "Path": file}
        },
        "workspacePaths": [cwd],
    })
    .to_string();
    ljos_cmd(
        &hab,
        &["hook", "--event", "PostToolUse"],
        &payload,
        &[("LJOS_TRACK_WORK", "on")],
    );
    let log = std::fs::read_to_string(hab.tracker.join("vissue.log")).unwrap_or_default();
    assert_eq!(
        log.lines().filter(|l| l.starts_with("create ")).count(),
        1,
        "one issue filed by write_file:\n{log}"
    );
    assert!(
        log.lines().any(|l| l.starts_with("claim ")),
        "no claim after write_file:\n{log}"
    );
}

#[test]
fn an_agy_idle_stop_finishes_and_a_mid_loop_stop_does_not() {
    let (_dir, hab) = habitat();
    let cwd = hab.repo.clone();
    let session = "agy-finish";
    let transcript = hab.root.join("turn.jsonl");
    std::fs::write(
        &transcript,
        "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Add a readme line\"}]}}\n\
         {\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"write_to_file\",\"input\":{\"TargetFile\":\"README.md\"}}]}}\n",
    )
    .unwrap();
    ljos(
        &hab,
        &serde_json::json!({
            "conversationId": session,
            "cwd": cwd,
            "toolCall": {"name": "write_to_file", "args": {"TargetFile": "README.md"}}
        })
        .to_string(),
        &[("LJOS_TRACK_WORK", "on")],
    );
    let claimed = std::fs::read_to_string(hab.tracker.join("vissue.log")).unwrap();
    assert!(claimed.contains("create"), "{claimed}");
    let held = ljos(
        &hab,
        &serde_json::json!({
            "conversationId": session,
            "cwd": cwd,
            "executionNum": 1,
            "transcriptPath": transcript,
        })
        .to_string(),
        &[("LJOS_TRACK_WORK", "on")],
    );
    let held_text = String::from_utf8_lossy(&held.stdout);
    assert!(held_text.contains("continue"), "{held_text}");
    assert!(held_text.contains("holds no issue"), "{held_text}");
    let mid = std::fs::read_to_string(hab.tracker.join("vissue.log")).unwrap();
    assert!(!mid.contains("-s DONE"), "{mid}");
    ljos(
        &hab,
        &serde_json::json!({
            "conversationId": session,
            "cwd": cwd,
            "executionNum": 1,
            "fullyIdle": true,
            "terminationReason": "NO_TOOL_CALL",
            "transcriptPath": transcript,
        })
        .to_string(),
        &[("LJOS_TRACK_WORK", "on")],
    );
    let done = std::fs::read_to_string(hab.tracker.join("vissue.log")).unwrap();
    assert!(done.contains("-s DONE"), "{done}");
}

#[test]
fn a_refused_commit_is_not_cited() {
    let (_dir, hab) = habitat();
    let cwd = hab.repo.clone();
    let session = "refused-commit";
    ljos(
        &hab,
        &claude(
            "PreToolUse",
            session,
            &cwd,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "README.md"}}),
        ),
        &[],
    );
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "git commit -m refused"},
                "permissionDecision": "ask",
                "error": "ljos: needs the person's approval"
            }),
        ),
        &[],
    );
    assert!(
        !hab.tracker.join("deedar.log").exists(),
        "a refused commit cited a deed"
    );
    let sha = commit_file(&hab.repo, "one.txt");
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "git commit -m one"},
                "tool_response": {"exit_code": 0}
            }),
        ),
        &[],
    );
    let deeds = std::fs::read_to_string(hab.tracker.join("deedar.log")).unwrap();
    assert!(deeds.contains(&sha), "{deeds}");
    assert_eq!(deeds.lines().count(), 1, "{deeds}");
}

const NORMAL_GIT: &str = "git add .; git commit -m note; git push origin topic";

fn child_repo(parent: &Path) -> PathBuf {
    let child = parent.join("path/to/git/repo");
    std::fs::create_dir_all(&child).unwrap();
    git(&child, &["init", "-q"]);
    git(&child, &["config", "user.email", "seat@example.invalid"]);
    git(&child, &["config", "user.name", "seat"]);
    git(&child, &["config", "commit.gpgsign", "false"]);
    git(&child, &["config", "core.hooksPath", "/dev/null"]);
    git(&child, &["branch", "-M", "main"]);
    std::fs::write(child.join("README.md"), "child\n").unwrap();
    git(&child, &["add", "README.md"]);
    git(&child, &["commit", "-q", "-m", "seed"]);
    child
}

fn pretool(cwd: &str, workdir: Option<&str>, command: &str) -> String {
    let mut tool_input = serde_json::json!({ "command": command });
    if let Some(workdir) = workdir {
        tool_input["workdir"] = serde_json::json!(workdir);
    }
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "workdir",
        "cwd": cwd,
        "tool_name": "Bash",
        "tool_input": tool_input,
    })
    .to_string()
}

fn assert_ordinary(out: &std::process::Output, payload: &str) {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "exit {}\nstdout:\n{stdout}\nstderr:\n{stderr}\npayload:\n{payload}",
        out.status
    );
    let joined = format!("{stdout}\n{stderr}");
    assert!(
        !joined.contains("tool working directory"),
        "a missing directory is not a seat rule\n{joined}"
    );
    assert!(
        !stdout.contains("\"permissionDecision\":\"deny\"")
            && !stdout.contains("\"permissionDecision\":\"ask\""),
        "{stdout}"
    );
}

/// A relative workdir resolves against the session directory, and a normal
/// git command there is allowed.
#[test]
fn a_relative_workdir_resolves_and_a_normal_git_command_is_allowed() {
    let (_dir, hab) = habitat();
    sit(&hab);
    let child = child_repo(&hab.repo);
    let session = hab.repo.to_str().unwrap();
    let forwarded = pretool(session, Some("path/to/git/repo"), NORMAL_GIT);
    let out = hook_in(&hab, &forwarded, &hab.repo, true, &[]);
    assert_ordinary(&out, &forwarded);
    assert!(child.join(".git").is_dir());
    let as_cwd = pretool("path/to/git/repo", None, NORMAL_GIT);
    let out = hook_in(&hab, &as_cwd, &hab.repo, true, &[]);
    assert_ordinary(&out, &as_cwd);
}

fn book_rows(hab: &Habitat) -> Vec<serde_json::Value> {
    let dir = hab.run.join("ljos");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("book-") && name.ends_with(".json")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(repos) = value.get("repos").and_then(|v| v.as_object()) else {
            continue;
        };
        rows.extend(repos.values().cloned());
    }
    rows
}

fn book_count(hab: &Habitat) -> usize {
    let dir = hab.run.join("ljos");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("book-") && name.ends_with(".json")
        })
        .count()
}

fn sitting<'a>(rows: &'a [serde_json::Value], issue: &str) -> &'a serde_json::Value {
    rows.iter()
        .find(|row| row["issue"].as_str() == Some(issue))
        .unwrap_or_else(|| panic!("no sitting for {issue} in {rows:?}"))
}

fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn claim_session(hab: &Habitat, session: &str, prompt: &str) {
    let cwd = hab.repo.clone();
    ljos(
        hab,
        &claude(
            "UserPromptSubmit",
            session,
            &cwd,
            serde_json::json!({"prompt": prompt}),
        ),
        &[],
    );
    ljos(
        hab,
        &claude(
            "PreToolUse",
            session,
            &cwd,
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "README.md"}}),
        ),
        &[],
    );
}

/// Claude Code's Bash PostToolUse carries stdout and `gitOperation`, and no
/// exit code. A missing code is not a failure, and every commit HEAD moved
/// past the recorded start is a deed, including a compound command.
#[test]
fn a_landed_commit_without_an_exit_code_is_deeded() {
    let (_dir, hab) = habitat();
    let cwd = hab.repo.clone();
    let session = "claude-landed";
    claim_session(&hab, session, "Add a kebab_case helper");
    let seed = head(&hab.repo);
    let rows = book_rows(&hab);
    let issue = rows
        .iter()
        .find_map(|row| row["issue"].as_str().map(str::to_string))
        .expect("the edit claimed");
    let start = sitting(&rows, &issue)["start_sha"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(start, seed);
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "git commit -m nope"},
                "tool_response": "error: nothing to commit, working tree clean\n"
            }),
        ),
        &[],
    );
    let rows = book_rows(&hab);
    let row = sitting(&rows, &issue);
    let failures = row["failures"].as_array().cloned().unwrap_or_default();
    assert!(
        !failures
            .iter()
            .any(|item| item.as_str() == Some("git commit exited non-zero")),
        "a missing exit was booked as a failure: {row}"
    );
    assert!(row["deeds"].as_array().unwrap().is_empty(), "{row}");
    assert_eq!(row["start_sha"].as_str(), Some(start.as_str()), "{row}");
    assert_eq!(row["finished"], false, "{row}");

    let landed = commit_file(&hab.repo, "slug.py");
    let short = short_sha(&landed);
    let stdout = format!(
        "[main {short}] Add kebab_case function and tests\n 3 files changed, 10 insertions(+)\n"
    );
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {
                    "command": "git add slug.py test_kebab.py test_slug.py && git commit -m \"Add kebab_case function and tests\""
                },
                "tool_response": stdout,
                "gitOperation": {"commit": {"sha": short, "kind": "committed"}}
            }),
        ),
        &[],
    );
    let deeds = std::fs::read_to_string(hab.tracker.join("deedar.log")).unwrap_or_default();
    assert!(
        deeds.contains(&format!("--id deed-file-commit-{landed}")),
        "{deeds}"
    );
    let rows = book_rows(&hab);
    let row = sitting(&rows, &issue);
    assert_eq!(row["start_sha"].as_str(), Some(start.as_str()), "{row}");
    assert!(
        !row["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str() == Some("git commit exited non-zero")),
        "{row}"
    );

    let first = commit_file(&hab.repo, "test_kebab.py");
    let second = commit_file(&hab.repo, "test_slug.py");
    let banner = format!("[main {}] test_slug.py\n", short_sha(&second));
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {
                    "command": "git add test_kebab.py; git commit -m kebab; git add test_slug.py; git commit -m slug"
                },
                "tool_response": {"stdout": banner, "stderr": "", "interrupted": false}
            }),
        ),
        &[],
    );
    let deeds = std::fs::read_to_string(hab.tracker.join("deedar.log")).unwrap();
    assert!(
        deeds.contains(&format!("--id deed-file-commit-{first}")),
        "the earlier commit in the compound command was not deeded:\n{deeds}"
    );
    assert!(
        deeds.contains(&format!("--id deed-file-commit-{second}")),
        "{deeds}"
    );

    let nested = commit_file(&hab.repo, "nested.txt");
    let nested_banner = format!("[main {}] nested\n", short_sha(&nested));
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "(git add nested.txt && git commit -m nested)"},
                "tool_response": {"stdout": nested_banner, "stderr": ""}
            }),
        ),
        &[],
    );
    let deeds = std::fs::read_to_string(hab.tracker.join("deedar.log")).unwrap();
    assert!(
        deeds.contains(&format!("--id deed-file-commit-{nested}")),
        "{deeds}"
    );

    let rejected = commit_file(&hab.repo, "rejected.txt");
    let rejected_banner = format!("[main {}] rejected\n", short_sha(&rejected));
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": "git commit -m rejected"},
                "tool_response": {"stdout": rejected_banner, "exit_code": 1},
                "gitOperation": {"commit": {"sha": short_sha(&rejected), "kind": "committed"}}
            }),
        ),
        &[],
    );
    let deeds = std::fs::read_to_string(hab.tracker.join("deedar.log")).unwrap();
    assert!(
        !deeds.contains(&format!("--id deed-file-commit-{rejected}")),
        "an explicit non-zero exit was deeded:\n{deeds}"
    );
    let rows = book_rows(&hab);
    let row = sitting(&rows, &issue);
    assert_eq!(row["start_sha"].as_str(), Some(start.as_str()), "{row}");
    assert_eq!(row["finished"], false, "{row}");
    assert!(
        row["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str() == Some("git commit exited non-zero")),
        "{row}"
    );
}

/// A failed `git commit -m X` stays on the claimed issue. The same command
/// exiting 0 a few seconds later deeds the new commit there, and so does
/// the git hook, instead of opening a book whose start is that sha.
#[test]
fn a_retried_commit_is_deeded_on_the_same_issue() {
    let (_dir, hab) = habitat();
    let cwd = hab.repo.clone();
    let session = "opencode-retry";
    claim_session(&hab, session, "Record the retry");
    let rows = book_rows(&hab);
    let issue = rows
        .iter()
        .find_map(|row| row["issue"].as_str().map(str::to_string))
        .expect("the edit claimed");
    let start = sitting(&rows, &issue)["start_sha"]
        .as_str()
        .unwrap()
        .to_string();
    let command = "git commit -m X";
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": command},
                "tool_response": {"exit_code": 1, "error": "nothing to commit"}
            }),
        ),
        &[],
    );
    let rows = book_rows(&hab);
    let row = sitting(&rows, &issue);
    assert_eq!(row["finished"], false, "{row}");
    assert_eq!(row["start_sha"].as_str(), Some(start.as_str()), "{row}");
    assert_eq!(row["issue"].as_str(), Some(issue.as_str()), "{row}");
    assert!(row["deeds"].as_array().unwrap().is_empty(), "{row}");
    assert!(
        row["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str() == Some("git commit exited non-zero")),
        "{row}"
    );
    assert_eq!(book_count(&hab), 1, "the failure opened another book");

    let sha = commit_file(&hab.repo, "retried.txt");
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": command},
                "tool_response": {"exit_code": 0}
            }),
        ),
        &[],
    );
    let deeds = std::fs::read_to_string(hab.tracker.join("deedar.log")).unwrap_or_default();
    assert!(
        deeds.contains(&format!("--id deed-file-commit-{sha}")),
        "{deeds}"
    );
    let log = std::fs::read_to_string(hab.tracker.join("vissue.log")).unwrap();
    assert!(
        log.contains(&format!("deed {issue}")),
        "the retry was not deeded on {issue}:\n{log}"
    );
    let rows = book_rows(&hab);
    let row = sitting(&rows, &issue);
    assert_eq!(row["start_sha"].as_str(), Some(start.as_str()), "{row}");
    assert_eq!(row["finished"], false, "{row}");
    assert!(
        row["deeds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == &sha),
        "{row}"
    );
    assert!(
        row["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str() == Some("git commit exited non-zero")),
        "{row}"
    );
    assert!(
        !rows.iter().any(|row| {
            row["start_sha"].as_str() == Some(sha.as_str()) && row["issue"].is_null()
        }),
        "the landed sha became the start of an empty book: {rows:?}"
    );

    let session = "opencode-hook";
    claim_session(&hab, session, "Record the hook retry");
    let rows = book_rows(&hab);
    let issue = rows
        .iter()
        .filter(|row| row["deeds"].as_array().is_none_or(|deeds| deeds.is_empty()))
        .find_map(|row| row["issue"].as_str().map(str::to_string))
        .expect("the second edit claimed");
    let start = sitting(&rows, &issue)["start_sha"]
        .as_str()
        .unwrap()
        .to_string();
    ljos(
        &hab,
        &claude(
            "PostToolUse",
            session,
            &cwd,
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": command},
                "tool_response": {"exit_code": 1, "error": "nothing to commit"}
            }),
        ),
        &[],
    );
    let hooked = commit_file(&hab.repo, "hooked.txt");
    ljos_cmd(&hab, &["hook", "--git-commit"], "", &[]);
    let log = std::fs::read_to_string(hab.tracker.join("vissue.log")).unwrap();
    assert!(
        log.contains(&format!("deed {issue}")),
        "the git hook did not deed {hooked} on {issue}:\n{log}"
    );
    assert_eq!(
        log.matches("create").count(),
        2,
        "the git hook filed another issue:\n{log}"
    );
    let rows = book_rows(&hab);
    let row = sitting(&rows, &issue);
    assert_eq!(row["start_sha"].as_str(), Some(start.as_str()), "{row}");
    assert_eq!(row["finished"], false, "{row}");
    assert!(
        row["deeds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == &hooked),
        "{row}"
    );
    assert!(
        row["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str() == Some("git commit exited non-zero")),
        "{row}"
    );
    assert!(
        !rows.iter().any(|row| {
            row["start_sha"].as_str() == Some(hooked.as_str()) && row["issue"].is_null()
        }),
        "the hook opened an empty book at {hooked}: {rows:?}"
    );
}

/// The first prompt of a fresh session says what is already on disk: the
/// user rules and a stored lesson, without waiting for the encoder to load.
#[test]
fn a_cold_first_prompt_shows_rules_and_memory() {
    let (_dir, hab) = habitat();
    let rules = hab.home.join(".config/ljos");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(
        rules.join("user-rules.json"),
        r#"{"rules":[{"id":"ur-slugify","text":"Never rename the slugify helper.","author":"ada","reason":"the seat asked","basis":"person","written_at":"2026-10-01T00:00:00Z"}]}
"#,
    )
    .unwrap();
    std::fs::create_dir_all(hab.run.join("ljos")).unwrap();
    std::fs::write(
        hab.run.join("ljos/book-held.json"),
        r#"{"repos":{"held":{"issue":"probe-held","finished":false}}}
"#,
    )
    .unwrap();
    let pack = hab.root.join("cold-pack.py");
    std::fs::write(
        &pack,
        r#"#!/usr/bin/env python3
import json, os, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
atoms = json.loads(os.environ["ATOMS"])
armed = False
class H(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def do_GET(self):
        global armed
        path = self.path.split("?", 1)[0]
        if path == "/v1/status":
            armed = True
            body = {"embedder": {"available": True, "answering": None}}
        elif path == "/v1/atoms":
            if armed:
                time.sleep(30)
            body = {"atoms": atoms}
        else:
            body = {"hits": []}
        self._send(body)
    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0") or 0)
        self.rfile.read(n)
        self._send({"hits": []})
    def _send(self, body):
        raw = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)
server = ThreadingHTTPServer(("127.0.0.1", 0), H)
print(server.server_address[1], flush=True)
server.serve_forever()
"#,
    )
    .unwrap();
    let atoms = serde_json::json!([{
        "id": "atom-slugify",
        "kind": "lesson",
        "text": "The slugify helper keeps kebab words whole.",
        "entities": ["horizon:standing"],
        "ts": "2026-10-17T00:00:00Z",
        "origin": "agent-derived"
    }]);
    let mut pack_proc = Command::new("/usr/bin/python3")
        .arg(&pack)
        .env("PATH", "/usr/bin:/bin")
        .env("ATOMS", atoms.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut port = String::new();
    let mut stdout = pack_proc.stdout.take().unwrap();
    let mut byte = [0u8; 1];
    let ready = std::time::Instant::now();
    while ready.elapsed() < Duration::from_secs(5) {
        if stdout.read(&mut byte).unwrap() == 0 {
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        port.push(byte[0] as char);
    }
    let port: u16 = port.trim().parse().expect("pack port");
    let url = format!("http://127.0.0.1:{port}");
    let cwd = hab.repo.clone();
    let started = std::time::Instant::now();
    let first = ljos(
        &hab,
        &claude(
            "UserPromptSubmit",
            "1",
            &cwd,
            serde_json::json!({"prompt": "Fix the slugify helper"}),
        ),
        &[
            ("PACKSET_URL", url.as_str()),
            ("LJOS_USER_RULES_SYNC", "off"),
        ],
    );
    let elapsed = started.elapsed();
    let _ = pack_proc.kill();
    let _ = pack_proc.wait();
    let note = context(&first.stdout);
    assert!(
        elapsed < Duration::from_secs(8),
        "the first prompt took {elapsed:?}\n{note}"
    );
    assert!(note.contains("User rules"), "{note}");
    assert!(note.contains("Never rename the slugify helper."), "{note}");
    assert!(
        note.contains("The slugify helper keeps kebab words whole."),
        "{note}"
    );
    assert!(note.contains("Remembered, oldest first"), "{note}");
    assert!(note.contains("probe-held"), "{note}");
    assert!(
        !note.contains("pack is loading its search model"),
        "the warming line hid the recall: {note}"
    );
}

/// A command whose directory does not resolve, a workdir or a session
/// `cwd` that is missing, a symlink loop, or a directory it cannot enter,
/// is denied as `cwd-unresolved`: judged in the session or the process
/// directory instead, it was judged where it does not run. With
/// `LJOS_GUARD_FAIL_OPEN=1` it is judged there, and a prompt from such a
/// directory is never denied.
#[test]
fn a_workdir_that_does_not_resolve_denies_the_command() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, hab) = habitat();
    sit(&hab);
    let session = hab.repo.to_str().unwrap();
    let missing = hab.root.join("gone");
    let looped = hab.root.join("loop-a");
    std::os::unix::fs::symlink(hab.root.join("loop-b"), &looped).unwrap();
    std::os::unix::fs::symlink(&looped, hab.root.join("loop-b")).unwrap();
    let locked = hab.root.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let on = [("LJOS_GUARD", "on"), ("LJOS_OFF", "0")];
    let payloads = [
        pretool(session, Some("/path/to/git/repo"), NORMAL_GIT),
        pretool(missing.to_str().unwrap(), None, NORMAL_GIT),
        pretool(session, Some(looped.to_str().unwrap()), "echo hi"),
        pretool(locked.to_str().unwrap(), None, "echo hi"),
    ];
    let denied: Vec<_> = payloads
        .iter()
        .map(|p| hook_in(&hab, p, &hab.repo, true, &on))
        .collect();
    let open = hook_in(
        &hab,
        &payloads[0],
        &hab.repo,
        true,
        &[on[0], on[1], ("LJOS_GUARD_FAIL_OPEN", "1")],
    );
    let prompt = claude(
        "UserPromptSubmit",
        "1",
        &missing,
        serde_json::json!({"prompt": "Fix the build"}),
    );
    let asked = hook_in(&hab, &prompt, &hab.repo, true, &on);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    for (payload, out) in payloads.iter().zip(&denied) {
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            !out.status.success()
                && stdout.contains("\"permissionDecision\":\"deny\"")
                && stdout.contains("cwd-unresolved"),
            "exit {}\nstdout:\n{stdout}\nstderr:\n{}\npayload:\n{payload}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert_ordinary(&open, &payloads[0]);
    assert!(
        asked.status.success() && !String::from_utf8_lossy(&asked.stdout).contains("deny"),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&asked.stdout),
        String::from_utf8_lossy(&asked.stderr)
    );
}

/// The hook process may run outside a repo while the payload carries `cwd`.
/// The guard must judge in that payload directory.
#[test]
fn payload_cwd_is_used_when_hook_process_runs_elsewhere() {
    let (_dir, hab) = habitat();
    let repo = std::fs::canonicalize(&hab.repo).unwrap();
    script(
        &hab.bin,
        "ljos-policyd",
        &format!(
            "#!/bin/sh\n\
             if [ \"$(/bin/pwd -P)\" = \"{}\" ]; then\n\
             printf 'deny\\tpayload-cwd\\n'\n\
             exit 2\n\
             fi\n\
             printf 'allow\\n'\n",
            repo.display()
        ),
    );
    let policyd = hab.bin.join("ljos-policyd");
    let policyd = policyd.to_str().unwrap().to_string();
    let elsewhere = tempfile::tempdir().unwrap();
    let payload = pretool(repo.to_str().unwrap(), None, "echo hi");
    let out = hook_in(
        &hab,
        &payload,
        elsewhere.path(),
        true,
        &[
            ("LJOS_GUARD", "on"),
            ("LJOS_OFF", "0"),
            ("POLICYD_BIN", policyd.as_str()),
        ],
    );
    assert!(
        !out.status.success(),
        "guard should deny in payload cwd\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let joined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(joined.contains("payload-cwd"), "{joined}");
}

/// ljos-qpiw: the opencode plugin reports each bash call, and no post
/// cites these commits: one ran from a script, so no command names
/// `git commit`, and one was chained before a push that exited 128. The
/// idle Stop cites both. The upstream commit a fast-forward brought in is
/// not this sitting's deed.
#[test]
fn an_opencode_commit_no_post_cited_is_deeded_at_stop() {
    let (_dir, hab) = habitat();
    git(&hab.repo, &["checkout", "-q", "-b", "upstream"]);
    let upstream = commit_file(&hab.repo, "upstream.txt");
    git(&hab.repo, &["checkout", "-q", "main"]);
    let session = "ses_7f3a91c2d4e5ffa1b2c3d4e5";
    let env = [("OPENCODE_SESSION_ID", session)];
    let cwd = hab.repo.clone();
    let send = |event: &str, extra: serde_json::Value| {
        ljos(&hab, &claude(event, session, &cwd, extra), &env);
    };
    let bash = |command: &str, exit: i64, run: &dyn Fn()| {
        let input = serde_json::json!({"command": command});
        send(
            "PreToolUse",
            serde_json::json!({"tool_name": "Bash", "tool_input": input}),
        );
        run();
        send(
            "PostToolUse",
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": input,
                "tool_response": {"exit_code": exit}
            }),
        );
    };
    send(
        "UserPromptSubmit",
        serde_json::json!({"prompt": "Add slugify with tests, then commit"}),
    );
    send("SessionStart", serde_json::json!({}));
    bash("git merge --ff-only upstream", 0, &|| {
        git(&hab.repo, &["merge", "-q", "--ff-only", "upstream"]);
    });
    let write = serde_json::json!({
        "tool_name": "Write",
        "tool_input": {"filePath": hab.repo.join("slug.py"), "content": "x\n"}
    });
    send("PreToolUse", write.clone());
    std::fs::write(hab.repo.join("slug.py"), "x\n").unwrap();
    send("PostToolUse", write);
    bash("python3 commit.py && git log --oneline -1", 0, &|| {
        git(&hab.repo, &["add", "slug.py"]);
        git(&hab.repo, &["commit", "-q", "-m", "add slugify"]);
    });
    let scripted = head(&hab.repo);
    let pushed = "git add test_slug.py && git commit -m tests && git push origin main";
    bash(pushed, 128, &|| {
        commit_file(&hab.repo, "test_slug.py");
    });
    let chained = head(&hab.repo);
    send("Stop", serde_json::json!({}));
    let deeds = std::fs::read_to_string(hab.tracker.join("deedar.log")).unwrap_or_default();
    for (sha, how) in [(&scripted, "script"), (&chained, "chained")] {
        assert!(
            deeds.contains(&format!("--id deed-file-commit-{sha}")),
            "the {how} commit got no deed\n{deeds}"
        );
    }
    assert!(
        !deeds.contains(&upstream),
        "a fast-forward is not the sitting's commit\n{deeds}"
    );
    let org = std::fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(
        org.contains(&format!("cited deed-file-commit-{scripted}")),
        "{org}"
    );
    assert!(org.contains("* DONE"), "{org}");
}

struct Daemon(std::process::Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// packsetd from PATH on a free port, with its home in the habitat, and
/// the env that points `ljos` at it.
fn packsetd(hab: &Habitat) -> (Daemon, Vec<(&'static str, String)>) {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(bin) = std::env::split_paths(&path)
        .map(|dir| dir.join("packsetd"))
        .find(|candidate| candidate.is_file())
    else {
        panic!("packsetd is not on PATH; CI builds it from packset develop");
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .unwrap()
        .port();
    let pack_home = hab.home.join(".local/share/packset");
    std::fs::create_dir_all(&pack_home).unwrap();
    let daemon = Daemon(
        Command::new(bin)
            .args(["--port", &port.to_string(), "--home"])
            .arg(&pack_home)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &hab.home)
            .env("PACKSET_HOME", &pack_home)
            .env("PACKSET_AUTH", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "packsetd did not listen"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let env = vec![
        ("PACKSET_URL", format!("http://127.0.0.1:{port}")),
        ("PACKSET_AUTH", "off".to_string()),
    ];
    (daemon, env)
}

/// ljos-gfiz: a Claude turn commits and stops, so the lesson cites that
/// commit. The next turn amends it under a new prompt. The lesson written
/// again cites the commit that landed and closes the first one, so the
/// next session is not handed the replaced sha.
#[test]
fn a_rewritten_lesson_closes_the_one_that_cited_an_amended_commit() {
    let (_dir, hab) = habitat();
    let (_daemon, pack) = packsetd(&hab);
    let pack: Vec<(&str, &str)> = pack.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let session = "4c1d7e2a-8b3f-4e6d-9a1c-2f5e8d7b6a90";
    let cwd = hab.repo.clone();
    let send = |session: &str, event: &str, extra: serde_json::Value| {
        let out = ljos(&hab, &claude(event, session, &cwd, extra), &pack);
        context(&out.stdout)
    };
    let bash = |command: &str| {
        serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "tool_response": {"exit_code": 0}
        })
    };
    send(
        session,
        "UserPromptSubmit",
        serde_json::json!({"prompt": "Add a slugify function in slug.py with tests and commit it"}),
    );
    send(
        session,
        "PreToolUse",
        serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "slug.py"}}),
    );
    let replaced = commit_file(&hab.repo, "slug.py");
    send(
        session,
        "PostToolUse",
        bash("git add slug.py && git commit -m 'add slugify'"),
    );
    send(
        session,
        "Stop",
        serde_json::json!({"last_assistant_message": "Committed slugify."}),
    );
    send(
        session,
        "UserPromptSubmit",
        serde_json::json!({"prompt": "Reword that commit message to say Add slugify with tests"}),
    );
    git(
        &hab.repo,
        &["commit", "-q", "--amend", "-m", "Add slugify with tests"],
    );
    let landed = head(&hab.repo);
    send(
        session,
        "PostToolUse",
        bash("git commit --amend -m 'Add slugify with tests'"),
    );
    send(
        session,
        "Stop",
        serde_json::json!({"last_assistant_message": "Reworded the commit."}),
    );
    let listed = ljos_cmd(&hab, &["memories", "list", "--all", "--json"], "", &pack);
    let listed = String::from_utf8_lossy(&listed.stdout).to_string();
    assert!(
        listed.contains(&short_sha(&landed)),
        "no lesson cites the landed commit: {listed}"
    );
    assert!(
        !listed.contains(&short_sha(&replaced)),
        "a live lesson still cites the replaced commit: {listed}"
    );
    let recall = send(
        "5d2e8f3b-9c40-4f7e-8b2d-3a6f9e8c7b01",
        "UserPromptSubmit",
        serde_json::json!({"prompt": "write a slugify function with tests"}),
    );
    assert!(recall.contains(&short_sha(&landed)), "{recall}");
    assert!(!recall.contains(&short_sha(&replaced)), "{recall}");
}
