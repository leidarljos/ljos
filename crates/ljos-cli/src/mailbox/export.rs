//! A draft as an approval file.
//!
//! ```text
//! ljos-outbox v1
//! draft <outbox draft id>
//! reachbook-hash <the outbox's sha256 of the message, hex>
//! expires <RFC 3339, at most 24 h out>
//! nonce <16 random bytes, hex>
//!
//! <the message, LF line endings>
//! ```
//!
//! The message is stored in one canonical form: LF line endings, written
//! as is to `sendmail -i -t`, with the MTA turning LF into CRLF. `H` is
//! sha256 of exactly those bytes, so mailgate can hash what it is about
//! to write and refuse on a mismatch. `H` is what the ballot and the
//! owner approve; the expiry and nonce are checked and spent by mailgate
//! on their own. The person
//! approving reads this file in a terminal or a chat card, so any byte
//! that could hide or reorder text there is refused, not escaped: what
//! they read must be exactly what is sent.

use std::io::Read as _;

use anyhow::{bail, Context as _, Result};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sha2::{Digest as _, Sha256};

use super::config::MailConfig;

/// The longest an approval file stays valid.
pub const MAX_LIFETIME: Duration = Duration::hours(24);

/// What the outbox holds for one draft.
pub struct Draft<'a> {
    pub id: &'a str,
    /// The outbox's sha256 of `message`, lowercase hex. It must match.
    pub reachbook_hash: &'a str,
    /// The message in its canonical form, `Message-ID` and `Date` set.
    pub message: &'a [u8],
}

/// An exported approval file and its hash.
#[derive(Debug, Clone)]
pub struct Approval {
    pub file: Vec<u8>,
    /// `H`: sha256 of the message bytes, the ones `sendmail` is given, hex.
    pub hash: String,
    pub nonce: String,
    pub expires: DateTime<Utc>,
}

/// The approval file for `draft`, refused when the content checks under
/// `config` refuse the message: no file is minted for a draft that could
/// not be sent.
pub fn export(
    draft: &Draft<'_>,
    config: &MailConfig,
    expires: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Approval> {
    for (name, value) in [
        ("draft", draft.id),
        ("reachbook-hash", draft.reachbook_hash),
    ] {
        if value.is_empty() || value.len() > 200 || !value.bytes().all(|b| b.is_ascii_graphic()) {
            bail!("export: {name} must be 1 to 200 printable characters with no space");
        }
    }
    if expires <= now || expires - now > MAX_LIFETIME {
        bail!("export: expires must be after now and at most 24 h out");
    }
    let message = std::str::from_utf8(draft.message).context("export: the message is not UTF-8")?;
    refuse_hiding(message)?;
    let Some((headers, _)) = message.split_once("\n\n") else {
        bail!("export: the message has no blank line between headers and body");
    };
    let fields = super::checks::unfold(headers)
        .context("export: a header line is neither a field nor a continuation")?;
    let one = |name: &str| -> Result<&str> {
        let mut values = fields.iter().filter(|(n, _)| n == name);
        match (values.next(), values.next()) {
            (Some((_, value)), None) => Ok(value),
            _ => bail!(
                "export: the message needs exactly one {name} header; the send path would add one"
            ),
        }
    };
    if DateTime::parse_from_rfc2822(one("date")?).is_err() {
        bail!("export: Date is not an RFC 5322 date-time");
    }
    if !is_msg_id(one("message-id")?) {
        bail!("export: Message-ID is not an RFC 5322 msg-id such as <id@example.test>");
    }
    if fields.iter().any(|(n, _)| n == "bcc") {
        bail!("export: sendmail -t strips Bcc, so the sent bytes would not be H; use To or Cc");
    }
    if let Some((name, _)) = fields.iter().find(|(n, _)| n.starts_with("resent-")) {
        bail!(
            "export: {name} is refused: with a Resent- header sendmail -t sends to the Resent- recipients, not To and Cc"
        );
    }
    let refused = super::checks::refusals(draft.message, config);
    if !refused.is_empty() {
        bail!(
            "export: the checks refuse this draft: {}",
            refused.join("; ")
        );
    }
    let hash = hex::encode(Sha256::digest(draft.message));
    if draft.reachbook_hash != hash {
        bail!("export: reachbook-hash is not the sha256 of the message, so the outbox holds other bytes");
    }
    let mut seed = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut seed))
        .context("export: no randomness for the nonce")?;
    let nonce = hex::encode(seed);
    let mut file = format!(
        "ljos-outbox v1\ndraft {}\nreachbook-hash {}\nexpires {}\nnonce {nonce}\n\n",
        draft.id,
        draft.reachbook_hash,
        expires.to_rfc3339_opts(SecondsFormat::Secs, true),
    )
    .into_bytes();
    file.extend_from_slice(draft.message);
    Ok(Approval {
        file,
        hash,
        nonce,
        expires,
    })
}

/// `<id-left@id-right>`: dot-atom text on the left, dot-atom text or a
/// `[...]` literal without folding on the right.
fn is_msg_id(value: &str) -> bool {
    let dot_atom = |s: &str| {
        s.split('.').all(|atom| {
            !atom.is_empty()
                && atom
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~".contains(&b))
        })
    };
    let Some((left, right)) = value
        .strip_prefix('<')
        .and_then(|v| v.strip_suffix('>'))
        .and_then(|v| v.split_once('@'))
    else {
        return false;
    };
    let literal = right
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .is_some_and(|d| {
            d.bytes()
                .all(|b| b.is_ascii_graphic() && !b"[]\\".contains(&b))
        });
    dot_atom(left) && (dot_atom(right) || literal)
}

fn refuse_hiding(message: &str) -> Result<()> {
    for (n, line) in message.split('\n').enumerate() {
        if let Some((col, c)) = line
            .chars()
            .enumerate()
            .find(|(_, c)| super::notmuch::hiding(*c))
        {
            bail!(
                "export: line {} column {} carries U+{:04X}; only tab and newline may appear",
                n + 1,
                col + 1,
                c as u32
            );
        }
    }
    Ok(())
}
