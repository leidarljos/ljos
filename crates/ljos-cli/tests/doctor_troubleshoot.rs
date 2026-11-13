//! What `ljos doctor` prints for the breaks a new seat hits, and that each
//! line names the cause and the command that fixes it.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::Stdio;

fn root() -> PathBuf {
    tempfile::tempdir().unwrap().keep()
}

fn write_exe(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Habitat stubs. `vissue identity` prints `VISSUE_ROOT` when it is set.
fn stubs(bin: &Path) {
    write_exe(
        &bin.join("habitat"),
        "#!/bin/sh\n\
name=$(basename \"$0\")\n\
case \"$name\" in\n\
  curl) echo 'curl: network is unreachable' >&2; exit 6;;\n\
  ljos-policyd)\n\
    if [ \"$1\" = version ]; then echo 'ljos-policyd 0.7.0 (phronesis)'; exit 0; fi\n\
    echo allow; exit 0;;\n\
  vissue)\n\
    if [ \"$1\" = --version ]; then echo 'vissue 0.23.1'; exit 0; fi\n\
    if [ \"$1\" = vote ]; then echo '--used --confidence'; exit 0; fi\n\
    if [ \"$1\" = identity ]; then\n\
      if [ -z \"${VISSUE_ROOT:-}${ISSUE_ROOT:-}\" ]; then echo 'vissue: not inside a tracker' >&2; exit 1; fi\n\
      root=${VISSUE_ROOT:-$ISSUE_ROOT}\n\
      echo 'vissue 0.23.1'\n\
      echo \"root=$root\"\n\
      echo 'prefix=Software'\n\
      exit 0\n\
    fi\n\
    echo 'vissue: unexpected' >&2; exit 1;;\n\
  *) echo \"$name 0.1.0\"; exit 0;;\n\
esac\n",
    );
    for name in [
        "curl",
        "ljos-policyd",
        "vissue",
        "deedar",
        "claimdag",
        "packset",
        "packsetd",
    ] {
        let link = bin.join(name);
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink("habitat", &link).unwrap();
    }
}

fn doctor(home: &Path, path: &str) -> (i32, String) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("doctor")
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("USER", "ada")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TMPDIR", home.join("tmp"))
        .env("PACKSET_TIMEOUT_MS", "200")
        .env("LJOS_HOOKD", "off")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(1), text)
}

fn layout(name: &str) -> (PathBuf, PathBuf) {
    let home = root().join(name);
    for dir in [
        ".config",
        ".local/share",
        ".local/state",
        ".cache",
        "run",
        "tmp",
        "bin",
    ] {
        std::fs::create_dir_all(home.join(dir)).unwrap();
    }
    let bin = home.join("bin");
    stubs(&bin);
    (home, bin)
}

#[test]
fn doctor_warns_when_no_seat_was_named_and_names_the_export() {
    let (home, bin) = layout("seat");
    let (code, text) = doctor(&home, &format!("{}:/usr/bin:/bin", bin.display()));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(
            "warn\tseat source\tno seat rule matched; reading every seat, so run `export LJOS_SEAT=NAME`.\n"
        ),
        "{text}"
    );
}

#[test]
fn doctor_names_a_seat_someone_exported() {
    let (home, bin) = layout("named-seat");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("doctor")
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("USER", "ada")
        .env("LJOS_SEAT", "ada")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TMPDIR", home.join("tmp"))
        .env("PACKSET_TIMEOUT_MS", "200")
        .env("LJOS_HOOKD", "off")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("seat source") && text.contains("ada named by LJOS_SEAT"),
        "{text}"
    );
}

#[test]
fn doctor_names_an_unknown_host_left_by_the_last_write() {
    let (home, bin) = layout("unknown-write");
    let host = ljos_cli::sync::host();
    assert!(!host.is_empty(), "this machine has no host name");
    let seat = format!("unknown-{host}");
    let dir = home.join(".local/state/ljos");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("seat-writes.log"),
        format!("2026-11-13T00:00:00Z\t{seat}\t{host}\n"),
    )
    .unwrap();
    let (code, text) = doctor(&home, &format!("{}:/usr/bin:/bin", bin.display()));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("seat source") && text.contains(&seat),
        "{text}"
    );
}

