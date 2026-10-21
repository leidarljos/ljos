//! After an upgrade, a skill ljos wrote is refreshed, and a hand edit is not.
//! A global hooks path the user already set is covered in `git_hooks.rs`.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const OLD_SKILL: &str = "---\nname: ljos\ndescription: old\n---\n\n# The seat protocol\n\nold\n";

struct Home {
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/doctor-upgrade-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn env(&self) -> Vec<(String, String)> {
        vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), self.root.join("home").display().to_string()),
            (
                "XDG_CONFIG_HOME".into(),
                self.root.join("config").display().to_string(),
            ),
            (
                "XDG_DATA_HOME".into(),
                self.root.join("share").display().to_string(),
            ),
            (
                "XDG_RUNTIME_DIR".into(),
                self.root.join("run").display().to_string(),
            ),
            (
                "XDG_STATE_HOME".into(),
                self.root.join("state").display().to_string(),
            ),
            (
                "GIT_CONFIG_GLOBAL".into(),
                self.root.join("gitconfig").display().to_string(),
            ),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("PACKSET_URL".into(), "off".into()),
        ]
    }

    fn ljos(&self, args: &[&str], limit_secs: &str) -> Output {
        let out = Command::new("timeout")
            .arg(limit_secs)
            .arg(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .envs(self.env())
            .output()
            .unwrap();
        assert_ne!(
            out.status.code(),
            Some(124),
            "ljos {args:?} timed out\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn sha256(bytes: &[u8]) -> String {
    let mut child = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// `written` is what ljos last wrote. `now` is the file as it stands.
fn plant(path: &Path, written: &str, now: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, now).unwrap();
    fs::write(
        path.with_file_name(".ljos-skill-sha256"),
        format!("{}\n", sha256(written.as_bytes())),
    )
    .unwrap();
}

fn harnesses(home: &Home, cursor: &Path, antigravity: &Path) {
    let dir = home.root.join("config/ljos");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(home.root.join("home")).unwrap();
    fs::write(
        dir.join("harnesses.toml"),
        format!(
            "[[harness]]\nname = \"cursor\"\nskills = \"{}\"\n\n[[harness]]\nname = \"antigravity\"\nskills = \"{}\"\n",
            cursor.display(),
            antigravity.display()
        ),
    )
    .unwrap();
}

fn skill_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.split('\t').nth(1) == Some("runner skill"))
        .collect()
}

