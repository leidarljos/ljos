//! A test's git call cannot reach the checkout's `.git`.
//!
//! A scratch repository whose `.git` is missing or half made is not a
//! repository, so git walks up from it. From the checkout's `target` that
//! walk ends in the ljos checkout, and a `git config` there rewrites its
//! `.git/config`.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::Output;

#[path = "common/seat_root.rs"]
mod seat_root;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn local_config(checkout: &Path) -> Option<Vec<u8>> {
    std::fs::read(checkout.join(".git/config")).ok()
}

/// A directory with no `.git` inside the checkout's `crates/target` is
/// not a repository to a test's git.
#[test]
fn a_test_git_in_a_dir_without_git_does_not_reach_the_checkout() {
    let checkout = ljos_test_boot::checkout();
    let scratch = Scratch(ljos_test_boot::fresh_dir(
        &ljos_test_boot::checkout_seats(),
        "ljos-git-ceiling",
    ));
    let nested = scratch.0.join("repo/sub");
    std::fs::create_dir_all(&nested).unwrap();
    let before = local_config(&checkout);

    let open = ljos_test_boot::git()
        .args(["rev-parse", "--absolute-git-dir"])
        .current_dir(&nested)
        .env_remove(ljos_test_boot::GIT_CEILING)
        .output()
        .unwrap();
    let found = String::from_utf8_lossy(&open.stdout).trim().to_string();
    if !open.status.success() || !Path::new(&found).starts_with(&checkout) {
        eprintln!(
            "skip: {} is not inside a git checkout\n{}",
            nested.display(),
            said(&open)
        );
        return;
    }

    let fenced = ljos_test_boot::git()
        .args(["rev-parse", "--absolute-git-dir"])
        .current_dir(&nested)
        .output()
        .unwrap();
    assert!(
        !fenced.status.success(),
        "git from {} found {}\n{}",
        nested.display(),
        String::from_utf8_lossy(&fenced.stdout).trim(),
        said(&fenced)
    );
    assert!(
        String::from_utf8_lossy(&fenced.stderr).contains("not a git repository"),
        "{}",
        said(&fenced)
    );

    let cleared = ljos_test_boot::git_at("/usr/bin/git")
        .args(["rev-parse", "--absolute-git-dir"])
        .current_dir(&nested)
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(!cleared.status.success(), "{}", said(&cleared));
    assert_eq!(
        local_config(&checkout),
        before,
        "the checkout config changed"
    );
}

/// A `HOME` with no room for seat homes is refused. No seat goes under
/// the checkout's `crates/target` in its place.
#[test]
fn a_seat_root_never_falls_back_to_the_checkout() {
    let is_temp = ljos_cli::bookkeep::path_is_temp;
    let temp = std::env::temp_dir().join("ljos-test-seats");
    let long = PathBuf::from(format!("/{}/.cache/ljos-test-seats", "h".repeat(40)));
    for cache in [
        None,
        Some(temp),
        Some(long),
        Some(ljos_test_boot::checkout_seats()),
    ] {
        let picked = seat_root::pick_seat_root(cache.clone(), is_temp);
        assert!(picked.is_err(), "{cache:?} gave {picked:?}");
    }
    let fine = PathBuf::from("/home/u/.cache/ljos-test-seats");
    assert_eq!(
        seat_root::pick_seat_root(Some(fine.clone()), |_| false),
        Ok(fine)
    );
}

/// Seat homes sit outside the checkout.
#[test]
fn a_seat_root_sits_outside_the_checkout() {
    let is_temp = ljos_cli::bookkeep::path_is_temp;
    let checkout = ljos_test_boot::checkout();
    let root = seat_root::seat_root(is_temp);
    assert!(
        !root.starts_with(&checkout),
        "seat root {} is inside the checkout {}",
        root.display(),
        checkout.display()
    );
    let dir = Scratch(seat_root::seat_dir(is_temp, "ljos-git-ceiling"));
    assert!(!dir.0.starts_with(&checkout), "{}", dir.0.display());
}

