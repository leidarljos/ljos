//! The vissue, claimdag and deedar commits in `.github/sibling-pin`.
//! A test that runs one of them asks [`require`]. A missing binary, or one
//! whose `--version` line is not the pin, fails the test with one line.
//! Nothing is skipped.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// One sibling tool: where it comes from, the commit CI builds, and the
/// version its `--version` line carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub name: String,
    pub repo: String,
    pub commit: String,
    pub version: String,
}

/// The pin file, next to the workflow that builds it.
pub fn pin_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.github/sibling-pin")
}

/// Every pin in `text`, in file order.
///
/// # Errors
///
/// A line is not four fields, a commit is not a full SHA-1, a repository
/// is not a leidarljos git url, or a name is listed twice. The error is
/// one line.
pub fn parse(text: &str) -> Result<Vec<Pin>, String> {
    let mut pins: Vec<Pin> = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = idx + 1;
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 4 {
            return Err(format!(
                ".github/sibling-pin:{at}: want name repository commit version"
            ));
        }
        let name = fields[0];
        let repo = fields[1];
        let commit = fields[2];
        let version = fields[3];
        if !is_sha1(commit) {
            return Err(format!(
                ".github/sibling-pin:{at}: {name} commit {commit} is not a full SHA-1"
            ));
        }
        if !repo.starts_with("https://github.com/leidarljos/") || !repo.ends_with(".git") {
            return Err(format!(
                ".github/sibling-pin:{at}: {name} repository is not a leidarljos git url"
            ));
        }
        if pins.iter().any(|have| have.name == name) {
            return Err(format!(".github/sibling-pin:{at}: {name} is listed twice"));
        }
        pins.push(Pin {
            name: name.to_string(),
            repo: repo.to_string(),
            commit: commit.to_string(),
            version: version.to_string(),
        });
    }
    Ok(pins)
}

/// The pins CI builds.
///
/// # Errors
///
/// The file cannot be read, or [`parse`] rejects it.
pub fn pins() -> Result<Vec<Pin>, String> {
    let path = pin_file();
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    parse(&text)
}

/// The pin named `name`.
///
/// # Errors
///
/// [`pins`] fails, or the file does not name `name`.
pub fn pin(name: &str) -> Result<Pin, String> {
    pins()?
        .into_iter()
        .find(|pin| pin.name == name)
        .ok_or_else(|| format!("no pin for {name} in .github/sibling-pin"))
}

/// Whether `output` is the version line the pin names.
///
/// clap and deedar print one line, `{name} {version}`. A substring such as
/// `0.20` also matches `0.20.1` and `0.200.0`, so the whole first line has
/// to be that text and nothing more.
#[must_use]
pub fn version_matches(output: &str, name: &str, version: &str) -> bool {
    let want = format!("{name} {version}");
    output
        .lines()
        .next()
        .is_some_and(|line| line.trim() == want)
}

