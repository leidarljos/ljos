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
