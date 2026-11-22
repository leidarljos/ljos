//! A named `LJOS_SEAT`, or the seat argument of a call, is what names the
//! seat. A seats file and the process tree do not.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;

use std::path::{Path, PathBuf};
use std::process::Command;

fn exec_daemon(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let src = dir.join("exec-daemon.c");
    let bin = dir.join("exec-daemon");
    std::fs::write(
        &src,
        "#include <unistd.h>\n\
         int main(int argc, char **argv) {\n\
         \tif (argc < 2) return 127;\n\
         \texecvp(argv[1], argv + 1);\n\
         \treturn 127;\n\
         }\n",
    )
    .unwrap();
    let out = Command::new("/usr/bin/cc")
        .args(["-O2", "-o"])
        .arg(&bin)
        .arg(&src)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "cc: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755));
    bin
}

fn seat_out(daemon: &Path, config: &Path, extra: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(daemon);
    cmd.arg(env!("CARGO_BIN_EXE_ljos"))
        .arg("seat")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", config)
        .env("XDG_CONFIG_HOME", config)
        .env("XDG_STATE_HOME", config.join("state"))
        .env("XDG_RUNTIME_DIR", config.join("run"))
        .env("USER", "probe-login");
    seat_env::clear(&mut cmd);
    for (key, value) in extra {
        cmd.env(key, value);
    }
    let out = cmd.output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    String::from_utf8(out.stdout).unwrap()
}

fn field<'a>(text: &'a str, name: &str) -> &'a str {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{name}\t")))
        .unwrap_or_else(|| panic!("no {name} in {text}"))
}

/// Nothing but `LJOS_SEAT` or a call's seat argument names the seat.
#[test]
fn only_a_named_seat_or_a_call_argument_names_the_seat() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    std::fs::create_dir_all(config.join("ljos")).unwrap();
    let id = "bc-1234abcd-rest";
    std::fs::write(
        config.join("ljos/seats.toml"),
        format!("{id:?} = \"rgoswami\"\n"),
    )
    .unwrap();
    let daemon = exec_daemon(dir.path());

    let unnamed = seat_out(
        &daemon,
        &config,
        &[("CURSOR_CONVERSATION_ID", id), ("LJOS_SEAT", "")],
    );
    assert_eq!(field(&unnamed, "seat"), "rgoswami", "{unnamed}");
    assert!(unnamed.contains("seats.toml"), "{unnamed}");
    assert!(!unnamed.contains("exec-daemon"), "{unnamed}");
    assert!(!unnamed.contains("process tree"), "{unnamed}");

    let named = seat_out(&daemon, &config, &[("LJOS_SEAT", "saddle")]);
    assert_eq!(field(&named, "seat"), "saddle", "{named}");
    assert!(field(&named, "source").contains("LJOS_SEAT"), "{named}");

    let mut isolated: std::collections::BTreeSet<String> = [
        "LJOS_SEAT",
        "VISSUE_AGENT",
        "LJOS_HOLD",
        "VISSUE_ROOT",
        "ISSUE_ROOT",
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    isolated.extend(seat_env::KEYS.iter().map(|key| (*key).to_string()));
    isolated.extend(std::env::vars_os().filter_map(|(key, _)| {
        let key = key.into_string().ok()?;
        (key.ends_with("_SESSION_ID")
            || key.ends_with("_THREAD_ID")
            || key.ends_with("_CONVERSATION_ID"))
        .then_some(key)
    }));
    let saved: Vec<_> = isolated
        .into_iter()
        .map(|key| {
            let value = std::env::var_os(&key);
            (key, value)
        })
        .collect();
    let tracker = dir.path().join("tracker");
    std::fs::create_dir_all(&tracker).unwrap();
    let state = dir.path().join("state");
    let runtime = dir.path().join("runtime");
    for path in [&state, &runtime] {
        std::fs::create_dir_all(path).unwrap();
    }
    let shared = ljos_cli::runtime_fallback(
        &std::env::temp_dir(),
        Some(&config),
        // SAFETY: geteuid has no arguments and reads the process identity.
        unsafe { libc::geteuid() },
    );
    std::fs::create_dir_all(&shared).unwrap();
    let intruder = shared.join(format!(
        "seat-{}",
        // SAFETY: getppid has no arguments and reads the parent process id.
        unsafe { libc::getppid() }
    ));
    let old_intruder = std::fs::read(&intruder).ok();
    std::fs::write(&intruder, "intruder\nintruder\n").unwrap();
    unsafe {
        for (key, _) in &saved {
            ljos_cli::test_env_guards::remove_var_unchecked(key);
        }
        ljos_cli::test_env_guards::remove_var_unchecked("VISSUE_AGENT");
        ljos_cli::test_env_guards::remove_var_unchecked("LJOS_HOLD");
        ljos_cli::test_env_guards::set_var_unchecked("HOME", &config);
        ljos_cli::test_env_guards::set_var_unchecked("XDG_CONFIG_HOME", &config);
        ljos_cli::test_env_guards::set_var_unchecked("XDG_STATE_HOME", &state);
        ljos_cli::test_env_guards::set_var_unchecked("XDG_RUNTIME_DIR", &runtime);
        ljos_cli::test_env_guards::set_var_unchecked("VISSUE_ROOT", &tracker);
        ljos_cli::test_env_guards::set_var_unchecked("ISSUE_ROOT", &tracker);
    }
    let missing = ljos_cli::resolve_call_seat(None);
    let from_arg = ljos_cli::resolve_call_seat(Some("inky"));
    unsafe { ljos_cli::test_env_guards::set_var_unchecked("LJOS_SEAT", "saddle") };
    let from_env = ljos_cli::resolve_call_seat(None);
    let explicit = ljos_cli::resolve_call_seat(Some("inky"));
    unsafe {
        for (key, value) in saved {
            match value {
                Some(value) => ljos_cli::test_env_guards::set_var_unchecked(&key, value),
                None => ljos_cli::test_env_guards::remove_var_unchecked(&key),
            }
        }
    }
    match old_intruder {
        Some(bytes) => std::fs::write(&intruder, bytes).unwrap(),
        None => std::fs::remove_file(&intruder).unwrap(),
    }
    let missing = missing.expect("a missing seat still resolves");
    assert!(
        missing.source.contains("no seat rule matched") || missing.seat.starts_with("unknown-"),
        "{missing:?}"
    );
    assert_ne!(missing.seat, "intruder", "{missing:?}");
    assert_eq!(from_arg.expect("the call's seat").seat, "inky");
    assert_eq!(from_env.expect("LJOS_SEAT").seat, "saddle");
    assert_eq!(explicit.expect("the explicit seat").seat, "inky");
}
