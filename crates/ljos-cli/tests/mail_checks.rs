//! Identity, term and attribution checks only ever refuse.

use std::path::{Path, PathBuf};

use ljos_cli::mailbox::checks::refusals;
use ljos_cli::mailbox::config::MailConfig;

const DENY: &str = "\
# synthetic deny list
address: blocked@example.test
domain: denied.example.test
handle: @blockedhandle
term: project-x
pair: topic-a + topic-b
";

const CLEAN: &str = "From: Owner <owner@example.test>\n\
To: Friend <friend@example.test>, \"Doe, Jo\" <jo@example.test>\n\
Subject: Friday\n\
Date: Thu, 22 Oct 2026 12:00:00 +0000\n\
Message-ID: <draft-7@example.test>\n\
\n\
See you at noon about topic-a.\n";

struct Setup {
    _dir: tempfile::TempDir,
    config: MailConfig,
}

fn setup(deny: &str) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deny.txt");
    std::fs::write(&path, deny).unwrap();
    Setup {
        config: MailConfig {
            identity: Some("owner@example.test".into()),
            deny_file: Some(path),
            attribution: vec!["drafted by the helper".into()],
            ..MailConfig::default()
        },
        _dir: dir,
    }
}

fn refused(message: &str, config: &MailConfig) -> Vec<String> {
    refusals(message.as_bytes(), config)
}

#[test]
fn a_clean_draft_has_no_refusals() {
    let s = setup(DENY);
    assert_eq!(refused(CLEAN, &s.config), Vec::<String>::new());
}

#[test]
fn from_must_be_exactly_the_identity() {
    let s = setup(DENY);
    for from in [
        "From: Someone <someone@example.test>\n",
        "From: owner@example.test, other@example.test\n",
        "From: \"owner@example.test\" <spoof@example.test>\n",
        "",
    ] {
        let draft = CLEAN.replace("From: Owner <owner@example.test>\n", from);
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains("From")), "{from:?}: {got:?}");
    }
    let upper = CLEAN.replace("owner@example.test", "OWNER@Example.Test");
    assert!(refused(&upper, &s.config).is_empty());
}

#[test]
fn denied_identities_in_routing_headers_and_recipients_are_refused() {
    let s = setup(DENY);
    for (extra, header) in [
        ("Reply-To: <x@mail.denied.example.test>\n", "reply-to"),
        ("Sender: blocked@example.test\n", "sender"),
        (
            "Return-Path: <bounce@example.test> (@blockedhandle)\n",
            "return-path",
        ),
        ("Cc: Pal <pal@denied.example.test>\n", "cc"),
        ("Bcc: BLOCKED@example.test\n", "bcc"),
    ] {
        let draft = CLEAN.replace("Subject:", &format!("{extra}Subject:"));
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains(header)), "{extra}: {got:?}");
    }
    let near = CLEAN.replace("friend@example.test", "friend@notdenied.example.test");
    assert!(refused(&near, &s.config).is_empty());
}

#[test]
fn terms_and_pairs_are_found_after_folding() {
    let s = setup(DENY);
    for body in [
        "Status of PROJECT-X.",
        "Status of \u{ff50}\u{ff52}\u{ff4f}\u{ff4a}\u{ff45}\u{ff43}\u{ff54}-x.",
        "Status of proj\u{200b}ect-x.",
        "Ask @BlockedHandle.",
    ] {
        let draft = CLEAN.replace("See you at noon about topic-a.", body);
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains("term")), "{body}: {got:?}");
    }
    let subject = CLEAN.replace("Subject: Friday", "Subject: Project-X");
    assert!(!refused(&subject, &s.config).is_empty());
    let both = CLEAN.replace("noon about topic-a", "noon about topic-a and Topic-B");
    let got = refused(&both, &s.config);
    assert!(got.iter().any(|r| r.contains("pair")), "{got:?}");
    let part = CLEAN.replace("topic-a", "topic-ab");
    assert!(
        refused(&part, &s.config).is_empty(),
        "a longer word is not the term"
    );
}

#[test]
fn attribution_is_refused_by_default_and_by_config() {
    let s = setup(DENY);
    for body in [
        "Generated with a writing tool.",
        "Co-Authored-By: someone",
        "As an AI, I cannot attend.",
        "Drafted by the helper.",
    ] {
        let draft = CLEAN.replace("See you at noon about topic-a.", body);
        let got = refused(&draft, &s.config);
        assert!(
            got.iter().any(|r| r.contains("attribution")),
            "{body}: {got:?}"
        );
    }
    let mailer = CLEAN.replace("Subject:", "X-Mailer: Mail Agent 2.1\nSubject:");
    let got = refused(&mailer, &s.config);
    assert!(got.iter().any(|r| r.contains("x-mailer")), "{got:?}");
    let person = CLEAN.replace("Subject:", "X-Mailer: Desk Mail 9\nSubject:");
    assert!(refused(&person, &s.config).is_empty());
}

#[test]
fn text_the_checks_cannot_read_is_refused() {
    let s = setup(DENY);
    let base64 = CLEAN.replace("Subject:", "Content-Transfer-Encoding: base64\nSubject:");
    let qp = CLEAN.replace(
        "See you",
        "--b\nContent-Transfer-Encoding: Quoted-Printable\n\nSee you",
    );
    let word = CLEAN.replace("Subject: Friday", "Subject: =?utf-8?b?UHJvamVjdC1Y?=");
    for draft in [base64, qp, word] {
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains("encoded")), "{got:?}");
    }
    assert!(!refusals(b"From: owner@example.test\n\ncaf\xe9", &s.config).is_empty());
}

#[test]
fn a_broken_setup_refuses_everything() {
    let s = setup(DENY);
    let no_identity = MailConfig {
        identity: None,
        ..s.config.clone()
    };
    assert!(!refused(CLEAN, &no_identity).is_empty());
    let missing = MailConfig {
        deny_file: Some(PathBuf::from("/nonexistent/deny.txt")),
        ..s.config.clone()
    };
    assert!(!refused(CLEAN, &missing).is_empty());
    for bad in [
        "nickname: x\n",
        "term:\n",
        "pair: only-one\n",
        "just words\n",
    ] {
        let b = setup(bad);
        assert!(!refused(CLEAN, &b.config).is_empty(), "{bad:?}");
    }
}

#[test]
fn a_spoofed_owner_approval_does_not_relax_the_checks() {
    let s = setup(DENY);
    let config = MailConfig {
        identity: Some("me@example.test".into()),
        ..s.config.clone()
    };
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mail/02-spoofed-approval.eml");
    let spoof = std::fs::read_to_string(fixture).unwrap();
    let got = refused(&spoof, &config);
    assert!(got.iter().any(|r| r.contains("From")), "{got:?}");
    let approved = format!(
        "X-Owner-Approved: yes\n{}",
        CLEAN.replace("Owner <owner", "Owner <boss")
    );
    assert!(!refused(&approved, &s.config).is_empty());
}
