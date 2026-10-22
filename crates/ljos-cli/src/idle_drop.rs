//! What a long-lived ljos-mcp gives back once nobody is calling it.
//!
//! A tool call parses whole pack listings, tracker files and transcripts.
//! The parse is freed when the call returns, but glibc keeps freed pages
//! in its arenas, so a server that answered a burst stayed at the burst's
//! size for days. After [`drop_after`] without a call the server drops
//! the listing it caches and asks the allocator to return free pages.

use std::path::Path;
use std::time::Duration;

const DEFAULT: Duration = Duration::from_secs(120);

/// How long the server sits without a call before it gives memory back.
/// `LJOS_MCP_IDLE_DROP`, else `[mcp] idle_drop` in the `ljos.toml`
/// layers, else 120 s. A bare number is seconds; `90s`, `5m` and `1h`
/// carry their unit. `0` and `off` keep everything until exit.
#[must_use]
pub fn drop_after() -> Option<Duration> {
    if let Ok(raw) = std::env::var("LJOS_MCP_IDLE_DROP") {
        if !raw.trim().is_empty() {
            return parse(&raw);
        }
    }
    let mut window = Some(DEFAULT);
    for path in crate::converge::config_paths(None) {
        if let Some(set) = configured(&path) {
            window = set;
        }
    }
    window
}

/// The `[mcp] idle_drop` of one file, or `None` when the file sets none.
fn configured(path: &Path) -> Option<Option<Duration>> {
    let text = std::fs::read_to_string(path).ok()?;
    let value = toml::from_str::<toml::Value>(&text).ok()?;
    match value.get("mcp")?.get("idle_drop")? {
        toml::Value::Integer(secs) => Some(u64::try_from(*secs).ok().and_then(window)),
        toml::Value::String(raw) => Some(parse(raw)),
        toml::Value::Boolean(false) => Some(None),
        _ => None,
    }
}

fn window(secs: u64) -> Option<Duration> {
    (secs > 0).then(|| Duration::from_secs(secs))
}

fn parse(raw: &str) -> Option<Duration> {
    let raw = raw.trim();
    if matches!(
        raw.to_ascii_lowercase().as_str(),
        "off" | "false" | "no" | "never"
    ) {
        return None;
    }
    if let Ok(secs) = raw.parse::<u64>() {
        return window(secs);
    }
    match crate::reap::parse_idle(raw) {
        Ok(found) if found.is_zero() => None,
        Ok(found) => Some(found),
        Err(_) => Some(DEFAULT),
    }
}

/// Drop what the process caches and return free pages to the system.
pub fn release_memory() {
    crate::drop_listing_cache();
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim takes no pointers and is safe from any thread.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// Touches the activity clock now and again when dropped, so a call that
/// outlasts the window is followed by a drop once it ends.
pub struct CallActivity(());

impl Drop for CallActivity {
    fn drop(&mut self) {
        crate::bookkeep::touch_activity();
    }
}

/// Hold for the length of one tool call.
#[must_use]
pub fn call_started() -> CallActivity {
    crate::bookkeep::touch_activity();
    CallActivity(())
}

/// Start the thread that calls [`release_memory`] once the last tool call
/// is [`drop_after`] old. One drop per quiet spell: a server nobody calls
/// does not trim again and again.
pub fn spawn_idle_drop() {
    let Some(limit) = drop_after() else {
        return;
    };
    crate::bookkeep::touch_activity();
    let tick = (limit / 2).clamp(Duration::from_secs(1), Duration::from_secs(10));
    let _ = std::thread::Builder::new()
        .name("ljos-mcp-drop".into())
        .stack_size(64 * 1024)
        .spawn(move || {
            let mut dropped_at = 0;
            loop {
                std::thread::sleep(tick);
                let last = crate::bookkeep::last_activity();
                if last != dropped_at
                    && crate::bookkeep::idle_due(crate::bookkeep::now_secs(), last, limit)
                {
                    release_memory();
                    dropped_at = last;
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_number_is_seconds_and_off_keeps_everything() {
        assert_eq!(parse("120"), Some(Duration::from_secs(120)));
        assert_eq!(parse("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse("0"), None);
        assert_eq!(parse("off"), None);
        assert_eq!(parse("soon"), Some(DEFAULT));
    }

    #[test]
    fn ljos_toml_sets_the_window_under_mcp() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ljos.toml");
        std::fs::write(&file, "[mcp]\nidle_drop = 30\n").unwrap();
        assert_eq!(configured(&file), Some(Some(Duration::from_secs(30))));
        std::fs::write(&file, "[mcp]\nidle_drop = \"off\"\n").unwrap();
        assert_eq!(configured(&file), Some(None));
        std::fs::write(&file, "[git_hooks]\nskip = []\n").unwrap();
        assert_eq!(configured(&file), None);
    }
}
