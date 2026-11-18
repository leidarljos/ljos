//! The how-to for making a tracker and a deed store by hand.
//!
//! The test runs the shell blocks of `docs/orgmode/howto-by-hand.org` in a
//! fresh home with no `ljos onboard`, then files a ticket and reads
//! `ljos doctor`. The rst page has to carry the same blocks, and the
//! refusal `ljos file` prints with no tracker has to name the page.

#[path = "common/boot.rs"]
mod boot;
#[path = "common/tools.rs"]
mod tools;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PAGE: &str = "howto-by-hand";
const URL: &str = "https://leidarljos.github.io/ljos/howto-by-hand.html";

fn docs(sub: &str, name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs")
        .join(sub)
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn org_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut open: Option<String> = None;
    for line in text.lines() {
        match &mut open {
            Some(body) if line.starts_with("#+end_src") => {
                blocks.push(std::mem::take(body));
                open = None;
            }
            Some(body) => {
                body.push_str(line);
                body.push('\n');
            }
            None if line.starts_with("#+begin_src bash") => open = Some(String::new()),
            None => {}
        }
    }
    blocks
}

fn rst_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut open: Option<String> = None;
    for line in text.lines() {
        match &mut open {
            Some(body) if !line.is_empty() && !line.starts_with("   ") => {
                blocks.push(std::mem::take(body));
                open = None;
            }
            Some(body) => {
                body.push_str(line.strip_prefix("   ").unwrap_or(""));
                body.push('\n');
            }
            None if line.starts_with(".. code:: bash") => open = Some(String::new()),
            None => {}
        }
    }
    if let Some(body) = open {
        blocks.push(body);
    }
    blocks
        .into_iter()
        .map(|block| format!("{}\n", block.trim_matches('\n')))
        .collect()
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[test]
fn the_rst_page_carries_the_same_shell_blocks() {
    let org = org_blocks(&docs("orgmode", "howto-by-hand.org"));
    let rst = rst_blocks(&docs("source", "howto-by-hand.rst"));
    assert_eq!(org.len(), 2, "{org:?}");
    assert_eq!(org, rst);
    for index in ["howto.org", "howto.rst"] {
        let sub = if index.ends_with("org") {
            "orgmode"
        } else {
            "source"
        };
        assert!(
            docs(sub, index).contains(PAGE),
            "{index} does not list {PAGE}"
        );
    }
}

#[test]
fn the_how_to_makes_a_seat_that_files_and_has_no_missing_rows() {
    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["vissue", "claimdag", "deedar", "packset", "packsetd"] {
        if let Some(path) = which(name) {
            std::os::unix::fs::symlink(path, bin.join(name)).unwrap();
        }
    }
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos"), bin.join("ljos")).unwrap();
    let script = format!(
        "set -e\n{}ljos file 'First ticket' -p demo\nljos doctor || true\n",
        org_blocks(&docs("orgmode", "howto-by-hand.org")).concat()
    );
    let out = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .current_dir(&home)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("USER", "walker")
        .env("TMPDIR", root.path())
        .env("LJOS_SEAT", "testseat")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_NO_AUTOSTART", "1")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("PACKSET_URL", "off")
        .env("VISSUE_NO_ROUTE", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Walker")
        .env("GIT_AUTHOR_EMAIL", "walker@example.org")
        .env("GIT_COMMITTER_NAME", "Walker")
        .env("GIT_COMMITTER_EMAIL", "walker@example.org")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}{stderr}");
    assert!(
        stdout.contains("demo-"),
        "the ticket was filed: {stdout}{stderr}"
    );
    for row in ["info\ttickets", "info\tdeeds", "no\ttickets", "no\tdeeds"] {
        assert!(!stdout.contains(row), "{row} is still printed: {stdout}");
    }
    assert!(
        home.join(".local/share/deedar/store").is_dir(),
        "the deed store was made"
    );
    assert!(
        !stderr.contains("not a git checkout"),
        "the tracker is a checkout: {stderr}"
    );
}

#[test]
fn the_refusal_with_no_tracker_names_the_how_to() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["ready"])
        .current_dir(&home)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("TMPDIR", root.path())
        .env("LJOS_SEAT", "testseat")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_URL", "off")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("so run `ljos onboard`."), "{err}");
    assert!(err.contains(URL), "{err}");
}
