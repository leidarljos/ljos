//! The person's passphrase. A shell can carry a runner's variable on a
//! host that exports it to everything it starts, and then the gates
//! cannot tell the person from a runner by the process alone. The
//! passphrase is what the person knows and a runner does not.
//!
//! Only an argon2id hash is kept, in PHC form with a random salt, in
//! `person-key` under the ljos config directory of the home the passwd
//! entry names. `HOME` and `XDG_CONFIG_HOME` do not move it: a runner
//! sets both for what it starts.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use argon2::password_hash::{PasswordHash, PasswordHasher as _, PasswordVerifier as _, SaltString};
use argon2::Argon2;

/// The shortest passphrase `ljos person-key set` takes, in bytes.
pub const MIN_LEN: usize = 12;

const KEY_NAME: &str = "person-key";

/// Wrong answers, one Unix time per line, beside the key.
const FAILS_NAME: &str = "person-key.failures";

/// Wrong answers within this many seconds count toward a lockout.
const WINDOW_SECS: u64 = 15 * 60;

/// This many wrong answers in the window and ljos stops asking.
const LOCK_AFTER: usize = 5;

/// The longest wait after a wrong answer, in seconds.
const MAX_PAUSE_SECS: u64 = 60;

#[cfg(any(test, feature = "test-hooks"))]
static DIR_FOR_PROCESS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Keep the key in `dir` for the rest of this process. Only a test build
/// has it: the `test-hooks` feature is on for this package's tests and
/// off in a release, so no variable, argument or file moves the key.
#[cfg(any(test, feature = "test-hooks"))]
#[doc(hidden)]
pub fn keep_in_for_this_process(dir: PathBuf) {
    let _ = DIR_FOR_PROCESS.set(dir);
}

/// The ljos config directory of the passwd entry's home.
#[must_use]
pub fn key_dir() -> Option<PathBuf> {
    #[cfg(any(test, feature = "test-hooks"))]
    if let Some(dir) = DIR_FOR_PROCESS.get() {
        return Some(dir.clone());
    }
    crate::proc_tree::own_home().map(|home| home.join(".config").join("ljos"))
}

/// Where the hash is kept.
#[must_use]
pub fn key_path() -> Option<PathBuf> {
    key_dir().map(|dir| dir.join(KEY_NAME))
}

/// Whether a person passphrase is kept.
#[must_use]
pub fn is_set() -> bool {
    key_path().is_some_and(|path| matches!(read(&path), Ok(Some(_))))
}

/// Where wrong answers are counted.
#[must_use]
pub fn fails_path() -> Option<PathBuf> {
    key_dir().map(|dir| dir.join(FAILS_NAME))
}

/// Whether `word`, read from `cwd` the way the shell would, names the key,
/// the failure count, or the directory that holds them. `~` and `$HOME`
/// are read as the passwd home, where the key is.
pub(crate) fn names_key(word: &str, cwd: Option<&Path>) -> bool {
    let (Some(dir), Some(key), Some(fails)) = (key_dir(), key_path(), fails_path()) else {
        return false;
    };
    let word = word.trim_matches(|c| c == '"' || c == '\'');
    let home = crate::proc_tree::own_home();
    let under_home = ["~/", "$HOME/", "${HOME}/"]
        .iter()
        .find_map(|prefix| word.strip_prefix(prefix));
    let path = if let Some(rest) = under_home {
        let Some(home) = home else { return false };
        home.join(rest)
    } else if Path::new(word).is_absolute() {
        PathBuf::from(word)
    } else if let Some(cwd) = cwd {
        cwd.join(word)
    } else {
        return false;
    };
    let mut plain = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                plain.pop();
            }
            other => plain.push(other),
        }
    }
    [dir, key, fails].contains(&plain)
}

