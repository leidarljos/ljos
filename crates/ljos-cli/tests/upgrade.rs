//! `ljos upgrade` against a release directory on disk. No network.

ljos_test_boot::harness!();
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn target() -> &'static str {
    "x86_64-unknown-linux-gnu"
}

fn asset() -> String {
    format!("ljos-v{}-{}.tar.gz", version(), target())
}

fn executable(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn says(name: &str, version: &str) -> String {
    format!("#!/bin/sh\necho \"{name} {version}\"\n")
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

/// The archive the release workflow would publish, built from this
/// crate's own binaries so the version line is real.
fn fixture_release(root: &Path) -> PathBuf {
    let top = format!("ljos-v{}-{}", version(), target());
    let stage = root.join("stage").join(&top);
    fs::create_dir_all(&stage).unwrap();
    for (name, src) in [
        ("ljos", env!("CARGO_BIN_EXE_ljos")),
        ("ljos-mcp", env!("CARGO_BIN_EXE_ljos-mcp")),
    ] {
        let dest = stage.join(name);
        fs::copy(src, &dest).unwrap();
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let rel = root.join("release");
    fs::create_dir_all(&rel).unwrap();
    let tarball = rel.join(asset());
    let made = Command::new("tar")
        .arg("-czf")
        .arg(&tarball)
        .arg("-C")
        .arg(root.join("stage"))
        .arg(&top)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    fs::write(
        rel.join(format!("{}.sha256", asset())),
        format!("{}  {}\n", sha256(&tarball), asset()),
    )
    .unwrap();
    rel
}

fn run(home: &Path, dir: &Path, rel: Option<&Path>, version: &str) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(["upgrade", version, "--dir"])
        .arg(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .stdin(std::process::Stdio::null());
    if let Some(rel) = rel {
        cmd.env("LJOS_RELEASE_DIR", rel);
    }
    cmd.output().unwrap()
}

fn text_of(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn version_line(path: &Path) -> String {
    let out = Command::new(path).arg("--version").output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A directory that already runs this release is left byte for byte.
#[test]
fn an_install_already_at_the_release_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("bin");
    fs::create_dir_all(&dir).unwrap();
    for name in ["ljos", "ljos-mcp"] {
        executable(&dir.join(name), &says(name, version()));
    }
    let before: Vec<Vec<u8>> = ["ljos", "ljos-mcp"]
        .iter()
        .map(|name| fs::read(dir.join(name)).unwrap())
        .collect();

    let out = run(root.path(), &dir, None, version());
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains(&format!(
            "ljos {} is already installed at {}",
            version(),
            dir.display()
        )),
        "{said}"
    );
    assert!(!said.contains("installed ljos "), "{said}");
    let after: Vec<Vec<u8>> = ["ljos", "ljos-mcp"]
        .iter()
        .map(|name| fs::read(dir.join(name)).unwrap())
        .collect();
    assert_eq!(
        before, after,
        "upgrade wrote into an install that was current"
    );
    assert!(
        !dir.join(format!("ljos.bak-pre-{}", version())).exists(),
        "a current install wrote a rollback"
    );
}

/// The rollback copy is the previous release. Installing again must not
/// replace it with the release just installed.
#[test]
fn a_second_upgrade_keeps_the_older_rollback() {
    let root = tempfile::tempdir().unwrap();
    let rel = fixture_release(root.path());
    let dir = root.path().join("bin");
    fs::create_dir_all(&dir).unwrap();
    for name in ["ljos", "ljos-mcp"] {
        executable(&dir.join(name), &says(name, "0.36.3"));
        executable(
            &dir.join(format!("{name}.bak-pre-{}", version())),
            &says(name, "0.36.2"),
        );
    }
    let kept: Vec<Vec<u8>> = ["ljos", "ljos-mcp"]
        .iter()
        .map(|name| fs::read(dir.join(format!("{name}.bak-pre-{}", version()))).unwrap())
        .collect();

    let out = run(root.path(), &dir, Some(&rel), version());
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains(&format!("installed ljos {} at", version())),
        "{said}"
    );
    for (name, before) in ["ljos", "ljos-mcp"].iter().zip(&kept) {
        let bak = dir.join(format!("{name}.bak-pre-{}", version()));
        assert_eq!(
            fs::read(&bak).unwrap(),
            *before,
            "{name} rollback was overwritten"
        );
        assert_eq!(
            version_line(&dir.join(name)),
            format!("{name} {}", version())
        );
    }
}
