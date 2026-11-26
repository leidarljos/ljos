//! One deadline for an external call.
//!
//! A child of vissue, git, packset, deedar, claimdag, or consensus, and a
//! lock taken the same way, stops when this runs out. `LJOS_CALL_TIMEOUT`
//! is whole seconds. Unset is 60. A few seconds in, one line names the
//! part still running. When the time is up the child and its process group
//! are killed, one line names the part and what was not written, and the
//! process exits non-zero.
//!
//! A hook, a sitting, and a tool call keep the shorter budget they already
//! armed. Pack sockets keep theirs too.

use std::time::{Duration, Instant};

/// The bound when `LJOS_CALL_TIMEOUT` is unset.
pub const DEFAULT_SECS: u64 = 60;

/// How long a quiet call runs before one `waiting on <part>...` line.
pub const NOTICE: Duration = Duration::from_secs(3);

/// How long one external call may run.
#[must_use]
pub fn timeout() -> Duration {
    let secs = std::env::var("LJOS_CALL_TIMEOUT")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_SECS);
    Duration::from_secs(secs)
}

/// The program name, without a directory.
#[must_use]
pub fn base(program: &str) -> &str {
    std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
}

/// What the `waiting on` line names. A known part, or the program itself.
#[must_use]
pub fn waiting_label(program: &str) -> String {
    let named = part(program);
    if named == "command" {
        base(program).to_string()
    } else {
        named.to_string()
    }
}

/// The part a person waits on.
#[must_use]
pub fn part(program: &str) -> &'static str {
    match base(program) {
        "vissue" => "tracker",
        "git" => "git",
        "packset" | "packsetd" => "pack",
        "deedar" => "deed store",
        "claimdag" => "claim graph",
        "ljos-consensus" => "consensus",
        "lock" => "lock",
        "age" | "age-keygen" | "rage" | "rage-keygen" => "age",
        _ => "command",
    }
}

fn arg(args: &[impl AsRef<str>], index: usize) -> &str {
    args.get(index).map(AsRef::as_ref).unwrap_or("")
}

/// The git verb, skipping the options that name a directory or a config.
fn git_verb(args: &[impl AsRef<str>]) -> &str {
    let mut index = 0;
    while index < args.len() {
        match arg(args, index) {
            "-C" | "--git-dir" | "--work-tree" | "--namespace" | "-c" => index += 2,
            word if word.starts_with('-') => index += 1,
            word => return word,
        }
    }
    ""
}

/// What this call did not finish.
#[must_use]
pub fn unfinished(program: &str, args: &[impl AsRef<str>]) -> &'static str {
    let verb = match base(program) {
        "git" => git_verb(args),
        _ => arg(args, 0),
    };
    match (base(program), verb) {
        ("vissue", "note") => "the note was not written, so run it again",
        ("vissue", "update" | "create" | "append" | "edit") => {
            "the tracker was not written, so run it again"
        }
        ("vissue", _) => "nothing was written",
        ("git", "commit" | "add" | "merge" | "rebase" | "cherry-pick" | "am") => {
            "the commit was not written, so run it again"
        }
        ("git", "push") => "the push was not finished, so run it again",
        ("git", _) => "nothing was written",
        ("claimdag", "get" | "list" | "ready" | "") => "nothing was written",
        ("claimdag", _) => "the claim was not written, so run it again",
        ("deedar", "current" | "log" | "evidence" | "show" | "") => "nothing was written",
        ("deedar", "host") if arg(args, 1) == "accept" => {
            "the deed was not written, so run it again"
        }
        ("deedar", "host") => "nothing was written",
        ("deedar", _) => "the deed was not written, so run it again",
        ("packset" | "packsetd", "ensure" | "start" | "serve") => {
            "the pack was not started, so run it again"
        }
        ("packset" | "packsetd", _) => "nothing was written",
        ("ljos-consensus", _) => "nothing was written",
        ("age" | "age-keygen" | "rage" | "rage-keygen", _) => {
            "the seal was not written, so run it again"
        }
        ("lock", _) => "nothing was written, so run it again",
        _ => "nothing was written, so run it again",
    }
}

/// One line for a call that ran out of time.
#[must_use]
pub fn stall_line(program: &str, args: &[impl AsRef<str>], limit: Duration) -> String {
    let when = match limit.as_secs() {
        0 => "time".to_string(),
        secs => format!("{secs}s"),
    };
    format!(
        "{} did not answer in {when}; {}",
        part(program),
        unfinished(program, args)
    )
}

/// Whether `text` is a call that ran out of time.
///
/// The old phrase `did not answer in time` and `did not answer in 60s`
/// are the same failure. A sitting and a finish tell them apart from a
/// missing program by this.
#[must_use]
pub fn stalled(text: &str) -> bool {
    text.contains("did not answer in ")
}

/// Print `waiting on <part>...` once, after [`NOTICE`].
pub fn note_waiting(part: &str, started: Instant, noted: &mut bool) {
    if *noted || started.elapsed() < NOTICE {
        return;
    }
    say!("waiting on {part}...");
    *noted = true;
}

/// Print `line` and exit 1. The child is already dead.
pub fn stop(line: &str) -> ! {
    say!("{line}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_names_the_tracker_and_the_unwritten_note() {
        let _env_read = crate::test_env_read_lock();
        let line = stall_line(
            "vissue",
            &["note", "demo", "hello"],
            Duration::from_secs(60),
        );
        assert_eq!(
            line,
            "tracker did not answer in 60s; the note was not written, so run it again"
        );
    }

    #[test]
    fn each_part_names_what_it_did_not_write() {
        let _env_read = crate::test_env_read_lock();
        let secs = Duration::from_secs(60);
        assert_eq!(
            stall_line("git", &["-C", "/tmp/x", "commit", "-m", "n"], secs),
            "git did not answer in 60s; the commit was not written, so run it again"
        );
        assert_eq!(
            stall_line(
                "git",
                &[
                    "-c",
                    "core.fsmonitor=false",
                    "-c",
                    "maintenance.auto=false",
                    "-C",
                    "/tmp/x",
                    "-c",
                    "gc.auto=0",
                    "commit",
                    "-m",
                    "n",
                ],
                secs,
            ),
            "git did not answer in 60s; the commit was not written, so run it again"
        );
        assert_eq!(
            stall_line("git", &["rev-parse", "--is-inside-work-tree"], secs),
            "git did not answer in 60s; nothing was written"
        );
        assert_eq!(
            stall_line("claimdag", &["claim", "abc"], secs),
            "claim graph did not answer in 60s; the claim was not written, so run it again"
        );
        assert_eq!(
            stall_line("claimdag", &["list"], secs),
            "claim graph did not answer in 60s; nothing was written"
        );
        assert_eq!(
            stall_line("deedar", &["current", "deed"], secs),
            "deed store did not answer in 60s; nothing was written"
        );
        assert_eq!(
            stall_line("packsetd", &["ensure"], secs),
            "pack did not answer in 60s; the pack was not started, so run it again"
        );
        assert_eq!(
            stall_line("ljos-consensus", &["settle"], secs),
            "consensus did not answer in 60s; nothing was written"
        );
        assert_eq!(
            stall_line("lock", &[] as &[&str], secs),
            "lock did not answer in 60s; nothing was written, so run it again"
        );
        assert!(stalled(&stall_line(
            "sh",
            &["-c", "sleep"],
            Duration::from_millis(400)
        )));
        assert!(stalled("sh did not answer in time"));
    }
}