/// Two tests asking for the same name at once get two directories, and
/// neither removes the other's.
#[test]
fn scratch_dirs_with_one_name_never_collide() {
    let root = Scratch(ljos_test_boot::fresh_dir(
        &std::env::temp_dir(),
        "ljos-git-ceiling",
    ));
    let made: Vec<PathBuf> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..16)
            .map(|_| s.spawn(|| ljos_test_boot::fresh_dir(&root.0, "same")))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut names = made.clone();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), made.len(), "{made:?}");
    assert!(made.iter().all(|dir| dir.is_dir()), "{made:?}");
}

/// No git runs without the ceiling. A `Command::new` whose program names
/// git (a literal such as `"git"` or `"/bin/git"`, or a variable or path
/// with a `git` word in it, or one bound to such a value) sets the
/// ceiling in that statement or the next, and so does every
/// `env_clear`. Test and library sources are both read.
#[test]
fn every_git_call_carries_the_ceiling() {
    let checkout = ljos_test_boot::checkout();
    let mut files = Vec::new();
    collect(&checkout.join("crates"), &mut files);
    let mut found = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let rel = file.strip_prefix(&checkout).unwrap_or(&file).display();
        for (line, what) in unfenced(&text) {
            found.push(format!("{rel}:{line}: {what}"));
        }
    }
    assert!(
        found.is_empty(),
        "build git with ljos_test_boot::git, git_at or git_cmd, and set \
         the ceiling again after env_clear:\n{}",
        found.join("\n")
    );
}

/// The scan sees the ways around a literal match.
#[test]
fn the_git_scan_sees_variables_paths_and_a_later_env_clear() {
    // `CMD` and `CLEAR` keep these samples from matching the scan of
    // this file.
    let caught = [
        r#"let bin = "git"; std::process::CMD::new(bin).output();"#,
        r#"let out = CMD::new(&git).output();"#,
        r#"let out = CMD::new(&self.real_git).output();"#,
        r#"let out = CMD::new("/bin/git").output();"#,
        r#"let out = CMD::new("/usr/local/bin/git").output();"#,
        r#"let out = CMD::new( "git" ).output();"#,
        r#"let out = CMD::new(which::which("git").unwrap()).output();"#,
        "let mut c = ljos_test_boot::git();\nc.CLEAR();\nlet out = c.output();",
        "let mut c = CMD::new(ljos);\nc.CLEAR();\nlet out = c.output();",
    ];
    let passed = [
        "let out = ljos_test_boot::git().CLEAR()\n.env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling()).output();",
        "let mut cmd = std::process::CMD::new(\"git\");\nfence_git(&mut cmd);",
        "let out = CMD::new(\"sh\").output();",
        "let gitleaks = 1; let out = CMD::new(&gitleaks).output();",
        "// CMD::new(\"git\") in a comment\nlet x = 1;",
    ];
    let real = |s: &str| s.replace("CMD", "Command").replace("CLEAR", "env_clear");
    for sample in caught {
        assert!(!unfenced(&real(sample)).is_empty(), "missed: {sample}");
    }
    for sample in passed {
        let hits = unfenced(&real(sample));
        assert!(hits.is_empty(), "{sample}: {hits:?}");
    }
}

const CEILING: [&str; 3] = ["GIT_CEILING", "git_ceiling", "fence_git"];

/// Line and reason for each git command, or environment clear, in `text`
/// that does not set the ceiling.
fn unfenced(text: &str) -> Vec<(usize, &'static str)> {
    let code = without_line_comments(text);
    let bound = git_names(&code);
    let mut found = Vec::new();
    for (at, arg) in calls(&code) {
        let names: Vec<String> = bound
            .iter()
            .filter(|(from, _)| in_scope(&code, *from, at))
            .map(|(_, name)| name.clone())
            .collect();
        if names_git(arg, &names) && !fenced(&code, at) {
            found.push((line_of(&code, at), "git command without the ceiling"));
        }
    }
    for at in clears(&code) {
        if !fenced(&code, at) {
            found.push((line_of(&code, at), "env_clear drops the ceiling"));
        }
    }
    found
}

