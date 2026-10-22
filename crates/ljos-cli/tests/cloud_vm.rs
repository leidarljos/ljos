//! A fresh machine shares one tracker. Two clones of a local bare remote
//! stand in for two cloud VMs. Nothing here talks to GitHub.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());

struct Guard {
    saved: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    _lock: MutexGuard<'static, ()>,
}

impl Guard {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let saved = std::env::vars_os().collect();
        Self { saved, _lock: lock }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let keep: Vec<_> = self.saved.iter().map(|(k, _)| k.clone()).collect();
        for (k, _) in std::env::vars_os() {
            if !keep.iter().any(|have| have == &k) {
                std::env::remove_var(&k);
            }
        }
        for (k, v) in &self.saved {
            std::env::set_var(k, v);
        }
    }
}

fn sh_at(dir: &Path, script: &str) -> std::process::Output {
    Command::new("sh")
        .current_dir(dir)
        .arg("-c")
        .arg(script)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_else(|e| panic!("sh -c in {}: {e}", dir.display()))
}

fn sh_ok(dir: &Path, script: &str) -> String {
    let out = sh_at(dir, script);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "sh -c {script:?} in {}: {text}",
        dir.display()
    );
    text
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}\n{}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn identity(dir: &Path) {
    let hooks = dir.join("no-hooks");
    fs::create_dir_all(&hooks).unwrap();
    for (key, value) in [
        ("user.email", "seat@example.invalid"),
        ("user.name", "seat"),
        ("commit.gpgsign", "false"),
    ] {
        git(dir, &["config", key, value]);
    }
    git(dir, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
}

fn isolate_git(root: &Path) {
    let cfg = root.join("gitconfig");
    fs::write(&cfg, b"").unwrap();
    std::env::set_var("GIT_CONFIG_GLOBAL", &cfg);
    std::env::set_var("GIT_CONFIG_SYSTEM", &cfg);
    std::env::set_var("GIT_TERMINAL_PROMPT", "0");
    std::env::set_var("PATH", "/usr/bin:/bin");
}

fn issue_block(id: &str, state: &str, props: &str, notes: &[&str]) -> String {
    let mut s = format!("* {state} [#A] Work on {id}\n:PROPERTIES:\n:ID:       {id}\n");
    if !props.is_empty() {
        s.push_str(props);
        if !props.ends_with('\n') {
            s.push('\n');
        }
    }
    s.push_str(":END:\n");
    if !notes.is_empty() {
        s.push_str(":LOGBOOK:\n");
        for note in notes {
            s.push_str(note);
            s.push('\n');
        }
        s.push_str(":END:\n");
    }
    s
}

fn board(blocks: &[String]) -> String {
    format!(
        "#+VISSUE: 1\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n{}",
        blocks.join("\n")
    )
}

fn write_project(repo: &Path, project: &str, body: &str) -> PathBuf {
    let dir = repo.join("Software").join(project);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("issues.org");
    fs::write(&path, body).unwrap();
    path
}

fn init_bare(root: &Path) -> PathBuf {
    let origin = root.join("origin.git");
    git(
        root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    git(&origin, &["config", "receive.denyNonFastForwards", "true"]);
    origin
}

fn seed(root: &Path, origin: &Path, body: &str) {
    let repo = root.join("seed");
    git(
        root,
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            repo.to_str().unwrap(),
        ],
    );
    identity(&repo);
    write_project(&repo, "probe", body);
    fs::write(
        repo.join(".gitattributes"),
        "*.org merge=ljos-org\n.ljos/atoms/* merge=ljos-atoms\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "seed"]);
    git(&repo, &["push", "-q", "-u", "origin", "HEAD"]);
}

fn clone_of(root: &Path, origin: &Path, name: &str) -> PathBuf {
    let dest = root.join(name);
    git(
        root,
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            dest.to_str().unwrap(),
        ],
    );
    identity(&dest);
    dest
}

fn show(origin: &Path, file: &str) -> String {
    git(origin, &["show", &format!("HEAD:{file}")])
}

fn state_of(text: &str, id: &str) -> String {
    let holds = |block: &str| {
        block.lines().any(|line| {
            let mut words = line.split_whitespace();
            words.next() == Some(":ID:") && words.next() == Some(id)
        })
    };
    let mut state = String::new();
    let mut block = String::new();
    for line in text.lines() {
        if line.starts_with("* ") {
            if holds(&block) {
                return state;
            }
            state = line.split_whitespace().nth(1).unwrap_or("").to_string();
            block.clear();
        }
        block.push_str(line);
        block.push('\n');
    }
    if holds(&block) {
        state
    } else {
        panic!("missing {id} in {text}")
    }
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn sha256(path: &Path) -> String {
    let out = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn sums(dir: &Path, names: &[&str]) -> String {
    let mut out = String::new();
    for name in names {
        out.push_str(&sha256(&dir.join(name)));
        out.push_str("  ");
        out.push_str(name);
        out.push('\n');
    }
    out
}

#[test]
fn a_checksum_mismatch_installs_nothing() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let rel = root.path().join("rel");
    let bins = root.path().join("bins");
    fs::create_dir_all(&rel).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\necho ok\n");
    }
    let mut listed = sums(&rel, &["ljos", "ljos-mcp", "ljos-policyd"]);
    listed = listed.replacen(
        &sha256(&rel.join("ljos-mcp")),
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        1,
    );
    fs::write(rel.join("SHA256SUMS"), listed).unwrap();
    std::env::set_var("HOME", root.path().join("home"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::set_var("LJOS_BIN_DIR", &bins);
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    let err = ljos_cli::cloud::up(false).unwrap_err().to_string();
    assert!(err.contains("nothing was installed"), "{err}");
    assert!(
        !bins.join("ljos").exists(),
        "a bad sum still copied a binary"
    );
    assert!(!bins.join("ljos-mcp").exists());
}

/// A home and a release directory with the required binaries and the
/// encoder archives as the release workflow packs them, each beside its
/// `.sha256`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn encoder_machine(root: &Path) -> (PathBuf, PathBuf) {
    let home = root.join("home");
    let rel = root.join("rel");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(rel.join("rt/lib/onnxruntime")).unwrap();
    fs::create_dir_all(rel.join("embed/bin")).unwrap();
    let names = ["ljos", "ljos-mcp", "ljos-policyd"];
    for name in names {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(rel.join("SHA256SUMS"), sums(&rel, &names)).unwrap();
    fs::write(
        rel.join("rt/lib/onnxruntime/libonnxruntime.so.1"),
        b"runtime",
    )
    .unwrap();
    executable(&rel.join("embed/bin/packset-embed"), "#!/bin/sh\nexit 0\n");
    for (dir, top, base) in [
        ("rt", "lib", "onnxruntime"),
        ("embed", "bin", "packset-embed"),
    ] {
        let name = format!("{base}-x86_64-unknown-linux-gnu.tar.gz");
        let out = Command::new("tar")
            .arg("-C")
            .arg(rel.join(dir))
            .arg("-czf")
            .arg(rel.join(&name))
            .arg(top)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let line = format!("{}  {name}\n", sha256(&rel.join(&name)));
        fs::write(rel.join(format!("{name}.sha256")), line).unwrap();
    }
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    std::env::set_var("CURSOR_AGENT_ID", "bc-encoder-run");
    (home, rel)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn cloud_up_installs_the_release_encoder_on_a_cloud_vm() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, _rel) = encoder_machine(root.path());
    let said = ljos_cli::cloud::up(false).unwrap();
    assert!(said.contains("encoder: installed"), "{said}");
    assert!(said.contains("(checksums match)"), "{said}");
    assert!(home.join(".local/bin/packset-embed").is_file(), "{said}");
    assert_eq!(
        fs::read(home.join(".local/lib/onnxruntime/libonnxruntime.so.1")).unwrap(),
        b"runtime",
        "the rpath $ORIGIN/../lib/onnxruntime finds the runtime here"
    );
    let left: Vec<_> = fs::read_dir(home.join(".local/lib"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["onnxruntime"], "the staging directory stays behind");
    let again = ljos_cli::cloud::up(false).unwrap();
    assert!(
        again.contains("encoder: packset-embed already in"),
        "{again}"
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn an_encoder_checksum_mismatch_installs_neither_archive() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, rel) = encoder_machine(root.path());
    let name = "packset-embed-x86_64-unknown-linux-gnu.tar.gz";
    let forged = format!("{}  {name}\n", "0".repeat(64));
    fs::write(rel.join(format!("{name}.sha256")), forged).unwrap();
    let err = ljos_cli::cloud::up(false).unwrap_err().to_string();
    assert!(err.contains("checksum mismatch"), "{err}");
    assert!(err.contains("nothing was installed"), "{err}");
    assert!(!home.join(".local/bin/packset-embed").exists());
    assert_eq!(
        fs::read_dir(home.join(".local/lib")).unwrap().count(),
        0,
        "the runtime's own sum matched, and it still waits for both"
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn repack(rel: &Path, archive: &str, work: &Path, top: &str, transform: Option<&str>) {
    let dest = rel.join(archive);
    let mut cmd = Command::new("tar");
    cmd.arg("-C").arg(work);
    if let Some(expr) = transform {
        cmd.args(["--transform", expr]);
    }
    let out = cmd.arg("-czf").arg(&dest).arg(top).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = format!("{}  {archive}\n", sha256(&dest));
    fs::write(rel.join(format!("{archive}.sha256")), line).unwrap();
}

/// Replace one release archive with a member `cloud up` must not install.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn plant_hostile(root: &Path, case: &str) -> (PathBuf, PathBuf) {
    let (home, rel) = encoder_machine(root);
    let outside = root.join("outside-secret");
    fs::write(&outside, b"secret").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o644)).unwrap();
    let work = root.join("work");
    let embed = "packset-embed-x86_64-unknown-linux-gnu.tar.gz";
    let runtime = "onnxruntime-x86_64-unknown-linux-gnu.tar.gz";
    match case {
        "symlink" => {
            fs::create_dir_all(work.join("bin")).unwrap();
            std::os::unix::fs::symlink(&outside, work.join("bin/packset-embed")).unwrap();
            repack(&rel, embed, &work, "bin", None);
        }
        "runtime-symlink" => {
            fs::create_dir_all(work.join("lib/onnxruntime")).unwrap();
            std::os::unix::fs::symlink(&outside, work.join("lib/onnxruntime/libonnxruntime.so.1"))
                .unwrap();
            repack(&rel, runtime, &work, "lib", None);
        }
        "hardlink" => {
            fs::create_dir_all(work.join("bin")).unwrap();
            fs::write(work.join("bin/packset-embed"), b"#!/bin/sh\nexit 0\n").unwrap();
            fs::hard_link(work.join("bin/packset-embed"), work.join("bin/also")).unwrap();
            repack(&rel, embed, &work, "bin", None);
        }
        "absolute" => {
            fs::create_dir_all(work.join("bin")).unwrap();
            executable(&work.join("bin/packset-embed"), "#!/bin/sh\nexit 0\n");
            repack(&rel, embed, &work, "bin", Some("s,^,/,"));
        }
        "dotdot" => {
            fs::create_dir_all(work.join("bin")).unwrap();
            executable(&work.join("bin/packset-embed"), "#!/bin/sh\nexit 0\n");
            repack(&rel, embed, &work, "bin", Some("s,^bin,..,"));
        }
        "fifo" => {
            fs::create_dir_all(work.join("bin")).unwrap();
            let made = Command::new("mkfifo")
                .arg(work.join("bin/packset-embed"))
                .status()
                .unwrap();
            assert!(made.success(), "mkfifo");
            repack(&rel, embed, &work, "bin", None);
        }
        other => panic!("unknown archive case {other}"),
    }
    (home, outside)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn a_hostile_encoder_archive_installs_nothing() {
    for (case, needle) in [
        ("symlink", "is a symlink"),
        ("runtime-symlink", "is a symlink"),
        ("hardlink", "is a hard link"),
        ("absolute", "absolute path"),
        ("dotdot", "absolute path"),
        ("fifo", "not a regular file"),
    ] {
        let _g = Guard::new();
        let root = tempfile::tempdir().unwrap();
        let (home, outside) = plant_hostile(root.path(), case);
        let err = ljos_cli::cloud::up(false).expect_err(case).to_string();
        assert!(err.contains(needle), "{case}: {err}");
        assert!(err.contains("nothing was installed"), "{case}: {err}");
        assert!(
            !home.join(".local/bin/packset-embed").exists(),
            "{case}: encoder installed: {err}"
        );
        assert!(
            !home.join(".local/lib/onnxruntime").exists(),
            "{case}: runtime installed: {err}"
        );
        if case == "symlink" || case == "runtime-symlink" {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&outside).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o644, "{case} changed the link target: {err}");
        }
    }
}

/// Replace one release archive and rewrite its `.sha256`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn replace_archive(rel: &Path, base: &str, top: &str, files: &[(&str, &[u8])]) {
    let work = rel.join(format!("rebuild-{base}"));
    let _ = fs::remove_dir_all(&work);
    for (rel_path, bytes) in files {
        let path = work.join(rel_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
    }
    let archive = format!("{base}-x86_64-unknown-linux-gnu.tar.gz");
    repack(rel, &archive, &work, top, None);
}

/// `0o555` lets the owner chmod the directory back and blocks a rename into it.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn lock_bin_dir(home: &Path) -> PathBuf {
    let bin_dir = home.join(".local/bin");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&bin_dir, fs::Permissions::from_mode(0o555)).unwrap();
    bin_dir
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn unlock_bin_dir(bin_dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(bin_dir, fs::Permissions::from_mode(0o755));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn a_failed_encoder_swap_keeps_the_previous_pair() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, rel) = encoder_machine(root.path());
    let said = ljos_cli::cloud::up(false).unwrap();
    assert!(said.contains("encoder: installed"), "{said}");
    let bin = home.join(".local/bin/packset-embed");
    let soname = home.join(".local/lib/onnxruntime/libonnxruntime.so.1");
    let old_bin = fs::read(&bin).unwrap();
    assert_eq!(fs::read(&soname).unwrap(), b"runtime");
    replace_archive(
        &rel,
        "onnxruntime",
        "lib",
        &[("lib/onnxruntime/libonnxruntime.so.1", b"runtime-new")],
    );
    replace_archive(
        &rel,
        "packset-embed",
        "bin",
        &[("bin/packset-embed", b"#!/bin/sh\nexit 7\n")],
    );
    let bin_dir = lock_bin_dir(&home);
    let err = ljos_cli::cloud::up(true).unwrap_err().to_string();
    unlock_bin_dir(&bin_dir);
    assert_eq!(fs::read(&soname).unwrap(), b"runtime", "{err}");
    assert_eq!(fs::read(&bin).unwrap(), old_bin, "{err}");
    assert!(err.contains("nothing was installed"), "{err}");
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn a_failed_first_encoder_install_leaves_no_runtime() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, rel) = encoder_machine(root.path());
    let bin_dir = home.join(".local/bin");
    fs::create_dir_all(&bin_dir).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        fs::copy(rel.join(name), bin_dir.join(name)).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin_dir.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
    lock_bin_dir(&home);
    let err = ljos_cli::cloud::up(false).unwrap_err().to_string();
    unlock_bin_dir(&bin_dir);
    assert!(
        !home.join(".local/lib/onnxruntime").exists(),
        "a failed first install left the runtime: {err}"
    );
    assert!(!bin_dir.join("packset-embed").exists(), "{err}");
    assert!(bin_dir.join("ljos").is_file());
    assert!(err.contains("nothing was installed"), "{err}");
}

#[test]
fn the_cloud_vm_doc_matches_the_encoder_install() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let docs = fs::read_to_string(root.join("docs/cloud-vm.md")).unwrap();
    assert!(
        !docs.contains("is built from source"),
        "docs still say an off-cloud cloud up builds packset-embed"
    );
    assert!(
        docs.contains("WITH_EMBED=0"),
        "docs omit that install.sh --cloud sets WITH_EMBED=0"
    );
    let src = fs::read_to_string(root.join("crates/ljos-cli/src/cloud.rs")).unwrap();
    assert!(
        !src.contains("from source"),
        "cloud.rs still says the encoder is built from source"
    );
}

#[test]
fn a_release_build_does_not_reuse_another_onnx_runtime() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/release.yml");
    let yml = fs::read_to_string(&path).unwrap();
    assert!(
        yml.contains("github.ref_type != 'tag'"),
        "a tag run restores the ONNX Runtime cache"
    );
    assert!(
        yml.contains("github.event_name != 'workflow_dispatch'"),
        "a dispatched release restores the ONNX Runtime cache"
    );
    assert!(
        yml.contains("github.ref_type == 'tag'"),
        "a tag run can skip the source build"
    );
    assert!(
        yml.lines()
            .any(|line| line.contains("sha256sum") && line.contains("libonnxruntime")),
        "the release sums omit the built library"
    );
    assert!(
        yml.contains("cp -L"),
        "the runtime archive still packs soname symlinks"
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn an_encoder_directory_is_not_already_installed() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, rel) = encoder_machine(root.path());
    let bin_dir = home.join(".local/bin");
    fs::create_dir_all(&bin_dir).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        fs::copy(rel.join(name), bin_dir.join(name)).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin_dir.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let encoder = bin_dir.join("packset-embed");
    fs::create_dir(&encoder).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&encoder, fs::Permissions::from_mode(0o755)).unwrap();
    let soname = home.join(".local/lib/onnxruntime/libonnxruntime.so.1");
    fs::create_dir_all(soname.parent().unwrap()).unwrap();
    fs::write(&soname, b"planted").unwrap();
    let name = "packset-embed-x86_64-unknown-linux-gnu.tar.gz";
    let forged = format!("{}  {name}\n", "ab".repeat(32));
    fs::write(rel.join(format!("{name}.sha256")), forged).unwrap();
    let err = ljos_cli::cloud::up(false).unwrap_err().to_string();
    assert!(err.contains("checksum mismatch"), "{err}");
    assert!(err.contains("nothing was installed"), "{err}");
    assert_eq!(fs::read(&soname).unwrap(), b"planted", "{err}");
    assert!(encoder.is_dir(), "the directory was replaced: {err}");
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn a_ci_job_downloads_no_encoder() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, _rel) = encoder_machine(root.path());
    std::env::set_var("GITHUB_ACTIONS", "true");
    std::env::set_var("GITHUB_RUN_ID", "555001");
    let said = ljos_cli::cloud::up(false).unwrap();
    assert!(said.contains("encoder: not a cloud VM"), "{said}");
    assert!(!home.join(".local/bin/packset-embed").exists());
    assert!(!home.join(".local/lib/onnxruntime").exists());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn a_generic_ci_job_downloads_no_encoder() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (home, _rel) = encoder_machine(root.path());
    std::env::set_var("CI", "true");
    let said = ljos_cli::cloud::up(false).unwrap();
    assert!(
        said.contains("encoder: not a cloud VM"),
        "CI=true is not a cloud VM: {said}"
    );
    assert!(!home.join(".local/bin/packset-embed").exists());
    assert!(!home.join(".local/lib/onnxruntime").exists());
}

