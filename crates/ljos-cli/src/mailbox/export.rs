//! A draft as an approval file.
//!
//! ```text
//! ljos-outbox v1
//! draft <outbox draft id>
//! reachbook-hash <the outbox's message hash>
//! expires <RFC 3339, at most 24 h out>
//! nonce <16 random bytes, hex>
//!
//! <exact RFC 5322 bytes to send>
//! ```
//!
//! `H = sha256(file)` is what the ballot and the owner approve. The person
//! approving reads this file in a terminal or a chat card, so any byte
//! that could hide or reorder text there is refused, not escaped: what
//! they read must be exactly what is sent.

use std::io::Read as _;

use anyhow::{bail, Context as _, Result};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sha2::{Digest as _, Sha256};

/// The longest an approval file stays valid.
pub const MAX_LIFETIME: Duration = Duration::hours(24);

/// What the outbox holds for one draft.
pub struct Draft<'a> {
    pub id: &'a str,
    pub reachbook_hash: &'a str,
    /// The exact RFC 5322 bytes, `Message-ID` and `Date` already set.
    pub message: &'a [u8],
}

/// An exported approval file and its hash.
#[derive(Debug, Clone)]
pub struct Approval {
    pub file: Vec<u8>,
    /// `H`, sha256 of `file`, hex.
    pub hash: String,
    pub nonce: String,
    pub expires: DateTime<Utc>,
}

pub fn export(draft: &Draft<'_>, expires: DateTime<Utc>, now: DateTime<Utc>) -> Result<Approval> {
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
    for want in ["message-id:", "date:"] {
        if !headers
            .lines()
            .any(|line| line.to_ascii_lowercase().starts_with(want))
        {
            bail!("export: the message has no {want} header; the send path would add one");
        }
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
    let hash = hex::encode(Sha256::digest(&file));
    Ok(Approval {
        file,
        hash,
        nonce,
        expires,
    })
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