/// `name` on `path`, at the pinned version.
///
/// `path` is a `PATH` value. The first file of that name is the one a shell
/// would run. The error is one line: `{name} {version} is not on PATH`,
/// and, when a binary did answer, ` (found …)` on that same line.
///
/// # Errors
///
/// No pin, no file on `path`, `--version` did not succeed, or its first
/// line is not `{name} {version}`.
pub fn require_on(path: &OsStr, name: &str) -> Result<PathBuf, String> {
    let pin = pin(name)?;
    let found = std::env::split_paths(path).find_map(|dir| {
        let bin = dir.join(name);
        bin.is_file().then_some(bin)
    });
    let Some(bin) = found else {
        return Err(missing(&pin));
    };
    let output = Command::new(&bin)
        .arg("--version")
        .env("PATH", path)
        .stdin(Stdio::null())
        .output();
    let Ok(output) = output else {
        return Err(found_line(&pin, "no version"));
    };
    if !output.status.success() {
        return Err(found_line(&pin, "no version"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if version_matches(&text, &pin.name, &pin.version) {
        return Ok(bin);
    }
    let shown = text.lines().next().unwrap_or("").trim();
    if shown.is_empty() {
        Err(found_line(&pin, "no version"))
    } else {
        Err(found_line(&pin, shown))
    }
}

/// [`require_on`] for this process's `PATH`.
///
/// # Errors
///
/// See [`require_on`].
pub fn require(name: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    require_on(&path, name)
}

fn missing(pin: &Pin) -> String {
    format!("{} {} is not on PATH", pin.name, pin.version)
}

fn found_line(pin: &Pin, found: &str) -> String {
    format!("{} (found {found})", missing(pin))
}

/// git names these objects with SHA-1, which is 40 hexadecimal digits.
/// A tag, or a short hash, is not a commit pin.
fn is_sha1(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A stand-in binary. `755` is the mode a shell needs to exec a script
/// the owner wrote.
///
/// A write descriptor in this process would be inherited by a parallel
/// test's fork until that child execs, and exec of the stand-in answers
/// ETXTBSY meanwhile. A short-lived `cat` writes it under a temporary
/// name, and it is renamed into place after `cat` has exited.
#[cfg(test)]
fn stand_in(dir: &std::path::Path, name: &str, body: &str) {
    use std::io::Write;
    let path = dir.join(name);
    let tmp = dir.join(format!(".{name}.new"));
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg("/bin/cat > \"$1\" && /bin/chmod 755 \"$1\"")
        .arg("stand-in")
        .arg(&tmp)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(body.as_bytes())
        .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    drop(stdin);
    let out = child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    assert!(
        out.status.success(),
        "write {}: {}",
        path.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::rename(&tmp, &path).unwrap_or_else(|e| panic!("install {}: {e}", path.display()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_names_vissue_claimdag_and_deedar_by_commit() {
        let pins = pins().unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<&str> = pins.iter().map(|pin| pin.name.as_str()).collect();
        assert_eq!(names, ["vissue", "claimdag", "deedar"]);
        for pin in &pins {
            assert!(is_sha1(&pin.commit), "{}", pin.commit);
            assert_eq!(
                pin.repo,
                format!("https://github.com/leidarljos/{}.git", pin.name)
            );
            assert!(!pin.version.is_empty(), "{}", pin.name);
            assert!(
                version_matches(
                    &format!("{} {}\n", pin.name, pin.version),
                    &pin.name,
                    &pin.version
                ),
                "{}",
                pin.version
            );
        }
        let commits: Vec<&str> = pins.iter().map(|pin| pin.commit.as_str()).collect();
        assert_eq!(commits.len(), 3);
        assert_ne!(commits[0], commits[1]);
        assert_ne!(commits[1], commits[2]);
    }

    #[test]
    fn a_tag_is_not_a_commit_pin() {
        let err = parse("claimdag https://github.com/leidarljos/claimdag.git v0.8.0 0.8.0\n")
            .unwrap_err();
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.contains("not a full SHA-1"), "{err}");
        assert!(err.contains("v0.8.0"), "{err}");
    }

    #[test]
    fn a_version_line_is_the_whole_first_line() {
        assert!(version_matches("vissue 0.24.0\n", "vissue", "0.24.0"));
        assert!(version_matches("vissue 0.24.0", "vissue", "0.24.0"));
        // The old check was `contains("0.20")`, which accepts a different
        // release and a longer number that merely holds those digits.
        assert!(!version_matches("vissue 0.20.0\n", "vissue", "0.24.0"));
        assert!(!version_matches("vissue 0.24.10\n", "vissue", "0.24.0"));
        assert!(!version_matches(
            "vissue 0.24.0 extra\n",
            "vissue",
            "0.24.0"
        ));
        assert!(!version_matches("note vissue 0.24.0\n", "vissue", "0.24.0"));
        assert!(!version_matches("claimdag 0.8.0\n", "claimdag", "0.8"));
        assert!(!version_matches("claimdag 0.8\n", "claimdag", "0.8.0"));
        assert!(!version_matches("claimdag 10.8.0\n", "claimdag", "0.8.0"));
    }

    #[test]
    fn a_missing_sibling_fails_with_one_line() {
        let err = require_on(OsStr::new("/no/such/sibling-bin"), "vissue").unwrap_err();
        let want = pin("vissue").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            err,
            format!("{} {} is not on PATH", want.name, want.version)
        );
        assert_eq!(err.lines().count(), 1, "{err}");
    }

    #[test]
    fn a_different_version_fails_with_one_line() {
        let dir = tempfile::tempdir().unwrap();
        stand_in(dir.path(), "vissue", "#!/bin/sh\necho 'vissue 0.20.0'\n");
        let err = require_on(dir.path().as_os_str(), "vissue").unwrap_err();
        let want = pin("vissue").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            err,
            format!(
                "{} {} is not on PATH (found vissue 0.20.0)",
                want.name, want.version
            )
        );
        assert_eq!(err.lines().count(), 1, "{err}");
    }

    #[test]
    fn a_binary_that_prints_no_version_fails_with_one_line() {
        let dir = tempfile::tempdir().unwrap();
        stand_in(dir.path(), "claimdag", "#!/bin/sh\nexit 1\n");
        let err = require_on(dir.path().as_os_str(), "claimdag").unwrap_err();
        let want = pin("claimdag").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            err,
            format!(
                "{} {} is not on PATH (found no version)",
                want.name, want.version
            )
        );
        assert_eq!(err.lines().count(), 1, "{err}");
    }

    #[test]
    fn the_pinned_version_on_path_is_that_binary() {
        let want = pin("deedar").unwrap_or_else(|e| panic!("{e}"));
        let dir = tempfile::tempdir().unwrap();
        stand_in(
            dir.path(),
            "deedar",
            &format!("#!/bin/sh\necho '{} {}'\n", want.name, want.version),
        );
        let got = require_on(dir.path().as_os_str(), "deedar").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got, dir.path().join("deedar"));
    }
}
