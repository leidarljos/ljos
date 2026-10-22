//! One harness session is one voice. Replays the failure where one worker
//! minted N seats and cast every ballot, and a run where each seat is its
//! own session.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BLIND: &str = "\
design demo-a is not blind: session sess-designer-0001 read demo-b before filing it";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct World {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    home: PathBuf,
    root: PathBuf,
    state: PathBuf,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let home = dir.path().join("home");
        let root = dir.path().join("tracker");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let state_s = state.display().to_string();
        script(
            dir.path(),
            "vissue",
            &format!(
                r#"#!/usr/bin/env python3
import json, os, sys
state = {state_s:?}
os.makedirs(state, exist_ok=True)

def load(name, default):
    path = os.path.join(state, name)
    if not os.path.exists(path):
        return default
    with open(path) as f:
        return json.load(f)

def save(name, data):
    path = os.path.join(state, name)
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(data, f)
    os.replace(tmp, path)

args = sys.argv[1:]
with open(os.path.join(state, "calls.log"), "a") as log:
    log.write(" ".join(args) + "\n")
cmd = args[0] if args else ""
if cmd == "vote":
    ident = args[1]
    rest = args[2:]
    ballots = load("ballots.json", {{}})
    if "--json" in rest:
        print(json.dumps(ballots.get(ident, [])))
        sys.exit(0)
    if "--withdraw" in rest:
        agent = os.environ.get("VISSUE_AGENT") or "seat"
        rows = ballots.get(ident, [])
        kept = [r for r in rows if r.get("agent") != agent]
        if len(kept) == len(rows):
            sys.stderr.write(ident + ": " + agent + " holds no ballot\n")
            sys.exit(1)
        ballots[ident] = kept
        save("ballots.json", ballots)
        print("withdrawn")
        sys.exit(0)
    choice = ""
    i = 0
    while i < len(rest):
        if rest[i] == "--for" and i + 1 < len(rest):
            choice = rest[i + 1]
            i += 2
        else:
            i += 1
    agent = os.environ.get("VISSUE_AGENT") or "seat"
    rows = [r for r in ballots.get(ident, []) if r.get("agent") != agent]
    rows.append({{"agent": agent, "choice": choice}})
    ballots[ident] = rows
    save("ballots.json", ballots)
    print("voted")
elif cmd == "note":
    ident = args[1]
    text = " ".join(args[2:])
    notes = load("notes.json", {{}})
    rows = notes.get(ident, [])
    rows.insert(0, {{"note": text}})
    notes[ident] = rows
    save("notes.json", notes)
    print(f"{{ident}}: noted")
elif cmd == "show":
    ident = args[1]
    issues = load("issues.json", {{}})
    meta = dict(issues.get(ident, {{"id": ident, "title": ident, "body": ""}}))
    meta["logbook"] = load("notes.json", {{}}).get(ident, [])
    meta.setdefault("id", ident)
    print(json.dumps(meta))
elif cmd == "children":
    ident = args[1] if len(args) > 1 else ""
    print(json.dumps(load("children.json", {{}}).get(ident, [])))
elif cmd == "create":
    parent = None
    body = ""
    title = args[-1] if args else ""
    if "--parent" in args:
        parent = args[args.index("--parent") + 1]
    if "--body" in args:
        body = args[args.index("--body") + 1]
    if title.startswith("design: mine"):
        ident = "demo-a"
    elif title.startswith("design:"):
        ident = "demo-d001"
    else:
        ident = "demo-x001"
    issues = load("issues.json", {{}})
    row = {{"id": ident, "title": title, "body": body}}
    if parent:
        row["parent"] = parent
    issues[ident] = row
    save("issues.json", issues)
    if parent:
        kids = load("children.json", {{}})
        kids.setdefault(parent, []).append({{"id": ident}})
        save("children.json", kids)
    print(ident)
elif cmd == "append":
    ident = args[1]
    text = " ".join(args[2:])
    issues = load("issues.json", {{}})
    meta = dict(issues.get(ident, {{"id": ident, "title": ident, "body": ""}}))
    body = meta.get("body") or ""
    if body and not body.endswith("\n"):
        body += "\n"
    body += text
    if text and not body.endswith("\n"):
        body += "\n"
    meta["body"] = body
    meta.setdefault("id", ident)
    issues[ident] = meta
    save("issues.json", issues)
    print(ident + ": appended")
elif cmd == "recall":
    sys.exit(0)
elif cmd == "consensus":
    print("consensus: ship 3 of 3, settled")
else:
    sys.stderr.write("unexpected vissue " + " ".join(args) + "\n")
    sys.exit(1)
"#,
            ),
        );
        script(
            dir.path(),
            "ljos-consensus",
            r#"#!/bin/sh
printf '%s\n' '{"options":["ship"],"shares":[1.0],"agents":["alice","bob","carol"],"settled":true,"engine":"degroot"}'
"#,
        );
        script(
            dir.path(),
            "member",
            r#"#!/bin/sh
d="${VISSUE_STATE}/members"
mkdir -p "$d"
id="${LJOS_HARNESS_SESSION:-missing}"
printf '%s\n' "$id" > "$d/$id.session"
printf '%s\n' "${LJOS_SESSION_ID-}" > "$d/$id.ljos"
printf '%s\n' "${SEAT_SESSION_ID-}" > "$d/$id.seat-session"
printf '%s\n' "${LJOS_PANEL_CHILD-}" > "$d/$id.child"
printf '%s\n' "${LJOS_SEAT-}" > "$d/$id.seat"
printf '%s\n' "${VISSUE_AGENT-}" > "$d/$id.agent"
printf '%s\n' "${LJOS_PROCESS_NONCE-UNSET}" > "$d/$id.nonce"
printf '%s\n' "$1" > "$d/$id.flag"
printf '%s' "$2" > "$d/$id.prompt"
"#,
        );
        Self {
            bin: dir.path().to_path_buf(),
            home,
            root,
            state,
            _dir: dir,
        }
    }

    fn ljos(&self, seat: &str, session: &str, args: &[&str]) -> Output {
        if session.is_empty() {
            self.ljos_env(seat, &[], args)
        } else {
            self.ljos_env(seat, &[("SEAT_SESSION_ID", session)], args)
        }
    }

    /// `ljos` as `seat`, with only the harness variables in `env`.
    fn ljos_env(&self, seat: &str, env: &[(&str, &str)], args: &[&str]) -> Output {
        self.command(seat, env, args).output().unwrap()
    }

    /// `ljos hook` fed `payload` on stdin, as a runner's hook is run.
    fn hook(&self, env: &[(&str, &str)], payload: &serde_json::Value) -> Output {
        use std::io::Write as _;
        use std::process::Stdio;
        let mut child = self
            .command("ljosbot", env, &["hook"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(payload.to_string().as_bytes()).unwrap();
        drop(stdin);
        child.wait_with_output().unwrap()
    }

    /// `ljos-mcp` sent `calls` after the handshake, each answer by its id.
    fn mcp(&self, env: &[(&str, &str)], calls: &[serde_json::Value]) -> Vec<serde_json::Value> {
        use std::io::Write as _;
        use std::process::Stdio;
        let mut child = self
            .program(env!("CARGO_BIN_EXE_ljos-mcp"), "ljosbot", env, &[])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let handshake = [
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "claude-code", "version": "0"}}}),
            serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        ];
        for line in handshake.iter().chain(calls) {
            writeln!(stdin, "{line}").unwrap();
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    fn command(&self, seat: &str, env: &[(&str, &str)], args: &[&str]) -> Command {
        self.program(env!("CARGO_BIN_EXE_ljos"), seat, env, args)
    }

    fn program(&self, exe: &str, seat: &str, env: &[(&str, &str)], args: &[&str]) -> Command {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut cmd = Command::new(exe);
        cmd.args(args)
            .current_dir(&self.bin)
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_STATE", &self.state)
            .env("VISSUE_NO_ROUTE", "1")
            .env("PACKSET_URL", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_SEAT", seat)
            .env("VISSUE_AGENT", seat)
            .envs(env.iter().copied());
        cmd
    }
}

fn text(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn vote(world: &World, seat: &str, session: &str, issue: &str) {
    let out = world.ljos(
        seat,
        session,
        &["vote", issue, "--for", "ship", "--used", "none"],
    );
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains(&format!("session {session}")),
        "ballot was not bound:\n{said}"
    );
}

#[test]
fn one_session_of_n_seats_is_refused_at_the_cast() {
    let world = World::new();
    vote(&world, "alice", "sess-shared-0001", "demo-ind1");
    for seat in ["bob", "carol"] {
        let out = world.ljos(
            seat,
            "sess-shared-0001",
            &["vote", "demo-ind1", "--for", "ship", "--used", "none"],
        );
        let said = text(&out);
        assert!(!out.status.success(), "{said}");
        assert!(
            said.contains("already cast") && said.contains("Nothing was cast"),
            "{said}"
        );
    }
    let tally = world.ljos("alice", "sess-alice-0001", &["vote", "demo-ind1"]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("1 ballot counts"), "{said}");
    assert!(stdout.contains("alice"), "{said}");
    assert!(!stdout.contains("bob"), "{said}");
}

#[test]
fn a_choice_with_a_space_is_one_ballot_per_session() {
    // The panel eval's unscored case on 7e21d3c: session spaced-1 cast
    // `beside binary` as seat-a, then `xdg home`, and both landed.
    let world = World::new();
    let issue = "panel-spcd";
    let first = world.ljos(
        "seat-a",
        "spaced-1",
        &["vote", issue, "--for", "beside binary", "--used", "none"],
    );
    let said = text(&first);
    assert!(first.status.success(), "{said}");
    assert!(said.contains("session spaced-1"), "{said}");
    let again = world.ljos(
        "seat-a",
        "spaced-1",
        &["vote", issue, "--for", "xdg home", "--used", "none"],
    );
    let said = text(&again);
    assert!(!again.status.success(), "{said}");
    assert!(
        said.contains("already cast") && said.contains("Nothing was cast"),
        "{said}"
    );
    let notes: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(world.state.join("notes.json")).unwrap())
            .unwrap();
    let sigs: Vec<&str> = notes[issue]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["note"].as_str())
        .filter(|n| n.starts_with("ballot-sig: "))
        .collect();
    assert_eq!(sigs.len(), 1, "{sigs:?}");
    assert!(
        sigs[0].starts_with("ballot-sig: seat-a beside binary "),
        "{sigs:?}"
    );
    let tally = world.ljos("seat-b", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("1 ballot counts"), "{said}");
    assert!(stdout.contains("seat-a  beside binary"), "{said}");
}