#[test]
fn cloud_up_installs_points_the_harness_and_clones() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    isolate_git(root.path());
    let origin = init_bare(root.path());
    seed(
        root.path(),
        &origin,
        &board(&[issue_block(
            "probe-seed",
            "TODO",
            "",
            &["- Note: \"seed\" [2026-01-01 Thu 00:00]"],
        )]),
    );
    let home = root.path().join("home");
    let rel = root.path().join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(home.join(".config/ljos")).unwrap();
    fs::write(home.join(".config/ljos/env"), "LJOS_SEAT=baked-seat\n").unwrap();
    let names = ["ljos", "ljos-mcp", "ljos-policyd", "packsetd"];
    for name in names {
        executable(&rel.join(name), "#!/bin/sh\necho ok\n");
    }
    fs::write(rel.join("SHA256SUMS"), sums(&rel, &names)).unwrap();
    let runtime = root.path().join("run");
    fs::create_dir_all(&runtime).unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_RUNTIME_DIR", &runtime);
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("LJOS_TRACKER_REMOTE", &origin);
    clear_agent_ids();
    std::env::set_var("LJOS_SEAT", "baked-seat");
    std::env::set_var("CURSOR_AGENT_ID", "abcdef012345");
    let host_key = home.join(".config/deedar/host.key");
    fs::create_dir_all(host_key.parent().unwrap()).unwrap();
    fs::write(&host_key, [9u8; 32]).unwrap();
    let first = ljos_cli::cloud::up(false).unwrap();
    assert!(first.contains("checksums match"), "{first}");
    assert!(first.contains("cursor-abcdef012345"), "{first}");
    assert!(!first.contains("push queued"), "{first}");
    let server = home.join(".local/bin/ljos-mcp");
    assert!(server.is_file(), "ljos-mcp was not installed");
    assert!(home.join(".local/bin/packsetd").is_file());
    let mcp = fs::read_to_string(home.join(".cursor/mcp.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&mcp).unwrap();
    assert_eq!(
        doc["mcpServers"]["ljos"]["command"],
        server.display().to_string(),
        "PATH did not have the install dir when cloud up started: {mcp}"
    );
    assert!(mcp.contains("\"LJOS_TRACKER_GIT\": \"push\""), "{mcp}");
    let env = fs::read_to_string(home.join(".config/ljos/env")).unwrap();
    assert!(env.contains("LJOS_SEAT=cursor-abcdef012345\n"), "{env}");
    assert!(env.contains("VISSUE_AGENT=cursor-abcdef012345\n"), "{env}");
    assert!(env.contains("LJOS_TRACKER_GIT=push\n"), "{env}");
    assert!(env.contains("VISSUE_ROOT="), "{env}");
    let signing = env
        .lines()
        .find(|line| line.starts_with("DEEDAR_HOST_SIGNING_KEY="))
        .unwrap_or("");
    assert!(signing.contains("ballot.key"), "{env}");
    assert!(!signing.contains("host.key"), "{env}");
    assert_eq!(fs::read(&host_key).unwrap(), vec![9u8; 32]);
    let profile = fs::read_to_string(home.join(".profile")).unwrap();
    assert!(
        profile.contains(&home.join(".local/bin").display().to_string()),
        "{profile}"
    );
    let tracker = home.join(".local/share/ljos/tracker");
    assert_eq!(
        git(&tracker, &["rev-parse", "--is-inside-work-tree"]).trim(),
        "true"
    );
    let second = ljos_cli::cloud::up(false).unwrap();
    assert!(second.contains("tracker:"), "{second}");
}

#[test]
fn cloud_up_puts_ljos_mcp_on_path_before_the_config() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let rel = root.path().join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(
        home.join(".bashrc"),
        "# test rc\n[ -z \"$PS1\" ] && return\necho after\n",
    )
    .unwrap();
    let names = ["ljos", "ljos-mcp", "ljos-policyd"];
    executable(&rel.join("ljos"), "#!/bin/sh\nexit 0\n");
    executable(&rel.join("ljos-policyd"), "#!/bin/sh\nexit 0\n");
    executable(&rel.join("ljos-mcp"), "#!/bin/sh\nprintf ran > \"$1\"\n");
    fs::write(rel.join("SHA256SUMS"), sums(&rel, &names)).unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    let said = ljos_cli::cloud::up(false).unwrap();
    let bin = home.join(".local/bin");
    let path = std::env::var("PATH").unwrap_or_default();
    assert!(
        path.split(':').next() == Some(bin.to_str().unwrap()),
        "PATH should start with the install dir: {path}\n{said}"
    );
    let marker = root.path().join("marker");
    let ran = Command::new("ljos-mcp").arg(&marker).output().unwrap();
    assert!(
        ran.status.success(),
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(fs::read_to_string(&marker).unwrap(), "ran");
    let mcp = fs::read_to_string(home.join(".cursor/mcp.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&mcp).unwrap();
    assert_eq!(
        doc["mcpServers"]["ljos"]["command"],
        bin.join("ljos-mcp").display().to_string(),
        "{mcp}"
    );
    let bashrc = fs::read_to_string(home.join(".bashrc")).unwrap();
    let bin_line = format!("export PATH=\"{}:$PATH\"", bin.display());
    assert!(bashrc.contains(&bin_line), "{bashrc}");
    let path_at = bashrc.find(&bin_line).unwrap();
    let guard_at = bashrc.find("[ -z \"$PS1\" ] && return").unwrap();
    assert!(path_at < guard_at, "{bashrc}");
    assert!(fs::read_to_string(home.join(".profile"))
        .unwrap()
        .contains(&bin_line));
    ljos_cli::cloud::up(false).unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    let dir = bin.display().to_string();
    assert_eq!(path.matches(&dir).count(), 1, "{path}");
    assert_eq!(
        fs::read_to_string(home.join(".bashrc"))
            .unwrap()
            .matches(&bin_line)
            .count(),
        1
    );
}

/// A stale `ljos-mcp` earlier on PATH is what `command -v` runs. Naming
/// the bare command would keep that binary; the install dir has to come
/// first, and the config has to name this install while the stale one
/// still wins.
#[test]
fn cloud_up_puts_the_install_dir_ahead_of_a_stale_binary() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let rel = root.path().join("rel");
    let custom = root.path().join("custom/bin");
    let stale = root.path().join("stale/bin");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&stale).unwrap();
    executable(
        &stale.join("ljos-mcp"),
        "#!/bin/sh\nprintf stale > \"$1\"\n",
    );
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\nprintf fresh > \"$1\"\n");
    }
    fs::write(
        rel.join("SHA256SUMS"),
        sums(&rel, &["ljos", "ljos-mcp", "ljos-policyd"]),
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::set_var("LJOS_BIN_DIR", &custom);
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var(
        "PATH",
        format!("{}:{}:/usr/bin:/bin", stale.display(), custom.display()),
    );
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    let said = ljos_cli::cloud::up(false).unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    assert_eq!(
        path.split(':').next(),
        Some(custom.to_str().unwrap()),
        "{path}\n{said}"
    );
    assert_eq!(path.matches(custom.to_str().unwrap()).count(), 1, "{path}");
    let found = Command::new("sh")
        .arg("-c")
        .arg("command -v ljos-mcp")
        .output()
        .unwrap();
    let found = String::from_utf8_lossy(&found.stdout);
    assert!(
        found.trim() == custom.join("ljos-mcp").to_str().unwrap(),
        "command -v: {found}"
    );
    let marker = root.path().join("marker");
    let ran = Command::new("ljos-mcp").arg(&marker).output().unwrap();
    assert!(ran.status.success(), "{ran:?}");
    assert_eq!(fs::read_to_string(&marker).unwrap(), "fresh");
    let mcp = fs::read_to_string(home.join(".cursor/mcp.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&mcp).unwrap();
    assert_eq!(
        doc["mcpServers"]["ljos"]["command"],
        custom.join("ljos-mcp").display().to_string(),
        "{mcp}\n{said}"
    );
    assert!(said.contains(stale.to_str().unwrap()), "{said}");
    assert!(said.contains("cloud up wrote"), "{said}");
}

/// The Cursor command `cloud up` writes when it starts with `path` and
/// installs into `bin`.
fn cloud_up_command(root: &Path, bin: &Path, path: &str) -> String {
    let home = root.join("home");
    let rel = root.join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(&home).unwrap();
    let names = ["ljos", "ljos-mcp", "ljos-policyd"];
    for name in names {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(rel.join("SHA256SUMS"), sums(&rel, &names)).unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::set_var("LJOS_BIN_DIR", bin);
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("PATH", path);
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    let said = ljos_cli::cloud::up(false).unwrap();
    let mcp = fs::read_to_string(home.join(".cursor/mcp.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&mcp).unwrap();
    let command = doc["mcpServers"]["ljos"]["command"].as_str();
    command
        .unwrap_or_else(|| panic!("{mcp}\n{said}"))
        .to_string()
}

/// Putting the install dir first reaches this process and shells that
/// read the edited profile. A client started any other way keeps the
/// PATH `cloud up` found, so the bare name is written only when that PATH
/// already runs this install. A trailing slash or a symlink names the
/// same directory.
#[test]
fn cloud_up_writes_the_bare_name_only_when_path_already_runs_this_install() {
    let _g = Guard::new();
    for (shape, bare) in [
        ("{stale}:/usr/bin:/bin", false),
        ("{stale}:{bin}/:/usr/bin:/bin", false),
        ("{stale}:{link}:/usr/bin:/bin", false),
        ("{bin}/:/usr/bin:/bin", true),
        ("{link}:{stale}:/usr/bin:/bin", true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("custom/bin");
        let stale = root.path().join("stale/bin");
        let link = root.path().join("link");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&stale).unwrap();
        executable(&stale.join("ljos-mcp"), "#!/bin/sh\nexit 1\n");
        std::os::unix::fs::symlink(&bin, &link).unwrap();
        let path = shape
            .replace("{stale}", stale.to_str().unwrap())
            .replace("{bin}", bin.to_str().unwrap())
            .replace("{link}", link.to_str().unwrap());
        let want = if bare {
            "ljos-mcp".to_string()
        } else {
            bin.join("ljos-mcp").display().to_string()
        };
        assert_eq!(cloud_up_command(root.path(), &bin, &path), want, "{shape}");
    }
}

#[test]
fn cloud_up_writes_custom_ljos_bin_dir_into_shell_profiles() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let rel = root.path().join("rel");
    let custom = root.path().join("custom/bin");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(
        home.join(".bashrc"),
        "if [ -z \"$PS1\" ]; then return; fi\necho prompt\n",
    )
    .unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(
        rel.join("SHA256SUMS"),
        sums(&rel, &["ljos", "ljos-mcp", "ljos-policyd"]),
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::set_var("LJOS_BIN_DIR", &custom);
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    ljos_cli::cloud::up(false).unwrap();
    let line = format!("export PATH=\"{}:$PATH\"", custom.display());
    let profile = fs::read_to_string(home.join(".profile")).unwrap();
    assert!(profile.contains(&line), "{profile}");
    let bashrc = fs::read_to_string(home.join(".bashrc")).unwrap();
    assert!(bashrc.contains(&line), "{bashrc}");
    let line_at = bashrc.find(&line).unwrap();
    let guard_at = bashrc.find("return").unwrap();
    assert!(line_at < guard_at, "{bashrc}");
}

/// The head of Ubuntu's `/etc/skel/.bashrc`, carried here because the
/// host's own copy depends on its distribution.
const UBUNTU_SKEL_BASHRC: &str = "\
# ~/.bashrc: executed by bash(1) for non-login shells.
# see /usr/share/doc/bash/examples/startup-files (in the package bash-doc)
# for examples

# If not running interactively, don't do anything
case $- in
    *i*) ;;
      *) return;;
esac

# don't put duplicate lines or lines starting with space in the history.
# See bash(1) for more options
HISTCONTROL=ignoreboth
";

/// Ubuntu's skel bashrc returns from a `case $-` before any later PATH
/// export. A non-interactive `bash -c '. ~/.bashrc'` never sees a line
/// appended after that case.
#[test]
fn cloud_up_inserts_path_before_ubuntu_skel_interactive_case() {
    let _g = Guard::new();
    assert_path_before_guard(UBUNTU_SKEL_BASHRC, "case $- in");
}

/// The same guard as an `if` whose test, `return` and `fi` are on lines
/// of their own.
#[test]
fn cloud_up_inserts_path_before_a_multi_line_if_guard() {
    let _g = Guard::new();
    assert_path_before_guard(
        "# rc\nif [ -z \"$PS1\" ]; then\n    return\nfi\necho prompt\n",
        "if [ -z",
    );
}

/// Cloud up over a home whose `.bashrc` is `rc` puts the PATH export
/// before `guard`, and a non-interactive bash that sources the file finds
/// this install's `ljos-mcp`.
fn assert_path_before_guard(rc: &str, guard: &str) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let rel = root.path().join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), rc).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(
        rel.join("SHA256SUMS"),
        sums(&rel, &["ljos", "ljos-mcp", "ljos-policyd"]),
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    ljos_cli::cloud::up(false).unwrap();
    let bin = home.join(".local/bin");
    let bashrc = fs::read_to_string(home.join(".bashrc")).unwrap();
    let line = format!("export PATH=\"{}:$PATH\"", bin.display());
    let path_at = bashrc.find(&line).unwrap_or_else(|| panic!("{bashrc}"));
    let guard_at = bashrc.find(guard).unwrap_or_else(|| panic!("{bashrc}"));
    assert!(path_at < guard_at, "{bashrc}");
    let out = Command::new("bash")
        .arg("-c")
        .arg(". \"$HOME/.bashrc\"; command -v ljos-mcp")
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let found = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        found.trim(),
        bin.join("ljos-mcp").display().to_string(),
        "stderr {}\n{bashrc}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cloud_up_wakes_box_seats_with_keys_passed_by_name() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let config = home.join(".config");
    let rel = root.path().join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(config.join("ljos")).unwrap();
    let names = ["ljos", "ljos-mcp", "ljos-policyd"];
    for name in names {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(rel.join("SHA256SUMS"), sums(&rel, &names)).unwrap();
    let wakes = config.join("ljos/wake.toml");
    fs::write(&wakes, "[claude-review]\ncommand = \"true\"\n").unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", &config);
    std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("LJOS_WAKE_") {
            std::env::remove_var(&key);
        }
    }
    let hook = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://127.0.0.1:{}/hook",
        hook.local_addr().unwrap().port()
    );
    std::env::set_var("LJOS_WAKE_INKY_URL", &url);
    std::env::set_var("LJOS_WAKE_INKY_KEY", "inky-sender-key");
    std::env::set_var(
        "LJOS_WAKE_GROK_BOT_URL",
        "https://api2.cursor.sh/automations/webhook/grok",
    );
    std::env::set_var("LJOS_WAKE_PLAIN_URL", "http://example.com/hook");

    let said = ljos_cli::cloud::up(false).unwrap();
    assert!(
        said.contains(
            "wake inky: posts to LJOS_WAKE_INKY_URL with the key in LJOS_WAKE_INKY_KEY\n"
        ),
        "{said}"
    );
    assert!(
        said.contains("wake grok-bot: posts to LJOS_WAKE_GROK_BOT_URL with the key in LJOS_WAKE_GROK_BOT_KEY, which is unset now"),
        "{said}"
    );
    assert!(
        said.contains("wake plain: LJOS_WAKE_PLAIN_URL is not https"),
        "{said}"
    );
    assert!(!said.contains(&url), "{said}");
    assert!(!said.contains("inky-sender-key"), "{said}");
    let text = fs::read_to_string(&wakes).unwrap();
    assert!(!text.contains("inky-sender-key"), "{text}");
    assert!(!text.contains("key_file"), "{text}");
    let table = ljos_cli::wake::load();
    assert_eq!(table["inky"].webhook.as_deref(), Some(url.as_str()));
    assert_eq!(table["inky"].key_env.as_deref(), Some("LJOS_WAKE_INKY_KEY"));
    assert_eq!(
        table["grok-bot"].key_env.as_deref(),
        Some("LJOS_WAKE_GROK_BOT_KEY")
    );
    assert!(!table.contains_key("plain"), "{text}");
    assert_eq!(table["claude-review"].command.as_deref(), Some("true"));
    ljos_cli::cloud::up(false).unwrap();
    assert_eq!(fs::read_to_string(&wakes).unwrap(), text);

    let heard = std::thread::spawn(move || {
        let (conn, _) = hook.accept().unwrap();
        let mut reader = BufReader::new(conn);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            head.push_str(&line);
        }
        let len = head
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|n| n.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        let mut body = vec![0; len];
        reader.read_exact(&mut body).unwrap();
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
        head
    });
    let fired = ljos_cli::wake::fire(&ljos_cli::wake::Event {
        seat: "inky".into(),
        from: "vm-seat".into(),
        msg: "m1".into(),
        ..ljos_cli::wake::Event::default()
    })
    .unwrap();
    assert!(fired.contains("posted, 200"), "{fired}");
    let head = heard.join().unwrap().to_ascii_lowercase();
    assert!(
        head.contains("authorization: bearer inky-sender-key"),
        "{head}"
    );
}

#[test]
fn cloud_up_retires_stale_env_wakes_and_keeps_a_hand_set_wake() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let config = home.join(".config");
    let rel = root.path().join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(config.join("ljos")).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(
        rel.join("SHA256SUMS"),
        sums(&rel, &["ljos", "ljos-mcp", "ljos-policyd"]),
    )
    .unwrap();
    let wakes = config.join("ljos/wake.toml");
    fs::write(
        &wakes,
        "\
[claude-review]
command = \"true\"

[inky]
webhook = \"https://old.example/hook\"
key_env = \"LJOS_WAKE_INKY_KEY\"
source = \"env\"

[grok-bot]
webhook = \"https://old.example/grok\"
key_env = \"LJOS_WAKE_GROK_BOT_KEY\"
source = \"env\"

[evil]
webhook = \"https://old.example/evil\"
key_env = \"LJOS_WAKE_EVIL_KEY\"
source = \"env\"

[retired]
webhook = \"https://old.example/retired\"
key_env = \"LJOS_WAKE_RETIRED_KEY\"
source = \"env\"

[saddle]
webhook = \"https://kept.example/hook\"
key_env = \"SADDLE_HAND_KEY\"
",
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", &config);
    std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("LJOS_WAKE_") {
            std::env::remove_var(&key);
        }
    }
    let inky = "https://api2.cursor.sh/automations/webhook/inky";
    std::env::set_var("LJOS_WAKE_INKY_URL", inky);
    std::env::set_var("LJOS_WAKE_INKY_KEY", "inky-sender-key");
    std::env::set_var(
        "LJOS_WAKE_GROK_BOT_URL",
        "https://api2.cursor.sh/automations/webhook/grok-a",
    );
    std::env::set_var(
        "LJOS_WAKE_GROK-BOT_URL",
        "https://api2.cursor.sh/automations/webhook/grok-b",
    );
    std::env::set_var(
        "LJOS_WAKE_SADDLE_URL",
        "https://api2.cursor.sh/automations/webhook/saddle",
    );
    std::env::set_var(
        "LJOS_WAKE_EVIL_URL",
        "http://localhost.attacker.invalid/hook",
    );

    let said = ljos_cli::cloud::up(false).unwrap();
    assert!(
        said.contains(
            "wake inky: posts to LJOS_WAKE_INKY_URL with the key in LJOS_WAKE_INKY_KEY\n"
        ),
        "{said}"
    );
    assert!(
        said.contains(
            "wake grok-bot: LJOS_WAKE_GROK-BOT_URL and LJOS_WAKE_GROK_BOT_URL name the same seat, so removed the stale wake cloud up wrote"
        ),
        "{said}"
    );
    assert!(
        said.contains(
            "wake saddle: left the wake cloud up did not write; LJOS_WAKE_SADDLE_URL did not replace it"
        ),
        "{said}"
    );
    assert!(
        said.contains(
            "wake evil: LJOS_WAKE_EVIL_URL is not https, nor http on this machine, so removed the stale wake cloud up wrote"
        ),
        "{said}"
    );
    assert!(
        said.contains(
            "wake retired: removed the stale wake cloud up wrote; no LJOS_WAKE_<SEAT>_URL names it now"
        ),
        "{said}"
    );
    assert!(!said.contains("localhost.attacker.invalid"), "{said}");
    assert!(!said.contains("inky-sender-key"), "{said}");
    assert!(!said.contains("api2.cursor.sh"), "{said}");
    let table = ljos_cli::wake::load();
    assert_eq!(table["inky"].webhook.as_deref(), Some(inky));
    assert_eq!(table["inky"].source.as_deref(), Some("env"));
    assert_eq!(table["inky"].key_env.as_deref(), Some("LJOS_WAKE_INKY_KEY"));
    assert_eq!(
        table["saddle"].webhook.as_deref(),
        Some("https://kept.example/hook")
    );
    assert_eq!(table["saddle"].key_env.as_deref(), Some("SADDLE_HAND_KEY"));
    assert!(table["saddle"].source.is_none(), "{table:?}");
    assert_eq!(table["claude-review"].command.as_deref(), Some("true"));
    assert!(!table.contains_key("grok-bot"), "{table:?}");
    assert!(!table.contains_key("evil"), "{table:?}");
    assert!(!table.contains_key("retired"), "{table:?}");
    let text = fs::read_to_string(&wakes).unwrap();
    assert!(!text.contains("inky-sender-key"), "{text}");
    ljos_cli::cloud::up(false).unwrap();
    assert_eq!(fs::read_to_string(&wakes).unwrap(), text);
}

#[test]
fn cloud_up_retires_env_wakes_even_when_cloud_age_fails() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let config = home.join(".config");
    let rel = root.path().join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(config.join("ljos")).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(
        rel.join("SHA256SUMS"),
        sums(&rel, &["ljos", "ljos-mcp", "ljos-policyd"]),
    )
    .unwrap();
    let wakes = config.join("ljos/wake.toml");
    fs::write(
        &wakes,
        "\
[inky]
webhook = \"https://old.example/inky\"
key_env = \"LJOS_WAKE_INKY_KEY\"
source = \"env\"

[saddle]
command = \"true\"
",
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", &config);
    std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("LJOS_WAKE_") {
            std::env::remove_var(&key);
        }
    }
    std::env::set_var("LJOS_WAKE_INKY_KEY", "rotated-key");
    std::env::set_var("LJOS_CLOUD_AGE_SECRET", "LJOS_FAKE_AGE_KEY");
    std::env::set_var("LJOS_FAKE_AGE_KEY", "definitely-not-an-age-identity");

    let err = ljos_cli::cloud::up(false).expect_err("bad cloud age must abort cloud up");
    let said = format!("{err:#}");
    assert!(said.contains("not an age identity"), "{said}");
    let table = ljos_cli::wake::load();
    assert!(!table.contains_key("inky"), "{table:?}");
    assert_eq!(table["saddle"].command.as_deref(), Some("true"));
    let text = fs::read_to_string(&wakes).unwrap();
    assert!(!text.contains("old.example/inky"), "{text}");
}

#[test]
fn cloud_up_refuses_to_overwrite_a_broken_wake_toml() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let config = home.join(".config");
    let rel = root.path().join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(config.join("ljos")).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(
        rel.join("SHA256SUMS"),
        sums(&rel, &["ljos", "ljos-mcp", "ljos-policyd"]),
    )
    .unwrap();
    let wakes = config.join("ljos/wake.toml");
    let broken = "[inky\nwebhook = \"https://old.example/hook\"\n";
    fs::write(&wakes, broken).unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", &config);
    std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("LJOS_WAKE_") {
            std::env::remove_var(&key);
        }
    }
    std::env::set_var(
        "LJOS_WAKE_INKY_URL",
        "https://api2.cursor.sh/automations/webhook/inky",
    );
    let err = ljos_cli::cloud::up(false).expect_err("broken wake.toml must abort cloud up");
    let said = format!("{err:#}");
    assert!(said.contains("wake"), "{said}");
    assert!(said.contains("parse"), "{said}");
    assert_eq!(fs::read_to_string(&wakes).unwrap(), broken);
}

/// A seat home in `root` with a release `ljos cloud up` can install, the
/// guard on, and git off the person's config. Returns the home.
fn seat_env(root: &Path) -> PathBuf {
    let home = root.join("home");
    let rel = root.join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(&home).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(
        rel.join("SHA256SUMS"),
        sums(&rel, &["ljos", "ljos-mcp", "ljos-policyd"]),
    )
    .unwrap();
    isolate_git(root);
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    std::env::set_var("LJOS_GUARD", "on");
    clear_agent_ids();
    home
}

