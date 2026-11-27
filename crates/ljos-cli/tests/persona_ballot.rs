//! A persona other than the casting seat is its own voter.
//! One session may cast many of them. The same persona twice, under any
//! spelling of that slug, is a repeated ballot. `--used` has to name a deed.

ljos_test_boot::harness!();

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn script(dir: &Path, name: &str, body: &str) {
    ljos_cli::install_executable(&dir.join(name), body).unwrap();
}

fn write_text(path: &Path, body: impl AsRef<[u8]>) {
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(path, body).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let _ = git_text(dir, args);
}

fn git_text(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn deed_sentence(said: &str, name: &str) {
    let line = format!("a ballot cast as {name} needs --used naming a deed");
    assert!(said.contains(&line), "{said}");
    assert_eq!(
        said.lines()
            .filter(|row| row.contains(line.as_str()))
            .count(),
        1,
        "{said}"
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
    used = args[args.index("--used") + 1] if "--used" in args else ""
    rows = load(ballots)
    kept = [r for r in rows.get(issue, []) if r.get("agent") != agent]
    kept.append({{"agent": agent, "choice": choice, "used": used}})
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
    script(
        &bin,
        "deedar",
        r#"#!/usr/bin/env python3
import sys
known = {
    "deed-rev1", "deed-rev2", "deed-rev1b", "deed-alice", "deed-bob",
    "deed-after", "deed-disk", "deed-reviewer",
}
if len(sys.argv) >= 3 and sys.argv[1] == "current" and sys.argv[2] in known:
    print(f"id={sys.argv[2]}")
    raise SystemExit(0)
raise SystemExit(1)
"#,
    );
    let consensus_log = dir.path().join("consensus.log");
    script(
        &bin,
        "ljos-consensus",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nprintf '%s\\n' '{{\"options\":[\"nit\",\"hold\",\"yes\",\"pass\"],\"shares\":[0.25,0.25,0.25,0.25],\"settled\":false}}'\n",
            consensus_log.display()
        ),
    );
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let ljos = |seat: &str, args: &[&str]| {
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
            .env("LJOS_SEAT", seat)
            .env("VISSUE_AGENT", seat)
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
    let mcp = |seat: &str, arguments: serde_json::Value| {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
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
            .env("LJOS_SEAT", seat)
            .env("VISSUE_AGENT", seat)
            .env("SEAT_SESSION_ID", "sess-tls-0001")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "probe", "version": "0"}}
            })
        )
        .unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .unwrap();
        let mut arguments = arguments;
        arguments["seat"] = serde_json::Value::String(seat.to_string());
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "ljos_vote", "arguments": arguments}
            })
        )
        .unwrap();
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };

    let (ok, help) = ljos("tls", &["vote", "--help"]);
    assert!(ok, "{help}");
    assert!(help.contains("one voice per session"), "{help}");
    assert!(help.contains("real deed"), "{help}");
    assert!(help.contains("one deed"), "{help}");
    assert!(!help.contains("--used none --as"), "{help}");

    let issue = "demo-y1fz";
    let (ok, said) = ljos(
        "tls",
        &[
            "vote", issue, "--for", "nit", "--used", "none", "--as", "tls-rev1",
        ],
    );
    assert!(!ok, "{said}");
    deed_sentence(&said, "tls-rev1");
    let bare = std::fs::read_to_string(state.join("ballots.json")).unwrap_or_default();
    assert!(!bare.contains("tls-rev1"), "{bare}");

    for used in ["not-a-deed", "x", "1", "nope"] {
        let (ok, said) = ljos(
            "tls",
            &[
                "vote", issue, "--for", "nit", "--used", used, "--as", "alice",
            ],
        );
        assert!(!ok, "{used}: {said}");
        deed_sentence(&said, "alice");
    }
    let fullwidth_none = "\u{FF4E}\u{FF4F}\u{FF4E}\u{FF45}";
    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "nit",
            "--used",
            fullwidth_none,
            "--as",
            "bob",
        ],
    );
    assert!(!ok, "{said}");
    deed_sentence(&said, "bob");
    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "nit",
            "--used",
            "n\u{200b}one",
            "--as",
            "carol",
        ],
    );
    assert!(!ok, "{said}");
    deed_sentence(&said, "carol");
    let (ok, said) = ljos(
        "tls",
        &[
            "vote", issue, "--for", "no", "--used", "none", "--as", "editor",
        ],
    );
    assert!(!ok, "{said}");
    deed_sentence(&said, "editor");
    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "nit",
            "--used",
            "deed-rev1",
            "--as",
            "tls-rev1@a",
        ],
    );
    assert!(!ok, "{said}");
    assert!(
        said.contains("a persona name may use only letters, digits, '.', '_' and '-'"),
        "{said}"
    );
    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "nit",
            "--used",
            "deed-rev1",
            "--as",
            "tls-rev1:aa",
        ],
    );
    assert!(!ok, "{said}");
    assert!(said.contains("a persona name may use only"), "{said}");
    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "nit",
            "--used",
            "deed-rev1",
            "--as",
            "inky-n9o",
        ],
    );
    assert!(!ok, "{said}");
    assert!(
        said.contains("a seat cannot cast a ballot for inky"),
        "{said}"
    );

    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "nit",
            "--used",
            "deed-rev1",
            "--as",
            "tls-rev1",
        ],
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: tls-rev1 for nit"),
        "{said}"
    );
    assert!(said.contains("tls-rev1 cast by tls"), "{said}");
    assert!(!said.contains("persona:tls:tls"), "{said}");

    for spelling in [
        "TLS-rev1",
        "tls--rev1",
        "tls---rev1",
        "tls_rev1",
        "tls.rev1",
        "tls\u{2010}rev1",
    ] {
        let (ok, said) = ljos(
            "tls",
            &[
                "vote",
                issue,
                "--for",
                "pass",
                "--used",
                "deed-rev2",
                "--as",
                spelling,
            ],
        );
        assert!(ok, "{spelling}: {said}");
        assert!(said.contains("label repeated-ballot"), "{spelling}: {said}");
        assert!(said.contains("Nothing was cast"), "{spelling}: {said}");
    }
    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "pass",
            "--used",
            "deed-rev2",
            "--as",
            "tls rev1",
        ],
    );
    assert!(!ok, "{said}");
    assert!(
        said.contains("a persona name may use only letters, digits, '.', '_' and '-'"),
        "{said}"
    );
    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "pass",
            "--used",
            "deed-rev2",
            "--as",
            "tls-rev1@extra",
        ],
    );
    assert!(
        !ok && said.contains("a persona name may use only letters, digits, '.', '_' and '-'"),
        "{said}"
    );

    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "hold",
            "--used",
            "deed-rev1",
            "--as",
            "tls-rev2",
        ],
    );
    assert!(!ok, "{said}");
    assert!(
        said.contains("cannot cite deed-rev1, which another persona already used"),
        "{said}"
    );
    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "hold",
            "--used",
            "deed-rev2",
            "--as",
            "tls-rev2",
        ],
    );
    assert!(ok, "{said}");
    assert!(said.contains("tls-rev2 cast by tls"), "{said}");

    let (ok, said) = ljos("tls", &["vote", issue, "--for", "yes", "--used", "none"]);
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: tls for yes"),
        "{said}"
    );
    let (ok, said) = ljos("tls", &["vote", issue, "--for", "no"]);
    assert!(ok, "{said}");
    assert!(said.contains("label repeated-ballot"), "{said}");
    let bare = std::fs::read_to_string(state.join("ballots.json")).unwrap();
    assert!(bare.contains("\"agent\": \"tls\""), "{bare}");
    assert!(bare.contains("\"choice\": \"yes\""), "{bare}");
    assert!(!bare.contains("\"choice\": \"no\""), "{bare}");

    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "pass",
            "--used",
            "deed-alice",
            "--as",
            "alice",
        ],
    );
    assert!(ok, "{said}");
    assert!(said.contains("alice cast by tls"), "{said}");
    let (ok, said) = ljos(
        "tls",
        &[
            "vote", issue, "--for", "hold", "--used", "deed-bob", "--as", "bob",
        ],
    );
    assert!(ok, "{said}");

    let (ok, said) = ljos("tls", &["vote", issue]);
    assert!(ok, "{said}");
    assert!(said.contains("5 ballots count"), "{said}");
    for line in [
        "tls-rev1  nit",
        "tls-rev2  hold",
        "tls  yes",
        "alice  pass",
        "bob  hold",
    ] {
        assert!(said.contains(line), "{line} missing from {said}");
    }
    assert!(!said.contains("folded into one voice"), "{said}");

    let (ok, said) = ljos("tls", &["consensus", issue, "--no-file", "--json"]);
    assert!(ok, "{said}");
    let logged = std::fs::read_to_string(&consensus_log).unwrap_or_default();
    for name in ["tls-rev1", "tls-rev2", "alice", "bob"] {
        assert!(logged.contains(name), "{name} missing from {logged}");
    }

    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "pass",
            "--used",
            "deed-rev1",
            "--as",
            "tls-rev1",
        ],
    );
    assert!(ok, "{said}");
    assert!(said.contains("label repeated-ballot"), "{said}");

    let (_ok, withdrawn) = ljos("tls", &["vote", issue, "--withdraw", "--as", "TLS-rev1"]);
    // The ballot is gone. `PACKSET_URL=off` then fails the forecast half,
    // which is the same non-zero the other withdraw tests expect.
    assert!(withdrawn.contains("withdrawn"), "{withdrawn}");
    assert!(
        withdrawn.contains("ballot withdrawn; the forecast for tls-rev1 was not"),
        "{withdrawn}"
    );
    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            issue,
            "--for",
            "pass",
            "--used",
            "deed-rev1b",
            "--as",
            "tls-rev1",
        ],
    );
    assert!(ok, "{said}");
    assert!(!said.contains("label repeated-ballot"), "{said}");
    assert!(
        said.contains("recorded on the tracker: tls-rev1 for pass"),
        "{said}"
    );
    let (ok, said) = ljos("tls", &["vote", issue]);
    assert!(ok, "{said}");
    assert!(said.contains("tls-rev1  pass"), "{said}");
    assert!(said.contains("5 ballots count"), "{said}");
    assert!(!said.contains("folded into one voice"), "{said}");

    let pins = std::fs::read_to_string(root.join(".ljos-seat-keys")).unwrap_or_default();
    assert!(
        pins.lines()
            .any(|line| line.split_whitespace().next() == Some("tls-rev1")),
        "{pins}"
    );
    assert!(!pins.contains("tls rev"), "{pins}");
    assert!(
        !pins
            .lines()
            .any(|line| line.split_whitespace().next().is_some_and(|voter| {
                voter
                    .chars()
                    .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')))
            })),
        "{pins}"
    );
    git(&root, &["add", ".ljos-seat-keys"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "pin"],
    );
    let (ok, said) = ljos(
        "tls",
        &[
            "vote",
            "after",
            "--for",
            "pass",
            "--used",
            "deed-after",
            "--as",
            "tls-rev1",
        ],
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: tls-rev1 for pass"),
        "{said}"
    );
    assert!(!said.contains("persona:tls:tls"), "{said}");
    assert!(said.contains("tls-rev1 cast by tls"), "{said}");

    let said = mcp(
        "tls",
        serde_json::json!({"issue": issue, "choice": "pass", "used": "deed-after", "as": "tls-rev1"}),
    );
    assert!(said.contains("repeated-ballot"), "{said}");
    let bare = std::fs::read_to_string(state.join("ballots.json")).unwrap();
    assert!(!bare.contains("persona:tls:tls"), "{bare}");
    let said = mcp(
        "tls",
        serde_json::json!({"issue": "mcp1", "choice": "nit", "used": "none", "as": "editor"}),
    );
    assert!(
        said.contains("a ballot cast as editor needs --used naming a deed"),
        "{said}"
    );
    let bare = std::fs::read_to_string(state.join("ballots.json")).unwrap();
    assert!(
        !bare.contains("mcp1") || !bare.contains("\"agent\": \"editor\""),
        "{bare}"
    );

    let disk = "disk-seat";
    let (ok, said) = ljos(
        "Disk Saver",
        &["vote", disk, "--for", "nit", "--used", "none"],
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: disk-saver for nit"),
        "{said}"
    );
    let (ok, said) = ljos(
        "Disk Saver",
        &[
            "vote",
            disk,
            "--for",
            "no",
            "--used",
            "not-a-deed",
            "--as",
            "alice",
        ],
    );
    assert!(!ok, "{said}");
    deed_sentence(&said, "alice");
    let (ok, said) = ljos(
        "Disk Saver",
        &[
            "vote",
            disk,
            "--for",
            "pass",
            "--used",
            "deed-disk",
            "--as",
            "alice",
        ],
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("alice cast by Disk Saver") || said.contains("alice cast by Disk_Saver"),
        "{said}"
    );
    let (ok, said) = ljos("Disk Saver", &["vote", disk]);
    assert!(ok, "{said}");
    assert!(said.contains("2 ballots count"), "{said}");
    assert!(said.contains("alice  pass"), "{said}");
    assert!(said.contains("disk-saver  nit"), "{said}");
    assert!(!said.contains("folded into one voice"), "{said}");
    let (ok, said) = ljos(
        "Disk Saver",
        &[
            "vote",
            disk,
            "--for",
            "hold",
            "--used",
            "none",
            "--as",
            "disk-saver-t4t",
        ],
    );
    assert!(ok, "{said}");
    assert!(said.contains("label repeated-ballot"), "{said}");

    let inky = "inky-seat";
    let (ok, said) = ljos("inky", &["vote", inky, "--for", "yes", "--used", "none"]);
    assert!(ok, "{said}");
    let (ok, said) = ljos(
        "inky",
        &[
            "vote", inky, "--for", "no", "--used", "none", "--as", "inky-n9o",
        ],
    );
    assert!(ok, "{said}");
    assert!(said.contains("label repeated-ballot"), "{said}");
    let (ok, said) = ljos(
        "inky",
        &[
            "vote",
            inky,
            "--for",
            "hold",
            "--used",
            "none",
            "--as",
            "inky-n9o@deadbeef",
        ],
    );
    assert!(ok, "{said}");
    assert!(said.contains("label repeated-ballot"), "{said}");
    let bare = std::fs::read_to_string(state.join("ballots.json")).unwrap();
    assert!(bare.contains("\"agent\": \"inky\""), "{bare}");
    assert!(!bare.contains("inky-n9o"), "{bare}");
    let (_ok, said) = ljos("inky", &["vote", inky, "--withdraw", "--as", "inky-n9o"]);
    assert!(said.contains("withdrawn"), "{said}");
    assert!(
        said.contains("ballot withdrawn; the forecast for inky was not"),
        "{said}"
    );
    let (ok, said) = ljos("inky", &["vote", inky]);
    assert!(ok, "{said}");
    assert!(
        said.contains("0 ballots count") || said.contains("no ballot"),
        "{said}"
    );
    assert!(!said.contains("inky  yes"), "{said}");

    let plain = "plain-none";
    for name in ["alpha", "beta"] {
        let (ok, said) = ljos(
            "tls",
            &[
                "vote", plain, "--for", "hold", "--used", "none", "--as", name,
            ],
        );
        assert!(!ok, "{name}: {said}");
        deed_sentence(&said, name);
    }

    let case_issue = "case-seat";
    let (ok, said) = ljos(
        "Tls",
        &["vote", case_issue, "--for", "yes", "--used", "none"],
    );
    assert!(ok, "{said}");
    let (ok, said) = ljos(
        "Tls",
        &[
            "vote", case_issue, "--for", "hold", "--used", "none", "--as", "tls",
        ],
    );
    assert!(ok && said.contains("label repeated-ballot"), "{said}");
    let (ok, said) = ljos(
        "Tls",
        &[
            "vote",
            case_issue,
            "--for",
            "nit",
            "--used",
            "deed-rev1",
            "--as",
            "Tls-rev1",
        ],
    );
    assert!(ok, "{said}");
    let (ok, said) = ljos(
        "Tls",
        &[
            "vote",
            case_issue,
            "--for",
            "hold",
            "--used",
            "deed-rev2",
            "--as",
            "Tls-rev2",
        ],
    );
    assert!(ok, "{said}");
    let (ok, said) = ljos("Tls", &["vote", case_issue]);
    assert!(ok, "{said}");
    assert!(said.contains("3 ballots count"), "{said}");
    assert!(said.contains("tls-rev1  nit"), "{said}");
    assert!(said.contains("tls-rev2  hold"), "{said}");
    assert!(!said.contains("folded into one voice"), "{said}");

    let inky_case = "inky-case";
    let (ok, said) = ljos(
        "INKY",
        &["vote", inky_case, "--for", "yes", "--used", "none"],
    );
    assert!(ok, "{said}");
    let (ok, said) = ljos(
        "INKY",
        &[
            "vote", inky_case, "--for", "no", "--used", "none", "--as", "inky",
        ],
    );
    assert!(ok && said.contains("label repeated-ballot"), "{said}");
    let (ok, said) = ljos(
        "INKY",
        &[
            "vote",
            inky_case,
            "--for",
            "nit",
            "--used",
            "deed-rev1",
            "--as",
            "Tls-rev1",
        ],
    );
    assert!(ok, "{said}");
    let (ok, said) = ljos(
        "INKY",
        &[
            "vote",
            inky_case,
            "--for",
            "hold",
            "--used",
            "deed-rev2",
            "--as",
            "Tls-rev2",
        ],
    );
    assert!(ok, "{said}");
    let (ok, said) = ljos("INKY", &["vote", inky_case]);
    assert!(ok, "{said}");
    assert!(said.contains("3 ballots count"), "{said}");
    assert!(
        said.contains("tls-rev1  nit") && said.contains("tls-rev2  hold"),
        "{said}"
    );
    assert!(!said.contains("folded into one voice"), "{said}");
}

