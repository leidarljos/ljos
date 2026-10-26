//! A split panel stays open for as many rounds as it is read. The round
//! count used to mark the ticket DONE and commit `settled with notes`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir)
        .output()
        .unwrap()
}

fn git_ok(dir: &Path, args: &[&str]) -> String {
    let out = git(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

struct Split {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    root: PathBuf,
    home: PathBuf,
    id: String,
}

impl Split {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tracker");
        let home = dir.path().join("home");
        let bin = dir.path().join("bin");
        let board_dir = root.join("Software/probe");
        std::fs::create_dir_all(&board_dir).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let id = "proj-s001";
        let board = board_dir.join("issues.org");
        let state = dir.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(
            state.join("issues.json"),
            serde_json::json!({
                id: {"id": id, "title": "where the bytes go", "state": "TODO"}
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(state.join("notes.json"), "{}").unwrap();
        let org = format!(
            "\
#+VISSUE: 1\n\
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\
\n\
* TODO [#C] where the bytes go\n\
:PROPERTIES:\n\
:ID:         {id}\n\
:END:\n"
        );
        std::fs::write(&board, &org).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "seat@example.invalid"]);
        git(&root, &["config", "user.name", "seat"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(&root, &["config", "core.hooksPath", "/dev/null"]);
        git(&root, &["add", "Software/probe/issues.org"]);
        git(&root, &["commit", "-q", "-m", "seed"]);
        let state_s = state.display().to_string();
        let board_s = board.display().to_string();
        script(
            &bin,
            "vissue",
            &format!(
                r##"#!/usr/bin/env python3
import json, os, sys
state = {state_s:?}
board = {board_s:?}

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

def render():
    issues = load("issues.json", {{}})
    notes = load("notes.json", {{}})
    lines = ["#+VISSUE: 1", "#+TODO: TODO STARTED BLOCKED | DONE CANCELLED", ""]
    for ident, issue in issues.items():
        lines.append("* %s [#C] %s" % (issue.get("state", "TODO"), issue["title"]))
        lines.append(":PROPERTIES:")
        lines.append(":ID:         %s" % ident)
        if issue.get("parent"):
            lines.append(":PARENT:     %s" % issue["parent"])
        lines.append(":END:")
        book = notes.get(ident, [])
        if book:
            lines.append(":LOGBOOK:")
            for note in book:
                lines.append('- Note: "%s" [2026-10-26 Mon 12:00]' % note)
            lines.append(":END:")
        lines.append("")
    text = "\n".join(lines)
    tmp = board + ".tmp"
    with open(tmp, "w") as f:
        f.write(text)
    os.replace(tmp, board)

args = sys.argv[1:]
with open(os.path.join(state, "calls.log"), "a") as log:
    log.write(" ".join(args) + "\n")
cmd = args[0] if args else ""
issues = load("issues.json", {{}})
if cmd == "vote":
    print(json.dumps(load("ballots.json", [])))
elif cmd == "show":
    ident = args[1]
    meta = dict(issues.get(ident, {{"id": ident, "title": ident, "state": "TODO"}}))
    meta["logbook"] = [{{"note": n}} for n in load("notes.json", {{}}).get(ident, [])]
    print(json.dumps(meta))
elif cmd == "note":
    ident = args[1]
    notes = load("notes.json", {{}})
    notes.setdefault(ident, []).insert(0, " ".join(args[2:]))
    save("notes.json", notes)
    render()
    print(ident + ": noted")
elif cmd == "children":
    kids = [v for v in issues.values() if v.get("parent") == args[1]]
    print(json.dumps([{{"id": k["id"], "title": k["title"], "state": k["state"]}} for k in kids]))
elif cmd == "q":
    title = args[-1]
    ident = "proj-k%03d" % len(issues)
    parent = args[args.index("--parent") + 1] if "--parent" in args else ""
    issues[ident] = {{"id": ident, "title": title, "state": "TODO", "parent": parent}}
    save("issues.json", issues)
    render()
    print(ident)
elif cmd == "update":
    ident = args[1]
    if "-s" in args:
        issues.setdefault(ident, {{"id": ident, "title": ident, "state": "TODO"}})["state"] = args[args.index("-s") + 1]
        save("issues.json", issues)
        render()
    print(ident + ": updated")
else:
    sys.stderr.write("unexpected vissue " + " ".join(args) + "\n")
    sys.exit(1)
"##,
            ),
        );
        script(
            &bin,
            "ljos-consensus",
            "#!/bin/sh\ncase \"$1\" in\n  settle) echo '{\"settled\":true}' ;;\n  *) echo '{}' ;;\nesac\n",
        );
        std::fs::write(
            state.join("ballots.json"),
            serde_json::json!([
                {"agent": "alice", "choice": "ship"},
                {"agent": "bob", "choice": "ship"},
                {"agent": "carol", "choice": "wait"},
                {"agent": "dave", "choice": "drop"}
            ])
            .to_string(),
        )
        .unwrap();
        Self {
            bin,
            root,
            home,
            id: id.to_string(),
            _dir: dir,
        }
    }

    fn consensus(&self) -> Output {
        let path = format!("{}:/usr/bin:/bin", self.bin.display());
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["consensus", &self.id, "--json"])
            .current_dir(&self.bin)
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", &self.home)
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("PACKSET_URL", "off")
            .env("LJOS_TRACKER_GIT", "commit")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_UNSIGNED_BALLOTS", "allow")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap()
    }

    fn org(&self) -> String {
        std::fs::read_to_string(self.root.join("Software/probe/issues.org")).unwrap()
    }

    fn heading_state(&self) -> String {
        let org = self.org();
        let lines: Vec<&str> = org.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let Some(rest) = line.trim().strip_prefix(":ID:") else {
                continue;
            };
            if rest.split_whitespace().next() != Some(self.id.as_str()) {
                continue;
            }
            for prev in lines[..=i].iter().rev() {
                let Some(rest) = prev.trim().strip_prefix('*') else {
                    continue;
                };
                return rest.split_whitespace().next().unwrap_or("").to_string();
            }
        }
        String::new()
    }
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn split_seats_stay_open_for_ten_rounds_and_no_commit_changes_state() {
    let split = Split::new();
    assert_eq!(split.heading_state(), "TODO");
    let mut named = String::new();
    for n in 1..=10 {
        let out = split.consensus();
        let text = said(&out);
        assert!(out.status.success(), "round {n}: {text}");
        assert_eq!(split.heading_state(), "TODO", "round {n}: {text}");
        assert!(!text.contains("closed as"), "round {n}: {text}");
        assert!(!text.contains("settled-with-notes"), "round {n}: {text}");
        assert!(
            text.contains(&format!("seats still split at round {n}")),
            "round {n}: {text}"
        );
        if n == 1 {
            named = text.clone();
        }
    }
    assert!(
        named.contains("open objections: proj-k"),
        "the first reading names an open objection child: {named}"
    );
    assert!(split.org().contains("Dissent holds"), "{}", split.org());
    let subjects = git_ok(&split.root, &["log", "--format=%s"]);
    assert!(
        subjects
            .lines()
            .all(|line| !line.contains("settled with notes")),
        "{subjects}"
    );
    let patch = git_ok(
        &split.root,
        &["log", "-p", "--", "Software/probe/issues.org"],
    );
    assert!(
        !patch.contains("+* DONE"),
        "a tracker commit changed the state:\n{patch}"
    );
    assert_eq!(split.heading_state(), "TODO");
}
