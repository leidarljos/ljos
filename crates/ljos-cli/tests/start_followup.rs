//! After `ljos_start` files and claims an issue, the next write in that
//! conversation stays on it. A second issue is not filed.

ljos_test_boot::harness!();
use std::io::{BufRead, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn ids(tracker: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn rec(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rec(&path, out);
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                let line = line.trim();
                if let Some(id) = line.strip_prefix(":ID:") {
                    let id = id.trim();
                    if !id.is_empty() {
                        out.push(id.to_string());
                    }
                }
            }
        }
    }
    rec(tracker, &mut out);
    out.sort();
    out.dedup();
    out
}

#[test]
fn the_write_after_start_stays_on_the_issue_start_opened() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let tracker = root.path().join("tracker");
    let product = root.path().join("app");
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&tracker).unwrap();
    std::fs::create_dir_all(&product).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(root.path().join("run")).unwrap();
    script(
        &bin,
        "claimdag",
        "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\n  list) echo '[]' ;;\n  *) echo 'gen=1' ;;\nesac\nexit 0\n",
    );
    script(&bin, "deedar", "#!/bin/sh\necho head\nexit 0\n");
    script(
        &bin,
        "vissue",
        "#!/bin/sh\necho \"vissue $*\" >> \"$CALLS\"\nexit 0\n",
    );
    let calls = root.path().join("calls.log");
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .current_dir(&product)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "saddle")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_URL", "off")
        .env("CALLS", &calls)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(40));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let call = |id: u32, name: &str, args: serde_json::Value| serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}});
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let send = |stdin: &mut std::process::ChildStdin, line: &serde_json::Value| {
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    };
    let read_id =
        |stdout: &mut BufReader<std::process::ChildStdout>, want: u64| -> serde_json::Value {
            let mut line = String::new();
            loop {
                line.clear();
                if stdout.read_line(&mut line).unwrap() == 0 {
                    panic!("server closed before answer {want}");
                }
                let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                    continue;
                };
                if value["id"] == want {
                    return value;
                }
            }
        };
    send(
        &mut stdin,
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"probe","version":"0"}}}),
    );
    send(
        &mut stdin,
        &serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    let _ = read_id(&mut stdout, 1);
    send(
        &mut stdin,
        &call(
            2,
            "ljos_start",
            serde_json::json!({"task": "fix the gate", "repo": product.display().to_string()}),
        ),
    );
    let mut answers = std::collections::BTreeMap::new();
    answers.insert(2, read_id(&mut stdout, 2));
    send(
        &mut stdin,
        &call(
            3,
            "ljos_file",
            serde_json::json!({"title": "a follow up write"}),
        ),
    );
    answers.insert(3, read_id(&mut stdout, 3));
    drop(stdin);
    let _ = child.wait();
    let text = |id: u64| {
        let answer = answers
            .get(&id)
            .unwrap_or_else(|| panic!("no answer {id}: {answers:?}"));
        assert!(
            answer.get("error").is_none() && answer["result"]["isError"] != true,
            "{answer}"
        );
        answer["result"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let started = text(2);
    let id = started
        .split("claimed ")
        .nth(1)
        .and_then(|rest| {
            rest.split(|c: char| c.is_whitespace() || c == '\\' || c == '"')
                .next()
        })
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| panic!("start did not claim an issue:\n{started}"))
        .to_string();
    let filed = text(3);
    assert!(
        filed.contains(&id),
        "the write did not stay on {id}:\n{filed}"
    );
    let found = ids(&tracker);
    assert_eq!(
        found,
        vec![id.clone()],
        "the conversation filed more than the issue start opened: {found:?}\nstart:\n{started}\nwrite:\n{filed}"
    );
    let board =
        std::fs::read_to_string(tracker.join("Software/app/issues.org")).unwrap_or_default();
    assert!(
        board.contains("a follow up write"),
        "the write was not attached:\n{board}"
    );
}
