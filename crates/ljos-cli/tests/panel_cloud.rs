//! `ljos panel ISSUE --cloud` is a flag on the panel command.

ljos_test_boot::harness!();
use std::process::Command;

fn ljos(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .output()
        .expect("ljos")
}

#[test]
fn panel_help_offers_cloud_and_refuses_to_mix_it_with_a_local_spawn() {
    let help = ljos(&["panel", "--help"]);
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(
        help.status.success(),
        "{}",
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(text.contains("--cloud"), "{text}");

    let jev = ljos(&["panel", "probe-l0g0", "--cloud", "--jev"]);
    let err = String::from_utf8_lossy(&jev.stderr);
    assert!(!jev.status.success(), "{err}");
    assert!(
        err.contains("cannot be used with") && err.contains("--jev"),
        "{err}"
    );

    let dry = ljos(&["panel", "probe-l0g0", "--cloud", "--dry-run"]);
    let err = String::from_utf8_lossy(&dry.stderr);
    assert!(!dry.status.success(), "{err}");
    assert!(err.contains("--dry-run"), "{err}");

    let designers = ljos(&["panel", "probe-l0g0", "--cloud", "--designers", "5"]);
    let err = String::from_utf8_lossy(&designers.stderr);
    assert!(!designers.status.success(), "{err}");
    assert!(
        err.contains("cannot be used with") && err.contains("--designers"),
        "{err}"
    );

    let run = ljos(&["panel", "run", "probe-l0g0", "--cloud"]);
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(!run.status.success(), "{err}");
    assert!(err.contains("does not spawn"), "{err}");
}
