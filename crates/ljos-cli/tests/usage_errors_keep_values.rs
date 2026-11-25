//! A usage error names the flag or the command word. It never repeats a
//! value the person typed, because a value may be a token, a path or a name.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;

use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str]) -> Output {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("TMPDIR", root)
        .env("LJOS_OFF", "1")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_URL", "off")
        .stdin(Stdio::null());
    seat_env::clear(&mut cmd);
    cmd.output().expect("run ljos")
}

const SECRETS: &[&str] = &[
    concat!("gh", "p_", "0123456789abcdefghijklmnopqrstuvwxyz"),
    "hunter2",
    "/home/rohit/private/notes.txt",
    "alice@example.org",
    "10.1.2.3",
    concat!("AK", "IA", "ABCDEFGHIJKLMNOP"),
];

#[test]
fn no_usage_error_repeats_a_typed_value() {
    let dir = tempfile::tempdir().unwrap();
    let mut cases: Vec<Vec<String>> = Vec::new();
    for secret in SECRETS {
        let flagged = format!("--limt={secret}");
        let glued = format!("-x{secret}");
        let project = format!("-p{secret}");
        cases.push(vec![secret.to_string()]);
        cases.push(vec![secret.to_string(), "kiln".into()]);
        cases.push(vec!["search".into(), flagged.clone(), "kiln".into()]);
        cases.push(vec!["search".into(), glued]);
        cases.push(vec!["search".into(), project]);
        cases.push(vec!["search".into(), "--limit".into(), secret.to_string()]);
        cases.push(vec!["search".into(), "--limit".into(), "3".into(), flagged]);
        cases.push(vec!["completions".into(), secret.to_string()]);
        cases.push(vec!["help".into(), secret.to_string()]);
        cases.push(vec!["help".into(), "search".into(), secret.to_string()]);
        cases.push(vec!["recall".into(), secret.to_string()]);
        cases.push(vec!["recall".into(), secret.to_string(), "more".into()]);
        cases.push(vec!["lessons".into(), secret.to_string()]);
        cases.push(vec!["sitting".into(), secret.to_string()]);
        cases.push(vec!["block".into(), "demo-1a2b".into(), secret.to_string()]);
        cases.push(vec!["finsh".into(), secret.to_string()]);
        cases.push(vec!["--seat".into(), secret.to_string(), "finsh".into()]);
        cases.push(vec![
            "--seat".into(),
            secret.to_string(),
            "completions".into(),
        ]);
        cases.push(vec![format!("--seat={secret}"), "completions".into()]);
    }
    let mut seen = 0;
    for args in &cases {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = run(dir.path(), &refs);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        for secret in SECRETS {
            let bare = secret.trim_start_matches('/');
            assert!(
                !said.contains(secret) && !said.contains(bare),
                "ljos {args:?} repeated a value:\n{said}"
            );
        }
        seen += 1;
    }
    assert!(seen > 100, "only {seen} cases ran");
}

#[test]
fn a_usage_error_still_names_the_flag_and_the_command_word() {
    let dir = tempfile::tempdir().unwrap();
    let flag = run(dir.path(), &["search", "--limt=hunter2", "kiln"]);
    assert_eq!(
        String::from_utf8_lossy(&flag.stderr).trim_end(),
        "ljos: search has no flag --limt, and the closest is --limit, so run `ljos search --limit=<value> <value>`."
    );
    let word = run(dir.path(), &["serch", "hunter2"]);
    assert_eq!(
        String::from_utf8_lossy(&word.stderr).trim_end(),
        "ljos: no command serch, and the closest is search, so run `ljos search <value>`."
    );
    let bad = run(dir.path(), &["search", "--limit", "hunter2"]);
    assert_eq!(
        String::from_utf8_lossy(&bad.stderr).trim_end(),
        "search: --limit does not accept that value. The values it takes are in `ljos search --help`."
    );
    let shell = run(dir.path(), &["--seat", "hunter2", "completions"]);
    assert_eq!(
        String::from_utf8_lossy(&shell.stderr).trim_end(),
        "ljos: completions needs a shell, so run `ljos --seat <value> completions bash`."
    );
    let glued = run(dir.path(), &["--seat=hunter2", "completions"]);
    assert_eq!(
        String::from_utf8_lossy(&glued.stderr).trim_end(),
        "ljos: completions needs a shell, so run `ljos --seat=<value> completions bash`."
    );
}
