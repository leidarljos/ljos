//! ljos-7wc3: one agy seat books under one name. `ljos onboard` gives
//! agy's server `LJOS_SEAT=antigravity`, the harness id, while the hooks
//! sit the seat `~/.config/ljos/env` names. On the willma-20261021g seat
//! replay the hook's write claimed under `free-agy@TAG`, and the server's
//! finish ran as `antigravity`: "the tracker lists no seat antigravity",
//! "not assignee", and the issue left STARTED.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const SEAT: &str = "free-agy";
const CONVERSATION: &str = "7f1c2d3e-4b5a-6978-8091-a2b3c4d5e6f7";

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

/// A tracker that stamps the claim with `VISSUE_AGENT`, as vissue does.
const VISSUE: &str = r#"#!/usr/bin/env python3
import os, sys, pathlib, re
root = pathlib.Path(os.environ["VISSUE_ROOT"])
args = sys.argv[1:]
with (root / "vissue.log").open("a") as fh:
    fh.write(" ".join(args) + "\n")
cmd, rest = args[0], args[1:]
board = root / "Software" / "work" / "issues.org"

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
    proj, title, i = "work", "work", 0
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

/// The claim graph: one assignee per node, and a completion or a release
/// by any other actor refused with claimdag's "not assignee".
const CLAIMDAG: &str = r#"#!/usr/bin/env python3
import json, os, pathlib, sys
state = pathlib.Path(os.environ["XDG_RUNTIME_DIR"]) / "claimdag.json"
nodes = json.loads(state.read_text()) if state.exists() else {}
args = sys.argv[1:]
with state.with_suffix(".log").open("a") as fh:
    fh.write(" ".join(args) + "\n")
cmd, rest = args[0], args[1:]

def flag(name):
    return rest[rest.index(name) + 1] if name in rest else None

def save():
    state.write_text(json.dumps(nodes))

if cmd == "upsert":
    nid = flag("--id")
    nodes.setdefault(nid, {"status": "ready", "gen": 1, "assignee": None, "summary": flag("--summary") or ""})
    save()
    print(nid)
elif cmd == "get":
    node = nodes.get(rest[0])
    if node is None:
        sys.exit("get: not found")
    print(f"{rest[0]}  {node['status']}  task  unset  gen={node['gen']}  assignee={node['assignee'] or '-'}  parent={'0' * 32}  {node['summary']}")
    print("deps  -")
elif cmd == "claim":
    node = nodes[rest[0]]
    node.update(status="claimed", gen=node["gen"] + 1, assignee=flag("--assignee"))
    save()
    print(f"gen={node['gen']}")
elif cmd in ("complete", "release"):
    node = nodes.get(rest[0])
    if node is None:
        sys.exit(f"{cmd}: not found")
    if node["assignee"] != flag("--actor"):
        sys.exit(f"{cmd}: not assignee")
    if cmd == "complete":
        node.update(status="done")
        print("ok")
    else:
        node.update(status="ready", gen=node["gen"] + 1, assignee=None)
        print(f"gen={node['gen']}")
    save()
elif cmd == "list":
    pass
else:
    sys.exit(f"{cmd}: unsupported")
"#;

fn habitat() -> Habitat {
    habitat_with(&[SEAT, "free-claude"], SEAT)
}

/// A tracker whose `seats` roster is `roster`, and an env file that sits
/// the hooks as `seat`.
fn habitat_with(roster: &[&str], seat: &str) -> Habitat {
    let dir = tempfile::tempdir().unwrap();
    let tracker = dir.path().join("tracker");
    let repo = dir.path().join("repo");
    let bin = dir.path().join("bin");
    let run = dir.path().join("run");
    let home = dir.path().join("home");
    for path in [&tracker, &repo, &bin, &run, &home.join(".config/ljos")] {
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
    let roster: Vec<String> = roster.iter().map(|s| format!("\"{s}\"")).collect();
    std::fs::write(
        tracker.join("vissue.toml"),
        format!("seats = [{}]\n", roster.join(", ")),
    )
    .unwrap();
    std::fs::write(home.join(".config/ljos/env"), format!("LJOS_SEAT={seat}\n")).unwrap();
    script(&bin, "vissue", VISSUE);
    script(&bin, "claimdag", CLAIMDAG);
    Habitat {
        _dir: dir,
        tracker,
        repo,
        bin,
        run,
        home,
    }
}

/// What agy hands the hooks and the server alike: its conversation id,
/// and no seat of its own.
fn agy_env(cmd: &mut Command, hab: &Habitat) {
    cmd.env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", hab.bin.display()))
        .env("HOME", &hab.home)
        .env("USER", "agy-runner")
        .env("XDG_CONFIG_HOME", hab.home.join(".config"))
        .env("XDG_STATE_HOME", hab.home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &hab.run)
        .env("VISSUE_ROOT", &hab.tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_TRACKER_GIT", "commit")
        .env("LJOS_PROJECT", "work")
        .env("LJOS_TRACK_WORK", "on")
        .env("PACKSET_URL", "off")
        .env("ANTIGRAVITY_CONVERSATION_ID", CONVERSATION)
        .current_dir(&hab.repo);
}

/// agy 1.4.3's after hook for a `write_file`, the payload of cell w4.
fn write_file_hook(hab: &Habitat) {
    let file = hab.repo.join("slug.py");
    std::fs::write(&file, "import re\n").unwrap();
    let payload = serde_json::json!({
        "artifactDirectoryPath": hab.home.join(".gemini/antigravity-cli/brain").join(CONVERSATION),
        "conversationId": CONVERSATION,
        "error": "",
        "modelName": "openai/gpt-oss-120b",
        "stepIdx": 8,
        "toolCall": {
            "name": "write_file",
            "args": {"Content": "import re\n", "Overwrite": false, "Path": file}
        },
        "workspacePaths": [hab.repo],
    });
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(["hook", "--event", "PostToolUse"]);
    agy_env(&mut cmd, hab);
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
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "ljos hook: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut drain = Command::new(env!("CARGO_BIN_EXE_ljos"));
    drain.args(["hook", "--drain"]);
    agy_env(&mut drain, hab);
    assert!(drain.status().unwrap().success(), "ljos hook --drain");
}

/// One `ljos_finish` to the server as `ljos onboard` registers it for agy,
/// `LJOS_SEAT=antigravity` in its entry, and the answer.
fn mcp_finish(hab: &Habitat) -> serde_json::Value {
    mcp_finish_as(hab, "antigravity", "antigravity")
}

/// [`mcp_finish`] from a server whose entry sets `LJOS_SEAT=seat`, to a
/// client that names itself `client`.
fn mcp_finish_as(hab: &Habitat, seat: &str, client: &str) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
    agy_env(&mut cmd, hab);
    cmd.env("LJOS_SEAT", seat);
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
            "clientInfo": {"name": client, "version": "1.4.3"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "ljos_finish", "arguments": {"lesson": "slug.py turns a title into a slug"}}}),
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
    answer.expect("ljos-mcp answered the finish")
}