/// Ask the person at the controlling terminal for the passphrase, to run
/// `what` (`git push`) as them once. `marks` are the runner variables
/// set, by name. Nothing is kept on a right answer but a cleared failure
/// count: no token, no variable, no cache.
///
/// # Errors
///
/// Why the person was not asked, or was asked and not answered: no key,
/// a lockout, no controlling terminal, nothing typed, a wrong answer.
pub fn ask_person(marks: &[String], what: &str) -> std::result::Result<(), String> {
    let (Some(key), Some(fails)) = (key_path(), fails_path()) else {
        return Err("this user's passwd entry names no home, so no passphrase is kept".into());
    };
    let tty = crate::person_tty::open().ok();
    ask_with(&key, &fails, tty, marks, what, now_secs, std::thread::sleep)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// [`ask_person`] with the key and failure paths, the terminal, the
/// clock and the pause given.
fn ask_with(
    key: &Path,
    fails: &Path,
    tty: Option<std::fs::File>,
    marks: &[String],
    what: &str,
    now: impl Fn() -> u64,
    pause: impl Fn(std::time::Duration),
) -> std::result::Result<(), String> {
    let phc = match read(key) {
        Ok(Some(phc)) => phc,
        Ok(None) => return Err("no person passphrase is set".into()),
        Err(e) => return Err(format!("{e:#}")),
    };
    let recent = recent_failures(fails, now())?;
    if recent.len() >= LOCK_AFTER {
        let until = recent[recent.len() - LOCK_AFTER] + WINDOW_SECS;
        return Err(format!(
            "{LOCK_AFTER} wrong passphrases in {} minutes, so ljos does not ask again until {}",
            WINDOW_SECS / 60,
            utc(until)
        ));
    }
    let Some(mut tty) = tty else {
        return Err("this process has no controlling terminal to ask on".into());
    };
    let (noun, names) = if marks.len() == 1 {
        ("a runner variable", marks[0].clone())
    } else {
        ("runner variables", marks.join(", "))
    };
    let prompt = format!(
        "ljos: this shell carries {noun} ({names}). Type your person passphrase to run this \
         {what} as you: "
    );
    let answer = match crate::person_tty::ask(&mut tty, &prompt) {
        Ok(Some(line)) if !line.bytes().is_empty() => line,
        Ok(_) => return Err("nothing was typed at the passphrase prompt".into()),
        Err(e) => return Err(format!("the passphrase prompt failed: {e:#}")),
    };
    if verify(answer.bytes(), &phc) {
        let _ = std::fs::remove_file(fails);
        return Ok(());
    }
    let count = record_failure(fails, now())
        .map(|()| recent.len() + 1)
        .unwrap_or(LOCK_AFTER);
    pause(std::time::Duration::from_secs(pause_secs(count)));
    Err("the passphrase did not match".into())
}

/// The wait after the `count`th wrong answer in the window: 2 seconds,
/// doubling each time, at most [`MAX_PAUSE_SECS`].
fn pause_secs(count: usize) -> u64 {
    let doublings = u32::try_from(count.saturating_sub(1))
        .unwrap_or(u32::MAX)
        .min(6);
    (2u64 << doublings).min(MAX_PAUSE_SECS)
}

/// The wrong answers in the window ending `now`, oldest first. A line
/// that does not read as a time counts as a wrong answer now.
fn recent_failures(fails: &Path, now: u64) -> std::result::Result<Vec<u64>, String> {
    let text = match std::fs::read_to_string(fails) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(format!(
                "the failure count at {} does not read: {e}",
                fails.display()
            ))
        }
    };
    let mut times: Vec<u64> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().parse().unwrap_or(now))
        .filter(|&t| t + WINDOW_SECS > now)
        .collect();
    times.sort_unstable();
    Ok(times)
}

fn record_failure(fails: &Path, now: u64) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(fails)?;
    writeln!(file, "{now}")?;
    file.sync_all()
}

fn utc(secs: u64) -> String {
    i64::try_from(secs)
        .ok()
        .and_then(|s| chrono::DateTime::<chrono::Utc>::from_timestamp(s, 0))
        .map_or_else(|| secs.to_string(), |t| t.format("%H:%M UTC").to_string())
}

/// The PHC string of `pass` under a fresh salt from the OS.
///
/// # Errors
///
/// The OS gave no random bytes, or the hash failed.
pub fn hash(pass: &[u8]) -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| anyhow::anyhow!("person-key: no salt from the OS: {e}"))?;
    let salt = SaltString::encode_b64(&bytes).map_err(|e| anyhow::anyhow!("person-key: {e}"))?;
    Argon2::default()
        .hash_password(pass, &salt)
        .map(|phc| phc.to_string())
        .map_err(|e| anyhow::anyhow!("person-key: {e}"))
}

