//! The closer reads the panel round from the tracker. Replays the panel
//! eval on 7e21d3c, where twelve panels titled with their question read as
//! round 0 and stayed TODO after three rounds.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Tracker {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    home: PathBuf,
    state: PathBuf,
}

impl Tracker {
    /// One open issue `id` titled `title`, holding `ballots` as
    /// `(agent, choice)`.
    fn new(id: &str, title: &str, ballots: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(home.join("tracker")).unwrap();
        let rows: Vec<serde_json::Value> = ballots
            .iter()
            .map(|(agent, choice)| serde_json::json!({"agent": agent, "choice": choice}))
            .collect();
        std::fs::write(
            state.join("ballots.json"),
            serde_json::json!({ id: rows }).to_string(),
        )
        .unwrap();
        std::fs::write(
            state.join("issues.json"),
            serde_json::json!({ id: {"id": id, "title": title, "state": "TODO", "project": "proj"} })
                .to_string(),
        )
        .unwrap();
        let state_s = state.display().to_string();
        script(
            dir.path(),
            "vissue",
            &format!(
                r#"#!/usr/bin/env python3
import json, os, sys
state = {state_s:?}

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
issues = load("issues.json", {{}})
if cmd == "vote":
    print(json.dumps(load("ballots.json", {{}}).get(args[1], [])))
elif cmd == "show":
    ident = args[1]
    meta = dict(issues.get(ident, {{"id": ident, "title": ident, "state": "TODO"}}))
    meta["logbook"] = load("notes.json", {{}}).get(ident, [])
    print(json.dumps(meta))
elif cmd == "note":
    ident = args[1]
    notes = load("notes.json", {{}})
    notes.setdefault(ident, []).insert(0, {{"note": " ".join(args[2:])}})
    save("notes.json", notes)
    print(ident + ": noted")
elif cmd == "children":
    kids = load("children.json", {{}}).get(args[1], [])
    print(json.dumps([{{"id": k, "title": issues[k]["title"]}} for k in kids]))
elif cmd == "q":
    title = args[-1]
    ident = "proj-k%03d" % len(issues)
    issues[ident] = {{"id": ident, "title": title, "state": "TODO", "project": "proj"}}
    save("issues.json", issues)
    if "--parent" in args:
        parent = args[args.index("--parent") + 1]
        kids = load("children.json", {{}})
        kids.setdefault(parent, []).append(ident)
        save("children.json", kids)
    print(ident)
elif cmd == "update":
    ident = args[1]
    if "-s" in args:
        issues.setdefault(ident, {{"id": ident, "title": ident}})["state"] = args[args.index("-s") + 1]
        save("issues.json", issues)
    print(ident + ": updated")
else:
    sys.stderr.write("unexpected vissue " + " ".join(args) + "\n")
    sys.exit(1)
"#,
            ),
        );
        script(
            dir.path(),
            "ljos-consensus",
            "#!/bin/sh\ncase \"$1\" in\n  settle) echo '{\"settled\":true}' ;;\n  *) echo '{}' ;;\nesac\n",
        );
        Self {
            bin: dir.path().to_path_buf(),
            home,
            state,
            _dir: dir,
        }
    }

    fn consensus(&self, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .arg("consensus")
            .args(args)
            .current_dir(&self.bin)
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", &self.home)
            .env("VISSUE_ROOT", self.home.join("tracker"))
            .env("PACKSET_URL", "off")
            .env("LJOS_TRACKER_GIT", "off")
            // A consensus that may file a dissent is a tracker write.
            .env("LJOS_SEAT", "saddle")
            // The stub tracker holds no signature notes; signed ballots
            // have their own suite.
            .env("LJOS_UNSIGNED_BALLOTS", "allow")
            .output()
            .unwrap()
    }

    fn notes(&self, id: &str) -> Vec<String> {
        let raw = std::fs::read_to_string(self.state.join("notes.json")).unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
        v[id]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n["note"].as_str().map(str::to_string))
            .collect()
    }

    /// The `panel-round: N` notes on `id`, oldest first.
    fn rounds(&self, id: &str) -> Vec<usize> {
        let mut out: Vec<usize> = self
            .notes(id)
            .iter()
            .filter_map(|n| n.strip_prefix("panel-round:")?.trim().parse().ok())
            .collect();
        out.reverse();
        out
    }

    fn state_of(&self, id: &str) -> String {
        let raw = std::fs::read_to_string(self.state.join("issues.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        v[id]["state"].as_str().unwrap_or("").to_string()
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

#[test]
fn a_panel_titled_with_its_question_stays_open_past_round_three() {
    // log-sink (panel-a7o4 in the eval): keep held stderr, weigh and share
    // chose file. The round count used to mark the issue DONE.
    let id = "proj-a7o4";
    let tracker = Tracker::new(
        id,
        "log-sink: where the log goes",
        &[("keep", "stderr"), ("weigh", "file"), ("share", "file")],
    );
    // A read sits no round.
    let read = tracker.consensus(&[id, "--json", "--no-file"]);
    assert!(read.status.success(), "{}", text(&read));
    assert!(tracker.rounds(id).is_empty(), "{:?}", tracker.notes(id));
    for n in 1..=4 {
        let out = tracker.consensus(&[id, "--json"]);
        let said = text(&out);
        assert!(out.status.success(), "{said}");
        assert_eq!(tracker.rounds(id), (1..=n).collect::<Vec<_>>(), "{said}");
        assert_eq!(tracker.state_of(id), "TODO", "{said}");
        assert!(!said.contains("closed as"), "{said}");
        assert!(!said.contains("settled-with-notes"), "{said}");
    }
    // The minority is one `Dissent holds` child, never a `fix:` child.
    let calls = std::fs::read_to_string(tracker.state.join("calls.log")).unwrap();
    assert_eq!(calls.matches("Dissent holds stderr").count(), 1, "{calls}");
    assert!(!calls.contains("fix:"), "{calls}");
    assert!(!calls.contains("update"), "{calls}");
}

#[test]
fn a_panel_r3_title_is_still_round_three_and_stays_open() {
    // A `panel r3` title is round 3 with no round note. It used to close
    // on that first reading.
    let id = "proj-hrd5";
    let tracker = Tracker::new(
        id,
        "panel r3: cache place",
        &[
            ("seat-a", "memory"),
            ("seat-b", "disk"),
            ("seat-c", "memory"),
        ],
    );
    let out = tracker.consensus(&[id, "--json"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert_eq!(tracker.state_of(id), "TODO", "{said}");
    assert!(!said.contains("closed as"), "{said}");
    assert!(!said.contains("settled-with-notes"), "{said}");
    assert!(tracker.rounds(id).is_empty(), "{:?}", tracker.notes(id));
}