struct Rig {
    _dir: tempfile::TempDir,
    state: std::path::PathBuf,
    root: std::path::PathBuf,
    home: std::path::PathBuf,
    path: String,
}

impl Rig {
    fn new() -> Self {
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
        git(&root, &["config", "commit.gpgsign", "false"]);
        let hooks = dir.path().join("no-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        git(
            &root,
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        );
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
    used = args[args.index("--used") + 1] if "--used" in args else ""
    rows = load(ballots)
    kept = [r for r in rows.get(issue, []) if r.get("agent") != agent]
    kept.append({{"agent": agent, "choice": choice, "used": used}})
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
elif cmd == "deed" and "--add" in args:
    added = args[args.index("--add") + 1]
    print(f"{{issue}}: deeds += {{added}}")
else:
    print("ok")
"#
            ),
        );
        script(
            &bin,
            "deedar",
            r#"#!/usr/bin/env python3
import sys
known = {"deed-alice", "deed-bob", "deed-file-two", "deed-fw", "deed-kelvin", "deed-ubuntu"}
if len(sys.argv) >= 3 and sys.argv[1] == "current" and sys.argv[2] in known:
    print(f"id={sys.argv[2]}")
    raise SystemExit(0)
raise SystemExit(1)
"#,
        );
        Self {
            path: format!("{}:/usr/bin:/bin", bin.display()),
            _dir: dir,
            state,
            root,
            home,
        }
    }

    fn run_at(&self, root: &std::path::Path, seat: &str, args: &[&str]) -> (bool, String) {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self._dir.path().join("run"))
            .env("GIT_CONFIG_GLOBAL", self.home.join("gitconfig"))
            .env("GIT_CONFIG_SYSTEM", self.home.join("gitconfig"))
            .env("VISSUE_ROOT", root)
            .env("VISSUE_STATE", &self.state)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", seat)
            .env("VISSUE_AGENT", seat)
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
    }

    fn ljos(&self, seat: &str, args: &[&str]) -> (bool, String) {
        self.run_at(&self.root, seat, args)
    }

    fn ballots(&self) -> String {
        std::fs::read_to_string(self.state.join("ballots.json")).unwrap_or_default()
    }

    fn notes(&self) -> String {
        std::fs::read_to_string(self.state.join("notes.json")).unwrap_or_default()
    }

    fn mcp(&self, seat: &str, calls: &[serde_json::Value]) -> String {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self._dir.path().join("run"))
            .env("GIT_CONFIG_GLOBAL", self.home.join("gitconfig"))
            .env("GIT_CONFIG_SYSTEM", self.home.join("gitconfig"))
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_STATE", &self.state)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", seat)
            .env("VISSUE_AGENT", seat)
            .env("SEAT_SESSION_ID", "sess-tls-0001")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = std::io::BufReader::new(stdout);
        let mut said = String::new();
        // The server runs each call on its own task. A later vote can
        // claim before an earlier one finishes, so each call waits for
        // its own answer before the next line is sent.
        let mut write_line = |value: serde_json::Value| {
            writeln!(stdin, "{value}").unwrap();
            stdin.flush().unwrap();
        };
        let mut read_id = |id: i64| {
            let mut line = String::new();
            for _ in 0..20 {
                line.clear();
                let n = std::io::BufRead::read_line(&mut reader, &mut line).unwrap();
                assert!(n > 0, "mcp closed stdout before id {id}: {said}");
                said.push_str(&line);
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                    if value.get("id").and_then(serde_json::Value::as_i64) == Some(id) {
                        return;
                    }
                }
            }
            panic!("no response id {id}: {said}");
        };
        write_line(serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "probe", "version": "0"}}
        }));
        read_id(1);
        write_line(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        for (i, call) in calls.iter().enumerate() {
            let mut arguments = call.clone();
            arguments["seat"] = serde_json::Value::String(seat.to_string());
            let id = i as i64 + 2;
            write_line(serde_json::json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": "ljos_vote", "arguments": arguments}
            }));
            read_id(id);
        }
        drop(stdin);
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut reader, &mut rest).unwrap();
        said.push_str(&rest);
        let out = child.wait_with_output().unwrap();
        said.push_str(&String::from_utf8_lossy(&out.stderr));
        said
    }
}