fn state_of(hab: &Habitat, issue: &str) -> String {
    let org = std::fs::read_to_string(hab.tracker.join("Software/work/issues.org")).unwrap();
    let at = org.find(&format!(":ID:         {issue}\n")).unwrap();
    let head = org[..at].rfind("* ").unwrap();
    org[head + 2..]
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn holder_of(hab: &Habitat, issue: &str) -> String {
    let org = std::fs::read_to_string(hab.tracker.join("Software/work/issues.org")).unwrap();
    let at = org.find(&format!(":ID:         {issue}\n")).unwrap();
    org[at..]
        .lines()
        .nth(1)
        .and_then(|line| line.strip_prefix(":CLAIMED_BY:"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The hook's Write claims `work-0001` under `seat`, and the agy server
/// finishes it.
fn the_server_finishes_the_write(hab: &Habitat, seat: &str) {
    write_file_hook(hab);
    let held = "work-0001";
    assert_eq!(state_of(hab, held), "STARTED");
    let hook_holder = holder_of(hab, held);
    assert!(
        hook_holder.starts_with(&format!("{seat}@")),
        "the hook holds under the seat the env file names: {hook_holder}"
    );
    let answer = mcp_finish(hab);
    let log = std::fs::read_to_string(hab.run.join("ljos/book.log")).unwrap_or_default();
    let graph = std::fs::read_to_string(hab.run.join("claimdag.log")).unwrap_or_default();
    let context = format!("{answer}\nbook.log:\n{log}\nclaimdag:\n{graph}");
    assert_ne!(answer["result"]["isError"], true, "{context}");
    assert_ne!(state_of(hab, held), "STARTED", "{context}");
    assert_eq!(holder_of(hab, held), "", "{context}");
}

#[test]
fn the_agy_server_finishes_the_claim_its_write_hook_took() {
    the_server_finishes_the_write(&habitat(), SEAT);
}

/// An old claim the server took as `antigravity`, before the hooks sat as
/// `free-agy`, does not make the harness id a seat: only the `seats`
/// roster decides that.
#[test]
fn an_old_antigravity_claim_does_not_keep_the_harness_id() {
    let hab = habitat();
    let board = hab.tracker.join("Software/work/issues.org");
    std::fs::create_dir_all(board.parent().unwrap()).unwrap();
    std::fs::write(
        &board,
        "* DONE [#C] older agy work\n:PROPERTIES:\n:ID:         work-00ff\n\
         :CLAIMED_BY: antigravity@deadbeef\n:END:\n",
    )
    .unwrap();
    the_server_finishes_the_write(&hab, SEAT);
    assert_eq!(state_of(&hab, "work-00ff"), "DONE");
}

/// A seat a person named after a harness is not that harness's seat. A
/// server whose entry sets `LJOS_SEAT=claude` neither sits as the
/// roster's only `claude-*` seat nor finishes the claim it holds.
#[test]
fn a_harness_id_does_not_finish_a_seat_named_after_it() {
    let hab = habitat_with(&["claude-reviewer"], "claude-reviewer");
    write_file_hook(&hab);
    let held = "work-0001";
    assert!(holder_of(&hab, held).starts_with("claude-reviewer@"));
    let answer = mcp_finish_as(&hab, "claude", "claude");
    let log = std::fs::read_to_string(hab.run.join("ljos/book.log")).unwrap_or_default();
    let context = format!("{answer}\nbook.log:\n{log}");
    assert!(!log.contains("sitting claude-reviewer"), "{context}");
    assert_eq!(state_of(&hab, held), "STARTED", "{context}");
    assert!(
        holder_of(&hab, held).starts_with("claude-reviewer@"),
        "{context}"
    );
}

/// A roster that lists `antigravity` keeps it: the hooks and the server
/// both sit as the harness id the person named.
#[test]
fn a_harness_id_on_the_roster_is_kept() {
    let hab = habitat_with(&["antigravity", SEAT], "antigravity");
    the_server_finishes_the_write(&hab, "antigravity");
    let log = std::fs::read_to_string(hab.run.join("ljos/book.log")).unwrap_or_default();
    assert!(!log.contains("for that harness"), "{log}");
}