#[test]
fn the_first_run_refreshes_a_stale_skill_and_leaves_a_hand_edit() {
    let home = Home::new();
    let cursor = home.root.join("cursor-skills/ljos/SKILL.md");
    let antigravity = home.root.join("antigravity-skills/ljos/SKILL.md");
    let edited = format!("{OLD_SKILL}hand edited line\n");
    plant(&cursor, OLD_SKILL, OLD_SKILL);
    plant(&antigravity, OLD_SKILL, &edited);
    harnesses(
        &home,
        cursor.parent().unwrap().parent().unwrap(),
        antigravity.parent().unwrap().parent().unwrap(),
    );
    let kept = fs::read(&antigravity).unwrap();

    let out = home.ljos(&["protocol"], "30");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("refreshed cursor:"), "{err}");
    assert!(err.contains("was edited by hand; ljos left it"), "{err}");
    let now = fs::read(&cursor).unwrap();
    let text = String::from_utf8(now.clone()).unwrap();
    assert!(text.contains("\n# The seat protocol\n"), "{text}");
    assert_ne!(text, OLD_SKILL);
    let side = fs::read_to_string(cursor.with_file_name(".ljos-skill-sha256")).unwrap();
    assert_eq!(side.trim(), sha256(&now));
    assert_eq!(fs::read(&antigravity).unwrap(), kept);

    let again = home.ljos(&["protocol"], "30");
    assert!(again.status.success());
    assert!(
        !String::from_utf8_lossy(&again.stderr).contains("refreshed"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert_eq!(fs::read(&cursor).unwrap(), now);
    assert_eq!(fs::read(&antigravity).unwrap(), kept);
    let stamp = home.root.join("share/ljos/skill-refresh-version");
    assert!(stamp.is_file(), "the version stamp was not written");
}

#[test]
fn doctor_warns_about_a_stale_skill_and_fix_refreshes_it() {
    let home = Home::new();
    let cursor = home.root.join("cursor-skills/ljos/SKILL.md");
    let antigravity = home.root.join("antigravity-skills/ljos/SKILL.md");
    let edited = format!("{OLD_SKILL}hand edited line\n");
    plant(&cursor, OLD_SKILL, OLD_SKILL);
    plant(&antigravity, OLD_SKILL, &edited);
    harnesses(
        &home,
        cursor.parent().unwrap().parent().unwrap(),
        antigravity.parent().unwrap().parent().unwrap(),
    );
    let stale = fs::read(&cursor).unwrap();
    let kept = fs::read(&antigravity).unwrap();

    let out = home.ljos(&["doctor"], "120");
    assert_eq!(fs::read(&cursor).unwrap(), stale, "doctor rewrote a skill");
    assert_eq!(fs::read(&antigravity).unwrap(), kept);
    assert!(!home.root.join("share/ljos/skill-refresh-version").exists());
    let text = String::from_utf8(out.stdout).unwrap();
    let lines = skill_lines(&text);
    assert!(
        lines.iter().any(|line| {
            line.starts_with("warn\t")
                && line.contains("is stale")
                && line.contains("ljos doctor --fix")
        }),
        "{text}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("warn\t") && line.contains("was edited by hand")),
        "{text}"
    );
    assert!(lines.iter().all(|line| !line.starts_with("no\t")), "{text}");

    let fixed = home.ljos(&["doctor", "--fix"], "120");
    let err = String::from_utf8_lossy(&fixed.stderr);
    assert!(err.contains("refreshed cursor:"), "{err}");
    assert!(err.contains("was edited by hand; ljos left it"), "{err}");
    let now = fs::read(&cursor).unwrap();
    assert_ne!(now, stale);
    assert!(String::from_utf8_lossy(&now).contains("\n# The seat protocol\n"));
    assert_eq!(
        fs::read_to_string(cursor.with_file_name(".ljos-skill-sha256"))
            .unwrap()
            .trim(),
        sha256(&now)
    );
    assert_eq!(fs::read(&antigravity).unwrap(), kept);
    let after = String::from_utf8(fixed.stdout).unwrap();
    assert!(!after.contains("is stale"), "{after}");
    let lines = skill_lines(&after);
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("warn\t") && line.contains("was edited by hand")),
        "{after}"
    );
    assert!(
        lines.iter().all(|line| !line.starts_with("no\t")),
        "{after}"
    );
}

#[test]
fn the_mcp_server_refreshes_a_stale_skill_on_start() {
    let home = Home::new();
    let cursor = home.root.join("cursor-skills/ljos/SKILL.md");
    let antigravity = home.root.join("antigravity-skills/ljos/SKILL.md");
    let edited = format!("{OLD_SKILL}hand edited line\n");
    plant(&cursor, OLD_SKILL, OLD_SKILL);
    plant(&antigravity, OLD_SKILL, &edited);
    harnesses(
        &home,
        cursor.parent().unwrap().parent().unwrap(),
        antigravity.parent().unwrap().parent().unwrap(),
    );
    let kept = fs::read(&antigravity).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .env_clear()
        .envs(home.env())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _stdin = child.stdin.take();
    let mut stderr = child.stderr.take().unwrap();
    let started = Instant::now();
    let changed = loop {
        if fs::read(&cursor).unwrap() != OLD_SKILL.as_bytes() {
            break true;
        }
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            let _ = stderr.read_to_string(&mut err);
            panic!("ljos-mcp exited {status} before refreshing\n{err}");
        }
        if started.elapsed() > Duration::from_secs(20) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = child.kill();
    let _ = child.wait();
    let mut err = String::new();
    let _ = stderr.read_to_string(&mut err);
    assert!(changed, "ljos-mcp did not refresh the skill\n{err}");
    assert!(err.contains("refreshed cursor:"), "{err}");
    assert!(err.contains("was edited by hand; ljos left it"), "{err}");
    assert!(
        String::from_utf8_lossy(&fs::read(&cursor).unwrap()).contains("\n# The seat protocol\n")
    );
    assert_eq!(fs::read(&antigravity).unwrap(), kept);
}
