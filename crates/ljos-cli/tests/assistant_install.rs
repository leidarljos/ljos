//! `ljos onboard` finds Claude Code, Cursor and Codex and registers each one.
//!
//! Detection is a command on `PATH` or the runner's config directory under
//! `HOME`. The writes are the MCP entry and the skill. A second run leaves
//! the bytes in place. `--dry-run` writes nothing. `ljos offboard` removes
//! what onboard added.
//!
//! A remembered lesson is handed back on the next prompt with no seat, no
//! sitting and no ticket.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

struct Home {
    root: tempfile::TempDir,
}

impl Home {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for sub in [
            ".config",
            ".local/share",
            ".local/state",
            ".cache",
            "run",
            "tmp",
            "bin",
        ] {
            std::fs::create_dir_all(root.path().join(sub)).unwrap();
        }
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn ljos(&self, args: &[&str]) -> Output {
        let home = self.path();
        let bins = Path::new(env!("CARGO_BIN_EXE_ljos")).parent().unwrap();
        let path = std::env::join_paths([
            home.join("bin"),
            bins.to_path_buf(),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ])
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("HOME", home)
            .env("PATH", path)
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", home.join("tmp"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("LJOS_TRACKER_GIT", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }
}

fn stubs(home: &Home) {
    let bin = home.path().join("bin");
    script(
        &bin,
        "claude",
        r#"#!/bin/sh
mark="$HOME/.claude/.ljos-mcp-added"
case "$1 $2" in
  "mcp get")
    [ -f "$mark" ] && exit 0
    exit 1
    ;;
  "mcp add")
    mkdir -p "$(dirname "$mark")"
    : > "$mark"
    exit 0
    ;;
esac
echo "claude stub: $*" >&2
exit 1
"#,
    );
    script(&bin, "codex", "#!/bin/sh\nexit 0\n");
    script(&bin, "cursor", "#!/bin/sh\nexit 0\n");
    script(
        &bin,
        "packset",
        "#!/bin/sh\n[ \"$1\" = ensure ] && echo writer up && exit 0\necho \"packset stub: $*\" >&2\nexit 1\n",
    );
    script(
        &bin,
        "deedar",
        "#!/bin/sh\n[ \"$1 $2\" = \"host accept\" ] && echo 'host key listed' && exit 0\necho \"deedar stub: $*\" >&2\nexit 1\n",
    );
}

fn registration(home: &Path) -> Vec<PathBuf> {
    [
        ".claude/skills/ljos/SKILL.md",
        ".claude/settings.json",
        ".cursor/mcp.json",
        ".cursor/skills/ljos/SKILL.md",
        ".cursor/hooks.json",
        ".codex/config.toml",
        ".codex/skills/ljos/SKILL.md",
        ".codex/hooks.json",
    ]
    .into_iter()
    .map(|rel| home.join(rel))
    .collect()
}

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn text_at(path: &Path) -> String {
    String::from_utf8_lossy(&bytes(path)).into_owned()
}

/// No coding tool and no runner directory. The line names what to run next.
#[test]
fn onboard_with_no_assistant_names_the_next_command() {
    let home = Home::new();
    let bin = home.path().join("bin");
    script(
        &bin,
        "packset",
        "#!/bin/sh\n[ \"$1\" = ensure ] && echo writer up && exit 0\necho \"packset stub: $*\" >&2\nexit 1\n",
    );
    script(
        &bin,
        "deedar",
        "#!/bin/sh\n[ \"$1 $2\" = \"host accept\" ] && echo 'host key listed' && exit 0\necho \"deedar stub: $*\" >&2\nexit 1\n",
    );
    let out = home.ljos(&["onboard", "--harness", "all", "--dry-run"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("found no coding tool"), "{text}");
    assert!(text.contains("`ljos onboard`"), "{text}");
    assert!(text.contains("`ljos onboard --example`"), "{text}");
    assert!(text.contains("`ljos onboard --harness NAME`"), "{text}");
}

/// Commands on `PATH`, and no config directory yet, are enough to find the three.
#[test]
fn detects_claude_cursor_and_codex_from_commands() {
    let home = Home::new();
    stubs(&home);
    let out = home.ljos(&["onboard", "--harness", "all", "--dry-run"]);
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("found claude, codex, cursor"), "{text}");
    assert!(text.contains("would "), "{text}");
    for path in registration(home.path()) {
        assert!(!path.exists(), "dry-run wrote {}", path.display());
    }
}

