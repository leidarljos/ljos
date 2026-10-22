//! The prompt the hook kept and its bind, as a tool call reaches them.
//! A path word is resolved before it is compared: variables and `~`
//! expanded, joined to the directory the shell stands in, `.` and `..`
//! taken out, and the symlinks of the part that exists followed. A word
//! the guard cannot resolve is compared by its file name alone.

use std::path::{Component, Path, PathBuf};

/// The file names of the prompt the hook kept and its bind.
pub(crate) const NAMES: &[&str] = &["latest-prompt", "latest-prompt.bind"];

/// Programs that remove, move or link what they are given: run on a
/// directory above the kept prompt, they replace it with everything else.
const MOVERS: &[&str] = &[
    "mv", "rm", "rmdir", "unlink", "ln", "rsync", "shred", "trash",
];

/// The runtime ljos directories a tool call could write into, resolved:
/// the one this process uses, `/run/user/UID/ljos`, and the fallback a
/// shell without `XDG_RUNTIME_DIR` uses.
fn dirs() -> Vec<PathBuf> {
    let uid = crate::euid();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut out: Vec<PathBuf> = [
        crate::runtime_dir_path(),
        PathBuf::from(format!("/run/user/{uid}/ljos")),
        crate::runtime_fallback(&std::env::temp_dir(), home.as_deref(), uid),
    ]
    .iter()
    .map(|d| resolve(d))
    .collect();
    out.dedup();
    out
}

/// The value of `$NAME` for a path: the environment, and the user id the
/// shell sets itself.
fn var(name: &str) -> Option<String> {
    match name {
        "UID" | "EUID" => Some(crate::euid().to_string()),
        _ => std::env::var(name).ok(),
    }
}

/// `word` with `~`, `$NAME` and `${NAME}`, `${NAME:-x}` and `${NAME-x}`
/// expanded. `None` for anything else a shell would expand.
fn expand(word: &str) -> Option<String> {
    let word = match word.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", var("HOME")?),
        Some(_) => return None,
        None => word.to_string(),
    };
    let mut out = String::new();
    let mut rest = word.as_str();
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        if let Some(inner) = rest.strip_prefix('{') {
            let close = inner.find('}')?;
            let body = &inner[..close];
            rest = &inner[close + 1..];
            let (name, default, empty_too) = match body.split_once(":-") {
                Some((n, d)) => (n, Some(d), true),
                None => match body.split_once('-') {
                    Some((n, d)) => (n, Some(d), false),
                    None => (body, None, false),
                },
            };
            let set = var(name).filter(|v| !(empty_too && v.is_empty()));
            out.push_str(&match (set, default) {
                (Some(v), _) => v,
                (None, Some(d)) => expand(d)?,
                (None, None) => return None,
            });
        } else {
            let len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            if len == 0 {
                return None;
            }
            out.push_str(&var(&rest[..len])?);
            rest = &rest[len..];
        }
    }
    out.push_str(rest);
    Some(out)
}

/// `path` as the filesystem reaches it: the longest part that exists with
/// its symlinks followed, and the rest with `.` and `..` taken out.
fn resolve(path: &Path) -> PathBuf {
    for top in path.ancestors() {
        if let Ok(real) = std::fs::canonicalize(top) {
            let rest = path.strip_prefix(top).unwrap_or(Path::new(""));
            return crate::lexical_path(&real.join(rest));
        }
    }
    crate::lexical_path(path)
}

/// The path a shell word names, standing in `cwd`. `None` when the word
/// needs an expansion the guard does not make, or is relative and the
/// directory is not known.
pub(crate) fn path_of(word: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let word = expand(word)?;
    let path = Path::new(&word);
    if path.is_absolute() {
        Some(resolve(path))
    } else {
        cwd.map(|cwd| resolve(&cwd.join(path)))
    }
}

/// Whether the shell pattern `pat` matches the whole of `name`.
fn glob(pat: &[char], name: &[char]) -> bool {
    match pat.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|i| glob(rest, &name[i..])),
        Some(('?', rest)) => !name.is_empty() && glob(rest, &name[1..]),
        Some(('[', rest)) => {
            let Some(close) = rest.iter().skip(1).position(|c| *c == ']').map(|i| i + 1) else {
                return name.first() == Some(&'[') && glob(rest, &name[1..]);
            };
            let Some((first, tail)) = name.split_first() else {
                return false;
            };
            let set = &rest[..close];
            let (negate, set) = match set.split_first() {
                Some(('!' | '^', s)) => (true, s),
                _ => (false, set),
            };
            let mut hit = false;
            let mut i = 0;
            while i < set.len() {
                if set.get(i + 1) == Some(&'-') && i + 2 < set.len() {
                    hit |= (set[i]..=set[i + 2]).contains(first);
                    i += 3;
                } else {
                    hit |= set[i] == *first;
                    i += 1;
                }
            }
            hit != negate && glob(&rest[close + 1..], tail)
        }
        Some((c, rest)) => name.first() == Some(c) && glob(rest, &name[1..]),
    }
}

