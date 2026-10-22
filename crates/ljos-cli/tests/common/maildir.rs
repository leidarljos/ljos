//! A synthetic Maildir and notmuch database built from the fixtures.
//!
//! notmuch must be installed. A missing notmuch fails the test: a mail
//! test that did not run is not a pass.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Mailbox {
    pub dir: tempfile::TempDir,
    pub config: PathBuf,
}

impl Mailbox {
    pub fn maildir(&self) -> PathBuf {
        self.dir.path().join("Mail")
    }

    pub fn notmuch(&self, args: &[&str]) -> String {
        let out = Command::new("notmuch")
            .env("NOTMUCH_CONFIG", &self.config)
            .args(args)
            .output()
            .expect("notmuch is installed");
        assert!(out.status.success(), "notmuch {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    }

    /// The thread id of the message with this Message-ID.
    pub fn thread_of(&self, message_id: &str) -> String {
        let query = format!("id:{message_id}");
        self.notmuch(&["search", "--output=threads", "--", &query])
            .trim()
            .trim_start_matches("thread:")
            .to_string()
    }
}

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mail")
}

/// Every fixture delivered, indexed, and `04-spam.eml` tagged spam.
pub fn mailbox() -> Mailbox {
    let dir = tempfile::tempdir().unwrap();
    let maildir = dir.path().join("Mail");
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(maildir.join(sub)).unwrap();
    }
    let mut names: Vec<_> = std::fs::read_dir(fixtures())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "eml"))
        .collect();
    names.sort();
    for (n, path) in names.iter().enumerate() {
        std::fs::copy(path, maildir.join("new").join(format!("{n}.fixture:2,"))).unwrap();
    }
    let config = dir.path().join("notmuch-config");
    std::fs::write(
        &config,
        format!(
            "[database]\npath={}\n[user]\nname=Owner\nprimary_email=owner@example.test\n\
             [new]\ntags=inbox;unread\n[search]\nexclude_tags=\n",
            maildir.display()
        ),
    )
    .unwrap();
    let mailbox = Mailbox { dir, config };
    mailbox.notmuch(&["new", "--quiet"]);
    mailbox.notmuch(&["tag", "+spam", "--", "id:spam-1@spam.example.test"]);
    mailbox
}
