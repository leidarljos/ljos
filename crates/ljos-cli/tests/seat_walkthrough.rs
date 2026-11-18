//! The first sitting of a new seat, start to finish, on a fresh tracker.
//!
//! The seat is `testseat`, set by `LJOS_SEAT`. Nothing runs `ljos onboard`,
//! `ljos install` or a git hook installer. The tracker is a directory with a
//! `vissue.toml` and a `Software/` directory, the deed store is an empty
//! directory, and the pack is a local `packsetd` on a port the kernel picked.
//! No step reaches the network.
//!
//! The steps are the ones README and getting-started print: search, file a
//! ticket, open a sitting, note, remember one sentence and then two, ready,
//! show, and finish without `--close`. A tool the walk runs and does not
//! stub is on `PATH` through `source scripts/sibling-tools.sh`. When one is
//! missing the test prints one line and returns, and
//! `LJOS_TEST_REQUIRE_TOOLS=1` fails it instead.

#[path = "common/boot.rs"]
mod boot;
#[path = "common/tools.rs"]
mod tools;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SEAT: &str = "testseat";
const ONE: &str = "Dry the kiln door seal before each firing.";
const TWO: &str = "Wipe the seal with a dry cloth. Check the hinge for rust.";
const NOTE: &str = "the door sticks when damp";

const NOISE: &[&str] = &[
    "panicked at",
    "stack backtrace",
    "Caused by:",
    "anyhow::",
    "called `Result",
    "called `Option",
    "Os {",
    "Error {",
];

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn tools_ready() -> bool {
    if tools::halt(tools::Tool::Vissue) || tools::halt(tools::Tool::Packsetd) {
        return false;
    }
    for name in ["claimdag", "deedar"] {
        if which(name).is_none() {
            let line = format!("{name} is not on PATH, so run `source scripts/sibling-tools.sh`.");
            if std::env::var(tools::REQUIRE_TOOLS).ok().as_deref() == Some("1") {
                panic!("{line}");
            }
            eprintln!("{line}");
            return false;
        }
    }
    true
}

