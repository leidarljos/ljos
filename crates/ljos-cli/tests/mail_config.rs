//! The `[mail]` table comes only from the user's own config files.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use ljos_cli::mailbox::config::{load_from, MailConfig};

fn layer(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    path
}

fn user(paths: &[PathBuf]) -> anyhow::Result<Option<MailConfig>> {
    load_from(paths, &[])
}

#[test]
fn no_mail_table_anywhere_is_no_mail_config() {
    let dir = tempfile::tempdir().unwrap();
    let other = layer(&dir, "ljos.toml", "[git_hooks]\nskip = []\n");
    let missing = dir.path().join("absent.toml");
    assert_eq!(user(&[missing.clone(), other.clone()]).unwrap(), None);
    assert_eq!(load_from(&[], &[missing, other]).unwrap(), None);
    assert_eq!(user(&[]).unwrap(), None);
}

#[test]
fn a_later_user_file_overrides_only_the_keys_it_sets() {
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
    let got = user(&[first, second]).unwrap().expect("a mail config");
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
fn a_workspace_layer_that_sets_mail_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mine = layer(
        &dir,
        "mail.toml",
        "[mail]\nidentity = \"owner@example.test\"\n\
         deny_file = \"/etc/ljos/mailgate/deny.txt\"\n",
    );
    let empty = layer(&dir, "empty-deny.txt", "");
    for text in [
        format!(
            "[mail]\nidentity = \"seat@example.test\"\ndeny_file = \"{}\"\n",
            empty.display()
        ),
        "[mail]\n".to_string(),
        "[mail]\nnotmuch_config = \"/tmp/other-notmuch\"\n".to_string(),
    ] {
        let swap = layer(&dir, "ljos.toml", &text);
        let err = load_from(std::slice::from_ref(&mine), &[swap])
            .unwrap_err()
            .to_string();
        assert!(err.contains("ljos.toml") && err.contains("[mail]"), "{err}");
    }
    let other = layer(&dir, "ljos.toml", "[panel]\nmax_rounds = 3\n");
    assert!(load_from(&[mine], &[other]).unwrap().is_some());
}

#[test]
fn an_unreadable_or_unparsable_layer_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let unreadable = dir.path().join("is-a-dir.toml");
    fs::create_dir(&unreadable).unwrap();
    let broken = layer(&dir, "broken.toml", "[mail\nidentity = 1\n");
    for (users, shared) in [
        (vec![unreadable.clone()], vec![]),
        (vec![], vec![unreadable.clone()]),
        (vec![broken.clone()], vec![]),
        (vec![], vec![broken.clone()]),
    ] {
        assert!(load_from(&users, &shared).is_err(), "{users:?} {shared:?}");
    }
}

#[test]
fn a_user_file_others_can_write_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = layer(&dir, "mail.toml", "[mail]\nnotmuch = \"notmuch\"\n");
    for mode in [0o620, 0o602, 0o666] {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        let err = user(std::slice::from_ref(&path)).unwrap_err().to_string();
        assert!(err.contains("writable"), "{mode:o}: {err}");
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(user(&[path]).unwrap().is_some());
}

#[test]
fn identity_without_a_deny_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let only = layer(
        &dir,
        "mail.toml",
        "[mail]\nidentity = \"owner@example.test\"\n",
    );
    let err = user(&[only]).unwrap_err().to_string();
    assert!(err.contains("deny_file"), "{err}");
}

#[test]
fn an_empty_mail_table_is_a_config_with_nothing_set() {
    let dir = tempfile::tempdir().unwrap();
    let only = layer(&dir, "mail.toml", "[mail]\n");
    assert_eq!(user(&[only]).unwrap(), Some(MailConfig::default()));
}

#[test]
fn an_unknown_mail_key_is_an_error_that_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let typo = layer(&dir, "typo.toml", "[mail]\ndeny_fiel = \"/etc/deny.txt\"\n");
    let err = user(&[typo]).unwrap_err().to_string();
    assert!(err.contains("typo.toml"), "{err}");
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
        assert!(user(&[path]).is_err(), "accepted {text}");
    }
}
