//! Runner env files ljos installed, refreshed when a new version starts.
//!
//! `onboard` writes the shell runner's env file and records the bytes it
//! wrote in `.<filename>.ljos-wrote` beside that file. The first run of a
//! new ljos, including `ljos-mcp` as a session starts, rewrites a file
//! that is still those bytes, or those bytes plus lines the user added.
//! Added lines are kept at the end, and ljos says what it changed.
//! `ljos doctor` only reports; `ljos doctor --fix` rewrites too. A file
//! whose ljos lines were edited by hand is left in place. The doctor
//! warns, which does not fail the command.
//!
//! An env file from before those bytes were recorded is refreshed once
//! when it still starts as an ljos env. A line that is not part of the
//! generated text is kept. A file that does not, and has no record, was
//! not written by ljos and is left alone.

use std::path::{Path, PathBuf};

const STALE_MARK: &str = " is stale; ljos doctor --fix";
const EDITED_MARK: &str = " was edited by hand; ljos left it";
const FOREIGN_MARK: &str = " was not written by ljos; ljos left it";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Missing,
    Current,
    Stale { kept: Vec<String> },
    HandEdited,
    Foreign,
}

/// The bytes ljos last wrote for this env file, beside it.
fn record_path(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("env");
    path.with_file_name(format!(".{name}.ljos-wrote"))
}

fn recorded(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(record_path(path)).ok()?;
    (!text.is_empty()).then_some(text)
}

