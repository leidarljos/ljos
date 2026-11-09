//! Which `ljos` a hook will run, and whether another one on the seat is older.
//!
//! Doctor names each mismatch. A sitting names at most one. A hook stays quiet.
//! Nothing here rewrites PATH, a hook file, an env file, or a binary.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::Habitat;

const FIX: &str = "put the new binary first on PATH, or point LJOS_BIN at it";

struct Found {
    what: String,
    path: PathBuf,
    version: String,
}

/// One doctor row per mismatch. Empty when every reachable `ljos` is the same version.
pub(crate) fn rows() -> Vec<Habitat> {
    lines()
        .into_iter()
        .map(|state| Habitat {
            name: "version skew",
            state,
            ok: false,
        })
        .collect()
}

/// The first mismatch, for the sitting's open summary. `None` when they match.
pub(crate) fn summary_line() -> Option<String> {
    lines().into_iter().next().map(|line| format!("{line}\n"))
}

fn lines() -> Vec<String> {
    let found = reachable();
    let Some(hook) = found
        .iter()
        .position(|item| item.what == "LJOS_BIN")
        .or_else(|| found.iter().position(|item| item.what == "PATH"))
    else {
        return Vec::new();
    };
    let ran = &found[hook];
    let mut out = Vec::new();
    for (index, item) in found.iter().enumerate() {
        if index == hook || item.version == ran.version || same_file(&item.path, &ran.path) {
            continue;
        }
        out.push(format!(
            "label version-skew: {} {} is {}; hooks run {} which is {}; {FIX}",
            item.what,
            item.path.display(),
            item.version,
            ran.path.display(),
            ran.version
        ));
    }
    out
}

fn reachable() -> Vec<Found> {
    let mut out = Vec::new();
    if let Ok(me) = std::env::current_exe() {
        out.push(Found {
            what: "this ljos".into(),
            path: me,
            version: env!("CARGO_PKG_VERSION").to_string(),
        });
    }
    if let Ok(path) = which::which("ljos") {
        if let Some(version) = version_of(&path) {
            out.push(Found {
                what: "PATH".into(),
                path,
                version,
            });
        }
    }
    if let Some(path) = std::env::var_os("LJOS_BIN").map(PathBuf::from) {
        if path.is_file() {
            if let Some(version) = version_of(&path) {
                out.push(Found {
                    what: "LJOS_BIN".into(),
                    path,
                    version,
                });
            }
        }
    }
    out.extend(from_runner_files());
    out
}

fn from_runner_files() -> Vec<Found> {
    let file = crate::harnesses_path();
    let Ok(all) = crate::harnesses_from(&file) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for harness in &all.harness {
        if let Some(hooks) = harness.hooks.as_deref() {
            push_file(&mut out, &crate::expand(hooks), "hook file");
        }
        if let Some(plugin) = harness.plugin.as_deref() {
            push_file(&mut out, &crate::expand(plugin), "hook file");
        }
        let env = crate::env_path_for(&file, harness);
        if env_written_by_ljos(&env) {
            push_file(&mut out, &env, "env file");
        }
    }
    let grok = crate::home()
        .ok()
        .map(|home| home.join(".grok/hooks/ljos.json"));
    if let Some(path) = grok {
        push_file(&mut out, &path, "hook file");
    }
    out
}

fn push_file(out: &mut Vec<Found>, file: &Path, kind: &str) {
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    for path in ljos_paths_in(&text) {
        let Some(version) = version_of(&path) else {
            continue;
        };
        out.push(Found {
            what: format!("{kind} {}", file.display()),
            path,
            version,
        });
    }
}

fn env_written_by_ljos(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("env");
    if path.with_file_name(format!(".{name}.ljos-wrote")).is_file() {
        return true;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.starts_with("# Source this in the shell that runs the agent's commands.\n")
        && text.contains("LJOS_SEAT")
}

fn ljos_paths_in(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for token in text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ',')) {
        let token = token.trim_end_matches([';', ')', ']', '}']);
        if token.ends_with("/ljos") || token.ends_with("/ljos.exe") {
            let path = PathBuf::from(token);
            if path.is_file() && !out.iter().any(|have: &PathBuf| have == &path) {
                out.push(path);
            }
        }
    }
    out
}

fn version_of(path: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new(path);
    cmd.arg("--version");
    cmd.env_remove("LJOS_BIN");
    let out = crate::output_within(cmd, None, Duration::from_secs(2)).ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    semver_in(&text)
}

fn semver_in(text: &str) -> Option<String> {
    for word in text.split_whitespace() {
        let word = word.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
        let mut parts = word.split('.');
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if [major, minor, patch]
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        {
            return Some(word.to_string());
        }
    }
    None
}

fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