#[test]
fn one_sand_subagent_five_personas_count_as_the_first_ballot() {
    let world = World::new();
    let session = "sand-subagent-fcdb9c57-9988-7d91-4311-9fc5e42f7529";
    let personas = ["academic", "transit", "safety", "cat-owner", "finance"];
    let issue = "travel-rx1d";
    // The drawer as the live tracker held it: five personas, then the same
    // five again, one session on every ballot-sig. Newest note first.
    let key = ed25519_dalek::SigningKey::from_bytes(&[11; 32]);
    let mut notes = Vec::new();
    for persona in personas.iter().rev() {
        let note = ljos_cli::ballot_sig::sign_bound(
            &key,
            issue,
            persona,
            "bus",
            session,
            &format!("n-{persona}-2"),
        );
        notes.push(serde_json::json!({ "note": note }));
    }
    for persona in personas.iter().rev() {
        let note = ljos_cli::ballot_sig::sign_bound(
            &key,
            issue,
            persona,
            "rail",
            session,
            &format!("n-{persona}-1"),
        );
        notes.push(serde_json::json!({ "note": note }));
    }
    let mut ballots = serde_json::Map::new();
    ballots.insert(
        issue.into(),
        serde_json::json!(personas
            .iter()
            .map(|persona| serde_json::json!({"agent": persona, "choice": "bus"}))
            .collect::<Vec<_>>()),
    );
    std::fs::write(
        world.state.join("notes.json"),
        serde_json::to_string(&serde_json::json!({issue: notes})).unwrap(),
    )
    .unwrap();
    std::fs::write(
        world.state.join("ballots.json"),
        serde_json::to_string(&serde_json::Value::Object(ballots)).unwrap(),
    )
    .unwrap();
    let tally = world.ljos("academic", session, &["vote", issue]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("1 ballot counts"), "{said}");
    assert!(stdout.contains("academic  rail"), "{said}");
    assert!(
        stdout.contains("a later ballot on this session is ignored"),
        "{said}"
    );
    for persona in &personas[1..] {
        assert!(stdout.contains(persona), "{said}");
        assert!(stdout.contains("same session"), "{said}");
    }
    assert!(stdout.contains(session), "{said}");
    assert!(!stdout.contains("finance  bus"), "{said}");

    for persona in personas {
        let out = world.ljos(
            persona,
            session,
            &[
                "vote", issue, "--for", "bus", "--used", "none", "--as", persona,
            ],
        );
        let said = text(&out);
        assert!(!out.status.success(), "{persona}: {said}");
        assert!(
            said.contains("already cast")
                && said.contains("Nothing was cast")
                && said.contains(session),
            "{persona}: {said}"
        );
        assert!(
            said.contains(
                "Each persona brief goes to its own subagent or run, \
                 which casts `ljos vote --as NAME` itself"
            ),
            "{persona}: {said}"
        );
    }
    let other = world.ljos(
        "clerk",
        "sess-clerk-0001",
        &[
            "vote", issue, "--for", "rail", "--used", "none", "--as", "clerk",
        ],
    );
    let said = text(&other);
    assert!(other.status.success(), "{said}");
    let again = world.ljos("clerk", "sess-clerk-0001", &["vote", issue]);
    let said = text(&again);
    assert!(again.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&again.stdout);
    assert!(stdout.contains("2 ballots count"), "{said}");
    assert!(stdout.contains("clerk  rail"), "{said}");
    assert!(stdout.contains("academic  rail"), "{said}");
}