struct Step {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Step {
    fn both(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

struct Walk {
    root: tempfile::TempDir,
    url: String,
    log: String,
}

impl Walk {
    fn open(url: &str, with_store: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        for sub in [
            "home/.config",
            "home/.local/share",
            "home/.local/state",
            "bin",
            "run",
            "tracker/Software",
        ] {
            std::fs::create_dir_all(root.path().join(sub)).unwrap();
        }
        std::fs::write(
            root.path().join("tracker/vissue.toml"),
            "prefix = \"Software\"\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                root.path().join("run"),
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
        if with_store {
            // An empty directory is a deed store. The first deed writes its layout.
            let store = home.join(".local/share/deedar/store");
            std::fs::create_dir_all(&store).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        for name in ["vissue", "claimdag", "deedar", "packset", "packsetd"] {
            let path = which(name).unwrap_or_else(|| panic!("{name} is not on PATH"));
            std::os::unix::fs::symlink(path, root.path().join("bin").join(name)).unwrap();
        }
        Self {
            root,
            url: url.to_string(),
            log: String::new(),
        }
    }

    fn run(&mut self, args: &[&str]) -> Step {
        let root = self.root.path();
        let home = root.join("home");
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(&home)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", root.join("bin").display()),
            )
            .env("HOME", &home)
            .env("USER", "walker")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("TMPDIR", root)
            .env("VISSUE_ROOT", root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", SEAT)
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_NO_AUTOSTART", "1")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("NO_COLOR", "1")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .stdin(Stdio::null())
            .output()
            .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")));
        let step = Step {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        };
        self.log.push_str(&format!(
            "$ ljos {}\nexit {}\n{}{}\n",
            args.join(" "),
            step.code,
            step.stdout,
            step.stderr
        ));
        for needle in NOISE {
            assert!(
                !step.both().contains(needle),
                "ljos {} printed `{needle}`:\n{}",
                args.join(" "),
                self.log
            );
        }
        step
    }

    fn ok(&mut self, args: &[&str]) -> Step {
        let step = self.run(args);
        assert_eq!(
            step.code,
            0,
            "ljos {} failed:\n{}",
            args.join(" "),
            self.log
        );
        step
    }

    fn expect(&self, text: &str, needle: &str, what: &str) {
        assert!(
            text.contains(needle),
            "{what}: no `{needle}` in:\n{text}\n--- walk so far ---\n{}",
            self.log
        );
    }
}

fn lesson_id(stdout: &str) -> String {
    let line = stdout
        .lines()
        .find(|line| line.trim_start().starts_with("id "))
        .unwrap_or_else(|| panic!("no id line in:\n{stdout}"));
    line.trim_start()
        .trim_start_matches("id ")
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect()
}

fn no_leftovers(root: &Path) {
    for name in [".git", "hooks", ".claude", ".cursor", ".codex"] {
        assert!(
            !root.join("home").join(name).exists(),
            "the walk wrote ~/{name}, which only an installer or onboarding writes"
        );
    }
}

#[test]
fn a_new_seat_walks_from_search_to_finish_without_close() {
    if !tools_ready() {
        return;
    }
    let pack = ljos_test_boot::Packsetd::open();
    let mut walk = Walk::open(pack.url(), true);

    let miss = walk.ok(&["search", "kiln door"]);
    walk.expect(
        &miss.stdout,
        "Nothing matched \"kiln door\"",
        "search on an empty pack",
    );

    let filed = walk.ok(&["file", "Seat the kiln door", "-p", "demo"]);
    let id = filed.stdout.trim().to_string();
    assert!(
        id.starts_with("demo-") && !id.contains(char::is_whitespace),
        "the id is the only line on stdout: {:?}\n{}",
        filed.stdout,
        walk.log
    );
    walk.expect(&filed.stderr, "project: demo", "file");
    walk.expect(&filed.stderr, &format!("{id}  TODO"), "file");
    walk.expect(&filed.stderr, "issues.org", "file");

    let sitting = walk.ok(&["sitting", &id]);
    walk.expect(
        &sitting.stdout,
        &format!("tracker: {id} STARTED under {SEAT}"),
        "sitting",
    );
    walk.expect(
        &sitting.stdout,
        &format!("`ljos finish {id} --lesson \"...\"`"),
        "sitting names its close",
    );

    let note = walk.ok(&["note", NOTE]);
    walk.expect(&note.stdout, &format!("{id}: noted"), "note");

    let one = walk.ok(&["remember", ONE]);
    walk.expect(
        &one.stdout,
        &format!("remembered (lesson): {ONE}"),
        "remember, one sentence",
    );
    let one_id = lesson_id(&one.stdout);
    assert_eq!(one_id.len(), 8, "{}", walk.log);

    let two = walk.ok(&["remember", TWO]);
    walk.expect(
        &two.stdout,
        &format!("remembered (lesson): {TWO}"),
        "remember, two sentences",
    );
    let two_ids: Vec<_> = two
        .stdout
        .lines()
        .filter(|line| line.trim_start().starts_with("id "))
        .collect();
    assert_eq!(
        two_ids.len(),
        1,
        "two sentences are one lesson and one printed id:\n{}",
        walk.log
    );
    let two_id = lesson_id(&two.stdout);

    let ready = walk.ok(&["ready"]);
    walk.expect(&ready.stdout, "ready  shown 1 of 1", "ready");
    walk.expect(
        &ready.stdout,
        &format!("{id}  STARTED"),
        "ready lists the claimed ticket",
    );
    walk.expect(&ready.stdout, "claimed", "ready names the claim");
    walk.expect(&ready.stdout, SEAT, "ready names the seat");

    let shown = walk.ok(&["show", &id]);
    walk.expect(&shown.stdout, &format!("{id}  STARTED"), "show");
    walk.expect(&shown.stdout, "Seat the kiln door", "show");
    walk.expect(&shown.stdout, NOTE, "show carries the note");

    let finish = walk.ok(&["finish", &id]);
    walk.expect(
        &finish.both(),
        &format!("sitting ended; ticket {id} is still open"),
        "finish without --close",
    );

    let after = walk.ok(&["show", &id]);
    walk.expect(
        &after.stdout,
        &format!("{id}  TODO"),
        "the ticket is open and unclaimed",
    );
    let ready = walk.ok(&["ready"]);
    walk.expect(&ready.stdout, &format!("{id}  TODO"), "ready after finish");

    let found = walk.ok(&["search", "seal hinge"]);
    walk.expect(
        &found.stdout,
        &two_id,
        "search finds the two-sentence lesson",
    );
    let found = walk.ok(&["search", "firing"]);
    walk.expect(
        &found.stdout,
        &one_id,
        "search finds the one-sentence lesson",
    );
    let kept = walk.ok(&["show", &two_id]);
    walk.expect(&kept.stdout, TWO, "show opens the stored lesson");

    no_leftovers(walk.root.path());
}
