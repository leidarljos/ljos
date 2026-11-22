//! `ljos_sitting` on an id the tracker does not hold.
//!
//! The call names the missing issue and leaves the product checkout
//! untouched. A tracker command run in that checkout used to write
//! `vissue.toml` there.

ljos_test_boot::harness!();

#[path = "common/git.rs"]
mod isolated_git;
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
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
        let out = isolated_git::command()
            .arg("-C")
            .arg(&product)
            .args(args)
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
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
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
            "name":"ljos_sitting","arguments":{"seat":"saddle","issue": MISSING}}}),
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

/// A product checkout and a tracker that writes `vissue.toml` on any call.
struct Probe {
    _root: tempfile::TempDir,
    product: PathBuf,
    bin: PathBuf,
    home: PathBuf,
    run: PathBuf,
}

impl Probe {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let product = root.path().join("product");
        let bin = root.path().join("bin");
        let run = root.path().join("run");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&product).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&run).unwrap();
        let git = |args: &[&str]| {
            let out = isolated_git::command()
                .arg("-C")
                .arg(&product)
                .args(args)
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
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos"), bin.join("ljos")).unwrap();
        Self {
            _root: root,
            product,
            bin,
            home,
            run,
        }
    }

    fn tree(&self) -> Vec<String> {
        files(&self.product)
    }

    fn command(&self, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(&self.product)
            .env_clear()
            .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .env("VISSUE_NO_ROUTE", "1");
        cmd
    }

    fn ljos(&self, args: &[&str]) -> Output {
        self.command(Path::new(env!("CARGO_BIN_EXE_ljos")))
            .args(args)
            .output()
            .unwrap()
    }
}

fn refusal(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn names_the_id_and_leaves_the_tree(out: &Output, before: &[String], probe: &Probe) {
    let text = refusal(out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains(MISSING),
        "the error did not name {MISSING}:\n{text}"
    );
    assert!(
        text.contains("`ljos search`"),
        "the error did not name `ljos search`:\n{text}"
    );
    assert!(
        !text.contains('\n') || text.lines().filter(|line| !line.is_empty()).count() == 1,
        "the error was not one line:\n{text}"
    );
    assert_eq!(
        probe.tree(),
        before,
        "the product checkout was written:\n{text}"
    );
    assert!(!probe.product.join("vissue.toml").exists(), "{text}");
}

#[test]
fn a_cli_sitting_on_a_missing_issue_names_it_and_writes_nothing() {
    let probe = Probe::new();
    let before = probe.tree();
    let out = probe.ljos(&["sitting", MISSING]);
    names_the_id_and_leaves_the_tree(&out, &before, &probe);
}

#[test]
fn note_finish_file_and_claim_on_a_missing_issue_write_nothing() {
    let probe = Probe::new();
    let before = probe.tree();
    for args in [
        vec!["note", MISSING, "seen again"],
        vec!["finish", MISSING],
        vec!["file", "--parent", MISSING, "child work"],
        vec!["claim", MISSING],
    ] {
        let out = probe.ljos(&args);
        let text = refusal(&out);
        assert_eq!(
            probe.tree(),
            before,
            "ljos {args:?} wrote the product checkout:\n{text}"
        );
        assert!(
            !probe.product.join("vissue.toml").exists(),
            "ljos {args:?} wrote vissue.toml:\n{text}"
        );
        if args[0] == "note" {
            assert!(!out.status.success(), "{text}");
            assert!(
                text.contains(MISSING) && text.contains("`ljos search`"),
                "ljos note did not name {MISSING} and `ljos search`:\n{text}"
            );
        } else if args[0] == "claim" {
            assert!(!out.status.success(), "{text}");
            assert!(out.stdout.is_empty(), "{text}");
            assert_eq!(
                String::from_utf8_lossy(&out.stderr).trim(),
                format!("issue {MISSING} not found. `ljos ready` lists the open tickets.")
            );
        }
    }
}

#[test]
fn mcp_write_verbs_on_a_missing_issue_name_it_and_write_nothing() {
    let probe = Probe::new();
    let before = probe.tree();
    let calls = [
        serde_json::json!({"name":"ljos_sitting","arguments":{"seat":"saddle","issue": MISSING}}),
        serde_json::json!({"name":"ljos_note","arguments":{"seat":"saddle","issue": MISSING, "text":"seen again"}}),
        serde_json::json!({"name":"ljos_finish","arguments":{"seat":"saddle","issue": MISSING}}),
        serde_json::json!({"name":"ljos_file","arguments":{"seat":"saddle","title":"child work","parent": MISSING}}),
        serde_json::json!({"name":"ljos_claim","arguments":{"seat":"saddle","node": MISSING}}),
    ];
    let mut child = probe
        .command(Path::new(env!("CARGO_BIN_EXE_ljos-mcp")))
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
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"probe","version":"0"}}})
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    for (n, params) in calls.iter().enumerate() {
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id": n + 2, "method":"tools/call","params": params})
        )
        .unwrap();
    }
    stdin.flush().unwrap();
    drop(stdin);
    let mut answers = Vec::new();
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value["id"].as_u64().is_some_and(|id| id >= 2) {
            answers.push(value);
        }
        if answers.len() == calls.len() {
            break;
        }
    }
    let _ = child.wait();
    assert_eq!(answers.len(), calls.len(), "{answers:?}");
    let sitting = answers
        .iter()
        .find(|answer| answer["id"] == 2)
        .unwrap_or_else(|| panic!("no sitting answer:\n{answers:?}"));
    let text = sitting["result"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        sitting["result"]["isError"] == true || sitting.get("error").is_some(),
        "{sitting}"
    );
    assert!(
        text.contains(MISSING),
        "the error did not name {MISSING}:\n{text}"
    );
    assert!(
        text.contains("`ljos search`"),
        "the error did not name `ljos search`:\n{text}"
    );
    assert_eq!(
        probe.tree(),
        before,
        "the product checkout was written:\n{answers:?}"
    );
    assert!(!probe.product.join("vissue.toml").exists());
}