/// A checkout at `root/name` on `develop` with one commit, an `origin`,
/// and each of `branches` beside it.
fn checkout(root: &Path, name: &str, branches: &[&str]) -> PathBuf {
    let dir = root.join(name);
    git(
        root,
        &["init", "-q", "-b", "develop", dir.to_str().unwrap()],
    );
    identity(&dir);
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "seed"]);
    git(
        &dir,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/ljos.git",
        ],
    );
    for branch in branches {
        git(&dir, &["branch", branch]);
    }
    dir
}

/// The cloud seat refusal the static guard gives `line` run in `cwd`. A
/// push gate ask on the same line is not one.
fn cloud_refusal(line: &str, cwd: &Path) -> Option<ljos_cli::Rule> {
    ljos_cli::static_guard_in(line, Some(cwd))
        .filter(|rule| rule.reason.starts_with("cloud seat rule"))
}

fn seat_rule(pattern: &str, reason: &str) -> ljos_cli::Rule {
    ljos_cli::Rule {
        pattern: pattern.to_string(),
        verdict: "deny".to_string(),
        reason: reason.to_string(),
    }
}

fn refused(line: &str, cwd: &Path) -> ljos_cli::Rule {
    let hit = cloud_refusal(line, cwd)
        .unwrap_or_else(|| panic!("`{line}` in {} was allowed", cwd.display()));
    assert_eq!(hit.verdict, "deny", "{line}: {hit:?}");
    hit
}

fn allowed(line: &str, cwd: &Path) {
    let hit = cloud_refusal(line, cwd);
    assert!(
        hit.is_none(),
        "`{line}` in {} was refused: {hit:?}",
        cwd.display()
    );
}

#[test]
fn cloud_up_installs_cloud_guard_refusals_idempotently() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = seat_env(root.path());

    let missing = ljos_cli::cloud::guard_row();
    assert_eq!(missing.name, "cloud guard");
    assert!(!missing.ok, "{missing:?}");
    assert!(missing.state.contains("run `ljos cloud up`"), "{missing:?}");

    let first = ljos_cli::cloud::up(false).unwrap();
    assert!(first.contains("guard:"), "{first}");
    let rules = home.join(".config/ljos/cloud-guard-rules.json");
    let before = fs::read_to_string(&rules).unwrap();
    assert!(!before.trim().is_empty(), "{before:?}");

    for denied in [
        "git push origin cursor/probe",
        "git push origin HEAD:cursor/probe",
        "gh pr create --fill",
        "gh pr merge 7",
    ] {
        let hit = ljos_cli::static_guard(denied).unwrap_or_else(|| panic!("{denied} was allowed"));
        assert_eq!(hit.verdict, "deny", "{denied}: {hit:?}");
        assert!(
            hit.reason.contains("cloud seat rule"),
            "{denied}: {:?}",
            hit.reason
        );
    }
    assert!(ljos_cli::static_guard("git push origin develop").is_none());

    let present = ljos_cli::cloud::guard_row();
    assert_eq!(present.name, "cloud guard");
    assert!(present.ok, "{present:?}");
    assert!(
        present.state.contains("denies cursor/* pushes"),
        "{present:?}"
    );

    let second = ljos_cli::cloud::up(false).unwrap();
    assert!(
        second.contains("already has cloud seat refusals"),
        "{second}"
    );
    assert_eq!(
        fs::read_to_string(&rules).unwrap(),
        before,
        "a second cloud up rewrote cloud guard rules"
    );
}

#[test]
fn a_machine_without_cloud_up_keeps_doctor_healthy() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = seat_env(root.path());
    let laptop = checkout(root.path(), "laptop", &["cursor/probe"]);
    git(&laptop, &["checkout", "-q", "cursor/probe"]);

    let row = ljos_cli::cloud::guard_row();
    assert_eq!(row.name, "cloud guard");
    assert!(!row.ok, "{row:?}");
    assert_eq!(ljos_cli::doctor_word(&row), "info", "{row:?}");
    let rows = vec![row];
    assert!(ljos_cli::failing(&rows).is_empty(), "{rows:?}");
    assert!(ljos_cli::healthy(&rows), "doctor would exit 1: {rows:?}");
    assert!(
        ljos_cli::doctor_summary(&rows).starts_with("ljos is ready"),
        "{}",
        ljos_cli::doctor_summary(&rows)
    );
    let short = ljos_cli::format_doctor_short(&rows);
    assert!(!short.contains("cloud guard"), "{short}");
    assert!(!home.join(".config/ljos/cloud-guard-rules.json").exists());

    for line in [
        "git push origin +cursor/probe",
        "git push",
        "gh pr create --fill",
        "gh pr merge 7",
    ] {
        allowed(line, &laptop);
    }
}

#[test]
fn cloud_guard_refuses_every_push_onto_a_cursor_ref() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let develop = checkout(root.path(), "develop", &["cursor/probe"]);
    let on_cursor = checkout(root.path(), "on-cursor", &[]);
    git(&on_cursor, &["checkout", "-q", "-b", "cursor/probe"]);

    for line in [
        "git push origin cursor/probe",
        "git push origin HEAD:cursor/probe",
        "git push origin +HEAD:cursor/probe",
        "git push origin refs/heads/cursor/probe",
        "git push origin +cursor/probe",
        "git push origin +refs/heads/cursor/probe",
        "git push --repo origin +cursor/probe",
        "git push --repo=origin +refs/heads/cursor/probe",
        "git push origin develop:cursor/probe",
        "git push origin develop:refs/heads/cursor/probe",
        "git push origin refs/tags/cursor/probe",
        "git push origin tag cursor/probe",
        "git push origin :cursor/probe",
        "git push origin --delete cursor/probe",
        "git push -o ci.skip origin cursor/probe",
        "git push origin 'refs/heads/*:refs/heads/*'",
        "git push origin \"$BRANCH\"",
        "git -c alias.p=push p origin cursor/probe",
        "git -c remote.origin.push=HEAD:cursor/probe push",
        "sh -c 'git push origin +cursor/probe'",
        "git status && git push origin +cursor/probe",
        "git push origin '+cursor/probe'",
        "\"git\" push origin \"+refs/heads/cursor/probe\"",
        "/usr/bin/git push origin +HEAD:cursor/probe",
        "FOO=1 git push origin +cursor/probe",
        "env GIT_TRACE=0 git push --repo=origin +refs/heads/cursor/probe",
        "git push --all",
        "git push --branches",
        "git push origin --all",
        "git push --mirror",
    ] {
        refused(line, &develop);
    }
    for line in [
        "git push",
        "git push origin",
        "git push origin HEAD",
        "git push origin @",
        "git push -u origin HEAD",
        "git push -u origin \"$(git branch --show-current)\"",
        "FOO=1 git push -u origin HEAD",
    ] {
        refused(line, &on_cursor);
    }
    let elsewhere = root.path();
    for line in [
        format!("git -C {} push origin +cursor/probe", develop.display()),
        format!("git -C '{}' push --all", develop.display()),
        format!("git -C {} push", on_cursor.display()),
        "git -C on-cursor push -u origin HEAD".to_string(),
        format!("cd {} && git push", on_cursor.display()),
        format!("FOO=1 git -C \"{}\" push origin HEAD", on_cursor.display()),
    ] {
        refused(&line, elsewhere);
    }

    let named = refused("git push origin +cursor/probe", &develop);
    assert_eq!(named.pattern, "cloud-cursor-push");
    assert!(
        named.reason.contains("this push updates cursor/probe"),
        "{}",
        named.reason
    );
    let implicit = refused("git push", &on_cursor);
    assert!(
        implicit
            .reason
            .contains("cursor/probe, the branch checked out"),
        "{}",
        implicit.reason
    );
    let all = refused("git push --all", &develop);
    assert!(
        all.reason
            .contains("`--all` pushes refs/heads/cursor/probe"),
        "{}",
        all.reason
    );
}

#[test]
fn cloud_guard_reads_valued_push_options_before_the_subcommand() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let origin = init_bare(root.path());
    let body = board(&[issue_block("probe-opts", "TODO", "", &[])]);
    seed(root.path(), &origin, &body);
    let repo = clone_of(root.path(), &origin, "probe");

    for line in [
        "git --attr-source HEAD push origin HEAD:cursor/attr",
        "git push --recurse-submodules check origin HEAD:cursor/submodule",
    ] {
        refused(line, &repo);
    }
}

#[test]
fn cloud_guard_denies_heads_and_tags_shortcuts_under_cursor() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let origin = init_bare(root.path());
    let body = board(&[issue_block("probe-short-refs", "TODO", "", &[])]);
    seed(root.path(), &origin, &body);
    let repo = clone_of(root.path(), &origin, "probe-short-refs");

    for line in [
        "git push origin HEAD:heads/cursor/existing",
        "git push origin HEAD:tags/cursor/probe-tag",
    ] {
        refused(line, &repo);
    }
}

#[test]
fn cloud_guard_reads_where_bare_refspecs_land() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let origin = init_bare(root.path());
    let body = board(&[issue_block("probe-bare-map", "TODO", "", &[])]);
    seed(root.path(), &origin, &body);
    let repo = clone_of(root.path(), &origin, "probe-bare-map");

    git(&repo, &["branch", "cursor/existing"]);
    git(
        &repo,
        &["push", "-q", "origin", "cursor/existing:cursor/existing"],
    );
    git(&repo, &["checkout", "-q", "-b", "fix"]);
    git(&repo, &["config", "push.default", "upstream"]);
    git(
        &repo,
        &[
            "branch",
            "--set-upstream-to",
            "origin/cursor/existing",
            "fix",
        ],
    );
    git(
        &repo,
        &[
            "config",
            "remote.origin.push",
            "refs/heads/main:refs/heads/cursor/mapped",
        ],
    );

    refused("git push origin fix", &repo);
    refused("git push origin main", &repo);
}

#[test]
fn cloud_guard_resolves_push_aliases_from_config_and_line_state() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let origin = init_bare(root.path());
    let body = board(&[issue_block("probe-alias", "TODO", "", &[])]);
    seed(root.path(), &origin, &body);
    let repo = clone_of(root.path(), &origin, "probe-alias");

    git(&repo, &["config", "--global", "alias.p", "push"]);

    for line in [
        "git p origin HEAD:cursor/alias-global",
        "git -c 'alias.p=!git push' p origin HEAD:cursor/alias-shell",
        "git config alias.p2 push && git p2 origin HEAD:cursor/alias-local",
    ] {
        refused(line, &repo);
    }
}

#[test]
fn cloud_guard_reads_git_env_and_env_chdir_context() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let origin = init_bare(root.path());
    let body = board(&[issue_block("probe-git-env", "TODO", "", &[])]);
    seed(root.path(), &origin, &body);
    let dev = clone_of(root.path(), &origin, "dev");
    let oncur = clone_of(root.path(), &origin, "oncur");
    git(&oncur, &["checkout", "-q", "-b", "cursor/probe"]);

    for line in [
        "GIT_DIR=../oncur/.git git push origin HEAD",
        "env -C ../oncur git push origin HEAD",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=remote.origin.push GIT_CONFIG_VALUE_0=HEAD:cursor/from-env git push origin HEAD",
    ] {
        refused(line, &dev);
    }

    allowed("GIT_DIR=../dev/.git git push origin HEAD", &oncur);
}

#[test]
fn cloud_guard_refuses_prune_and_mirror_when_they_can_delete_heads() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let origin = init_bare(root.path());
    let body = board(&[issue_block("probe-prune", "TODO", "", &[])]);
    seed(root.path(), &origin, &body);
    let repo = clone_of(root.path(), &origin, "probe-prune");

    refused("git push --prune origin 'refs/heads/*:refs/heads/*'", &repo);
    refused("git push --mirror origin", &repo);
}

#[test]
fn cloud_guard_refuses_shell_trick_push_forms() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let origin = init_bare(root.path());
    let body = board(&[issue_block("probe-shell-tricks", "TODO", "", &[])]);
    seed(root.path(), &origin, &body);
    let repo = clone_of(root.path(), &origin, "probe-shell-tricks");

    for line in [
        "git \"$(echo push)\" origin HEAD:cursor/subcommand",
        "git $'pu\\163h' origin HEAD:cursor/ansi-c",
        "printf 'HEAD:cursor/xargs\\n' | xargs git push origin",
        "git subtree push --prefix . origin cursor/subtree",
        "git send-pack origin HEAD:refs/heads/cursor/send-pack",
        "git submodule foreach 'git push origin HEAD:cursor/foreach'",
    ] {
        refused(line, &repo);
    }
}

#[test]
fn cloud_guard_reads_the_gh_pr_verb_and_never_its_arguments() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let here = root.path();

    for line in [
        "gh pr create --fill",
        "gh pr new --fill",
        "gh pr --repo owner/name create",
        "gh pr --repo=owner/name create",
        "gh pr -R owner/name create",
        "gh pr create --repo owner/name",
        "gh --repo owner/name pr create",
        "FOO=1 gh pr create",
        "'gh' pr \"create\" --fill",
        "git push origin develop && gh pr create --fill",
    ] {
        let hit = refused(line, here);
        assert_eq!(hit.pattern, "cloud-gh-pr-create", "{line}: {hit:?}");
        assert!(hit.reason.contains("never open pull requests"), "{hit:?}");
    }
    for line in [
        "gh pr merge 7",
        "gh pr --repo owner/name merge",
        "gh pr -R owner/name merge",
        "gh -R owner/name pr merge 7",
        "env GH_TOKEN=x gh pr --repo=owner/name merge 7",
        "/usr/local/bin/gh pr merge --squash 7",
        "sh -c 'gh pr merge 7'",
    ] {
        let hit = refused(line, here);
        assert_eq!(hit.pattern, "cloud-gh-pr-merge", "{line}: {hit:?}");
        assert!(hit.reason.contains("never merge pull requests"), "{hit:?}");
    }
    for line in [
        "gh pr comment --body 'please pr create a follow-up'",
        "gh pr comment -b 'do not pr merge yet'",
        "gh pr review 5 --comment 'LGTM, pr merge when green'",
        "gh help pr create",
        "gh pr list",
        "gh pr view 7 --repo owner/name",
        "gh pr -R owner/name checks 7",
        "gh issue create --title 'pr create'",
        "FOO=1 gh pr comment 7 --body \"pr merge\"",
        "echo gh pr create",
    ] {
        allowed(line, here);
    }
}

/// `gh api` and a gh alias open or merge a pull request without the words
/// `gh pr create` or `gh pr merge`. The bare remote is the repo those
/// commands are judged in.
#[test]
fn cloud_guard_refuses_gh_api_aliases_ready_and_reopen() {
    let _g = Guard::new();
    let gh_dir = gh_bin_dir();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    std::env::set_var("PATH", format!("{}:/usr/bin:/bin", gh_dir.display()));
    let cfg = root.path().join("ghcfg");
    let empty = root.path().join("empty-gh");
    fs::create_dir_all(&cfg).unwrap();
    fs::create_dir_all(&empty).unwrap();
    std::env::set_var("GH_CONFIG_DIR", &cfg);
    ljos_cli::cloud::up(false).unwrap();
    let (origin, repo) = bare_checkout(root.path());
    assert!(
        git(&origin, &["show-ref"]).contains("refs/heads/develop"),
        "the bare remote has no develop"
    );

    let set = |name: &str, expansion: &str| {
        let out = Command::new("gh")
            .args(["alias", "set", name, expansion])
            .output()
            .unwrap_or_else(|e| panic!("gh alias set {name}: {e}"));
        assert!(
            out.status.success(),
            "gh alias set {name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    set("pc", "pr create --fill");
    set("ship", "pr merge --squash");
    set(
        "mkpr",
        "api repos/acme/widgets/pulls -f title=t -f head=feature-x -f base=develop",
    );
    set("bang", "!gh pr reopen 3");

    let here = repo.as_path();
    let open_query = "gh api graphql -f query='mutation { createPullRequest(input: {repositoryId: \"R\"}) { clientMutationId } }'";
    let merge_query = "gh api graphql -f query='mutation { mergePullRequest(input: {pullRequestId: \"P\"}) { clientMutationId } }'";
    let reopen_query = "gh api graphql -f query='mutation { reopenPullRequest(input: {pullRequestId: \"P\"}) { clientMutationId } }'";
    let ready_query = "gh api graphql -f query='mutation { markPullRequestReadyForReview(input: {pullRequestId: \"P\"}) { clientMutationId } }'";
    let auto_query = "gh api graphql -f query='mutation { enablePullRequestAutoMerge(input: {pullRequestId: \"P\"}) { clientMutationId } }'";
    for line in [
        "gh pr ready 7",
        "gh pr reopen 7",
        "gh pr --repo acme/widgets ready 4",
        "gh pc --fill",
        "gh ship 9",
        "gh bang",
        "gh mkpr",
        "gh api repos/acme/widgets/pulls -f title=t -f head=feature-x -f base=develop",
        "gh api --method POST /repos/acme/widgets/pulls -f title=t -f head=feature-x -f base=develop",
        "gh api --method PUT repos/acme/widgets/pulls/7/merge",
        "gh api repos/acme/widgets/pulls/7/merge -f merge_method=squash",
        "gh api -X PUT https://api.github.com/repos/acme/widgets/pulls/7/merge",
        open_query,
        merge_query,
        reopen_query,
        ready_query,
        auto_query,
    ] {
        let hit = refused(line, here);
        assert!(
            hit.pattern == "cloud-gh-pr-create" || hit.pattern == "cloud-gh-pr-merge",
            "{line}: {hit:?}"
        );
    }
    assert_eq!(refused("gh pr ready 7", here).pattern, "cloud-gh-pr-create",);
    assert_eq!(
        refused("gh pr reopen 7", here).pattern,
        "cloud-gh-pr-create",
    );
    assert_eq!(
        refused("gh api --method PUT repos/acme/widgets/pulls/7/merge", here).pattern,
        "cloud-gh-pr-merge",
    );
    assert_eq!(refused(open_query, here).pattern, "cloud-gh-pr-create");
    assert_eq!(refused(merge_query, here).pattern, "cloud-gh-pr-merge");
    assert_eq!(refused(auto_query, here).pattern, "cloud-gh-pr-merge");

    fs::write(
        repo.join("open.graphql"),
        "mutation { createPullRequest(input: {}) { clientMutationId } }\n",
    )
    .unwrap();
    fs::write(repo.join("read.graphql"), "query { viewer { login } }\n").unwrap();
    assert_eq!(
        refused("gh api graphql -F query=@open.graphql", here).pattern,
        "cloud-gh-pr-create"
    );

    for line in [
        "gh api repos/acme/widgets/pulls",
        "gh api repos/acme/widgets/pulls/7",
        "gh api --method GET repos/acme/widgets/pulls/7/merge",
        "gh api -X GET repos/acme/widgets/pulls -f per_page=5",
        "gh api repos/acme/widgets/issues -f title=t -f body=b",
        "gh api graphql -f query='query { viewer { login } }'",
        "gh api graphql -F query=@read.graphql",
        "gh pr comment --body 'please pr create a follow-up'",
        "gh pr list",
        "gh pr checkout 7",
    ] {
        allowed(line, here);
    }

    // The alias is in `cfg`, not in the process config. The guard has to
    // read the directory the command itself names.
    std::env::set_var("GH_CONFIG_DIR", &empty);
    allowed("gh pc --fill", here);
    let prefixed = format!("GH_CONFIG_DIR={} gh pc --fill", cfg.display());
    assert_eq!(refused(&prefixed, here).pattern, "cloud-gh-pr-create");
}

fn gh_bin_dir() -> PathBuf {
    let out = Command::new("sh")
        .arg("-c")
        .arg("command -v gh")
        .output()
        .unwrap();
    assert!(out.status.success(), "gh is not on PATH");
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
        .parent()
        .unwrap()
        .to_path_buf()
}

/// A clone of a local bare remote on `develop`, with `feature-x` and
/// `cursor/probe` beside it.
fn bare_checkout(root: &Path) -> (PathBuf, PathBuf) {
    let origin = root.join("origin.git");
    git(
        root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "develop",
            origin.to_str().unwrap(),
        ],
    );
    let repo = root.join("repo");
    git(
        root,
        &["init", "-q", "-b", "develop", repo.to_str().unwrap()],
    );
    identity(&repo);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "seed"]);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "-u", "origin", "develop"]);
    git(&repo, &["branch", "feature-x"]);
    git(&repo, &["branch", "cursor/probe"]);
    (origin, repo)
}

