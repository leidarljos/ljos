//! One harness session is one voice. Replays the failure where one worker
//! minted N seats and cast every ballot, and a run where each seat is its
//! own session.

use std::path::{Path, PathBuf};
use std::process::Output;

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
    title = args[-1] if args else ""
    if "--parent" in args:
        parent = args[args.index("--parent") + 1]
    if title.startswith("design: mine"):
        ident = "demo-a"
    elif title.startswith("design:"):
        ident = "demo-d001"
    else:
        ident = "demo-x001"
    issues = load("issues.json", {{}})
    issues[ident] = {{"id": ident, "title": title, "body": ""}}
    save("issues.json", issues)
    if parent:
        kids = load("children.json", {{}})
        kids.setdefault(parent, []).append({{"id": ident}})
        save("children.json", kids)
    print(ident)
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
        let path = format!(
            "{}:{}",
            self.bin.display(),
            ljos_cli::process_env::var("PATH").unwrap_or_default()
        );
        let mut cmd = ljos_cli::process_env::command(env!("CARGO_BIN_EXE_ljos"));
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
            .env("VISSUE_AGENT", seat);
        if !session.is_empty() {
            cmd.env("SEAT_SESSION_ID", session);
        }
        cmd.output().unwrap()
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
fn one_sand_subagent_five_personas_count_as_the_first_ballot() {
    let world = World::new();
    let session = "sand-subagent-fcdb9c57-9988-7d91-4311-9fc5e42f7529";
    let personas = ["academic", "transit", "safety", "cat-owner", "finance"];
    let issue = "travel-rx1d";
    // The drawer as the live tracker held it: five personas, then the same
    // five again, one session on every ballot-sig. Newest note first.
    let mut notes = Vec::new();
    for persona in personas.iter().rev() {
        notes.push(serde_json::json!({"note": format!("ballot-sig: {persona} bus ab cd {session} n-{persona}-2")}));
    }
    for persona in personas.iter().rev() {
        notes.push(serde_json::json!({"note": format!("ballot-sig: {persona} rail ab cd {session} n-{persona}-1")}));
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
    for seat in ["alice", "bob", "carol"] {
        vote(&world, seat, &format!("sess-{seat}-0001"), "demo-ind2");
    }
    let out = world.ljos("alice", "sess-alice-0001", &["consensus", "demo-ind2"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("decides ship for a push by alice"),
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
    for seat in ["alice", "bob", "carol"] {
        vote(&world, seat, &format!("sess-{seat}-0001"), "demo-blind");
    }
    let out = world.ljos("alice", "sess-alice-0001", &["consensus", "demo-blind"]);
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

    let nested = ljos_cli::process_env::command(env!("CARGO_BIN_EXE_ljos"))
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
                ljos_cli::process_env::var("PATH").unwrap_or_default()
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

    let spawned = ljos_cli::process_env::command(env!("CARGO_BIN_EXE_ljos"))
        .args(["panel", "run", "demo-brief", "--designers", "3"])
        .current_dir(&world.bin)
        .env_clear()
        .env(
            "PATH",
            format!(
                "{}:{}",
                world.bin.display(),
                ljos_cli::process_env::var("PATH").unwrap_or_default()
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
