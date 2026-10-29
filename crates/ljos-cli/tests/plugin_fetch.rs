//! The plugin launcher on a cloud VM with nothing installed.
//!
//! The MCP client starts `bin/ljos-plugin ljos-mcp` at boot with a bare
//! environment. The launcher takes `ljos` and `ljos-mcp` from this
//! version's release, checks the published SHA-256, and runs the server.
//! A tarball that does not match installs nothing.

ljos_test_boot::harness!();
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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

fn launch(home: &Path, rel: &Path, fetch: &str, shims: Option<&Path>) -> Command {
    let path = shims.map_or_else(
        || "/usr/bin:/bin".to_string(),
        |dir| format!("{}:/usr/bin:/bin", dir.display()),
    );
    let mut cmd = Command::new("sh");
    cmd.arg(launcher())
        .args(["ljos-mcp", "--probe"])
        .env_clear()
        .env("HOME", home)
        .env("PATH", path)
        .env("LJOS_RELEASE_DIR", rel)
        .env("LJOS_PLUGIN_FETCH", fetch);
    cmd
}

fn start(home: &Path, rel: &Path, fetch: &str) -> Output {
    launch(home, rel, fetch, None).output().unwrap()
}

/// Everything the launcher leaves in the bin directory besides binaries.
fn leftovers(bin: &Path) -> Vec<String> {
    let mut left: Vec<String> = fs::read_dir(bin)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.') || n.contains(".new."))
        .collect();
    left.sort();
    left
}

/// The real tool, run at once, with its answer held back for `delay`.
/// A tool this machine lacks gets no shim.
fn slow_shim(dir: &Path, tool: &str, delay: &str) {
    let Some(real) = ["/usr/bin", "/bin"]
        .iter()
        .map(|d| Path::new(d).join(tool))
        .find(|p| p.is_file())
    else {
        return;
    };
    executable(
        &dir.join(tool),
        &format!(
            "#!/bin/sh\nout=$({} \"$@\")\ns=$?\nsleep {delay}\n[ -z \"$out\" ] || printf '%s\\n' \"$out\"\nexit $s\n",
            real.display()
        ),
    );
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
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    let lock = bin.join(".ljos-plugin-fetch");
    fs::write(&lock, "4194303\n").unwrap();
    fs::create_dir_all(bin.join(".ljos-plugin-stage.4194302/unpacked")).unwrap();
    let rel = release(root.path(), None);
    let out = start(&home, &rel, "1");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!lock.exists());
    assert!(leftovers(&bin).is_empty(), "{:?}", leftovers(&bin));
}

/// A launcher killed between making the lock and writing its pid left a
/// lock with no pid. The next one waited out its 90 seconds and gave up.
#[test]
fn a_lock_with_no_pid_is_taken_over_at_once() {
    for planted in ["empty file", "empty directory"] {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let bin = home.join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        let lock = bin.join(".ljos-plugin-fetch");
        if planted == "empty file" {
            fs::write(&lock, "").unwrap();
        } else {
            fs::create_dir(&lock).unwrap();
        }
        let rel = release(root.path(), None);
        let began = std::time::Instant::now();
        let out = start(&home, &rel, "1");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{planted}: {err}");
        assert!(
            began.elapsed() < std::time::Duration::from_secs(20),
            "{planted}: waited {:?}",
            began.elapsed()
        );
        assert!(bin.join("ljos-mcp").is_file(), "{planted}");
        assert!(
            leftovers(&bin).is_empty(),
            "{planted}: {:?}",
            leftovers(&bin)
        );
    }
}

/// The install loop ignored a failed `mv`, dropped the lock, and said both
/// binaries were installed.
#[test]
fn a_move_that_fails_is_reported_and_claims_nothing() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let shims = root.path().join("shims");
    fs::create_dir_all(&shims).unwrap();
    executable(
        &shims.join("mv"),
        "#!/bin/sh\necho 'mv: cannot move: no space left on device' >&2\nexit 1\n",
    );
    let rel = release(root.path(), None);
    let out = launch(&home, &rel, "1", Some(&shims)).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(!err.contains("SHA-256 checked"), "{err}");
    assert!(
        err.contains("ljos could not be moved into") && err.contains("nothing was installed"),
        "{err}"
    );
    let bin = home.join(".local/bin");
    assert!(!bin.join("ljos").exists());
    assert!(!bin.join("ljos-mcp").exists());
    assert!(leftovers(&bin).is_empty(), "{:?}", leftovers(&bin));
}

/// Two launchers both read the same dead pid. The later one used to
/// remove the lock the earlier one had just taken, stage and all.
#[test]
fn two_launchers_over_one_dead_lock_both_start_the_server() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    let lock = bin.join(".ljos-plugin-fetch");
    fs::write(&lock, "4194303\n").unwrap();
    let shims = root.path().join("shims");
    fs::create_dir_all(&shims).unwrap();
    slow_shim(&shims, "cat", "0.3");
    slow_shim(&shims, "sha256sum", "0.6");
    let rel = release(root.path(), None);
    let spawn = || {
        launch(&home, &rel, "1", Some(&shims))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let first = spawn();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let second = spawn();
    for (i, child) in [first, second].into_iter().enumerate() {
        let out = child.wait_with_output().unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "launcher {i}: {said}{err}");
        assert_eq!(
            said.trim(),
            "release ljos-mcp --probe",
            "launcher {i}: {err}"
        );
    }
    assert!(bin.join("ljos-mcp").is_file());
    assert!(bin.join("ljos").is_file());
    assert!(!lock.exists());
    assert!(leftovers(&bin).is_empty(), "{:?}", leftovers(&bin));
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