fn record(path: &Path, template: &str) -> std::io::Result<()> {
    if let Some(dir) = record_path(path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(record_path(path), template)
}

fn read_env(path: &Path) -> Result<Option<String>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Lines of `file` that are not the next line of `body`, in order.
/// `None` when a line of `body` is missing or changed: the user edited
/// ljos's text, rather than adding a line.
fn added_lines(body: &str, file: &str) -> Option<Vec<String>> {
    let body_lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    let mut extra = Vec::new();
    for line in file.lines() {
        if i < body_lines.len() && line == body_lines[i] {
            i += 1;
        } else {
            extra.push(line.to_string());
        }
    }
    (i == body_lines.len()).then_some(extra)
}

fn user_lines(extra: Vec<String>) -> Vec<String> {
    extra
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .collect()
}

/// An env ljos wrote starts with the seat header and carries the guard.
fn looks_like_ljos_env(text: &str) -> bool {
    text.starts_with("# Source this in the shell that runs the agent's commands.\n")
        && text.contains("LJOS_SEAT")
        && (text.contains("_ljos_guard") || text.contains("ljos/guard-bin"))
}

/// A line an older ljos env would have carried that this ljos no longer
/// writes. Dropped on the one refresh of a file with no record, so a
/// changed shell block is not appended under the new one. A line the user
/// added that is only `fi` or `if` is kept once a record exists; this
/// guess is only for the unrecorded file.
fn legacy_generated(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    // A comment this ljos still writes matches the template. Anything else
    // was added by the user.
    if t.starts_with('#') {
        return false;
    }
    if t.contains("LJOS_") || t.contains("_ljos") {
        return true;
    }
    if t.starts_with("export PATH=") || t.starts_with("unset ") {
        return true;
    }
    matches!(t, "fi" | "then" | "do" | "done" | "esac" | ";;" | "else")
        || t.starts_with("if ")
        || t.starts_with("elif ")
        || t.starts_with("case ")
        || t.starts_with(";;")
        || t.starts_with("*)")
}

/// Lines of an unrecorded ljos env that this ljos did not generate.
fn legacy_user_lines(file: &str, template: &str) -> Vec<String> {
    let template_lines: Vec<&str> = template.lines().collect();
    let mut i = 0;
    let mut extra = Vec::new();
    for line in file.lines() {
        if i < template_lines.len() && line == template_lines[i] {
            i += 1;
            continue;
        }
        if legacy_generated(line) {
            continue;
        }
        if let Some(pos) = template_lines[i..].iter().position(|cand| *cand == line) {
            i += pos + 1;
            continue;
        }
        extra.push(line.to_string());
    }
    extra
}

fn classify(path: &Path, template: &str) -> Kind {
    let text = match read_env(path) {
        Ok(None) => return Kind::Missing,
        Ok(Some(text)) => text,
        // Unreadable, or not text: not a file this ljos can claim.
        Err(_) => return Kind::Foreign,
    };
    if text == template {
        return Kind::Current;
    }
    if let Some(body) = recorded(path) {
        return match added_lines(&body, &text) {
            Some(_) if body == template => Kind::Current,
            Some(extra) => Kind::Stale {
                kept: user_lines(extra),
            },
            None => Kind::HandEdited,
        };
    }
    if looks_like_ljos_env(&text) {
        return Kind::Stale {
            kept: legacy_user_lines(&text, template),
        };
    }
    Kind::Foreign
}

/// The new env: the text this ljos writes, then lines the user added
/// that the new text does not already carry.
fn compose(template: &str, kept: &[String]) -> String {
    let mut out = template.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    let have: Vec<&str> = template.lines().collect();
    for line in kept {
        if line.trim().is_empty() || have.contains(&line.as_str()) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn export_name(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("export ")?.trim();
    let name = rest.split(['=', ' ', '"', '\'']).next()?.trim();
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some(name)
}

fn lines_missing(source: &str, other: &str) -> Vec<String> {
    let mut pool: Vec<&str> = other
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let mut out = Vec::new();
    for line in source.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(pos) = pool.iter().position(|cand| *cand == line) {
            pool.remove(pos);
        } else {
            out.push(line.to_string());
        }
    }
    out
}

fn phrase(kind: &str, lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let mut names = Vec::new();
    let mut others = 0usize;
    for line in lines {
        match export_name(line) {
            Some(name) => names.push(name),
            None => others += 1,
        }
    }
    let mut bits = Vec::new();
    if !names.is_empty() {
        bits.push(names.join(", "));
    }
    if others == 1 {
        bits.push("1 other line".to_string());
    } else if others > 1 {
        bits.push(format!("{others} other lines"));
    }
    Some(format!("{kind} {}", bits.join(" and ")))
}

/// `; added ...; removed ...; kept ...`, or empty when nothing a person
/// would read changed.
fn change_suffix(before: &str, after: &str, kept: &[String]) -> String {
    let kept_set: std::collections::BTreeSet<&str> = kept.iter().map(String::as_str).collect();
    let added: Vec<String> = lines_missing(after, before)
        .into_iter()
        .filter(|line| !kept_set.contains(line.as_str()))
        .collect();
    let removed: Vec<String> = lines_missing(before, after)
        .into_iter()
        .filter(|line| !kept_set.contains(line.as_str()))
        .collect();
    let mut parts = Vec::new();
    if let Some(text) = phrase("added", &added) {
        parts.push(text);
    }
    if let Some(text) = phrase("removed", &removed) {
        parts.push(text);
    }
    if let Some(text) = phrase("kept", kept) {
        parts.push(text);
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("; {}", parts.join("; "))
    }
}

fn refreshed_line(
    name: &str,
    path: &Path,
    before: &str,
    after: &str,
    kept: &[String],
) -> Option<String> {
    if before == after {
        return None;
    }
    Some(format!(
        "ljos: refreshed {name}: {}{}",
        path.display(),
        change_suffix(before, after, kept)
    ))
}

fn write_env(path: &Path, text: &str, template: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)?;
    record(path, template)
}

fn step(detail: String, ok: bool) -> crate::Step {
    crate::Step {
        what: "env".into(),
        detail,
        ok,
    }
}

fn source_note(path: &Path) -> String {
    format!("source {}", path.display())
}

/// Write the env file, or leave a hand edit and a file ljos did not write.
/// A dry run writes nothing. Lines the user added to a file ljos wrote
/// are kept, and the step says what changed.
pub(crate) fn install(path: &Path, name: &str, dry: bool) -> crate::Step {
    let template = crate::seat_env_text(name);
    match classify(path, &template) {
        Kind::Current => {
            if !dry {
                let _ = record(path, &template);
            }
            step(
                format!("{} is current; {}", path.display(), source_note(path)),
                true,
            )
        }
        Kind::HandEdited => step(
            format!(
                "{} {}; it was edited by hand",
                if dry { "would leave" } else { "left" },
                path.display()
            ),
            true,
        ),
        Kind::Foreign => step(
            format!(
                "{} {}; ljos did not write it",
                if dry { "would leave" } else { "left" },
                path.display()
            ),
            true,
        ),
        Kind::Missing => {
            if dry {
                return step(
                    format!("would write {}; {}", path.display(), source_note(path)),
                    true,
                );
            }
            match write_env(path, &template, &template) {
                Ok(()) => step(
                    format!("wrote {}; {}", path.display(), source_note(path)),
                    true,
                ),
                Err(err) => step(format!("{}: {err}", path.display()), false),
            }
        }
        Kind::Stale { kept } => {
            let after = compose(&template, &kept);
            let before = std::fs::read_to_string(path).unwrap_or_default();
            let suffix = change_suffix(&before, &after, &kept);
            if dry {
                return step(
                    format!(
                        "would write {}; {}{suffix}",
                        path.display(),
                        source_note(path)
                    ),
                    true,
                );
            }
            if before == after {
                return match record(path, &template) {
                    Ok(()) => step(
                        format!("{} is current; {}", path.display(), source_note(path)),
                        true,
                    ),
                    Err(err) => step(format!("{}: {err}", path.display()), false),
                };
            }
            match write_env(path, &after, &template) {
                Ok(()) => step(
                    format!("wrote {}; {}{suffix}", path.display(), source_note(path)),
                    true,
                ),
                Err(err) => step(format!("{}: {err}", path.display()), false),
            }
        }
    }
}

/// The doctor's `runner env` row. A stale file ljos wrote, and a hand
/// edit, are warnings ([`is_env_warning`]). A missing file, and a file
/// ljos did not write, are not.
#[must_use]
pub(crate) fn doctor_env(name: &str, path: &Path) -> (bool, String) {
    let template = crate::seat_env_text(name);
    match classify(path, &template) {
        Kind::Missing => (
            false,
            format!("{name}: no ljos env file; ljos onboard --harness {name}"),
        ),
        Kind::Current => (true, format!("{name}: {}", source_note(path))),
        Kind::Stale { .. } => (false, format!("{name}: {}{STALE_MARK}", path.display())),
        Kind::HandEdited => (false, format!("{name}: {}{EDITED_MARK}", path.display())),
        Kind::Foreign => (false, format!("{name}: {}{FOREIGN_MARK}", path.display())),
    }
}

/// Whether a `runner env` row is a warning rather than a failure.
#[must_use]
pub(crate) fn is_env_warning(state: &str) -> bool {
    state.contains(STALE_MARK) || state.contains(EDITED_MARK)
}

/// Rewrite env files whose bytes are still what ljos wrote.
///
/// Returns false when a rewrite could not be recorded, so the caller tries
/// again on the next run. A hand edit, and a file ljos did not write, are
/// reported and are not failures.
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
        if !seen.insert(path.clone()) {
            continue;
        }
        if !refresh_one(&harness.name, &path) {
            held = false;
        }
    }
    held
}