/// An earlier assignment, `cd` to the checkout top, and `git push --tags`
/// do not update a `cursor/*` ref. The bare remote is where those commands
/// actually push.
#[test]
fn cloud_guard_allows_resolved_destinations_toplevel_and_tags() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let (origin, repo) = bare_checkout(root.path());
    let sub = repo.join("sub");
    fs::create_dir(&sub).unwrap();
    let remote_has = |needle: &str| {
        git(&origin, &["show-ref"])
            .lines()
            .any(|l| l.ends_with(needle))
    };

    allowed(r#"B=feature-x; git push origin "$B""#, &repo);
    sh_ok(&repo, r#"B=feature-x; git push origin "$B""#);
    assert!(
        remote_has("refs/heads/feature-x"),
        "feature-x was not pushed"
    );
    assert!(
        !remote_has("refs/heads/cursor/probe"),
        "the assignment pushed cursor/probe"
    );
    allowed(r#"export B=feature-x; git push origin "${B}""#, &repo);
    allowed(r#"sh -c 'B=feature-x; git push origin "$B"'"#, &repo);
    allowed(r#"B=feature-x; git push origin "$B":develop"#, &repo);
    allowed(r#"git push origin "$(echo HEAD):develop""#, &repo);

    let onto = refused(r#"B=cursor/probe; git push origin "$B""#, &repo);
    assert!(
        onto.reason.contains("this push updates cursor/probe"),
        "{}",
        onto.reason
    );
    let unresolved = refused(r#"git push origin "$B""#, &repo);
    assert!(
        unresolved.reason.contains("`$B` is only known"),
        "{}",
        unresolved.reason
    );
    let named = refused(r#"git push origin "$(echo HEAD:cursor/x)""#, &repo);
    assert!(
        named
            .reason
            .contains("`$(echo HEAD:cursor/x)` is only known"),
        "{}",
        named.reason
    );
    assert!(
        !named.reason.contains("`cursor/x)` is only known"),
        "{}",
        named.reason
    );

    let up = r#"cd "$(git rev-parse --show-toplevel)" && git push -u origin HEAD"#;
    let up_tick = "cd `git rev-parse --show-toplevel` && git push -u origin HEAD";
    allowed(up, &sub);
    allowed(up_tick, &sub);
    sh_ok(&sub, up);
    assert!(!remote_has("refs/heads/cursor/probe"));

    git(&repo, &["checkout", "-q", "cursor/probe"]);
    let hit = refused(up, &sub);
    assert!(
        hit.reason.contains("cursor/probe, the branch checked out"),
        "{}",
        hit.reason
    );
    let dry = sh_at(
        &sub,
        r#"cd "$(git rev-parse --show-toplevel)" && git push --dry-run -u origin HEAD"#,
    );
    let dry_text = format!(
        "{}{}",
        String::from_utf8_lossy(&dry.stdout),
        String::from_utf8_lossy(&dry.stderr)
    );
    assert!(
        dry.status.success() && dry_text.contains("cursor/probe"),
        "{dry_text}"
    );
    assert!(!remote_has("refs/heads/cursor/probe"));

    git(&repo, &["tag", "v1"]);
    allowed("git push --tags", &repo);
    sh_ok(&repo, "git push --tags");
    assert!(remote_has("refs/tags/v1"));
    assert!(
        !remote_has("refs/heads/cursor/probe"),
        "--tags pushed the branch"
    );
    let followed = refused("git push --follow-tags", &repo);
    assert!(
        followed
            .reason
            .contains("cursor/probe, the branch checked out"),
        "{}",
        followed.reason
    );
    refused("git push --tags --follow-tags", &repo);

    git(&repo, &["tag", "cursor/mark"]);
    let tagged = refused("git push --tags", &repo);
    assert!(
        tagged
            .reason
            .contains("`--tags` pushes refs/tags/cursor/mark"),
        "{}",
        tagged.reason
    );
    let tag_dry = sh_at(&repo, "git push --dry-run --tags");
    let tag_text = format!(
        "{}{}",
        String::from_utf8_lossy(&tag_dry.stdout),
        String::from_utf8_lossy(&tag_dry.stderr)
    );
    assert!(
        tag_dry.status.success() && tag_text.contains("cursor/mark"),
        "{tag_text}"
    );
    assert!(!remote_has("refs/tags/cursor/mark"));
}

/// The rule file an older `ljos cloud up` wrote: a push regex and two gh
/// globs. Reason text differs from the current file, and the order does too.
fn older_rules() -> Vec<ljos_cli::Rule> {
    let push = "re:git push(?:\\s+\\S+)*\\s(?:\\S*:)?(?:refs/heads/)?cursor/\\S+(?:\\s|$)";
    vec![
        seat_rule(
            "gh* pr merge*",
            "cloud seat rule: merge refusal, older wording",
        ),
        seat_rule(push, "cloud seat rule: push refusal, older wording"),
        seat_rule(
            "gh* pr create*",
            "cloud seat rule: open refusal, older wording",
        ),
    ]
}

/// An older rule file is still a cloud seat. Its patterns are judged as the
/// current refusals, doctor warns, and `--fix` replaces that file. The bare
/// remote is what the old regex let a push update.
#[test]
fn cloud_guard_reads_an_older_rule_file_as_the_current_refusals() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = seat_env(root.path());
    let (origin, repo) = bare_checkout(root.path());
    let rules = home.join(".config/ljos/cloud-guard-rules.json");
    ljos_cli::keep_rules(&rules, &older_rules());

    assert!(
        ljos_cli::cloud::cloud_seat(),
        "a known older file is a cloud seat"
    );
    let row = ljos_cli::cloud::guard_row();
    assert!(!row.ok, "{row:?}");
    assert!(row.state.contains("is stale; ljos doctor --fix"), "{row:?}");
    assert_eq!(ljos_cli::doctor_word(&row), "warn", "{row:?}");
    assert!(ljos_cli::healthy(std::slice::from_ref(&row)), "{row:?}");
    assert!(
        ljos_cli::format_doctor_short(std::slice::from_ref(&row)).contains("cloud guard"),
        "{}",
        ljos_cli::format_doctor_short(std::slice::from_ref(&row))
    );

    let pushed = refused("git push origin +cursor/probe", &repo);
    assert!(
        pushed.reason.contains("this push updates cursor/probe"),
        "{}",
        pushed.reason
    );
    assert!(pushed.reason.contains("older wording"), "{}", pushed.reason);
    let dry = sh_at(&repo, "git push --dry-run origin +cursor/probe");
    let dry_text = format!(
        "{}{}",
        String::from_utf8_lossy(&dry.stdout),
        String::from_utf8_lossy(&dry.stderr)
    );
    assert!(
        dry.status.success() && dry_text.contains("cursor/probe"),
        "{dry_text}"
    );
    assert!(
        !git(&origin, &["show-ref"]).contains("refs/heads/cursor/probe"),
        "the refused push updated the remote"
    );
    allowed("git push origin cursor/probe:develop", &repo);
    sh_ok(&repo, "git push origin cursor/probe:develop");
    assert!(!git(&origin, &["show-ref"]).contains("refs/heads/cursor/probe"));
    allowed("git push git@github.com:cursor/ljos.git develop", &repo);
    allowed("gh pr comment --body 'please pr create a follow-up'", &repo);
    let opened = refused("gh pr -R owner/name create", &repo);
    assert_eq!(opened.pattern, "gh* pr create*");
    assert!(opened.reason.contains("older wording"), "{opened:?}");
    assert_eq!(
        refused("gh pr -R owner/name merge 7", &repo).pattern,
        "gh* pr merge*"
    );

    let said = ljos_cli::cloud::up(false).unwrap();
    assert!(said.contains("now denies"), "{said}");
    let canonical = fs::read_to_string(&rules).unwrap();
    let present = ljos_cli::cloud::guard_row();
    assert!(present.ok, "{present:?}");
    assert!(
        present.state.contains("denies cursor/* pushes"),
        "{present:?}"
    );
    let again = ljos_cli::cloud::up(false).unwrap();
    assert!(again.contains("already has cloud seat refusals"), "{again}");
    assert_eq!(fs::read_to_string(&rules).unwrap(), canonical);

    ljos_cli::keep_rules(&rules, &older_rules());
    ljos_cli::cloud::replace_stale_guard();
    assert_eq!(fs::read_to_string(&rules).unwrap(), canonical);
    assert!(ljos_cli::cloud::guard_row().ok);

    ljos_cli::keep_rules(
        &rules,
        &[
            seat_rule("cloud-cursor-push", "cloud seat rule: push"),
            seat_rule("gh* pr create*", "cloud seat rule: open"),
            seat_rule("gh* pr merge*", "cloud seat rule: merge"),
        ],
    );
    assert!(ljos_cli::cloud::cloud_seat());
    assert!(
        ljos_cli::cloud::guard_row().state.contains("is stale;"),
        "{:?}",
        ljos_cli::cloud::guard_row()
    );
    ljos_cli::cloud::replace_stale_guard();
    assert_eq!(fs::read_to_string(&rules).unwrap(), canonical);

    ljos_cli::keep_rules(
        &rules,
        &[
            seat_rule("cloud-gh-pr-merge", "cloud seat rule: still a merge"),
            seat_rule("cloud-cursor-push", "cloud seat rule: still a push"),
            seat_rule("cloud-gh-pr-create", "cloud seat rule: still an open"),
        ],
    );
    let reworded = fs::read_to_string(&rules).unwrap();
    assert!(ljos_cli::cloud::cloud_seat());
    let same = ljos_cli::cloud::guard_row();
    assert!(same.ok, "{same:?}");
    assert_eq!(ljos_cli::doctor_word(&same), "ok");
    ljos_cli::cloud::replace_stale_guard();
    assert_eq!(fs::read_to_string(&rules).unwrap(), reworded);

    fs::remove_file(&rules).unwrap();
    ljos_cli::cloud::replace_stale_guard();
    assert!(
        !rules.exists(),
        "doctor --fix installed a missing guard file"
    );
    let missing = ljos_cli::cloud::guard_row();
    assert!(missing.state.contains("run `ljos cloud up`"), "{missing:?}");
    assert_eq!(ljos_cli::doctor_word(&missing), "info");
    assert!(!ljos_cli::cloud::cloud_seat());

    ljos_cli::keep_rules(
        &rules,
        &[seat_rule("git push*", "cloud seat rule: hand edit")],
    );
    let hand = fs::read_to_string(&rules).unwrap();
    let edited = ljos_cli::cloud::guard_row();
    assert!(
        edited
            .state
            .contains("differs from the cloud seat refusals"),
        "{edited:?}"
    );
    assert_eq!(ljos_cli::doctor_word(&edited), "warn");
    assert!(!ljos_cli::cloud::cloud_seat());
    ljos_cli::cloud::replace_stale_guard();
    assert_eq!(fs::read_to_string(&rules).unwrap(), hand);
}

#[test]
fn cloud_guard_allows_pushes_that_leave_cursor_refs_alone() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    seat_env(root.path());
    ljos_cli::cloud::up(false).unwrap();
    let develop = checkout(root.path(), "develop", &["cursor/probe"]);
    let clean = checkout(root.path(), "clean", &["feature/x"]);

    for line in [
        "git push origin develop",
        "git push origin cursor/probe:develop",
        "git push origin refs/heads/cursor/probe:refs/heads/develop",
        "git push origin +cursor/probe:develop",
        "git push git@github.com:cursor/ljos.git develop",
        "git push --repo git@github.com:cursor/ljos.git develop",
        "git push --repo=git@github.com:cursor/ljos.git develop",
        "git push --repo cursor/local develop",
        "git push -o cursor/skip origin develop",
        "git push --push-option cursor/skip origin develop",
        "git push --receive-pack cursor/helper origin develop",
        "git push --exec cursor/helper origin develop",
        "git push",
        "git push origin",
        "git push origin HEAD",
        "git push -u origin HEAD",
        "git push -u origin \"$(git branch --show-current)\"",
        "git push origin develop 2>&1",
        "git push origin develop > cursor/push.log",
        "git push origin 'refs/heads/feature/*:refs/heads/feature/*'",
        "echo 'git push origin cursor/probe'",
        "git log --oneline cursor/probe",
        "git push origin 'cursor/probe:develop'",
        "\"git\" push origin \"cursor/probe:refs/heads/develop\"",
        "FOO=1 git push origin cursor/probe:develop",
        "env GIT_TRACE=0 git push --repo git@github.com:cursor/ljos.git develop",
    ] {
        allowed(line, &develop);
    }
    for line in ["git push --all", "git push --branches", "git push --tags"] {
        allowed(line, &clean);
    }
    let elsewhere = root.path();
    for line in [
        format!(
            "git -C {} push origin cursor/probe:develop",
            develop.display()
        ),
        format!("git -C '{}' push", develop.display()),
        "git -C clean push --all".to_string(),
        format!("cd {} && git push -u origin HEAD", develop.display()),
        format!("FOO=1 git -C \"{}\" push origin HEAD", develop.display()),
    ] {
        allowed(&line, elsewhere);
    }
}

#[test]
fn cloud_up_writes_the_scope_cloud_identity_from_a_named_secret() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let rel = root.path().join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(&home).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(
        rel.join("SHA256SUMS"),
        sums(&rel, &["ljos", "ljos-mcp", "ljos-policyd"]),
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    std::env::remove_var("LJOS_CLOUD_AGE_SECRET");
    std::env::remove_var("LJOS_CLOUD_AGE_KEY");

    let bare = ljos_cli::cloud::up(false).unwrap();
    assert!(bare.contains("LJOS_CLOUD_AGE_SECRET is unset"), "{bare}");
    let path = ljos_cli::sync::cloud_identity_path();
    assert!(
        path.starts_with(&home),
        "identity left the test home: {}",
        path.display()
    );
    assert!(
        !path.exists(),
        "an unset secret still wrote {}",
        path.display()
    );
    let unset = ljos_cli::cloud::cloud_age_row();
    assert_eq!(ljos_cli::doctor_word(&unset), "info");
    assert!(ljos_cli::failing(std::slice::from_ref(&unset)).is_empty());

    let made = Command::new("age-keygen")
        .arg("-o")
        .arg(root.path().join("id.txt"))
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "age-keygen must be installed: {}",
        String::from_utf8_lossy(&made.stderr)
    );
    let identity = fs::read_to_string(root.path().join("id.txt")).unwrap();
    let secret_line = identity
        .lines()
        .find(|line| line.starts_with("AGE-SECRET-KEY-"))
        .unwrap()
        .to_string();
    let public = Command::new("age-keygen")
        .arg("-y")
        .arg(root.path().join("id.txt"))
        .output()
        .unwrap();
    assert!(public.status.success());
    let public = String::from_utf8_lossy(&public.stdout).trim().to_string();

    std::env::set_var("LJOS_CLOUD_AGE_SECRET", &identity);
    let pasted = ljos_cli::cloud::up(false).unwrap_err().to_string();
    assert!(pasted.contains("holds an age identity"), "{pasted}");
    assert!(!pasted.contains(&secret_line), "{pasted}");
    assert!(!path.exists());

    std::env::set_var("LJOS_CLOUD_AGE_SECRET", "LJOS_CLOUD_AGE_KEY");
    let missing = ljos_cli::cloud::up(false).unwrap_err().to_string();
    assert!(missing.contains("LJOS_CLOUD_AGE_KEY is unset"), "{missing}");
    let row = ljos_cli::cloud::cloud_age_row();
    assert!(!row.ok, "{row:?}");
    assert_eq!(ljos_cli::doctor_word(&row), "no");
    assert!(row.state.contains("LJOS_CLOUD_AGE_KEY"), "{row:?}");
    assert!(!row.state.contains(&secret_line), "{row:?}");
    assert_eq!(
        ljos_cli::failing(std::slice::from_ref(&row)),
        vec!["cloud age"]
    );

    std::env::set_var("LJOS_CLOUD_AGE_KEY", "nope");
    let bad = ljos_cli::cloud::up(false).unwrap_err().to_string();
    assert!(bad.contains("is not an age identity"), "{bad}");
    assert!(!path.exists());

    let outside = root.path().join("outside");
    fs::write(&outside, b"keep\n").unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    std::env::set_var("LJOS_CLOUD_AGE_KEY", &identity);
    let linked = ljos_cli::cloud::up(false).unwrap_err().to_string();
    assert!(linked.contains("is a symlink"), "{linked}");
    assert!(!linked.contains(&secret_line), "{linked}");
    assert_eq!(fs::read(&outside).unwrap(), b"keep\n");
    assert!(fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_file(&path).unwrap();
    let wrote = ljos_cli::cloud::up(false).unwrap();
    assert!(wrote.contains("wrote"), "{wrote}");
    assert!(wrote.contains("0600"), "{wrote}");
    assert!(!wrote.contains(&secret_line), "{wrote}");
    assert_eq!(fs::read(&outside).unwrap(), b"keep\n");
    let meta = fs::symlink_metadata(&path).unwrap();
    assert!(
        meta.file_type().is_file(),
        "cloud.age was not installed as a file"
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    let stored = fs::read_to_string(&path).unwrap();
    assert!(stored.contains(&secret_line));
    assert!(!home.join(".config/ljos/age.key").exists());
    let env = fs::read_to_string(home.join(".config/ljos/env")).unwrap();
    let mcp = fs::read_to_string(home.join(".cursor/mcp.json")).unwrap();
    assert!(!env.contains("AGE-SECRET-KEY-"), "{env}");
    assert!(!mcp.contains("AGE-SECRET-KEY-"), "{mcp}");
    assert!(!env.contains("cloud.age"), "{env}");

    let sealed = root.path().join("msg.age");
    let mut child = Command::new("age")
        .args(["-r", &public, "-o"])
        .arg(&sealed)
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"scope cloud\n")
        .unwrap();
    let done = child.wait_with_output().unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let opened = Command::new("age")
        .args(["-d", "-i"])
        .arg(&path)
        .arg(&sealed)
        .output()
        .unwrap();
    assert!(
        opened.status.success(),
        "{}",
        String::from_utf8_lossy(&opened.stderr)
    );
    assert_eq!(opened.stdout, b"scope cloud\n");

    let good = ljos_cli::cloud::cloud_age_row();
    assert!(good.ok, "{good:?}");
    assert_eq!(ljos_cli::doctor_word(&good), "ok");
    assert!(good.state.contains("0600"), "{good:?}");
    assert!(!good.state.contains(&secret_line), "{good:?}");
    assert!(ljos_cli::failing(&[good]).is_empty());

    let bytes = fs::read(&path).unwrap();
    let again = ljos_cli::cloud::up(false).unwrap();
    assert!(again.contains("already holds"), "{again}");
    assert_eq!(fs::read(&path).unwrap(), bytes);

    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let loose = ljos_cli::cloud::cloud_age_row();
    assert!(!loose.ok, "{loose:?}");
    assert_eq!(ljos_cli::doctor_word(&loose), "no");
    assert!(loose.state.contains("0644"), "{loose:?}");
    assert_eq!(ljos_cli::failing(&[loose]), vec!["cloud age"]);
    let fixed = ljos_cli::cloud::up(false).unwrap();
    assert!(fixed.contains("mode 0600"), "{fixed}");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        fs::symlink_metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    std::env::set_var("LJOS_CLOUD_AGE_KEY", "nope");
    let refused = ljos_cli::cloud::up(false).unwrap_err().to_string();
    assert!(refused.contains("is not an age identity"), "{refused}");
    assert_eq!(fs::read(&path).unwrap(), bytes);

    std::env::remove_var("LJOS_CLOUD_AGE_SECRET");
    std::env::remove_var("LJOS_CLOUD_AGE_KEY");
    let left = ljos_cli::cloud::up(false).unwrap();
    assert!(left.contains("is unset"), "{left}");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let still = ljos_cli::cloud::cloud_age_row();
    assert!(still.ok, "{still:?}");
    assert_eq!(ljos_cli::doctor_word(&still), "ok");
}

#[test]
fn two_vms_land_different_issues_and_merge_notes_on_one() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    isolate_git(root.path());
    std::env::set_var("LJOS_TRACKER_GIT", "push");
    std::env::set_var("LJOS_ORG_DRIVER", "false");
    std::env::set_var("XDG_RUNTIME_DIR", root.path().join("run"));
    let origin = init_bare(root.path());
    seed(
        root.path(),
        &origin,
        &board(&[issue_block(
            "probe-note",
            "TODO",
            "",
            &["- Note: \"base\" [2026-01-01 Thu 00:00]"],
        )]),
    );

    let a = clone_of(root.path(), &origin, "vm-a");
    let b = clone_of(root.path(), &origin, "vm-b");
    let alpha = write_project(
        &a,
        "alpha",
        &board(&[issue_block("alpha-aaaa", "TODO", "", &[])]),
    );
    let beta = write_project(
        &b,
        "beta",
        &board(&[issue_block("beta-bbbb", "TODO", "", &[])]),
    );
    let left =
        std::thread::spawn(move || ljos_cli::persist_tracker_file(&alpha, "alpha-aaaa", "noted"));
    let right =
        std::thread::spawn(move || ljos_cli::persist_tracker_file(&beta, "beta-bbbb", "noted"));
    let left = left.join().unwrap();
    let right = right.join().unwrap();
    assert!(left.contains("pushed"), "{left}");
    assert!(right.contains("pushed"), "{right}");
    let remote_alpha = show(&origin, "Software/alpha/issues.org");
    let remote_beta = show(&origin, "Software/beta/issues.org");
    assert!(
        remote_alpha.contains(":ID:       alpha-aaaa"),
        "{remote_alpha}"
    );
    assert!(
        remote_beta.contains(":ID:       beta-bbbb"),
        "{remote_beta}"
    );

    let a = clone_of(root.path(), &origin, "vm-c");
    let b = clone_of(root.path(), &origin, "vm-d");
    let note_a = a.join("Software/probe/issues.org");
    let note_b = b.join("Software/probe/issues.org");
    let add = |path: &Path, line: &str| {
        let text = fs::read_to_string(path).unwrap();
        let next = text.replacen(":LOGBOOK:\n", &format!(":LOGBOOK:\n{line}\n"), 1);
        fs::write(path, next).unwrap();
    };
    add(&note_a, "- Note: \"from vm-a only\" [2026-02-01 Sun 01:00]");
    add(&note_b, "- Note: \"from vm-b only\" [2026-02-01 Sun 02:00]");
    let left =
        std::thread::spawn(move || ljos_cli::persist_tracker_file(&note_a, "probe-note", "noted"));
    let right =
        std::thread::spawn(move || ljos_cli::persist_tracker_file(&note_b, "probe-note", "noted"));
    let left = left.join().unwrap();
    let right = right.join().unwrap();
    assert!(
        left.contains("pushed") && right.contains("pushed"),
        "a: {left}\nb: {right}"
    );
    let merged = show(&origin, "Software/probe/issues.org");
    assert!(merged.contains("from vm-a only"), "{merged}");
    assert!(merged.contains("from vm-b only"), "{merged}");
    assert!(merged.contains("base"), "{merged}");
    assert_eq!(merged.matches("* TODO").count(), 1, "{merged}");
    assert!(!merged.contains("<<<<<<<"), "{merged}");
}

#[test]
fn a_refused_push_stays_queued_until_the_next_drain() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    isolate_git(root.path());
    let origin = init_bare(root.path());
    seed(
        root.path(),
        &origin,
        &board(&[issue_block(
            "probe-queue",
            "TODO",
            "",
            &["- Note: \"base\" [2026-01-01 Thu 00:00]"],
        )]),
    );
    let hook = origin.join("hooks/pre-receive");
    executable(&hook, "#!/bin/sh\nexit 1\n");
    let vm = clone_of(root.path(), &origin, "vm");
    let issues = vm.join("Software/probe/issues.org");
    let text = fs::read_to_string(&issues).unwrap();
    fs::write(
        &issues,
        text.replacen(
            ":LOGBOOK:\n",
            ":LOGBOOK:\n- Note: \"queued on the vm\" [2026-03-01 Sun 03:00]\n",
            1,
        ),
    )
    .unwrap();
    let home = root.path().join("home");
    let runtime = root.path().join("run");
    let cache = root.path().join("cache");
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    fs::create_dir_all(cache.join("ljos")).unwrap();
    for name in [
        "ljos",
        "ljos-policyd",
        "ljos-consensus",
        "vissue-cli",
        "vissue-mcp",
        "deedar-cli",
        "claimdag-cli",
        "packset",
        "packset-embed",
        "ljos-hud",
    ] {
        fs::write(cache.join("ljos").join(format!("crate-{name}")), "9.9.9\n").unwrap();
    }
    fs::write(
        home.join(".cursor/mcp.json"),
        "{\"mcpServers\":{\"ljos\":{\"command\":\"/no/such/ljos-mcp\",\"args\":[]}}}\n",
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_RUNTIME_DIR", &runtime);
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("LJOS_TRACKER_GIT", "push");
    std::env::set_var("VISSUE_ROOT", &vm);
    std::env::set_var("ISSUE_ROOT", &vm);
    std::env::set_var("LJOS_TRACKER_PUSH_WAIT", "0");
    std::env::set_var("PACKSET_URL", "off");
    let said = ljos_cli::persist_tracker_file(&issues, "probe-queue", "noted");
    assert!(said.contains("push queued"), "{said}");
    assert!(
        !show(&origin, "Software/probe/issues.org").contains("queued on the vm"),
        "a refused push still updated the remote"
    );

    let doctor = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("doctor")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CACHE_HOME", &cache)
        .env("PATH", "/usr/bin:/bin")
        .env("VISSUE_ROOT", &vm)
        .env("ISSUE_ROOT", &vm)
        .env("LJOS_TRACKER_GIT", "push")
        .env("LJOS_TRACKER_PUSH_WAIT", "0")
        .env("PACKSET_URL", "off")
        .env("GIT_CONFIG_GLOBAL", root.path().join("gitconfig"))
        .env("GIT_CONFIG_SYSTEM", root.path().join("gitconfig"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&doctor.stdout);
    assert!(
        stdout.contains("unpushed"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    assert!(stdout.contains("does not exist"), "{stdout}");
    assert!(!doctor.status.success(), "doctor exited 0:\n{stdout}");

    fs::remove_file(&hook).unwrap();
    let drained = ljos_cli::drain_tracker_pushes();
    assert!(drained.contains("pushed"), "{drained}");
    let remote = show(&origin, "Software/probe/issues.org");
    assert!(remote.contains("queued on the vm"), "{remote}");
}

#[test]
fn a_gone_vm_does_not_keep_a_started_claim() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    isolate_git(root.path());
    let origin = init_bare(root.path());
    let fresh = Command::new("date")
        .args(["+%Y-%m-%d %a %H:%M"])
        .output()
        .unwrap();
    let fresh = format!("[{}]", String::from_utf8_lossy(&fresh.stdout).trim());
    let old = ":CLAIMED_BY: vm-dead\n:CLAIMED_AT: [2020-01-01 Wed 00:00]\n";
    let now = format!(":CLAIMED_BY: vm-live\n:CLAIMED_AT: {fresh}\n");
    let kept = ":CLAIMED_BY: vm-kept\n:CLAIMED_AT: [2020-01-01 Wed 00:00]\n";
    seed(
        root.path(),
        &origin,
        &board(&[
            issue_block("probe-old1", "STARTED", old, &[]),
            issue_block("probe-new1", "STARTED", &now, &[]),
            issue_block("probe-kept", "STARTED", kept, &[]),
        ]),
    );
    let vm = clone_of(root.path(), &origin, "vm");
    let runtime = root.path().join("run");
    fs::create_dir_all(runtime.join("ljos")).unwrap();
    let stat = fs::read_to_string("/proc/self/stat").unwrap();
    let open = stat.find('(').unwrap();
    let close = stat.rfind(')').unwrap();
    let comm = &stat[open + 1..close];
    fs::write(
        runtime.join("ljos").join("hold-vm-kept"),
        format!(
            "vm-kept\nseat\n{}\n{comm}\n2020-01-01T00:00:00Z\nprobe-kept\n-\n",
            std::process::id()
        ),
    )
    .unwrap();
    std::env::set_var("HOME", root.path().join("home"));
    std::env::set_var("XDG_RUNTIME_DIR", &runtime);
    std::env::set_var("LJOS_TRACKER_GIT", "push");
    std::env::set_var("VISSUE_ROOT", &vm);
    std::env::set_var("ISSUE_ROOT", &vm);
    let said = ljos_cli::cloud::reap_gone_owners(900);
    assert!(said.contains("probe-old1"), "{said}");
    let local = fs::read_to_string(vm.join("Software/probe/issues.org")).unwrap();
    assert_eq!(state_of(&local, "probe-old1"), "TODO", "{local}");
    assert_eq!(state_of(&local, "probe-new1"), "STARTED", "{local}");
    assert_eq!(state_of(&local, "probe-kept"), "STARTED", "{local}");
    let remote = show(&origin, "Software/probe/issues.org");
    assert_eq!(state_of(&remote, "probe-old1"), "TODO", "{remote}");
}

#[test]
fn a_live_hold_keeps_its_issue_and_a_dead_process_is_released() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    isolate_git(root.path());
    let origin = init_bare(root.path());
    let old = ":CLAIMED_BY: seat-live\n:CLAIMED_AT: [2020-01-01 Wed 00:00]\n";
    let dead = ":CLAIMED_BY: seat-dead\n:CLAIMED_AT: [2020-01-01 Wed 00:00]\n";
    seed(
        root.path(),
        &origin,
        &board(&[
            issue_block("probe-live", "STARTED", old, &[]),
            issue_block("probe-other", "STARTED", old, &[]),
            issue_block("probe-dead", "STARTED", dead, &[]),
        ]),
    );
    let vm = clone_of(root.path(), &origin, "vm");
    let runtime = root.path().join("run");
    fs::create_dir_all(runtime.join("ljos")).unwrap();
    let stat = fs::read_to_string("/proc/self/stat").unwrap();
    let open = stat.find('(').unwrap();
    let close = stat.rfind(')').unwrap();
    let comm = &stat[open + 1..close];
    let hold = |actor: &str, assignee: &str, pid: u32, comm: &str, issue: &str| {
        fs::write(
            runtime.join("ljos").join(format!("hold-{actor}")),
            format!("{assignee}\n{assignee}\n{pid}\n{comm}\n2020-01-01T00:00:00Z\n{issue}\n-\n"),
        )
        .unwrap();
    };
    // The filename is the claim-graph actor, not the seat the tracker stores.
    hold(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "seat-live",
        std::process::id(),
        comm,
        "probe-live",
    );
    hold(
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "seat-dead",
        4_194_303,
        "no-such-runner",
        "probe-dead",
    );
    std::env::set_var("HOME", root.path().join("home"));
    std::env::set_var("XDG_RUNTIME_DIR", &runtime);
    std::env::set_var("LJOS_TRACKER_GIT", "push");
    std::env::set_var("VISSUE_ROOT", &vm);
    std::env::set_var("ISSUE_ROOT", &vm);
    let said = ljos_cli::cloud::reap_gone_owners(5);
    assert!(said.contains("probe-dead"), "{said}");
    assert!(said.contains("probe-other"), "{said}");
    assert!(!said.contains("probe-live"), "{said}");
    let local = fs::read_to_string(vm.join("Software/probe/issues.org")).unwrap();
    assert_eq!(state_of(&local, "probe-live"), "STARTED", "{local}");
    assert_eq!(state_of(&local, "probe-other"), "TODO", "{local}");
    assert_eq!(state_of(&local, "probe-dead"), "TODO", "{local}");
    let remote = show(&origin, "Software/probe/issues.org");
    assert_eq!(state_of(&remote, "probe-live"), "STARTED", "{remote}");
    assert_eq!(state_of(&remote, "probe-dead"), "TODO", "{remote}");
}

#[test]
fn ljos_claim_commits_the_issue() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    isolate_git(root.path());
    let origin = init_bare(root.path());
    seed(
        root.path(),
        &origin,
        &board(&[issue_block("probe-1a2b", "TODO", "", &[])]),
    );
    let vm = clone_of(root.path(), &origin, "vm");
    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(
        &bin.join("claimdag"),
        "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\n  *) exit 0 ;;\nesac\n",
    );
    executable(&bin.join("vissue"), "#!/bin/sh\nexit 0\n");
    let home = root.path().join("home");
    let runtime = root.path().join("run");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["claim", "probe-1a2b"])
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("VISSUE_ROOT", &vm)
        .env("ISSUE_ROOT", &vm)
        .env("LJOS_TRACKER_GIT", "push")
        .env("LJOS_SEAT", "proof-seat")
        .env("VISSUE_AGENT", "proof-seat")
        .env("PACKSET_URL", "off")
        .env("GIT_CONFIG_GLOBAL", root.path().join("gitconfig"))
        .env("GIT_CONFIG_SYSTEM", root.path().join("gitconfig"))
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("tracker git:"),
        "claim did not commit the tracker: {said}"
    );
}

#[test]
fn doctor_warns_on_unpushed_commits_and_not_on_zero() {
    let row = |state: &str, ok: bool| ljos_cli::Habitat {
        name: "tracker",
        state: state.into(),
        ok,
    };
    assert_eq!(ljos_cli::doctor_word(&row("1 unpushed", true)), "warn");
    assert_eq!(ljos_cli::doctor_word(&row("10 unpushed", true)), "warn");
    assert_eq!(
        ljos_cli::doctor_word(&row("vissue missing; 2 unpushed", false)),
        "warn"
    );
    assert_eq!(ljos_cli::doctor_word(&row("0 unpushed", true)), "ok");
}

#[test]
fn a_missing_mcp_binary_is_named() {
    let _g = Guard::new();
    clear_agent_ids();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::write(
        home.join(".cursor/mcp.json"),
        "{\"mcpServers\":{\"ljos\":{\"command\":\"/no/such/ljos-mcp\",\"args\":[]}}}\n",
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::remove_var("LJOS_MCP_CONFIG");
    let line = ljos_cli::cloud::missing_mcp_line().unwrap();
    assert!(line.contains("does not exist"), "{line}");
    assert!(line.contains("ljos onboard --harness cursor"), "{line}");
    assert!(
        !line.contains("ljos cloud up"),
        "a desktop cursor file is not a cloud VM: {line}"
    );
    let row = ljos_cli::cloud::mcp_row();
    assert!(!row.ok);
    assert_eq!(ljos_cli::doctor_word(&row), "no");

    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let plugin = repo.join("bin/ljos-plugin");
    let out = Command::new("sh")
        .arg(&plugin)
        .arg("ljos-mcp")
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.lines()
            .next()
            .unwrap()
            .contains("ljos-mcp: not installed, so the MCP server has no tools"),
        "{err}"
    );
    assert!(!out.status.success());
}

#[test]
fn doctor_advises_cloud_up_only_on_a_cloud_vm() {
    let _g = Guard::new();
    clear_agent_ids();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::write(
        home.join(".cursor/mcp.json"),
        "{\"mcpServers\":{\"ljos\":{\"command\":\"/no/such/ljos-mcp\",\"args\":[]}}}\n",
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::remove_var("LJOS_MCP_CONFIG");

    std::env::set_var("CURSOR_AGENT_ID", "bc-0a1b2c3d");
    let line = ljos_cli::cloud::missing_mcp_line().unwrap();
    assert!(line.contains("ljos cloud up"), "{line}");
    assert!(!line.contains("onboard --harness cursor"), "{line}");

    std::env::remove_var("CURSOR_AGENT_ID");
    fs::create_dir_all(home.join(".config/ljos")).unwrap();
    fs::write(
        home.join(".config/ljos/env"),
        "LJOS_SEAT=cursor-bc0a1b2c3d\n",
    )
    .unwrap();
    let from_file = ljos_cli::cloud::missing_mcp_line().unwrap();
    assert!(from_file.contains("ljos cloud up"), "{from_file}");

    fs::remove_file(home.join(".config/ljos/env")).unwrap();
    std::env::set_var("GITLAB_CI", "true");
    let ci = ljos_cli::cloud::missing_mcp_line().unwrap();
    assert!(ci.contains("ljos cloud up"), "{ci}");
}

/// `CURSOR_AGENT=1` is the desktop agent terminal. A CI variable counts
/// only when its value says so: `false` is not a job, and Buildkite,
/// Azure Pipelines, CircleCI, Jenkins, and `CI=true` are.
#[test]
fn desktop_cursor_and_ci_values_choose_cloud_up() {
    let _g = Guard::new();
    clear_agent_ids();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::write(
        home.join(".cursor/mcp.json"),
        "{\"mcpServers\":{\"ljos\":{\"command\":\"/no/such/ljos-mcp\",\"args\":[]}}}\n",
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::remove_var("LJOS_MCP_CONFIG");

    std::env::set_var("CURSOR_AGENT", "1");
    let desktop = ljos_cli::cloud::missing_mcp_line().unwrap();
    assert!(
        desktop.contains("ljos onboard --harness cursor"),
        "{desktop}"
    );
    assert!(
        !desktop.contains("ljos cloud up"),
        "CURSOR_AGENT=1 without a cloud id is the desktop agent: {desktop}"
    );

    std::env::remove_var("CURSOR_AGENT");
    for (key, value) in [("GITHUB_ACTIONS", "false"), ("CI", "false")] {
        clear_agent_ids();
        std::env::set_var("HOME", &home);
        std::env::set_var(key, value);
        let line = ljos_cli::cloud::missing_mcp_line().unwrap();
        assert!(
            !line.contains("ljos cloud up"),
            "{key}={value} is not a CI job: {line}"
        );
    }

    for (key, value) in [
        ("GITHUB_ACTIONS", "true"),
        ("GITLAB_CI", "true"),
        ("CI", "true"),
        ("BUILDKITE", "true"),
        ("TF_BUILD", "True"),
        ("CIRCLECI", "true"),
        ("JENKINS_URL", "https://jenkins.example/"),
    ] {
        clear_agent_ids();
        std::env::set_var("HOME", &home);
        std::env::set_var(key, value);
        let line = ljos_cli::cloud::missing_mcp_line().unwrap();
        assert!(
            line.contains("ljos cloud up"),
            "{key}={value} should be CI: {line}"
        );
        assert!(
            !line.contains("onboard --harness cursor"),
            "{key}={value}: {line}"
        );
    }
}

#[test]
fn doctor_flags_a_command_from_another_home() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let foreign = "/home/box/.cargo/bin/ljos-mcp";
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        format!("[mcp_servers.ljos]\ncommand = \"{foreign}\"\nargs = []\n"),
    )
    .unwrap();
    fs::write(
        home.join(".claude.json"),
        format!("{{\"mcpServers\":{{\"ljos\":{{\"command\":\"{foreign}\"}}}}}}\n"),
    )
    .unwrap();
    fs::create_dir_all(home.join(".config/opencode")).unwrap();
    fs::write(
        home.join(".config/opencode/opencode.json"),
        format!("{{\"mcp\":{{\"servers\":{{\"ljos\":{{\"command\":[\"{foreign}\"]}}}}}}}}\n"),
    )
    .unwrap();
    fs::create_dir_all(home.join(".config/goose")).unwrap();
    fs::write(
        home.join(".config/goose/config.yaml"),
        format!("extensions:\n  ljos:\n    cmd: \"{foreign}\"\n"),
    )
    .unwrap();
    fs::create_dir_all(home.join(".config/zed")).unwrap();
    fs::write(
        home.join(".config/zed/settings.json"),
        "// zed allows comments\n{ \"context_servers\": {} }\n",
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::remove_var("LJOS_MCP_CONFIG");
    std::env::set_var("PATH", "/usr/bin:/bin");
    let line = ljos_cli::cloud::missing_mcp_line().unwrap();
    assert!(line.contains(foreign), "{line}");
    assert!(line.contains(".codex"), "{line}");
    assert!(line.contains(".claude.json"), "{line}");
    assert!(line.contains("opencode.json"), "{line}");
    assert!(line.contains("config.yaml"), "{line}");
    assert!(line.contains("onboard --harness codex"), "{line}");
    assert!(line.contains("onboard --harness claude"), "{line}");
    assert!(line.contains("onboard --harness opencode"), "{line}");
    assert!(line.contains("onboard --harness goose"), "{line}");
    assert!(!line.contains("settings.json"), "{line}");
    assert!(!line.contains("not JSON"), "{line}");
    let row = ljos_cli::cloud::mcp_row();
    assert!(!row.ok, "{line}");
    assert_eq!(ljos_cli::doctor_word(&row), "no");

    fs::write(
        home.join(".codex/config.toml"),
        "[mcp_servers.ljos]\ncommand = \"ljos-mcp\"\nargs = []\n",
    )
    .unwrap();
    for extra in [
        ".claude.json",
        ".config/opencode/opencode.json",
        ".config/goose/config.yaml",
    ] {
        fs::remove_file(home.join(extra)).unwrap();
    }
    let bare = ljos_cli::cloud::missing_mcp_line().unwrap();
    assert!(bare.contains("is not on PATH"), "{bare}");
    assert!(bare.contains("onboard --harness codex"), "{bare}");

    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    executable(&bin.join("ljos-mcp"), "#!/bin/sh\nexit 0\n");
    fs::write(
        home.join(".codex/config.toml"),
        "[mcp_servers.ljos]\ncommand = \"$HOME/.local/bin/ljos-mcp\"\nargs = []\n",
    )
    .unwrap();
    let literal = ljos_cli::cloud::missing_mcp_line().unwrap();
    assert!(
        literal.contains("$HOME/.local/bin/ljos-mcp does not exist"),
        "{literal}"
    );
    assert!(literal.contains("onboard --harness codex"), "{literal}");
}

/// OpenCode expands `{env:HOME}` and leaves `$HOME`, `${HOME}`, `${env:HOME}`
/// and `~/` as text. A file that exists under the real home is not proof
/// that `$HOME/...` will run.
#[test]
fn doctor_leaves_dollar_home_literal_for_opencode() {
    let _g = Guard::new();
    clear_agent_ids();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    executable(&bin.join("ljos-mcp"), "#!/bin/sh\nexit 0\n");
    fs::create_dir_all(home.join(".config/opencode")).unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::remove_var("LJOS_MCP_CONFIG");
    std::env::set_var("PATH", "/usr/bin:/bin");
    let write = |command: &str| {
        let doc = serde_json::json!({
            "mcp": { "servers": { "ljos": { "command": [command] } } }
        });
        fs::write(
            home.join(".config/opencode/opencode.json"),
            serde_json::to_string(&doc).unwrap(),
        )
        .unwrap();
    };
    write("$HOME/.local/bin/ljos-mcp");
    let literal = ljos_cli::cloud::missing_mcp_line().unwrap_or_default();
    assert!(
        literal.contains("$HOME/.local/bin/ljos-mcp does not exist"),
        "{literal}"
    );
    write("{env:HOME}/.local/bin/ljos-mcp");
    assert!(
        ljos_cli::cloud::missing_mcp_line().is_none(),
        "opencode expands {{env:HOME}}: {:?}",
        ljos_cli::cloud::missing_mcp_line()
    );
    write("${env:HOME}/.local/bin/ljos-mcp");
    let shell = ljos_cli::cloud::missing_mcp_line().unwrap_or_default();
    assert!(
        shell.contains("${env:HOME}/.local/bin/ljos-mcp does not exist"),
        "{shell}"
    );
    write("~/.local/bin/ljos-mcp");
    let tilde = ljos_cli::cloud::missing_mcp_line().unwrap_or_default();
    assert!(
        tilde.contains("~/.local/bin/ljos-mcp does not exist"),
        "{tilde}"
    );
    fs::remove_file(home.join(".config/opencode/opencode.json")).unwrap();
    let config = root.path().join("mcp.json");
    fs::write(
        &config,
        serde_json::json!({
            "mcpServers": { "ljos": { "command": "$HOME/.local/bin/ljos-mcp" } }
        })
        .to_string(),
    )
    .unwrap();
    std::env::set_var("LJOS_MCP_CONFIG", &config);
    assert!(
        ljos_cli::cloud::missing_mcp_line().is_none(),
        "a shell harness still expands $HOME: {:?}",
        ljos_cli::cloud::missing_mcp_line()
    );
}

/// Write `command` as the ljos entry of the JSON file at `path`, check it
/// with doctor, and return the missing line.
fn doctor_on_command(path: &Path, command: &str) -> Option<String> {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let doc = serde_json::json!({ "mcpServers": { "ljos": { "command": command } } });
    fs::write(path, doc.to_string()).unwrap();
    ljos_cli::cloud::missing_mcp_line()
}

/// Cursor substitutes `${env:NAME}` and `${userHome}` and nothing else
/// (https://cursor.com/docs/context/mcp, "Config interpolation").
#[test]
fn doctor_expands_home_in_cursor_mcp_json_the_way_cursor_does() {
    let _g = Guard::new();
    clear_agent_ids();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    executable(&home.join(".local/bin/ljos-mcp"), "#!/bin/sh\nexit 0\n");
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::remove_var("LJOS_MCP_CONFIG");
    std::env::set_var("PATH", "/usr/bin:/bin");
    let config = home.join(".cursor/mcp.json");
    for ready in ["${env:HOME}", "${userHome}"] {
        let command = format!("{ready}/.local/bin/ljos-mcp");
        let line = doctor_on_command(&config, &command);
        assert!(line.is_none(), "Cursor expands {command}: {line:?}");
    }
    for literal in ["$HOME", "${HOME}", "~"] {
        let command = format!("{literal}/.local/bin/ljos-mcp");
        let line = doctor_on_command(&config, &command).unwrap_or_default();
        assert!(
            line.contains(&format!("{command} does not exist")),
            "Cursor leaves {command} as text: {line}"
        );
    }
}

/// Claude Code substitutes `${VAR}` and `${VAR:-default}` in the entries
/// `claude mcp add` stores in ~/.claude.json, and keeps an unset `${VAR}`
/// as written (https://docs.claude.com/en/docs/claude-code/mcp).
#[test]
fn doctor_expands_home_in_claude_json_the_way_claude_code_does() {
    let _g = Guard::new();
    clear_agent_ids();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    executable(&home.join(".local/bin/ljos-mcp"), "#!/bin/sh\nexit 0\n");
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::remove_var("LJOS_MCP_CONFIG");
    std::env::remove_var("LJOS_TEST_NO_SUCH_HOME");
    std::env::set_var("PATH", "/usr/bin:/bin");
    let config = home.join(".claude.json");
    let fallback = format!("${{LJOS_TEST_NO_SUCH_HOME:-{}}}", home.display());
    for ready in ["${HOME}", fallback.as_str()] {
        let command = format!("{ready}/.local/bin/ljos-mcp");
        let line = doctor_on_command(&config, &command);
        assert!(line.is_none(), "Claude Code expands {command}: {line:?}");
    }
    for literal in [
        "$HOME",
        "~",
        "${env:HOME}",
        "${userHome}",
        "${LJOS_TEST_NO_SUCH_HOME}",
    ] {
        let command = format!("{literal}/.local/bin/ljos-mcp");
        let line = doctor_on_command(&config, &command).unwrap_or_default();
        assert!(
            line.contains(&format!("{command} does not exist")),
            "Claude Code leaves {command} as text: {line}"
        );
    }
}

#[test]
fn doctor_harness_advice_clears_after_running_onboard() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let tracker = home.join("tracker");
    fs::create_dir_all(tracker.join("Software")).unwrap();
    fs::create_dir_all(home.join(".local/share")).unwrap();
    fs::create_dir_all(root.path().join("run")).unwrap();
    let foreign = "/home/box/.cargo/bin/ljos-mcp";
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        format!("[mcp_servers.ljos]\ncommand = \"{foreign}\"\nargs = []\n"),
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("XDG_RUNTIME_DIR", root.path().join("run"));
    std::env::set_var("PATH", "/usr/bin:/bin");
    let line = ljos_cli::cloud::missing_mcp_line().unwrap();
    assert!(line.contains("onboard --harness codex"), "{line}");

    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["onboard", "--harness", "codex"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("PATH", "/usr/bin:/bin")
        .env("PACKSET_URL", "off")
        .env("DEEDAR_HOST_SIGNING_KEY", "off")
        .env(
            "DEEDAR_URL",
            format!("file://{}", home.join("deeds").display()),
        )
        .env("VISSUE_ROOT", &tracker)
        .env("ISSUE_ROOT", &tracker)
        .env("GIT_CONFIG_GLOBAL", root.path().join("gitconfig"))
        .env("GIT_CONFIG_SYSTEM", root.path().join("gitconfig"))
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        ljos_cli::cloud::missing_mcp_line().is_none(),
        "following the onboard advice should clear the mcp row"
    );
}

#[test]
fn doctor_warns_when_shell_path_finds_ljos_mcp() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    executable(&bin.join("ljos-mcp"), "#!/bin/sh\nexit 0\n");
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        "[mcp_servers.ljos]\ncommand = \"ljos-mcp\"\nargs = []\n",
    )
    .unwrap();
    let shell = root.path().join("shell");
    executable(
        &shell,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"-lc\" ]; then printf '%s\\n' '{}'; exit 0; fi\nexit 1\n",
            bin.join("ljos-mcp").display()
        ),
    );
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("SHELL", &shell);
    assert!(
        ljos_cli::cloud::missing_mcp_line().is_none(),
        "a shell-resolved command should warn, not fail"
    );
    let row = ljos_cli::cloud::mcp_row();
    assert!(row.ok, "{row:?}");
    assert_eq!(ljos_cli::doctor_word(&row), "warn");
    assert!(row.state.contains("warning:"), "{row:?}");
}

