//! Hyphen-leading memory, search, preference and mail text reaches the real writer.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::process::{Output, Stdio};

struct Seat {
    root: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn new(url: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        for sub in [
            "home/.config",
            "home/.local/state",
            "home/.local/share",
            "run",
        ] {
            std::fs::create_dir_all(root.path().join(sub)).unwrap();
        }
        Self {
            root,
            url: url.to_string(),
        }
    }

    fn run(&self, seat: &str, args: &[&str]) -> Output {
        let home = self.root.path().join("home");
        ljos_cli::program(env!("CARGO_BIN_EXE_ljos"))
            .unwrap()
            .args(args)
            .current_dir(self.root.path())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("LJOS_SEAT", seat)
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "hyphen-text")
            .env("PACKSET_AUTH", "off")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn success(out: &Output, args: &[&str]) -> String {
    let said = format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{args:?}\n{said}");
    said
}

#[test]
fn memory_search_preference_and_mail_keep_hyphen_leading_text() {
    if tools::halt(tools::Tool::Packsetd) {
        return;
    }
    let daemon = ljos_test_boot::Packsetd::open();
    let seat = Seat::new(daemon.url());

    let remembered = ["remember", "--", "-remembered kiln fact"];
    success(&seat.run("alice", &remembered), &remembered);
    let searched = ["search", "--", "-remembered"];
    let page = success(&seat.run("alice", &searched), &searched);
    assert!(page.contains("-remembered kiln fact"), "{page}");

    let preferred = ["prefer", "--", "-prefer quiet output"];
    success(&seat.run("alice", &preferred), &preferred);
    let preference_search = ["search", "--", "-prefer"];
    let page = success(&seat.run("alice", &preference_search), &preference_search);
    assert!(page.contains("-prefer quiet output"), "{page}");

    let sent = ["send", "bob", "--new-seat", "--", "-message from alice"];
    success(&seat.run("alice", &sent), &sent);
    let inbox = ["inbox"];
    let page = success(&seat.run("bob", &inbox), &inbox);
    assert!(page.contains("-message from alice"), "{page}");
}
