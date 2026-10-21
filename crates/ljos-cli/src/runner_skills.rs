//! Runner skills ljos installed, refreshed when a new version starts.
//!
//! `onboard` writes `<skills>/ljos/SKILL.md` and records the sha256 of those
//! bytes in `<skills>/ljos/.ljos-skill-sha256`. The first run of a new ljos,
//! including `ljos-mcp` as a session starts, rewrites a file whose bytes
//! still match that hash. `ljos doctor` only reports; `ljos doctor --fix`
//! rewrites too. A file whose bytes differ from the recorded hash was
//! edited by hand: it is left in place and the doctor warns, which does
//! not fail the command.
//!
//! A skill installed before hashes were recorded has no sidecar. It is
//! refreshed once when it still starts as an ljos skill, and the hash is
//! recorded then. A file that does not, with no hash, is left alone.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const STALE_MARK: &str = " is stale; ljos doctor --fix";
const EDITED_MARK: &str = " was edited by hand; ljos left it";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillKind {
    Missing,
    Current,
    StaleOwned,
    HandEdited,
}

/// Sha256 of the bytes ljos wrote, or of the file as it stands.
#[must_use]
pub(crate) fn skill_hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn sidecar(path: &Path) -> PathBuf {
    path.with_file_name(".ljos-skill-sha256")
}

fn recorded(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(sidecar(path)).ok()?;
    let hash = text.trim();
    (hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| hash.to_ascii_lowercase())
}

fn record(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = sidecar(path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(sidecar(path), format!("{}\n", skill_hash(bytes)))
}

fn looks_like_ljos_skill(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    text.starts_with("---\nname: ljos\n") && text.contains("\n# The seat protocol\n")
}

pub(crate) fn classify(path: &Path, want: &str) -> SkillKind {
    let Ok(bytes) = std::fs::read(path) else {
        return SkillKind::Missing;
    };
    if bytes == want.as_bytes() {
        return SkillKind::Current;
    }
    let have = skill_hash(&bytes);
    match recorded(path) {
        Some(saved) if saved == have => SkillKind::StaleOwned,
        Some(_) => SkillKind::HandEdited,
        None if looks_like_ljos_skill(&bytes) => SkillKind::StaleOwned,
        None => SkillKind::HandEdited,
    }
}

fn write_skill_file(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)?;
    record(path, text.as_bytes())
}

/// Write the skill, or leave a hand edit. A dry run writes nothing.
pub(crate) fn install_skill(path: &Path, dry: bool) -> crate::Step {
    let want = crate::skill_text();
    let step = |detail: String, ok: bool| crate::Step {
        what: "skill".into(),
        detail,
        ok,
    };
    match classify(path, &want) {
        SkillKind::Current => {
            if !dry {
                let _ = record(path, want.as_bytes());
            }
            step(format!("{} is current", path.display()), true)
        }
        SkillKind::HandEdited => step(
            format!(
                "{} {}; it was edited by hand",
                if dry { "would leave" } else { "left" },
                path.display()
            ),
            true,
        ),
        SkillKind::Missing | SkillKind::StaleOwned => {
            if dry {
                step(format!("would write {}", path.display()), true)
            } else {
                match write_skill_file(path, &want) {
                    Ok(()) => step(format!("wrote {}", path.display()), true),
                    Err(err) => step(format!("{}: {err}", path.display()), false),
                }
            }
        }
    }
}

/// The doctor's `runner skill` row. A stale file ljos wrote, and a hand
/// edit, are warnings ([`is_skill_warning`]). An absent skill is not.
#[must_use]
pub(crate) fn doctor_skill(name: &str, path: Option<&Path>) -> (bool, String) {
    let Some(path) = path else {
        return (false, format!("{name}: no skills directory named"));
    };
    match classify(path, &crate::skill_text()) {
        SkillKind::Missing => (
            false,
            format!("{name}: absent; ljos onboard --harness {name}"),
        ),
        SkillKind::Current => (true, format!("{name}: {}", path.display())),
        SkillKind::StaleOwned => (false, format!("{name}: {}{STALE_MARK}", path.display())),
        SkillKind::HandEdited => (false, format!("{name}: {}{EDITED_MARK}", path.display())),
    }
}

