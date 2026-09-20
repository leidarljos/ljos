//! Append-only HUD log (`LJOS_HUD_LOG`, else next to the summon socket).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Env override for the log path.
pub const LOG_ENV: &str = "LJOS_HUD_LOG";

/// Default and override path for the HUD log.
pub fn path() -> PathBuf {
    if let Ok(raw) = std::env::var(LOG_ENV) {
        let t = raw.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    let sock = crate::summon::default_socket_path();
    sock.parent()
        .unwrap_or_else(|| Path::new("/tmp"))
        .join("hud.log")
}

/// Append one line.
pub fn info(msg: &str) {
    let msg = msg.trim();
    if msg.is_empty() {
        return;
    }
    let path = path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{msg}");
    }
}

/// Append an error line.
pub fn error(msg: &str) {
    info(&format!("error {msg}"));
}