/// An absolute `ljos-mcp` that exists is not proof it is this install. A
/// stale copy left in another directory is what the client starts.
#[test]
fn doctor_warns_on_an_absolute_ljos_mcp_from_another_install() {
    let _g = Guard::new();
    clear_agent_ids();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join(".local/bin");
    let old = home.join("old/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&old).unwrap();
    executable(&bin.join("ljos-mcp"), "#!/bin/sh\nexit 0\n");
    executable(&old.join("ljos-mcp"), "#!/bin/sh\nexit 1\n");
    fs::create_dir_all(home.join(".cursor")).unwrap();
    let write = |command: &Path| {
        let doc = serde_json::json!({
            "mcpServers": { "ljos": { "command": command, "args": [] } }
        });
        fs::write(home.join(".cursor/mcp.json"), doc.to_string()).unwrap();
    };
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    std::env::remove_var("LJOS_MCP_CONFIG");

    write(&old.join("ljos-mcp"));
    let row = ljos_cli::cloud::mcp_row();
    assert_eq!(ljos_cli::doctor_word(&row), "warn", "{row:?}");
    assert!(
        row.state
            .contains(&old.join("ljos-mcp").display().to_string()),
        "{row:?}"
    );
    assert!(
        row.state
            .contains(&bin.join("ljos-mcp").display().to_string()),
        "{row:?}"
    );
    assert!(
        row.state.contains("ljos onboard --harness cursor"),
        "{row:?}"
    );

    write(&bin.join("ljos-mcp"));
    let row = ljos_cli::cloud::mcp_row();
    assert_eq!(ljos_cli::doctor_word(&row), "ok", "{row:?}");
}

#[test]
fn doctor_respects_env_path_in_the_entry() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join("entry-bin");
    fs::create_dir_all(&bin).unwrap();
    executable(&bin.join("ljos-mcp"), "#!/bin/sh\nexit 0\n");
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::write(
        home.join(".cursor/mcp.json"),
        format!(
            "{{\"mcpServers\":{{\"ljos\":{{\"command\":\"ljos-mcp\",\"args\":[],\"env\":{{\"PATH\":\"{}\"}}}}}}}}\n",
            bin.display()
        ),
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::remove_var("LJOS_MCP_CONFIG");
    assert!(
        ljos_cli::cloud::missing_mcp_line().is_none(),
        "env.PATH should be used to resolve the command"
    );
    let row = ljos_cli::cloud::mcp_row();
    assert_eq!(ljos_cli::doctor_word(&row), "ok");
}

#[test]
fn doctor_reads_path_from_the_ljos_entry_only() {
    let _g = Guard::new();
    clear_agent_ids();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(&bin.join("ljos-mcp"), "#!/bin/sh\nexit 0\n");
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        "[mcp_servers.ljos]\ncommand = \"ljos-mcp\"\nargs = []\n\n[mcp_servers.ljos.env]\nNODE_PATH = \"/no/such/node\"\nLD_LIBRARY_PATH = \"/no/such/lib\"\n\n[mcp_servers.other]\ncommand = \"other\"\nPATH = \"/no/such/bin\"\n",
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    std::env::remove_var("LJOS_MCP_CONFIG");
    let line = ljos_cli::cloud::missing_mcp_line();
    assert!(
        line.is_none(),
        "NODE_PATH and another table are not ljos env.PATH: {}",
        line.unwrap_or_default()
    );

    fs::write(
        home.join(".codex/config.toml"),
        "[mcp_servers.ljos]\ncommand = \"ljos-mcp\"\n\n[mcp_servers.ljos.env]\nPATH = \"/no/such/bin\"\nNODE_PATH = \"/no/such/node\"\n",
    )
    .unwrap();
    let owned = ljos_cli::cloud::missing_mcp_line().unwrap_or_default();
    assert!(
        owned.contains("not on env.PATH"),
        "the ljos table's own PATH still counts: {owned}"
    );
}

/// env.PATH is read with the client's own syntax: Cursor `${env:PATH}`
/// (https://cursor.com/docs/context/mcp), Claude Code `${PATH}`
/// (https://docs.claude.com/en/docs/claude-code/mcp), hermes `${PATH}` or
/// `${env:PATH}` (https://hermes-agent.nousresearch.com/docs/reference/mcp-config-reference),
/// and Codex none.
#[test]
fn doctor_expands_env_path_the_way_each_client_does() {
    let _g = Guard::new();
    clear_agent_ids();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(&bin.join("ljos-mcp"), "#!/bin/sh\nexit 0\n");
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    std::env::remove_var("LJOS_MCP_CONFIG");
    let json = |path: &Path, value: &str| {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let doc = serde_json::json!({
            "mcpServers": { "ljos": { "command": "ljos-mcp", "env": { "PATH": value } } }
        });
        fs::write(path, doc.to_string()).unwrap();
        ljos_cli::cloud::missing_mcp_line()
    };
    let cases = [
        (
            home.join(".cursor/mcp.json"),
            "${env:PATH}",
            ["$PATH", "${PATH}"],
        ),
        (
            home.join(".claude.json"),
            "${PATH}",
            ["$PATH", "${env:PATH}"],
        ),
    ];
    for (path, ready, literals) in cases {
        let line = json(&path, ready);
        assert!(
            line.is_none(),
            "{} expands {ready}: {line:?}",
            path.display()
        );
        for literal in literals {
            let line = json(&path, literal).unwrap_or_default();
            assert!(
                line.contains("ljos-mcp is not on env.PATH"),
                "{} leaves {literal} as text: {line}",
                path.display()
            );
        }
        fs::remove_file(&path).unwrap();
    }

    let hermes = home.join(".hermes/config.yaml");
    fs::create_dir_all(hermes.parent().unwrap()).unwrap();
    for ready in ["${PATH}", "${env:PATH}"] {
        fs::write(
            &hermes,
            format!(
                "mcp_servers:\n  ljos:\n    command: ljos-mcp\n    env:\n      PATH: \"{ready}\"\n"
            ),
        )
        .unwrap();
        let line = ljos_cli::cloud::missing_mcp_line();
        assert!(line.is_none(), "hermes expands {ready}: {line:?}");
    }
    fs::remove_file(&hermes).unwrap();

    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        "[mcp_servers.ljos]\ncommand = \"ljos-mcp\"\n\n[mcp_servers.ljos.env]\nPATH = \"$PATH\"\n",
    )
    .unwrap();
    let line = ljos_cli::cloud::missing_mcp_line().unwrap_or_default();
    assert!(
        line.contains("ljos-mcp is not on env.PATH"),
        "Codex passes $PATH as text: {line}"
    );
}

