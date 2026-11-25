//! The commit identity a tracker git child takes.
//!
//! git names the author and committer of a commit from the
//! `GIT_AUTHOR_*` and `GIT_COMMITTER_*` variables, then `author.*`,
//! `committer.*` and `user.*` config, then `EMAIL`. Config also arrives
//! through the environment: `git -c` writes `GIT_CONFIG_PARAMETERS`, and
//! `GIT_CONFIG_COUNT` with `GIT_CONFIG_KEY_<n>` and `GIT_CONFIG_VALUE_<n>`
//! adds more, an include among them. A seat that calls ljos exports
//! whatever its own runner set, so a tracker git child drops all of these
//! and keeps the identity git's config files give it. Dates are not
//! identity and stay.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::process::Command;

/// Variables git reads an author or committer from outside config: the
/// `GIT_*` names before it, `EMAIL` when config names no email.
const IDENTITY_ENV: &[&str] = &[
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "EMAIL",
];

const PARAMETERS: &str = "GIT_CONFIG_PARAMETERS";
const COUNT: &str = "GIT_CONFIG_COUNT";

/// git refuses a `GIT_CONFIG_COUNT` above `INT_MAX`.
const MAX_COUNT: u64 = i32::MAX as u64;

/// One change to a child's environment: a value, or `None` to unset it.
type EnvEdit = (String, Option<OsString>);

/// Drop every identity the environment would hand the git `cmd` runs.
///
/// # Errors
///
/// `GIT_CONFIG_PARAMETERS` or `GIT_CONFIG_COUNT` is in a form git would
/// refuse. The line names the variable, and `cmd` is not to be run.
pub(crate) fn strip_git_identity(cmd: &mut Command) -> std::io::Result<()> {
    let edits = identity_edits(|key| child_env(cmd, key))
        .map_err(|why| std::io::Error::new(std::io::ErrorKind::InvalidInput, why))?;
    for (key, value) in edits {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
    Ok(())
}

/// The value `cmd` will give `key`, including an explicit removal.
fn child_env(cmd: &Command, key: &str) -> Option<OsString> {
    match cmd.get_envs().find(|(name, _)| *name == OsStr::new(key)) {
        Some((_, value)) => value.map(OsStr::to_os_string),
        None => std::env::var_os(key),
    }
}

/// The edits that leave a child with no identity from `env`.
///
/// # Errors
///
/// A config variable git would refuse, as one line.
fn identity_edits(env: impl Fn(&str) -> Option<OsString>) -> Result<Vec<EnvEdit>, String> {
    let mut edits: Vec<EnvEdit> = IDENTITY_ENV
        .iter()
        .map(|key| ((*key).to_string(), None))
        .collect();
    if let Some(raw) = env(PARAMETERS) {
        if let Some(kept) = kept_parameters(raw.as_bytes()).ok_or_else(|| {
            format!(
                "the tracker git was not started: {PARAMETERS} is not in the quoted form git reads"
            )
        })? {
            edits.push((
                PARAMETERS.to_string(),
                (!kept.is_empty()).then(|| OsString::from_vec(kept)),
            ));
        }
    }
    if let Some(raw) = env(COUNT) {
        edits.extend(counted_edits(&raw, &env)?);
    }
    Ok(edits)
}

/// Whether a config key can name a commit identity: a `user`, `author` or
/// `committer` name or email, or an include, which can carry any of them.
fn identity_key(key: &[u8]) -> bool {
    let key = key.trim_ascii().to_ascii_lowercase();
    let section = key.split(|&b| b == b'.').next().unwrap_or_default();
    let variable = key.rsplit(|&b| b == b'.').next().unwrap_or_default();
    matches!(section, b"include" | b"includeif")
        || (matches!(section, b"user" | b"author" | b"committer")
            && matches!(variable, b"name" | b"email"))
}

/// `GIT_CONFIG_PARAMETERS` without its identity entries. `Some(None)` when
/// it has none to drop, and `None` when git would refuse it.
fn kept_parameters(raw: &[u8]) -> Option<Option<Vec<u8>>> {
    let entries = parameter_entries(raw)?;
    if !entries.iter().any(|(key, _)| identity_key(key)) {
        return Some(None);
    }
    let kept: Vec<&[u8]> = entries
        .iter()
        .filter(|(key, _)| !identity_key(key))
        .map(|(_, span)| *span)
        .collect();
    Some(Some(kept.join(&b' ')))
}

/// git's whitespace: space, tab, newline and carriage return.
fn git_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

fn skip_space(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|&&b| git_space(b)).count();
    &s[n..]
}