/// Config directories under `HOME` find the same three when the commands are absent.
#[test]
fn detects_claude_cursor_and_codex_from_directories() {
    let home = Home::new();
    stubs(&home);
    for dir in [".claude", ".cursor", ".codex"] {
        std::fs::create_dir_all(home.path().join(dir)).unwrap();
    }
    // The directory is the signal. Take the commands off PATH.
    for name in ["claude", "codex", "cursor"] {
        std::fs::remove_file(home.path().join("bin").join(name)).unwrap();
    }
    // Claude's MCP registration runs `claude`. Without it the step fails,
    // and the directories are still what detection named.
    let out = home.ljos(&["onboard", "--harness", "all", "--dry-run"]);
    let text = said(&out);
    assert!(text.contains("found claude, codex, cursor"), "{text}");
    assert!(text.contains("claude not on PATH"), "{text}");
    for path in registration(home.path()) {
        assert!(!path.exists(), "dry-run wrote {}", path.display());
    }
}

#[test]
fn onboard_writes_once_and_offboard_removes() {
    let home = Home::new();
    stubs(&home);
    for dir in [".claude", ".cursor", ".codex"] {
        std::fs::create_dir_all(home.path().join(dir)).unwrap();
    }

    let dry = home.ljos(&["onboard", "--harness", "all", "--dry-run"]);
    let dry_text = said(&dry);
    assert!(dry.status.success(), "{dry_text}");
    assert!(
        dry_text.contains("found claude, codex, cursor"),
        "{dry_text}"
    );
    for path in registration(home.path()) {
        assert!(!path.exists(), "dry-run wrote {}", path.display());
    }

    let first = home.ljos(&["onboard", "--harness", "all"]);
    let first_text = said(&first);
    assert!(first.status.success(), "{first_text}");
    assert!(
        first_text.contains("found claude, codex, cursor"),
        "{first_text}"
    );

    let skill = text_at(&home.path().join(".claude/skills/ljos/SKILL.md"));
    assert!(skill.starts_with("---\nname: ljos\n"), "{skill}");
    assert_eq!(
        text_at(&home.path().join(".cursor/skills/ljos/SKILL.md")),
        skill
    );
    assert_eq!(
        text_at(&home.path().join(".codex/skills/ljos/SKILL.md")),
        skill
    );

    let cursor = text_at(&home.path().join(".cursor/mcp.json"));
    assert!(cursor.contains("\"ljos\""), "{cursor}");
    assert!(cursor.contains("ljos-mcp"), "{cursor}");
    let codex = text_at(&home.path().join(".codex/config.toml"));
    assert!(codex.contains("[mcp_servers.ljos]"), "{codex}");
    let claude = text_at(&home.path().join(".claude/settings.json"));
    assert!(claude.contains("UserPromptSubmit"), "{claude}");
    assert!(claude.contains("ljos"), "{claude}");
    let cursor_hooks = text_at(&home.path().join(".cursor/hooks.json"));
    assert!(cursor_hooks.contains("ljos"), "{cursor_hooks}");
    let codex_hooks = text_at(&home.path().join(".codex/hooks.json"));
    assert!(codex_hooks.contains("ljos"), "{codex_hooks}");
    assert!(
        home.path().join(".claude/.ljos-mcp-added").is_file(),
        "claude mcp add did not run"
    );

    let snap: Vec<Vec<u8>> = registration(home.path()).iter().map(|p| bytes(p)).collect();

    let second = home.ljos(&["onboard", "--harness", "all"]);
    let second_text = said(&second);
    assert!(second.status.success(), "{second_text}");
    assert!(second_text.contains("is current"), "{second_text}");
    assert!(second_text.contains("ljos registered"), "{second_text}");
    for (path, before) in registration(home.path()).iter().zip(&snap) {
        assert_eq!(
            &bytes(path),
            before,
            "second onboard rewrote {}",
            path.display()
        );
    }

    let again = home.ljos(&["onboard", "--harness", "all", "--dry-run"]);
    let again_text = said(&again);
    assert!(again.status.success(), "{again_text}");
    for (path, before) in registration(home.path()).iter().zip(&snap) {
        assert_eq!(&bytes(path), before, "dry-run rewrote {}", path.display());
    }

    let off = home.ljos(&["offboard"]);
    let off_text = said(&off);
    assert!(off.status.success(), "{off_text}");
    for path in registration(home.path()) {
        if path.exists() {
            let body = text_at(&path);
            assert!(
                !body.contains("name: ljos")
                    && !body.contains("mcp_servers.ljos")
                    && !body.contains("\"ljos\""),
                "offboard left ljos in {}: {body}",
                path.display()
            );
        }
    }

    let off_again = home.ljos(&["offboard"]);
    let off_again_text = said(&off_again);
    assert!(off_again.status.success(), "{off_again_text}");
    assert!(
        off_again_text.contains("nothing installed by ljos onboard"),
        "{off_again_text}"
    );
}