#[test]
fn canonical_names_share_one_voter_and_a_bad_choice_writes_nothing() {
    let rig = Rig::new();
    write_text(&rig.home.join("gitconfig"), b"");

    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "c5",
            "--for",
            "yes",
            "--used",
            "none",
            "--as",
            "persona:alice",
        ],
    );
    assert!(!ok, "{said}");
    deed_sentence(&said, "alice");
    assert!(
        !rig.ballots().contains("persona:tls:alice"),
        "{}",
        rig.ballots()
    );

    for name in ["persona:inky:x", "persona:carol@evil"] {
        let (ok, said) = rig.ljos(
            "tls",
            &[
                "vote",
                "sep",
                "--for",
                "no",
                "--used",
                "deed-alice",
                "--as",
                name,
            ],
        );
        assert!(!ok, "{name}: {said}");
        assert!(
            said.contains("a persona name may use only letters, digits, '.', '_' and '-'"),
            "{name}: {said}"
        );
    }
    assert!(!rig.ballots().contains("inky"), "{}", rig.ballots());

    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "c6",
            "--for",
            "yes",
            "--used",
            "deed-alice",
            "--as",
            "alice",
        ],
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: alice for yes"),
        "{said}"
    );
    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "c6",
            "--for",
            "no",
            "--used",
            "none",
            "--as",
            "persona:alice",
        ],
    );
    assert!(ok && said.contains("label repeated-ballot"), "{said}");
    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "c7",
            "--for",
            "hold",
            "--used",
            "deed-bob",
            "--as",
            "persona:Alice",
        ],
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: alice for hold"),
        "{said}"
    );
    assert!(!said.contains("persona:tls:"), "{said}");
    assert!(
        rig.ballots().contains("\"used\": \"deed-bob\""),
        "{}",
        rig.ballots()
    );

    let (ok, said) = rig.ljos("Tls", &["vote", "case1", "--for", "yes", "--used", "none"]);
    assert!(ok, "{said}");
    let (ok, said) = rig.ljos("tls", &["vote", "case1", "--for", "no", "--used", "none"]);
    assert!(ok && said.contains("label repeated-ballot"), "{said}");
    let (ok, said) = rig.ljos("TLS", &["vote", "case1", "--for", "hold", "--used", "none"]);
    assert!(ok && said.contains("label repeated-ballot"), "{said}");
    let (ok, said) = rig.ljos("tls", &["vote", "case1"]);
    assert!(ok, "{said}");
    assert!(said.contains("1 ballot counts"), "{said}");
    assert!(said.contains("tls  yes"), "{said}");
    assert!(!said.contains("hold"), "{said}");
    let (_ok, withdrawn) = rig.ljos("tls", &["vote", "case1", "--withdraw"]);
    assert!(
        withdrawn.contains("withdrawn") && !withdrawn.contains("holds no ballot"),
        "{withdrawn}"
    );
    let (ok, said) = rig.ljos("TLS", &["vote", "case1"]);
    assert!(ok && said.contains("0 ballots count"), "{said}");
    let (ok, said) = rig.ljos("tls", &["vote", "case1", "--for", "no", "--used", "none"]);
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: tls for no"),
        "{said}"
    );
    let (ok, said) = rig.ljos("TLS", &["vote", "case1"]);
    assert!(
        ok && said.contains("1 ballot counts") && said.contains("tls  no"),
        "{said}"
    );

    let (ok, said) = rig.ljos(
        "disk-saver",
        &["vote", "hy", "--for", "yes", "--used", "none"],
    );
    assert!(ok, "{said}");
    let (ok, said) = rig.ljos(
        "disk--saver",
        &["vote", "hy", "--for", "no", "--used", "none"],
    );
    assert!(ok && said.contains("label repeated-ballot"), "{said}");
    let (ok, said) = rig.ljos(
        "Disk Saver",
        &["vote", "hy", "--for", "hold", "--used", "none"],
    );
    assert!(ok && said.contains("label repeated-ballot"), "{said}");
    let notes = rig.notes();
    assert!(notes.contains("ballot-sig: disk-saver "), "{notes}");
    assert!(!notes.contains("ballot-sig: Disk "), "{notes}");
    let (ok, said) = rig.ljos("disk-saver", &["vote", "hy"]);
    assert!(ok, "{said}");
    assert!(said.contains("1 ballot counts"), "{said}");
    assert!(said.contains("disk-saver  yes"), "{said}");

    let (ok, said) = rig.ljos(
        "Disk Saver",
        &["vote", "space", "--for", "hold", "--used", "none"],
    );
    assert!(ok, "{said}");
    let (ok, said) = rig.ljos("inky", &["vote", "space", "--for", "yes", "--used", "none"]);
    assert!(ok, "{said}");
    let (ok, said) = rig.ljos("inky", &["vote", "space"]);
    assert!(
        ok && said.contains("1 ballot counts")
            && said.contains("folded into one voice")
            && said.contains("inky")
            && !said.contains("Saver hold")
            && !said.contains("Saver yes"),
        "{said}"
    );
    let (ok, said) = rig.ljos(
        "Disk Saver",
        &["vote", "hit", "--for", "hold", "--used", "none"],
    );
    assert!(ok, "{said}");
    let (ok, said) = rig.ljos("disk", &["vote", "hit", "--for", "yes", "--used", "none"]);
    assert!(ok, "{said}");
    let bare = rig.ballots();
    assert!(
        bare.contains("\"agent\": \"disk-saver\"")
            && bare.contains("\"choice\": \"hold\"")
            && bare.contains("\"agent\": \"disk\"")
            && bare.contains("\"choice\": \"yes\"")
            && !bare.contains("Saver"),
        "{bare}"
    );
    let (ok, said) = rig.ljos("disk", &["vote", "hit"]);
    assert!(
        ok && !said.contains("Saver hold") && !said.contains("Saver yes"),
        "{said}"
    );
    let key = ljos_cli::ballot_sig::load_or_create(&rig.home.join("seat.key")).unwrap();
    let note =
        ljos_cli::ballot_sig::sign_bound(&key, "probe-space", "Disk Saver", "hold", "sess-1", "-");
    let parsed = ljos_cli::ballot_sig::parse_note(&note).expect("quoted voter");
    assert!(
        note.contains("\"Disk Saver\"")
            && parsed.voter == "Disk Saver"
            && ljos_cli::ballot_sig::verifies("probe-space", &parsed)
            && ljos_cli::voter_id(&parsed.voter) != ljos_cli::voter_id("disk"),
        "{note}"
    );

    let fullwidth_inky = "\u{FF49}\u{FF4E}\u{FF4B}\u{FF59}";
    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "cross",
            "--for",
            "ship",
            "--used",
            "deed-fw",
            "--as",
            fullwidth_inky,
        ],
    );
    assert!(!ok, "{said}");
    assert!(
        said.contains("a seat cannot cast a ballot for inky"),
        "{said}"
    );
    let kelvin = "dis\u{212A}-saver";
    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "cross",
            "--for",
            "no",
            "--used",
            "deed-kelvin",
            "--as",
            kelvin,
        ],
    );
    assert!(!ok, "{said}");
    assert!(
        said.contains("a seat cannot cast a ballot for disk-saver"),
        "{said}"
    );
    assert!(!rig.ballots().contains("cross"), "{}", rig.ballots());
    let (ok, said) = rig.ljos(
        "inky",
        &["vote", "inky-own", "--for", "keep", "--used", "none"],
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: inky for keep"),
        "{said}"
    );

    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote", "acct", "--for", "yes", "--used", "none", "--as", "ubuntu",
        ],
    );
    assert!(!ok, "{said}");
    deed_sentence(&said, "ubuntu");
    assert!(
        !rig.ballots().contains("persona:tls:ubuntu"),
        "{}",
        rig.ballots()
    );
    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "acct",
            "--for",
            "yes",
            "--used",
            "deed-ubuntu",
            "--as",
            "ubuntu",
        ],
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: ubuntu for yes"),
        "{said}"
    );
    let wide_u = "\u{FF55}buntu";
    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote", "acct", "--for", "no", "--used", "none", "--as", wide_u,
        ],
    );
    assert!(ok && said.contains("label repeated-ballot"), "{said}");

    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "probe-cdqb",
            "--for",
            "nit used=not-a-deed",
            "--used",
            "deed-file-two",
            "--as",
            "nina",
        ],
    );
    assert!(!ok, "{said}");
    assert!(said.contains("nothing was cast"), "{said}");
    assert!(!rig.ballots().contains("nina"), "{}", rig.ballots());
    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "probe-cdqb",
            "--for",
            "hold",
            "--used",
            "deed-file-two",
            "--as",
            "omar",
        ],
    );
    assert!(ok, "{said}");
    let (ok, said) = rig.ljos("tls", &["vote", "probe-cdqb"]);
    assert!(ok, "{said}");
    assert!(said.contains("1 ballot counts"), "{said}");
    assert!(said.contains("omar  hold"), "{said}");
    assert!(!said.contains("nina"), "{said}");
}