/// Each `GIT_CONFIG_PARAMETERS` entry as git parses it: its key, and the
/// bytes it spans, which are kept as written. Both `'key=value'` and
/// `'key'='value'` are read. `None` when git would call it bogus.
fn parameter_entries(raw: &[u8]) -> Option<Vec<(Vec<u8>, &[u8])>> {
    let mut entries = Vec::new();
    let mut rest = skip_space(raw);
    while !rest.is_empty() {
        let start = rest;
        let (word, after) = sq_dequote_step(rest)?;
        let (key, after) = match after.first() {
            None => (old_style_key(&word)?, after),
            Some(&b) if git_space(b) => (old_style_key(&word)?, after),
            Some(b'=') => {
                let value = &after[1..];
                let after = match value.first() {
                    Some(b'\'') => {
                        let (_, after) = sq_dequote_step(value)?;
                        if after.first().is_some_and(|&b| !git_space(b)) {
                            return None;
                        }
                        after
                    }
                    Some(&b) if !git_space(b) => return None,
                    _ => value,
                };
                (word, after)
            }
            Some(_) => return None,
        };
        let span = &start[..start.len() - after.len()];
        entries.push((key, span));
        rest = skip_space(after);
    }
    Some(entries)
}

/// The key of an old-style `'key=value'` entry, trimmed as git trims it.
fn old_style_key(word: &[u8]) -> Option<Vec<u8>> {
    let key = word.split(|&b| b == b'=').next().unwrap_or_default();
    let key = key.trim_ascii();
    (!key.is_empty()).then(|| key.to_vec())
}

/// One single-quoted word as git's `sq_dequote_step` reads it: `'\''` and
/// `'\!'` resume the quote. The word and what follows it, or `None` when
/// the quote never closes or does not open.
fn sq_dequote_step(s: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    if s.first() != Some(&b'\'') {
        return None;
    }
    let mut word = Vec::new();
    let mut i = 1;
    loop {
        let &c = s.get(i)?;
        i += 1;
        if c != b'\'' {
            word.push(c);
            continue;
        }
        match (s.get(i), s.get(i + 1), s.get(i + 2)) {
            (Some(b'\\'), Some(&q @ (b'\'' | b'!')), Some(b'\'')) => {
                word.push(q);
                i += 3;
            }
            _ => return Some((word, &s[i..])),
        }
    }
}

/// Edits that renumber `GIT_CONFIG_COUNT` without its identity entries.
/// The entries are read one at a time and stop at the count, so a count
/// larger than the environment holds is refused, not allocated.
fn counted_edits(
    raw: &OsStr,
    env: &impl Fn(&str) -> Option<OsString>,
) -> Result<Vec<EnvEdit>, String> {
    let shown = raw.to_string_lossy();
    let refused = |why: String| format!("the tracker git was not started: {why}");
    let count = config_count(raw.as_bytes())
        .ok_or_else(|| refused(format!("{COUNT}={shown} is not a count git reads")))?;
    if count > MAX_COUNT {
        return Err(refused(format!(
            "{COUNT}={shown} is more entries than git reads"
        )));
    }
    let mut entries = Vec::new();
    for index in 0..count {
        let key_name = format!("GIT_CONFIG_KEY_{index}");
        let value_name = format!("GIT_CONFIG_VALUE_{index}");
        let key = env(&key_name)
            .ok_or_else(|| refused(format!("{COUNT}={shown} but {key_name} is not set")))?;
        let value = env(&value_name)
            .ok_or_else(|| refused(format!("{COUNT}={shown} but {value_name} is not set")))?;
        entries.push((key, value));
    }
    if !entries.iter().any(|(key, _)| identity_key(key.as_bytes())) {
        return Ok(Vec::new());
    }
    let mut edits: Vec<EnvEdit> = Vec::new();
    for index in 0..entries.len() {
        edits.push((format!("GIT_CONFIG_KEY_{index}"), None));
        edits.push((format!("GIT_CONFIG_VALUE_{index}"), None));
    }
    let kept: Vec<(OsString, OsString)> = entries
        .into_iter()
        .filter(|(key, _)| !identity_key(key.as_bytes()))
        .collect();
    edits.push((COUNT.to_string(), Some(kept.len().to_string().into())));
    for (index, (key, value)) in kept.into_iter().enumerate() {
        edits.push((format!("GIT_CONFIG_KEY_{index}"), Some(key)));
        edits.push((format!("GIT_CONFIG_VALUE_{index}"), Some(value)));
    }
    Ok(edits)
}

