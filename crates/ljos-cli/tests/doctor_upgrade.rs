//! After an upgrade, a skill or a runner env ljos wrote is refreshed, and a
//! hand edit is not. An env file ljos did not write is left alone. A global
//! hooks path the user already set is covered in `git_hooks.rs`.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const OLD_SKILL: &str = "---\nname: ljos\ndescription: old\n---\n\n# The seat protocol\n\nold\n";

/// An env an older ljos wrote: the seat header, a shell block this ljos no
/// longer writes, and the guard.
const OLD_ENV: &str = "\
# Source this in the shell that runs the agent's commands.\n\
export LJOS_SEAT=\"${LJOS_SEAT:-grokbot}\"\n\
if [ -z \"${LJOS_SEAT:-}\" ]; then\n\
\t_ljos_named=old\n\
fi\n\
export LJOS_OLD_MARKER=1\n\
_ljos_guard=\"${XDG_DATA_HOME:-$HOME/.local/share}/ljos/guard-bin\"\n";

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

#[test]
fn a_same_version_stamp_from_an_older_build_does_not_block_skill_refresh() {
    let home = Home::new();
    let cursor = home.root.join("cursor-skills/ljos/SKILL.md");
    let antigravity = home.root.join("antigravity-skills/ljos/SKILL.md");
    plant(&cursor, OLD_SKILL, OLD_SKILL);
    plant(&antigravity, OLD_SKILL, OLD_SKILL);
    harnesses(
        &home,
        cursor.parent().unwrap().parent().unwrap(),
        antigravity.parent().unwrap().parent().unwrap(),
    );
    let stamp = home.root.join("share/ljos/skill-refresh-version");
    fs::create_dir_all(stamp.parent().unwrap()).unwrap();
    fs::write(&stamp, format!("{}\n", env!("CARGO_PKG_VERSION"))).unwrap();

    let out = home.ljos(&["protocol"], "30");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("refreshed cursor:"), "{err}");
    let now = fs::read_to_string(&cursor).unwrap();
    assert!(now.contains("\n# The seat protocol\n"), "{now}");
    assert_ne!(now, OLD_SKILL);
}

fn row<'a>(text: &'a str, what: &str) -> &'a str {
    text.lines()
        .find(|line| line.split('\t').nth(1) == Some(what))
        .unwrap_or("")
}

/// A grokbot env an older ljos wrote, and a stale skill ljos wrote, are not
/// ready. `ljos onboard --harness grokbot` makes both rows ok.
#[test]
fn onboard_turns_a_stale_grokbot_env_and_skill_green() {
    let home = Home::new();
    let config = home.root.join("config/ljos");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(home.root.join("home")).unwrap();
    fs::write(
        config.join("harnesses.toml"),
        "[[harness]]\nname = \"grokbot\"\n",
    )
    .unwrap();
    let skill = home.root.join("home/.grokbot/skills/ljos/SKILL.md");
    plant(&skill, OLD_SKILL, OLD_SKILL);
    fs::write(config.join("grokbot.env"), OLD_ENV).unwrap();

    let before = home.ljos(&["doctor", "--all"], "120");
    let text = String::from_utf8(before.stdout).unwrap();
    let env = row(&text, "runner env");
    let skill_row = row(&text, "runner skill");
    assert!(
        env.starts_with("warn\t")
            && env.contains("grokbot")
            && env.contains("is stale")
            && env.contains("ljos doctor --fix"),
        "{text}"
    );
    assert!(!env.starts_with("no\t"), "{text}");
    assert!(
        skill_row.starts_with("warn\t")
            && skill_row.contains("grokbot")
            && skill_row.contains("stale"),
        "{text}"
    );

    let onboard = home.ljos(&["onboard", "--harness", "grokbot"], "120");
    let onboard_out = format!(
        "{}\n{}",
        String::from_utf8_lossy(&onboard.stdout),
        String::from_utf8_lossy(&onboard.stderr)
    );
    let after = home.ljos(&["doctor", "--all"], "120");
    let text = String::from_utf8(after.stdout).unwrap();
    let env = row(&text, "runner env");
    let skill_row = row(&text, "runner skill");
    assert!(
        env.starts_with("ok\t") && env.contains("grokbot"),
        "onboard:\n{onboard_out}\ndoctor:\n{text}"
    );
    assert!(
        skill_row.starts_with("ok\t") && skill_row.contains("grokbot"),
        "onboard:\n{onboard_out}\ndoctor:\n{text}"
    );
    let got = fs::read_to_string(config.join("grokbot.env")).unwrap();
    assert!(got.contains("export LJOS_GIT_HOOKS="), "{got}");
    assert!(!got.contains("LJOS_OLD_MARKER"), "{got}");
}

