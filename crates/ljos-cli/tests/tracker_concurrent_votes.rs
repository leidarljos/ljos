//! Parallel persona votes and notes on one tracker checkout. On the box
//! they met on `.git/index.lock`, hashed a file another seat was still
//! writing, and refused commits another seat had already made
//! (ljos-omg1). Every write here is a real one: `vissue` is this test
//! binary run as a shim over vissue-core, the tracker is a git checkout,
//! and `LJOS_TRACKER_GIT=commit` commits each write.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

const PERSONAS: [&str; 8] = [
    "academic",
    "finance",
    "rust-perf",
    "rust-idiom",
    "docs",
    "ops",
    "security",
    "ux",
];
const NOTERS: [&str; 4] = ["saddle", "bridle", "stirrup", "girth"];

/// `vissue ARGS` over vissue-core, for the stub below. Not a test: it
/// returns at once unless the stub ran it.
#[test]
#[ignore = "run by the vissue stub"]
fn vissue_shim() {
    let (Ok(out), Ok(args)) = (
        std::env::var("LJOS_SHIM_OUT"),
        std::env::var("LJOS_SHIM_ARGS"),
    ) else {
        return;
    };
    let args: Vec<&str> = args
        .strip_suffix('\x1f')
        .unwrap_or(&args)
        .split('\x1f')
        .collect();
    let layout = vissue_core::Layout::resolve(None, None).unwrap();
    let agent = std::env::var("VISSUE_AGENT").unwrap_or_else(|_| "seat".into());
    let flag = |name: &str| {
        args.iter()
            .position(|a| *a == name)
            .and_then(|i| args.get(i + 1).copied())
    };
    let said = match args.as_slice() {
        ["vote", id, rest @ ..] if rest.contains(&"--json") => {
            let rows: Vec<serde_json::Value> = vissue_core::ops::ballots(&layout, id)
                .unwrap()
                .iter()
                .map(|b| serde_json::json!({"agent": b.agent, "choice": b.choice}))
                .collect();
            serde_json::to_string(&rows).unwrap()
        }
        ["vote", id, ..] if flag("--for").is_some() => vissue_core::ops::vote_with(
            &layout,
            id,
            flag("--for"),
            &agent,
            flag("--used"),
            flag("--confidence"),
        )
        .unwrap(),
        ["vote", id] => vissue_core::ops::vote(&layout, id, None, &agent).unwrap(),
        ["note", id, text @ ..] => vissue_core::ops::note(&layout, id, &text.join(" ")).unwrap(),
        ["show", id, ..] => vissue_core::agent::show_json(&layout, id)
            .unwrap()
            .to_string(),
        _ => panic!("the shim does not run vissue {args:?}"),
    };
    std::fs::write(out, said).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

struct Box {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    home: PathBuf,
    tracker: PathBuf,
}

impl Box {
    fn new(issues: &[&str]) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let home = dir.path().join("home");
        let tracker = dir.path().join("tracker");
        for d in [&bin, &home, &tracker] {
            std::fs::create_dir_all(d).unwrap();
        }
        let shim = bin.join("vissue");
        std::fs::write(
            &shim,
            format!(
                "#!/bin/sh\nout=\"$(mktemp)\"\nLJOS_SHIM_OUT=\"$out\" LJOS_SHIM_ARGS=\"$(printf '%s\\037' \"$@\")\" '{}' --exact vissue_shim --ignored --test-threads=1 --nocapture >\"$out.log\" 2>&1\ncode=$?\ncat \"$out\"\n[ $code -eq 0 ] || cat \"$out.log\" >&2\nrm -f \"$out\" \"$out.log\"\nexit $code\n",
                std::env::current_exe().unwrap().display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();

        git(&tracker, &["init", "-q"]);
        git(&tracker, &["config", "user.email", "seat@example.invalid"]);
        git(&tracker, &["config", "user.name", "seat"]);
        git(&tracker, &["config", "core.hooksPath", "/dev/null"]);
        let board = tracker.join("Software/probe");
        std::fs::create_dir_all(&board).unwrap();
        let mut org = String::from("#+TITLE: probe issues\n#+TODO: TODO STARTED | DONE\n\n");
        for id in issues {
            org.push_str(&format!(
                "* TODO [#C] Pick a rail for {id}\n:PROPERTIES:\n:ID:         {id}\n:END:\n"
            ));
        }
        std::fs::write(board.join("issues.org"), org).unwrap();
        git(&tracker, &["add", "."]);
        git(&tracker, &["commit", "-q", "-m", "seed"]);
        Self {
            _dir: dir,
            bin,
            home,
            tracker,
        }
    }

    fn ljos(&self, seat: &str, agent: &str, session: &str, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(&self.tracker)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("PACKSET_URL", "off")
            .env("LJOS_TRACKER_GIT", "commit")
            .env("LJOS_SEAT", seat)
            .env("VISSUE_AGENT", agent)
            .env("SEAT_SESSION_ID", session)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }
}

/// The heading in `org` whose `:ID:` is `id`, up to the next heading.
fn heading_of<'a>(org: &'a str, id: &str) -> &'a str {
    let at = org
        .find(&format!(":ID:         {id}\n"))
        .unwrap_or_else(|| panic!("{id} is not in the board:\n{org}"));
    let start = org[..at].rfind("\n* ").map_or(0, |i| i + 1);
    let end = org[at..].find("\n* ").map_or(org.len(), |i| at + i + 1);
    &org[start..end]
}

