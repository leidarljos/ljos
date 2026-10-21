//! The env file a shell runner sources.
//!
//! `onboard` writes it beside the runners file. Doctor reports a file
//! whose bytes are not the text this ljos writes.

use std::path::Path;

fn step(detail: String, ok: bool) -> crate::Step {
    crate::Step {
        what: "env".into(),
        detail,
        ok,
    }
}

/// Write the env file. A dry run writes nothing. A file that already
/// matches is left in place.
pub(crate) fn install(path: &Path, name: &str, dry: bool) -> crate::Step {
    let text = crate::seat_env_text(name);
    let source = format!("source {}", path.display());
    if std::fs::read_to_string(path).is_ok_and(|have| have == text) {
        return step(format!("{} is current; {source}", path.display()), true);
    }
    if dry {
        return step(format!("would write {}; {source}", path.display()), true);
    }
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, text));
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
}