const LESSON: &str =
    "Seat the kiln door only when KILN_DOOR=cold. A missing setting is a config gap.";

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Remember, then the prompt hook, with no seat, sitting or ticket.
#[test]
fn a_lesson_comes_back_without_a_seat_sitting_or_ticket() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let packsetd = on_path("packsetd").expect("packsetd");
    let dir = tempfile::tempdir().unwrap();
    for sub in [
        "tmp",
        "run",
        ".config",
        ".local/share",
        ".local/state",
        ".cache",
    ] {
        std::fs::create_dir_all(dir.path().join(sub)).unwrap();
    }
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_ljos")).parent().unwrap();
    let path = std::env::join_paths([
        bin_dir,
        packsetd.parent().unwrap(),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .unwrap();
    let pack_home = dir.path().join(".local/share/packset");
    std::fs::create_dir_all(&pack_home).unwrap();
    let mut started = ljos_test_boot::Packsetd::build();
    started.bin(&packsetd);
    started.pack_at(&pack_home);
    {
        let cmd = started.command();
        cmd.env_clear()
            .env("HOME", dir.path())
            .env("PATH", &path)
            .env("TMPDIR", dir.path().join("tmp"))
            .env("XDG_CONFIG_HOME", dir.path().join(".config"))
            .env("XDG_DATA_HOME", dir.path().join(".local/share"))
            .env("XDG_STATE_HOME", dir.path().join(".local/state"))
            .env("XDG_CACHE_HOME", dir.path().join(".cache"))
            .env("XDG_RUNTIME_DIR", dir.path().join("run"))
            .env("LJOS_TRACKER_GIT", "off");
    }
    let daemon = started.spawn();

    let run = |args: &[&str], stdin: Option<&str>| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(dir.path())
            .env_clear()
            .env("HOME", dir.path())
            .env("PATH", &path)
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", dir.path().join("tmp"))
            .env("XDG_CONFIG_HOME", dir.path().join(".config"))
            .env("XDG_DATA_HOME", dir.path().join(".local/share"))
            .env("XDG_STATE_HOME", dir.path().join(".local/state"))
            .env("XDG_CACHE_HOME", dir.path().join(".cache"))
            .env("XDG_RUNTIME_DIR", dir.path().join("run"))
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_PORT", daemon.port().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        if let Some(text) = stdin {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
        }
        child.wait_with_output().unwrap()
    };

    let remembered = run(&["remember", LESSON], None);
    let remembered_text = said(&remembered);
    assert!(remembered.status.success(), "{remembered_text}");
    assert!(
        !remembered_text.contains("no seat named")
            && !remembered_text.contains("Nothing was written")
            && !remembered_text.contains("holds no sitting"),
        "{remembered_text}"
    );
    assert!(
        remembered_text.contains("standing:") || remembered_text.contains("episode:"),
        "{remembered_text}"
    );

    let found = run(&["search", "kiln door"], None);
    let found_text = said(&found);
    assert!(found.status.success(), "{found_text}");
    assert!(found_text.contains("KILN_DOOR=cold"), "{found_text}");

    let hook = run(&["hook", "--prompt", "--text"], Some("seat the kiln door"));
    let hook_text = said(&hook);
    assert!(hook.status.success(), "{hook_text}");
    assert!(
        hook_text.contains("KILN_DOOR=cold"),
        "the next prompt did not recall the lesson: {hook_text}"
    );
    assert!(
        !hook_text.contains("Nothing remembered bears on this."),
        "{hook_text}"
    );

    let tracker = dir.path().join(".local/share/vissue/tracker");
    assert!(
        !tracker.join("issues.org").exists()
            && !tracker.join("Software").join("issues.org").exists(),
        "a ticket was filed for a recall"
    );
}
