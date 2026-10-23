//! Identity, term and attribution checks only ever refuse.

use std::os::unix::fs::PermissionsExt;
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
MIME-Version: 1.0\n\
Content-Type: text/plain; charset=utf-8\n\
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
fn a_term_split_by_any_default_ignorable_character_is_found() {
    let s = setup(DENY);
    for (split, what) in [
        ('\u{200b}', "zero width space"),
        ('\u{fe0f}', "variation selector"),
        ('\u{e0100}', "supplementary variation selector"),
        ('\u{e0041}', "tag character"),
    ] {
        let draft = CLEAN.replace("about topic-a", &format!("about proj{split}ect-x"));
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains("term")), "{what}: {got:?}");
    }
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
    let cte = |value: &str| {
        CLEAN.replace(
            "Subject:",
            &format!("Content-Transfer-Encoding:{value}\nSubject:"),
        )
    };
    let word = CLEAN.replace("Subject: Friday", "Subject: =?utf-8?b?UHJvamVjdC1Y?=");
    let body_word = CLEAN.replace("See you", "=?utf-8?q?project-x?= See you");
    for draft in [
        cte(" base64"),
        cte("\tbase64"),
        cte("\n base64"),
        cte("\n\tQuoted-Printable"),
        cte(" binary"),
        cte("\t8BIT"),
        cte("\n 8bit"),
        cte(" 7bit").replace("Subject:", "Content-Transfer-Encoding: 7bit\nSubject:"),
        word,
        body_word,
    ] {
        let got = refused(&draft, &s.config);
        assert!(
            got.iter().any(|r| r.contains("encoded")),
            "{draft}: {got:?}"
        );
    }
    for ok in [" 7bit", "\t7BIT", "\n 7bit"] {
        assert!(refused(&cte(ok), &s.config).is_empty(), "{ok:?}");
    }
    assert!(!refusals(b"From: owner@example.test\n\ncaf\xe9", &s.config).is_empty());
}

#[test]
fn what_the_mta_would_rewrite_is_refused() {
    let s = setup(DENY);
    let body = |n: usize| CLEAN.replace("See you at noon about topic-a.", &"a".repeat(n));
    assert_eq!(refused(&body(998), &s.config), Vec::<String>::new());
    let subject = CLEAN.replace("Subject: Friday", &format!("Subject: {}", "b".repeat(990)));
    for draft in [body(999), subject] {
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains("998 octets")), "{got:?}");
    }
    for draft in [
        CLEAN.replace("noon", "midi, caf\u{e9}"),
        CLEAN.replace("Friday", "Fr\u{e9}d\u{e9}ric"),
    ] {
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains("not ASCII")), "{got:?}");
    }
}

#[test]
fn only_one_plain_utf8_part_is_accepted() {
    let s = setup(DENY);
    let plain = "Content-Type: text/plain; charset=utf-8\n";
    for kind in [
        "",
        "Content-Type: text/html; charset=utf-8\n",
        "Content-Type: text/plain\n",
        "Content-Type: text/plain; charset=iso-8859-1\n",
        "Content-Type: text/plain; charset=utf-8; format=flowed; delsp=yes\n",
        "Content-Type: multipart/alternative; boundary=b\n",
        "Content-Type:\n\tmultipart/mixed;\n boundary=b\n",
        "Content-Type: text/plain; charset=utf-8\nContent-Type: text/html\n",
    ] {
        let draft = CLEAN.replace(plain, kind);
        let got = refused(&draft, &s.config);
        assert!(
            got.iter().any(|r| r.contains("text/plain")),
            "{kind:?}: {got:?}"
        );
    }
    let folded = CLEAN.replace(plain, "Content-Type: text/plain;\n\tcharset=\"UTF-8\"\n");
    assert!(refused(&folded, &s.config).is_empty());
    let broken = CLEAN.replace("Subject:", "not a header\nSubject:");
    assert!(!refused(&broken, &s.config).is_empty());
}

