//! `policyd_config` in the person's `ljos.toml` names the file
//! `ljos-policyd check --config` reads. The policyd here stands in for one
//! whose config takes `block-on-warning true`: a warning is then a deny
//! with the same rule and warning, and `check` exits 2. Without that line
//! a warning is one warning line, and the command runs. A config it cannot
//! open fails open. The warnings are the lines ljos-policyd 0.7.0 prints.

ljos_test_boot::harness!();
use std::io::Write;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const POLICYD: &str = r#"#!/bin/sh
[ "$1" = check ] && shift
conf=
if [ "$1" = --config ]; then conf=$2; shift 2; fi
[ "$1" = -- ] && shift
case "$*" in
  *sudo*) rule=sudo; said='a privilege runner is on the command' ;;
  *"rm -r"*) rule=rm-rf-outside-tmp; said='a recursive delete reaches outside /tmp and /var/tmp' ;;
  *"git clean -f"*) rule=git-clean-force; said='git clean -f deletes untracked files' ;;
  *"find "*" -delete"*) rule=find-delete; said='find -delete reaches outside /tmp and /var/tmp' ;;
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

/// A command run through a guard shim: the exit code, stderr, and whether
/// its victim is still there.
type Shimmed = (Option<i32>, String, bool);

const RM_WARNING: &str = "a recursive delete reaches outside /tmp and /var/tmp";

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
        for shimmed in ljos_cli::GUARD_SHIMMED {
            seat.executable(&format!("guard/{shimmed}"), ljos_cli::GUARD_SHIM);
        }
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
        self.policy_on(&["sudo", "true"])
    }

    /// `ljos policy --fail-on-deny -- ARGV`: its exit code and stdout.
    fn policy_on(&self, argv: &[&str]) -> (Option<i32>, String) {
        let out = self.ljos(&[&["policy", "--fail-on-deny", "--"], argv].concat(), "");
        (out.status.code(), String::from_utf8(out.stdout).unwrap())
    }

    /// What `ljos hook` answers the terminal harness's `PreToolUse` on `sudo true`.
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

    /// `ljos doctor`: its exit code and its `policyd config` rows.
    fn doctor(&self) -> (Option<i32>, Vec<String>) {
        let out = self.ljos(&["doctor"], "");
        let rows = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .filter(|line| line.split('\t').nth(1) == Some("policyd config"))
            .map(str::to_string)
            .collect();
        (out.status.code(), rows)
    }

    /// `line` through the guard shims from `cwd`, with `victim` as what it
    /// would remove.
    fn shim(&self, line: &str, cwd: &Path, victim: &Path) -> Shimmed {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(line).env(
            "PATH",
            format!(
                "{}:{}:/usr/bin:/bin",
                self.path("guard").display(),
                self.path("bin").display()
            ),
        );
        self.env(&mut cmd);
        let out = cmd.current_dir(cwd).output().unwrap();
        (
            out.status.code(),
            String::from_utf8(out.stderr).unwrap(),
            victim.exists(),
        )
    }

    /// `rm -r`, `git clean -fdq` and `find -delete`, each on a fresh victim
    /// through its shim: the rule policyd warns of, how the shim names the
    /// command, and what [`Seat::shim`] says.
    fn shims(&self) -> [(&'static str, &'static str, Shimmed); 3] {
        let fresh = |rel: &str| {
            let dir = self.path(rel);
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        };
        let work = self.path("work");
        let victim = fresh("victim");
        let repo = fresh("repo");
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        std::fs::write(repo.join("untracked"), "").unwrap();
        let found = fresh("found");
        std::fs::write(found.join("file"), "").unwrap();
        [
            (
                "rm-rf-outside-tmp",
                "rm -r ",
                self.shim(&format!("rm -r '{}'", victim.display()), &work, &victim),
            ),
            (
                "git-clean-force",
                "git clean -fdq",
                self.shim("git clean -fdq", &repo, &repo.join("untracked")),
            ),
            (
                "find-delete",
                "find ",
                self.shim(
                    &format!("find '{}' -delete", found.display()),
                    &work,
                    &found,
                ),
            ),
        ]
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

/// `ljos policy`, the hook and the rm, git and find shims each show the
/// rule's one warning, and the command runs.
fn assert_label(seat: &Seat, why: &str) {
    assert_eq!(
        seat.policy(),
        (
            Some(0),
            "sudo true\nwarning: sudo: a privilege runner is on the command (ljos rule \
             `ljos-policyd`)\n"
                .to_string()
        ),
        "{why}"
    );
    let answer = seat.hook();
    assert!(
        !ljos_cli::answer_denies(&answer) && !answer.contains("\"ask\""),
        "{why}: {answer}"
    );
    assert!(
        answer.contains("warning: sudo: a privilege runner is on the command"),
        "{why}: {answer}"
    );
    assert_shims_allow(seat, why, shim_warning);
    assert_eq!(
        seat.policy_on(&["rm", "-r", "/srv/data"]),
        (
            Some(0),
            format!("rm -r /srv/data\n{}", shim_warning("rm-rf-outside-tmp"))
        ),
        "{why}"
    );
}

/// The line a guard shim prints on stderr for the warning policyd gives
/// `rule` here: policyd's own line, for every rule.
fn shim_warning(rule: &str) -> String {
    match rule {
        "rm-rf-outside-tmp" => format!("allow\twarn\t{rule}\t{RM_WARNING}\n"),
        "git-clean-force" => format!("allow\twarn\t{rule}\tgit clean -f deletes untracked files\n"),
        "find-delete" => {
            format!("allow\twarn\t{rule}\tfind -delete reaches outside /tmp and /var/tmp\n")
        }
        _ => String::new(),
    }
}

/// Each guard shim ran its command. It exited 0, the victim is gone, and
/// stderr shows what `said` gives for its rule.
fn assert_shims_allow(seat: &Seat, why: &str, said: fn(&str) -> String) {
    let ran = seat.shims().map(|(rule, _, shimmed)| (rule, shimmed));
    let allowed = ran
        .clone()
        .map(|(rule, _)| (rule, (Some(0), said(rule), false)));
    assert_eq!(ran, allowed, "{why}");
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
    for (rule, command, (code, err, kept)) in seat.shims() {
        assert_eq!(code, Some(126), "{why}: {err}");
        assert!(
            err.starts_with(&format!("ljos guard: denied ({rule}): {command}")),
            "{why}: {err}"
        );
        assert!(kept, "{why}: the refused {rule} command ran");
    }
    assert_eq!(
        seat.policy_on(&["rm", "-r", "/srv/data"]),
        (
            Some(1),
            format!("rm -r /srv/data\ndeny\trm-rf-outside-tmp\t{RM_WARNING}\n")
        ),
        "{why}"
    );
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

/// Deleting the root runs. It names `uproots-person-key` with the tree and
/// the person's key directory beside policyd's own warning, and keeps a
/// `warn` row of each. When the person's `ljos.toml` names a config with
/// `block-on-warning true`, policyd's warning on the same command becomes
/// its deny. ljos applies that deny and keeps no row for the label.
#[test]
fn a_named_config_holding_block_on_warning_denies_deleting_the_root() {
    let seat = Seat::new();
    let key = ljos_cli::person_key::key_dir().expect("a passwd home");
    let uproots = format!(
        "allow\twarn\tuproots-person-key\tthe command deletes /, a tree that holds the \
         person's key directory {}\n",
        key.display()
    );
    let labelled = || {
        std::fs::read_to_string(seat.path(".local/share/ljos/bypass.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|row| serde_json::from_str::<serde_json::Value>(row).ok())
            .filter(|row| row["rule"] == "uproots-person-key")
            .count()
    };
    assert_eq!(
        seat.policy_on(&["rm", "-rf", "/"]),
        (
            Some(0),
            format!("rm -rf /\n{uproots}allow\twarn\trm-rf-outside-tmp\t{RM_WARNING}\n")
        )
    );
    assert_eq!(labelled(), 1);
    seat.write(".config/ljos/blocks.conf", BLOCKS);
    seat.write(
        ".config/ljos/ljos.toml",
        "policyd_config = \"blocks.conf\"\n",
    );
    assert_eq!(
        seat.policy_on(&["rm", "-rf", "/"]),
        (
            Some(1),
            format!("rm -rf /\ndeny\trm-rf-outside-tmp\t{RM_WARNING}\n")
        )
    );
    assert_eq!(labelled(), 1, "the deny keeps no row for the label");
}

/// `block-on-warning true` reaches only what policyd warns on. Under the
/// config that denies deleting the root, moving the home that contains the
/// person's key directory and deleting a remote branch get no warning
/// from policyd, so each still runs with ljos's own label.
#[test]
fn block_on_warning_leaves_a_label_policyd_does_not_warn_on() {
    let seat = Seat::new();
    seat.write(".config/ljos/blocks.conf", BLOCKS);
    seat.write(
        ".config/ljos/ljos.toml",
        "policyd_config = \"blocks.conf\"\n",
    );
    let key = ljos_cli::person_key::key_dir().expect("a passwd home");
    let home = key.ancestors().nth(2).expect("the home above the config");
    let home = home.display().to_string();
    let moved = format!(
        "allow\twarn\tuproots-person-key\tthe command moves {home}, a tree that holds the \
         person's key directory {}",
        key.display()
    );
    for (argv, said) in [
        (vec!["mv", &home, "/tmp/x"], moved.as_str()),
        (
            vec!["git", "push", "origin", "--delete", "old"],
            "allow\twarn\tgit-push-delete\ta push deletes a remote branch",
        ),
    ] {
        let (code, text) = seat.policy_on(&argv);
        assert_eq!(code, Some(0), "{argv:?}\n{text}");
        assert!(text.lines().any(|line| line == said), "{argv:?}\n{text}");
        assert!(
            !text.lines().any(|line| line.starts_with("deny")),
            "{argv:?}\n{text}"
        );
    }
}

/// The same rules on the same commands, under one named config: with
/// `block-on-warning` off each is one warning and the command runs, and
/// with it on each is policyd's deny in `ljos policy`, the hook and the rm,
/// git and find shims.
#[test]
fn block_on_warning_turns_the_same_warning_into_a_deny() {
    let seat = Seat::new();
    seat.write(
        ".config/ljos/ljos.toml",
        "policyd_config = \"policyd.conf\"\n",
    );
    seat.write(".config/ljos/policyd.conf", "block-on-warning false\n");
    assert_label(&seat, "block-on-warning is off");
    seat.write(".config/ljos/policyd.conf", BLOCKS);
    assert_denied(&seat, "block-on-warning is on");
    seat.write(".config/ljos/policyd.conf", "block-on-warning false\n");
    assert_label(&seat, "block-on-warning is off again");
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
    assert_shims_allow(&seat, "a named config that is not there", |_| String::new());
}

/// `policyd_config` names a config only at the top of `ljos.toml` or under
/// `[gates]`. Under `[identity]` it names none: `ljos policy`, the hook
/// and the rm, git and find shims all leave the warning a label. Under
/// `[identity]` after `[gates]`, it does not replace the one `[gates]`
/// names.
#[test]
fn a_policyd_config_under_another_table_names_none() {
    let seat = Seat::new();
    seat.write(".config/ljos/blocks.conf", BLOCKS);
    seat.write(
        ".config/ljos/ljos.toml",
        "[identity]\nname = \"A Person\"\npolicyd_config = \"blocks.conf\"\n",
    );
    assert_label(&seat, "the user's ljos.toml names it under [identity]");
    seat.write(
        ".config/ljos/ljos.toml",
        "[gates]\npolicyd_config = \"blocks.conf\"\n\n[identity]\npolicyd_config = \"gone.conf\"\n",
    );
    assert_denied(&seat, "[gates] names it and [identity] names another");
}

/// `ljos doctor` names a `policyd_config` that is not there on a `warn`
/// row, and exits as it does with nothing named.
#[test]
fn the_doctor_warns_of_a_named_policyd_config_that_is_not_there() {
    let seat = Seat::new();
    let (code, rows) = seat.doctor();
    assert!(rows.is_empty(), "nothing named: {rows:?}");
    seat.write(".config/ljos/ljos.toml", "policyd_config = \"gone.conf\"\n");
    let gone = seat.path(".config/ljos/gone.conf");
    assert_eq!(
        seat.doctor(),
        (
            code,
            vec![format!(
                "warn\tpolicyd config\tpolicyd_config names {}, which is not a file: every `ljos-policyd check` fails open, with no rule or warning",
                gone.display()
            )]
        )
    );
    seat.write(".config/ljos/gone.conf", BLOCKS);
    assert_eq!(seat.doctor(), (code, Vec::new()), "the file is there");
}
