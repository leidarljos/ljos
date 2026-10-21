//! Env files a shell runner sources.
//!
//! `onboard` writes the file and records those bytes in
//! `.<filename>.ljos-wrote`. The first run of a new ljos rewrites a file
//! that is still those bytes, keeps lines the user added, and leaves a
//! file it did not write alone. `ljos doctor` warns when a file it wrote
//! is stale, and `ljos doctor --fix` rewrites it.

use std::path::{Path, PathBuf};

const STALE_MARK: &str = " is stale; ljos doctor --fix";
const EDITED_MARK: &str = " was edited by hand; ljos left it";
const FOREIGN_MARK: &str = " was not written by ljos; ljos left it";

fn step(detail: String, ok: bool) -> crate::Step {
    crate::Step {
        what: "env".into(),
        detail,
        ok,
    }
}

fn mark_path(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("env");
    path.with_file_name(format!(".{name}.ljos-wrote"))
}

fn read_mark(path: &Path) -> Option<String> {
    std::fs::read_to_string(mark_path(path))
        .ok()
        .filter(|text| !text.is_empty())
}

fn ensure_parent(path: &Path) -> std::io::Result<()> {
    path.parent().map_or(Ok(()), std::fs::create_dir_all)
}

enum Kind {
    Missing,
    Current,
    Stale(Vec<String>),
    Hand,
    Foreign,
}

/// Lines of `file` past the lines of `body`, in order. `None` when a
/// line `body` wrote was changed or removed.
fn added(body: &str, file: &str) -> Option<Vec<String>> {
    let want: Vec<&str> = body.lines().collect();
    let mut i = 0;
    let mut extra = Vec::new();
    for line in file.lines() {
        if i < want.len() && line == want[i] {
            i += 1;
        } else if !line.trim().is_empty() {
            extra.push(line.to_string());
        }
    }
    (i == want.len()).then_some(extra)
}

fn looks_like_ours(text: &str) -> bool {
    text.starts_with("# Source this in the shell that runs the agent's commands.\n")
        && text.contains("LJOS_SEAT")
        && (text.contains("_ljos_guard") || text.contains("ljos/guard-bin"))
}

/// A line an older env carried. Dropped only when the file has no record,
/// so a shell block this ljos no longer writes is not appended.
fn generated(line: &str) -> bool {
    let t = line.trim();
    t.is_empty()
        || t.contains("LJOS_")
        || t.contains("_ljos")
        || t.starts_with("export PATH=")
        || t.starts_with("unset ")
        || t.starts_with("if ")
        || t.starts_with("elif ")
        || t.starts_with("case ")
        || t.starts_with(";;")
        || t.starts_with("*)")
        || matches!(t, "fi" | "then" | "do" | "done" | "esac" | "else")
}

fn classify(path: &Path, template: &str) -> Kind {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Kind::Missing,
        Err(_) => return Kind::Foreign,
    };
    if text == template {
        return Kind::Current;
    }
    if let Some(body) = read_mark(path) {
        return match added(&body, &text) {
            Some(_) if body == template => Kind::Current,
            Some(extra) => Kind::Stale(extra),
            None => Kind::Hand,
        };
    }
    if !looks_like_ours(&text) {
        return Kind::Foreign;
    }
    let have: Vec<&str> = template.lines().collect();
    Kind::Stale(
        text.lines()
            .filter(|line| !have.contains(line) && !generated(line))
            .map(str::to_string)
            .collect(),
    )
}