fn env_record(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap().to_string_lossy();
    path.with_file_name(format!(".{name}.ljos-wrote"))
}

/// The first run rewrites a stale env, keeps a line the user added, and
/// does not touch a file ljos did not write or a hand edit.
#[test]
fn the_first_run_refreshes_a_stale_env_and_leaves_what_ljos_did_not_write() {
    let home = Home::new();
    let config = home.root.join("config/ljos");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(home.root.join("home")).unwrap();
    fs::write(
        config.join("harnesses.toml"),
        "[[harness]]\nname = \"grokbot\"\n\n[[harness]]\nname = \"shell\"\n\n[[harness]]\nname = \"box\"\nshell = true\n",
    )
    .unwrap();
    let grok = config.join("grokbot.env");
    let foreign = config.join("shell.env");
    let edited = config.join("box.env");
    let user = "export LJOS_EXTRA=user\nexport BOX_NOTES=keep-me\n";
    fs::write(&grok, format!("{OLD_ENV}{user}")).unwrap();
    fs::write(env_record(&grok), OLD_ENV).unwrap();
    let foreign_text = "# written by an older ljos\nexport BOX_NOTES=keep-me\n";
    fs::write(&foreign, foreign_text).unwrap();
    let body = "\
# Source this in the shell that runs the agent's commands.\n\
export LJOS_SEAT=\"${LJOS_SEAT:-box}\"\n\
_ljos_guard=\"${XDG_DATA_HOME:-$HOME/.local/share}/ljos/guard-bin\"\n";
    let hand = body.replacen(
        "# Source this in the shell that runs the agent's commands.",
        "# Source this in the shell that runs the agent's commands. edited",
        1,
    );
    fs::write(&edited, &hand).unwrap();
    fs::write(env_record(&edited), body).unwrap();

    let out = home.ljos(&["protocol"], "30");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("refreshed grokbot:"), "{err}");
    assert!(
        err.contains("LJOS_GIT_HOOKS") && err.contains("kept") && err.contains("BOX_NOTES"),
        "{err}"
    );
    assert!(err.contains("was not written by ljos"), "{err}");
    assert!(err.contains("was edited by hand"), "{err}");
    let got = fs::read_to_string(&grok).unwrap();
    assert!(
        got.contains("export LJOS_GIT_HOOKS=")
            && got.contains("export LJOS_EXTRA=user\n")
            && got.contains("export BOX_NOTES=keep-me\n"),
        "{got}"
    );
    assert!(!got.contains("LJOS_OLD_MARKER"), "{got}");
    let syntax = Command::new("sh").arg("-n").arg(&grok).output().unwrap();
    assert!(
        syntax.status.success(),
        "{}",
        String::from_utf8_lossy(&syntax.stderr)
    );
    assert_eq!(fs::read(&foreign).unwrap(), foreign_text.as_bytes());
    assert!(!env_record(&foreign).exists());
    assert_eq!(fs::read(&edited).unwrap(), hand.as_bytes());

    let again = home.ljos(&["protocol"], "30");
    assert!(again.status.success());
    assert!(!String::from_utf8_lossy(&again.stderr).contains("refreshed"));
    assert_eq!(fs::read_to_string(&grok).unwrap(), got);
    assert!(home.root.join("share/ljos/env-refresh-version").is_file());
}

#[test]
fn a_same_version_stamp_from_an_older_build_does_not_block_env_refresh() {
    let home = Home::new();
    let config = home.root.join("config/ljos");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(home.root.join("home")).unwrap();
    fs::write(
        config.join("harnesses.toml"),
        "[[harness]]\nname = \"grokbot\"\n",
    )
    .unwrap();
    let env_path = config.join("grokbot.env");
    fs::write(&env_path, format!("{OLD_ENV}export BOX_NOTES=keep-me\n")).unwrap();
    fs::write(env_record(&env_path), OLD_ENV).unwrap();
    let stamp = home.root.join("share/ljos/env-refresh-version");
    fs::create_dir_all(stamp.parent().unwrap()).unwrap();
    fs::write(&stamp, format!("{}\n", env!("CARGO_PKG_VERSION"))).unwrap();

    let out = home.ljos(&["protocol"], "30");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("refreshed grokbot:") && err.contains("BOX_NOTES"),
        "{err}"
    );
    let got = fs::read_to_string(&env_path).unwrap();
    assert!(
        got.contains("export LJOS_GIT_HOOKS=") && got.contains("export BOX_NOTES=keep-me\n"),
        "{got}"
    );
}

