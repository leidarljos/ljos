//! The owner's mailbox: read it, and check what a seat drafts for it.
//!
//! Seat-to-seat mail is [`crate::mail`]. This module is the person's own
//! mail, so nothing here sends, and every byte read from a message is
//! untrusted. `docs/design/mail.md` has the whole shape.

pub mod config;
pub mod html;
pub mod notmuch;
