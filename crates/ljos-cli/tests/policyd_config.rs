//! `policyd_config` in the person's `ljos.toml` names the file
//! `ljos-policyd check --config` reads. The policyd here stands in for one
//! whose config takes `block-on-warning true`: a warning is then a deny
//! with the same rule and warning, and `check` exits 2. A config it cannot
//! open fails open. The warnings are the lines ljos-policyd 0.7.0 prints.

ljos_test_boot::harness!();
use std::io::Write;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const POLICYD: &str = r#"#!/bin/sh
[ "$1" = check ] && shift
conf=
if [ "$1" = --config ]; then conf=$2; shift 2; fi
[ "$1" = -- ] && shift
case "$*" in
  *sudo*) rule=sudo; said='a privilege runner is on the command' ;;
  *"rm -r"*) rule=rm-rf-outside-tmp; said='a recursive delete reaches outside /tmp and /var/tmp' ;;
  *) echo allow; exit 0 ;;
esac
if [ -n "$conf" ]; then
  if [ ! -f "$conf" ]; then
    printf 'allow\tfail-open\tmissing config\n'
    exit 0
  fi
  while IFS= read -r line; do
    if [ "$line" = 'block-on-warning true' ]; then
      printf 'deny\t%s\t%s\n' "$rule" "$said"
      exit 2
    fi
  done < "$conf"
fi
printf 'allow\twarn\t%s\t%s\n' "$rule" "$said"
"#;

const BLOCKS: &str = "block-on-warning true\n";

struct Seat {
    home: tempfile::TempDir,
    seat: Option<&'static str>,
}

