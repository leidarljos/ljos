//! The plugin launcher on a cloud VM with nothing installed.
//!
//! The MCP client starts `bin/ljos-plugin ljos-mcp` at boot with a bare
//! environment. The launcher takes `ljos` and `ljos-mcp` from this
//! version's release, checks the published SHA-256, and runs the server.
//! A tarball that does not match installs nothing.

ljos_test_boot::harness!();
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn launcher() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bin/ljos-plugin")
}

fn target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        other => panic!("no release target for {other:?}"),
    }
}

fn asset() -> String {
    format!("ljos-v{}-{}.tar.gz", env!("CARGO_PKG_VERSION"), target())
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn sha256(path: &Path) -> String {
    let out = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// A release directory as the workflow publishes it: the tarball with
/// one top directory, and its `.sha256` beside it.
fn release(root: &Path, sum: Option<&str>) -> PathBuf {
    let rel = root.join("release");
    let top = format!("ljos-v{}-{}", env!("CARGO_PKG_VERSION"), target());
    let stage = root.join("stage").join(&top);
    fs::create_dir_all(&stage).unwrap();
    fs::create_dir_all(&rel).unwrap();
    executable(
        &stage.join("ljos-mcp"),
        "#!/bin/sh\necho \"release ljos-mcp $*\"\n",
    );
    executable(&stage.join("ljos"), "#!/bin/sh\necho \"release ljos $*\"\n");
    let tarball = rel.join(asset());
    let made = Command::new("tar")
        .arg("-czf")
        .arg(&tarball)
        .arg("-C")
        .arg(root.join("stage"))
        .arg(&top)
        .output()
        .unwrap();
    assert!(made.status.success());
    let sum = sum.map_or_else(|| sha256(&tarball), str::to_string);
    fs::write(
        rel.join(format!("{}.sha256", asset())),
        format!("{sum}  {}\n", asset()),
    )
    .unwrap();
    rel
}

fn start(home: &Path, rel: &Path, fetch: &str) -> Output {
    Command::new("sh")
        .arg(launcher())
        .args(["ljos-mcp", "--probe"])
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("LJOS_RELEASE_DIR", rel)
        .env("LJOS_PLUGIN_FETCH", fetch)
        .output()
        .unwrap()
}

#[test]
fn a_cloud_vm_runs_the_release_server_when_nothing_is_installed() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let rel = release(root.path(), None);
    let out = start(&home, &rel, "1");
    let said = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}{err}");
    assert_eq!(said.trim(), "release ljos-mcp --probe", "{err}");
    assert!(err.contains("SHA-256 checked"), "{err}");
    let bin = home.join(".local/bin");
    assert!(bin.join("ljos-mcp").is_file());
    assert!(bin.join("ljos").is_file());
    assert!(!bin.join(".ljos-plugin-fetch").exists());

    // The next start finds the install and fetches nothing.
    fs::remove_dir_all(&rel).unwrap();
    let again = start(&home, &rel, "1");
    assert!(again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).is_empty());
}

#[test]
fn a_tarball_that_does_not_match_its_sum_installs_nothing() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let rel = release(root.path(), Some(&"0".repeat(64)));
    let out = start(&home, &rel, "1");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        err.lines()
            .next()
            .unwrap()
            .contains("ljos-mcp: not installed, so the MCP server has no tools"),
        "{err}"
    );
    assert!(
        err.contains("does not match its published SHA-256"),
        "{err}"
    );
    let bin = home.join(".local/bin");
    assert!(!bin.join("ljos-mcp").exists());
    assert!(!bin.join("ljos").exists());
    assert!(!bin.join(".ljos-plugin-fetch").exists());
}

#[test]
fn a_launcher_told_not_to_fetch_leaves_the_machine_alone() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let rel = release(root.path(), None);
    let out = start(&home, &rel, "0");
    assert!(!out.status.success());
    assert!(!home.join(".local/bin/ljos-mcp").exists());
}

#[test]
fn a_stale_fetch_lock_is_taken_over() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let lock = home.join(".local/bin/.ljos-plugin-fetch");
    fs::create_dir_all(&lock).unwrap();
    fs::write(lock.join("pid"), "4194303\n").unwrap();
    let rel = release(root.path(), None);
    let out = start(&home, &rel, "1");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!lock.exists());
}

#[test]
fn the_plugin_manifests_name_this_version() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for manifest in [".cursor-plugin/plugin.json", ".claude-plugin/plugin.json"] {
        let text = fs::read_to_string(root.join(manifest)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            v["version"].as_str(),
            Some(env!("CARGO_PKG_VERSION")),
            "{manifest}"
        );
    }
}