/// `phc` read as an argon2id hash with its salt and output. Any other
/// algorithm is not a key.
///
/// # Errors
///
/// The text is not a PHC string, names another algorithm, or lacks a
/// salt or an output.
pub fn parse(phc: &str) -> Result<PasswordHash<'_>> {
    let parsed = PasswordHash::new(phc.trim()).map_err(|e| anyhow::anyhow!("person-key: {e}"))?;
    if parsed.algorithm != argon2::Algorithm::Argon2id.ident() {
        bail!("person-key: {} is not argon2id", parsed.algorithm);
    }
    if parsed.salt.is_none() || parsed.hash.is_none() {
        bail!("person-key: the PHC string has no salt or no hash");
    }
    Ok(parsed)
}

/// Whether `pass` is the passphrase `phc` was made from. The comparison
/// is the hash crate's, in constant time. A hash that does not parse
/// matches nothing.
#[must_use]
pub fn verify(pass: &[u8], phc: &str) -> bool {
    parse(phc).is_ok_and(|parsed| Argon2::default().verify_password(pass, &parsed).is_ok())
}

/// Write `phc` to `path`, mode 0600. The text goes to a new file beside
/// it that is renamed over the old one, so a reader sees either.
///
/// # Errors
///
/// The directory or the file cannot be written.
pub fn store(path: &Path, phc: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let dir = path
        .parent()
        .context("person-key: the key path has no directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("person-key: {}", dir.display()))?;
    let tmp = dir.join(format!(".{KEY_NAME}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(phc.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        std::fs::File::open(dir)?.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("person-key: {}", path.display()));
    }
    Ok(())
}

/// The hash kept at `path`. `Ok(None)` is no key. A file that is not a
/// regular file, or not argon2id, is an error.
///
/// # Errors
///
/// The file cannot be read or does not hold a key.
pub fn read(path: &Path) -> Result<Option<String>> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("person-key: {}", path.display())),
        Ok(meta) if !meta.is_file() => {
            bail!("person-key: {} is not a regular file", path.display())
        }
        Ok(_) => {}
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("person-key: {}", path.display()))?;
    parse(&text).with_context(|| format!("person-key: {}", path.display()))?;
    Ok(Some(text.trim().to_string()))
}

/// Run `ljos person-key ACTION`: `set`, `status` or `clear`.
///
/// # Errors
///
/// An unknown action, a refusal, or a key that cannot be written.
pub fn run(action: &str) -> Result<()> {
    let path = key_path().context("person-key: this user's passwd entry names no home")?;
    match action {
        "set" => set_at(&path),
        "clear" => clear_at(&path),
        "status" => {
            println!("{}", status_at(&path));
            Ok(())
        }
        other => bail!("person-key: no action `{other}`; use set, status or clear"),
    }
}

/// Why `set` or `clear` is not the person's, from the runner cause, the
/// terminal check and the runner variables set. `None` when it is.
fn not_the_person(
    cause: Option<crate::RunnerCause>,
    present: bool,
    marks: &[String],
) -> Option<String> {
    if cause == Some(crate::RunnerCause::Hard) {
        return Some("a runner is in this process tree or its scope".into());
    }
    if !marks.is_empty() {
        return Some(format!(
            "this shell carries {}, which a runner leaves on what it starts",
            marks.join(", ")
        ));
    }
    if cause.is_some() || !present {
        return Some("ljos cannot attribute this terminal to you".into());
    }
    None
}

fn person_only(verb: &str) -> Result<()> {
    let marks = crate::runner_marks();
    match not_the_person(crate::runner_cause(), crate::person_present(), &marks) {
        Some(why) => bail!(
            "person-key: `ljos person-key {verb}` is the person's, from a terminal of their \
             own with no runner variable set, and {why}. Run it on your own machine"
        ),
        None => Ok(()),
    }
}

fn set_at(path: &Path) -> Result<()> {
    person_only("set")?;
    let mut tty = crate::person_tty::open().context("person-key")?;
    set_from(path, &mut tty)
}