fn text(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn parallel_persona_votes_and_notes_all_land_in_one_tracker() {
    let issues = ["probe-r001", "probe-r002", "probe-r003"];
    let world = Box::new(&issues);
    for issue in issues {
        let mut runs: Vec<(String, Child)> = Vec::new();
        for (i, persona) in PERSONAS.iter().enumerate() {
            let choice = if i % 3 == 0 { "bus" } else { "rail" };
            let session = format!("sess-{issue}-{persona}");
            let child = world
                .ljos(
                    persona,
                    persona,
                    &session,
                    &[
                        "vote", issue, "--for", choice, "--used", "none", "--as", persona,
                    ],
                )
                .spawn()
                .unwrap();
            runs.push((format!("vote {persona}"), child));
        }
        for seat in NOTERS {
            let session = format!("sess-{issue}-{seat}");
            let child = world
                .ljos(
                    seat,
                    seat,
                    &session,
                    &["note", issue, &format!("{seat} read the rail costs")],
                )
                .spawn()
                .unwrap();
            runs.push((format!("note {seat}"), child));
        }
        for (what, child) in runs {
            let out = child.wait_with_output().unwrap();
            let said = text(&out);
            assert!(out.status.success(), "{issue} {what}: {said}");
            assert!(
                said.contains("tracker git: committed")
                    || said.contains("tracker git: nothing to commit"),
                "{issue} {what}: {said}"
            );
            for bad in ["refused", "index.lock", "unstable object", "fatal:"] {
                assert!(!said.contains(bad), "{issue} {what}: {said}");
            }
        }
    }

    let dir = &world.tracker;
    assert!(
        !dir.join(".git/index.lock").exists(),
        "an index.lock was left"
    );
    let dirty = git(dir, &["status", "--porcelain", "--untracked-files=no"]);
    assert!(dirty.is_empty(), "uncommitted tracker writes:\n{dirty}");
    let excluded = git(dir, &["status", "--porcelain", "--", ".ljos-commit.lock"]);
    assert!(
        excluded.is_empty(),
        "the commit lock shows in status: {excluded}"
    );
    let head = git(dir, &["show", "HEAD:Software/probe/issues.org"]);
    for issue in issues {
        let section = heading_of(&head, issue);
        for persona in PERSONAS {
            assert!(
                section.contains(&format!("ballot-sig: {persona} ")),
                "{issue}: {persona}'s signed ballot is not committed:\n{section}"
            );
        }
        for seat in NOTERS {
            assert!(
                section.contains(&format!("{seat} read the rail costs")),
                "{issue}: {seat}'s note is not committed:\n{section}"
            );
        }
        let tally = world
            .ljos("clerk", "clerk", "sess-reader-0001", &["vote", issue])
            .output()
            .unwrap();
        let said = text(&tally);
        assert!(tally.status.success(), "{said}");
        assert!(
            String::from_utf8_lossy(&tally.stdout)
                .contains(&format!("{} ballots count", PERSONAS.len())),
            "{issue}: {said}"
        );
    }
}
