//! Two homes of one user, with `XDG_RUNTIME_DIR` unset in both.
//!
//! The runtime directory then falls back to `ljos-UID` under the temporary
//! directory. A hold is kept there by the runner's conversation id, which
//! every shell under one runner shares, so a sitting opened in one home was
//! read as the held issue in another: `ljos file` there answered
//! "held issue ... is on another tracker". The fallback is now keyed by
//! `HOME` too.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const CONVERSATION: &str = "bc-872164c1-e14f-56d0-a32e-fc0bd5afad41";

fn script(dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    ljos_cli::install_executable(&path, body).unwrap();
}

fn vissue_dir() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .find(|dir| dir.join("vissue").is_file())
        .unwrap_or_else(|| PathBuf::from("/usr/bin"))
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The temporary directory both homes share, and one home with its own
/// tracker.
struct World {
    dir: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = dir.path().join("bin");
        script(
            &bin,
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  get) echo 'not found' >&2; exit 1 ;;\n  list) echo '[]' ;;\n  *) echo 'gen=1' ;;\nesac\n",
        );
        script(&bin, "deedar", "#!/bin/sh\necho head\n");
        std::fs::create_dir_all(dir.path().join("tmp")).unwrap();
        Self { dir }
    }

    fn home(&self, name: &str) -> PathBuf {
        let home = self.dir.path().join(name);
        std::fs::create_dir_all(home.join("tracker/Software")).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = Your Name\n\temail = you@example.com\n",
        )
        .unwrap();
        home
    }

    /// `ljos` in `home`, as a shell under one Cursor conversation with no
    /// `XDG_RUNTIME_DIR` and no seat named.
    fn ljos(&self, home: &Path, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}:/usr/bin:/bin",
            self.dir.path().join("bin").display(),
            vissue_dir().display()
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(home)
            .env_clear()
            .env("PATH", path)
            .env("HOME", home)
            .env("USER", "ada")
            .env("TMPDIR", self.dir.path().join("tmp"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("CURSOR_CONVERSATION_ID", CONVERSATION)
            .env("CURSOR_AGENT", "1")
            .env("VISSUE_ROOT", home.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_NO_AUTOSTART", "1")
            .env("PACKSET_URL", "off")
            .stdin(Stdio::null())
            .output()
            .unwrap();
    }

    /// Every hold record under the shared temporary directory.
    fn hold_files(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("hold-"))
                {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.dir.path().join("tmp"), &mut out);
        out.sort();
        out
    }
}

fn filed_id(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn a_hold_in_one_home_is_not_held_in_another() {
    if tools::halt(tools::Tool::Vissue) {
        return;
    }
    let world = World::new();
    let first = world.home("first");
    let second = world.home("second");

    let filed = world.ljos(&first, &["file", "Ship the codec", "-p", "demo"]);
    assert!(filed.status.success(), "{}", said(&filed));
    let parent = filed_id(&filed);
    assert!(!parent.is_empty(), "{}", said(&filed));
    let sat = world.ljos(&first, &["sitting", &parent]);
    assert!(sat.status.success(), "{}", said(&sat));
    assert_eq!(world.hold_files().len(), 1, "{}", said(&sat));

    let child = world.ljos(&second, &["file", "Write the note", "-p", "demo"]);
    let text = said(&child);
    assert!(child.status.success(), "{text}");
    assert!(!text.contains("held issue"), "{text}");
    assert!(!text.contains(&parent), "{text}");
    assert!(!filed_id(&child).is_empty(), "{text}");

    let held = world.ljos(&first, &["file", "Measure the files", "-p", "demo"]);
    assert!(held.status.success(), "{}", said(&held));
    assert!(
        !said(&held).contains("held issue"),
        "the home that opened the sitting lost its hold:\n{}",
        said(&held)
    );
    assert_eq!(world.hold_files().len(), 1, "{}", said(&held));
}