/// Read the new passphrase twice from `tty` and keep its hash at `path`.
fn set_from(path: &Path, tty: &mut std::fs::File) -> Result<()> {
    let Some(first) = crate::person_tty::ask(tty, "New person passphrase: ")? else {
        bail!("person-key: the terminal closed; the key is unchanged");
    };
    if first.bytes().len() < MIN_LEN {
        bail!("person-key: a passphrase needs at least {MIN_LEN} bytes; the key is unchanged");
    }
    let Some(second) = crate::person_tty::ask(tty, "The same again: ")? else {
        bail!("person-key: the terminal closed; the key is unchanged");
    };
    if first.bytes() != second.bytes() {
        bail!("person-key: the two entries differ; the key is unchanged");
    }
    store(path, &hash(first.bytes())?)?;
    println!("person-key: set in {}", path.display());
    Ok(())
}

fn clear_at(path: &Path) -> Result<()> {
    person_only("clear")?;
    let _ = std::fs::remove_file(path.with_file_name(FAILS_NAME));
    match std::fs::remove_file(path) {
        Ok(()) => println!("person-key: cleared {}", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("person-key: no key at {}", path.display());
        }
        Err(e) => return Err(e).with_context(|| format!("person-key: {}", path.display())),
    }
    Ok(())
}

