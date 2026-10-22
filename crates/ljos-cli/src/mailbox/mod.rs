//! The owner's mailbox: read it, and check what a seat drafts for it.
//!
//! Seat-to-seat mail is [`crate::mail`]. This module is the person's own
//! mail, so nothing here sends, and every byte read from a message is
//! untrusted. `docs/design/mail.md` has the whole shape.

pub mod config;
pub mod html;
pub mod notmuch;

use anyhow::{bail, Context as _, Result};

/// A reader over the mailbox the `[mail]` table names. A cloud seat is
/// refused: it has no mailbox, and the owner's mail stays on the machine
/// that holds it.
pub fn open() -> Result<notmuch::Reader> {
    if crate::cloud::cloud_seat() {
        bail!("mail: a cloud seat has no mailbox; mail is read only on the machine that holds it");
    }
    let config = config::load()?.context("mail: no [mail] table in ljos config")?;
    notmuch::Reader::new(&config)
}