impl Seat {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        for dir in ["bin", "guard", "work"] {
            std::fs::create_dir_all(home.path().join(dir)).unwrap();
        }
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(home.path().join("work"))
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        let seat = Self { home, seat: None };
        seat.executable("bin/ljos-policyd", POLICYD);
        seat.executable("guard/rm", ljos_cli::GUARD_SHIM);
        seat
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.home.path().join(rel)
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn executable(&self, rel: &str, body: &str) {
        self.write(rel, body);
        std::fs::set_permissions(self.path(rel), std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn env(&self, cmd: &mut Command) {
        let home = self.home.path();
        cmd.current_dir(home.join("work"))
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("PACKSET_URL", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_GUARD", "on")
            .env("LJOS_OFF", "0")
            .env_remove("POLICYD_REQUIRED")
            .env_remove("LJOS_HOOK_RELAY")
            .env_remove("LJOS_CONFIG");
        match self.seat {
            Some(seat) => cmd.env("LJOS_SEAT", seat),
            None => cmd.env_remove("LJOS_SEAT"),
        };
    }

    fn ljos(&self, args: &[&str], stdin: &str) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env("PATH", "/usr/bin:/bin")
            .env("POLICYD_BIN", self.path("bin/ljos-policyd"));
        self.env(&mut cmd);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// `ljos policy --fail-on-deny -- sudo true`: its exit code and stdout.
    fn policy(&self) -> (Option<i32>, String) {
        let out = self.ljos(&["policy", "--fail-on-deny", "--", "sudo", "true"], "");
        (out.status.code(), String::from_utf8(out.stdout).unwrap())
    }

    /// What `ljos hook` answers Claude Code's `PreToolUse` on `sudo true`.
    fn hook(&self) -> String {
        let work = self.path("work");
        let payload = serde_json::json!({
            "session_id": "s-1",
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "sudo true"},
            "cwd": work,
        });
        let out = self.ljos(&["hook"], &payload.to_string());
        assert!(
            out.status.success(),
            "ljos hook exited {:?}\nstderr:\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// `rm -r` on a fresh directory through the guard shim: the exit code,
    /// stderr, and whether the directory is still there.
    fn shim_rm(&self) -> (Option<i32>, String, bool) {
        let victim = self.path("victim");
        std::fs::create_dir_all(&victim).unwrap();
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(format!("rm -r '{}'", victim.display()))
            .env(
                "PATH",
                format!(
                    "{}:{}:/usr/bin:/bin",
                    self.path("guard").display(),
                    self.path("bin").display()
                ),
            );
        self.env(&mut cmd);
        let out = cmd.output().unwrap();
        (
            out.status.code(),
            String::from_utf8(out.stderr).unwrap(),
            victim.exists(),
        )
    }
}

fn deny_of(answer: &str) -> (String, String) {
    let v: serde_json::Value =
        serde_json::from_str(answer.trim()).unwrap_or_else(|e| panic!("{e}: {answer}"));
    let out = &v["hookSpecificOutput"];
    (
        out["permissionDecision"].as_str().unwrap_or("").to_string(),
        out["permissionDecisionReason"]
            .as_str()
            .unwrap_or("")
            .to_string(),
    )
}

fn assert_label(seat: &Seat, why: &str) {
    assert_eq!(
        seat.policy(),
        (
            Some(0),
            "sudo true\nallow\twarn\tsudo\ta privilege runner is on the command\n".to_string()
        ),
        "{why}"
    );
    assert_eq!(seat.hook(), "", "{why}: the hook allows");
    let (code, err, kept) = seat.shim_rm();
    assert_eq!((code, err.as_str(), kept), (Some(0), "", false), "{why}");
}

fn assert_denied(seat: &Seat, why: &str) {
    assert_eq!(
        seat.policy(),
        (
            Some(1),
            "sudo true\ndeny\tsudo\ta privilege runner is on the command\n".to_string()
        ),
        "{why}"
    );
    let (decision, reason) = deny_of(&seat.hook());
    assert_eq!(decision, "deny", "{why}: {reason}");
    assert!(
        reason.starts_with("sudo: ") && reason.contains("`ljos-policyd`"),
        "{why}: {reason}"
    );
    let (code, err, kept) = seat.shim_rm();
    assert_eq!(code, Some(126), "{why}: {err}");
    assert!(
        err.starts_with("ljos guard: denied (rm-rf-outside-tmp): rm -r "),
        "{why}: {err}"
    );
    assert!(kept, "{why}: the refused rm did not run");
}

/// A config that would block warnings changes nothing until the person's
/// own `ljos.toml` names it. A repository's `ljos.toml` does not name it.
#[test]
fn with_no_policyd_config_a_warning_stays_a_label() {
    let seat = Seat::new();
    assert_label(&seat, "nothing configured");
    seat.write(".config/ljos/blocks.conf", BLOCKS);
    seat.write(
        ".config/ljos/ljos.toml",
        "[identity]\nname = \"A Person\"\nemail = \"a@example.org\"\n",
    );
    assert_label(&seat, "a user ljos.toml that names no policyd config");
    let named = format!(
        "policyd_config = \"{}\"\n",
        seat.path(".config/ljos/blocks.conf").display()
    );
    seat.write("work/ljos.toml", &named);
    seat.write("work/.ljos/gates.toml", &named);
    assert_label(&seat, "only the repository names one");
}

/// The user's or the seat's `ljos.toml` names a config that holds
/// `block-on-warning true`: the warned command is policyd's deny in
/// `ljos policy`, the hook and the guard shim. The seat's file wins, and
/// `""` there names none.
#[test]
fn a_named_config_holding_block_on_warning_reports_policyd_deny() {
    let mut seat = Seat::new();
    seat.write(".config/ljos/blocks.conf", BLOCKS);
    seat.write(
        ".config/ljos/ljos.toml",
        "policyd_config = \"blocks.conf\"\n\n[identity]\nname = \"A Person\"\n",
    );
    assert_denied(
        &seat,
        "the user's ljos.toml names it, relative to that file",
    );
    seat.seat = Some("box");
    seat.write(
        ".config/ljos/seats/box/ljos.toml",
        "policyd_config = \"\"\n",
    );
    assert_label(&seat, "the seat's file names none");
    seat.write(".config/ljos/ljos.toml", "");
    seat.write(
        ".config/ljos/seats/box/ljos.toml",
        "[gates]\npolicyd_config = '~/.config/ljos/blocks.conf'\n",
    );
    assert_denied(&seat, "the seat's ljos.toml names it under [gates]");
}

/// A named config policyd cannot open is a check that fails open: the
/// command still runs.
#[test]
fn a_check_that_fails_open_still_allows() {
    let seat = Seat::new();
    seat.write(".config/ljos/ljos.toml", "policyd_config = \"gone.conf\"\n");
    assert_eq!(
        seat.policy(),
        (
            Some(0),
            "sudo true\nallow\tfail-open\tmissing config\n".to_string()
        )
    );
    assert_eq!(seat.hook(), "", "the hook allows");
    let (code, err, kept) = seat.shim_rm();
    assert_eq!((code, err.as_str(), kept), (Some(0), "", false));
}
