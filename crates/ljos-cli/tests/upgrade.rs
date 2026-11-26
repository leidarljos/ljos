//! `ljos upgrade` against a release directory on disk. No network.

ljos_test_boot::harness!();
use std::fs;
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
    ljos_cli::install_executable(path, body).unwrap();
}

fn says(name: &str, version: &str) -> String {
    format!("#!/bin/sh\necho \"{name} {version}\"\n")
}

fn elf_says(path: &Path, name: &str, version: &str) {
    let src = path.with_extension("c");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    fs::write(
        &src,
        format!(
            "#include <stdio.h>\nint main(void) {{\n    printf(\"{name} {version}\\n\");\n    return 0;\n}}\n"
        ),
    )
    .unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let compiled = Command::new("cc")
        .args(["-O2", "-o"])
        .arg(path)
        .arg(&src)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
}

fn sha256(path: &Path) -> String {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// The archive the release workflow would publish. The binaries are
/// stubs that print the version line the upgrade reads.
fn fixture_release(root: &Path) -> PathBuf {
    let top = format!("ljos-v{}-{}", version(), target());
    let stage = root.join("stage").join(&top);
    fs::create_dir_all(&stage).unwrap();
    for name in ["ljos", "ljos-mcp"] {
        // The upgrade refuses a script. The stub is a real binary that
        // prints the version line and exits.
        elf_says(&stage.join(name), name, version());
    }
    let rel = root.join("release");
    fs::create_dir_all(&rel).unwrap();
    let tarball = rel.join(asset());
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    fs::write(
        &src,
        format!(
            "#include <stdio.h>\nint main(void) {{ printf(\"{name} {version}\\n\"); return 0; }}\n"
        ),
    )
    .unwrap();
    let bin = dir.join(format!("{name}-{version}"));
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    fs::write(rel.join(format!("{repo}.version")), format!("{version}\n")).unwrap();
    let stage_root = rel.join(format!("stage-{repo}-{bin_name}"));
    let stage = stage_root.join(stem_of(asset));
    fs::create_dir_all(&stage).unwrap();
    let dest = stage.join(bin_name);
    ljos_cli::copy_executable(bin, &dest).unwrap();
    let tarball = rel.join(asset);
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
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

/// A file, a directory or a dangling symlink at the rollback name is left
/// as it is and is not taken for the rollback. The old binary is saved at
/// the next free name, and the output names that copy.
#[test]
fn a_taken_rollback_name_is_left_and_the_old_binary_is_saved_beside_it() {
    for kind in ["file", "directory", "dangling symlink"] {
        let root = tempfile::tempdir().unwrap();
        let rel = fixture_release(root.path());
        let dir = root.path().join("bin");
        fs::create_dir_all(&dir).unwrap();
        for name in ["ljos", "ljos-mcp"] {
            executable(&dir.join(name), &says(name, "0.36.3"));
            let taken = dir.join(format!("{name}.bak-pre-{}", version()));
            match kind {
                "file" => executable(&taken, &says(name, "0.36.2")),
                "directory" => fs::create_dir(&taken).unwrap(),
                _ => {
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "plants a dangling link at the rollback name"
                    )]
                    std::os::unix::fs::symlink(root.path().join("gone"), &taken).unwrap();
                }
            }
        }
        let old: Vec<Vec<u8>> = ["ljos", "ljos-mcp"]
            .iter()
            .map(|name| fs::read(dir.join(name)).unwrap())
            .collect();

        let out = run(root.path(), &dir, Some(&rel), version());
        let said = text_of(&out);
        assert!(out.status.success(), "{kind}: {said}");
        for (name, before) in ["ljos", "ljos-mcp"].iter().zip(&old) {
            let taken = dir.join(format!("{name}.bak-pre-{}", version()));
            let meta = fs::symlink_metadata(&taken).unwrap();
            match kind {
                "file" => assert_eq!(fs::read(&taken).unwrap(), says(name, "0.36.2").as_bytes()),
                "directory" => assert!(meta.is_dir(), "{kind}: {meta:?}"),
                _ => assert_eq!(fs::read_link(&taken).unwrap(), root.path().join("gone")),
            }
            let saved = dir.join(format!("{name}.bak-pre-{}.1", version()));
            let meta = fs::symlink_metadata(&saved)
                .unwrap_or_else(|err| panic!("{kind}: no {}: {err}\n{said}", saved.display()));
            assert!(meta.is_file(), "{kind}: {meta:?}");
            assert_eq!(fs::read(&saved).unwrap(), *before, "{kind}: {name}");
            assert!(
                said.contains(&format!("{name} 0.36.3 saved as {}", saved.display())),
                "{kind}: {said}"
            );
            assert_eq!(
                version_line(&dir.join(name)),
                format!("{name} {}", version())
            );
        }
    }
}

