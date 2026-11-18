//! Help text a newcomer reads on `--help` and `-h`.

ljos_test_boot::harness!();

use std::process::Command;

fn help(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .env("LJOS_OFF", "1")
        .env("LJOS_HOOKD", "off")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "ljos {}\n{text}{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    text
}

#[test]
fn policy_example_has_no_trailing_period() {
    for flag in ["--help", "-h"] {
        let page = help(&["policy", flag]);
        assert!(
            page.contains("Example: `ljos policy -- git push --force`\n"),
            "{flag}\n{page}"
        );
        assert!(
            !page.contains("git push --force`."),
            "{flag} example ends with a period\n{page}"
        );
    }
}
