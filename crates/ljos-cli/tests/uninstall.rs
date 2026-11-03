//! `ljos uninstall` takes a seat off a home: runner files, git hooks,
//! programs, and the stub that answers a cached hook.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn executable(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    write(path, text);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A home with a copy of this ljos in `bin`, a fake `the terminal harness` that answers
/// onboard, and another seat program that must stay.
fn seat_home() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().to_path_buf();
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_ljos"), bin.join("ljos")).unwrap();
    executable(&bin.join("ljos-mcp"), "#!/bin/sh\nexit 0\n");
    executable(&bin.join("packset"), "#!/bin/sh\nexit 0\n");
    executable(
        &bin.join("claude"),
        r#"#!/bin/sh
case "$*" in
  "mcp get ljos") exit 1 ;;
  "mcp add"*) mkdir -p "$HOME"; printf '%s\n' '{"mcpServers":{"ljos":{"command":"ljos-mcp"}}}' > "$HOME/.claude.json" ;;
esac
"#,
    );
    (temp, bin)
}

fn run(home: &Path, bin: &Path, args: &[&str]) -> Output {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(bin.to_path_buf()).chain(std::env::split_paths(&inherited)),
    )
    .unwrap();
    Command::new(bin.join("ljos"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("PATH", path)
        .env("PACKSET_URL", "off")
        .env("DEEDAR_URL", "file:///nonexistent/ljos-uninstall-test")
        .env("DEEDAR_HOST_SIGNING_KEY", "off")
        .env("VISSUE_ROOT", "/nonexistent/ljos-uninstall-test")
        .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LJOS_HOOKD", "off")
        .output()
        .unwrap()
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn uninstall_takes_the_seat_off_and_leaves_the_stub() {
    let (temp, bin) = seat_home();
    let home = temp.path();
    std::fs::create_dir_all(home.join("run")).unwrap();
    let onboard = run(home, &bin, &["onboard", "--harness", "claude"]);
    assert!(onboard.status.success(), "{}", said(&onboard));
    let settings = home.join(".claude/settings.json");
    assert!(read(&settings).contains("ljos hook"), "{}", read(&settings));
    assert!(home.join(".claude/skills/ljos/SKILL.md").is_file());
    let manifest = home.join(".local/state/ljos/onboard/manifest.json");
    assert!(manifest.is_file());
    write(&home.join(".local/state/ljos/keep.txt"), "state\n");

    let dry = run(home, &bin, &["uninstall", "--dry-run"]);
    let text = said(&dry);
    assert!(dry.status.success(), "{text}");
    assert!(text.contains("would replace"), "{text}");
    assert!(
        read(&settings).contains("ljos hook"),
        "a dry run writes nothing"
    );
    assert!(manifest.is_file());
    let ljos_len = std::fs::metadata(bin.join("ljos")).unwrap().len();

    let live = run(home, &bin, &["uninstall"]);
    let text = said(&live);
    assert!(live.status.success(), "{text}");
    assert!(
        !read(&settings).contains("ljos hook"),
        "{}",
        read(&settings)
    );
    assert!(!home.join(".claude/skills/ljos").exists());
    assert!(!manifest.exists(), "the onboard manifest is spent");
    assert!(!bin.join("ljos-mcp").exists());
    assert!(bin.join("packset").is_file(), "other seat programs stay");
    assert!(
        home.join(".local/state/ljos/keep.txt").is_file(),
        "state stays without --purge"
    );
    let stub = read(&bin.join("ljos"));
    assert!(stub.starts_with("#!/bin/sh"), "{stub}");
    assert!(stub.contains("ljos was uninstalled"), "{stub}");
    assert!(std::fs::metadata(bin.join("ljos")).unwrap().len() < ljos_len);
    let answer = Command::new("sh")
        .arg(bin.join("ljos"))
        .arg("hook")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(answer.status.code(), Some(0));
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&answer.stdout).trim()).unwrap();
    assert_eq!(v["permission"], "allow");
}

#[test]
fn purge_removes_the_state_and_no_stub_leaves_nothing() {
    let (temp, bin) = seat_home();
    let home = temp.path();
    write(&home.join(".local/state/ljos/keep.txt"), "state\n");
    write(&home.join(".config/ljos/harnesses.toml"), "");
    write(&home.join(".local/share/ljos/guard.jsonl"), "");
    let out = run(home, &bin, &["uninstall", "--purge", "--no-stub"]);
    assert!(out.status.success(), "{}", said(&out));
    assert!(!bin.join("ljos").exists(), "no stub asked for");
    assert!(!home.join(".local/state/ljos").exists());
    assert!(!home.join(".config/ljos").exists());
    assert!(!home.join(".local/share/ljos").exists());
    assert!(bin.join("packset").is_file());
}