#[test]
fn the_mcp_server_refreshes_a_stale_env_on_start() {
    let home = Home::new();
    let config = home.root.join("config/ljos");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(home.root.join("home")).unwrap();
    fs::write(
        config.join("harnesses.toml"),
        "[[harness]]\nname = \"grokbot\"\n",
    )
    .unwrap();
    let env_path = config.join("grokbot.env");
    fs::write(&env_path, format!("{OLD_ENV}export BOX_NOTES=keep-me\n")).unwrap();

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
        if fs::read(&env_path).unwrap() != format!("{OLD_ENV}export BOX_NOTES=keep-me\n").as_bytes()
        {
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
    assert!(changed, "ljos-mcp did not refresh the env\n{err}");
    assert!(
        err.contains("refreshed grokbot:") && err.contains("BOX_NOTES"),
        "{err}"
    );
    assert!(fs::read_to_string(&env_path)
        .unwrap()
        .contains("export LJOS_GIT_HOOKS="));
}

#[test]
fn doctor_warns_about_a_stale_env_and_fix_refreshes_it() {
    let home = Home::new();
    let config = home.root.join("config/ljos");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(home.root.join("home")).unwrap();
    fs::write(
        config.join("harnesses.toml"),
        "[[harness]]\nname = \"grokbot\"\n",
    )
    .unwrap();
    let env_path = config.join("grokbot.env");
    let stale = format!("{OLD_ENV}export BOX_NOTES=keep-me\n");
    fs::write(&env_path, &stale).unwrap();

    let out = home.ljos(&["doctor"], "120");
    assert_eq!(fs::read(&env_path).unwrap(), stale.as_bytes());
    assert!(!home.root.join("share/ljos/env-refresh-version").exists());
    let text = String::from_utf8(out.stdout).unwrap();
    let env = row(&text, "runner env");
    assert!(
        env.starts_with("warn\t") && env.contains("is stale") && env.contains("ljos doctor --fix"),
        "{text}"
    );
    assert!(!text.contains("no\trunner env"), "{text}");

    let fixed = home.ljos(&["doctor", "--all", "--fix"], "120");
    let err = String::from_utf8_lossy(&fixed.stderr);
    assert!(
        err.contains("refreshed grokbot:")
            && err.contains("LJOS_GIT_HOOKS")
            && err.contains("BOX_NOTES"),
        "{err}"
    );
    let got = fs::read_to_string(&env_path).unwrap();
    assert!(
        got.contains("export LJOS_GIT_HOOKS=") && got.contains("export BOX_NOTES=keep-me\n"),
        "{got}"
    );
    let after = String::from_utf8(fixed.stdout).unwrap();
    assert!(row(&after, "runner env").starts_with("ok\t"), "{after}");

    fs::write(&env_path, &stale).unwrap();
    let _ = fs::remove_file(env_record(&env_path));
    let quiet = home.ljos(&["protocol"], "30");
    assert!(!String::from_utf8_lossy(&quiet.stderr).contains("refreshed"));
    assert_eq!(fs::read(&env_path).unwrap(), stale.as_bytes());
    let again = home.ljos(&["doctor", "--all", "--fix"], "120");
    assert!(String::from_utf8_lossy(&again.stderr).contains("refreshed grokbot:"));
    assert!(fs::read_to_string(&env_path)
        .unwrap()
        .contains("export LJOS_GIT_HOOKS="));
}

#[test]
fn doctor_leaves_an_env_ljos_did_not_write() {
    let home = Home::new();
    let config = home.root.join("config/ljos");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(home.root.join("home")).unwrap();
    fs::write(
        config.join("harnesses.toml"),
        "[[harness]]\nname = \"grokbot\"\n",
    )
    .unwrap();
    let env_path = config.join("grokbot.env");
    let kept = "# written by an older ljos\nexport BOX_NOTES=keep-me\n";
    fs::write(&env_path, kept).unwrap();

    let doctor = home.ljos(&["doctor", "--all"], "120");
    let text = String::from_utf8(doctor.stdout).unwrap();
    let env = row(&text, "runner env");
    assert!(
        env.starts_with("no\t") && env.contains("was not written by ljos"),
        "{env}"
    );
    let fixed = home.ljos(&["doctor", "--fix"], "120");
    assert!(String::from_utf8_lossy(&fixed.stderr).contains("was not written by ljos"));
    assert_eq!(fs::read(&env_path).unwrap(), kept.as_bytes());
    assert!(!env_record(&env_path).exists());
    let onboard = home.ljos(&["onboard", "--harness", "grokbot"], "120");
    let said = format!(
        "{}\n{}",
        String::from_utf8_lossy(&onboard.stdout),
        String::from_utf8_lossy(&onboard.stderr)
    );
    assert!(said.contains("did not write"), "{said}");
    assert_eq!(fs::read(&env_path).unwrap(), kept.as_bytes());
    assert!(!env_record(&env_path).exists());
}