#[test]
fn n_distinct_sessions_settle() {
    let world = World::new();
    // A cite stands for these seats because they are on the standing list.
    // A seat off that list needs a slot, which this panel does not write,
    // so carol's ballot is left out and named and the rest still decide.
    for seat in ["ljosbot", "inky", "disk-saver", "carol"] {
        vote(&world, seat, &format!("sess-{seat}-0001"), "demo-ind2");
    }
    let out = world.ljos("ljosbot", "sess-ljosbot-0001", &["consensus", "demo-ind2"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(
            "decides ship for a push by ljosbot; \
             counted ljosbot for ship, inky for ship, disk-saver for ship; \
             left out: carol is not a standing seat, and no slot on demo-ind2 names it"
        ),
        "{said}"
    );
    assert!(stdout.contains("the settle is unanimous"), "{said}");
    assert!(!stdout.contains("refuses to settle"), "{said}");
}

#[test]
fn an_author_who_votes_is_refused() {
    let world = World::new();
    let filed = world.ljos(
        "alice",
        "sess-alice-0001",
        &[
            "file",
            "design: cache",
            "--parent",
            "demo-vote",
            "-p",
            "demo",
            "-t",
            "task",
        ],
    );
    let said = text(&filed);
    assert!(filed.status.success(), "{said}");
    assert!(
        said.contains("candidate demo-d001 bound to session sess-alice-0001"),
        "{said}"
    );
    let author = world.ljos(
        "alice",
        "sess-alice-0001",
        &["vote", "demo-vote", "--for", "ship", "--used", "none"],
    );
    let said = text(&author);
    assert!(!author.status.success(), "{said}");
    assert!(
        said.contains("authored candidate") && said.contains("Nothing was cast"),
        "{said}"
    );
    vote(&world, "bob", "sess-bob-0001", "demo-vote");
    let out = world.ljos("bob", "sess-bob-0001", &["consensus", "demo-vote"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("refuses to settle"),
        "{said}"
    );
}

#[test]
fn a_design_that_saw_another_candidate_is_flagged_and_distinct_sessions_still_settle() {
    let world = World::new();
    std::fs::write(
        world.state.join("issues.json"),
        r#"{"demo-blind":{"id":"demo-blind","title":"Ship the cache only.","body":"Ship the cache only."},"demo-b":{"id":"demo-b","title":"design: other","body":"OTHER-CANDIDATE-BODY"}}"#,
    )
    .unwrap();
    std::fs::write(
        world.state.join("children.json"),
        r#"{"demo-blind":[{"id":"demo-b"}]}"#,
    )
    .unwrap();
    let recalled = world.ljos("designer", "sess-designer-0001", &["recall", "demo-b"]);
    assert!(recalled.status.success(), "{}", text(&recalled));
    let filed = world.ljos(
        "designer",
        "sess-designer-0001",
        &[
            "file",
            "design: mine",
            "--parent",
            "demo-blind",
            "-p",
            "demo",
            "-t",
            "task",
        ],
    );
    let said = text(&filed);
    assert!(filed.status.success(), "{said}");
    assert!(said.contains(&format!("blind design: {BLIND}")), "{said}");
    for seat in ["ljosbot", "inky", "disk-saver"] {
        vote(&world, seat, &format!("sess-{seat}-0001"), "demo-blind");
    }
    let out = world.ljos("ljosbot", "sess-ljosbot-0001", &["consensus", "demo-blind"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(&format!("blind design: {BLIND}")), "{said}");
    assert!(stdout.contains("decides ship"), "{said}");
    assert!(!stdout.contains("refuses to settle"), "{said}");
}

#[test]
fn panel_run_gives_each_designer_its_own_session_and_only_the_brief() {
    let world = World::new();
    std::fs::write(
        world.state.join("issues.json"),
        r#"{"demo-brief":{"id":"demo-brief","title":"Ship the cache only.","body":"Ship the cache only."},"demo-b":{"id":"demo-b","title":"design: other","body":"OTHER-CANDIDATE-BODY"}}"#,
    )
    .unwrap();
    let dry = world.ljos(
        "alice",
        "sess-alice-0001",
        &[
            "panel",
            "run",
            "demo-brief",
            "--dry-run",
            "--designers",
            "3",
        ],
    );
    let said = text(&dry);
    assert!(dry.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&dry.stdout);
    assert!(stdout.contains("dry run: nothing spawned"), "{said}");
    assert_eq!(stdout.matches("designer, brief only").count(), 3, "{said}");
    assert_eq!(stdout.matches("argv: cursor-agent -p").count(), 3, "{said}");
    assert!(stdout.contains("Ship the cache only."), "{said}");
    assert!(stdout.contains("Do not read another design"), "{said}");
    assert!(stdout.contains("Do not vote"), "{said}");
    assert!(!stdout.contains("OTHER-CANDIDATE-BODY"), "{said}");
    let mut sessions = Vec::new();
    for line in stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix("designer-") {
            let mut parts = rest.split_whitespace();
            let _n = parts.next();
            assert_eq!(parts.next(), Some("session"));
            sessions.push(parts.next().unwrap().to_string());
        }
    }
    assert_eq!(sessions.len(), 3, "{said}");
    let mut unique = sessions.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 3, "{sessions:?}");
    assert!(
        sessions
            .iter()
            .all(|s| s.starts_with("panel-demo-brief-designer-")),
        "{sessions:?}"
    );

    let claude = world.ljos(
        "alice",
        "sess-alice-0001",
        &[
            "panel",
            "run",
            "demo-brief",
            "--dry-run",
            "--designers",
            "1",
            "--harness",
            "claude",
        ],
    );
    let said = text(&claude);
    assert!(claude.status.success(), "{said}");
    assert!(
        String::from_utf8_lossy(&claude.stdout).contains("argv: claude -p"),
        "{said}"
    );

    let nested = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args([
            "panel",
            "run",
            "demo-brief",
            "--dry-run",
            "--designers",
            "1",
        ])
        .current_dir(&world.bin)
        .env_clear()
        .env(
            "PATH",
            format!(
                "{}:{}",
                world.bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("HOME", &world.home)
        .env("VISSUE_ROOT", &world.root)
        .env("VISSUE_STATE", &world.state)
        .env("PACKSET_URL", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_SEAT", "designer-1")
        .env("LJOS_PANEL_CHILD", "1")
        .output()
        .unwrap();
    let said = text(&nested);
    assert!(nested.status.success(), "{said}");
    assert!(
        String::from_utf8_lossy(&nested.stdout)
            .contains("already inside a panel member; nothing spawned"),
        "{said}"
    );

    let spawned = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["panel", "run", "demo-brief", "--designers", "3"])
        .current_dir(&world.bin)
        .env_clear()
        .env(
            "PATH",
            format!(
                "{}:{}",
                world.bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("HOME", &world.home)
        .env("VISSUE_ROOT", &world.root)
        .env("VISSUE_STATE", &world.state)
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_SEAT", "alice")
        .env("LJOS_MEMBER_BIN", world.bin.join("member"))
        .env("LJOS_PANEL_CONCURRENCY", "3")
        .output()
        .unwrap();
    let said = text(&spawned);
    assert!(spawned.status.success(), "{said}");
    let members = world.state.join("members");
    let mut prompts = Vec::new();
    for entry in std::fs::read_dir(&members).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("prompt") {
            continue;
        }
        let id = path.file_stem().unwrap().to_string_lossy().into_owned();
        let prompt = std::fs::read_to_string(&path).unwrap();
        assert!(prompt.contains("Ship the cache only."), "{prompt}");
        assert!(prompt.contains("Do not read another design"), "{prompt}");
        assert!(prompt.contains("Do not vote"), "{prompt}");
        assert!(!prompt.contains("OTHER-CANDIDATE-BODY"), "{prompt}");
        assert_eq!(
            std::fs::read_to_string(members.join(format!("{id}.flag")))
                .unwrap()
                .trim(),
            "-p"
        );
        assert_eq!(
            std::fs::read_to_string(members.join(format!("{id}.session")))
                .unwrap()
                .trim(),
            id
        );
        assert_eq!(
            std::fs::read_to_string(members.join(format!("{id}.ljos")))
                .unwrap()
                .trim(),
            id
        );
        assert_eq!(
            std::fs::read_to_string(members.join(format!("{id}.seat-session")))
                .unwrap()
                .trim(),
            id
        );
        assert_eq!(
            std::fs::read_to_string(members.join(format!("{id}.child")))
                .unwrap()
                .trim(),
            "1"
        );
        assert_eq!(
            std::fs::read_to_string(members.join(format!("{id}.nonce")))
                .unwrap()
                .trim(),
            "UNSET"
        );
        let seat = std::fs::read_to_string(members.join(format!("{id}.seat"))).unwrap();
        assert_eq!(
            seat.trim(),
            std::fs::read_to_string(members.join(format!("{id}.agent")))
                .unwrap()
                .trim()
        );
        assert!(seat.trim().starts_with("designer-"), "{seat}");
        prompts.push(id);
    }
    assert_eq!(prompts.len(), 3, "{said}");
}

#[test]
fn a_session_that_cast_on_one_child_can_cast_on_its_sibling() {
    let world = World::new();
    std::fs::write(
        world.state.join("issues.json"),
        r#"{"parent-1":{"id":"parent-1","title":"parent","body":""},"child-a":{"id":"child-a","title":"child a","body":"","parent":"parent-1"},"child-b":{"id":"child-b","title":"child b","body":"","parent":"parent-1"}}"#,
    )
    .unwrap();
    std::fs::write(
        world.state.join("children.json"),
        r#"{"parent-1":[{"id":"child-a"},{"id":"child-b"}]}"#,
    )
    .unwrap();
    std::fs::write(
        world.state.join("notes.json"),
        r#"{"child-a":[{"note":"candidate-sig: child-a alice sess-author-0001 n-author - ab cd"}]}"#,
    )
    .unwrap();
    let author = world.ljos(
        "alice",
        "sess-author-0001",
        &["vote", "child-b", "--for", "ship", "--used", "none"],
    );
    let said = text(&author);
    assert!(!author.status.success(), "{said}");
    assert!(
        said.contains("authored candidate") && said.contains("Nothing was cast"),
        "{said}"
    );
    let first = world.ljos(
        "saddle",
        "sess-saddle-0001",
        &["vote", "child-a", "--for", "ship", "--used", "none"],
    );
    assert!(first.status.success(), "{}", text(&first));
    let second = world.ljos(
        "saddle",
        "sess-saddle-0001",
        &["vote", "child-b", "--for", "hold", "--used", "none"],
    );
    assert!(second.status.success(), "{}", text(&second));
    let tally = world.ljos("saddle", "sess-reader-0001", &["vote", "child-b"]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("1 ballot counts"), "{said}");
    assert!(stdout.contains("saddle  hold"), "{said}");
}

#[test]
fn a_persona_cast_then_withdraw_leaves_the_tally_at_zero() {
    let world = World::new();
    let issue = "demo-wd";
    let session = "sess-review-1";
    let cast = world.ljos(
        "clerk",
        session,
        &[
            "vote", issue, "--for", "ship", "--used", "none", "--as", "reviewer",
        ],
    );
    assert!(cast.status.success(), "{}", text(&cast));
    let before = world.ljos("clerk", "sess-reader-0001", &["vote", issue]);
    let said = text(&before);
    assert!(before.status.success(), "{said}");
    assert!(
        String::from_utf8_lossy(&before.stdout).contains("1 ballot counts"),
        "{said}"
    );
    let withdrawn = world.ljos(
        "clerk",
        session,
        &["vote", issue, "--withdraw", "--as", "reviewer"],
    );
    let said = text(&withdrawn);
    assert!(
        withdrawn.status.success() || said.contains("forecast"),
        "{said}"
    );
    assert!(!said.contains("unexpected vissue"), "{said}");
    let notes = std::fs::read_to_string(world.state.join("notes.json")).unwrap();
    assert!(notes.contains("ballot-withdrawn: reviewer"), "{notes}");
    let tally = world.ljos("clerk", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("0 ballots count"), "{said}");
    assert!(!stdout.contains("ship"), "{said}");
    // Another persona from the session is still not another voice.
    let other = world.ljos(
        "clerk",
        session,
        &[
            "vote", issue, "--for", "hold", "--used", "none", "--as", "editor",
        ],
    );
    let said = text(&other);
    assert!(!other.status.success(), "{said}");
    assert!(
        said.contains("withdrawn")
            && said.contains("first ballot stands")
            && said.contains("Nothing was cast"),
        "{said}"
    );
    // The persona that withdrew casts again, and the new ballot counts.
    let again = world.ljos(
        "clerk",
        session,
        &[
            "vote", issue, "--for", "hold", "--used", "none", "--as", "reviewer",
        ],
    );
    let said = text(&again);
    assert!(again.status.success(), "{said}");
    let tally = world.ljos("clerk", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("1 ballot counts"), "{said}");
    assert!(stdout.contains("reviewer  hold"), "{said}");
    assert!(!stdout.contains("ship"), "{said}");
}

#[test]
fn a_withdrawn_voter_recasting_the_same_option_counts_once() {
    let world = World::new();
    let issue = "demo-wd-same";
    let session = "sess-review-same";
    let cast = world.ljos(
        "clerk",
        session,
        &[
            "vote", issue, "--for", "ship", "--used", "none", "--as", "reviewer",
        ],
    );
    assert!(cast.status.success(), "{}", text(&cast));
    let withdrawn = world.ljos(
        "clerk",
        session,
        &["vote", issue, "--withdraw", "--as", "reviewer"],
    );
    let said = text(&withdrawn);
    assert!(
        withdrawn.status.success() || said.contains("forecast"),
        "{said}"
    );
    let recast = world.ljos(
        "clerk",
        session,
        &[
            "vote", issue, "--for", "ship", "--used", "none", "--as", "reviewer",
        ],
    );
    assert!(recast.status.success(), "{}", text(&recast));
    let tally = world.ljos("clerk", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("1 ballot counts"), "{said}");
    assert_eq!(stdout.matches("reviewer  ship").count(), 1, "{said}");
    let consensus = world.ljos("clerk", "sess-reader-0001", &["consensus", issue]);
    let said = text(&consensus);
    assert!(consensus.status.success(), "{said}");
    assert!(!said.contains("holds no ballots"), "{said}");
}

#[test]
fn a_voter_holding_a_ballot_is_told_to_withdraw_then_cast() {
    let world = World::new();
    let issue = "demo-held";
    let cast = world.ljos(
        "clerk",
        "sess-review-1",
        &[
            "vote", issue, "--for", "ship", "--used", "none", "--as", "reviewer",
        ],
    );
    assert!(cast.status.success(), "{}", text(&cast));
    for (session, choice) in [("sess-review-1", "ship"), ("sess-review-2", "hold")] {
        let again = world.ljos(
            "clerk",
            session,
            &[
                "vote", issue, "--for", choice, "--used", "none", "--as", "reviewer",
            ],
        );
        let said = text(&again);
        assert!(!again.status.success(), "{said}");
        assert!(
            said.contains("Nothing was cast")
                && said.contains("for ship")
                && said.contains(&format!("`ljos vote {issue} --withdraw --as reviewer`")),
            "{said}"
        );
        assert!(!said.contains("already voted"), "{said}");
    }
    let withdrawn = world.ljos(
        "clerk",
        "sess-review-2",
        &["vote", issue, "--withdraw", "--as", "reviewer"],
    );
    let said = text(&withdrawn);
    assert!(
        withdrawn.status.success() || said.contains("forecast"),
        "{said}"
    );
    let recast = world.ljos(
        "clerk",
        "sess-review-2",
        &[
            "vote", issue, "--for", "hold", "--used", "none", "--as", "reviewer",
        ],
    );
    assert!(recast.status.success(), "{}", text(&recast));
    let tally = world.ljos("clerk", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("1 ballot counts"), "{said}");
    assert!(stdout.contains("reviewer  hold"), "{said}");
}

#[test]
fn file_options_let_a_jev_ballot_read_them() {
    let world = World::new();
    let filed = world.ljos(
        "clerk",
        "sess-file-0001",
        &[
            "file",
            "Which cipher",
            "-p",
            "demo",
            "-t",
            "decision",
            "--options",
            "age,gpg",
        ],
    );
    let said = text(&filed);
    assert!(filed.status.success(), "{said}");
    let id = String::from_utf8_lossy(&filed.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    assert_eq!(id, "demo-x001", "{said}");
    let issues: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(world.state.join("issues.json")).unwrap())
            .unwrap();
    let body = issues[&id]["body"].as_str().unwrap_or("");
    assert!(body.starts_with("Options: age, gpg"), "{body}");
    let jev = world.ljos(
        "clerk",
        "sess-jev-0001",
        &["vote", &id, "--as", "reviewer", "--jev"],
    );
    let said = text(&jev);
    assert!(!said.contains("has no `Options:"), "{said}");
}

#[test]
fn options_appends_a_line_an_existing_issue_can_vote() {
    let world = World::new();
    let filed = world.ljos(
        "clerk",
        "sess-file-0001",
        &["file", "Which cipher", "-p", "demo", "-t", "decision"],
    );
    let said = text(&filed);
    assert!(filed.status.success(), "{said}");
    let id = String::from_utf8_lossy(&filed.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    let missing = world.ljos(
        "clerk",
        "sess-jev-0001",
        &["vote", &id, "--as", "reviewer", "--jev"],
    );
    assert!(
        text(&missing).contains("has no `Options:"),
        "{}",
        text(&missing)
    );
    let one = world.ljos("clerk", "sess-file-0001", &["options", &id, "age"]);
    let said = text(&one);
    assert!(!one.status.success(), "{said}");
    assert!(said.contains("two or more"), "{said}");
    let many = world.ljos("clerk", "sess-file-0001", &["options", &id, "age,gpg"]);
    assert!(many.status.success(), "{}", text(&many));
    let issues: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(world.state.join("issues.json")).unwrap())
            .unwrap();
    let body = issues[&id]["body"].as_str().unwrap_or("");
    assert!(body.contains("Options: age, gpg"), "{body}");
    let jev = world.ljos(
        "clerk",
        "sess-jev-0002",
        &["vote", &id, "--as", "reviewer", "--jev"],
    );
    assert!(!text(&jev).contains("has no `Options:"), "{}", text(&jev));
}

#[test]
fn runs_with_their_own_harness_ids_each_count_and_one_run_counts_once() {
    // One seat on every run, as a coordinator's seat is: only the harness
    // id tells the runs apart.
    type Env = &'static [(&'static str, &'static str)];
    let world = World::new();
    let issue = "demo-runs";
    let cloud: Env = &[("CURSOR_AGENT_ID", "bc-0a1b2c3d-run-a")];
    let runs: [(&str, Env, &str); 4] = [
        ("reviewer-a", cloud, "bc-0a1b2c3d-run-a"),
        (
            "reviewer-b",
            &[
                ("CLAUDE_CODE_SESSION_ID", "5f0c8e2a-run-b"),
                ("CLAUDE_CODE_BRIDGE_SESSION_ID", "session_bridge-b"),
            ],
            "5f0c8e2a-run-b",
        ),
        (
            "reviewer-c",
            &[
                ("OPENCODE_SESSION_ID", "ses_opencode-run-c"),
                ("CURSOR_CONVERSATION_ID", "conv-outer-0001"),
            ],
            "ses_opencode-run-c",
        ),
        (
            "reviewer-d",
            &[
                ("GITHUB_ACTIONS", "true"),
                ("GITHUB_RUN_ID", "18123456789"),
                ("CURSOR_AGENT_ID", "bc-not-the-run"),
            ],
            "18123456789",
        ),
    ];
    for (name, env, session) in runs {
        let cast = world.ljos_env(
            "ljosbot",
            env,
            &[
                "vote", issue, "--for", "ship", "--used", "none", "--as", name,
            ],
        );
        let said = text(&cast);
        assert!(cast.status.success(), "{said}");
        assert!(said.contains(&format!("session {session}")), "{said}");
        assert!(!said.contains("unverified"), "{said}");
    }
    for (name, choice) in [("reviewer-e", "ship"), ("reviewer-a", "hold")] {
        let again = world.ljos_env(
            "ljosbot",
            cloud,
            &[
                "vote", issue, "--for", choice, "--used", "none", "--as", name,
            ],
        );
        let said = text(&again);
        assert!(!again.status.success(), "{said}");
        assert!(
            said.contains("already cast") && said.contains("Nothing was cast"),
            "{said}"
        );
    }
    let tally = world.ljos("ljosbot", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("4 ballots count"), "{said}");
    for name in ["reviewer-a", "reviewer-b", "reviewer-c", "reviewer-d"] {
        assert!(stdout.contains(&format!("{name}  ship")), "{said}");
    }
    assert!(!stdout.contains("reviewer-e"), "{said}");
    assert!(!stdout.contains("ignored"), "{said}");
}

#[test]
fn subagents_of_one_session_each_count_and_one_subagent_counts_once() {
    // A coordinator in one Claude Code session starts its reviewers as
    // subagents. Every reviewer's shell carries the parent's session id;
    // only the hook payload names the agent.
    let world = World::new();
    let issue = "demo-subagents";
    let parent = "5f0c8e2a-parent";
    let env: &[(&str, &str)] = &[("CLAUDE_CODE_SESSION_ID", parent)];
    let hook = |tool_use: &str, agent: Option<&str>, args: &[&str]| {
        let mut payload = serde_json::json!({
            "session_id": parent,
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_use_id": tool_use,
            "tool_input": {"command": format!("ljos {}", args.join(" "))},
        });
        if let Some(agent) = agent {
            payload["agent_id"] = agent.into();
            payload["agent_type"] = "reviewer".into();
        }
        let out = world.hook(env, &payload);
        let said = text(&out);
        assert!(out.status.success(), "{said}");
        assert!(!said.contains("deny"), "{said}");
    };
    let cast = |tool_use: &str, agent: Option<&str>, name: &str, choice: &str| {
        let args = [
            "vote", issue, "--for", choice, "--used", "none", "--as", name,
        ];
        hook(tool_use, agent, &args);
        world.ljos_env("ljosbot", env, &args)
    };
    for (tool_use, agent, name) in [
        ("toolu_01a", Some("agent-a1"), "reviewer-a"),
        ("toolu_01b", Some("agent-b2"), "reviewer-b"),
    ] {
        let out = cast(tool_use, agent, name, "ship");
        let said = text(&out);
        assert!(out.status.success(), "{said}");
        let session = format!("session {parent}:{}\n", agent.unwrap());
        assert!(said.contains(&session), "{said}");
    }
    // A subagent call the hook saw but that never ran is not the parent's.
    let coordinator = [
        "vote",
        issue,
        "--for",
        "ship",
        "--used",
        "none",
        "--as",
        "coordinator",
    ];
    hook("toolu_01z", Some("agent-z9"), &coordinator);
    let out = cast("toolu_01p", None, "coordinator", "ship");
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(&format!("session {parent}\n")), "{said}");
    let again = cast("toolu_01c", Some("agent-a1"), "reviewer-c", "hold");
    let said = text(&again);
    assert!(!again.status.success(), "{said}");
    assert!(
        said.contains("already cast") && said.contains(&format!("({parent}:agent-a1)")),
        "{said}"
    );
    assert!(said.contains(ljos_cli::panel_gate::OWN_BALLOT), "{said}");
    let tally = world.ljos("ljosbot", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("3 ballots count"), "{said}");
    for name in ["reviewer-a", "reviewer-b", "coordinator"] {
        assert!(stdout.contains(&format!("{name}  ship")), "{said}");
    }
    assert!(!stdout.contains("reviewer-c"), "{said}");
    assert!(!stdout.contains("ignored"), "{said}");
}

#[test]
fn a_box_seat_with_no_session_counts_each_subagent() {
    // A seat on a shared box: the shell names the seat and no session.
    // The hook still sees the Claude Code session and each subagent, and
    // a Grok Bot subagent carries its own conversation id.
    let world = World::new();
    let issue = "demo-box-panel";
    let parent = "9b2d41f0-box-parent";
    let none: &[(&str, &str)] = &[];
    let cast = |tool_use: &str, agent: Option<&str>, name: &str, choice: &str| {
        let args = [
            "vote", issue, "--for", choice, "--used", "none", "--as", name,
        ];
        let mut payload = serde_json::json!({
            "session_id": parent,
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_use_id": tool_use,
            "tool_input": {"command": format!("ljos {}", args.join(" "))},
        });
        if let Some(agent) = agent {
            payload["agent_id"] = agent.into();
        }
        let hook = world.hook(none, &payload);
        assert!(hook.status.success(), "{}", text(&hook));
        world.ljos_env("eessi", none, &args)
    };
    for (tool_use, agent, name, session) in [
        (
            "toolu_b1",
            Some("agent-a1"),
            "reviewer-a",
            format!("{parent}:agent-a1"),
        ),
        (
            "toolu_b2",
            Some("agent-b2"),
            "reviewer-b",
            format!("{parent}:agent-b2"),
        ),
        ("toolu_b3", None, "coordinator", parent.to_string()),
    ] {
        let out = cast(tool_use, agent, name, "ship");
        let said = text(&out);
        assert!(out.status.success(), "{said}");
        assert!(said.contains(&format!("session {session}\n")), "{said}");
        assert!(!said.contains("unverified"), "{said}");
    }
    let again = cast("toolu_b4", Some("agent-a1"), "reviewer-c", "hold");
    let said = text(&again);
    assert!(!again.status.success(), "{said}");
    assert!(
        said.contains(&format!(
            "already cast a ballot on {issue} ({parent}:agent-a1)"
        )),
        "{said}"
    );
    for (conversation, name) in [
        ("sand-subagent-box-0001", "reviewer-d"),
        ("sand-subagent-box-0002", "reviewer-e"),
    ] {
        let out = world.ljos_env(
            "eessi",
            &[("CURSOR_CONVERSATION_ID", conversation)],
            &[
                "vote", issue, "--for", "ship", "--used", "none", "--as", name,
            ],
        );
        let said = text(&out);
        assert!(out.status.success(), "{said}");
        assert!(
            said.contains(&format!("session {conversation}\n")),
            "{said}"
        );
    }
    let tally = world.ljos("eessi", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("5 ballots count"), "{said}");
    assert!(!stdout.contains("reviewer-c"), "{said}");
    assert!(!stdout.contains("unverified"), "{said}");
}

#[test]
fn a_shared_shell_holder_is_not_a_ballot_session() {
    // A shell runner's env file gives every conversation on the box the
    // holder `grokbot-shell`. Two seats are two voices, each unverified,
    // and a subagent its hook saw still binds its own session.
    let world = World::new();
    let issue = "demo-shared-holder";
    let shared: &[(&str, &str)] = &[("LJOS_SESSION_ID", "grokbot-shell")];
    let args = |name: &'static str| {
        [
            "vote", issue, "--for", "ship", "--used", "none", "--as", name,
        ]
    };
    for (seat, name) in [("eessi", "reviewer-a"), ("inky", "reviewer-b")] {
        let out = world.ljos_env(seat, shared, &args(name));
        let said = text(&out);
        assert!(out.status.success(), "{said}");
        assert!(said.contains(&format!("session {seat}\n")), "{said}");
        let unverified = ljos_cli::ballot_sig::unverified_sentence(seat);
        assert!(said.contains(&unverified), "{said}");
    }
    let parent = "3c7a90e1-shared-parent";
    let line = args("reviewer-c");
    let hook = world.hook(
        shared,
        &serde_json::json!({
            "session_id": parent,
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_use_id": "toolu_s1",
            "agent_id": "agent-s1",
            "tool_input": {"command": format!("ljos {}", line.join(" "))},
        }),
    );
    assert!(hook.status.success(), "{}", text(&hook));
    let out = world.ljos_env("eessi", shared, &line);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains(&format!("session {parent}:agent-s1\n")),
        "{said}"
    );
    let tally = world.ljos("eessi", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("3 ballots count"), "{said}");
    assert!(!stdout.contains("grokbot-shell"), "{said}");
}

fn now_ms() -> u64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    u64::try_from(since.as_millis()).unwrap()
}

