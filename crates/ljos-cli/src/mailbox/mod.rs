//! The owner's mailbox: read it, and check what a seat drafts for it.
//!
//! Seat-to-seat mail is [`crate::mail`]. This module is the person's own
//! mail, so nothing here sends, and every byte read from a message is
//! untrusted. `docs/design/mail.md` has the whole shape.

pub mod checks;
pub mod config;
pub mod export;
pub mod html;
pub mod notmuch;

use anyhow::{Context as _, Result};

/// A reader over the mailbox the `[mail]` table names. A cloud seat
/// prints `cloud-seat-mail` and still reads a mailbox this machine holds.
/// Any cloud marker counts.
pub fn open() -> Result<notmuch::Reader> {
    if crate::cloud::any_cloud_marker() {
        eprintln!(
            "ljos: cloud-seat-mail: a cloud seat has no mailbox of its own; reading the mailbox on this machine"
        );
    }
    let config = config::load()?.context("mail: no [mail] table in ljos config")?;
    notmuch::Reader::new(&config)
}
