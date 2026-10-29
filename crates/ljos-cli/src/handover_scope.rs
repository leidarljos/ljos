//! `ljos handover --scope NAME`. The bag carries only the atoms tagged
//! `scope:NAME`, sealed with age to that scope's recipients, so memory can
//! go to a peer without the rest of the store. No atom travels in
//! plaintext. The deeds that go are the ones the satchel needs and the
//! ones those atoms cite.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;

/// A sealed scope file in a bag: `data/atoms/<scope>.jsonl.age`.
pub(crate) const SEALED_SUFFIX: &str = ".jsonl.age";

/// Export the pack's `workspace` beside the bag, keep the atoms of
/// `scope`, and seal them into `atoms_dir`. Returns the lines to print and
/// the accessions those atoms cite. The plaintext export is removed before
/// this returns.
///
/// # Errors
///
/// The scope has no recipients, age refused, or the export could not be
/// read. Nothing is sealed then, and the plaintext export is still removed.
pub(crate) fn enclose(
    out: &Path,
    atoms_dir: &Path,
    workspace: &str,
    scope: &str,
) -> Result<(Vec<String>, String)> {
    let recipients = crate::sync::handover_recipients(scope)?;
    let export = private_dir(out)?;
    let result = (|| {
        let said = crate::run_captured(
            "packset",
            &["export", "--into", &export.display().to_string(), workspace],
        )?;
        let (kept, left) = scoped_lines(&export, scope)?;
        let cited = cited_by(&kept, &said.stdout);
        std::fs::create_dir_all(atoms_dir)
            .with_context(|| format!("create {}", atoms_dir.display()))?;
        let file = atoms_dir.join(format!("{scope}{SEALED_SUFFIX}"));
        let mut plain = kept.join("\n");
        if !plain.is_empty() {
            plain.push('\n');
        }
        crate::sync::seal_to(&plain, &recipients, &file)?;
        let lines = vec![format!(
            "{} scope {scope} atoms sealed to {} recipients in {}; {left} atoms of other scopes left out",
            kept.len(),
            recipients.len(),
            file.display()
        )];
        Ok((lines, cited))
    })();
    let _ = std::fs::remove_dir_all(&export);
    result
}

/// A directory only this user can read, beside the bag and outside its
/// payload, for the plaintext export.
fn private_dir(out: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;
    let dir = out.join(format!(".scope-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

/// The atom lines under `export` that carry the entity `scope:<scope>`,
/// and how many lines were left out.
fn scoped_lines(export: &Path, scope: &str) -> Result<(Vec<String>, usize)> {
    let tag = format!("scope:{scope}");
    let mut kept = Vec::new();
    let mut left = 0usize;
    let mut files: Vec<PathBuf> = std::fs::read_dir(export)
        .with_context(|| format!("read {}", export.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    for file in files {
        let text =
            std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let atom: Value = serde_json::from_str(line)
                .with_context(|| format!("an atom in {}", file.display()))?;
            if entities(&atom).any(|e| e == tag) {
                kept.push(line.to_string());
            } else {
                left += 1;
            }
        }
    }
    Ok((kept, left))
}

fn entities(atom: &Value) -> impl Iterator<Item = &str> {
    atom.get("entities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

/// The accessions in `cited` that one of `kept` names as an entity.
fn cited_by(kept: &[String], cited: &str) -> String {
    let named: std::collections::BTreeSet<String> = kept
        .iter()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .flat_map(|atom| {
            entities(&atom)
                .flat_map(|e| [e.to_string(), e.rsplit(':').next().unwrap_or(e).to_string()])
                .collect::<Vec<_>>()
        })
        .collect();
    cited
        .lines()
        .map(str::trim)
        .filter(|acc| !acc.is_empty() && named.contains(*acc))
        .map(|acc| format!("{acc}\n"))
        .collect()
}

/// The atoms in a bag's sealed scope files that this machine's identities
/// open, and a line for each file they do not.
///
/// # Errors
///
/// age failed on a file sealed to this machine, or an opened file holds a
/// line that is not an atom.
pub(crate) fn open_sealed(atoms_dir: &Path) -> Result<(Vec<Value>, Vec<String>)> {
    let Ok(entries) = std::fs::read_dir(atoms_dir) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| sealed_scope(p).is_some())
        .collect();
    files.sort();
    let (mut atoms, mut lines) = (Vec::new(), Vec::new());
    for file in files {
        let scope = sealed_scope(&file).unwrap_or_default();
        let Some(plain) = crate::sync::open_scoped(&file, &scope)? else {
            lines.push(format!(
                "scope {scope} atoms are not sealed to this machine; left closed"
            ));
            continue;
        };
        let mut n = 0usize;
        for line in plain.lines().filter(|l| !l.trim().is_empty()) {
            let atom: Value = serde_json::from_str(line)
                .with_context(|| format!("an atom in {}", file.display()))?;
            if !entities(&atom).any(|e| e == format!("scope:{scope}")) {
                bail!(
                    "{}: an atom without scope:{scope} was sealed as that scope",
                    file.display()
                );
            }
            atoms.push(atom);
            n += 1;
        }
        lines.push(format!("scope {scope}: {n} sealed atoms opened"));
    }
    Ok((atoms, lines))
}

/// The scope a sealed file in a bag is for.
pub(crate) fn sealed_scope(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let scope = name.strip_suffix(SEALED_SUFFIX)?;
    (!scope.is_empty() && !scope.starts_with('.')).then(|| scope.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_atoms_tagged_with_the_scope_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("seat.jsonl"),
            concat!(
                "{\"id\":\"a\",\"text\":\"cloud\",\"entities\":[\"scope:cloud\",\"deed-1\"]}\n",
                "{\"id\":\"b\",\"text\":\"box\",\"entities\":[\"scope:grokbot\",\"deed-2\"]}\n",
                "{\"id\":\"c\",\"text\":\"untagged\",\"entities\":[]}\n",
                "{\"id\":\"d\",\"text\":\"in text only\",\"entities\":[\"scope:cloudy\"]}\n",
            ),
        )
        .unwrap();
        let (kept, left) = scoped_lines(dir.path(), "cloud").unwrap();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].contains("\"id\":\"a\""));
        assert_eq!(left, 3);
        assert_eq!(cited_by(&kept, "deed-1\ndeed-2\n"), "deed-1\n");
    }

    #[test]
    fn a_sealed_file_names_its_scope() {
        assert_eq!(
            sealed_scope(Path::new("data/atoms/cloud.jsonl.age")).as_deref(),
            Some("cloud")
        );
        assert_eq!(sealed_scope(Path::new("data/atoms/seat.jsonl")), None);
        assert_eq!(sealed_scope(Path::new("data/atoms/.jsonl.age")), None);
    }
}