/// A call record as a hook leaves it, written straight into the runtime
/// directory under `session`.
fn forge_call(world: &World, session: &str, agent: &str, args: &[&str], tree: serde_json::Value) {
    let dir = world.home.join("run/ljos/agent-calls/shell").join(session);
    std::fs::create_dir_all(&dir).unwrap();
    let call = serde_json::json!({
        "session": session,
        "agent": agent,
        "argv": args,
        "at": now_ms(),
        "tree": tree,
    });
    std::fs::write(dir.join(format!("toolu_{agent}.0.json")), call.to_string()).unwrap();
}

#[test]
fn a_call_record_no_hook_above_this_shell_left_names_no_session() {
    // Records under agent-calls are not signed. One written by hand, or by
    // a hook in another process tree, must not bind a session: the seat
    // stays one unverified session and casts once.
    let world = World::new();
    let issue = "demo-forged-calls";
    let mut sibling = Command::new("sleep").arg("30").spawn().unwrap();
    let elsewhere = ljos_cli::proc_tree::of(sibling.id()).unwrap();
    let a = [
        "vote",
        issue,
        "--for",
        "ship",
        "--used",
        "none",
        "--as",
        "reviewer-a",
    ];
    let b = [
        "vote",
        issue,
        "--for",
        "ship",
        "--used",
        "none",
        "--as",
        "reviewer-b",
    ];
    forge_call(
        &world,
        "forged-sess-0001",
        "agent-x",
        &a,
        serde_json::json!([]),
    );
    forge_call(
        &world,
        "forged-sess-0002",
        "agent-y",
        &b,
        serde_json::json!([elsewhere]),
    );
    let unverified = ljos_cli::ballot_sig::unverified_sentence("eessi");
    let first = world.ljos_env("eessi", &[], &a);
    let said = text(&first);
    assert!(first.status.success(), "{said}");
    assert!(!said.contains("forged-sess"), "{said}");
    assert!(
        said.contains("session eessi") && said.contains(&unverified),
        "{said}"
    );
    let second = world.ljos_env("eessi", &[], &b);
    let said = text(&second);
    assert!(!second.status.success(), "{said}");
    assert!(
        said.contains("already cast") && said.contains("Nothing was cast"),
        "{said}"
    );
    sibling.kill().unwrap();
    sibling.wait().unwrap();
    let tally = world.ljos("eessi", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    assert!(!said.contains("2 ballots count"), "{said}");
    assert!(!said.contains("reviewer-b"), "{said}");
}

#[test]
fn a_line_run_again_after_its_call_was_taken_is_already_cast() {
    // A shell loop, or a line pasted twice, runs one hooked call more than
    // once. The second run binds the session the first took and is refused,
    // rather than casting again as the seat or as the parent.
    let world = World::new();
    let issue = "demo-replay";
    let parent = "7c1e93aa-replay-parent";
    let args = [
        "vote",
        issue,
        "--for",
        "ship",
        "--used",
        "none",
        "--as",
        "reviewer-a",
    ];
    let hook = |env: &[(&str, &str)], tool_use: &str, agent: &str| {
        let payload = serde_json::json!({
            "session_id": parent,
            "agent_id": agent,
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_use_id": tool_use,
            "tool_input": {"command": format!("ljos {}", args.join(" "))},
        });
        let out = world.hook(env, &payload);
        assert!(out.status.success(), "{}", text(&out));
    };
    let none: &[(&str, &str)] = &[];
    hook(none, "toolu_r1", "agent-r1");
    let first = world.ljos_env("eessi", none, &args);
    let said = text(&first);
    assert!(first.status.success(), "{said}");
    assert!(
        said.contains(&format!("session {parent}:agent-r1\n")),
        "{said}"
    );
    let again = world.ljos_env("eessi", none, &args);
    let said = text(&again);
    assert!(!again.status.success(), "{said}");
    assert!(
        said.contains(&format!(
            "already cast a ballot on {issue} ({parent}:agent-r1)"
        )) && said.contains("Nothing was cast"),
        "{said}"
    );

    let issue = "demo-replay-env";
    let args = [
        "vote",
        issue,
        "--for",
        "ship",
        "--used",
        "none",
        "--as",
        "reviewer-b",
    ];
    let named: &[(&str, &str)] = &[("SEAT_SESSION_ID", parent)];
    let payload = serde_json::json!({
        "session_id": parent,
        "agent_id": "agent-r2",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_use_id": "toolu_r2",
        "tool_input": {"command": format!("ljos {}", args.join(" "))},
    });
    let out = world.hook(named, &payload);
    assert!(out.status.success(), "{}", text(&out));
    let first = world.ljos_env("eessi", named, &args);
    let said = text(&first);
    assert!(first.status.success(), "{said}");
    assert!(
        said.contains(&format!("session {parent}:agent-r2\n")),
        "{said}"
    );
    let again = world.ljos_env("eessi", named, &args);
    let said = text(&again);
    assert!(!again.status.success(), "{said}");
    assert!(
        said.contains(&format!(
            "already cast a ballot on {issue} ({parent}:agent-r2)"
        )),
        "{said}"
    );
    for issue in ["demo-replay", "demo-replay-env"] {
        let tally = world.ljos("eessi", "sess-reader-0001", &["vote", issue]);
        let said = text(&tally);
        assert!(tally.status.success(), "{said}");
        assert!(
            String::from_utf8_lossy(&tally.stdout).contains("1 ballot counts"),
            "{said}"
        );
    }
}

/// `script` run by one `sh -c`, as one shell command of an agent runs,
/// with `$LJOS` the binary under test.
fn shell(world: &World, env: &[(&str, &str)], script: &str) -> Output {
    world
        .program("/bin/sh", "eessi", env, &["-c", script])
        .env("LJOS", env!("CARGO_BIN_EXE_ljos"))
        .output()
        .unwrap()
}

fn counted(world: &World, issue: &str) -> String {
    let tally = world.ljos("eessi", "sess-reader-0001", &["vote", issue]);
    assert!(tally.status.success(), "{}", text(&tally));
    String::from_utf8_lossy(&tally.stdout).into_owned()
}

#[test]
fn one_shell_that_renames_its_session_between_casts_casts_once() {
    // One shell casts, sets another session or run id, and casts again on
    // the same issue. Each variable that names a session is a way to try.
    let world = World::new();
    for (i, var) in [
        "LJOS_SESSION_ID",
        "CURSOR_AGENT_ID",
        "CLOUD_AGENT_ID",
        "CURSOR_CONVERSATION_ID",
    ]
    .iter()
    .enumerate()
    {
        let issue = format!("demo-flip-{i}");
        let script = format!(
            "{var}=sess-flip-a-000{i} \"$LJOS\" vote {issue} --for ship --used none --as reviewer-a; echo \"first $?\"\n\
             {var}=sess-flip-b-000{i} \"$LJOS\" vote {issue} --for ship --used none --as reviewer-b; echo \"second $?\"\n"
        );
        let out = shell(&world, &[], &script);
        let said = text(&out);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("session sess-flip-a-000"), "{var}: {said}");
        assert!(stdout.contains("first 0"), "{var}: {said}");
        assert!(!stdout.contains("second 0"), "{var}: {said}");
        assert!(
            said.contains(&format!(
                "this shell already cast a ballot on {issue} as session sess-flip-a-000{i}."
            )) && said.contains("Nothing was cast"),
            "{var}: {said}"
        );
        assert!(counted(&world, &issue).contains("1 ballot counts"), "{var}");
    }
    // Two agents, each running its own command in its own shell, count.
    let issue = "demo-flip-apart";
    for session in ["sess-apart-a-0001", "sess-apart-b-0002"] {
        let out = shell(
            &world,
            &[("LJOS_SESSION_ID", session)],
            &format!("\"$LJOS\" vote {issue} --for ship --used none --as reviewer-{session}"),
        );
        assert!(out.status.success(), "{}", text(&out));
    }
    assert!(counted(&world, issue).contains("2 ballots count"));
}

