//! The `[mail]` table is optional and layered like the rest of ljos config.

use std::fs;
use std::path::PathBuf;

use ljos_cli::mailbox::config::{load_from, MailConfig};

fn layer(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    path
}

#[test]
fn no_mail_table_anywhere_is_no_mail_config() {
    let dir = tempfile::tempdir().unwrap();
    let other = layer(&dir, "ljos.toml", "[git_hooks]\nskip = []\n");
    let missing = dir.path().join("absent.toml");
    assert_eq!(load_from(&[missing, other]).unwrap(), None);
    assert_eq!(load_from(&[]).unwrap(), None);
}

#[test]
fn a_later_layer_overrides_only_the_keys_it_sets() {
    let dir = tempfile::tempdir().unwrap();
    let first = layer(
        &dir,
        "first.toml",
        "[mail]\nidentity = \"owner@example.test\"\n\
         deny_file = \"/etc/ljos/mailgate/deny.txt\"\n\
         notmuch_config = \"/srv/mail/notmuch-config\"\n\
         attribution = [\"drafted by a helper\"]\n",
    );
    let second = layer(
        &dir,
        "second.toml",
        "[panel]\nmax_rounds = 3\n\n[mail]\nidentity = \"other@example.test\"\nnotmuch = \"notmuch\"\n",
    );
    let got = load_from(&[first, second]).unwrap().expect("a mail config");
    assert_eq!(
        got,
        MailConfig {
            notmuch_config: Some(PathBuf::from("/srv/mail/notmuch-config")),
            notmuch: Some(PathBuf::from("notmuch")),
            identity: Some("other@example.test".into()),
            deny_file: Some(PathBuf::from("/etc/ljos/mailgate/deny.txt")),
            attribution: vec!["drafted by a helper".into()],
        }
    );
}

#[test]
fn an_empty_mail_table_is_a_config_with_nothing_set() {
    let dir = tempfile::tempdir().unwrap();
    let only = layer(&dir, "ljos.toml", "[mail]\n");
    assert_eq!(load_from(&[only]).unwrap(), Some(MailConfig::default()));
}

#[test]
fn an_unknown_mail_key_is_an_error_that_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let typo = layer(&dir, "typo.toml", "[mail]\ndeny_fiel = \"/etc/deny.txt\"\n");
    let err = load_from(&[typo]).unwrap_err().to_string();
    assert!(err.contains("typo.toml"), "{err}");
}

#[test]
fn a_file_that_does_not_parse_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let broken = layer(&dir, "broken.toml", "[mail\nidentity = 1\n");
    let err = load_from(&[broken]).unwrap_err().to_string();
    assert!(err.contains("broken.toml"), "{err}");
}

#[test]
fn relative_paths_and_odd_identities_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    for text in [
        "[mail]\ndeny_file = \"deny.txt\"\n",
        "[mail]\nnotmuch_config = \"cfg\"\n",
        "[mail]\nnotmuch = \"bin/notmuch\"\n",
        "[mail]\nidentity = \"Owner <owner@example.test>\"\n",
        "[mail]\nidentity = \"owner\"\n",
    ] {
        let path = layer(&dir, "bad.toml", text);
        assert!(load_from(&[path]).is_err(), "accepted {text}");
    }
}
