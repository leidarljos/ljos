//! `ljos_sitting` on an id the tracker does not hold.
//!
//! The call names the missing issue and leaves the product checkout
//! untouched. A tracker command run in that checkout used to write
//! `vissue.toml` there.

ljos_test_boot::harness!();
use std::io::{BufRead, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

const MISSING: &str = "missing-ab12";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn files(dir: &Path) -> Vec<String> {
    fn rec(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            out.push(path.strip_prefix(root).unwrap().display().to_string());
            if path.is_dir() {
                rec(&path, root, out);
            }
        }
    }
    let mut out = Vec::new();
    rec(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn a_sitting_on_a_missing_issue_names_it_and_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let product = root.path().join("product");
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&product).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(root.path().join("run")).unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&product)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(product.join("README"), "a product\n").unwrap();
    git(&["add", "README"]);
    git(&[
        "-c",
        "user.name=seat",
        "-c",
        "user.email=seat@example.test",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "-m",
        "product",
    ]);
    // Any tracker command in the product checkout leaves `vissue.toml`.
    script(
        &bin,
        "vissue",
        "#!/bin/sh\nprintf 'prefix = \"Software\"\\n' > vissue.toml\nexit 0\n",
    );
    script(
        &bin,
        "claimdag",
        "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\n  list) echo '[]' ;;\n  *) echo 'gen=1' ;;\nesac\nexit 0\n",
    );
    script(&bin, "deedar", "#!/bin/sh\necho head\nexit 0\n");
    let before = files(&product);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .current_dir(&product)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LJOS_SEAT", "saddle")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_URL", "off")
        .env("VISSUE_NO_ROUTE", "1")
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
    let mut stdin = child.stdin.take().unwrap();
    for line in [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"probe","version":"0"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"ljos_sitting","arguments":{"issue": MISSING}}}),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    stdin.flush().unwrap();
    drop(stdin);
    let mut answer = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value["id"] == 2 {
            answer = Some(value);
            break;
        }
    }
    let _ = child.wait();
    let answer = answer.expect("no answer from ljos_sitting");
    let text = answer["result"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        answer["result"]["isError"] == true || answer.get("error").is_some(),
        "{answer}"
    );
    assert!(
        text.contains(MISSING),
        "the error did not name {MISSING}:\n{text}"
    );
    assert_eq!(
        files(&product),
        before,
        "the product checkout was written:\n{text}"
    );
    assert!(
        !product.join("vissue.toml").exists(),
        "vissue.toml was written into the product checkout"
    );
}