#[test]
fn a_hooked_harness_that_renames_its_session_casts_once() {
    // Separate commands under one harness whose hook saw each line. A
    // subagent of the hook's session counts; a command that names another
    // root session in its environment is the same harness and is refused.
    let world = World::new();
    let issue = "demo-flip-harness";
    let parent = "4f0a6c2e-flip-parent";
    let none: &[(&str, &str)] = &[];
    let cast = |tool_use: &str, agent: Option<&str>, name: &str, env: &[(&str, &str)]| {
        let args = [
            "vote", issue, "--for", "ship", "--used", "none", "--as", name,
        ];
        let mut payload = serde_json::json!({
            "session_id": parent,
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_use_id": tool_use,
            "tool_input": {"command": format!("ljos {}", args.join(" "))},
        });
        if let Some(agent) = agent {
            payload["agent_id"] = agent.into();
        }
        let hook = world.hook(none, &payload);
        assert!(hook.status.success(), "{}", text(&hook));
        world.ljos_env("eessi", env, &args)
    };
    let out = cast("toolu_f1", None, "reviewer-a", none);
    assert!(out.status.success(), "{}", text(&out));
    let out = cast("toolu_f2", Some("agent-f2"), "reviewer-b", none);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains(&format!("session {parent}:agent-f2\n")),
        "{said}"
    );
    let out = cast(
        "toolu_f3",
        None,
        "reviewer-c",
        &[("LJOS_SESSION_ID", "sess-flip-other-0001")],
    );
    let said = text(&out);
    assert!(!out.status.success(), "{said}");
    assert!(
        said.contains(&format!(
            "the harness running this line already cast a ballot on {issue} as session {parent}."
        )) && said.contains("Nothing was cast"),
        "{said}"
    );
    let tally = counted(&world, issue);
    assert!(tally.contains("2 ballots count"), "{tally}");
    assert!(!tally.contains("reviewer-c"), "{tally}");
}