#[test]
fn install_sh_cloud_reuses_binaries_that_already_answer() {
    let root = tempfile::tempdir().unwrap();
    let bins = root.path().join("bin");
    fs::create_dir_all(&bins).unwrap();
    let marker = root.path().join("ran");
    executable(
        &bins.join("ljos"),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = cloud ] && [ \"$2\" = up ] && [ \"$3\" = --help ]; then exit 0; fi\nif [ \"$1\" = cloud ] && [ \"$2\" = up ]; then echo ran > '{}'; exit 0; fi\nexit 1\n",
            marker.display()
        ),
    );
    executable(&bins.join("ljos-mcp"), "#!/bin/sh\nexit 0\n");
    executable(&bins.join("ljos-policyd"), "#!/bin/sh\nexit 0\n");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh");
    let help = Command::new("sh")
        .arg(&script)
        .arg("--help")
        .output()
        .unwrap();
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(
        help.status.success(),
        "{}",
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(help_text.contains("--cloud"), "{help_text}");
    let out = Command::new("sh")
        .arg(&script)
        .arg("--cloud")
        .arg("--bin-dir")
        .arg(&bins)
        .env("HOME", root.path().join("home"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{said}");
    assert!(marker.is_file(), "cloud up did not run: {said}");
    assert!(!said.contains("Installing ljos"), "{said}");
}

#[test]
fn cloud_up_tags_vm_writes_scope_cloud_without_a_default_scope() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let rel = root.path().join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(&home).unwrap();
    for name in ["ljos", "ljos-mcp", "ljos-policyd"] {
        executable(&rel.join(name), "#!/bin/sh\nexit 0\n");
    }
    fs::write(
        rel.join("SHA256SUMS"),
        sums(&rel, &["ljos", "ljos-mcp", "ljos-policyd"]),
    )
    .unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("LJOS_RELEASE_DIR", &rel);
    std::env::remove_var("LJOS_BIN_DIR");
    std::env::remove_var("LJOS_TRACKER_REMOTE");
    std::env::remove_var("LJOS_TRACKER_GIT");
    std::env::remove_var("LJOS_TRACKER_DIR");
    std::env::remove_var("LJOS_WRITE_SCOPE");
    std::env::set_var("PATH", "/usr/bin:/bin");
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", "off");
    clear_agent_ids();
    let said = ljos_cli::cloud::up(false).unwrap();
    let env = fs::read_to_string(home.join(".config/ljos/env")).unwrap();
    assert!(env.contains("LJOS_WRITE_SCOPE=cloud\n"), "{env}\n{said}");
    assert!(
        !env.contains("default_scope"),
        "cloud up set a default scope:\n{env}"
    );
    let sync_toml = home.join(".config/ljos/sync.toml");
    if sync_toml.exists() {
        let text = fs::read_to_string(&sync_toml).unwrap();
        assert!(!text.contains("default_scope"), "{text}");
    }
    assert_eq!(std::env::var("LJOS_WRITE_SCOPE").unwrap(), "cloud");
}

fn clear_agent_ids() {
    for key in [
        "CURSOR_AGENT_ID",
        "CURSOR_AGENT",
        "CURSOR_CONVERSATION_ID",
        "CURSOR_AGENT_SOCKET",
        "CLOUD_AGENT_ID",
        "AGENT_ID",
        "CODESPACE_NAME",
        "GITHUB_RUN_ID",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CI",
        "BUILDKITE",
        "TF_BUILD",
        "CIRCLECI",
        "JENKINS_URL",
        "CODESPACES",
        "LJOS_SEAT",
        "LJOS_SEAT_KEY",
        "VISSUE_AGENT",
        "DEEDAR_HOST_SIGNING_KEY",
    ] {
        std::env::remove_var(key);
    }
}

#[test]
fn a_run_id_suffixes_an_explicit_seat() {
    let _g = Guard::new();
    clear_agent_ids();
    std::env::set_var("LJOS_SEAT", "ridley-ci-a");
    assert_eq!(ljos_cli::cloud::default_seat(), "ridley-ci-a");

    std::env::set_var("GITHUB_ACTIONS", "true");
    std::env::set_var("GITHUB_RUN_ID", "555001");
    std::env::set_var("CURSOR_AGENT_ID", "should-not-win");
    assert_eq!(ljos_cli::cloud::default_seat(), "ridley-ci-a-555001");

    std::env::remove_var("GITHUB_ACTIONS");
    std::env::remove_var("GITHUB_RUN_ID");
    std::env::set_var("CURSOR_AGENT_ID", "cafebabe0001abcd");
    assert_eq!(
        ljos_cli::cloud::default_seat(),
        "ridley-ci-a-cafebabe0001abcd"
    );
    std::env::set_var("LJOS_SEAT", "ridley-ci-a-cafebabe0001abcd");
    assert_eq!(
        ljos_cli::cloud::default_seat(),
        "ridley-ci-a-cafebabe0001abcd"
    );
}

fn sig_key(text: &str) -> String {
    text.split_whitespace()
        .find(|word| word.len() == 64 && word.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or("")
        .to_string()
}

struct Cast<'a> {
    dir: &'a str,
    seat: &'a str,
    project: &'a str,
    issue: &'a str,
    note: &'a str,
    confidence: &'a str,
}

fn land(root: &Path, origin: &Path, cast: Cast<'_>) {
    let repo = clone_of(root, origin, cast.dir);
    std::env::set_var("VISSUE_ROOT", &repo);
    std::env::set_var("ISSUE_ROOT", &repo);
    std::env::set_var("LJOS_SEAT", cast.seat);
    std::env::set_var("VISSUE_AGENT", cast.seat);
    std::env::remove_var("LJOS_SEAT_KEY");
    std::env::remove_var("DEEDAR_HOST_SIGNING_KEY");
    let path = write_project(
        &repo,
        cast.project,
        &board(&[issue_block(cast.issue, "TODO", "", &[])]),
    );
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    let voted = vissue_core::ops::vote_with(
        &layout,
        cast.issue,
        Some("ship"),
        cast.seat,
        Some("none"),
        Some(cast.confidence),
    )
    .unwrap();
    assert!(voted.contains("voted"), "{voted}");
    vissue_core::ops::note(&layout, cast.issue, cast.note).unwrap();
    let signed = ljos_cli::ballot_sig::sign_own_ballot(cast.issue).unwrap();
    assert!(signed.contains("ballot signed"), "{signed}");
    let pushed = ljos_cli::persist_tracker_file(&path, cast.issue, "noted");
    assert!(pushed.contains("pushed"), "{pushed}");
}

#[test]
fn a_ci_seat_and_a_cloud_seat_vote_note_and_push_and_the_box_does_not_re_sign() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    isolate_git(root.path());
    clear_agent_ids();
    std::env::set_var("LJOS_TRACKER_GIT", "push");
    std::env::set_var("LJOS_ORG_DRIVER", "false");
    std::env::set_var("HOME", root.path().join("home"));
    std::env::set_var("XDG_RUNTIME_DIR", root.path().join("run"));
    std::env::remove_var("XDG_DATA_HOME");
    fs::create_dir_all(root.path().join("run")).unwrap();
    let origin = init_bare(root.path());
    seed(
        root.path(),
        &origin,
        &board(&[issue_block("probe-seed", "TODO", "", &[])]),
    );

    std::env::set_var("GITHUB_ACTIONS", "true");
    std::env::set_var("GITHUB_RUN_ID", "555001");
    std::env::set_var("LJOS_SEAT", "ridley-ci-a");
    let ci_seat = ljos_cli::cloud::default_seat();
    assert_eq!(ci_seat, "ridley-ci-a-555001");
    land(
        root.path(),
        &origin,
        Cast {
            dir: "ci",
            seat: &ci_seat,
            project: "cijob",
            issue: "probe-ci",
            note: "built on the runner",
            confidence: "0.8",
        },
    );

    clear_agent_ids();
    std::env::set_var("CURSOR_AGENT_ID", "cafebabe0001abcd");
    std::env::set_var("LJOS_SEAT", "ridley-ci-a");
    let cloud_seat = ljos_cli::cloud::default_seat();
    assert_eq!(cloud_seat, "ridley-ci-a-cafebabe0001abcd");
    land(
        root.path(),
        &origin,
        Cast {
            dir: "agent",
            seat: &cloud_seat,
            project: "agentjob",
            issue: "probe-agent",
            note: "built on the agent",
            confidence: "0.6",
        },
    );

    let ci = show(&origin, "Software/cijob/issues.org");
    let agent = show(&origin, "Software/agentjob/issues.org");
    assert!(ci.contains("confidence=0.8"), "{ci}");
    assert!(ci.contains("built on the runner"), "{ci}");
    assert!(ci.contains("ballot-sig:"), "{ci}");
    assert!(agent.contains("confidence=0.6"), "{agent}");
    assert!(agent.contains("built on the agent"), "{agent}");
    let ci_key = sig_key(&ci);
    let agent_key = sig_key(&agent);
    assert_ne!(ci_key, agent_key);
    assert_eq!(ci_key.len(), 64);
    let pins = show(&origin, ".ljos-seat-keys");
    assert!(pins.contains(&format!("{ci_seat} {ci_key}")), "{pins}");
    assert!(
        pins.contains(&format!("{cloud_seat} {agent_key}")),
        "{pins}"
    );

    let box_repo = clone_of(root.path(), &origin, "box");
    std::env::set_var("VISSUE_ROOT", &box_repo);
    std::env::set_var("ISSUE_ROOT", &box_repo);
    std::env::set_var("XDG_DATA_HOME", root.path().join("box-data"));
    std::env::set_var("LJOS_SEAT", "box");
    std::env::set_var("VISSUE_AGENT", "box");
    let host = root.path().join("box-config/deedar/host.key");
    fs::create_dir_all(host.parent().unwrap()).unwrap();
    fs::write(&host, [4u8; 32]).unwrap();
    std::env::set_var("LJOS_SEAT_KEY", &host);
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", &host);
    let issues = box_repo.join("Software/cijob/issues.org");
    let before = fs::read(&issues).unwrap();
    let err = ljos_cli::ballot_sig::sign_ballot("probe-ci", &ci_seat, "ship")
        .unwrap_err()
        .to_string();
    assert!(err.contains("will not re-sign"), "{err}");
    assert_eq!(fs::read(&issues).unwrap(), before);
    assert_eq!(fs::read(&host).unwrap(), vec![4u8; 32]);
}

