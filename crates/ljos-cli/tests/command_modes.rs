#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

fn reading(mode: Option<&str>) -> Output {
    let home = tempfile::tempdir().expect("fixture home");
    let policy = home.path().join("policy-fixture");
    std::fs::write(
        &policy,
        "#!/bin/sh\nprintf 'deny\\tfixture-command-rule\\n'\n",
    )
    .expect("fixture policy");
    std::fs::set_permissions(&policy, std::fs::Permissions::from_mode(0o700))
        .expect("fixture executable");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ljos"));
    command
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("LJOS_GUARD", "on")
        .env_remove("LJOS_OFF")
        .env_remove("LJOS_GUARD_MODE")
        .env("POLICYD_BIN", policy)
        .args([
            "policy",
            "--fail-on-deny",
            "--",
            "printf",
            "fixture-command",
        ]);
    if let Some(mode) = mode {
        command.env("LJOS_GUARD_MODE", mode);
    }
    command.output().expect("policy reading")
}

#[test]
fn an_unset_mode_runs_the_advisory_contract() {
    let out = reading(None);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{out:?}");
    assert!(text.contains("warning"), "{text}");
    assert!(text.contains("fixture-command-rule"), "{text}");
}

#[test]
fn an_explicit_advisory_mode_keeps_the_same_contract() {
    let out = reading(Some("advisory"));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{out:?}");
    assert!(text.contains("warning"), "{text}");
}

#[test]
fn an_explicit_strict_mode_retains_the_refusal() {
    let out = reading(Some("strict"));
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(text.contains("deny"), "{text}");
    assert!(text.contains("fixture-command-rule"), "{text}");
}