#[test]
fn one_mcp_process_recasts_after_withdraw() {
    let rig = Rig::new();
    write_text(&rig.home.join("gitconfig"), b"");
    let said = rig.mcp(
        "tls",
        &[
            serde_json::json!({
                "issue": "p5", "choice": "yes", "used": "deed-alice", "as": "alice"
            }),
            serde_json::json!({"issue": "p5", "withdraw": true, "as": "alice"}),
            serde_json::json!({
                "issue": "p5", "choice": "no", "used": "deed-bob", "as": "alice"
            }),
        ],
    );
    assert!(
        said.contains("recorded on the tracker: alice for no"),
        "{said}"
    );
    assert!(
        !said.contains("already cast a ballot on p5 in this process"),
        "{said}"
    );
    let bare = rig.ballots();
    assert!(bare.contains("\"agent\": \"alice\""), "{bare}");
    assert!(bare.contains("\"choice\": \"no\""), "{bare}");
    assert!(bare.contains("\"used\": \"deed-bob\""), "{bare}");
}

#[test]
fn a_shallow_clone_still_requires_a_deed() {
    let rig = Rig::new();
    write_text(&rig.home.join("gitconfig"), b"");
    let origin = rig._dir.path().join("origin");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "-b", "main"]);
    git(&origin, &["config", "user.email", "seat@example.invalid"]);
    git(&origin, &["config", "user.name", "seat"]);
    git(&origin, &["config", "commit.gpgsign", "false"]);
    let hooks = rig._dir.path().join("no-hooks");
    git(
        &origin,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    write_text(&origin.join("issues.org"), "board\n");
    git(&origin, &["add", "."]);
    git(&origin, &["commit", "-q", "-m", "board"]);
    write_text(&origin.join("issues.org"), "board\nmore\n");
    git(&origin, &["add", "."]);
    git(&origin, &["commit", "-q", "-m", "more"]);
    let shallow = rig._dir.path().join("shallow");
    let url = format!("file://{}", origin.display());
    git(
        rig._dir.path(),
        &["clone", "-q", "--depth", "1", &url, "shallow"],
    );
    git(&shallow, &["remote", "remove", "origin"]);
    let hooks = rig._dir.path().join("shallow-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    git(
        &shallow,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    assert_eq!(
        git_text(&shallow, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    let (ok, said) = rig.run_at(
        &shallow,
        "tls",
        &[
            "vote",
            "shallow-1",
            "--for",
            "yes",
            "--used",
            "none",
            "--as",
            "alice",
        ],
    );
    assert!(!ok, "{said}");
    deed_sentence(&said, "alice");
    assert!(!rig.ballots().contains("alice"), "{}", rig.ballots());
    assert!(
        !rig.ballots().contains("persona:tls:alice"),
        "{}",
        rig.ballots()
    );
    let (ok, said) = rig.run_at(
        &shallow,
        "tls",
        &["vote", "shallow-seat", "--for", "yes", "--used", "none"],
    );
    assert!(ok, "{said}");
    let (ok, said) = rig.run_at(
        &shallow,
        "tls",
        &[
            "vote",
            "shallow-1",
            "--for",
            "yes",
            "--used",
            "deed-alice",
            "--as",
            "alice",
        ],
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: alice for yes"),
        "{said}"
    );
    assert_eq!(
        git_text(&shallow, &["rev-parse", "--is-shallow-repository"]),
        "true",
        "the deed check fetched history"
    );
}

#[test]
fn a_pin_that_stood_is_that_seat() {
    let rig = Rig::new();
    write_text(&rig.home.join("gitconfig"), b"");
    let repo = rig._dir.path().join("stood");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "seat@example.invalid"]);
    git(&repo, &["config", "user.name", "seat"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    let hooks = rig._dir.path().join("no-hooks");
    git(
        &repo,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    write_text(&repo.join("issues.org"), "board\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "board"]);
    write_text(&repo.join(".ljos-seat-keys"), "mina bb\n");
    git(&repo, &["add", ".ljos-seat-keys"]);
    git(&repo, &["commit", "-q", "-m", "pin mina"]);
    write_text(
        &repo.join("issues.org"),
        "board\n- Note: \"slot: mina model=m run=r key=k ada aa bb\"\n",
    );
    git(&repo, &["add", "issues.org"]);
    git(&repo, &["commit", "-q", "-m", "slot"]);

    let (ok, said) = rig.run_at(
        &repo,
        "mina",
        &["vote", "legacy", "--for", "yes", "--used", "none"],
    );
    assert!(ok, "{said}");
    let (ok, said) = rig.run_at(
        &repo,
        "mina",
        &[
            "vote", "legacy", "--for", "no", "--used", "none", "--as", "mina-n9o",
        ],
    );
    assert!(ok && said.contains("label repeated-ballot"), "{said}");
    let (ok, said) = rig.run_at(
        &repo,
        "mina",
        &["vote", "pull", "--for", "yes", "--used", "none"],
    );
    assert!(ok, "{said}");
    let (_ok, said) = rig.run_at(
        &repo,
        "mina",
        &["vote", "pull", "--withdraw", "--as", "mina@deadbeef"],
    );
    assert!(said.contains("withdrawn"), "{said}");
    let (ok, said) = rig.run_at(&repo, "mina", &["vote", "pull"]);
    assert!(ok, "{said}");
    assert!(
        said.contains("0 ballots count") || said.contains("no ballot"),
        "{said}"
    );

    let (ok, said) = rig.run_at(
        &repo,
        "ada",
        &[
            "vote", "cross", "--for", "ship", "--used", "none", "--as", "mina",
        ],
    );
    assert!(!ok, "{said}");
    assert!(
        said.contains("a seat cannot cast a ballot for mina"),
        "{said}"
    );
    assert!(
        !rig.ballots().contains("persona:ada:mina"),
        "{}",
        rig.ballots()
    );
}

#[test]
fn a_choice_with_a_control_character_writes_nothing() {
    let rig = Rig::new();
    write_text(&rig.home.join("gitconfig"), b"");
    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "cr1",
            "--for",
            "aa\rbb",
            "--used",
            "deed-file-two",
            "--as",
            "omar",
        ],
    );
    assert!(!ok, "{said}");
    assert!(said.contains("control character"), "{said}");
    assert!(!rig.ballots().contains("cr1"), "{}", rig.ballots());
    assert!(!rig.notes().contains("aa bb"), "{}", rig.notes());
    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "cr2",
            "--for",
            "nit\rused=not-a-deed",
            "--used",
            "deed-file-two",
            "--as",
            "omar",
        ],
    );
    assert!(!ok, "{said}");
    assert!(said.contains("control character"), "{said}");
    assert!(!rig.ballots().contains("cr2"), "{}", rig.ballots());
    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "cr1",
            "--for",
            "hold",
            "--used",
            "deed-file-two",
            "--as",
            "nina",
        ],
    );
    assert!(ok, "{said}");
    assert!(
        said.contains("recorded on the tracker: nina for hold"),
        "{said}"
    );
    let notes = rig.notes();
    assert!(notes.contains("ballot-sig: nina hold "), "{notes}");
}