fn refresh_one(name: &str, path: &Path) -> bool {
    let template = crate::seat_env_text(name);
    let before = std::fs::read_to_string(path).unwrap_or_default();
    match classify(path, &template) {
        Kind::Missing => true,
        Kind::Current => {
            if recorded(path).as_deref() == Some(template.as_str()) {
                return true;
            }
            record(path, &template).is_ok()
        }
        Kind::Stale { kept } => {
            let after = compose(&template, &kept);
            if after == before {
                return record(path, &template).is_ok();
            }
            match write_env(path, &after, &template) {
                Ok(()) => {
                    if let Some(line) = refreshed_line(name, path, &before, &after, &kept) {
                        eprintln!("{line}");
                    }
                    true
                }
                Err(err) => {
                    eprintln!("ljos: {}: {err}", path.display());
                    false
                }
            }
        }
        Kind::HandEdited => {
            eprintln!("ljos: {name}: {}{EDITED_MARK}", path.display());
            true
        }
        Kind::Foreign => {
            eprintln!("ljos: {name}: {}{FOREIGN_MARK}", path.display());
            true
        }
    }
}

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn stamp_path() -> Option<PathBuf> {
    let path = crate::data_base()?.join("ljos").join("env-refresh-version");
    path.is_absolute().then_some(path)
}

fn stamp_is_current() -> bool {
    stamp_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|text| text.trim() == version())
}

fn write_stamp() {
    let Some(path) = stamp_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, format!("{}\n", version()));
}

