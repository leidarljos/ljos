//! Deletes inside the working tree that lose no work: a build output
//! directory, a scratch directory at the top of the tree, or a `find`
//! cleanup of files a tool writes again. The static guard allows these, as
//! `ljos-policyd` does, so a stock seat with no rules of its own does not
//! stop `rm -rf target/` or `find . -name '*.pyc' -delete`. The lists and
//! the reading are the ones in ljos-policyd's `src/lib.rs`; keep the two in
//! step.

/// Directories a build, a package manager or a test run writes and can
/// write again.
const BUILD_OUTPUT_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "_build",
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".nox",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".angular",
    ".gradle",
    "coverage",
    "htmlcov",
];

/// Scratch directories a project keeps at its top for throwaway output.
const SCRATCH_DIRS: &[&str] = &["tmp", ".tmp", "temp"];

/// Names a compiler, an interpreter, an editor or a desktop writes and
/// writes again.
const REGENERABLE_NAMES: &[&str] = &[
    "*.pyc",
    "*.pyo",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    "*.o",
    "*.obj",
    "*.class",
    "*.gcda",
    "*.gcno",
    "*.orig",
    "*.rej",
    "*.swp",
    "*.swo",
    "*~",
    ".DS_Store",
    "._*",
    "Thumbs.db",
    "desktop.ini",
];

/// A path written out in plain characters that stays below the working
/// directory: not absolute, no `~`, no `$`, no glob, no `..` and no `.`
/// part. A leading `./` and a trailing `/` are allowed.
fn plain_relative(p: &str) -> bool {
    let mut rest = p;
    while let Some(r) = rest.strip_prefix("./") {
        rest = r;
    }
    let rest = rest.trim_end_matches('/');
    !rest.is_empty()
        && !rest.starts_with('-')
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '@' | '/'))
        && rest
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// A plain relative path inside a build output directory, or the
/// directory itself: `target/`, `./build`, `web/node_modules`. A path
/// through `.git` is not one.
#[must_use]
pub fn build_output(p: &str) -> bool {
    plain_relative(p)
        && !p.split('/').any(|part| part == ".git")
        && p.split('/').any(|part| BUILD_OUTPUT_DIRS.contains(&part))
}

/// A plain relative path under a scratch directory at the top of the tree,
/// or the directory itself: `tmp/`, `./tmp/test-output`. A scratch name
/// deeper down (`src/tmp`) says nothing about what the path holds.
#[must_use]
pub fn scratch(p: &str) -> bool {
    let mut rest = p;
    while let Some(r) = rest.strip_prefix("./") {
        rest = r;
    }
    plain_relative(p)
        && rest
            .split('/')
            .next()
            .is_some_and(|first| SCRATCH_DIRS.contains(&first))
        && !rest.split('/').any(|part| part == ".git")
}

/// A `cd`, `pushd` or `popd` that can leave the working tree: an absolute
/// target, `~`, `-`, `..`, a variable, or none at all (home). `words` is
/// one command, wrappers already stripped.
#[must_use]
pub fn leaves_tree(words: &[String]) -> bool {
    let Some(first) = words.first() else {
        return false;
    };
    let name = first.rsplit('/').next().unwrap_or(first);
    if name == "popd" {
        return true;
    }
    if name != "cd" && name != "pushd" {
        return false;
    }
    match words[1..].iter().find(|a| !a.starts_with('-') || *a == "-") {
        Some(t) => !plain_relative(t),
        None => true,
    }
}

/// A `find` cleanup inside the working tree: every starting point is `.`
/// or a plain relative path outside `.git`, and every name test names a
/// file in [`REGENERABLE_NAMES`], with at least one. An `-o`, a `!`, a
/// `-not` or a group can turn a name test around, so none passes.
/// `find . -delete` has no name test and `find . -name '*.rs' -delete`
/// names source, so both stay refused.
#[must_use]
pub fn regenerable_cleanup(starts: &[&str], expr: &[&str]) -> bool {
    let in_tree = |p: &&str| {
        let p = p.trim_end_matches('/');
        (p == "." || plain_relative(p)) && !p.split('/').any(|part| part == ".git")
    };
    if !starts.iter().all(in_tree) {
        return false;
    }
    if expr.iter().any(|a| {
        matches!(
            *a,
            "-o" | "-or" | "!" | "-not" | "," | "(" | ")" | "\\(" | "\\)"
        )
    }) {
        return false;
    }
    let mut named = 0;
    for w in expr.windows(2) {
        match w[0] {
            "-name" | "-iname" => {
                if !REGENERABLE_NAMES.contains(&w[1]) {
                    return false;
                }
                named += 1;
            }
            "-path" | "-ipath" | "-wholename" => {
                let parts: Vec<&str> = w[1].split('/').filter(|p| *p != "*" && *p != ".").collect();
                if parts.is_empty() || !parts.iter().all(|p| REGENERABLE_NAMES.contains(p)) {
                    return false;
                }
                named += 1;
            }
            "-regex" | "-iregex" | "-lname" | "-ilname" => return false,
            _ => {}
        }
    }
    named > 0
}