/// Whether a `runner skill` row is a warning rather than a failure.
#[must_use]
pub(crate) fn is_skill_warning(state: &str) -> bool {
    state.contains(STALE_MARK) || state.contains(EDITED_MARK)
}

fn hash_matches(path: &Path, bytes: &[u8]) -> bool {
    recorded(path).is_some_and(|saved| saved == skill_hash(bytes))
}

/// Rewrite installed skills whose bytes are still what ljos wrote.
///
/// Returns false when a rewrite could not be recorded, so the caller tries
/// again on the next run. A hand edit is reported and is not a failure.
pub fn refresh_installed() -> bool {
    let all = match crate::harnesses_with_shipped(&crate::harnesses_path()) {
        Ok(all) => all,
        Err(err) => {
            eprintln!("ljos: runner skills were not refreshed: {err:#}");
            return true;
        }
    };
    let want = crate::skill_text();
    let mut held = true;
    let mut seen = std::collections::BTreeSet::new();
    for harness in &all.harness {
        let Some(dir) = harness.skills.as_deref() else {
            continue;
        };
        let skill = crate::expand(dir).join("ljos").join("SKILL.md");
        if !seen.insert(skill.clone()) {
            continue;
        }
        match classify(&skill, &want) {
            SkillKind::Missing => {}
            SkillKind::Current => {
                if !hash_matches(&skill, want.as_bytes())
                    && record(&skill, want.as_bytes()).is_err()
                {
                    held = false;
                }
            }
            SkillKind::StaleOwned => match write_skill_file(&skill, &want) {
                Ok(()) => {
                    eprintln!("ljos: refreshed {}: {}", harness.name, skill.display());
                }
                Err(err) => {
                    eprintln!("ljos: {}: {err}", skill.display());
                    held = false;
                }
            },
            SkillKind::HandEdited => {
                eprintln!("ljos: {}: {}{EDITED_MARK}", harness.name, skill.display());
            }
        }
    }
    held
}

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn stamp_path() -> Option<PathBuf> {
    let path = crate::data_base()?
        .join("ljos")
        .join("skill-refresh-version");
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

    fn old_skill() -> &'static str {
        "---\nname: ljos\ndescription: old\n---\n\n# The seat protocol\n\nold\n"
    }

    #[test]
    fn a_matching_hash_is_owned_and_a_different_file_is_a_hand_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        let old = old_skill();
        std::fs::write(&path, old).unwrap();
        record(&path, old.as_bytes()).unwrap();
        let want = "---\nname: ljos\ndescription: new\n---\n\n# The seat protocol\n\nnew\n";
        assert_eq!(classify(&path, want), SkillKind::StaleOwned);

        std::fs::write(&path, format!("{old}hand\n")).unwrap();
        assert_eq!(classify(&path, want), SkillKind::HandEdited);

        std::fs::remove_file(sidecar(&path)).unwrap();
        assert_eq!(
            classify(&path, want),
            SkillKind::StaleOwned,
            "a skill with no hash that still starts as ljos is refreshed once"
        );
        std::fs::write(&path, b"not a skill\n").unwrap();
        assert_eq!(classify(&path, want), SkillKind::HandEdited);
        std::fs::write(&path, want).unwrap();
        assert_eq!(classify(&path, want), SkillKind::Current);
    }

    #[test]
    fn install_refreshes_a_stale_copy_and_leaves_a_hand_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        let old = old_skill();
        std::fs::write(&path, old).unwrap();
        record(&path, old.as_bytes()).unwrap();
        let step = install_skill(&path, false);
        assert!(step.ok && step.detail.contains("wrote"), "{step:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), crate::skill_text());
        assert_eq!(
            recorded(&path).as_deref(),
            Some(skill_hash(crate::skill_text().as_bytes()).as_str())
        );

        let edited = format!("{}hand\n", crate::skill_text());
        std::fs::write(&path, &edited).unwrap();
        let step = install_skill(&path, false);
        assert!(step.ok, "{step:?}");
        assert!(step.detail.contains("edited by hand"), "{step:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    }
}