#[test]
fn mcp_vote_refuses_the_same_modes_as_the_cli() {
    let rig = Rig::new();
    write_text(&rig.home.join("gitconfig"), b"");
    let (ok, said) = rig.ljos("tls", &["vote", "keep", "--for", "yes", "--used", "none"]);
    assert!(ok, "{said}");
    let kept = rig.ballots();
    let (ok, said) = rig.ljos("tls", &["vote", "keep", "--withdraw", "--used", "none"]);
    assert!(!ok, "{said}");
    assert!(
        said.contains("the argument '--withdraw' cannot be used with '--used <USED>'"),
        "{said}"
    );
    assert_eq!(rig.ballots(), kept);
    let said = rig.mcp(
        "tls",
        &[serde_json::json!({"issue": "keep", "withdraw": true, "used": "none"})],
    );
    assert!(
        said.contains("the argument '--withdraw' cannot be used with '--used <USED>'"),
        "{said}"
    );
    assert_eq!(rig.ballots(), kept);

    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote",
            "mix",
            "--for",
            "yes",
            "--used",
            "none",
            "--withdraw",
        ],
    );
    assert!(!ok, "{said}");
    assert!(
        said.contains("the argument '--for <CHOICE>' cannot be used with '--withdraw'"),
        "{said}"
    );
    assert!(!rig.ballots().contains("mix"), "{}", rig.ballots());
    let said = rig.mcp(
        "tls",
        &[serde_json::json!({
            "issue": "mix", "choice": "yes", "used": "none", "withdraw": true
        })],
    );
    assert!(
        said.contains("the argument '--for <CHOICE>' cannot be used with '--withdraw'"),
        "{said}"
    );
    assert!(!rig.ballots().contains("mix"), "{}", rig.ballots());

    let (ok, said) = rig.ljos(
        "tls",
        &[
            "vote", "jc3", "--for", "yes", "--used", "deed-c", "--as", "omar", "--jev",
        ],
    );
    assert!(!ok, "{said}");
    assert!(
        said.contains("the argument '--for <CHOICE>' cannot be used with '--jev'"),
        "{said}"
    );
    let said = rig.mcp(
        "tls",
        &[serde_json::json!({
            "issue": "jc3", "choice": "yes", "used": "deed-c", "as": "omar", "jev": true
        })],
    );
    assert!(
        said.contains("the argument '--for <CHOICE>' cannot be used with '--jev'"),
        "{said}"
    );
    assert!(!rig.ballots().contains("omar"), "{}", rig.ballots());
    assert!(!rig.ballots().contains("jc3"), "{}", rig.ballots());

    let (ok, said) = rig.ljos("tls", &["vote", "jev1", "--jev"]);
    assert!(!ok, "{said}");
    assert!(said.contains("vote needs --as <AS_PERSONA>"), "{said}");
    let said = rig.mcp("tls", &[serde_json::json!({"issue": "jev1", "jev": true})]);
    assert!(said.contains("vote needs --as <AS_PERSONA>"), "{said}");

    let (ok, said) = rig.ljos("tls", &["vote", "blank", "--for", "yes", "--used", "none"]);
    assert!(ok, "{said}");
    let kept = rig.ballots();
    let (ok, said) = rig.ljos("tls", &["vote", "blank", "--withdraw", "--expect", " "]);
    assert!(!ok, "{said}");
    assert!(
        said.contains("the argument '--withdraw' cannot be used with '--expect <EXPECT>'"),
        "{said}"
    );
    assert_eq!(rig.ballots(), kept);
    for blank in [" ", ""] {
        let said = rig.mcp(
            "tls",
            &[serde_json::json!({"issue": "blank", "withdraw": true, "expect": blank})],
        );
        assert!(
            said.contains("the argument '--withdraw' cannot be used with '--expect <EXPECT>'"),
            "{blank:?}: {said}"
        );
        assert_eq!(rig.ballots(), kept, "{blank:?}");
    }

    let said = rig.mcp(
        "tls",
        &[serde_json::json!({"issue": "keep", "withdraw": true})],
    );
    assert!(said.contains("withdrawn"), "{said}");
    assert!(
        said.contains("ballot withdrawn; the forecast for"),
        "{said}"
    );
    assert!(!said.contains("packset url missing"), "{said}");

    let said = rig.mcp(
        "tls",
        &[serde_json::json!({
            "issue": "ex1", "choice": "yes", "used": "none", "expect": "yes"
        })],
    );
    assert!(said.contains("ballot cast; forecast for"), "{said}");
    assert!(
        said.contains("PACKSET_URL=off: this seat has no pack on purpose"),
        "{said}"
    );
    assert!(!said.contains("packset url missing"), "{said}");
    assert!(rig.ballots().contains("ex1"), "{}", rig.ballots());
}