#[test]
fn a_signed_handover_is_not_signed_again_by_the_machine_key() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    clear_agent_ids();
    let home = root.path().join("home");
    std::env::set_var("HOME", &home);
    std::env::remove_var("XDG_DATA_HOME");
    std::env::set_var("LJOS_SEAT", "ridley-ci-a-555001");
    let seat_key = ljos_cli::cloud::prepare_seat_key("ridley-ci-a-555001").unwrap();
    let host = home.join(".config/deedar/host.key");
    fs::create_dir_all(host.parent().unwrap()).unwrap();
    fs::write(&host, [8u8; 32]).unwrap();
    std::env::set_var("DEEDAR_HOST_SIGNING_KEY", &host);

    let bag = root.path().join("bag");
    fs::create_dir_all(&bag).unwrap();
    let manifest = bag.join("manifest-sha256.txt");
    fs::write(&manifest, b"abc\n").unwrap();
    let sig = bag.join("manifest-sha256.txt.sig");
    fs::write(&sig, b"seat-sig\n").unwrap();
    let log = root.path().join("deedar.log");
    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(
        &bin.join("deedar"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$DEEDAR_HOST_SIGNING_KEY\" > '{}'\necho signed\n",
            log.display()
        ),
    );
    std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display()));

    let said = ljos_cli::sign_handover_manifest(&manifest).unwrap();
    assert!(said.contains("left unchanged"), "{said}");
    assert_eq!(fs::read(&sig).unwrap(), b"seat-sig\n");
    assert!(!log.exists(), "deedar signed an existing manifest");

    fs::remove_file(&sig).unwrap();
    let said = ljos_cli::sign_handover_manifest(&manifest).unwrap();
    assert!(said.contains("signed"), "{said}");
    let used = fs::read_to_string(&log).unwrap();
    assert_eq!(used.trim(), seat_key.display().to_string());
    assert_ne!(used.trim(), host.display().to_string());
    assert_eq!(fs::read(&host).unwrap(), vec![8u8; 32]);
}

/// Pin `seat` at author time `unix` and push it. The date is what a tracker
/// writer can set; the standing list does not read it.
fn pin_at(repo: &Path, seat: &str, unix: &str) -> String {
    let path = ljos_cli::cloud::prepare_seat_key(seat).unwrap();
    let key = ljos_cli::ballot_sig::load_or_create(&path).unwrap();
    let (public, _) = ljos_cli::ballot_sig::signed_hex(&key, b"pin");
    let pins = repo.join(".ljos-seat-keys");
    let mut text = fs::read_to_string(&pins).unwrap_or_default();
    text.push_str(&format!("{seat} {public}\n"));
    fs::write(&pins, text).unwrap();
    git(repo, &["add", ".ljos-seat-keys"]);
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["commit", "-q", "-m", &format!("pin {seat}")])
        .env("GIT_AUTHOR_DATE", format!("@{unix} +0000"))
        .env("GIT_COMMITTER_DATE", format!("@{unix} +0000"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    git(repo, &["push", "-q", "origin", "HEAD"]);
    public
}

/// Pin a standing seat with an author date after slots began, and push it.
/// A pin committed before the first slot makes the seat standing. The
/// date would refuse it if the gate still trusted git time.
fn pin_standing(repo: &Path, seat: &str) -> String {
    pin_at(repo, seat, "1792700000")
}

/// The public key the run `base` with id `run` mints in the `vm-data`
/// beside this world's `HOME`, the hex `ljos cloud up` prints on that run.
fn run_key(base: &str, run: &str) -> String {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let before = std::env::var_os("XDG_DATA_HOME");
    std::env::set_var("XDG_DATA_HOME", home.parent().unwrap().join("vm-data"));
    let seat = ljos_cli::cloud::run_seat(base, run).unwrap();
    let path = ljos_cli::cloud::prepare_seat_key(&seat).unwrap();
    let key = ljos_cli::ballot_sig::load_or_create(&path).unwrap();
    match before {
        Some(v) => std::env::set_var("XDG_DATA_HOME", v),
        None => std::env::remove_var("XDG_DATA_HOME"),
    }
    ljos_cli::ballot_sig::signed_hex(&key, b"pin").0
}

/// Slot the run `base` with id `run` on `issue` under its minted key, as
/// the seat this process signs as.
fn slot_run(issue: &str, base: &str, run: &str) -> anyhow::Result<String> {
    let key = run_key(base, run);
    ljos_cli::slot::write(issue, base, "grok-4", run, &key)
}

/// A box clone signing as `ljosbot`, over an origin holding `issues` on
/// one board.
fn slot_world(root: &Path, issues: &[&str]) -> (PathBuf, PathBuf) {
    isolate_git(root);
    clear_agent_ids();
    std::env::set_var("LJOS_TRACKER_GIT", "push");
    std::env::set_var("LJOS_ORG_DRIVER", "false");
    std::env::set_var("HOME", root.join("home"));
    std::env::set_var("XDG_RUNTIME_DIR", root.join("run"));
    std::env::set_var("XDG_DATA_HOME", root.join("box-data"));
    fs::create_dir_all(root.join("run")).unwrap();
    let origin = init_bare(root);
    let blocks: Vec<String> = issues
        .iter()
        .map(|id| issue_block(id, "TODO", "", &[]))
        .collect();
    seed(root, &origin, &board(&blocks));
    let box_repo = clone_of(root, &origin, "box");
    std::env::set_var("VISSUE_ROOT", &box_repo);
    std::env::set_var("ISSUE_ROOT", &box_repo);
    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");
    (origin, box_repo)
}

#[test]
fn a_launcher_slots_the_seat_its_run_takes() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (origin, box_repo) = slot_world(root.path(), &["probe-slot"]);
    let err = slot_run("probe-slot", "ridley-review", "cafebabe0001abcd")
        .unwrap_err()
        .to_string();
    assert!(err.contains("ljosbot has no pinned key"), "{err}");

    let public = pin_standing(&box_repo, "ljosbot");
    let said = slot_run("probe-slot", "ridley-review", "cafebabe0001abcd").unwrap();
    let pushed = ljos_cli::persist_tracker("probe-slot", "slotted");
    assert!(pushed.contains("pushed"), "{pushed}");

    std::env::set_var("CURSOR_AGENT_ID", "cafebabe0001abcd");
    std::env::set_var("LJOS_SEAT", "ridley-review");
    let run_seat = ljos_cli::cloud::default_seat();
    std::env::remove_var("CURSOR_AGENT_ID");
    std::env::set_var("LJOS_SEAT", "ljosbot");
    assert!(
        said.contains(&format!("slot: {run_seat} on probe-slot")),
        "{said}"
    );
    let text = show(&origin, "Software/probe/issues.org");
    let note = text
        .lines()
        .find_map(|line| line.split('"').find(|part| part.starts_with("slot: ")))
        .unwrap_or_else(|| panic!("no slot on the remote: {text}"));
    let slot = ljos_cli::slot::parse(note).unwrap();
    assert_eq!(slot.seat, run_seat);
    assert_eq!(slot.model, "grok-4");
    assert_eq!(slot.run, "cafebabe0001abcd");
    assert_eq!(slot.run_key, run_key("ridley-review", "cafebabe0001abcd"));
    assert_eq!(slot.launcher, "ljosbot");
    assert_eq!(slot.key, public);
    assert!(ljos_cli::slot::verifies(
        &ljos_cli::slot::tracker_id(),
        "probe-slot",
        &slot
    ));
    let err = ljos_cli::slot::write(
        "probe-slot",
        "ridley-review",
        "grok-4",
        "cafebabe0001abcd",
        "not-a-key",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("is not a public seat key"), "{err}");

    let pins = box_repo.join(".ljos-seat-keys");
    let mut text = fs::read_to_string(&pins).unwrap();
    text.push_str(&format!("ridley-taken-cafebabe0001abcd {public}\n"));
    fs::write(&pins, text).unwrap();
    let err = slot_run("probe-slot", "ridley-taken", "cafebabe0001abcd")
        .unwrap_err()
        .to_string();
    assert!(err.contains("is already pinned"), "{err}");
    let err = slot_run("probe-slot", "ljosbot", "ljosbot")
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not slot itself"), "{err}");
}

/// `ridley-review` on run `cafebabe0001abcd` boots as
/// `ridley-review-cafebabe0001abcd`, and so would a seat already named
/// that, so the launcher cannot tell from the slot which one it vouches
/// for. The suffixed name is refused and the bare one is slotted.
#[test]
fn a_launcher_does_not_slot_a_seat_that_already_ends_with_the_run_id() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, box_repo) = slot_world(root.path(), &["probe-slot"]);
    pin_standing(&box_repo, "ljosbot");
    let err = ljos_cli::slot::write(
        "probe-slot",
        "ridley-review-cafebabe0001abcd",
        "grok-4",
        "cafebabe0001abcd",
        &run_key("ridley-review", "cafebabe0001abcd"),
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("ridley-review-cafebabe0001abcd already ends with run id cafebabe0001abcd"),
        "{err}"
    );
    assert!(err.contains("ridley-review on that run"), "{err}");
    let said = slot_run("probe-slot", "ridley-review", "cafebabe0001abcd").unwrap();
    assert!(
        said.contains("slot: ridley-review-cafebabe0001abcd on probe-slot"),
        "{said}"
    );
}

#[test]
fn a_run_ballot_counts_for_a_push_under_its_slot_and_not_without_one() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (origin, box_repo) = slot_world(root.path(), &["probe-slot", "probe-bare"]);
    pin_standing(&box_repo, "ljosbot");
    slot_run("probe-slot", "ridley-review", "cafebabe0001abcd").unwrap();
    let pushed = ljos_cli::persist_tracker("probe-slot", "slotted");
    assert!(pushed.contains("pushed"), "{pushed}");

    let vm = clone_of(root.path(), &origin, "vm");
    std::env::set_var("VISSUE_ROOT", &vm);
    std::env::set_var("ISSUE_ROOT", &vm);
    std::env::set_var("XDG_DATA_HOME", root.path().join("vm-data"));
    let seat = ljos_cli::cloud::run_seat("ridley-review", "cafebabe0001abcd").unwrap();
    std::env::set_var("LJOS_SEAT", &seat);
    std::env::set_var("VISSUE_AGENT", &seat);
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    for issue in ["probe-slot", "probe-bare"] {
        vissue_core::ops::vote_with(&layout, issue, Some("ship"), &seat, Some("none"), None)
            .unwrap();
        let signed = ljos_cli::ballot_sig::sign_own_ballot(issue).unwrap();
        assert!(signed.contains("ballot signed"), "{signed}");
        let pushed = ljos_cli::persist_tracker(issue, "voted");
        assert!(pushed.contains("pushed"), "{pushed}");
    }

    git(&box_repo, &["pull", "-q", "--ff-only"]);
    std::env::set_var("VISSUE_ROOT", &box_repo);
    std::env::set_var("ISSUE_ROOT", &box_repo);
    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");
    let rows = [serde_json::json!({"agent": seat, "choice": "ship"})];
    let personas = std::collections::BTreeSet::new();
    let slotted = ljos_cli::check_seat_signatures("probe-slot", &rows, &personas);
    assert_eq!(slotted, Ok(ljos_cli::SeatSignatures::default()));
    let bare = ljos_cli::check_seat_signatures("probe-bare", &rows, &personas).unwrap();
    assert_eq!(
        bare.left_out,
        vec![(
            seat.clone(),
            format!("{seat} is not a standing seat, and no slot on probe-bare names it")
        )]
    );
}

/// A writer that sees the slot and pins the run's seat first, under its
/// own key, does not vote as the run: the slot names the key the run
/// minted, and only that key's ballots count.
#[test]
fn a_seat_pinned_first_under_another_key_does_not_vote_as_the_run() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-k"]);
    pin_standing(&repo, "ljosbot");
    slot_run("probe-k", "ridley-review", "cafebabe0001abcd").unwrap();
    let pushed = ljos_cli::persist_tracker("probe-k", "slotted");
    assert!(pushed.contains("pushed"), "{pushed}");
    std::env::set_var("XDG_DATA_HOME", root.path().join("mallory-data"));
    let run = ljos_cli::cloud::run_seat("ridley-review", "cafebabe0001abcd").unwrap();
    cast(&run, "probe-k", "hold");

    let rows = [serde_json::json!({"agent": run, "choice": "hold"})];
    let left =
        ljos_cli::check_seat_signatures("probe-k", &rows, &std::collections::BTreeSet::new())
            .unwrap()
            .left_out;
    assert_eq!(
        left,
        vec![(
            run.clone(),
            format!("{run} is pinned to a key other than the one its slot names")
        )]
    );
}

/// A slot signed on another tracker, for an issue with the same id under
/// a launcher pinned to the same key, does not count here: the slot
/// signs the tracker's root commits.
#[test]
fn a_slot_copied_from_another_tracker_does_not_count() {
    let _g = Guard::new();
    let other = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (origin, repo) = slot_world(other.path(), &["probe-e", "probe-a"]);
    std::env::set_var("XDG_DATA_HOME", root.path().join("box-data"));
    pin_standing(&repo, "ljosbot");
    slot_run("probe-e", "ridley-review", "cafebabe0001abcd").unwrap();
    let pushed = ljos_cli::persist_tracker("probe-e", "slotted");
    assert!(pushed.contains("pushed"), "{pushed}");
    let text = show(&origin, "Software/probe/issues.org");
    let note = text
        .lines()
        .find_map(|line| line.split('"').find(|part| part.starts_with("slot: ")))
        .unwrap_or_else(|| panic!("no slot on the other tracker: {text}"))
        .to_string();

    let (_origin, repo) = slot_world(root.path(), &["probe-e"]);
    pin_standing(&repo, "ljosbot");
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    vissue_core::ops::note(&layout, "probe-e", &note).unwrap();
    let pushed = ljos_cli::persist_tracker("probe-e", "copied the slot");
    assert!(pushed.contains("pushed"), "{pushed}");
    std::env::set_var("XDG_DATA_HOME", other.path().join("vm-data"));
    let run = ljos_cli::cloud::run_seat("ridley-review", "cafebabe0001abcd").unwrap();
    cast(&run, "probe-e", "hold");

    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");
    let rows = [serde_json::json!({"agent": run, "choice": "hold"})];
    let left =
        ljos_cli::check_seat_signatures("probe-e", &rows, &std::collections::BTreeSet::new())
            .unwrap()
            .left_out;
    assert_eq!(
        left,
        vec![(run.clone(), format!("{run}'s slot does not verify"))]
    );
}

/// Cast and sign `seat`'s ballot for `choice` on `issue`, and push it.
fn cast(seat: &str, issue: &str, choice: &str) {
    std::env::set_var("LJOS_SEAT", seat);
    std::env::set_var("VISSUE_AGENT", seat);
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    vissue_core::ops::vote_with(&layout, issue, Some(choice), seat, Some("none"), None).unwrap();
    let signed = ljos_cli::ballot_sig::sign_own_ballot(issue).unwrap();
    assert!(signed.contains("ballot signed"), "{signed}");
    let pushed = ljos_cli::persist_tracker(issue, "voted");
    assert!(pushed.contains("pushed"), "{pushed}");
}

/// A box seat off the list whose key was pinned before the first slot
/// stands without one, and launches. A seat first pinned after the slot
/// votes against it without a slot: that ballot is left out and named,
/// and the issue still decides.
#[test]
fn a_seat_pinned_before_slots_stands_and_an_unslotted_one_is_left_out() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-box", "probe-slot"]);
    pin_standing(&repo, "eessi");
    pin_standing(&repo, "ljosbot");
    slot_run("probe-slot", "ridley-review", "cafebabe0001abcd").unwrap();
    let pushed = ljos_cli::persist_tracker("probe-slot", "slotted");
    assert!(pushed.contains("pushed"), "{pushed}");
    cast("eessi", "probe-box", "ship");
    cast("mallory", "probe-box", "hold");

    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");
    let rows = [
        serde_json::json!({"agent": "eessi", "choice": "ship"}),
        serde_json::json!({"agent": "mallory", "choice": "hold"}),
    ];
    let (words, json) = ljos_cli::push_cite_reading(
        "probe-box",
        &rows,
        &std::collections::BTreeSet::new(),
        "ljosbot",
    );
    assert_eq!(json["push_cite"]["stands"], true, "{words}");
    assert!(words.contains("decides ship"), "{words}");
    assert!(words.contains("counted eessi for ship;"), "{words}");
    assert!(
        words.contains(
            "left out: mallory is not a standing seat, and no slot on probe-box names it"
        ),
        "{words}"
    );

    std::env::set_var("LJOS_SEAT", "eessi");
    std::env::set_var("VISSUE_AGENT", "eessi");
    let said = slot_run("probe-box", "ridley-other", "feedface0003abcd");
    assert!(
        said.is_ok(),
        "a seat pinned before slots did not launch: {said:?}"
    );
}

/// A second pin line for a standing seat, appended under another key,
/// does not re-pin it: the gate refuses the pin file instead of letting
/// the last line win.
#[test]
fn an_appended_pin_line_does_not_re_pin_a_standing_seat() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-c"]);
    pin_standing(&repo, "ljosbot");
    std::env::set_var("XDG_DATA_HOME", root.path().join("mallory-data"));
    pin_standing(&repo, "ljosbot");
    cast("ljosbot", "probe-c", "hold");

    let rows = [serde_json::json!({"agent": "ljosbot", "choice": "hold"})];
    let err = ljos_cli::check_seat_signatures("probe-c", &rows, &std::collections::BTreeSet::new())
        .unwrap_err();
    assert!(err.contains("duplicate pin voter `ljosbot`"), "{err}");
}

/// Rewrite `.ljos-seat-keys` line by line with `edit`, dropping a line it
/// maps to `None`, then commit and push it.
fn rewrite_pins(repo: &Path, edit: impl Fn(&str) -> Option<String>) {
    let path = repo.join(".ljos-seat-keys");
    let text: String = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter_map(edit)
        .map(|line| format!("{line}\n"))
        .collect();
    fs::write(&path, text).unwrap();
    git(repo, &["commit", "-q", "-am", "rewrite pins"]);
    git(repo, &["push", "-q", "origin", "HEAD"]);
}

/// A standing seat's pin changed in place to another key does not re-pin
/// it: a pin once committed stays.
#[test]
fn a_pin_changed_in_place_does_not_re_pin_a_standing_seat() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-c"]);
    pin_standing(&repo, "ljosbot");
    std::env::set_var("XDG_DATA_HOME", root.path().join("mallory-data"));
    let key = ljos_cli::ballot_sig::load_or_create(
        &ljos_cli::cloud::prepare_seat_key("ljosbot").unwrap(),
    )
    .unwrap();
    let (mine, _) = ljos_cli::ballot_sig::signed_hex(&key, b"pin");
    rewrite_pins(&repo, |line| {
        Some(if line.starts_with("ljosbot ") {
            format!("ljosbot {mine}")
        } else {
            line.to_string()
        })
    });
    cast("ljosbot", "probe-c", "hold");

    let rows = [serde_json::json!({"agent": "ljosbot", "choice": "hold"})];
    let err = ljos_cli::check_seat_signatures("probe-c", &rows, &std::collections::BTreeSet::new())
        .unwrap_err();
    assert!(
        err.contains("the tracker committed the pin ljosbot ")
            && err.contains("no longer holds it"),
        "{err}"
    );
}

/// A run seat's pin removed after it voted is refused. Before, the run's
/// ballot was only left out, so dropping its pin dropped its vote.
#[test]
fn a_removed_pin_does_not_drop_a_run_ballot() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-d"]);
    pin_standing(&repo, "ljosbot");
    slot_run("probe-d", "ridley-review", "cafebabe0001abcd").unwrap();
    let pushed = ljos_cli::persist_tracker("probe-d", "slotted");
    assert!(pushed.contains("pushed"), "{pushed}");
    cast("ljosbot", "probe-d", "ship");
    std::env::set_var("XDG_DATA_HOME", root.path().join("vm-data"));
    let run = ljos_cli::cloud::run_seat("ridley-review", "cafebabe0001abcd").unwrap();
    cast(&run, "probe-d", "hold");

    let rows = [
        serde_json::json!({"agent": "ljosbot", "choice": "ship"}),
        serde_json::json!({"agent": run, "choice": "hold"}),
    ];
    let personas = std::collections::BTreeSet::new();
    assert_eq!(
        ljos_cli::check_seat_signatures("probe-d", &rows, &personas),
        Ok(ljos_cli::SeatSignatures::default())
    );
    rewrite_pins(&repo, |line| {
        (!line.starts_with(&format!("{run} "))).then(|| line.to_string())
    });
    let err = ljos_cli::check_seat_signatures("probe-d", &rows, &personas).unwrap_err();
    assert!(
        err.contains(&format!("the tracker committed the pin {run} ")),
        "{err}"
    );
}

/// A pin whose author date is before slots began, committed after the
/// first slot, does not make a seat standing, and a standing seat still
/// launches when its pin is dated after.
#[test]
fn a_backdated_pin_does_not_make_a_standing_seat() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-bare", "probe-slot"]);
    pin_standing(&repo, "ljosbot");
    slot_run("probe-slot", "ridley-first", "cafebabe0001abcd").unwrap();
    let pushed = ljos_cli::persist_tracker("probe-slot", "slotted");
    assert!(pushed.contains("pushed"), "{pushed}");
    let public = pin_at(&repo, "mallory", "1700000000");
    let needle = format!("-Smallory {public}");
    let when = git(
        &repo,
        &[
            "log",
            "-1",
            "--format=%at",
            &needle,
            "--",
            ".ljos-seat-keys",
        ],
    );
    let when: i64 = when.trim().parse().unwrap();
    assert!(
        when < ljos_cli::slot::SLOTS_SINCE,
        "the forged author time {when} is not before slots"
    );

    std::env::set_var("LJOS_SEAT", "mallory");
    std::env::set_var("VISSUE_AGENT", "mallory");
    let err = slot_run("probe-bare", "ridley-review", "feedface0002abcd")
        .unwrap_err()
        .to_string();
    assert!(err.contains("mallory is not a standing seat"), "{err}");

    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    vissue_core::ops::vote_with(
        &layout,
        "probe-bare",
        Some("ship"),
        "mallory",
        Some("none"),
        None,
    )
    .unwrap();
    let signed = ljos_cli::ballot_sig::sign_own_ballot("probe-bare").unwrap();
    assert!(signed.contains("ballot signed"), "{signed}");
    let rows = [serde_json::json!({"agent": "mallory", "choice": "ship"})];
    let bare =
        ljos_cli::check_seat_signatures("probe-bare", &rows, &std::collections::BTreeSet::new())
            .unwrap();
    assert_eq!(
        bare.left_out,
        vec![(
            "mallory".to_string(),
            "mallory is not a standing seat, and no slot on probe-bare names it".to_string()
        )]
    );

    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");
    let said = slot_run("probe-bare", "ridley-review", "feedface0002abcd");
    assert!(
        said.is_ok(),
        "a seat pinned before slots, dated after them, was refused: {said:?}"
    );
}