/// Upgrade, roll one binary back, upgrade again. The second run exits 0.
#[test]
fn a_second_upgrade_after_a_partial_rollback_exits_0() {
    let root = tempfile::tempdir().unwrap();
    let rel = fixture_release(root.path());
    let dir = root.path().join("bin");
    fs::create_dir_all(&dir).unwrap();
    for name in ["ljos", "ljos-mcp"] {
        executable(&dir.join(name), &says(name, "0.1.0"));
    }
    let first = run(root.path(), &dir, Some(&rel), version());
    assert!(first.status.success(), "{}", text_of(&first));
    let ljos_bytes = fs::read(dir.join("ljos")).unwrap();
    let ljos_bak = fs::read(dir.join(format!("ljos.bak-pre-{}", version()))).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "puts one installed binary back to its rollback for the next upgrade"
    )]
    fs::rename(
        dir.join(format!("ljos-mcp.bak-pre-{}", version())),
        dir.join("ljos-mcp"),
    )
    .unwrap();

    let second = run(root.path(), &dir, Some(&rel), version());
    let said = text_of(&second);
    assert!(second.status.success(), "{said}");
    assert_eq!(fs::read(dir.join("ljos")).unwrap(), ljos_bytes);
    assert_eq!(
        fs::read(dir.join(format!("ljos.bak-pre-{}", version()))).unwrap(),
        ljos_bak
    );
    assert_eq!(
        version_line(&dir.join("ljos-mcp")),
        format!("ljos-mcp {}", version())
    );
    assert_eq!(
        version_line(&dir.join("ljos")),
        format!("ljos {}", version())
    );
}

/// `--dir` may be a symlink to the directory that holds the binaries.
#[test]
fn upgrade_follows_a_symlinked_dir() {
    let root = tempfile::tempdir().unwrap();
    let rel = fixture_release(root.path());
    let real = root.path().join("real-bin");
    fs::create_dir_all(&real).unwrap();
    for name in ["ljos", "ljos-mcp"] {
        executable(&real.join(name), &says(name, "0.1.0"));
    }
    let link = root.path().join("bin");
    #[expect(
        clippy::disallowed_methods,
        reason = "plants the install directory link this test follows"
    )]
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let out = run(root.path(), &link, Some(&rel), version());
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert_eq!(
        version_line(&real.join("ljos")),
        format!("ljos {}", version())
    );
    assert_eq!(
        version_line(&real.join("ljos-mcp")),
        format!("ljos-mcp {}", version())
    );
    assert!(real.join(format!("ljos.bak-pre-{}", version())).is_file());
    assert_eq!(fs::read_link(&link).unwrap(), real);
}

fn install_built(dir: &Path, name: &str, bin: &Path) {
    fs::create_dir_all(dir).unwrap();
    let dest = dir.join(name);
    ljos_cli::copy_executable(bin, &dest).unwrap();
}

/// Every installed component already matches its release, so nothing is written.
#[test]
fn every_installed_component_at_its_release_says_current() {
    let root = tempfile::tempdir().unwrap();
    let rel = root.path().join("release");
    fs::create_dir_all(&rel).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    fs::write(rel.join("vissue.version"), "1.2.3\n").unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
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
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
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

/// The release directory has no archive. The error names the next command.
#[test]
fn a_missing_release_archive_names_the_next_command() {
    let root = tempfile::tempdir().unwrap();
    let rel = root.path().join("release");
    fs::create_dir_all(&rel).unwrap();
    let dir = root.path().join("bin");
    fs::create_dir_all(&dir).unwrap();
    executable(&dir.join("ljos"), &says("ljos", "0.1.0"));
    executable(&dir.join("ljos-mcp"), &says("ljos-mcp", "0.1.0"));
    let out = run(root.path(), &dir, Some(&rel), version());
    let said = text_of(&out);
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("could not read"), "{said}");
    assert!(said.contains("LJOS_RELEASE_DIR"), "{said}");
    assert!(said.contains("`ljos upgrade`"), "{said}");
}

struct Stop(std::process::Child);

impl Drop for Stop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `ljos-mcp` is running. Upgrade replaces the file and names that pid.
#[test]
fn upgrade_names_a_running_mcp_server() {
    let root = tempfile::tempdir().unwrap();
    let rel = fixture_release(root.path());
    let dir = root.path().join("bin");
    fs::create_dir_all(&dir).unwrap();
    executable(&dir.join("ljos"), &says("ljos", "0.1.0"));
    let src = root.path().join("sleeper.c");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    fs::write(
        &src,
        r#"
#include <stdio.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
  if (argc > 1 && strcmp(argv[1], "--version") == 0) {
    printf("ljos-mcp 0.0.1\n");
    return 0;
  }
  for (;;) pause();
}
"#,
    )
    .unwrap();
    let sleeper = root.path().join("ljos-mcp-old");
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let compiled = Command::new("cc")
        .args(["-O0", "-o"])
        .arg(&sleeper)
        .arg(&src)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    install_built(&dir, "ljos-mcp", &sleeper);
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let child = Command::new(dir.join("ljos-mcp"))
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let _stop = Stop(child);
    let out = run(root.path(), &dir, Some(&rel), version());
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains(&format!(
            "ljos-mcp is still running (pid {pid}) and is the previous binary"
        )),
        "{said}"
    );
    assert!(said.contains(&format!("kill {pid}")), "{said}");
    assert!(said.contains("so run `ljos doctor`."), "{said}");
    assert_eq!(
        version_line(&dir.join("ljos-mcp")),
        format!("ljos-mcp {}", version())
    );
}
