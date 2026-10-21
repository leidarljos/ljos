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
    assert_eq!(doc["mcpServers"]["ljos"]["command"], "ljos-mcp", "{mcp}");
    assert!(
        !doc["mcpServers"]["ljos"]["command"]
            .as_str()
            .unwrap_or("")
            .contains(home.to_str().unwrap()),
        "{mcp}"
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
    assert_eq!(doc["mcpServers"]["ljos"]["command"], "ljos-mcp", "{mcp}");
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

/// Ubuntu's skel bashrc returns from a `case $-` before any later PATH
/// export. A non-interactive `bash -c '. ~/.bashrc'` never sees a line
/// appended after that case.
#[test]
fn cloud_up_inserts_path_before_ubuntu_skel_interactive_case() {
    let _g = Guard::new();
    let skel = fs::read_to_string("/etc/skel/.bashrc").unwrap();
    assert!(
        skel.contains("case $- in\n    *i*) ;;\n      *) return;;\n"),
        "expected the Ubuntu skel case guard, got:\n{skel}"
    );
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let rel = root.path().join("rel");
    fs::create_dir_all(&rel).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), &skel).unwrap();
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
    let guard_at = bashrc
        .find("case $- in")
        .unwrap_or_else(|| panic!("{bashrc}"));
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
fn cloud_up_installs_cloud_guard_refusals_idempotently() {
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
    std::env::set_var("LJOS_GUARD", "on");
    clear_agent_ids();

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
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::write(
        home.join(".cursor/mcp.json"),
        serde_json::json!({
            "mcpServers": { "ljos": { "command": "$HOME/.local/bin/ljos-mcp" } }
        })
        .to_string(),
    )
    .unwrap();
    assert!(
        ljos_cli::cloud::missing_mcp_line().is_none(),
        "a shell harness still expands $HOME: {:?}",
        ljos_cli::cloud::missing_mcp_line()
    );
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

    fs::remove_file(home.join(".codex/config.toml")).unwrap();
    fs::create_dir_all(home.join(".cursor")).unwrap();
    for value in ["${env:PATH}", "$PATH"] {
        fs::write(
            home.join(".cursor/mcp.json"),
            format!(
                "{{\"mcpServers\":{{\"ljos\":{{\"command\":\"ljos-mcp\",\"args\":[],\"env\":{{\"PATH\":\"{value}\"}}}}}}}}\n"
            ),
        )
        .unwrap();
        let expanded = ljos_cli::cloud::missing_mcp_line();
        assert!(
            expanded.is_none(),
            "{value} in JSON env.PATH should expand: {}",
            expanded.unwrap_or_default()
        );
    }
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
