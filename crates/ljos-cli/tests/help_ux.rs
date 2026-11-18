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

#[test]
fn status_and_verdict_help_start_with_a_capital() {
    for (args, line) in [
        (
            &["finish", "--help"][..],
            "Status to record: done, failed, or cancelled.",
        ),
        (
            &["finish", "-h"][..],
            "Status to record: done, failed, or cancelled.",
        ),
        (
            &["complete", "--help"][..],
            "Status to record: done, failed, or cancelled. The default is done.",
        ),
        (
            &["complete", "-h"][..],
            "Status to record: done, failed, or cancelled. The default is done.",
        ),
        (
            &["rule", "--help"][..],
            "Label the warning deny or ask.",
        ),
        (&["rule", "-h"][..], "Label the warning deny or ask."),
    ] {
        let page = help(args);
        assert!(page.contains(line), "{args:?} missing {line:?}\n{page}");
    }
    for args in [
        &["finish", "--help"][..],
        &["complete", "--help"][..],
        &["rule", "--help"][..],
    ] {
        let page = help(args);
        assert!(
            !page.contains("\n          done, failed")
                && !page.contains("\n          done (default)")
                && !page.contains("\n          deny or ask labels"),
            "{args:?} still opens a flag with a lowercase fragment\n{page}"
        );
    }
}

#[test]
fn json_flags_name_the_rows_they_print() {
    for flag in ["--help", "-h"] {
        let imports = help(&["memory", "imports", flag]);
        assert!(
            imports.contains("Print each import as JSON, the same rows as the text."),
            "memory imports {flag}\n{imports}"
        );
        let rules = help(&["rules", flag]);
        assert!(
            rules.contains("Print pattern, verdict and reason as JSON."),
            "rules {flag}\n{rules}"
        );
    }
}