#[test]
fn claude_subagents_voting_over_mcp_each_count() {
    // One ljos-mcp answers the parent and both subagents. Only the hook
    // payload names the agent; the call carries the tool use id.
    let world = World::new();
    let issue = "demo-mcp-subagents";
    let parent = "5f0c8e2a-mcp-parent";
    let env: &[(&str, &str)] = &[("CLAUDE_CODE_SESSION_ID", parent)];
    for (tool_use, agent) in [
        ("toolu_m1", "agent-m1"),
        ("toolu_m2", "agent-m2"),
        ("toolu_m4", "agent-m1"),
    ] {
        let hook = world.hook(
            env,
            &serde_json::json!({
                "session_id": parent,
                "hook_event_name": "PreToolUse",
                "tool_name": "mcp__plugin_ljos_ljos__ljos_vote",
                "tool_use_id": tool_use,
                "agent_id": agent,
                "agent_type": "reviewer",
                "tool_input": {"issue": issue, "choice": "ship"},
            }),
        );
        assert!(hook.status.success(), "{}", text(&hook));
        assert!(hook.stdout.is_empty(), "{}", text(&hook));
    }
    let vote = |id: u64, tool_use: &str, name: &str, choice: &str| {
        serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {
            "name": "ljos_vote",
            "arguments": {"issue": issue, "choice": choice, "used": "none", "as": name},
            "_meta": {"claudecode/toolUseId": tool_use, "progressToken": id}}})
    };
    // A server answers a batch in any order; each call waits for the last.
    let answers: Vec<serde_json::Value> = [
        vote(3, "toolu_m1", "reviewer-a", "ship"),
        vote(4, "toolu_m2", "reviewer-b", "ship"),
        vote(5, "toolu_m3", "coordinator", "ship"),
        vote(6, "toolu_m4", "reviewer-c", "hold"),
        vote(7, "toolu_m1", "reviewer-d", "hold"),
    ]
    .iter()
    .flat_map(|call| world.mcp(env, std::slice::from_ref(call)))
    .collect();
    let answer = |id: u64| {
        let found = answers
            .iter()
            .find(|a| a["id"] == id)
            .unwrap_or_else(|| panic!("no answer {id}: {answers:?}"));
        let said = found["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        let said = serde_json::from_str::<serde_json::Value>(said)
            .ok()
            .and_then(|v| v["text"].as_str().map(str::to_string))
            .unwrap_or_else(|| said.to_string());
        (found["result"]["isError"] == true, said)
    };
    for (id, session) in [
        (3, format!("{parent}:agent-m1")),
        (4, format!("{parent}:agent-m2")),
        (5, parent.to_string()),
    ] {
        let (error, said) = answer(id);
        assert!(!error, "{said}");
        assert!(said.contains(&format!("session {session}\n")), "{said}");
    }
    let (error, said) = answer(6);
    assert!(
        error
            && said.contains(&format!(
                "already cast a ballot on {issue} ({parent}:agent-m1)"
            )),
        "{said}"
    );
    let (error, said) = answer(7);
    assert!(
        error && said.contains(&format!("({parent})")),
        "a tool use id names its agent once: {said}"
    );
    let tally = world.ljos("ljosbot", "sess-reader-0001", &["vote", issue]);
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("3 ballots count"), "{}", text(&tally));
    assert!(!stdout.contains("reviewer-c") && !stdout.contains("reviewer-d"));
}