/// A listed name this tracker had not pinned when slots began does not
/// stand when a writer pins it after: the first to sign as `disk-saver`
/// is not the box seat, so its ballot needs a slot and it cannot launch.
#[test]
fn an_unpinned_listed_name_pinned_after_slots_does_not_stand() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-b", "probe-slot"]);
    pin_standing(&repo, "ljosbot");
    slot_run("probe-slot", "ridley-first", "cafebabe0001abcd").unwrap();
    let pushed = ljos_cli::persist_tracker("probe-slot", "slotted");
    assert!(pushed.contains("pushed"), "{pushed}");
    std::env::set_var("XDG_DATA_HOME", root.path().join("mallory-data"));
    cast("disk-saver", "probe-b", "hold");

    let rows = [serde_json::json!({"agent": "disk-saver", "choice": "hold"})];
    let left =
        ljos_cli::check_seat_signatures("probe-b", &rows, &std::collections::BTreeSet::new())
            .unwrap()
            .left_out;
    assert_eq!(
        left,
        vec![(
            "disk-saver".to_string(),
            "disk-saver is not a standing seat, and no slot on probe-b names it".to_string()
        )]
    );
    let err = slot_run("probe-b", "ridley-review", "feedface0002abcd")
        .unwrap_err()
        .to_string();
    assert!(err.contains("disk-saver is not a standing seat"), "{err}");
}

/// Stage `paths` and commit them at author time `unix`.
fn commit_dated(repo: &Path, paths: &[&str], message: &str, unix: &str) {
    let mut args = vec!["add", "--"];
    args.extend_from_slice(paths);
    git(repo, &args);
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["commit", "-q", "-m", message])
        .env("GIT_AUTHOR_DATE", format!("@{unix} +0000"))
        .env("GIT_COMMITTER_DATE", format!("@{unix} +0000"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "commit {message}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// An unsigned ballot is legacy by commit order: one already on its issue
/// when the first pin was committed counts whatever its dates say, and
/// one committed after does not, however far back its dates are set.
#[test]
fn a_legacy_ballot_is_decided_by_commit_order_not_by_date() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-old", "probe-new"]);
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    let issues = ["Software/probe/issues.org"];
    vissue_core::ops::vote_with(&layout, "probe-old", Some("ship"), "eessi", None, None).unwrap();
    commit_dated(&repo, &issues, "vote before signing", "1792800000");
    pin_standing(&repo, "inky");
    vissue_core::ops::vote_with(&layout, "probe-new", Some("hold"), "inky", None, None).unwrap();
    commit_dated(&repo, &issues, "vote after signing", "1700000000");

    let personas = std::collections::BTreeSet::new();
    let old = [serde_json::json!({"agent": "eessi", "choice": "ship"})];
    assert_eq!(
        ljos_cli::check_seat_signatures("probe-old", &old, &personas),
        Ok(ljos_cli::SeatSignatures {
            legacy: vec!["eessi".to_string()],
            left_out: Vec::new(),
        })
    );
    let new = [serde_json::json!({"agent": "inky", "choice": "hold"})];
    let err = ljos_cli::check_seat_signatures("probe-new", &new, &personas).unwrap_err();
    assert!(err.contains("inky's ballot is unsigned"), "{err}");
}

/// The same slot line, committed earlier on another issue, does not make
/// this issue's pin look later than the slot. The oldest author time of
/// the line is before the pin; the gate still refuses.
#[test]
fn a_slot_line_replayed_from_another_issue_does_not_backdate_it() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-slot", "probe-other"]);
    pin_standing(&repo, "ljosbot");
    let launcher = ljos_cli::ballot_sig::load_or_create(
        &ljos_cli::cloud::prepare_seat_key("ljosbot").unwrap(),
    )
    .unwrap();
    let run = "review-replay-0001";
    let run_key =
        ljos_cli::ballot_sig::load_or_create(&ljos_cli::cloud::prepare_seat_key(run).unwrap())
            .unwrap();
    let (public, _) = ljos_cli::ballot_sig::signed_hex(&run_key, b"pin");
    let note = ljos_cli::slot::sign(
        &launcher,
        &ljos_cli::slot::tracker_id(),
        "probe-slot",
        run,
        "grok",
        "r1",
        &public,
        "ljosbot",
    );
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    vissue_core::ops::note(&layout, "probe-other", &note).unwrap();
    commit_dated(
        &repo,
        &["Software/probe/issues.org"],
        "note the slot on the other issue",
        "1700000000",
    );

    let pins = repo.join(".ljos-seat-keys");
    let mut text = fs::read_to_string(&pins).unwrap();
    text.push_str(&format!("{run} {public}\n"));
    fs::write(&pins, text).unwrap();
    commit_dated(
        &repo,
        &[".ljos-seat-keys"],
        "pin the run seat",
        "1792700000",
    );

    let pick = format!("-S{note}");
    let times = git(&repo, &["log", "--format=%at", &pick]);
    let earliest: i64 = times
        .lines()
        .filter_map(|t| t.trim().parse().ok())
        .min()
        .unwrap();
    let pin_time: i64 = git(
        &repo,
        &[
            "log",
            "-1",
            "--format=%at",
            &format!("-S{run} {public}"),
            "--",
            ".ljos-seat-keys",
        ],
    )
    .trim()
    .parse()
    .unwrap();
    assert!(
        earliest < pin_time,
        "the replayed line at {earliest} is not older than the pin at {pin_time}"
    );
    assert_eq!(
        ljos_cli::slot::recorded_before_pin("probe-other", &note, run, &public),
        Ok(Some(true)),
        "the other issue's own entry did not hold the replayed line"
    );
    assert_eq!(
        ljos_cli::slot::recorded_before_pin("probe-slot", &note, run, &public),
        Ok(Some(false)),
        "the replay counted as this issue's slot"
    );

    vissue_core::ops::note(&layout, "probe-slot", &note).unwrap();
    std::env::set_var("LJOS_SEAT", run);
    std::env::set_var("VISSUE_AGENT", run);
    vissue_core::ops::vote_with(&layout, "probe-slot", Some("ship"), run, Some("none"), None)
        .unwrap();
    let signed = ljos_cli::ballot_sig::sign_own_ballot("probe-slot").unwrap();
    assert!(signed.contains("ballot signed"), "{signed}");
    commit_dated(
        &repo,
        &["Software/probe/issues.org"],
        "slot this issue after the pin",
        "1792710000",
    );
    assert_eq!(
        ljos_cli::slot::recorded_before_pin("probe-slot", &note, run, &public),
        Ok(Some(false)),
        "a later copy on this issue backdated the pin"
    );
    let rows = [serde_json::json!({"agent": run, "choice": "ship"})];
    let left =
        ljos_cli::check_seat_signatures("probe-slot", &rows, &std::collections::BTreeSet::new())
            .unwrap()
            .left_out;
    assert_eq!(
        left,
        vec![(
            run.to_string(),
            "review-replay-0001's key was pinned before its slot".to_string()
        )]
    );
}

/// A run's pin that is only in the working tree has no commit to read its
/// slot at, so the ballot is left out until the pin is committed.
#[test]
fn an_uncommitted_run_pin_does_not_count_under_its_slot() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-u"]);
    pin_standing(&repo, "ljosbot");
    slot_run("probe-u", "ridley-review", "cafebabe0001abcd").unwrap();
    let pushed = ljos_cli::persist_tracker("probe-u", "slotted");
    assert!(pushed.contains("pushed"), "{pushed}");
    std::env::set_var("XDG_DATA_HOME", root.path().join("vm-data"));
    let run = ljos_cli::cloud::run_seat("ridley-review", "cafebabe0001abcd").unwrap();
    std::env::set_var("LJOS_SEAT", &run);
    std::env::set_var("VISSUE_AGENT", &run);
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    vissue_core::ops::vote_with(&layout, "probe-u", Some("ship"), &run, Some("none"), None)
        .unwrap();
    let signed = ljos_cli::ballot_sig::sign_own_ballot("probe-u").unwrap();
    assert!(signed.contains("ballot signed"), "{signed}");

    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");
    let rows = [serde_json::json!({"agent": run, "choice": "ship"})];
    let personas = std::collections::BTreeSet::new();
    let left = ljos_cli::check_seat_signatures("probe-u", &rows, &personas)
        .unwrap()
        .left_out;
    assert_eq!(
        left,
        vec![(
            run.clone(),
            format!("{run}'s key was pinned before its slot")
        )]
    );
    let pushed = ljos_cli::persist_tracker("probe-u", "voted");
    assert!(pushed.contains("pushed"), "{pushed}");
    assert_eq!(
        ljos_cli::check_seat_signatures("probe-u", &rows, &personas),
        Ok(ljos_cli::SeatSignatures::default())
    );
}

/// A run's pin introduced twice, once under its slot and once on a branch
/// whose issue had no slot, then merged, was pinned before its slot: one
/// introduction under the slot is not enough.
#[test]
fn a_run_pin_merged_from_a_branch_without_its_slot_does_not_count() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-m"]);
    pin_standing(&repo, "ljosbot");
    let main = git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let main = main.trim();
    git(&repo, &["branch", "side"]);
    slot_run("probe-m", "ridley-review", "cafebabe0001abcd").unwrap();
    let pushed = ljos_cli::persist_tracker("probe-m", "slotted");
    assert!(pushed.contains("pushed"), "{pushed}");
    std::env::set_var("XDG_DATA_HOME", root.path().join("vm-data"));
    let run = ljos_cli::cloud::run_seat("ridley-review", "cafebabe0001abcd").unwrap();
    cast(&run, "probe-m", "ship");

    git(&repo, &["checkout", "-q", "side"]);
    let pins = repo.join(".ljos-seat-keys");
    let mut text = fs::read_to_string(&pins).unwrap();
    text.push_str(&format!(
        "{run} {}\n",
        run_key("ridley-review", "cafebabe0001abcd")
    ));
    fs::write(&pins, text).unwrap();
    git(
        &repo,
        &["commit", "-q", "-am", "pin the run without its slot"],
    );
    git(&repo, &["checkout", "-q", main]);
    git(
        &repo,
        &["merge", "-q", "--no-ff", "-m", "merge side", "side"],
    );
    git(&repo, &["push", "-q", "origin", "HEAD"]);

    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");
    let rows = [serde_json::json!({"agent": run, "choice": "ship"})];
    let left =
        ljos_cli::check_seat_signatures("probe-m", &rows, &std::collections::BTreeSet::new())
            .unwrap()
            .left_out;
    assert_eq!(
        left,
        vec![(
            run.clone(),
            format!("{run}'s key was pinned before its slot")
        )]
    );
}

/// A tracker that is not a git repository keeps no commit order to read a
/// pin against its slot, so a run pinned to the key its slot names counts
/// there under a launcher on the standing list.
#[test]
fn a_slotted_run_counts_on_a_tracker_that_is_not_a_git_repository() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    isolate_git(root.path());
    clear_agent_ids();
    std::env::set_var("LJOS_TRACKER_GIT", "off");
    std::env::set_var("LJOS_ORG_DRIVER", "false");
    std::env::set_var("HOME", root.path().join("home"));
    std::env::set_var("XDG_RUNTIME_DIR", root.path().join("run"));
    std::env::set_var("XDG_DATA_HOME", root.path().join("box-data"));
    fs::create_dir_all(root.path().join("run")).unwrap();
    let tracker = root.path().join("tracker");
    write_project(
        &tracker,
        "probe",
        &board(&[issue_block("probe-n", "TODO", "", &[])]),
    );
    std::env::set_var("VISSUE_ROOT", &tracker);
    std::env::set_var("ISSUE_ROOT", &tracker);
    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");
    let key = ljos_cli::ballot_sig::load_or_create(
        &ljos_cli::cloud::prepare_seat_key("ljosbot").unwrap(),
    )
    .unwrap();
    let (public, _) = ljos_cli::ballot_sig::signed_hex(&key, b"pin");
    fs::write(
        tracker.join(".ljos-seat-keys"),
        format!("ljosbot {public}\n"),
    )
    .unwrap();
    assert_eq!(ljos_cli::slot::tracker_id(), "-");
    slot_run("probe-n", "ridley-review", "cafebabe0001abcd").unwrap();

    std::env::set_var("XDG_DATA_HOME", root.path().join("vm-data"));
    let run = ljos_cli::cloud::run_seat("ridley-review", "cafebabe0001abcd").unwrap();
    std::env::set_var("LJOS_SEAT", &run);
    std::env::set_var("VISSUE_AGENT", &run);
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    vissue_core::ops::vote_with(&layout, "probe-n", Some("ship"), &run, Some("none"), None)
        .unwrap();
    let signed = ljos_cli::ballot_sig::sign_own_ballot("probe-n").unwrap();
    assert!(signed.contains("ballot signed"), "{signed}");

    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");
    let rows = [serde_json::json!({"agent": run, "choice": "ship"})];
    assert_eq!(
        ljos_cli::check_seat_signatures("probe-n", &rows, &std::collections::BTreeSet::new()),
        Ok(ljos_cli::SeatSignatures::default())
    );
}

/// A writer that answers `GET /v1/atoms` with an empty roster.
fn serve_empty_pack() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            let mut buf = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                let n = s.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let body = r#"{"atoms":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    format!("http://{addr}")
}

/// Two standing seats vote ship from one harness session. The older note
/// is the pusher. Raw rows are two ballots, so a cite that does not count
/// them stands; consensus, which counts, does not. Both paths refuse.
#[test]
fn duplicate_ballots_from_one_session_have_one_push_verdict() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-dup"]);
    pin_standing(&repo, "ljosbot");
    pin_standing(&repo, "inky");
    let issue = "probe-dup";
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    for seat in ["ljosbot", "inky"] {
        vissue_core::ops::vote_with(&layout, issue, Some("ship"), seat, Some("none"), None)
            .unwrap();
    }
    let harness = "sess-dup-0001";
    for seat in ["ljosbot", "inky"] {
        let key =
            ljos_cli::ballot_sig::load_or_create(&ljos_cli::cloud::prepare_seat_key(seat).unwrap())
                .unwrap();
        let note = ljos_cli::ballot_sig::sign_bound(&key, issue, seat, "ship", harness, "-");
        vissue_core::ops::note(&layout, issue, &note).unwrap();
    }

    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(
        &bin.join("vissue"),
        "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = vote ]; then\n    printf '%s\\n' '[{\"agent\":\"ljosbot\",\"choice\":\"ship\"},{\"agent\":\"inky\",\"choice\":\"ship\"}]'\n    exit 0\n  fi\ndone\nexit 1\n",
    );
    std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    std::env::set_var("PACKSET_URL", serve_empty_pack());
    std::env::set_var("PACKSET_WORKSPACE", "seat");
    std::env::remove_var("LJOS_UNSIGNED_BALLOTS");
    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");

    assert_eq!(ljos_cli::seat_name(), "ljosbot");
    let rows = ljos_cli::ballot_rows(issue);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["agent"].as_str(), Some("ljosbot"), "{rows:?}");
    let cite = ljos_cli::cite_stands(issue);
    let (words, json) = ljos_cli::push_cite_reading(
        issue,
        &rows,
        &std::collections::BTreeSet::new(),
        &ljos_cli::seat_name(),
    );
    let stands = json["push_cite"]["stands"].as_bool().unwrap();
    assert!(!stands, "{words}");
    assert!(words.contains("does not stand"), "{words}");
    assert_eq!(cite.is_ok(), stands, "{cite:?} {words}");
    let err = cite.unwrap_err();
    assert!(err.contains("the only seat ballot"), "{err}");
    assert!(err.contains("ljosbot"), "{err}");
}

/// A tracker under `root` where `inky`, pinned before any slot, voted
/// ship on `probe-cite` and nobody voted on `probe-none`, read through a
/// stub `vissue`, with `ljosbot` pushing.
fn cite_world(root: &Path) {
    let (_origin, repo) = slot_world(root, &["probe-cite", "probe-none"]);
    pin_standing(&repo, "inky");
    cast("inky", "probe-cite", "ship");

    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(
        &bin.join("vissue"),
        "#!/bin/sh\n[ \"$1\" = vote ] || exit 1\nif [ \"$2\" = probe-cite ]; then\n  printf '%s\\n' '[{\"agent\":\"inky\",\"choice\":\"ship\"}]'\nelse\n  echo '[]'\nfi\n",
    );
    std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    std::env::set_var("PACKSET_WORKSPACE", "seat");
    std::env::set_var("LJOS_GUARD", "on");
    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");
}

/// The push gate runs a push whose `LJOS_CITE` stands: another seat's
/// signed ballot decides the issue. The same push citing an issue no
/// ballot decides is denied, and says why.
#[test]
fn the_push_gate_denies_a_cite_that_does_not_stand() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    cite_world(root.path());
    std::env::set_var("PACKSET_URL", serve_empty_pack());

    let stands = ljos_cli::cite_stands("probe-cite");
    assert!(stands.is_ok(), "{stands:?}");
    let pushed = ljos_cli::gate_push(None, "LJOS_CITE=probe-cite git push origin main", None);
    assert_eq!(pushed, None);
    let refused = ljos_cli::gate_push(None, "LJOS_CITE=probe-none git push origin main", None)
        .expect("a cite that does not stand was let through");
    assert_eq!(refused.verdict, "deny");
    assert!(
        refused
            .reason
            .starts_with("LJOS_CITE=probe-none does not stand: "),
        "{}",
        refused.reason
    );
}

/// The push gate and the push cite `ljos consensus` prints read the
/// roster the same way. A pack that does not answer, and a pack that is
/// off, both leave the cite standing on nothing, for the same reason: a
/// persona's ballot cannot be told from a seat's.
#[test]
fn the_gate_and_consensus_agree_when_the_roster_is_not_readable() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    cite_world(root.path());
    let rows = [serde_json::json!({"agent": "inky", "choice": "ship"})];
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let down = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    std::env::set_var("PACKSET_URL", &down);

    let why = ljos_cli::cite_stands("probe-cite").unwrap_err();
    assert!(why.contains("the roster is not readable"), "{why}");
    let (words, json) = ljos_cli::push_cite_reading_on_roster("probe-cite", &rows, "ljosbot");
    assert_eq!(json["push_cite"]["stands"], false, "{words}");
    assert_eq!(json["push_cite"]["refusal"], why.as_str(), "{words}");
    let refused = ljos_cli::gate_push(None, "LJOS_CITE=probe-cite git push origin main", None);
    assert_eq!(refused.map(|r| r.verdict), Some("deny".to_string()));

    std::env::set_var("PACKSET_URL", "off");
    let why = ljos_cli::cite_stands("probe-cite").unwrap_err();
    assert!(why.contains("the roster is not readable"), "{why}");
    let (words, json) = ljos_cli::push_cite_reading_on_roster("probe-cite", &rows, "ljosbot");
    assert_eq!(json["push_cite"]["stands"], false, "{words}");
    assert_eq!(json["push_cite"]["refusal"], why.as_str(), "{words}");
}

/// `hn-skeptic` is a persona: pinned and signed, and the only ballot on
/// the issue besides the pusher's absence. With the pack off that ballot
/// used to count as a seat, and the push cite stood on it.
#[test]
fn a_persona_ballot_does_not_count_as_a_seat_when_the_pack_is_off() {
    let _g = Guard::new();
    let root = tempfile::tempdir().unwrap();
    let (_origin, repo) = slot_world(root.path(), &["probe-cite"]);
    pin_standing(&repo, "hn-skeptic");
    cast("hn-skeptic", "probe-cite", "ship");

    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(
        &bin.join("vissue"),
        "#!/bin/sh\n[ \"$1\" = vote ] || exit 1\nprintf '%s\\n' '[{\"agent\":\"hn-skeptic\",\"choice\":\"ship\"}]'\n",
    );
    std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    std::env::set_var("PACKSET_URL", "off");
    std::env::set_var("PACKSET_WORKSPACE", "seat");
    std::env::set_var("LJOS_GUARD", "on");
    std::env::remove_var("LJOS_UNSIGNED_BALLOTS");
    std::env::set_var("LJOS_SEAT", "ljosbot");
    std::env::set_var("VISSUE_AGENT", "ljosbot");

    let why = ljos_cli::cite_stands("probe-cite").unwrap_err();
    assert!(why.contains("the roster is not readable"), "{why}");
    assert!(
        !why.contains("counted hn-skeptic"),
        "a persona was counted as a seat: {why}"
    );
    let rows = [serde_json::json!({"agent": "hn-skeptic", "choice": "ship"})];
    let (words, json) = ljos_cli::push_cite_reading_on_roster("probe-cite", &rows, "ljosbot");
    assert_eq!(json["push_cite"]["stands"], false, "{words}");
    assert!(
        json["push_cite"]["refusal"]
            .as_str()
            .unwrap_or("")
            .contains("the roster is not readable"),
        "{words}"
    );
    let refused = ljos_cli::gate_push(None, "LJOS_CITE=probe-cite git push origin main", None)
        .expect("a persona ballot let a push through");
    assert_eq!(refused.verdict, "deny");
    assert!(
        refused.reason.contains("the roster is not readable"),
        "{}",
        refused.reason
    );
}