/// `text` with each `//` comment line blanked, so lines keep their numbers.
fn without_line_comments(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            if line.trim_start().starts_with("//") {
                line.chars()
                    .map(|c| if c == '\n' { c } else { ' ' })
                    .collect()
            } else {
                line.to_string()
            }
        })
        .collect()
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn skip_ws(code: &str, at: usize) -> usize {
    at + code[at..].len() - code[at..].trim_start().len()
}

fn word_at(code: &str, at: usize, word: &str) -> bool {
    code[at..].starts_with(word)
        && !code[at + word.len()..].starts_with(is_ident)
        && !code[..at].ends_with(is_ident)
}

/// Whether `expr` names git: a `git` word, as in `"git"`, `/bin/git` or
/// `real_git`, or a name bound to such a value.
fn names_git(expr: &str, names: &[String]) -> bool {
    expr.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|piece| piece == "git")
        || expr
            .split(|c: char| !is_ident(c))
            .any(|ident| names.iter().any(|name| name == ident))
}

/// Whether a binding at `from` is still in scope at `at`: no block
/// opened before `at` closes the one `from` is in.
fn in_scope(code: &str, from: usize, at: usize) -> bool {
    if from >= at {
        return false;
    }
    let mut depth = 0i64;
    for c in code[from..at].chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return false;
        }
    }
    true
}

/// Each `let` that binds a name to a value that names git, with its
/// offset.
fn git_names(code: &str) -> Vec<(usize, String)> {
    let mut names = Vec::new();
    for (at, _) in code.match_indices("let") {
        if !word_at(code, at, "let") {
            continue;
        }
        let mut i = skip_ws(code, at + 3);
        if word_at(code, i, "mut") {
            i = skip_ws(code, i + 3);
        }
        let name: String = code[i..].chars().take_while(|c| is_ident(*c)).collect();
        let Some(eq) = code[i..].find('=').map(|eq| i + eq) else {
            continue;
        };
        let end = code[eq..].find(';').map_or(code.len(), |end| eq + end);
        if !name.is_empty() && names_git(&code[eq + 1..end], &[]) {
            names.push((at, name));
        }
    }
    names
}

/// Each `Command::new(ARG)`, with its offset and `ARG`.
fn calls(code: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    for (at, _) in code.match_indices("Command") {
        if !word_at(code, at, "Command") {
            continue;
        }
        let mut i = skip_ws(code, at + "Command".len());
        if !code[i..].starts_with("::") {
            continue;
        }
        i = skip_ws(code, i + 2);
        if !word_at(code, i, "new") {
            continue;
        }
        i = skip_ws(code, i + 3);
        if !code[i..].starts_with('(') {
            continue;
        }
        let open = i + 1;
        let mut depth = 1;
        let mut close = code.len();
        for (j, c) in code[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                close = open + j;
                break;
            }
        }
        out.push((at, &code[open..close]));
    }
    out
}

/// The offset of each `.env_clear()`.
fn clears(code: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (at, _) in code.match_indices("env_clear") {
        if !word_at(code, at, "env_clear") || !code[..at].trim_end().ends_with('.') {
            continue;
        }
        let i = skip_ws(code, at + "env_clear".len());
        if code[i..].starts_with('(') && code[skip_ws(code, i + 1)..].starts_with(')') {
            out.push(at);
        }
    }
    out
}

/// Whether the ceiling is set after `at`, in its statement or the next.
fn fenced(code: &str, at: usize) -> bool {
    let end = |from: usize| {
        code[from..]
            .find(';')
            .map_or(code.len(), |end| from + end + 1)
    };
    let reach = &code[at..end(end(at))];
    CEILING.iter().any(|token| reach.contains(token))
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// Rust sources under `crates`: library, binary and test trees.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name == "target" || name == "fixtures" {
                continue;
            }
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}