#[test]
fn codex_subagent_threads_each_count_under_their_root_session() {
    // Codex gives a subagent's shell its own thread id and the root's
    // session id. The root thread's id is the session's.
    let world = World::new();
    let issue = "demo-codex-threads";
    let root = "019a7c3e-root-0001";
    let runs = [
        (
            "reviewer-a",
            "019a7c3e-thread-a",
            format!("{root}:019a7c3e-thread-a"),
        ),
        (
            "reviewer-b",
            "019a7c3e-thread-b",
            format!("{root}:019a7c3e-thread-b"),
        ),
        ("coordinator", root, root.to_string()),
    ];
    let cast = |name: &str, thread: &str, choice: &str| {
        world.ljos_env(
            "ljosbot",
            &[("CODEX_SESSION_ID", root), ("CODEX_THREAD_ID", thread)],
            &[
                "vote", issue, "--for", choice, "--used", "none", "--as", name,
            ],
        )
    };
    for (name, thread, session) in &runs {
        let out = cast(name, thread, "ship");
        let said = text(&out);
        assert!(out.status.success(), "{said}");
        assert!(said.contains(&format!("session {session}\n")), "{said}");
    }
    let again = cast("reviewer-c", "019a7c3e-thread-a", "hold");
    let said = text(&again);
    assert!(!again.status.success(), "{said}");
    assert!(said.contains("already cast"), "{said}");
    let tally = world.ljos("ljosbot", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("3 ballots count"), "{said}");
    assert!(!stdout.contains("reviewer-c"), "{said}");
}

