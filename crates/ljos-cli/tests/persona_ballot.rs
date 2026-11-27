//! A persona other than the casting seat is its own voter.
//! One session may cast many of them. The same persona twice is a
//! repeated ballot. `--used none` is refused.

ljos_test_boot::harness!();

use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    ljos_cli::install_executable(&dir.join(name), body).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts git outside git hook code"
    )]
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_COUNT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn personas_from_one_seat_each_count() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let home = dir.path().join("home");
    let root = dir.path().join("tracker");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&state).unwrap();
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
def load(path):
    return json.loads(path.read_text()) if path.exists() else {{}}
def save(path, value):
    path.write_text(json.dumps(value))
args = sys.argv[1:]
cmd = args[0] if args else ""
issue = args[1] if len(args) > 1 else ""
agent = os.environ.get("VISSUE_AGENT", "")
if cmd == "vote" and "--json" in args:
    print(json.dumps(load(ballots).get(issue, [])))
elif cmd == "vote" and "--withdraw" in args:
    rows = load(ballots)
    rows[issue] = [r for r in rows.get(issue, []) if r.get("agent") != agent]
    save(ballots, rows)
    print("withdrawn")
elif cmd == "vote" and "--for" in args:
    choice = args[args.index("--for") + 1]
    rows = load(ballots)
    kept = [r for r in rows.get(issue, []) if r.get("agent") != agent]
    kept.append({{"agent": agent, "choice": choice}})
    rows[issue] = kept
    save(ballots, rows)
    print("ok")
elif cmd == "show":
    kept = load(notes).get(issue, [])
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
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nprintf '%s\\n' '{{\"options\":[\"nit\",\"hold\"],\"shares\":[0.5,0.5],\"settled\":false}}'\n",
            consensus_log.display()
        ),
    );
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let ljos = |args: &[&str]| {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
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
            .env("LJOS_SEAT", "tls")
            .env("VISSUE_AGENT", "tls")
            .env("SEAT_SESSION_ID", "sess-tls-0001")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), said)
    };

    let issue = "demo-y1fz";
    let (ok, said) = ljos(&[
        "vote", issue, "--for", "nit", "--used", "none", "--as", "tls-rev1",
    ]);
    assert!(!ok, "{said}");
    assert!(
        said.contains("a ballot cast as tls-rev1 needs --used naming a deed"),
        "{said}"
    );
    assert_eq!(
        said.lines()
            .filter(|line| line.contains("needs --used naming a deed"))
            .count(),
        1,
        "{said}"
    );
    let bare = std::fs::read_to_string(state.join("ballots.json")).unwrap_or_default();
    assert!(!bare.contains("tls-rev1"), "{bare}");

    let (ok, said) = ljos(&[
        "vote",
        issue,
        "--for",
        "nit",
        "--used",
        "deed-rev1",
        "--as",
        "tls-rev1",
    ]);
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: tls-rev1 for nit"),
        "{said}"
    );
    assert!(said.contains("tls-rev1 cast by tls"), "{said}");
    assert!(
        !said.contains("recorded on the tracker: tls for nit"),
        "{said}"
    );

    let (ok, said) = ljos(&[
        "vote",
        issue,
        "--for",
        "hold",
        "--used",
        "deed-rev2",
        "--as",
        "tls-rev2",
    ]);
    assert!(ok, "{said}");
    assert!(said.contains("tls-rev2 cast by tls"), "{said}");

    let (ok, said) = ljos(&["vote", issue]);
    assert!(ok, "{said}");
    assert!(said.contains("2 ballots count"), "{said}");
    assert!(said.contains("tls-rev1  nit"), "{said}");
    assert!(said.contains("tls-rev2  hold"), "{said}");
    assert!(!said.contains("folded into one voice"), "{said}");

    let (ok, said) = ljos(&["consensus", issue, "--no-file", "--json"]);
    assert!(ok, "{said}");
    let logged = std::fs::read_to_string(&consensus_log).unwrap_or_default();
    assert!(logged.contains("tls-rev1"), "{logged}");
    assert!(logged.contains("tls-rev2"), "{logged}");
    assert!(said.contains("tls-rev1 cast by tls"), "{said}");
    assert!(said.contains("tls-rev2 cast by tls"), "{said}");

    let (ok, said) = ljos(&[
        "vote",
        issue,
        "--for",
        "pass",
        "--used",
        "deed-rev1",
        "--as",
        "tls-rev1",
    ]);
    assert!(ok, "{said}");
    assert!(said.contains("label repeated-ballot"), "{said}");
    assert!(said.contains("Nothing was cast"), "{said}");

    let (_ok, withdrawn) = ljos(&["vote", issue, "--withdraw", "--as", "tls-rev1"]);
    assert!(
        withdrawn.contains("withdrawn") || withdrawn.contains("tls-rev1"),
        "{withdrawn}"
    );
    let (ok, said) = ljos(&[
        "vote",
        issue,
        "--for",
        "pass",
        "--used",
        "deed-rev1b",
        "--as",
        "tls-rev1",
    ]);
    assert!(ok, "{said}");
    assert!(!said.contains("label repeated-ballot"), "{said}");
    let (ok, said) = ljos(&["vote", issue]);
    assert!(ok, "{said}");
    assert!(said.contains("tls-rev1  pass"), "{said}");
    assert!(said.contains("tls-rev2  hold"), "{said}");
    assert!(said.contains("2 ballots count"), "{said}");
}
