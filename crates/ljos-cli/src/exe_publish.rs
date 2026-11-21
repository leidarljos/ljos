//! Publish an executable without leaving a write descriptor in this process.
//!
//! A descriptor opened here is copied into a child at fork and stays open
//! until that child reaches exec. Starting the new file in that window fails
//! with ETXTBSY ("Text file busy"). The bytes are written by `/bin/sh`, which
//! exits before this process renames the temporary name into place, so a fork
//! here never sees that descriptor.

use std::io::{Error, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// Write `body` to `path` and mark it executable (`0o755`).
///
/// # Errors
///
/// The directory cannot be written, or the short-lived writer fails.
pub fn install_executable(path: &Path, body: impl AsRef<[u8]>) -> Result<(), Error> {
    install_executable_mode(path, body, 0o755)
}

/// Write `body` to `path` and mark it with `mode`.
///
/// `mode` is the permission bits passed to `chmod`, masked to `0o7777`.
///
/// # Errors
///
/// The directory cannot be written, or the short-lived writer fails.
pub fn install_executable_mode(
    path: &Path,
    body: impl AsRef<[u8]>,
    mode: u32,
) -> Result<(), Error> {
    if let Some(parent) = dir_of(path) {
        std::fs::create_dir_all(parent)?;
    }
    let mode = mode & 0o7777;
    #[cfg(unix)]
    {
        let tmp = temp_name(path)?;
        let _ = std::fs::remove_file(&tmp);
        let script = format!("/bin/cat > \"$1\" && /bin/chmod {mode:o} \"$1\"");
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(&script)
            .arg("install-executable")
            .arg(&tmp)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let write_in = (|| {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| Error::other("install executable: writer has no stdin"))?;
            stdin.write_all(body.as_ref())?;
            stdin.flush()?;
            Ok(())
        })();
        if let Err(err) = write_in {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&tmp);
            return Err(err);
        }
        let out = child.wait_with_output()?;
        if !out.status.success() {
            let _ = std::fs::remove_file(&tmp);
            return Err(writer_failed(&out.status, &out.stderr));
        }
        settle(&tmp, path)
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        std::fs::write(path, body.as_ref())
    }
}

/// Copy `src` onto `dest` as an executable.
///
/// `cp` runs in another process and exits before the rename, so this process
/// never has `dest` open for writing.
///
/// # Errors
///
/// The source cannot be read, or the destination directory cannot be written.
pub fn copy_executable(src: &Path, dest: &Path) -> Result<(), Error> {
    if let Some(parent) = dir_of(dest) {
        std::fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    {
        let tmp = temp_name(dest)?;
        let _ = std::fs::remove_file(&tmp);
        let out = Command::new("/bin/cp")
            .arg(src)
            .arg(&tmp)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        if !out.status.success() {
            let _ = std::fs::remove_file(&tmp);
            return Err(writer_failed(&out.status, &out.stderr));
        }
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(err) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            {
                let _ = std::fs::remove_file(&tmp);
                return Err(err);
            }
        }
        settle(&tmp, dest)
    }
    #[cfg(not(unix))]
    {
        std::fs::copy(src, dest)?;
        Ok(())
    }
}

/// Flush the closed temporary file, rename it onto `path`, then flush the
/// directory so the new name survives a crash.
#[cfg(unix)]
fn settle(tmp: &Path, path: &Path) -> Result<(), Error> {
    let _ = flush(tmp);
    if let Err(err) = std::fs::rename(tmp, path) {
        let _ = std::fs::remove_file(tmp);
        return Err(err);
    }
    if let Some(parent) = dir_of(path) {
        let _ = flush(parent);
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    static FLUSHED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `sync_all` on `path`, a file or a directory. `settle` drops the error:
/// the rename has already happened or is about to, and a flush cannot
/// undo it.
#[cfg(unix)]
fn flush(path: &Path) -> Result<(), Error> {
    std::fs::File::open(path)?.sync_all()?;
    #[cfg(test)]
    FLUSHED.with(|done| done.borrow_mut().push(path.to_path_buf()));
    Ok(())
}

fn dir_of(path: &Path) -> Option<&Path> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => Some(dir),
        _ => None,
    }
}

fn temp_name(path: &Path) -> Result<PathBuf, Error> {
    let parent = dir_of(path).unwrap_or_else(|| Path::new("."));
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(".ljos-exe.{}.{n}", std::process::id())))
}

fn writer_failed(status: &std::process::ExitStatus, stderr: &[u8]) -> Error {
    let detail = String::from_utf8_lossy(stderr).trim().to_string();
    let msg = if detail.is_empty() {
        format!("install executable: writer exited {status}")
    } else {
        format!("install executable: {detail}")
    };
    Error::other(msg)
}