#[test]
fn a_folded_choice_is_what_the_ballot_and_the_note_store() {
    let rig = Rig::new();
    let cases = [
        ("fold-sp", "xdg  home", "xdg home"),
        ("fold-qu", "say \"yes\"", "say 'yes'"),
        ("fold-nb", "ship\u{00a0}it", "ship it"),
    ];
    for (issue, raw, stored) in cases {
        let (ok, said) = rig.ljos("tls", &["vote", issue, "--for", raw, "--used", "none"]);
        assert!(ok, "{raw:?}: {said}");
        let (ok, tally) = rig.ljos("tls", &["vote", issue]);
        assert!(
            ok && tally.contains("1 ballot counts") && tally.contains(stored),
            "{raw:?}: {tally}"
        );
    }
    let bare = rig.ballots();
    let notes = rig.notes();
    assert!(
        bare.contains("\"choice\": \"xdg home\"")
            && !bare.contains("xdg  home")
            && bare.contains("say 'yes'")
            && bare.contains("ship it")
            && notes.contains("xdg home")
            && !notes.contains("xdg  home")
            && notes.contains("say 'yes'")
            && notes.contains("ship it"),
        "ballots={bare}\nnotes={notes}"
    );

    let before_ballots = rig.ballots();
    let before_notes = rig.notes();
    let (ok, said) = rig.ljos(
        "inky",
        &[
            "vote",
            "hide-nbsp",
            "--for",
            "nit\u{00a0}used=not-a-deed",
            "--used",
            "none",
        ],
    );
    assert!(!ok, "{said}");
    assert!(said.contains("nothing was cast"), "{said}");
    assert_eq!(rig.ballots(), before_ballots);
    assert_eq!(rig.notes(), before_notes);

    let unfolded_ballots = rig
        .ballots()
        .replace("xdg home", "xdg  home")
        .replace("say 'yes'", "say \\\"yes\\\"");
    let unfolded_notes = rig
        .notes()
        .replace("xdg home", "xdg  home")
        .replace("say 'yes'", "say \\\"yes\\\"");
    write_text(&rig.state.join("ballots.json"), &unfolded_ballots);
    write_text(&rig.state.join("notes.json"), &unfolded_notes);
    for issue in ["fold-sp", "fold-qu"] {
        let (ok, tally) = rig.ljos("tls", &["vote", issue]);
        assert!(ok && tally.contains("1 ballot counts"), "{issue}: {tally}");
    }
}