/// Whether a key is kept at `path`, and when it was written. Never the hash.
fn status_at(path: &Path) -> String {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            let when = meta
                .modified()
                .map(|t| {
                    chrono::DateTime::<chrono::Utc>::from(t)
                        .format("%Y-%m-%d %H:%M:%S UTC")
                        .to_string()
                })
                .unwrap_or_else(|_| "an unknown time".into());
            format!("person-key: set, written {when} ({})", path.display())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => format!(
            "person-key: not set ({}); run `ljos person-key set` from your own terminal",
            path.display()
        ),
        Err(e) => format!("person-key: {} does not read: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hard runner, any runner variable, or no terminal refuses. Only
    /// the person at a terminal with no variable set may change the key.
    #[test]
    fn only_the_person_without_a_mark_may_change_the_key() {
        use crate::RunnerCause::{Hard, MarkOnly};
        let mark = vec!["CURSOR_AGENT".to_string()];
        for (cause, present, marks) in [
            (Some(Hard), false, vec![]),
            (Some(Hard), false, mark.clone()),
            (Some(MarkOnly), false, mark.clone()),
            (None, true, mark.clone()),
            (None, false, vec![]),
        ] {
            assert!(
                not_the_person(cause, present, &marks).is_some(),
                "{cause:?} {present} {marks:?}"
            );
        }
        assert!(not_the_person(None, true, &[]).is_none());
        let why = not_the_person(None, true, &mark).unwrap();
        assert!(why.contains("CURSOR_AGENT"), "{why}");
    }

    /// `set` and `clear` with a runner variable set refuse before they
    /// read the terminal, with or without a key, and leave the key as it
    /// was. The test stands in for the person at a terminal.
    #[test]
    fn a_mark_refuses_set_and_clear_with_or_without_a_key() {
        let _g = crate::tests::env_guard();
        let dir = tempfile::tempdir().unwrap();
        let none = dir.path().join("none").join(KEY_NAME);
        let kept = dir.path().join("kept").join(KEY_NAME);
        let phc = hash(b"correct horse battery").unwrap();
        store(&kept, &phc).unwrap();
        let saved = std::env::var_os("CURSOR_AGENT");
        // SAFETY: under env_guard, and restored below.
        unsafe { std::env::set_var("CURSOR_AGENT", "1") };
        let mut said = Vec::new();
        for at_terminal in [true, false] {
            crate::PERSON_AT_TERMINAL.with(|p| p.set(at_terminal));
            for path in [&none, &kept] {
                said.push(set_at(path).unwrap_err().to_string());
                said.push(clear_at(path).unwrap_err().to_string());
            }
        }
        crate::PERSON_AT_TERMINAL.with(|p| p.set(false));
        unsafe {
            match saved {
                Some(value) => std::env::set_var("CURSOR_AGENT", value),
                None => std::env::remove_var("CURSOR_AGENT"),
            }
        }
        for why in &said {
            assert!(why.contains("is the person's"), "{why}");
            assert!(!why.contains("terminal closed"), "{why}");
        }
        assert!(!none.exists());
        assert_eq!(read(&kept).unwrap().as_deref(), Some(phc.as_str()));
    }

    /// A pty pair: what the test types goes to the master, and the slave
    /// is the terminal the code reads.
    fn pty_pair() -> (std::fs::File, std::fs::File) {
        use std::os::fd::FromRawFd as _;
        let (mut master, mut slave) = (-1, -1);
        // SAFETY: openpty writes two fresh descriptors into the locals.
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert_eq!(rc, 0);
        // SAFETY: both descriptors are owned here alone. Close-on-exec keeps
        // them out of what other tests start meanwhile.
        unsafe {
            libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(slave, libc::F_SETFD, libc::FD_CLOEXEC);
            (
                std::fs::File::from_raw_fd(master),
                std::fs::File::from_raw_fd(slave),
            )
        }
    }

    fn echo_on(tty: &std::fs::File) -> bool {
        use std::os::fd::AsRawFd as _;
        // SAFETY: termios is plain data, filled in from a tty descriptor.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(tty.as_raw_fd(), &mut t), 0);
            t.c_lflag & libc::ECHO != 0
        }
    }

    /// Types each line on `master` once the terminal shows its prompt,
    /// the way a person answers, and returns all the terminal showed.
    /// Echo off drops type-ahead, so nothing is typed sooner.
    fn typist(
        mut master: std::fs::File,
        answers: Vec<(&'static str, &'static str)>,
    ) -> std::thread::JoinHandle<String> {
        use std::io::{Read as _, Write as _};
        std::thread::spawn(move || {
            let mut shown = Vec::new();
            let mut since = 0;
            let mut answers = answers.into_iter().peekable();
            let mut buf = [0u8; 256];
            while let Ok(n) = master.read(&mut buf) {
                if n == 0 {
                    break;
                }
                shown.extend_from_slice(&buf[..n]);
                if let Some((prompt, line)) = answers.peek() {
                    if String::from_utf8_lossy(&shown[since..]).contains(prompt) {
                        master.write_all(format!("{line}\n").as_bytes()).unwrap();
                        since = shown.len();
                        answers.next();
                    }
                }
            }
            String::from_utf8_lossy(&shown).into_owned()
        })
    }

    /// Typed twice on the terminal, a long enough passphrase is kept and
    /// verifies; echo is back on after, and the terminal never showed it.
    #[test]
    fn set_keeps_a_passphrase_typed_twice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_NAME);
        let (master, mut slave) = pty_pair();
        assert!(echo_on(&slave));
        let shown = typist(
            master,
            vec![
                ("New person passphrase: ", "correct horse battery"),
                ("The same again: ", "correct horse battery"),
            ],
        );
        set_from(&path, &mut slave).unwrap();
        assert!(echo_on(&slave), "echo stayed off");
        drop(slave);
        let shown = shown.join().unwrap();
        assert!(shown.contains("The same again: "), "{shown}");
        assert!(!shown.contains("horse"), "{shown}");
        let phc = read(&path).unwrap().expect("a key");
        assert!(verify(b"correct horse battery", &phc));
    }

    #[test]
    fn set_refuses_a_short_or_mismatched_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_NAME);
        for answers in [
            vec![("New person passphrase: ", "short one")],
            vec![
                ("New person passphrase: ", "correct horse battery"),
                ("The same again: ", "correct horse batterz"),
            ],
        ] {
            let (master, mut slave) = pty_pair();
            let shown = typist(master, answers);
            assert!(set_from(&path, &mut slave).is_err());
            assert!(echo_on(&slave));
            drop(slave);
            shown.join().unwrap();
        }
        assert!(!path.exists());
    }

    const PROMPT: &str = "Type your person passphrase to run this git push as you: ";

    struct Asked {
        out: std::result::Result<(), String>,
        shown: String,
        pauses: Vec<u64>,
    }

    /// [`ask_with`] on a pty that types `typed` at the prompt (or the
    /// end-of-file key for `None`), at time `now`, with the pauses kept.
    fn ask_at(dir: &Path, typed: Option<&'static str>, now: u64) -> Asked {
        let (master, slave) = pty_pair();
        let answer = typed.unwrap_or("\u{4}");
        let shown = typist(master, vec![(PROMPT, answer)]);
        let pauses = std::cell::RefCell::new(Vec::new());
        let out = ask_with(
            &dir.join(KEY_NAME),
            &dir.join(FAILS_NAME),
            Some(slave),
            &["CURSOR_AGENT".to_string()],
            "git push",
            || now,
            |d| pauses.borrow_mut().push(d.as_secs()),
        );
        Asked {
            out,
            shown: shown.join().unwrap(),
            pauses: pauses.into_inner(),
        }
    }

    fn keyed() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        store(
            &dir.path().join(KEY_NAME),
            &hash(b"correct horse battery").unwrap(),
        )
        .unwrap();
        dir
    }

    #[test]
    fn the_right_passphrase_answers_once_and_clears_the_count() {
        let dir = keyed();
        std::fs::write(dir.path().join(FAILS_NAME), "1000\n").unwrap();
        let asked = ask_at(dir.path(), Some("correct horse battery"), 1100);
        assert_eq!(asked.out, Ok(()));
        assert!(
            asked
                .shown
                .contains("this shell carries a runner variable (CURSOR_AGENT)"),
            "{}",
            asked.shown
        );
        assert!(!asked.shown.contains("horse"), "{}", asked.shown);
        assert!(asked.pauses.is_empty());
        assert!(!dir.path().join(FAILS_NAME).exists());
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(
            names,
            vec![std::ffi::OsString::from(KEY_NAME)],
            "a token was kept"
        );
    }

    #[test]
    fn a_wrong_passphrase_waits_and_is_counted() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = keyed();
        let asked = ask_at(dir.path(), Some("correct horse batterz"), 1000);
        assert_eq!(asked.out, Err("the passphrase did not match".into()));
        assert_eq!(asked.pauses, vec![2]);
        let fails = dir.path().join(FAILS_NAME);
        assert_eq!(std::fs::read_to_string(&fails).unwrap(), "1000\n");
        let mode = std::fs::metadata(&fails).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let asked = ask_at(dir.path(), Some("correct horse batterz"), 1010);
        assert_eq!(asked.pauses, vec![4]);
        assert_eq!(
            (1..=7).map(pause_secs).collect::<Vec<_>>(),
            vec![2, 4, 8, 16, 32, 60, 60]
        );
    }

    #[test]
    fn five_wrong_in_the_window_lock_out_without_a_prompt() {
        let dir = keyed();
        let mut pauses = Vec::new();
        for i in 0..5 {
            let asked = ask_at(dir.path(), Some("not the passphrase"), 1000 + i);
            assert!(asked.out.is_err());
            pauses.extend(asked.pauses);
        }
        assert_eq!(pauses, vec![2, 4, 8, 16, 32]);
        let (master, slave) = pty_pair();
        let shown = typist(master, vec![]);
        let out = ask_with(
            &dir.path().join(KEY_NAME),
            &dir.path().join(FAILS_NAME),
            Some(slave),
            &["CURSOR_AGENT".to_string()],
            "git push",
            || 1010,
            |_| panic!("a lockout waited"),
        );
        let why = out.unwrap_err();
        assert!(why.contains("does not ask again until"), "{why}");
        assert!(shown.join().unwrap().is_empty(), "the lockout prompted");
        let after = ask_at(
            dir.path(),
            Some("correct horse battery"),
            1004 + WINDOW_SECS,
        );
        assert_eq!(after.out, Ok(()), "the window passed and still locked");
    }

    /// No key, no terminal, or nothing typed: refused, and nothing typed
    /// is not counted as a wrong answer.
    #[test]
    fn no_key_no_terminal_or_no_answer_refuses() {
        let empty = tempfile::tempdir().unwrap();
        let asked = ask_at(empty.path(), Some("correct horse battery"), 1000);
        assert_eq!(asked.out, Err("no person passphrase is set".into()));
        assert!(asked.shown.is_empty(), "{}", asked.shown);
        let dir = keyed();
        let out = ask_with(
            &dir.path().join(KEY_NAME),
            &dir.path().join(FAILS_NAME),
            None,
            &["CURSOR_AGENT".to_string()],
            "git push",
            || 1000,
            |_| {},
        );
        assert!(out.unwrap_err().contains("no controlling terminal"));
        let asked = ask_at(dir.path(), None, 1000);
        assert!(asked.out.unwrap_err().contains("nothing was typed"));
        assert!(!dir.path().join(FAILS_NAME).exists());
    }

    #[test]
    fn status_names_the_key_but_not_its_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_NAME);
        assert!(status_at(&path).contains("not set"));
        let phc = hash(b"correct horse battery").unwrap();
        store(&path, &phc).unwrap();
        let said = status_at(&path);
        assert!(said.contains("person-key: set, written "), "{said}");
        assert!(!said.contains("argon2"), "{said}");
        assert!(!said.contains(phc.rsplit('$').next().unwrap()), "{said}");
    }

    #[test]
    fn a_hash_matches_its_passphrase_only() {
        let phc = hash(b"correct horse battery").unwrap();
        assert!(phc.starts_with("$argon2id$"), "{phc}");
        assert!(verify(b"correct horse battery", &phc));
        assert!(!verify(b"correct horse batterz", &phc));
        assert!(!verify(b"", &phc));
        let again = hash(b"correct horse battery").unwrap();
        assert_ne!(phc, again, "two hashes shared a salt");
    }

    #[test]
    fn only_an_argon2id_phc_string_parses() {
        let phc = hash(b"correct horse battery").unwrap();
        let parsed = parse(&phc).unwrap();
        assert_eq!(parsed.algorithm.as_str(), "argon2id");
        assert!(parsed.salt.is_some());
        let argon2i = phc.replacen("$argon2id$", "$argon2i$", 1);
        assert!(parse(&argon2i).is_err());
        assert!(!verify(b"correct horse battery", &argon2i));
        for junk in [
            "",
            "correct horse battery",
            "$argon2id$",
            "$argon2id$v=19$m=1",
        ] {
            assert!(parse(junk).is_err(), "{junk:?}");
            assert!(!verify(b"correct horse battery", junk));
        }
    }

    #[test]
    fn the_stored_key_is_mode_0600_and_reads_back() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ljos").join(KEY_NAME);
        assert!(read(&path).unwrap().is_none());
        let phc = hash(b"correct horse battery").unwrap();
        store(&path, &phc).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(read(&path).unwrap().as_deref(), Some(phc.as_str()));
        let next = hash(b"another long phrase").unwrap();
        store(&path, &next).unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some(next.as_str()));
        let left: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from(KEY_NAME)]);
        std::fs::write(&path, "not a key\n").unwrap();
        assert!(read(&path).is_err());
    }

    #[test]
    fn a_link_in_place_of_the_key_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("elsewhere");
        store(&real, &hash(b"correct horse battery").unwrap()).unwrap();
        let path = dir.path().join(KEY_NAME);
        std::os::unix::fs::symlink(&real, &path).unwrap();
        assert!(read(&path).is_err());
    }

    /// `HOME` and `XDG_CONFIG_HOME` point at a temporary directory, and
    /// the key path still names the passwd entry's home.
    #[test]
    fn the_key_path_follows_the_passwd_home() {
        let _g = crate::tests::env_guard();
        let own = crate::proc_tree::own_home().expect("a passwd home");
        let tmp = tempfile::tempdir().unwrap();
        let saved: Vec<_> = ["HOME", "XDG_CONFIG_HOME"]
            .iter()
            .map(|k| (*k, std::env::var_os(k)))
            .collect();
        // SAFETY: under env_guard, and restored below.
        unsafe {
            std::env::set_var("HOME", tmp.path());
            std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("cfg"));
        }
        let got = key_path();
        unsafe {
            for (k, v) in saved {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
        assert_eq!(got, Some(own.join(".config/ljos/person-key")));
        assert!(!got.unwrap().starts_with(tmp.path()));
    }
}