/// Linux and macOS both use 26 for ETXTBSY.
#[cfg(test)]
fn text_file_busy(err: &Error) -> bool {
    err.kind() == std::io::ErrorKind::ExecutableFileBusy
        || err.raw_os_error() == Some(26)
        || err
            .to_string()
            .to_ascii_lowercase()
            .contains("text file busy")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn a_published_stub_has_its_bytes_and_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stub");
        install_executable(&path, b"#!/bin/sh\necho hi\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"#!/bin/sh\necho hi\n");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
        let out = Command::new(&path).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, b"hi\n");
    }

    #[test]
    fn a_copied_executable_runs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        install_executable(&src, b"#!/bin/sh\necho copied\n").unwrap();
        let dest = dir.path().join("dest");
        copy_executable(&src, &dest).unwrap();
        let out = Command::new(&dest).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, b"copied\n");
    }

    /// The temporary file is flushed before the rename and the directory
    /// after it, for both a written and a copied executable.
    #[test]
    fn a_publish_flushes_the_file_then_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dest = dir.path().join("dest");
        for step in ["install", "copy"] {
            FLUSHED.with(|done| done.borrow_mut().clear());
            if step == "install" {
                install_executable(&src, b"#!/bin/sh\necho copied\n").unwrap();
            } else {
                copy_executable(&src, &dest).unwrap();
            }
            let done = FLUSHED.with(|done| done.borrow().clone());
            assert_eq!(done.len(), 2, "{step} flushed {done:?}");
            let tmp = done[0].file_name().unwrap().to_string_lossy();
            assert!(tmp.starts_with(".ljos-exe."), "{step} flushed {done:?}");
            assert_eq!(done[0].parent(), Some(dir.path()), "{step}");
            assert_eq!(done[1], dir.path(), "{step} flushed {done:?}");
        }
    }

    /// `/dev/null` opens but cannot be synced, so a flush that skips the
    /// sync returns Ok here.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_flush_reaches_the_sync() {
        let err = flush(Path::new("/dev/null")).unwrap_err();
        assert_eq!(err.raw_os_error(), Some(22), "{err}");
        assert!(flush(Path::new("/")).is_ok());
    }

    /// Many threads publish one stub and start it. A writer that leaves a
    /// descriptor in this process makes `exec` and the next write fail with
    /// ETXTBSY. Both counts are zero when the publisher is the short-lived
    /// process above.
    #[test]
    fn concurrent_publish_then_start_is_not_text_busy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sibling-stub");
        install_executable(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        let exec_busy = Arc::new(AtomicUsize::new(0));
        let write_busy = Arc::new(AtomicUsize::new(0));
        let spawns = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();
        for _ in 0..8 {
            let path = path.clone();
            let exec_busy = Arc::clone(&exec_busy);
            let write_busy = Arc::clone(&write_busy);
            let spawns = Arc::clone(&spawns);
            threads.push(thread::spawn(move || {
                for _ in 0..40 {
                    match install_executable(&path, b"#!/bin/sh\nexit 0\n") {
                        Ok(()) => {}
                        Err(err) if text_file_busy(&err) => {
                            write_busy.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        Err(err) => panic!("{err}"),
                    }
                    spawns.fetch_add(1, Ordering::Relaxed);
                    match Command::new(&path).output() {
                        Ok(out) if out.status.success() => {}
                        Err(err) if text_file_busy(&err) => {
                            exec_busy.fetch_add(1, Ordering::Relaxed);
                        }
                        other => panic!("{other:?}"),
                    }
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let exec_busy = exec_busy.load(Ordering::Relaxed);
        let write_busy = write_busy.load(Ordering::Relaxed);
        let spawns = spawns.load(Ordering::Relaxed);
        eprintln!(
            "concurrent publish: spawns={spawns} exec_etxtbsy={exec_busy} write_etxtbsy={write_busy}"
        );
        assert_eq!(
            exec_busy + write_busy,
            0,
            "exec_etxtbsy={exec_busy} write_etxtbsy={write_busy} spawns={spawns}"
        );
    }

    /// Files under `crates/` that may write an executable in this process,
    /// each with the reason it has to.
    const IN_PROCESS_EXECUTABLES: &[(&str, &str)] = &[];

    /// A file written in this process and then given an exec bit, or a git
    /// hook rewritten where it lies, can be started while a forked child
    /// still has it open. Each of those goes through this module instead.
    #[test]
    fn executables_are_published_through_this_module() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut files = Vec::new();
        rust_sources(crates, &mut files);
        assert!(files.len() > 100, "found {} sources", files.len());
        let mut hits = Vec::new();
        for file in &files {
            let rel = file.strip_prefix(crates).unwrap().to_string_lossy();
            if rel.ends_with("exe_publish.rs")
                || IN_PROCESS_EXECUTABLES.iter().any(|(path, _)| rel == *path)
            {
                continue;
            }
            let text = std::fs::read_to_string(file).unwrap();
            for line in write_then_exec(&text) {
                hits.push(format!("crates/{rel}:{line}"));
            }
        }
        assert!(
            hits.is_empty(),
            "publish these with install_executable, install_executable_mode or copy_executable:\n{}",
            hits.join("\n")
        );
    }

    #[test]
    fn the_site_scan_names_a_write_then_chmod_and_a_hook_rewrite() {
        let chmod = r##"
fn stub(bin: &Path) {
    fs::write(bin.join("a"), "#!/bin/sh\n").unwrap();
    fs::write(bin.join("conf"), "x").unwrap();
    for name in ["a"] {
        fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
}
fn other(dest: &Path) {
    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755)).unwrap();
}
"##;
        assert_eq!(write_then_exec(chmod), vec![3]);
        let open = r#"
fn server(path: &Path) {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(&path, perms).unwrap();
}
"#;
        assert_eq!(write_then_exec(open), vec![3]);
        let hook = r#"
fn strip(dir: &Path) {
    for name in HOOK_NAMES {
        let path = dir.join(name);
        fs::write(&path, "x")?;
    }
}
"#;
        assert_eq!(write_then_exec(hook), vec![5]);
        let named = r#"
fn rewrite(ours: &Path, share: &Path) {
    std::fs::write(ours.join("pre-push"), &stale).unwrap();
    let path = share.join("ljos/git-hooks/pre-push");
    let stub = fs::read_to_string(&path).unwrap();
    fs::write(&path, stub).unwrap();
    let path = share.join("notes.txt");
    fs::write(&path, "x").unwrap();
}
fn later(path: &Path) {
    fs::write(&path, "x").unwrap();
}
"#;
        assert_eq!(write_then_exec(named), vec![3, 6]);
        let fine = r#"
fn dir(path: &Path) {
    std::fs::write(path.join("f"), "x").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(path.join("f"), std::fs::Permissions::from_mode(0o644)).unwrap();
}
"#;
        assert!(write_then_exec(fine).is_empty());
    }

    #[test]
    fn the_site_scan_follows_a_hook_name_through_lets() {
        let chain = r#"
fn chain(dir: &Path) {
    let name = "pre-push";
    let path = dir.join(name);
    fs::write(&path, "x").unwrap();
    let name = "notes.txt";
    let path = dir.join(name);
    fs::write(&path, "x").unwrap();
}
"#;
        assert_eq!(write_then_exec(chain), vec![5]);
    }

    #[test]
    fn the_site_scan_names_a_helper_given_a_hook_path() {
        let helper = r##"
fn helper_write(path: &Path) {
    fs::write(path, "#!/bin/sh\n").unwrap();
}
fn install(dir: &Path) {
    helper_write(&dir.join("pre-push"));
}
fn note(path: &Path) {
    fs::write(path, "x").unwrap();
}
fn notes(dir: &Path) {
    note(&dir.join("notes.txt"));
}
"##;
        assert_eq!(write_then_exec(helper), vec![3, 6]);
    }

    fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "target") {
                    rust_sources(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && path
                    .components()
                    .any(|part| part.as_os_str() == "src" || part.as_os_str() == "tests")
            {
                out.push(path);
            }
        }
    }

    /// One-based lines of in-process writes that `text` later marks
    /// executable in the same function, rewrites in a `HOOK_NAMES` loop,
    /// or aims at a git hook by name or through a `let` bound to one.
    fn write_then_exec(text: &str) -> Vec<usize> {
        let code: Vec<&str> = text
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("//") {
                    ""
                } else {
                    line
                }
            })
            .collect();
        let joined = |from: usize, len: usize| code[from..(from + len).min(code.len())].join(" ");
        let mut hits = Vec::new();
        let mut writes: Vec<(usize, String)> = Vec::new();
        let mut lists: Vec<(String, Vec<String>)> = Vec::new();
        let mut hook_loop = None;
        let mut hook_vars: Vec<String> = Vec::new();
        let mut fn_name = String::new();
        let mut param_vars: Vec<(String, usize)> = Vec::new();
        let mut param_writes: Vec<(String, usize, usize)> = Vec::new();
        let mut hook_calls: Vec<(usize, String, usize)> = Vec::new();
        for (i, line) in code.iter().enumerate() {
            let head = line.trim_start();
            let item = ["pub(crate) ", "pub ", "async ", "const ", "unsafe "]
                .iter()
                .fold(head, |rest, pre| rest.strip_prefix(pre).unwrap_or(rest));
            if let Some(sig) = item.strip_prefix("fn ") {
                writes.clear();
                lists.clear();
                hook_loop = None;
                hook_vars.clear();
                fn_name = idents(sig).next().unwrap_or("").to_string();
                param_vars = args_after(&joined(i, 6), "(")
                    .iter()
                    .enumerate()
                    .filter_map(|(n, arg)| {
                        let name = arg.split(':').next()?.split_whitespace().last()?;
                        Some((name.to_string(), n))
                    })
                    .collect();
            }
            if let Some((name, value)) = let_binding(&joined(i, 4)) {
                let hook = names_hook(&value) || mentions(&value, &hook_vars);
                let from = param_of(&value, &param_vars);
                hook_vars.retain(|bound| *bound != name);
                param_vars.retain(|(bound, _)| *bound != name);
                if hook {
                    hook_vars.push(name.clone());
                }
                if let Some(n) = from {
                    param_vars.push((name, n));
                }
            }
            let ahead = joined(i, 4);
            for (at, _) in line.match_indices('(') {
                let before = &line[..at];
                let callee = before.rsplit(|c: char| !is_ident(c)).next().unwrap_or("");
                let lead = before[..before.len() - callee.len()].chars().next_back();
                if callee.is_empty() || matches!(lead, Some('.' | ':')) {
                    continue;
                }
                for (n, arg) in args_after(&ahead[at..], "(").iter().enumerate() {
                    if names_hook(arg) || mentions(arg, &hook_vars) {
                        hook_calls.push((i, callee.to_string(), n));
                    }
                }
            }
            if let Some(rest) = head.strip_prefix("for ") {
                if rest.contains("HOOK_NAMES") {
                    hook_loop = Some(i);
                }
                if let Some((name, list)) = rest.split_once(" in [") {
                    let names = list.split('"').skip(1).step_by(2);
                    lists.push((
                        name.trim().to_string(),
                        names.map(|lit| format!("\"{lit}\"")).collect(),
                    ));
                }
            }
            let target = if line.contains("fs::write(") {
                arg_after(&ahead, "fs::write(", 0)
            } else if line.contains("File::create(") {
                arg_after(&ahead, "File::create(", 0)
            } else if line.contains("fs::copy(") {
                arg_after(&ahead, "fs::copy(", 1)
            } else if line.contains("OpenOptions::new()") {
                arg_after(&joined(i, 8), ".open(", 0)
            } else {
                bare_write(line).and_then(|_| arg_after(&ahead, "write(", 0))
            };
            if let Some(target) = target.filter(|t| !t.is_empty()) {
                if hook_loop.is_some_and(|at| i - at <= 20)
                    || names_hook(&target)
                    || mentions(&target, &hook_vars)
                {
                    hits.push(i + 1);
                }
                if let Some(n) = param_of(&target, &param_vars) {
                    param_writes.push((fn_name.clone(), n, i));
                }
                writes.push((i, target));
            }
            if line.contains("set_permissions(") {
                let (Some(target), Some(perm)) = (
                    arg_after(&ahead, "set_permissions(", 0),
                    arg_after(&ahead, "set_permissions(", 1),
                ) else {
                    continue;
                };
                let mode = mode_of(&perm).or_else(|| {
                    let set = format!("{perm}.set_mode(");
                    (i.saturating_sub(4)..i)
                        .rev()
                        .map(|at| code[at])
                        .find(|line| squeeze(line).contains(&set))
                        .and_then(mode_of)
                });
                if mode.is_some_and(|mode| mode & 0o111 != 0) {
                    for (at, written) in &writes {
                        if same_path(written, &target, &lists) {
                            hits.push(at + 1);
                        }
                    }
                }
            }
        }
        for (line, callee, n) in &hook_calls {
            for (name, param, at) in &param_writes {
                if name == callee && param == n {
                    hits.extend([line + 1, at + 1]);
                }
            }
        }
        hits.sort_unstable();
        hits.dedup();
        hits
    }

    fn is_ident(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    /// The identifiers in `text` outside string literals.
    fn idents(text: &str) -> impl Iterator<Item = &str> {
        text.split('"')
            .step_by(2)
            .flat_map(|code| code.split(|c: char| !is_ident(c)))
            .filter(|word| !word.is_empty())
    }

    /// Whether `text` uses one of `names` as a whole identifier.
    fn mentions(text: &str, names: &[String]) -> bool {
        idents(text).any(|word| names.iter().any(|name| name == word))
    }

    /// The parameter index behind the first name in `text` that is a
    /// parameter or a `let` bound from one.
    fn param_of(text: &str, vars: &[(String, usize)]) -> Option<usize> {
        idents(text).find_map(|word| {
            vars.iter()
                .rev()
                .find(|(name, _)| name == word)
                .map(|(_, n)| *n)
        })
    }

    /// A string literal in `text` whose last path part is a git hook name.
    fn names_hook(text: &str) -> bool {
        text.split('"').skip(1).step_by(2).any(|lit| {
            let leaf = lit.rsplit('/').next().unwrap_or(lit);
            crate::git_hooks::HOOK_NAMES.contains(&leaf)
        })
    }

    /// The name and value of a `let` that starts `text`, up to the `;`.
    fn let_binding(text: &str) -> Option<(String, String)> {
        let rest = text.trim_start().strip_prefix("let ")?;
        let rest = rest.strip_prefix("mut ").unwrap_or(rest);
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        let (name, after) = rest.split_at(end);
        let after = after.trim_start();
        if name.is_empty() || !(after.starts_with('=') || after.starts_with(':')) {
            return None;
        }
        let value = after.split_once('=')?.1;
        let value = value.split(';').next().unwrap_or(value);
        Some((name.to_string(), value.to_string()))
    }

    fn bare_write(line: &str) -> Option<usize> {
        let at = line.find("write(")?;
        let before = line[..at].chars().next_back();
        match before {
            Some(c) if c.is_alphanumeric() || matches!(c, '_' | '.' | ':') => None,
            _ => Some(at),
        }
    }

    fn squeeze(text: &str) -> String {
        text.split_whitespace().collect()
    }

    /// The `n`th top-level argument after `open`, without spaces or a
    /// leading `&`.
    fn arg_after(text: &str, open: &str, n: usize) -> Option<String> {
        args_after(text, open)
            .get(n)
            .map(|arg| squeeze(arg).trim_start_matches('&').to_string())
    }

    /// The top-level arguments after the first `open` in `text`.
    fn args_after(text: &str, open: &str) -> Vec<String> {
        let Some(at) = text.find(open) else {
            return Vec::new();
        };
        let rest = &text[at + open.len()..];
        let mut args = vec![String::new()];
        let mut depth = 0usize;
        let mut quoted = false;
        let mut escaped = false;
        for c in rest.chars() {
            if quoted {
                quoted = escaped || c != '"';
                escaped = !escaped && c == '\\';
            } else if c == '"' {
                quoted = true;
            } else if matches!(c, '(' | '[' | '{') {
                depth += 1;
            } else if matches!(c, ')' | ']' | '}') {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            } else if c == ',' && depth == 0 {
                args.push(String::new());
                continue;
            }
            args.last_mut().unwrap().push(c);
        }
        args
    }

    fn mode_of(text: &str) -> Option<u32> {
        ["from_mode(0o", "set_mode(0o"].iter().find_map(|open| {
            let rest = &text[text.find(open)? + open.len()..];
            let digits: String = rest.chars().take_while(|c| c.is_digit(8)).collect();
            u32::from_str_radix(&digits, 8).ok()
        })
    }

    /// `dir.join(name)` in a loop is the same path as `dir.join("a")` when
    /// `"a"` is one of the names the loop walks, or any name when the loop
    /// is not over a literal list.
    fn same_path(written: &str, target: &str, lists: &[(String, Vec<String>)]) -> bool {
        if written == target {
            return true;
        }
        let (Some((dir_a, leaf_a)), Some((dir_b, leaf_b))) =
            (written.split_once(".join("), target.split_once(".join("))
        else {
            return false;
        };
        if dir_a != dir_b {
            return false;
        }
        let leaf_a = leaf_a.trim_end_matches(')');
        let leaf_b = leaf_b.trim_end_matches(')');
        let (literal, name) = match (leaf_a.starts_with('"'), leaf_b.starts_with('"')) {
            (true, true) => return false,
            (true, false) => (leaf_a, leaf_b),
            (false, true) => (leaf_b, leaf_a),
            (false, false) => return leaf_a == leaf_b,
        };
        lists
            .iter()
            .rev()
            .find(|(bound, _)| bound == name)
            .is_none_or(|(_, names)| names.iter().any(|lit| lit == literal))
    }
}