/// Refresh on the first run of this version. Later runs of the same
/// version do nothing. `ljos doctor` does not call this.
pub fn refresh_on_new_version() {
    if stamp_is_current() {
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

    fn old_body() -> String {
        crate::seat_env_text("grokbot")
            .lines()
            .filter(|line| {
                !line.contains("LJOS_GIT_HOOKS") && !line.contains("Git hooks directory")
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    /// Header, an obsolete shell block, the guard, and one user line.
    fn legacy_env() -> String {
        "\
# Source this in the shell that runs the agent's commands.\n\
# This runner has no MCP client, so the process tree is not its seat.\n\
# LJOS_SEAT is the name memory, ballots and trust use.\n\
export LJOS_SEAT=\"${LJOS_SEAT:-grokbot}\"\n\
if [ -z \"${LJOS_SEAT:-}\" ] && [ -n \"${CURSOR_CONVERSATION_ID:-}\" ]; then\n\
\t_ljos_named=$(ljos seat 2>/dev/null | head -n 1 | awk '{ print $NF }')\n\
\tcase \"$_ljos_named\" in\n\
\t\t\"\"|*[!A-Za-z0-9_-]*) LJOS_SEAT=grokbot-$(printf %.8s \"$CURSOR_CONVERSATION_ID\") ;;\n\
\t\t*) LJOS_SEAT=$_ljos_named ;;\n\
\tesac\n\
\texport LJOS_SEAT\n\
\tunset _ljos_named\n\
fi\n\
export LJOS_OLD_MARKER=1\n\
_ljos_guard=\"${XDG_DATA_HOME:-$HOME/.local/share}/ljos/guard-bin\"\n\
export BOX_NOTES=keep-me\n"
            .to_string()
    }

    fn parses(text: &str) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env.sh");
        std::fs::write(&path, text).unwrap();
        let out = std::process::Command::new("sh")
            .arg("-n")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}\n{}",
            text,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn a_recorded_file_refreshes_and_keeps_a_line_the_user_added() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grokbot.env");
        let old = old_body();
        let user = "export LJOS_EXTRA=user\nexport BOX_NOTES=keep-me\n";
        std::fs::write(&path, format!("{old}{user}")).unwrap();
        record(&path, old.as_str()).unwrap();
        let step = install(&path, "grokbot", false);
        assert!(step.ok, "{step:?}");
        assert!(step.detail.contains("wrote"), "{step:?}");
        assert!(step.detail.contains("LJOS_GIT_HOOKS"), "{step:?}");
        assert!(step.detail.contains("kept"), "{step:?}");
        assert!(step.detail.contains("BOX_NOTES"), "{step:?}");
        let got = std::fs::read_to_string(&path).unwrap();
        let template = crate::seat_env_text("grokbot");
        assert!(got.starts_with(&template), "{got}");
        assert!(
            got.contains("export LJOS_EXTRA=user\n"),
            "a recorded line is kept even when it names LJOS_\n{got}"
        );
        assert!(got.contains("export BOX_NOTES=keep-me\n"), "{got}");
        assert_eq!(recorded(&path).as_deref(), Some(template.as_str()));
        parses(&got);

        let again = install(&path, "grokbot", false);
        assert!(again.detail.contains("is current"), "{again:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), got);
    }

    #[test]
    fn an_unrecorded_ljos_env_drops_old_shell_and_keeps_the_user_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grokbot.env");
        let legacy = legacy_env();
        std::fs::write(&path, &legacy).unwrap();
        assert!(recorded(&path).is_none());
        let step = install(&path, "grokbot", false);
        assert!(
            step.ok && step.detail.contains("LJOS_GIT_HOOKS"),
            "{step:?}"
        );
        assert!(
            step.detail.contains("kept") && step.detail.contains("BOX_NOTES"),
            "{step:?}"
        );
        assert!(
            step.detail.contains("removed") && step.detail.contains("LJOS_OLD_MARKER"),
            "{step:?}"
        );
        let got = std::fs::read_to_string(&path).unwrap();
        let template = crate::seat_env_text("grokbot");
        assert_eq!(got, format!("{template}export BOX_NOTES=keep-me\n"));
        assert!(!got.contains("LJOS_OLD_MARKER"), "{got}");
        parses(&got);
        let (ok, state) = doctor_env("grokbot", &path);
        assert!(ok && state.contains("source "), "{state}");
    }

    #[test]
    fn a_file_ljos_did_not_write_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grokbot.env");
        let text = "# written by an older ljos\nexport BOX_NOTES=keep-me\n";
        std::fs::write(&path, text).unwrap();
        let step = install(&path, "grokbot", false);
        assert!(step.ok, "{step:?}");
        assert!(step.detail.contains("did not write"), "{step:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert!(recorded(&path).is_none());
        let (ok, state) = doctor_env("grokbot", &path);
        assert!(!ok && state.contains(FOREIGN_MARK), "{state}");
        assert!(!state.contains("is stale"), "{state}");
        assert!(!state.contains("no ljos env file"), "{state}");
    }

    #[test]
    fn a_hand_edit_of_a_recorded_line_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grokbot.env");
        let old = old_body();
        record(&path, old.as_str()).unwrap();
        let edited = old.replacen(
            "# Source this in the shell that runs the agent's commands.",
            "# Source this in the shell that runs the agent's commands. edited",
            1,
        );
        std::fs::write(&path, &edited).unwrap();
        let dry = install(&path, "grokbot", true);
        assert!(
            dry.detail.contains("would leave") && dry.detail.contains("edited by hand"),
            "{dry:?}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        let step = install(&path, "grokbot", false);
        assert!(
            step.ok && step.detail.contains("edited by hand"),
            "{step:?}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        let (ok, state) = doctor_env("grokbot", &path);
        assert!(
            !ok && state.contains(EDITED_MARK) && !state.contains("no ljos env file"),
            "{state}"
        );
    }

    #[test]
    fn a_missing_env_is_not_called_stale() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grokbot.env");
        let (ok, state) = doctor_env("grokbot", &path);
        assert!(!ok, "{state}");
        assert!(state.contains("no ljos env file"), "{state}");
        assert!(state.contains("ljos onboard --harness grokbot"), "{state}");
        assert!(!state.contains("is stale"), "{state}");
        let dry = install(&path, "grokbot", true);
        assert!(dry.detail.contains("would write"), "{dry:?}");
        assert!(!path.exists(), "a dry run wrote the env");
    }
}