#[test]
fn doctor_fails_an_unwritable_tracker_and_names_chmod() {
    let (home, bin) = layout("ro");
    let tracker = home.join("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tracker, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("doctor")
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("USER", "ada")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TMPDIR", home.join("tmp"))
        .env("PACKSET_TIMEOUT_MS", "200")
        .env("LJOS_HOOKD", "off")
        .env("VISSUE_ROOT", &tracker)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1), "{text}");
    let line = format!(
        "{} is not writable. `chmod u+w {}` lets this account file tickets",
        tracker.display(),
        tracker.display()
    );
    assert!(text.contains(&line), "{text}");
    assert!(text.contains("ljos is not ready:"), "{text}");
    assert!(
        text.contains("tracker does not answer") || text.contains("tracker do not answer"),
        "{text}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tracker, std::fs::Permissions::from_mode(0o755));
    }
}

#[test]
fn doctor_names_a_stale_tracker_config_and_mkdir() {
    let (home, bin) = layout("stale");
    let root = home.join("old-tracker");
    let cfg = home.join(".config/vissue");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("config.toml"),
        format!("root = \"{}\"\n", root.display()),
    )
    .unwrap();
    let (code, text) = doctor(&home, &format!("{}:/usr/bin:/bin", bin.display()));
    assert_eq!(code, 1, "{text}");
    let line = format!(
        "{} names {}, which is not a directory. `mkdir -p {}` creates it",
        cfg.join("config.toml").display(),
        root.display(),
        root.join("Software").display()
    );
    assert!(text.contains(&line), "{text}");
}

#[test]
fn doctor_names_the_install_when_a_habitat_binary_is_missing() {
    let (home, bin) = layout("habitat");
    let _ = std::fs::remove_file(bin.join("deedar"));
    let tracker = home.join("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["doctor", "--all"])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("USER", "ada")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TMPDIR", home.join("tmp"))
        .env("PACKSET_TIMEOUT_MS", "200")
        .env("LJOS_HOOKD", "off")
        .env("VISSUE_ROOT", &tracker)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("no\tdeedar\t")
            && text.contains("so run `cargo install --locked deedar-cli`."),
        "{text}"
    );
    assert!(
        text.contains(
            "info\tdeeds\tdeedar not on PATH, so run `cargo install --locked deedar-cli`.\n"
        ),
        "{text}"
    );
}

#[test]
fn doctor_warns_when_crates_io_does_not_answer() {
    let (home, bin) = layout("net");
    let tracker = home.join("tracker");
    std::fs::create_dir_all(tracker.join("Software")).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("doctor")
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("USER", "ada")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TMPDIR", home.join("tmp"))
        .env("PACKSET_TIMEOUT_MS", "200")
        .env("LJOS_HOOKD", "off")
        .env("VISSUE_ROOT", &tracker)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("warn\tcrates.io\tdid not answer, so the version check was skipped\n"),
        "{text}"
    );
}

#[test]
fn doctor_names_onboard_when_no_tracker_exists() {
    let (home, bin) = layout("none");
    let _ = std::fs::remove_file(bin.join("vissue"));
    // vissue is gone, so the row is the missing binary, which already
    // names the install. Put vissue back and leave the root unset so
    // identity says the seat is not inside a tracker.
    std::os::unix::fs::symlink("habitat", bin.join("vissue")).unwrap();
    let (code, text) = doctor(&home, &format!("{}:/usr/bin:/bin", bin.display()));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(
            "info\ttickets\tvissue failed (exit 1): not inside a tracker, so run `ljos onboard`.\n"
        ),
        "{text}"
    );
    assert!(
        !text
            .lines()
            .any(|line| line.contains("tracker does not answer")),
        "{text}"
    );
}
