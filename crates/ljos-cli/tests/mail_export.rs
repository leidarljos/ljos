//! A draft exported as an approval file: a statement block, a blank
//! line, the exact message bytes, and `H` = sha256 of those bytes.

use chrono::{Duration, TimeZone as _, Utc};
use ljos_cli::mailbox::export::{export, Draft};
use sha2::{Digest as _, Sha256};

const MESSAGE: &str = "From: Owner <owner@example.test>\n\
To: Friend <friend@example.test>\n\
Subject: Friday\n\
Date: Thu, 22 Oct 2026 12:00:00 +0000\n\
Message-ID: <draft-7@example.test>\n\
\n\
See you at noon.\n\
\tIndented by a tab.\n";

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 22, 12, 0, 0).unwrap()
}

fn draft(message: &str) -> Draft<'_> {
    Draft {
        id: "draft-7",
        reachbook_hash: "ab12cd34",
        message: message.as_bytes(),
    }
}

#[test]
fn the_file_is_the_statement_block_then_the_exact_bytes() {
    let expires = now() + Duration::hours(2);
    let got = export(&draft(MESSAGE), expires, now()).unwrap();
    let text = String::from_utf8(got.file.clone()).unwrap();
    let (block, message) = text.split_once("\n\n").unwrap();
    let lines: Vec<&str> = block.lines().collect();
    assert_eq!(
        lines[..4],
        [
            "ljos-outbox v1",
            "draft draft-7",
            "reachbook-hash ab12cd34",
            "expires 2026-10-22T14:00:00Z",
        ]
    );
    let nonce = lines[4].strip_prefix("nonce ").expect("a nonce line");
    assert_eq!(nonce.len(), 32);
    assert!(nonce
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    assert_eq!(nonce, got.nonce);
    assert_eq!(lines.len(), 5);
    assert_eq!(message, MESSAGE);
    assert_eq!(got.hash, hex::encode(Sha256::digest(MESSAGE.as_bytes())));
    assert_eq!(got.expires, expires);

    let again = export(&draft(MESSAGE), expires, now()).unwrap();
    assert_ne!(again.nonce, got.nonce);
    assert_eq!(again.hash, got.hash, "H is the message, not the nonce");
}

#[test]
fn the_hash_covers_the_bytes_sendmail_is_given_including_a_lone_dot() {
    let dotted = MESSAGE.replace("See you at noon.\n", "See you at noon.\n.\nStill here.\n");
    let got = export(&draft(&dotted), now() + Duration::hours(1), now()).unwrap();
    assert!(got.file.ends_with(dotted.as_bytes()));
    assert_eq!(got.hash, hex::encode(Sha256::digest(dotted.as_bytes())));
}

#[test]
fn crlf_is_refused_because_the_stored_form_is_lf() {
    let crlf = MESSAGE.replace('\n', "\r\n");
    let err = export(&draft(&crlf), now() + Duration::hours(1), now()).unwrap_err();
    assert!(err.to_string().contains("U+000D"), "{err}");
}

#[test]
fn control_and_bidirectional_characters_are_refused_anywhere() {
    for (bad, what) in [
        (MESSAGE.replace("noon", "no\u{1b}[8mon"), "escape"),
        (MESSAGE.replace('\n', "\r\n"), "carriage return"),
        (MESSAGE.replace("Friday", "Fri\0day"), "nul"),
        (MESSAGE.replace("noon", "no\u{7f}on"), "delete"),
        (MESSAGE.replace("noon", "no\u{85}on"), "next line"),
        (
            MESSAGE.replace("noon", "\u{202e}noon"),
            "right-to-left override",
        ),
        (
            MESSAGE.replace("Friday", "\u{2066}Friday\u{2069}"),
            "isolate",
        ),
        (MESSAGE.replace("noon", "no\u{0c}on"), "form feed"),
    ] {
        let err = export(&draft(&bad), now() + Duration::hours(1), now());
        assert!(err.is_err(), "{what} was exported");
        assert!(err.unwrap_err().to_string().contains("U+"), "{what}");
    }
}

#[test]
fn the_expiry_is_after_now_and_at_most_a_day_out() {
    let ok = |hours: i64| export(&draft(MESSAGE), now() + Duration::hours(hours), now()).is_ok();
    assert!(ok(24));
    assert!(ok(1));
    assert!(!ok(0));
    assert!(!ok(-1));
    assert!(!ok(25));
    assert!(export(
        &draft(MESSAGE),
        now() + Duration::hours(24) + Duration::seconds(1),
        now()
    )
    .is_err());
}

#[test]
fn a_message_without_its_fixed_headers_or_a_bad_field_is_refused() {
    let no_id = MESSAGE.replace("Message-ID: <draft-7@example.test>\n", "");
    let no_date = MESSAGE.replace("Date: Thu, 22 Oct 2026 12:00:00 +0000\n", "");
    let no_body = MESSAGE.split("\n\n").next().unwrap().to_string();
    let in_body = MESSAGE.replace("See you", "Message-ID: <x@example.test>\nSee you");
    let later = now() + Duration::hours(1);
    for message in [&no_id, &no_date, &no_body] {
        assert!(export(&draft(message), later, now()).is_err(), "{message}");
    }
    let only_in_body = in_body.replace("Message-ID: <draft-7@example.test>\n", "");
    assert!(export(&draft(&only_in_body), later, now()).is_err());

    for (id, hash) in [
        ("", "ab"),
        ("draft 7", "ab"),
        ("draft-7", "a\nb"),
        ("draft-7", ""),
    ] {
        let bad = Draft {
            id,
            reachbook_hash: hash,
            message: MESSAGE.as_bytes(),
        };
        assert!(export(&bad, later, now()).is_err(), "{id:?} {hash:?}");
    }
    let latin1 = Draft {
        message: b"Date: x\nMessage-ID: <a@example.test>\n\ncaf\xe9\n",
        ..draft(MESSAGE)
    };
    assert!(export(&latin1, later, now()).is_err());
}

#[test]
fn date_and_message_id_must_be_valid_not_only_present() {
    let later = now() + Duration::hours(1);
    let date = "Date: Thu, 22 Oct 2026 12:00:00 +0000\n";
    let id = "Message-ID: <draft-7@example.test>\n";
    for bad in [
        MESSAGE.replace(date, "Date: \n"),
        MESSAGE.replace(date, "Date: tomorrow\n"),
        MESSAGE.replace(date, "Date: 2026-10-22T12:00:00Z\n"),
        MESSAGE.replace(date, &format!("{date}{date}")),
        MESSAGE.replace(id, "Message-ID: \n"),
        MESSAGE.replace(id, "Message-ID: <>\n"),
        MESSAGE.replace(id, "Message-ID: draft-7@example.test\n"),
        MESSAGE.replace(id, "Message-ID: <draft 7@example.test>\n"),
        MESSAGE.replace(id, "Message-ID: <\"d\"@example.test>\n"),
        MESSAGE.replace(id, "Message-ID: <a..b@example.test>\n"),
        MESSAGE.replace(id, "Message-ID: <a@b@example.test>\n"),
        MESSAGE.replace(id, &format!("{id}{id}")),
    ] {
        assert!(export(&draft(&bad), later, now()).is_err(), "{bad}");
    }
    for good in [
        MESSAGE.replace(date, "Date: Thu, 22 Oct 2026\n 12:00:00 +0000\n"),
        MESSAGE.replace(id, "Message-ID: <a.b+c@[192.0.2.1]>\n"),
    ] {
        assert!(export(&draft(&good), later, now()).is_ok(), "{good}");
    }
}

#[test]
fn a_bcc_header_is_refused_because_sendmail_t_strips_it() {
    let later = now() + Duration::hours(1);
    for bcc in [
        "Bcc: hidden@example.test\n",
        "bcc:\n hidden@example.test\n",
        "Resent-Bcc: x@example.test\n",
    ] {
        let message = MESSAGE.replace("Subject:", &format!("{bcc}Subject:"));
        let err = export(&draft(&message), later, now()).unwrap_err();
        assert!(err.to_string().contains("Bcc"), "{bcc}: {err}");
    }
}
