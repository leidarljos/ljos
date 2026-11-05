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
    invoke(home, dir, rel, &["upgrade", version])
}

fn invoke(home: &Path, dir: &Path, rel: Option<&Path>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .arg("--dir")
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

/// A small executable whose `--version` line is `{name} {version}`.
fn built(dir: &Path, name: &str, version: &str) -> PathBuf {
    let src = dir.join(format!("{name}-{version}.c"));
    fs::write(
        &src,
        format!(
            "#include <stdio.h>\nint main(void) {{ printf(\"{name} {version}\\n\"); return 0; }}\n"
        ),
    )
    .unwrap();
    let bin = dir.join(format!("{name}-{version}"));
    let out = Command::new("cc")
        .args(["-O0", "-o"])
        .arg(&bin)
        .arg(&src)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    bin
}

fn stem_of(asset: &str) -> &str {
    asset
        .trim_end_matches(".tar.xz")
        .trim_end_matches(".tar.gz")
}

/// The archive, its sha256 and `{repo}.version`, as a release would publish them.
fn publish(rel: &Path, repo: &str, version: &str, asset: &str, bin_name: &str, bin: &Path) {
    fs::create_dir_all(rel).unwrap();
    fs::write(rel.join(format!("{repo}.version")), format!("{version}\n")).unwrap();
    let stage_root = rel.join(format!("stage-{repo}-{bin_name}"));
    let stage = stage_root.join(stem_of(asset));
    fs::create_dir_all(&stage).unwrap();
    let dest = stage.join(bin_name);
    fs::copy(bin, &dest).unwrap();
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).unwrap();
    let tarball = rel.join(asset);
    let made = Command::new("tar")
        .arg(if asset.ends_with(".tar.xz") {
            "-cJf"
        } else {
            "-czf"
        })
        .arg(&tarball)
        .arg("-C")
        .arg(&stage_root)
        .arg(stem_of(asset))
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    fs::write(
        rel.join(format!("{asset}.sha256")),
        format!("{}  {asset}\n", sha256(&tarball)),
    )
    .unwrap();
}

fn vissue_asset(version: &str) -> String {
    format!("vissue-v{version}-{}.tar.gz", target())
}

fn deedar_asset(version: &str) -> String {
    format!("deedar-v{version}-{}.tar.gz", target())
}

fn current_ljos(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    for name in ["ljos", "ljos-mcp"] {
        executable(&dir.join(name), &says(name, version()));
    }
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

fn install_built(dir: &Path, name: &str, bin: &Path) {
    fs::create_dir_all(dir).unwrap();
    let dest = dir.join(name);
    fs::copy(bin, &dest).unwrap();
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Every installed component already matches its release, so nothing is written.
#[test]
fn every_installed_component_at_its_release_says_current() {
    let root = tempfile::tempdir().unwrap();
    let rel = root.path().join("release");
    fs::create_dir_all(&rel).unwrap();
    fs::write(rel.join("vissue.version"), "1.2.3\n").unwrap();
    fs::write(rel.join("deedar.version"), "4.5.6\n").unwrap();
    let dir = root.path().join("bin");
    current_ljos(&dir);
    let src = root.path().join("src");
    fs::create_dir_all(&src).unwrap();
    install_built(&dir, "vissue", &built(&src, "vissue", "1.2.3"));
    install_built(&dir, "deedar", &built(&src, "deedar", "4.5.6"));
    let before_v = fs::read(dir.join("vissue")).unwrap();
    let before_d = fs::read(dir.join("deedar")).unwrap();

    let out = run(root.path(), &dir, Some(&rel), version());
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("vissue 1.2.3 current"), "{said}");
    assert!(said.contains("deedar 4.5.6 current"), "{said}");
    assert!(!said.contains("warning:"), "{said}");
    assert!(!said.contains("packset"), "{said}");
    assert_eq!(fs::read(dir.join("vissue")).unwrap(), before_v);
    assert_eq!(fs::read(dir.join("deedar")).unwrap(), before_d);
    assert!(!dir.join("vissue.bak-pre-1.2.3").exists());
    assert!(!dir.join("deedar.bak-pre-4.5.6").exists());
}

/// One component is behind its release. It is replaced and the old binary kept.
#[test]
fn a_component_behind_its_release_is_replaced_and_the_old_one_kept() {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let rel = root.path().join("release");
    let old = built(&src, "vissue", "1.0.0");
    let new = built(&src, "vissue", "1.2.3");
    publish(
        &rel,
        "vissue",
        "1.2.3",
        &vissue_asset("1.2.3"),
        "vissue",
        &new,
    );
    fs::write(rel.join("deedar.version"), "4.5.6\n").unwrap();
    let dir = root.path().join("bin");
    current_ljos(&dir);
    install_built(&dir, "vissue", &old);
    install_built(&dir, "deedar", &built(&src, "deedar", "4.5.6"));
    let old_bytes = fs::read(dir.join("vissue")).unwrap();
    let deedar_bytes = fs::read(dir.join("deedar")).unwrap();

    let out = run(root.path(), &dir, Some(&rel), version());
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("vissue 1.0.0 1.2.3"), "{said}");
    assert!(said.contains("deedar 4.5.6 current"), "{said}");
    assert!(!said.contains("warning:"), "{said}");
    assert_eq!(version_line(&dir.join("vissue")), "vissue 1.2.3");
    assert_eq!(
        fs::read(dir.join("vissue.bak-pre-1.2.3")).unwrap(),
        old_bytes
    );
    assert_eq!(fs::read(dir.join("deedar")).unwrap(), deedar_bytes);
    assert!(!dir.join("deedar.bak-pre-4.5.6").exists());
}