fn compose(template: &str, kept: &[String]) -> String {
    let mut out = template.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    let have: Vec<&str> = template.lines().collect();
    for line in kept {
        if !have.contains(&line.as_str()) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn export_name(line: &str) -> Option<&str> {
    let name = line
        .trim()
        .strip_prefix("export ")?
        .split(['=', ' ', '"', '\''])
        .next()?
        .trim();
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some(name)
}

/// `; added NAME; kept NAME`. Names the template already has are neither.
fn change_suffix(before: &str, after: &str, template: &str) -> String {
    let old: Vec<&str> = before.lines().filter_map(export_name).collect();
    let new: Vec<&str> = after.lines().filter_map(export_name).collect();
    let own: Vec<&str> = template.lines().filter_map(export_name).collect();
    let added: Vec<&str> = new.iter().copied().filter(|n| !old.contains(n)).collect();
    let kept: Vec<&str> = new
        .iter()
        .copied()
        .filter(|n| old.contains(n) && !own.contains(n))
        .collect();
    let mut bits = Vec::new();
    if !added.is_empty() {
        bits.push(format!("added {}", added.join(", ")));
    }
    if !kept.is_empty() {
        bits.push(format!("kept {}", kept.join(", ")));
    }
    if bits.is_empty() {
        String::new()
    } else {
        format!("; {}", bits.join("; "))
    }
}

fn write_new(path: &Path, text: &str, template: &str) -> std::io::Result<()> {
    ensure_parent(path)?;
    std::fs::write(path, text)?;
    std::fs::write(mark_path(path), template)
}

/// Write the env file, or leave a hand edit and a file ljos did not write.
/// A dry run writes nothing.
pub(crate) fn install(path: &Path, name: &str, dry: bool) -> crate::Step {
    let template = crate::seat_env_text(name);
    let source = format!("source {}", path.display());
    let verb = if dry { "would leave" } else { "left" };
    match classify(path, &template) {
        Kind::Current => {
            if !dry && read_mark(path).as_deref() != Some(template.as_str()) {
                if let Err(err) =
                    ensure_parent(path).and_then(|()| std::fs::write(mark_path(path), &template))
                {
                    return step(format!("{}: {err}", path.display()), false);
                }
            }
            step(format!("{} is current; {source}", path.display()), true)
        }
        Kind::Hand => step(
            format!("{verb} {}; it was edited by hand", path.display()),
            true,
        ),
        Kind::Foreign => step(
            format!("{verb} {}; ljos did not write it", path.display()),
            true,
        ),
        Kind::Missing => {
            if dry {
                return step(format!("would write {}; {source}", path.display()), true);
            }
            match write_new(path, &template, &template) {
                Ok(()) => step(format!("wrote {}; {source}", path.display()), true),
                Err(err) => step(format!("{}: {err}", path.display()), false),
            }
        }
        Kind::Stale(kept) => {
            let before = std::fs::read_to_string(path).unwrap_or_default();
            let after = compose(&template, &kept);
            let suffix = change_suffix(&before, &after, &template);
            if dry {
                return step(
                    format!("would write {}; {source}{suffix}", path.display()),
                    true,
                );
            }
            match write_new(path, &after, &template) {
                Ok(()) => step(format!("wrote {}; {source}{suffix}", path.display()), true),
                Err(err) => step(format!("{}: {err}", path.display()), false),
            }
        }
    }
}

/// Doctor's `runner env` row. A stale file ljos wrote, and a hand edit,
/// are warnings ([`is_env_warning`]). A missing file, and a file ljos
/// did not write, are not.
pub(crate) fn doctor_env(name: &str, path: &Path) -> (bool, String) {
    let template = crate::seat_env_text(name);
    match classify(path, &template) {
        Kind::Missing => (
            false,
            format!("{name}: no ljos env file; ljos onboard --harness {name}"),
        ),
        Kind::Current => (true, format!("{name}: source {}", path.display())),
        Kind::Stale(_) => (false, format!("{name}: {}{STALE_MARK}", path.display())),
        Kind::Hand => (false, format!("{name}: {}{EDITED_MARK}", path.display())),
        Kind::Foreign => (false, format!("{name}: {}{FOREIGN_MARK}", path.display())),
    }
}

pub(crate) fn is_env_warning(state: &str) -> bool {
    state.contains(STALE_MARK) || state.contains(EDITED_MARK)
}

fn refresh_one(name: &str, path: &Path) -> bool {
    let template = crate::seat_env_text(name);
    if matches!(classify(path, &template), Kind::Missing) {
        return true;
    }
    let before = std::fs::read_to_string(path).unwrap_or_default();
    let done = install(path, name, false);
    if !done.ok {
        eprintln!("ljos: {}", done.detail);
        return false;
    }
    let after = std::fs::read_to_string(path).unwrap_or_default();
    if after != before {
        eprintln!(
            "ljos: refreshed {name}: {}{}",
            path.display(),
            change_suffix(&before, &after, &template)
        );
    } else if done.detail.contains("edited by hand") {
        eprintln!("ljos: {name}: {}{EDITED_MARK}", path.display());
    } else if done.detail.contains("did not write") {
        eprintln!("ljos: {name}: {}{FOREIGN_MARK}", path.display());
    }
    true
}

/// Rewrite env files whose bytes are still what ljos wrote.
///
/// False when a rewrite could not be recorded, so the next run tries
/// again. A hand edit, and a file ljos did not write, are not failures.
pub fn refresh_installed() -> bool {
    let all = match crate::harnesses_with_shipped(&crate::harnesses_path()) {
        Ok(all) => all,
        Err(err) => {
            eprintln!("ljos: runner env files were not refreshed: {err:#}");
            return true;
        }
    };
    let mut held = true;
    let mut seen = std::collections::BTreeSet::new();
    for harness in &all.harness {
        if !harness.shell {
            continue;
        }
        let path = crate::env_path_for(&crate::harnesses_path(), harness);
        if seen.insert(path.clone()) && !refresh_one(&harness.name, &path) {
            held = false;
        }
    }
    held
}

fn stamp_path() -> Option<PathBuf> {
    let path = crate::data_base()?.join("ljos").join("env-refresh-version");
    path.is_absolute().then_some(path)
}

fn write_stamp() {
    let Some(path) = stamp_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, format!("{}\n", env!("CARGO_PKG_VERSION")));
}

/// The first run of this version. Later runs do nothing. `ljos doctor`
/// does not call this.
pub fn refresh_on_new_version() {
    if stamp_path().is_some_and(|path| {
        std::fs::read_to_string(path).is_ok_and(|text| text.trim() == env!("CARGO_PKG_VERSION"))
    }) {
        return;
    }
    if refresh_installed() {
        write_stamp();
    }
}

/// `ljos doctor --fix`: refresh even when this version already ran.
pub fn refresh_for_fix() {
    if refresh_installed() {
        write_stamp();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_env_is_not_called_stale() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grokbot.env");
        let (ok, state) = doctor_env("grokbot", &path);
        assert!(!ok, "{state}");
        assert!(state.contains("no ljos env file"), "{state}");
        assert!(!state.contains("is stale"), "{state}");
    }

    #[test]
    fn install_records_the_bytes_and_a_dry_run_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grokbot.env");
        assert!(install(&path, "grokbot", true)
            .detail
            .contains("would write"));
        assert!(!path.exists() && !mark_path(&path).exists());
        assert!(install(&path, "grokbot", false).detail.contains("wrote"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, crate::seat_env_text("grokbot"));
        assert_eq!(std::fs::read_to_string(mark_path(&path)).unwrap(), text);
        assert!(install(&path, "grokbot", false)
            .detail
            .contains("is current"));
    }

    #[test]
    fn a_current_file_with_no_record_is_marked_and_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grokbot.env");
        let text = crate::seat_env_text("grokbot");
        std::fs::write(&path, &text).unwrap();
        assert!(install(&path, "grokbot", false)
            .detail
            .contains("is current"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(read_mark(&path).as_deref(), Some(text.as_str()));
    }
}