#[test]
fn a_seat_no_harness_names_is_an_unverified_session() {
    // Two reviewers recorded from one coordinator that no harness id and
    // no run id names: the seat is the session, and every output says so.
    let world = World::new();
    let issue = "demo-seat";
    let unverified = ljos_cli::ballot_sig::unverified_sentence("ljosbot");
    let first = world.ljos_env(
        "ljosbot",
        &[],
        &[
            "vote",
            issue,
            "--for",
            "ship",
            "--used",
            "none",
            "--as",
            "reviewer-a",
        ],
    );
    let said = text(&first);
    assert!(first.status.success(), "{said}");
    assert!(
        said.contains("session ljosbot") && said.contains(&unverified),
        "{said}"
    );
    let second = world.ljos_env(
        "ljosbot",
        &[],
        &[
            "vote",
            issue,
            "--for",
            "hold",
            "--used",
            "none",
            "--as",
            "reviewer-b",
        ],
    );
    let said = text(&second);
    assert!(!second.status.success(), "{said}");
    assert!(
        said.contains("already cast")
            && said.contains(&unverified)
            && said.contains("Nothing was cast"),
        "{said}"
    );
    // A ballot that went around the cast gate, as `vissue vote` and a later
    // signature on the same seat leave it.
    let ballots_path = world.state.join("ballots.json");
    let mut ballots: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&ballots_path).unwrap()).unwrap();
    ballots[issue]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"agent": "reviewer-b", "choice": "hold"}));
    std::fs::write(&ballots_path, ballots.to_string()).unwrap();
    let notes_path = world.state.join("notes.json");
    let mut notes: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&notes_path).unwrap()).unwrap();
    let key = ed25519_dalek::SigningKey::from_bytes(&[12; 32]);
    let sig = ljos_cli::ballot_sig::sign_bound(
        &key,
        issue,
        "reviewer-b",
        "hold",
        "ljosbot",
        ljos_cli::ballot_sig::SEAT_NONCE,
    );
    notes[issue]
        .as_array_mut()
        .unwrap()
        .insert(0, serde_json::json!({ "note": sig }));
    std::fs::write(&notes_path, notes.to_string()).unwrap();
    let tally = world.ljos("ljosbot", "sess-reader-0001", &["vote", issue]);
    let said = text(&tally);
    assert!(tally.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&tally.stdout);
    assert!(stdout.contains("1 ballot counts"), "{said}");
    assert!(
        stdout.contains("reviewer-a  ship  (unverified session)"),
        "{said}"
    );
    assert!(
        stdout.contains("reviewer-b  ljosbot  (same session, unverified)"),
        "{said}"
    );
    assert!(stdout.contains(&unverified), "{said}");
}