/// A component with no release is a warning. The one that has a release still moves.
#[test]
fn a_component_with_no_release_warns_and_the_others_still_move() {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let rel = root.path().join("release");
    publish(
        &rel,
        "deedar",
        "2.0.0",
        &deedar_asset("2.0.0"),
        "deedar",
        &built(&src, "deedar", "2.0.0"),
    );
    let dir = root.path().join("bin");
    current_ljos(&dir);
    install_built(&dir, "vissue", &built(&src, "vissue", "1.0.0"));
    install_built(&dir, "deedar", &built(&src, "deedar", "1.0.0"));
    let vissue_bytes = fs::read(dir.join("vissue")).unwrap();

    let out = run(root.path(), &dir, Some(&rel), version());
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("warning: vissue: no release"), "{said}");
    assert!(said.contains("deedar 1.0.0 2.0.0"), "{said}");
    assert_eq!(fs::read(dir.join("vissue")).unwrap(), vissue_bytes);
    assert!(!dir.join("vissue.bak-pre-1.0.0").exists());
    assert_eq!(version_line(&dir.join("deedar")), "deedar 2.0.0");
    assert!(dir.join("deedar.bak-pre-2.0.0").is_file());
}

/// A checksum that does not match is a warning. The old binary stays, and the next component moves.
#[test]
fn a_checksum_mismatch_warns_and_keeps_the_old_binary() {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let rel = root.path().join("release");
    let asset = vissue_asset("1.2.3");
    publish(
        &rel,
        "vissue",
        "1.2.3",
        &asset,
        "vissue",
        &built(&src, "vissue", "1.2.3"),
    );
    fs::write(
        rel.join(format!("{asset}.sha256")),
        format!("{}  {asset}\n", "0".repeat(64)),
    )
    .unwrap();
    publish(
        &rel,
        "deedar",
        "2.0.0",
        &deedar_asset("2.0.0"),
        "deedar",
        &built(&src, "deedar", "2.0.0"),
    );
    let dir = root.path().join("bin");
    current_ljos(&dir);
    install_built(&dir, "vissue", &built(&src, "vissue", "1.0.0"));
    install_built(&dir, "deedar", &built(&src, "deedar", "1.0.0"));
    let vissue_bytes = fs::read(dir.join("vissue")).unwrap();

    let out = run(root.path(), &dir, Some(&rel), version());
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("warning: vissue: checksum mismatch"),
        "{said}"
    );
    assert!(said.contains("kept 1.0.0"), "{said}");
    assert!(said.contains("deedar 1.0.0 2.0.0"), "{said}");
    assert_eq!(fs::read(dir.join("vissue")).unwrap(), vissue_bytes);
    assert!(!dir.join("vissue.bak-pre-1.2.3").exists());
    assert_eq!(version_line(&dir.join("deedar")), "deedar 2.0.0");
}

/// `--only` moves the named program and leaves the others where they are.
#[test]
fn only_the_named_component_moves() {
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let rel = root.path().join("release");
    publish(
        &rel,
        "vissue",
        "1.2.3",
        &vissue_asset("1.2.3"),
        "vissue",
        &built(&src, "vissue", "1.2.3"),
    );
    publish(
        &rel,
        "deedar",
        "2.0.0",
        &deedar_asset("2.0.0"),
        "deedar",
        &built(&src, "deedar", "2.0.0"),
    );
    let dir = root.path().join("bin");
    fs::create_dir_all(&dir).unwrap();
    executable(&dir.join("ljos"), &says("ljos", "0.1.0"));
    install_built(&dir, "vissue", &built(&src, "vissue", "1.0.0"));
    install_built(&dir, "deedar", &built(&src, "deedar", "1.0.0"));
    let ljos_bytes = fs::read(dir.join("ljos")).unwrap();
    let deedar_bytes = fs::read(dir.join("deedar")).unwrap();

    let out = invoke(
        root.path(),
        &dir,
        Some(&rel),
        &["upgrade", "--only", "vissue"],
    );
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("vissue 1.0.0 1.2.3"), "{said}");
    assert!(!said.contains("deedar"), "{said}");
    assert!(!said.contains("installed ljos"), "{said}");
    assert_eq!(version_line(&dir.join("vissue")), "vissue 1.2.3");
    assert_eq!(fs::read(dir.join("ljos")).unwrap(), ljos_bytes);
    assert_eq!(fs::read(dir.join("deedar")).unwrap(), deedar_bytes);
}