#[test]
fn a_named_deed_is_stored_without_following_its_successor() {
    let rig = Rig::new();
    let bin = rig.path.split(':').next().unwrap();
    script(
        Path::new(bin),
        "deedar",
        r#"#!/usr/bin/env python3
import sys
cmd = sys.argv[1] if len(sys.argv) >= 2 else ""
token = sys.argv[2] if len(sys.argv) >= 3 else ""
stale = "deed-file-fuse-v1"
nxt = "deed-file-fuse-v2"
digest = "sha256:" + ("ab" * 32)
if cmd == "current" and token in {stale, digest, "file=" + digest}:
    print(f"id={nxt}")
    raise SystemExit(0)
if cmd == "current" and token == nxt:
    print(f"id={nxt}")
    raise SystemExit(0)
if cmd == "get" and token in {stale, digest, "file=" + digest}:
    print(f"id={stale} kind=file")
    raise SystemExit(0)
if cmd == "get" and token == nxt:
    print(f"id={nxt} kind=file")
    raise SystemExit(0)
raise SystemExit(1)
"#,
    );
    let (reviewer_ok, reviewer) = rig.ljos(
        "mina",
        &[
            "vote",
            "fuse",
            "--for",
            "ship",
            "--used",
            "deed-file-fuse-v1",
            "--as",
            "reviewer",
        ],
    );
    let after_reviewer = rig.ballots();
    let (clerk_ok, clerk) = rig.ljos(
        "mina",
        &[
            "vote",
            "fuse",
            "--for",
            "pass",
            "--used",
            "deed-file-fuse-v2",
            "--as",
            "clerk",
        ],
    );
    let after_clerk = rig.ballots();
    assert!(
        reviewer_ok
            && after_reviewer.contains("\"used\": \"deed-file-fuse-v1\"")
            && !after_reviewer.contains("deed-file-fuse-v2")
            && clerk_ok
            && after_clerk.contains("\"used\": \"deed-file-fuse-v2\""),
        "reviewer {reviewer}\nclerk {clerk}\n{after_clerk}"
    );
    let digest = format!("sha256:{}", "ab".repeat(32));
    let (hash_ok, hash_said) = rig.ljos(
        "mina",
        &[
            "vote",
            "hash",
            "--for",
            "pass",
            "--used",
            &digest,
            "--as",
            "historian",
        ],
    );
    let file_used = format!("file={digest}");
    let (file_ok, file_said) = rig.ljos(
        "mina",
        &[
            "vote",
            "filehash",
            "--for",
            "hold",
            "--used",
            &file_used,
            "--as",
            "archivist",
        ],
    );
    let (add_ok, added) = rig.ljos("mina", &["deed", "demo-e5", "--add", "deed-file-fuse-v1"]);
    let ballots = rig.ballots();
    assert!(
        hash_ok
            && ballots.contains("\"used\": \"deed-file-fuse-v1\"")
            && file_ok
            && !hash_said.contains("deed-file-fuse-v2")
            && !file_said.contains("deed-file-fuse-v2")
            && add_ok
            && added.contains("deeds += deed-file-fuse-v1")
            && !added.contains("deed-file-fuse-v2"),
        "hash {hash_said}\nfile {file_said}\nadd {added}\n{ballots}"
    );
}

#[test]
fn a_base_seat_row_withdraws_and_recasts() {
    let rig = Rig::new();
    write_text(
        &rig.state.join("ballots.json"),
        r#"{"keep":[{"agent":"Disk Saver","choice":"nit","used":"none"}]}"#,
    );
    let (_withdraw_ok, withdrawn) = rig.ljos("Disk Saver", &["vote", "keep", "--withdraw"]);
    let gone = rig.ballots();
    let (recast_ok, recast) = rig.ljos(
        "Disk Saver",
        &["vote", "keep", "--for", "no", "--used", "none"],
    );
    let bare = rig.ballots();
    let key = ljos_cli::ballot_sig::load_or_create(&rig.home.join("seat.key")).unwrap();
    let spellings = ["Tls", "INKY", "disk_saver", "Disk-Saver"];
    let signed = spellings.iter().all(|voter| {
        let note =
            ljos_cli::ballot_sig::sign_bound(&key, "probe-case", voter, "nit", "sess-1", "-");
        ljos_cli::ballot_sig::parse_note(&note).is_some_and(|parsed| {
            parsed.voter == *voter && ljos_cli::ballot_sig::verifies("probe-case", &parsed)
        })
    });
    assert!(
        withdrawn.contains("withdrawn")
            && !withdrawn.contains("holds no ballot")
            && !gone.contains("Disk Saver")
            && recast_ok
            && recast.contains("recorded on the tracker: disk-saver for no")
            && bare.contains("\"agent\": \"disk-saver\"")
            && bare.contains("\"choice\": \"no\"")
            && signed,
        "signed={signed}\nwithdraw={withdrawn}\ngone={gone}\nrecast={recast}\nballots={bare}"
    );
}

#[test]
fn a_jev_ballot_withdraws_by_the_printed_name() {
    let rig = Rig::new();
    let bin = Path::new(rig.path.split(':').next().unwrap());
    let state_s = rig.state.display().to_string();
    script(
        bin,
        "vissue",
        &format!(
            r#"#!/usr/bin/env python3
import json, os, sys, pathlib
root = pathlib.Path({state_s:?})
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
    used = args[args.index("--used") + 1] if "--used" in args else ""
    rows = load(ballots)
    kept = [r for r in rows.get(issue, []) if r.get("agent") != agent]
    kept.append({{"agent": agent, "choice": choice, "used": used}})
    rows[issue] = kept
    save(ballots, rows)
    print("ok")
elif cmd == "show":
    kept = load(notes).get(issue, [])
    logbook = [{{"note": note}} for note in reversed(kept)]
    print(json.dumps({{"id": issue, "title": "rig", "body": "Options: yes, no", "logbook": logbook}}))
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
    let stub = bin.join("jev-stub");
    script(
        bin,
        "jev-stub",
        r#"#!/usr/bin/env python3
import json, sys
json.load(sys.stdin)
print(json.dumps({"answers": {
    "ballot": {"choice": "yes", "probabilities": {"yes": 0.99, "no": 0.01}},
    "forecast": {"choice": "yes", "probabilities": {"yes": 0.99, "no": 0.01}}
}}))
"#,
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let pack = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            loop {
                let n = std::io::Read::read(&mut sock, &mut tmp).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&buf);
            let body = if head.contains("kind=persona") {
                r#"{"atoms":[{"kind":"persona","name":"ada","anchor":0.5,"text":"careful","entities":["cipher"],"ts":"2026-01-01T00:00:00Z"}]}"#
            } else {
                r#"{"atoms":[],"ok":true}"#
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = std::io::Write::write_all(&mut sock, resp.as_bytes());
        }
    });
    let cfg = rig.home.join(".config/ljos");
    std::fs::create_dir_all(&cfg).unwrap();
    write_text(
        &cfg.join("jev.toml"),
        format!(
            "enabled = true\nbackend = \"command\"\nmodel = \"stub-judge\"\ncache_days = 0\ncommand = [\"{}\"]\n",
            stub.display()
        ),
    );
    let run = |args: &[&str]| {
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", &rig.path)
            .env("HOME", &rig.home)
            .env("XDG_CONFIG_HOME", rig.home.join(".config"))
            .env("XDG_DATA_HOME", rig.home.join(".local/share"))
            .env("XDG_STATE_HOME", rig.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", rig._dir.path().join("run"))
            .env("GIT_CONFIG_GLOBAL", rig.home.join("gitconfig"))
            .env("GIT_CONFIG_SYSTEM", rig.home.join("gitconfig"))
            .env("VISSUE_ROOT", &rig.root)
            .env("VISSUE_STATE", &rig.state)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "mina")
            .env("VISSUE_AGENT", "mina")
            .env("SEAT_SESSION_ID", "sess-jev-0001")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", &pack)
            .env("PACKSET_WORKSPACE", "seat")
            .output()
            .unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), said)
    };
    let (ok, cast) = run(&["vote", "demo-ajqr", "--jev", "--as", "ada"]);
    let printed = cast
        .lines()
        .find(|line| line.contains(": cast "))
        .and_then(|line| line.split(": cast ").next())
        .unwrap_or("")
        .trim()
        .to_string();
    let cast_rows = rig.ballots();
    let (withdraw_ok, withdrawn) = run(&["vote", "demo-ajqr", "--withdraw", "--as", &printed]);
    let (tally_ok, tally) = run(&["vote", "demo-ajqr"]);
    let (recast_ok, recast) = run(&[
        "vote",
        "demo-ajqr",
        "--for",
        "no",
        "--used",
        "deed-alice",
        "--as",
        "ada",
    ]);
    let (stray_ok, stray_cast) = run(&["vote", "demo-stray", "--jev", "--as", "ada"]);
    let stray_rows = rig.ballots();
    let (persona_withdraw_ok, persona_withdrawn) =
        run(&["vote", "demo-stray", "--withdraw", "--as", "ada"]);
    let after_persona = rig.ballots();
    let (stray_tally_ok, stray_tally) = run(&["vote", "demo-stray"]);
    let (again_ok, again) = run(&[
        "vote",
        "demo-stray",
        "--for",
        "yes",
        "--used",
        "deed-alice",
        "--as",
        "ada",
    ]);
    let (again_tally_ok, again_tally) = run(&["vote", "demo-stray"]);
    assert!(
        ok && printed == "judge:stub-judge"
            && cast_rows.contains("\"agent\": \"judge:stub-judge\"")
            && !cast_rows.contains("\"agent\": \"ada\"")
            && withdraw_ok
            && withdrawn.contains("withdrawn")
            && tally_ok
            && tally.contains("0 ballots count")
            && recast_ok
            && !recast.contains("already cast")
            && stray_ok
            && stray_rows.contains("\"agent\": \"judge:stub-judge\"")
            && persona_withdraw_ok
            && persona_withdrawn.contains("withdrawn")
            && !after_persona.contains("judge:stub-judge")
            && stray_tally_ok
            && stray_tally.contains("0 ballots count")
            && again_ok
            && again_tally_ok
            && again_tally.contains("1 ballot counts")
            && !again_tally.contains("2 ballots"),
        "cast={cast}\nwithdraw={withdrawn}\ntally={tally}\nrecast={recast}\nstray={stray_cast}\npersona_withdraw={persona_withdrawn}\nstray_tally={stray_tally}\nagain={again}\nagain_tally={again_tally}\nballots={}",
        rig.ballots()
    );
}

