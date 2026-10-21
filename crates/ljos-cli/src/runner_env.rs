//! The env file a shell runner sources.
//!
//! `onboard` writes it beside the runners file and records those bytes
//! in `.<filename>.ljos-wrote`. Doctor reports a file whose bytes are
//! not the text this ljos writes.

use std::path::{Path, PathBuf};

fn step(detail: String, ok: bool) -> crate::Step {
    crate::Step {
        what: "env".into(),
        detail,
        ok,
    }
}

/// The bytes ljos last wrote, beside the env file.
fn mark_path(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("env");
    path.with_file_name(format!(".{name}.ljos-wrote"))
}

fn read_mark(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(mark_path(path)).ok()?;
    (!text.is_empty()).then_some(text)
}

fn write_mark(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(mark_path(path), text)
}

/// Write the env file and record the bytes. A dry run writes nothing.
/// A file that already matches is left in place, and its record is
/// filled in when this ljos has not written one yet.
pub(crate) fn install(path: &Path, name: &str, dry: bool) -> crate::Step {
    let text = crate::seat_env_text(name);
    let source = format!("source {}", path.display());
    if std::fs::read_to_string(path).is_ok_and(|have| have == text) {
        if !dry && read_mark(path).as_deref() != Some(text.as_str()) {
            if let Err(err) = write_mark(path, &text) {
                return step(format!("{}: {err}", path.display()), false);
            }
        }
        return step(format!("{} is current; {source}", path.display()), true);
    }
    if dry {
        return step(format!("would write {}; {source}", path.display()), true);
    }
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, &text))
        .and_then(|()| write_mark(path, &text));
    match written {
        Ok(()) => step(format!("wrote {}; {source}", path.display()), true),
        Err(err) => step(format!("{}: {err}", path.display()), false),
    }
}

/// The doctor's `runner env` row. A file an older ljos wrote is stale,
/// not missing: the text this ljos writes changed, and `ljos onboard`
/// rewrites it.
pub(crate) fn state(name: &str, env_path: &Path, ready: bool, exists: bool) -> String {
    match (ready, exists) {
        (true, _) => format!("{name}: source {}", env_path.display()),
        (false, true) => format!(
            "{name}: {} is stale, an older ljos wrote it; ljos onboard --harness {name}",
            env_path.display()
        ),
        (false, false) => format!("{name}: no ljos env file; ljos onboard --harness {name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stale_runner_env_is_not_called_missing() {
        let path = Path::new("/h/.config/ljos/grokbot.env");
        assert!(state("grokbot", path, true, true).contains("source /h/"));
        let stale = state("grokbot", path, false, true);
        assert!(stale.contains("is stale"), "{stale}");
        assert!(stale.contains("ljos onboard --harness grokbot"), "{stale}");
        let none = state("grokbot", path, false, false);
        assert!(none.contains("no ljos env file"), "{none}");
    }

    #[test]
    fn install_records_the_bytes_it_wrote_and_a_dry_run_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grokbot.env");
        let dry = install(&path, "grokbot", true);
        assert!(dry.ok && dry.detail.contains("would write"), "{dry:?}");
        assert!(!path.exists(), "a dry run wrote the env");
        assert!(!mark_path(&path).exists(), "a dry run wrote the record");

        let wrote = install(&path, "grokbot", false);
        assert!(wrote.ok && wrote.detail.contains("wrote"), "{wrote:?}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, crate::seat_env_text("grokbot"));
        assert_eq!(std::fs::read_to_string(mark_path(&path)).unwrap(), text);

        let again = install(&path, "grokbot", false);
        assert!(again.ok && again.detail.contains("is current"), "{again:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(std::fs::read_to_string(mark_path(&path)).unwrap(), text);
    }

    #[test]
    fn a_current_file_with_no_record_is_marked_and_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grokbot.env");
        let text = crate::seat_env_text("grokbot");
        std::fs::write(&path, &text).unwrap();
        let step = install(&path, "grokbot", false);
        assert!(step.ok && step.detail.contains("is current"), "{step:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(std::fs::read_to_string(mark_path(&path)).unwrap(), text);
    }
}