#[test]
fn markup_in_the_body_is_refused_so_a_term_cannot_hide_in_it() {
    let s = setup(DENY);
    for body in [
        "Status of proj&#101;ct-x.",
        "Status of proj&#x65;ct-x.",
        "Status of pro<span>ject-x</span>.",
        "Gener<b></b>ated by a tool.",
        "Fine &amp; dandy.",
        "<!-- project-x --> hi",
    ] {
        let draft = CLEAN.replace("See you at noon about topic-a.", body);
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains("HTML")), "{body}: {got:?}");
    }
    for body in ["1 < 2 & 3 > 2", "AT&T at 5", "a <= b"] {
        let draft = CLEAN.replace("See you at noon about topic-a.", body);
        assert!(refused(&draft, &s.config).is_empty(), "{body}");
    }
}

#[test]
fn a_numeric_reference_without_a_semicolon_is_markup() {
    let s = setup(DENY);
    for body in [
        "Status of proj&#1077ect-x.",
        "Status of pro&#xfff9ject-x.",
        "Status of pro&#X65ject-x.",
        "Status of proj&#101ct-x.",
    ] {
        let draft = CLEAN.replace("See you at noon about topic-a.", body);
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains("HTML")), "{body}: {got:?}");
    }
}

#[test]
fn a_bracketed_link_passes_and_a_term_joined_by_tags_or_references_does_not() {
    let s = setup(DENY);
    for body in [
        "See <https://example.test/a> and <mailto:jo@example.test>.",
        "Press <Enter> when 1 <2.",
    ] {
        let draft = CLEAN.replace("See you at noon about topic-a.", body);
        assert_eq!(refused(&draft, &s.config), Vec::<String>::new(), "{body}");
    }
    for (body, kind) in [
        ("Status of pro<span>ject-x.", "term"),
        ("Status of pro<https://example.test/a>ject-x.", "term"),
        ("Status of proj&#101ct-x.", "term"),
        ("Status of pr&#X6Fject-x.", "term"),
        ("Ask @blocked<i>handle.", "term"),
        ("About topic-a and top<b>ic-b.", "pair"),
        ("Gener<b>ated by a tool.", "attribution"),
    ] {
        let draft = CLEAN.replace("See you at noon about topic-a.", body);
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains(kind)), "{body}: {got:?}");
    }
}

#[test]
fn folded_headers_are_checked_unfolded() {
    let s = setup(DENY);
    let subject = CLEAN.replace("Subject: Friday", "Subject: Status of\n\tPROJECT-X");
    let got = refused(&subject, &s.config);
    assert!(got.iter().any(|r| r.contains("term")), "{got:?}");
    let cc = CLEAN.replace("Subject:", "Cc: Pal\n <pal@denied.example.test>\nSubject:");
    let got = refused(&cc, &s.config);
    assert!(got.iter().any(|r| r.contains("cc")), "{got:?}");
}

#[test]
fn a_trailing_dot_on_a_recipient_domain_is_the_same_domain() {
    let s = setup(DENY);
    for to in [
        "user@denied.example.test.",
        "user@mail.denied.example.test.",
        "blocked@example.test.",
    ] {
        let draft = CLEAN.replace("friend@example.test", to);
        let got = refused(&draft, &s.config);
        assert!(got.iter().any(|r| r.contains("recipient")), "{to}: {got:?}");
    }
}

#[test]
fn a_broken_setup_refuses_everything() {
    let s = setup(DENY);
    let no_identity = MailConfig {
        identity: None,
        ..s.config.clone()
    };
    assert!(!refused(CLEAN, &no_identity).is_empty());
    let no_deny = MailConfig {
        deny_file: None,
        ..s.config.clone()
    };
    let got = refused(CLEAN, &no_deny);
    assert!(got.iter().any(|r| r.contains("deny_file")), "{got:?}");
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
fn a_deny_file_others_could_change_refuses_everything() {
    let s = setup(DENY);
    let real = s.config.deny_file.clone().unwrap();
    let dir = real.parent().unwrap();
    let link = dir.join("linked.txt");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let shared = dir.join("shared");
    std::fs::create_dir(&shared).unwrap();
    std::fs::write(shared.join("deny.txt"), DENY).unwrap();
    std::fs::set_permissions(&shared, PermissionsExt::from_mode(0o775)).unwrap();
    for (path, why) in [(link, "symlink"), (shared.join("deny.txt"), "writable")] {
        let config = MailConfig {
            deny_file: Some(path),
            ..s.config.clone()
        };
        let got = refused(CLEAN, &config);
        assert!(got.iter().any(|r| r.contains(why)), "{why}: {got:?}");
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
