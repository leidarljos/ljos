//! A named `LJOS_SEAT`, or the seat argument of a call, is what names the
//! seat. A seats file and the process tree do not.

ljos_test_boot::harness!();

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
        .env("USER", "probe-login");
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
    assert_eq!(field(&unnamed, "seat"), "probe-login", "{unnamed}");
    assert!(!unnamed.contains("rgoswami"), "{unnamed}");
    assert!(!unnamed.contains("exec-daemon"), "{unnamed}");
    assert!(!unnamed.contains("seats.toml"), "{unnamed}");
    assert!(!unnamed.contains("process tree"), "{unnamed}");

    let named = seat_out(&daemon, &config, &[("LJOS_SEAT", "saddle")]);
    assert_eq!(field(&named, "seat"), "saddle", "{named}");
    assert!(field(&named, "source").contains("LJOS_SEAT"), "{named}");

    let saved: Vec<_> = [
        "LJOS_SEAT",
        "VISSUE_AGENT",
        "LJOS_HOLD",
        "VISSUE_ROOT",
        "ISSUE_ROOT",
        "HOME",
        "XDG_CONFIG_HOME",
    ]
    .into_iter()
    .map(|key| (key, std::env::var_os(key)))
    .collect();
    let tracker = dir.path().join("tracker");
    std::fs::create_dir_all(&tracker).unwrap();
    unsafe {
        std::env::set_var("HOME", &config);
        std::env::set_var("XDG_CONFIG_HOME", &config);
        std::env::set_var("VISSUE_ROOT", &tracker);
        std::env::set_var("ISSUE_ROOT", &tracker);
        std::env::remove_var("LJOS_SEAT");
        std::env::remove_var("VISSUE_AGENT");
        std::env::remove_var("LJOS_HOLD");
    }
    let missing = ljos_cli::resolve_call_seat(None);
    let from_arg = ljos_cli::resolve_call_seat(Some("inky"));
    unsafe { std::env::set_var("LJOS_SEAT", "saddle") };
    let from_env = ljos_cli::resolve_call_seat(None);
    unsafe {
        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
    let missing = missing
        .expect_err("a call with no seat was named")
        .to_string();
    assert!(
        missing.contains("LJOS_SEAT") || missing.contains("pass seat"),
        "{missing}"
    );
    assert_eq!(from_arg.expect("the call's seat").seat, "inky");
    assert_eq!(from_env.expect("LJOS_SEAT").seat, "saddle");
}
