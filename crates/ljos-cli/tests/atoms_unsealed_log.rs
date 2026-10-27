//! A tracker log sealed only to another machine, larger than the pipe
//! buffers, is skipped by `ljos sync --import`. age stops reading after
//! the header when no identity matches, and the rest of the ciphertext
//! used to end ljos with SIGPIPE before it printed a line. Runs the built
//! `ljos` binary, since the test harness ignores SIGPIPE.
#![cfg(target_os = "linux")]

ljos_test_boot::harness!();
use std::io::Write as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

fn keygen(path: &Path) -> String {
    let made = Command::new("age-keygen")
        .arg("-o")
        .arg(path)
        .output()
        .expect("age-keygen must be installed");
    assert!(made.status.success(), "age-keygen");
    let public = Command::new("age-keygen")
        .arg("-y")
        .arg(path)
        .output()
        .unwrap();
    assert!(public.status.success(), "age-keygen -y");
    String::from_utf8_lossy(&public.stdout).trim().to_string()
}

fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

#[test]
fn a_large_log_for_another_machine_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let config = home.join(".config");
    std::fs::create_dir_all(config.join("ljos")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    keygen(&config.join("ljos/age.key"));
    let other = keygen(&dir.path().join("other.key"));
    let repo = dir.path().join("tracker");
    std::fs::create_dir_all(repo.join(".ljos/atoms")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "core.hooksPath", "/dev/null"]);
    std::fs::write(
        repo.join(".ljos/sync.toml"),
        format!("name = \"probe\"\nrecipients = [\"{other}\"]\n"),
    )
    .unwrap();
    std::fs::write(
        config.join("ljos/sync.toml"),
        format!("repos = [\"{}\"]\n", repo.display()),
    )
    .unwrap();
    let mut plain = String::from("{\"ljos_log\":1,\"host\":\"peer\",\"scope\":\"probe\"}\n");
    for n in 0..2000 {
        plain.push_str(&format!(
            "{{\"id\":\"a{n:04}\",\"kind\":\"lesson\",\"text\":\"note {n} {}\"}}\n",
            "x".repeat(200)
        ));
    }
    let mut seal = Command::new("age")
        .args(["-r", &other, "-o"])
        .arg(repo.join(".ljos/atoms/peer.jsonl.age"))
        .stdin(Stdio::piped())
        .spawn()
        .expect("age must be installed");
    seal.stdin
        .take()
        .unwrap()
        .write_all(plain.as_bytes())
        .unwrap();
    assert!(seal.wait().unwrap().success(), "age seal");
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["sync", "--import"])
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("LJOS_TEST_HOSTNAME", "me")
        .env("PACKSET_URL", "off")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(&repo)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.status.signal(),
        None,
        "ljos was killed by a signal: {text}"
    );
    assert!(
        text.contains("peer's probe log is not sealed to this machine"),
        "{text}"
    );
}
