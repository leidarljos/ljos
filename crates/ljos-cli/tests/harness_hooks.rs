//! The five real-harness fixes, each driven through `ljos hook` (the ballot
//! case through `ljos vote`, which is the process that casts and tallies).

ljos_test_boot::harness!();
use std::io::Write;
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

/// A second sitting, on `probe-0002`, held by `holder`.
fn sit_as(hab: &Habitat, holder: &str) {
    std::fs::OpenOptions::new()
        .append(true)
        .open(hab.tracker.join("Software/probe/issues.org"))
        .unwrap()
        .write_all(
            format!(
                "* STARTED [#C] probe work two\n:PROPERTIES:\n:ID:         probe-0002\n:CLAIMED_BY: {holder}\n:END:\n"
            )
            .as_bytes(),
        )
        .unwrap();
    let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
    std::fs::write(
        hab.run.join("ljos/hold-probe-sitting-2"),
        format!(
            "{holder}\nprobe-seat\n{}\n{}\n2026-01-01T00:00:00Z\nprobe-0002\n-\n",
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
    let out = child.wait_with_output().unwrap();
    // A hook's book is applied after its answer; what the test reads next
    // is the book after it.
    if args.first() == Some(&"hook") && !args.contains(&"--drain") {
        let _ = run_ljos(hab, &["hook", "--drain"], "", dir, extra);
    }
    out
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
    // Cursor beforeShellExecution enforces permission ask. The payload
    // names no conversation, so the habitat's `LJOS_HOLD` sits for it.
    let shell = ljos(
        &hab,
        &serde_json::json!({
            "cursor_version": "2.4.0",
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
    // Cursor preToolUse accepts ask and does not enforce it. The payload
    // names its conversation, which sits under its own holder.
    sit_as(
        &hab,
        &format!("probe-seat@{}", ljos_cli::session_tag("force-cursor-tool")),
    );
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
        let seat = args
            .iter()
            .position(|a| *a == "--as")
            .map_or("sess-1", |i| args[i + 1]);
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
            .env("VISSUE_AGENT", seat)
            .env("LJOS_SEAT", seat)
            .env("SEAT_SESSION_ID", "sess-holder-0001")
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
        "session_id": "workdir-sess",
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

fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
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
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success() && !stdout.contains("\"permissionDecision\":\"deny\""),
            "exit {}\nstdout:\n{stdout}\nstderr:\n{stderr}\npayload:\n{payload}",
            out.status
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
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
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
    let _landed = landed;
    let listed = ljos_cmd(&hab, &["memories", "list", "--all", "--json"], "", &pack);
    let listed = String::from_utf8_lossy(&listed.stdout).to_string();
    assert!(
        !listed.contains(&short_sha(&replaced)),
        "a lesson still cites the replaced commit: {listed}"
    );
}