/// Whether each component of `pat`, a pattern or a name, matches the one
/// beside it in `real`.
fn same_parts(pat: &[Component<'_>], real: &[Component<'_>]) -> bool {
    pat.len() == real.len()
        && pat.iter().zip(real).all(|(p, r)| match (p, r) {
            (Component::Normal(p), Component::Normal(r)) => {
                let p: Vec<char> = p.to_string_lossy().chars().collect();
                let r: Vec<char> = r.to_string_lossy().chars().collect();
                glob(&p, &r)
            }
            _ => p == r,
        })
}

/// Whether `path` is a runtime ljos directory or inside one.
pub(crate) fn inside(path: &Path) -> bool {
    let p: Vec<Component<'_>> = path.components().collect();
    dirs().iter().any(|d| {
        let d: Vec<Component<'_>> = d.components().collect();
        p.len() >= d.len() && same_parts(&p[..d.len()], &d)
    })
}

/// The name right below `path` on the way down to a runtime ljos
/// directory, when `path` is above one. The root is left to the rules
/// that refuse removing it.
pub(crate) fn above(path: &Path) -> Option<String> {
    let p: Vec<Component<'_>> = path.components().collect();
    dirs().iter().find_map(|d| {
        let d: Vec<Component<'_>> = d.components().collect();
        (p.len() > 1 && p.len() < d.len() && same_parts(&p, &d[..p.len()]))
            .then(|| d[p.len()].as_os_str().to_string_lossy().into_owned())
    })
}

/// Whether a word's file name is the kept prompt's or its bind's.
pub(crate) fn names_one(word: &str) -> bool {
    let name = word
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(word);
    NAMES.contains(&name)
}

/// The word of one command, its wrappers off, that reaches the kept prompt
/// when the shell stands in `cwd`: a redirection into the runtime ljos
/// directory, a path into it given to anything but a reader, or a
/// directory above it given to a program in [`MOVERS`].
pub(crate) fn reached(words: &[String], cwd: Option<&Path>, reader: bool) -> Option<String> {
    let cmd = words.first()?.rsplit('/').next().unwrap_or("");
    let mut operands: Vec<&str> = Vec::new();
    let mut it = words.iter().skip(1);
    while let Some(w) = it.next() {
        if w == ">" {
            let target = it.next()?;
            if names_one(target) || path_of(target, cwd).is_some_and(|p| inside(&p)) {
                return Some(target.clone());
            }
            continue;
        }
        if w.contains(char::is_whitespace) || w.contains("://") {
            continue;
        }
        operands.push(w);
        if let Some((_, value)) = w.split_once('=') {
            operands.push(value);
        }
    }
    if reader {
        return None;
    }
    if let Some(w) = operands.iter().find(|w| names_one(w)) {
        return Some((*w).to_string());
    }
    let paths: Vec<(&str, Option<PathBuf>)> =
        operands.iter().map(|w| (*w, path_of(w, cwd))).collect();
    if let Some((w, _)) = paths.iter().find(|(_, p)| p.as_deref().is_some_and(inside)) {
        return Some((*w).to_string());
    }
    if !MOVERS.contains(&cmd) {
        return None;
    }
    let files: Vec<&(&str, Option<PathBuf>)> =
        paths.iter().filter(|(w, _)| !w.starts_with('-')).collect();
    let last = files.len().saturating_sub(1);
    for (i, (w, p)) in files.iter().enumerate() {
        let Some(below) = p.as_deref().and_then(above) else {
            continue;
        };
        // `mv x DIR/` puts x inside DIR, which replaces nothing unless x
        // is named as what DIR holds on the way down.
        let into = i == last && i > 0 && matches!(cmd, "mv" | "ln" | "rsync");
        let named = files[..i]
            .iter()
            .any(|(s, _)| s.trim_end_matches('/').rsplit('/').next() == Some(below.as_str()));
        if !into || named {
            return Some((*w).to_string());
        }
    }
    None
}

/// Where the shell stands after `cd` or `pushd` with `words`, from `cwd`.
/// `None` when that is not known.
pub(crate) fn after_cd(words: &[String], cwd: Option<&Path>) -> Option<PathBuf> {
    let target = words
        .iter()
        .skip(1)
        .find(|w| !w.starts_with('-') || *w == "-");
    match target.map(String::as_str) {
        None => var("HOME").map(PathBuf::from).map(|h| resolve(&h)),
        Some("-") => None,
        Some(t) => path_of(t, cwd),
    }
}