/// `GIT_CONFIG_COUNT` as git's `strtoul` reads it: leading whitespace and
/// a `+`, then digits to the end. Empty is zero.
fn config_count(raw: &[u8]) -> Option<u64> {
    if raw.is_empty() {
        return Some(0);
    }
    let digits = skip_space(raw);
    let digits = digits.strip_prefix(b"+").unwrap_or(digits);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits)
        .ok()?
        .parse::<u64>()
        .ok()
        .or(Some(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn edits(env: &[(&str, &[u8])]) -> Result<HashMap<String, Option<Vec<u8>>>, String> {
        let env: HashMap<String, OsString> = env
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from_vec(v.to_vec())))
            .collect();
        identity_edits(|key| env.get(key).cloned()).map(|edits| {
            edits
                .into_iter()
                .map(|(k, v)| (k, v.map(OsString::into_vec)))
                .collect()
        })
    }

    fn after(env: &[(&str, &[u8])], key: &str) -> Option<Vec<u8>> {
        let edits = edits(env).unwrap();
        match edits.get(key) {
            Some(value) => value.clone(),
            None => env.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_vec()),
        }
    }

    #[test]
    fn the_identity_variables_and_email_are_unset() {
        let edits = edits(&[("GIT_AUTHOR_NAME", b"a"), ("EMAIL", b"e@box")]).unwrap();
        for key in IDENTITY_ENV {
            assert_eq!(edits.get(*key), Some(&None), "{key}");
        }
        assert!(!edits.contains_key("GIT_AUTHOR_DATE"));
        assert!(!edits.contains_key("GIT_COMMITTER_DATE"));
    }

    #[test]
    fn identity_keys_are_user_author_committer_and_includes() {
        for key in [
            "user.name",
            "User.Email",
            "author.name",
            "AUTHOR.EMAIL",
            "committer.name",
            "committer.email",
            " user.name ",
            "include.path",
            "includeIf.gitdir:/.path",
            "INCLUDEIF.onbranch:main.path",
        ] {
            assert!(identity_key(key.as_bytes()), "{key}");
        }
        for key in [
            "core.abbrev",
            "user.signingkey",
            "user.useConfigOnly",
            "gc.auto",
            "username.name",
        ] {
            assert!(!identity_key(key.as_bytes()), "{key}");
        }
    }

    #[test]
    fn parameters_keep_their_other_entries_as_written() {
        let raw: &[u8] = b"'core.abbrev=9' 'user.name'='bad name' 'author.email=a@box' \
'committer.name'='c' 'x.y'='it'\\''s' 'core.bare' 'include.path=/x' 'k.v'=";
        assert_eq!(
            after(&[(PARAMETERS, raw)], PARAMETERS).unwrap(),
            b"'core.abbrev=9' 'x.y'='it'\\''s' 'core.bare' 'k.v'=".to_vec()
        );
    }

    #[test]
    fn parameters_with_only_identity_are_unset() {
        let raw: &[u8] = b"'user.name=bad' 'user.email'='bad@box'";
        assert_eq!(edits(&[(PARAMETERS, raw)]).unwrap()[PARAMETERS], None);
    }

    #[test]
    fn parameters_with_no_identity_are_left_alone() {
        let raw: &[u8] = b"'core.abbrev=9'";
        assert!(!edits(&[(PARAMETERS, raw)])
            .unwrap()
            .contains_key(PARAMETERS));
    }

    #[test]
    fn parameters_that_are_not_utf8_are_read_as_bytes() {
        let raw: &[u8] = b"'ljos.probe=\xff' 'user.name=bytes' 'user.email'='b\xfe@box'";
        assert_eq!(
            after(&[(PARAMETERS, raw)], PARAMETERS).unwrap(),
            b"'ljos.probe=\xff'".to_vec()
        );
    }

    #[test]
    fn parameters_git_would_refuse_are_refused() {
        for raw in [
            &b"user.name=bad"[..],
            b"'user.name=bad",
            b"'user.name'x",
            b"'user.name'='bad'x",
            b"'user.name'=bad",
        ] {
            let why = edits(&[(PARAMETERS, raw)]).unwrap_err();
            assert!(why.contains(PARAMETERS), "{why}");
        }
    }

    #[test]
    fn counted_identity_entries_are_dropped_and_the_rest_renumbered() {
        let env: &[(&str, &[u8])] = &[
            (COUNT, b"4"),
            ("GIT_CONFIG_KEY_0", b"author.name"),
            ("GIT_CONFIG_VALUE_0", b"a"),
            ("GIT_CONFIG_KEY_1", b"core.abbrev"),
            ("GIT_CONFIG_VALUE_1", b"9"),
            ("GIT_CONFIG_KEY_2", b"includeIf.gitdir:/.path"),
            ("GIT_CONFIG_VALUE_2", b"/x"),
            ("GIT_CONFIG_KEY_3", b"gc.auto"),
            ("GIT_CONFIG_VALUE_3", b"\xff"),
        ];
        assert_eq!(after(env, COUNT).unwrap(), b"2");
        assert_eq!(after(env, "GIT_CONFIG_KEY_0").unwrap(), b"core.abbrev");
        assert_eq!(after(env, "GIT_CONFIG_VALUE_0").unwrap(), b"9");
        assert_eq!(after(env, "GIT_CONFIG_KEY_1").unwrap(), b"gc.auto");
        assert_eq!(after(env, "GIT_CONFIG_VALUE_1").unwrap(), b"\xff");
        assert_eq!(after(env, "GIT_CONFIG_KEY_2"), None);
        assert_eq!(after(env, "GIT_CONFIG_VALUE_3"), None);
    }

    #[test]
    fn a_count_with_no_identity_is_left_alone() {
        let edits = edits(&[
            (COUNT, b"1"),
            ("GIT_CONFIG_KEY_0", b"user.useConfigOnly"),
            ("GIT_CONFIG_VALUE_0", b"true"),
        ])
        .unwrap();
        assert!(!edits.contains_key(COUNT));
        assert!(!edits.contains_key("GIT_CONFIG_KEY_0"));
    }

    /// `GIT_CONFIG_COUNT=99999999999999` used to size a vector up front,
    /// and the allocation failure aborted ljos.
    #[test]
    fn a_huge_count_is_a_plain_refusal() {
        for count in [
            &b"99999999999999"[..],
            b"2147483648",
            b"99999999999999999999999",
        ] {
            let why = edits(&[(COUNT, count)]).unwrap_err();
            assert!(why.contains("more entries than git reads"), "{why}");
        }
        let why = edits(&[(COUNT, b"2147483647")]).unwrap_err();
        assert!(why.contains("GIT_CONFIG_KEY_0 is not set"), "{why}");
    }

    #[test]
    fn a_count_git_would_refuse_is_a_plain_refusal() {
        for (env, said) in [
            (&[(COUNT, &b"x"[..])][..], "is not a count git reads"),
            (&[(COUNT, b"-1")], "is not a count git reads"),
            (&[(COUNT, b"\xff")], "is not a count git reads"),
            (
                &[
                    (COUNT, b"2"),
                    ("GIT_CONFIG_KEY_0", b"a.b"),
                    ("GIT_CONFIG_VALUE_0", b"c"),
                ],
                "GIT_CONFIG_KEY_1 is not set",
            ),
            (
                &[(COUNT, b"1"), ("GIT_CONFIG_KEY_0", b"user.name")],
                "GIT_CONFIG_VALUE_0 is not set",
            ),
        ] {
            let why = edits(env).unwrap_err();
            assert!(
                why.starts_with("the tracker git was not started: "),
                "{why}"
            );
            assert!(why.contains(said), "{why}");
        }
        assert!(edits(&[(COUNT, b"")]).is_ok());
        assert!(edits(&[(COUNT, b" +0")]).is_ok());
    }
}