#[test]
fn a_judge_id_withdraws_by_both_commands() {
    let rig = Rig::new();
    let row = r#"{"keep":[{"agent":"judge:stub-judge","choice":"yes","used":"judge:stub-judge"}]}"#;
    let judge = "judge:stub-judge";
    let refusal = "a persona name may use only";
    write_text(&rig.state.join("ballots.json"), row);
    let (_cli_ok, cli) = rig.ljos("mina", &["withdraw", "keep", "--as", judge]);
    let after_cli = rig.ballots();
    write_text(&rig.state.join("ballots.json"), row);
    let (_vote_ok, vote) = rig.ljos("mina", &["vote", "keep", "--withdraw", "--as", judge]);
    let after_vote = rig.ballots();
    let (_again_ok, again) = rig.ljos("mina", &["vote", "keep", "--withdraw", "--as", judge]);
    assert!(
        !cli.contains(refusal)
            && !after_cli.contains(judge)
            && cli.contains("forecast for judge:stub-judge")
            && !vote.contains(refusal)
            && !after_vote.contains(judge)
            && vote.contains("forecast for judge:stub-judge")
            && !again.contains(refusal)
            && again.contains("forecast for judge:stub-judge"),
        "cli={cli}\nafter_cli={after_cli}\nvote={vote}\nafter_vote={after_vote}\nagain={again}"
    );
}

/// One voter key: the pusher's spelling, a roster spelling, and a
/// conversation tag. `Tls` is the stored `tls`. `Alice` is the stored
/// `alice`. `inky@sha` is the seat `inky`, and a spaced name stays one
/// quoted token on a signature note.
#[test]
fn one_voter_key_folds_the_pusher_the_roster_and_the_seat() {
    let mut misses = Vec::new();
    let none = std::collections::BTreeSet::new();
    let lone = vec![serde_json::json!({"agent": "tls", "choice": "ship"})];
    match ljos_cli::seats_decide("cite-r1", &lone, &none, "Tls") {
        Err(err) if err.contains("the only seat ballot") && err.contains("Tls") => {}
        other => misses.push(format!("pusher Tls: {other:?}")),
    }
    match ljos_cli::seats_decide("cite-r1", &lone, &none, "tls") {
        Err(err) if err.contains("the only seat ballot") => {}
        other => misses.push(format!("pusher tls: {other:?}")),
    }

    let roster: std::collections::BTreeSet<String> = ["Alice".to_string()].into();
    let rows = vec![
        serde_json::json!({"agent": "alice", "choice": "ship"}),
        serde_json::json!({"agent": "tls", "choice": "ship"}),
    ];
    match ljos_cli::seats_decide("cite-r2", &rows, &roster, "tls") {
        Err(err) if err.contains("the only seat ballot") && !err.contains("alice for") => {}
        other => misses.push(format!("roster Alice: {other:?}")),
    }

    let (seat, _) = ljos_cli::holder_parts("inky@sha");
    let tagged = vec![serde_json::json!({"agent": "inky@sha", "choice": "yes"})];
    for pusher in ["inky", "inky@sha"] {
        match ljos_cli::seats_decide("cite-r3", &tagged, &none, pusher) {
            Err(err) if err.contains("the only seat ballot") => {}
            other => misses.push(format!("tag {pusher} holder {seat}: {other:?}")),
        }
    }
    if seat != "inky" {
        misses.push(format!("holder {seat}"));
    }
    if ljos_cli::voter_id("tls-rev1") != "tls-rev1" {
        misses.push(format!("pid suffix {}", ljos_cli::voter_id("tls-rev1")));
    }
    if ljos_cli::voter_id("inky@39u") == ljos_cli::voter_id("inky") {
        misses.push("a pin inky@39u must stay a different voter from inky".to_string());
    }

    let rig = Rig::new();
    write_text(&rig.home.join("gitconfig"), b"");
    let (ok, said) = rig.ljos(
        "inky@sha",
        &["vote", "tag", "--for", "yes", "--used", "none"],
    );
    let bare = rig.ballots();
    let notes = rig.notes();
    if !ok
        || !bare.contains("\"agent\": \"inky\"")
        || bare.contains("inky-sha")
        || bare.contains("inky@")
        || !notes.contains("ballot-sig: inky ")
        || notes.contains("inky-sha")
    {
        misses.push(format!(
            "stored seat: ok={ok} said={said} ballots={bare} notes={notes}"
        ));
    }

    let key = ljos_cli::ballot_sig::load_or_create(&rig.home.join("seat.key")).unwrap();
    let note =
        ljos_cli::ballot_sig::sign_bound(&key, "probe-space", "Disk Saver", "hold", "sess-1", "-");
    let quoted = ljos_cli::ballot_sig::parse_note(&note).is_some_and(|parsed| {
        parsed.voter == "Disk Saver" && ljos_cli::ballot_sig::verifies("probe-space", &parsed)
    });
    if !note.contains("\"Disk Saver\"") || !quoted {
        misses.push(format!("quoted seat: {note}"));
    }

    assert!(misses.is_empty(), "{}", misses.join("\n"));
}
