//! The person's passphrase. A shell can carry a runner's variable on a
//! host that exports it to everything it starts, and then the gates
//! cannot tell the person from a runner by the process alone. The
//! passphrase is what the person knows and a runner does not.
//!
//! Only an argon2id hash is kept, in PHC form with a random salt, in
//! `person-key` under the ljos config directory of the home the passwd
//! entry names. `HOME` and `XDG_CONFIG_HOME` do not move it: a runner
//! sets both for what it starts.
//!
//! The hash is made with a pepper, 32 random bytes in
//! `person-key.pepper` beside it, which only `ljos person-key set`
//! writes. The pepper keeps a copy of the hash alone from being guessed.
//! It does not stop a process of the same Unix user, which can read both
//! files and guess offline; the cost of each guess is all that slows it.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use argon2::password_hash::{Output, ParamsString, PasswordHash, SaltString};
use argon2::{Algorithm, Argon2, Block, Params, Version};
use zeroize::Zeroizing;

/// The shortest passphrase `ljos person-key set` takes, in bytes.
pub const MIN_LEN: usize = 16;

/// The argon2id cost of a new hash: 64 MiB, three passes, four lanes.
/// RFC 9106 recommends it where 2 GiB is too much. OWASP's floor is
/// 19 MiB and two passes, or 46 MiB and one. A hash below it on any
/// count is not a key.
const M_COST_KIB: u32 = 64 * 1024;
const T_COST: u32 = 3;
const P_COST: u32 = 4;

const KEY_NAME: &str = "person-key";

/// The pepper argon2id takes as its secret, beside the key.
const PEPPER_NAME: &str = "person-key.pepper";
const PEPPER_LEN: usize = 32;

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

/// The directories a tool call may not write into, move, link over or
/// change the mode of, each resolved and as spelled: the key directory
/// with all it holds, and the config directory above it, itself only.
fn guarded() -> Option<(Vec<PathBuf>, Vec<PathBuf>)> {
    let dir = key_dir()?;
    let both = |p: PathBuf| {
        let real = crate::kept_prompt::resolve(&p);
        if real == p {
            vec![p]
        } else {
            vec![real, p]
        }
    };
    let above = dir.parent().map(|p| both(p.to_path_buf()));
    Some((both(dir), above.unwrap_or_default()))
}

/// The paths `word` names from `cwd`: as the shell expands it, and with
/// `~` and `$HOME` read as the passwd home, where the key is. `None` for
/// a word the guard cannot expand.
fn paths_of(word: &str, cwd: Option<&Path>) -> Vec<Option<PathBuf>> {
    let mut out = vec![crate::kept_prompt::path_of(word, cwd)];
    if let Some(home) = crate::proc_tree::own_home() {
        for prefix in ["~", "$HOME", "${HOME}"] {
            if let Some(rest) = word
                .strip_prefix(prefix)
                .filter(|r| r.is_empty() || r.starts_with('/'))
            {
                let word = format!("{}{rest}", home.display());
                out.push(crate::kept_prompt::path_of(&word, cwd));
            }
        }
    }
    out
}

/// The word of one command, its wrappers off, that would write, move,
/// link over or change the mode of the key, its pepper, its failure
/// count, their directory, or the config directory above it. Each word
/// is resolved first: `~`, `$HOME` and the directory the shell stands in
/// expanded, `.`, `..` and `//` taken out, and the symlinks of the part
/// that exists followed. A redirection counts for any command, and an
/// operand for anything but a reader. A word the guard cannot expand
/// counts when its file name is one of the key's files.
pub(crate) fn reached(words: &[String], cwd: Option<&Path>, reader: bool) -> Option<String> {
    let (inside, above) = guarded()?;
    let hits = |word: &str| {
        let word = word.trim_matches(|c| c == '"' || c == '\'');
        paths_of(word, cwd).into_iter().any(|path| match path {
            Some(p) => {
                inside.iter().any(|d| crate::kept_prompt::within(&p, d))
                    || above.iter().any(|d| crate::kept_prompt::names_dir(&p, d))
            }
            None => [KEY_NAME, FAILS_NAME, PEPPER_NAME].contains(
                &word
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or(word),
            ),
        })
    };
    let mut it = words.iter().skip(1);
    while let Some(w) = it.next() {
        if w == ">" {
            match it.next() {
                Some(t) if hits(t) => return Some(t.clone()),
                _ => continue,
            }
        }
        if reader || w.contains(char::is_whitespace) || w.contains("://") {
            continue;
        }
        if hits(w) || w.split_once('=').is_some_and(|(_, v)| hits(v)) {
            return Some(w.clone());
        }
    }
    None
}

/// Programs a `find -exec` runs that remove or move what it finds.
const UPROOTERS: &[&str] = &["rm", "rmdir", "unlink", "mv", "shred", "trash", "trash-put"];

/// A program's long options, and the long option each short letter is.
type Options = (
    &'static str,
    &'static [&'static str],
    &'static [(char, &'static str)],
);

/// The long options of `rm` and `mv` as GNU names them, those that take
/// a value ending in `=`, and the long option each short letter is.
const OPTIONS: &[Options] = &[
    (
        "rm",
        &[
            "dir",
            "force",
            "interactive",
            "no-preserve-root",
            "one-file-system",
            "preserve-root",
            "recursive",
            "verbose",
            "help",
            "version",
        ],
        &[
            ('d', "dir"),
            ('f', "force"),
            ('i', "interactive"),
            ('I', "interactive"),
            ('r', "recursive"),
            ('R', "recursive"),
            ('v', "verbose"),
        ],
    ),
    (
        "mv",
        &[
            "backup",
            "context",
            "debug",
            "exchange",
            "force",
            "interactive",
            "no-clobber",
            "no-copy",
            "no-target-directory",
            "strip-trailing-slashes",
            "suffix=",
            "target-directory=",
            "update",
            "verbose",
            "help",
            "version",
        ],
        &[
            ('b', "backup"),
            ('f', "force"),
            ('i', "interactive"),
            ('n', "no-clobber"),
            ('S', "suffix="),
            ('t', "target-directory="),
            ('T', "no-target-directory"),
            ('u', "update"),
            ('v', "verbose"),
            ('Z', "context"),
        ],
    ),
];

/// The most words [`braces`] makes of one.
const MAX_BRACED: usize = 64;

/// `word` with its brace expansions made, as the shell makes them before
/// any other: `~/{.config,x}` is `~/.config` and `~/x`. A `${` is a
/// variable, and a word with a blank in it was quoted, so both stay.
fn braces(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    if word.contains(char::is_whitespace) {
        return vec![word.to_string()];
    }
    for open in (0..chars.len()).filter(|&i| chars[i] == '{' && (i == 0 || chars[i - 1] != '$')) {
        let mut depth = 0;
        let mut cuts = vec![open];
        for (j, c) in chars.iter().enumerate().skip(open) {
            match c {
                '{' => depth += 1,
                ',' if depth == 1 => cuts.push(j),
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        cuts.push(j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = cuts[cuts.len() - 1];
        if depth != 0 || cuts.len() < 3 {
            continue;
        }
        let pre: String = chars[..open].iter().collect();
        let post: String = chars[close + 1..].iter().collect();
        let mut out = Vec::new();
        for pair in cuts.windows(2) {
            let alt: String = chars[pair[0] + 1..pair[1]].iter().collect();
            out.extend(braces(&format!("{pre}{alt}{post}")));
            if out.len() >= MAX_BRACED {
                out.truncate(MAX_BRACED);
                break;
            }
        }
        return out;
    }
    vec![word.to_string()]
}

/// The words of one command as the program reads them: wrappers such as
/// `env`, `nice`, `timeout` and `xargs` off, and each brace expansion
/// made. For `rm` and `mv` each option is spelled out whole, as `--name`
/// or `--name=value`: a long option by the one name it is a prefix of,
/// and each letter of a short group by its long name, a value taken
/// from the rest of the group or the next word.
fn as_run(words: &[String]) -> Vec<String> {
    let words: Vec<String> = words[crate::prefix_end(words).0..]
        .iter()
        .flat_map(|w| braces(w))
        .collect();
    let cmd = words
        .first()
        .map_or("", |w| w.rsplit('/').next().unwrap_or(w));
    let Some((_, long, short)) = OPTIONS.iter().find(|(n, _, _)| *n == cmd) else {
        return words;
    };
    let mut out = words[..1].to_vec();
    let mut it = words.iter().skip(1);
    while let Some(w) = it.next() {
        let mut take = |name: &str, value: Option<&str>| match name.strip_suffix('=') {
            Some(name) => {
                let value = value.map(str::to_string).or_else(|| it.next().cloned());
                out.push(format!("--{name}={}", value.unwrap_or_default()));
            }
            None => {
                out.push(value.map_or_else(|| format!("--{name}"), |v| format!("--{name}={v}")))
            }
        };
        if w == "--" {
            out.push(w.clone());
            out.extend(it.cloned());
            break;
        } else if let Some(given) = w.strip_prefix("--") {
            let (given, value) = given
                .split_once('=')
                .map_or((given, None), |(n, v)| (n, Some(v)));
            let exact = long.iter().find(|n| n.trim_end_matches('=') == given);
            let mut prefixed = long.iter().filter(|n| n.starts_with(given));
            match exact.or_else(|| prefixed.next().filter(|_| prefixed.next().is_none())) {
                Some(name) => take(name, value),
                None => out.push(w.clone()),
            }
        } else if w.len() > 1 && w.starts_with('-') {
            for (i, c) in w.char_indices().skip(1) {
                let Some((_, name)) = short.iter().find(|(s, _)| *s == c) else {
                    continue;
                };
                if name.ends_with('=') {
                    let rest = &w[i + c.len_utf8()..];
                    take(name, Some(rest).filter(|r| !r.is_empty()));
                    break;
                }
                take(name, None);
            }
        } else {
            out.push(w.clone());
        }
    }
    out
}

/// The operands one command, its wrappers off, removes or moves with all
/// they hold: those of `rm -r`, the sources of `mv`, `trash` and
/// `gio trash`, the destination of `rsync --delete` and the sources of
/// `rsync --remove-source-files`, and the starting points of a `find`
/// that deletes or runs one of [`UPROOTERS`]. Options are read as
/// [`as_run`] spells them.
fn uprooted(words: &[String]) -> Vec<String> {
    let words = as_run(words);
    let cmd = words
        .first()
        .map_or("", |w| w.rsplit('/').next().unwrap_or(w));
    let mut args: Vec<&str> = Vec::new();
    let mut flags: Vec<&str> = Vec::new();
    let mut target = None;
    let mut it = words.iter().skip(1).map(String::as_str);
    let mut ended = false;
    while let Some(w) = it.next() {
        if w == ">" || w == ">>" {
            it.next();
        } else if ended || !w.starts_with('-') || w == "-" {
            args.push(w);
        } else if w == "--" {
            ended = true;
        } else if let Some(dir) = w.strip_prefix("--target-directory=") {
            target = Some(dir);
        } else {
            flags.push(w);
        }
    }
    let recursive = flags.contains(&"--recursive");
    let picked: Vec<&str> = match cmd {
        "rm" if recursive => args,
        "mv" if target.is_some() => args,
        "mv" => args
            .split_last()
            .map(|(_, s)| s.to_vec())
            .unwrap_or_default(),
        "trash" | "trash-put" => args,
        "gio" if args.first() == Some(&"trash") => args[1..].to_vec(),
        "rsync" => {
            let mut out = Vec::new();
            if let Some((last, sources)) = args.split_last() {
                if flags.iter().any(|f| f.starts_with("--del")) {
                    out.push(*last);
                }
                if flags.contains(&"--remove-source-files") {
                    out.extend(sources);
                }
            }
            out
        }
        "find" => {
            let rest: Vec<&str> = words.iter().skip(1).map(String::as_str).collect();
            let deletes = rest.iter().enumerate().any(|(i, w)| {
                *w == "-delete"
                    || (matches!(*w, "-exec" | "-execdir" | "-ok" | "-okdir")
                        && as_run(&words[i + 2..]).first().is_some_and(|p| {
                            UPROOTERS.contains(&p.rsplit('/').next().unwrap_or(p))
                        }))
            });
            let mut starts = Vec::new();
            let mut i = 0;
            while i < rest.len() {
                match rest[i] {
                    "-H" | "-L" | "-P" => {}
                    "-D" => i += 1,
                    w if w.starts_with("-O") => {}
                    w if w.starts_with('-') || w == "(" || w == "!" || w == "," => break,
                    w => starts.push(w),
                }
                i += 1;
            }
            if deletes {
                starts
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };
    picked.into_iter().map(str::to_string).collect()
}

/// The word of one command that removes or moves, with all it holds, the
/// key directory or a directory above it: `rm -rf ~`, `find ~ -delete`,
/// `mv ~/.config x`. Each word is resolved as [`reached`] resolves it.
pub(crate) fn uproots(words: &[String], cwd: Option<&Path>) -> Option<String> {
    let (inside, _) = guarded()?;
    uprooted(words).into_iter().find_map(|word| {
        let bare = word.trim_matches(|c| c == '"' || c == '\'');
        paths_of(bare, cwd)
            .into_iter()
            .flatten()
            .any(|p| inside.iter().any(|d| crate::kept_prompt::encloses(&p, d)))
            .then(|| word.clone())
    })
}

/// Calls in Python, Perl, Ruby and Node code that remove or move the
/// tree their first argument names.
const TREE_CALLS: &str = "rmtree removedirs remove_tree rm_rf rm_r rm rmSync rmdir rmdirSync \
                          move mv rename renames renameSync";

/// Calls whose arguments build the path they return.
const PATH_CALLS: &str = "join expanduser expand_path abspath realpath normpath resolve Path \
                          PurePath str fspath catdir catfile";

/// Ways code names the home, each read as `'~'`.
const HOME_CALLS: &[&str] = &[
    "Path.home()",
    "homedir()",
    "Dir.home",
    "getenv('HOME')",
    "getenv(\"HOME\")",
    "environ['HOME']",
    "environ[\"HOME\"]",
    "environ.get('HOME')",
    "environ.get(\"HOME\")",
    "ENV['HOME']",
    "ENV[\"HOME\"]",
    "$ENV{HOME}",
    "$ENV{'HOME'}",
    "process.env.HOME",
];

fn ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

fn listed(list: &str, name: &str) -> bool {
    list.split_whitespace().any(|n| n == name)
}

/// `code` with each of [`HOME_CALLS`] read as `'~'`.
fn homes_quoted(code: &str) -> Vec<char> {
    let mut code = code.to_string();
    for call in HOME_CALLS {
        code = code.replace(call, "'~'");
    }
    code.chars().collect()
}

/// The text of the string that opens at `chars[at]`, and where it ends.
fn quoted(chars: &[char], at: usize) -> (String, usize) {
    let mut i = at + 1;
    while i < chars.len() && chars[i] != chars[at] {
        i += usize::from(chars[i] == '\\') + 1;
    }
    let end = i.min(chars.len());
    (
        chars[at + 1..end].iter().collect(),
        (end + 1).min(chars.len()),
    )
}

/// Where the bracket that opens at `chars[at]` closes, or, with
/// `comma`, where the first comma inside it is.
fn bracket_end(chars: &[char], at: usize, comma: bool) -> usize {
    let mut depth = 0;
    let mut i = at;
    while i < chars.len() {
        match chars[i] {
            '\'' | '"' | '`' => i = quoted(chars, i).1 - 1,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 1 => return i,
            ')' | ']' | '}' => depth -= 1,
            ',' if comma && depth == 1 => return i,
            _ => {}
        }
        i += 1;
    }
    chars.len()
}

/// The path an expression of code builds: its string literals, homes,
/// the arguments of [`PATH_CALLS`] and the variables `vars` knows, in
/// order and joined by `/`. `None` for a variable not known, a string
/// with interpolation, or no part at all.
fn built(expr: &str, vars: &std::collections::HashMap<String, String>) -> Option<String> {
    let chars = homes_quoted(expr);
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '\'' | '"' | '`') {
            let (text, end) = quoted(&chars, i);
            let formatted = i > 0 && matches!(chars[i - 1], 'f' | 'F');
            if (formatted && text.contains('{')) || text.contains("${") {
                return None;
            }
            parts.push(text);
            i = end;
        } else if ident(c) && (i == 0 || !ident(chars[i - 1])) {
            let start = i;
            while i < chars.len() && ident(chars[i]) {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            let after = i + chars[i..].iter().take_while(|c| c.is_whitespace()).count();
            let next = chars.get(after).copied();
            if next == Some('(') && !listed(PATH_CALLS, &name) {
                i = bracket_end(&chars, after, false) + 1;
            } else if next.is_none_or(|n| !matches!(n, '(' | '.' | '\'' | '"'))
                && (start == 0 || chars[start - 1] != '.')
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && !matches!(name.as_str(), "True" | "False" | "None" | "true" | "false")
            {
                parts.push(vars.get(&name)?.clone());
            }
        } else {
            i += 1;
        }
    }
    let mut out = parts.first()?.clone();
    for part in &parts[1..] {
        if !out.ends_with('/') && !part.starts_with('/') {
            out.push('/');
        }
        out.push_str(part);
    }
    Some(out)
}

/// The call in the code one command runs, as
/// [`crate::kept_prompt::code_of`] finds it, that removes or moves the
/// key directory or a directory above it, such as
/// `shutil.rmtree(os.path.expanduser('~'))`. The first argument of each
/// of [`TREE_CALLS`] is read as [`built`] reads it, with the variables
/// earlier statements set. When it cannot be read, a string or home
/// anywhere in the code that alone names such a directory counts.
pub(crate) fn code_uproots(words: &[String], line: &str, cwd: Option<&Path>) -> Option<String> {
    let (inside, _) = guarded()?;
    let (code, _) = crate::kept_prompt::code_of(words, line)?;
    let hits = |word: &str| {
        paths_of(word, cwd)
            .into_iter()
            .flatten()
            .any(|p| inside.iter().any(|d| crate::kept_prompt::encloses(&p, d)))
    };
    let mut vars = std::collections::HashMap::new();
    for stmt in code.split(['\n', ';']) {
        let stmt = stmt.trim();
        let stmt = ["const ", "let ", "var ", "my "]
            .iter()
            .find_map(|k| stmt.strip_prefix(k))
            .unwrap_or(stmt);
        let Some((name, value)) = stmt.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() && name.chars().all(ident) && !value.starts_with(['=', '>']) {
            match built(value, &vars) {
                Some(path) => vars.insert(name.to_string(), path),
                None => vars.remove(name),
            };
        }
    }
    let named_anywhere = || {
        let chars = homes_quoted(code);
        let mut i = 0;
        while i < chars.len() {
            if matches!(chars[i], '\'' | '"') {
                let (text, end) = quoted(&chars, i);
                if hits(&text) {
                    return true;
                }
                i = end;
            } else {
                i += 1;
            }
        }
        false
    };
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        while i < chars.len() && ident(chars[i]) {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect();
        if i == start || chars.get(i) != Some(&'(') || !listed(TREE_CALLS, &name) {
            i += 1;
            continue;
        }
        let end = bracket_end(&chars, i, true).max(i + 1);
        let arg: String = chars[i + 1..end].iter().collect();
        if built(&arg, &vars).map_or_else(named_anywhere, |path| hits(&path)) {
            return Some(format!("{name}({})", arg.trim()));
        }
    }
    None
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
    let Some(dir) = key_dir() else {
        return Err("this user's passwd entry names no home, so no passphrase is kept".into());
    };
    let tty = crate::person_tty::open().ok();
    ask_with(&dir, tty, marks, what, now_secs, std::thread::sleep)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// [`ask_person`] with the key directory, the terminal, the clock and
/// the pause given. The directory is opened once, and the key, pepper
/// and failure count are all reached through it.
fn ask_with(
    dir: &Path,
    tty: Option<std::fs::File>,
    marks: &[String],
    what: &str,
    now: impl Fn() -> u64,
    pause: impl Fn(std::time::Duration),
) -> std::result::Result<(), String> {
    let at = match KeyDir::open(dir, false) {
        Ok(Some(at)) => at,
        Ok(None) => return Err("no person passphrase is set".into()),
        Err(e) => return Err(format!("{e:#}")),
    };
    let phc = match key_in(&at, KEY_NAME) {
        Ok(Some(phc)) => phc,
        Ok(None) => return Err("no person passphrase is set".into()),
        Err(e) => return Err(format!("{e:#}")),
    };
    let pepper = pepper_in(&at).map_err(|e| format!("{e:#}"))?;
    let recent = recent_failures(&at, now())?;
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
    if verify(answer.bytes(), &phc, &pepper) {
        let _ = at.remove(FAILS_NAME);
        return Ok(());
    }
    let count = record_failure(&at, now())
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
fn recent_failures(at: &KeyDir, now: u64) -> std::result::Result<Vec<u64>, String> {
    let text = match at.read(FAILS_NAME) {
        Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
        Ok(None) => return Ok(Vec::new()),
        Err(e) => return Err(format!("the failure count does not read: {e:#}")),
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

fn record_failure(at: &KeyDir, now: u64) -> std::io::Result<()> {
    let mut file = at.open_at(FAILS_NAME, libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT)?;
    writeln!(file, "{now}")?;
    file.sync_all()
}

fn utc(secs: u64) -> String {
    i64::try_from(secs)
        .ok()
        .and_then(|s| chrono::DateTime::<chrono::Utc>::from_timestamp(s, 0))
        .map_or_else(|| secs.to_string(), |t| t.format("%H:%M UTC").to_string())
}

/// Fill `out` with the hash of `pass` under `salt`. The working memory,
/// every block of it derived from `pass`, is zeroed before it is freed,
/// on an error too. The hash crate's own `hash_password` frees it as is.
fn derive(argon: &Argon2<'_>, pass: &[u8], salt: &[u8], out: &mut [u8]) -> Result<()> {
    let mut blocks = Zeroizing::new(vec![Block::default(); argon.params().block_count()]);
    argon
        .hash_password_into_with_memory(pass, salt, out, &mut blocks[..])
        .map_err(|e| anyhow::anyhow!("person-key: {e}"))
}

/// Fresh pepper from the OS.
///
/// # Errors
///
/// The OS gave no random bytes.
pub fn new_pepper() -> Result<Zeroizing<Vec<u8>>> {
    let mut pepper = Zeroizing::new(vec![0u8; PEPPER_LEN]);
    getrandom::fill(&mut pepper)
        .map_err(|e| anyhow::anyhow!("person-key: no pepper from the OS: {e}"))?;
    Ok(pepper)
}

/// The pepper kept at `path`. A missing file, a link, or a file of
/// another length is an error: the key does not verify without it.
///
/// # Errors
///
/// The pepper is missing, not a regular file, or not [`PEPPER_LEN`] bytes.
pub fn read_pepper(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let (dir, name) = split(path)?;
    match KeyDir::open(dir, false)? {
        Some(at) => pepper_named(&at, name),
        None => bail!("{}", pepper_gone(path)),
    }
}

fn pepper_gone(path: &Path) -> String {
    format!(
        "person-key: the pepper at {} is gone; run `ljos person-key set` again",
        path.display()
    )
}

fn pepper_in(at: &KeyDir) -> Result<Zeroizing<Vec<u8>>> {
    pepper_named(at, PEPPER_NAME)
}

fn pepper_named(at: &KeyDir, name: &str) -> Result<Zeroizing<Vec<u8>>> {
    let gone = || pepper_gone(&at.path.join(name));
    match at.read(name)? {
        Some(pepper) if pepper.len() == PEPPER_LEN => Ok(pepper),
        _ => bail!("{}", gone()),
    }
}

/// The PHC string of `pass` under a fresh salt from the OS, with
/// `pepper` as the argon2id secret.
///
/// # Errors
///
/// The OS gave no random bytes, or the hash failed.
pub fn hash(pass: &[u8], pepper: &[u8]) -> Result<String> {
    let err = |e: argon2::password_hash::Error| anyhow::anyhow!("person-key: {e}");
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| anyhow::anyhow!("person-key: no salt from the OS: {e}"))?;
    let salt = SaltString::encode_b64(&bytes).map_err(err)?;
    let params = Params::new(M_COST_KIB, T_COST, P_COST, None)
        .map_err(|e| anyhow::anyhow!("person-key: {e}"))?;
    let argon = Argon2::new_with_secret(pepper, Algorithm::Argon2id, Version::V0x13, params)
        .map_err(|e| anyhow::anyhow!("person-key: {e}"))?;
    let mut out = crate::memlock::Locked::zeroed(Params::DEFAULT_OUTPUT_LEN);
    derive(&argon, pass, &bytes, &mut out)?;
    let phc = PasswordHash {
        algorithm: Algorithm::Argon2id.ident(),
        version: Some(Version::V0x13.into()),
        params: ParamsString::try_from(argon.params()).map_err(err)?,
        salt: Some(salt.as_salt()),
        hash: Some(Output::new(&out[..]).map_err(err)?),
    };
    Ok(phc.to_string())
}

/// `phc` read as an argon2id hash with its salt and output, at no less
/// than the cost a new hash gets. Any other algorithm is not a key.
///
/// # Errors
///
/// The text is not a PHC string, names another algorithm, lacks a salt
/// or an output, or was made at a lower cost.
pub fn parse(phc: &str) -> Result<PasswordHash<'_>> {
    let parsed = PasswordHash::new(phc.trim()).map_err(|e| anyhow::anyhow!("person-key: {e}"))?;
    if parsed.algorithm != argon2::Algorithm::Argon2id.ident() {
        bail!("person-key: {} is not argon2id", parsed.algorithm);
    }
    if parsed.salt.is_none() || parsed.hash.is_none() {
        bail!("person-key: the PHC string has no salt or no hash");
    }
    let params = Params::try_from(&parsed).map_err(|e| anyhow::anyhow!("person-key: {e}"))?;
    if params.m_cost() < M_COST_KIB || params.t_cost() < T_COST || params.p_cost() < P_COST {
        bail!(
            "person-key: the hash was made at m={},t={},p={}, below the m={M_COST_KIB},\
             t={T_COST},p={P_COST} this ljos takes; run `ljos person-key set` again",
            params.m_cost(),
            params.t_cost(),
            params.p_cost()
        );
    }
    Ok(parsed)
}

/// Whether `pass` is the passphrase `phc` was made from with `pepper`.
/// The output is derived into locked memory and compared there, in
/// constant time. A hash that does not parse matches nothing.
#[must_use]
pub fn verify(pass: &[u8], phc: &str, pepper: &[u8]) -> bool {
    let Ok(parsed) = parse(phc) else {
        return false;
    };
    let (Some(salt), Some(want)) = (parsed.salt, parsed.hash) else {
        return false;
    };
    let version = parsed
        .version
        .map_or(Ok(Version::default()), Version::try_from);
    let (Ok(params), Ok(version)) = (Params::try_from(&parsed), version) else {
        return false;
    };
    let mut salt_bytes = [0u8; 64];
    let Ok(salt) = salt.decode_b64(&mut salt_bytes) else {
        return false;
    };
    let Ok(argon) = Argon2::new_with_secret(pepper, Algorithm::Argon2id, version, params) else {
        return false;
    };
    let mut out = crate::memlock::Locked::zeroed(want.len());
    derive(&argon, pass, salt, &mut out).is_ok() && same(&out, want.as_bytes())
}

/// Whether `a` and `b` are equal, in time that depends on their length
/// alone. `Output` would copy the derived bytes into memory that is
/// neither locked nor zeroed.
fn same(a: &[u8], b: &[u8]) -> bool {
    let diff = a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y));
    a.len() == b.len() && std::hint::black_box(diff) == 0
}

/// Keep the hash of `pass` at `path`, under a fresh pepper written
/// beside it first. A crash between the two leaves a key that does not
/// verify, and `set` run again mends it.
///
/// # Errors
///
/// No random bytes, the hash failed, or a file cannot be written.
pub fn keep(path: &Path, pass: &[u8]) -> Result<()> {
    let (dir, name) = split(path)?;
    let pepper = new_pepper()?;
    let phc = hash(pass, &pepper)?;
    let at = KeyDir::create(dir)?;
    at.write(PEPPER_NAME, &pepper)?;
    at.write(name, format!("{phc}\n").as_bytes())
}

/// Write `phc` to `path`, mode 0600. The text goes to a new file beside
/// it that is renamed over the old one, so a reader sees either.
///
/// # Errors
///
/// The directory or the file cannot be written.
pub fn store(path: &Path, phc: &str) -> Result<()> {
    let (dir, name) = split(path)?;
    KeyDir::create(dir)?.write(name, format!("{phc}\n").as_bytes())
}

/// The directory and file name of `path`.
fn split(path: &Path) -> Result<(&Path, &str)> {
    match (path.parent(), path.file_name().and_then(|n| n.to_str())) {
        (Some(dir), Some(name)) => Ok((dir, name)),
        _ => bail!(
            "person-key: {} names no file in a directory",
            path.display()
        ),
    }
}

/// The key directory, opened once. Each file in it is reached through
/// this descriptor and never by its path again, and none through a
/// link, so a file or the directory swapped for a symlink after the
/// open is not followed. The directories above it are followed.
struct KeyDir {
    fd: std::os::fd::OwnedFd,
    path: PathBuf,
}

fn c_name(name: &str) -> std::io::Result<std::ffi::CString> {
    std::ffi::CString::new(name).map_err(|_| std::io::ErrorKind::InvalidInput.into())
}

fn os_result(rc: libc::c_int) -> std::io::Result<libc::c_int> {
    if rc < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(rc)
    }
}

impl KeyDir {
    /// `dir`, or `None` when it does not exist. A symlink or anything
    /// but a directory in its place is an error.
    fn open(dir: &Path, create: bool) -> Result<Option<Self>> {
        use std::os::fd::{AsRawFd as _, FromRawFd as _};
        let ctx = || format!("person-key: {}", dir.display());
        let (parent, name) = split(dir)?;
        let up = match std::fs::File::open(parent) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !create => return Ok(None),
            up => up.with_context(ctx)?,
        };
        let name = c_name(name)?;
        if create {
            // SAFETY: a directory descriptor and a NUL-terminated name.
            let made = os_result(unsafe { libc::mkdirat(up.as_raw_fd(), name.as_ptr(), 0o700) });
            if let Err(e) = made {
                if e.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(e).with_context(ctx);
                }
            }
        }
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: as above; the flags refuse a link in place of the directory.
        match os_result(unsafe { libc::openat(up.as_raw_fd(), name.as_ptr(), flags) }) {
            // SAFETY: a fresh descriptor, owned here alone.
            Ok(fd) => Ok(Some(Self {
                fd: unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) },
                path: dir.to_path_buf(),
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !create => Ok(None),
            Err(e) if matches!(e.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => {
                bail!("person-key: {} is a link or not a directory", dir.display())
            }
            Err(e) => Err(e).with_context(ctx),
        }
    }

    /// `dir` opened, made mode 0700 first when missing, with the
    /// directories above it.
    fn create(dir: &Path) -> Result<Self> {
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("person-key: {}", parent.display()))?;
        }
        Self::open(dir, true)?.context("person-key: the key directory was not made")
    }

    /// `name` in the directory opened with `flags`, never through a link.
    /// A file it makes is mode 0600.
    fn open_at(&self, name: &str, flags: libc::c_int) -> std::io::Result<std::fs::File> {
        use std::os::fd::{AsRawFd as _, FromRawFd as _};
        let name = c_name(name)?;
        let flags = flags | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: a directory descriptor, a NUL-terminated name, and a mode.
        let fd = os_result(unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                flags,
                0o600 as libc::c_uint,
            )
        })?;
        // SAFETY: a fresh descriptor, owned here alone.
        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    }

    /// The bytes of `name`, or `None` when it is missing. A link, or
    /// anything but a regular file, is an error.
    fn read(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        use std::io::Read as _;
        let at = self.path.join(name);
        let not_file = || anyhow::anyhow!("person-key: {} is not a regular file", at.display());
        let mut file = match self.open_at(name, libc::O_RDONLY | libc::O_NONBLOCK) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Err(not_file()),
            Err(e) => return Err(e).with_context(|| format!("person-key: {}", at.display())),
        };
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(not_file());
        }
        let len = usize::try_from(meta.len()).unwrap_or(0);
        let mut bytes = Zeroizing::new(Vec::with_capacity(len + 1));
        file.read_to_end(&mut bytes)
            .with_context(|| format!("person-key: {}", at.display()))?;
        Ok(Some(bytes))
    }

    /// Write `bytes` to `name`, mode 0600. They go to a new file that is
    /// renamed over the old one, so a reader sees either.
    fn write(&self, name: &str, bytes: &[u8]) -> Result<()> {
        use std::os::fd::AsRawFd as _;
        let tmp = format!(".{name}.{}.tmp", std::process::id());
        let _ = self.remove(&tmp);
        let written = (|| -> std::io::Result<()> {
            let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL;
            let mut file = self.open_at(&tmp, flags)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            let (from, to) = (c_name(&tmp)?, c_name(name)?);
            let fd = self.fd.as_raw_fd();
            // SAFETY: one directory descriptor and two NUL-terminated names.
            os_result(unsafe { libc::renameat(fd, from.as_ptr(), fd, to.as_ptr()) })?;
            // SAFETY: the directory descriptor this owns.
            os_result(unsafe { libc::fsync(fd) }).map(drop)
        })();
        if let Err(e) = written {
            let _ = self.remove(&tmp);
            return Err(e)
                .with_context(|| format!("person-key: {}", self.path.join(name).display()));
        }
        Ok(())
    }

    /// Remove `name`, a link itself and never what it names.
    fn remove(&self, name: &str) -> std::io::Result<()> {
        use std::os::fd::AsRawFd as _;
        let name = c_name(name)?;
        // SAFETY: a directory descriptor and a NUL-terminated name.
        os_result(unsafe { libc::unlinkat(self.fd.as_raw_fd(), name.as_ptr(), 0) }).map(drop)
    }
}

/// The hash kept at `path`. `Ok(None)` is no key. A file that is not a
/// regular file, or not argon2id, is an error.
///
/// # Errors
///
/// The file cannot be read or does not hold a key.
pub fn read(path: &Path) -> Result<Option<String>> {
    let (dir, name) = split(path)?;
    match KeyDir::open(dir, false)? {
        Some(at) => key_in(&at, name),
        None => Ok(None),
    }
}

fn key_in(at: &KeyDir, name: &str) -> Result<Option<String>> {
    let path = at.path.join(name);
    let Some(bytes) = at.read(name)? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes)
        .with_context(|| format!("person-key: {} is not text", path.display()))?;
    parse(text).with_context(|| format!("person-key: {}", path.display()))?;
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
    keep(path, first.bytes())?;
    println!("person-key: set in {}", path.display());
    Ok(())
}

fn clear_at(path: &Path) -> Result<()> {
    person_only("clear")?;
    let (dir, name) = split(path)?;
    let Some(at) = KeyDir::open(dir, false)? else {
        println!("person-key: no key at {}", path.display());
        return Ok(());
    };
    let _ = at.remove(FAILS_NAME);
    match at.remove(name) {
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
    let meta = split(path).and_then(|(dir, name)| {
        let opened = KeyDir::open(dir, false)?.map(|at| {
            at.open_at(name, libc::O_RDONLY | libc::O_NONBLOCK)
                .and_then(|file| file.metadata())
        });
        Ok(opened.unwrap_or_else(|| Err(std::io::ErrorKind::NotFound.into())))
    });
    match meta {
        Err(e) => format!("person-key: {} does not read: {e:#}", path.display()),
        Ok(Ok(meta)) => {
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
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => format!(
            "person-key: not set ({}); run `ljos person-key set` from your own terminal",
            path.display()
        ),
        Ok(Err(e)) => format!("person-key: {} does not read: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEPPER: &[u8] = &[9u8; PEPPER_LEN];

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
        keep(&kept, b"correct horse battery").unwrap();
        let phc = read(&kept).unwrap().unwrap();
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
        let pepper = read_pepper(&path.with_file_name(PEPPER_NAME)).unwrap();
        assert!(verify(b"correct horse battery", &phc, &pepper));
    }

    #[test]
    fn set_refuses_a_short_or_mismatched_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_NAME);
        for answers in [
            vec![("New person passphrase: ", "short one")],
            vec![("New person passphrase: ", "fifteen bytes!!")],
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
            dir,
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
        keep(&dir.path().join(KEY_NAME), b"correct horse battery").unwrap();
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
        let mut names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        names.sort();
        assert_eq!(names, [KEY_NAME, PEPPER_NAME], "a token was kept");
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
            dir.path(),
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
            dir.path(),
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
        keep(&path, b"correct horse battery").unwrap();
        let phc = read(&path).unwrap().unwrap();
        let said = status_at(&path);
        assert!(said.contains("person-key: set, written "), "{said}");
        assert!(!said.contains("argon2"), "{said}");
        assert!(!said.contains(phc.rsplit('$').next().unwrap()), "{said}");
    }

    #[test]
    fn a_hash_matches_its_passphrase_and_pepper_only() {
        let phc = hash(b"correct horse battery", PEPPER).unwrap();
        assert!(phc.starts_with("$argon2id$"), "{phc}");
        assert!(verify(b"correct horse battery", &phc, PEPPER));
        assert!(!verify(b"correct horse batterz", &phc, PEPPER));
        assert!(!verify(b"", &phc, PEPPER));
        assert!(!verify(b"correct horse battery", &phc, &[8u8; PEPPER_LEN]));
        assert!(!verify(b"correct horse battery", &phc, &[]));
        let again = hash(b"correct horse battery", PEPPER).unwrap();
        assert_ne!(phc, again, "two hashes shared a salt");
    }

    /// `keep` writes a fresh pepper beside the key, mode 0600, and each
    /// call writes another. Without the pepper the key is refused before
    /// the prompt, and that is not counted as a wrong answer.
    #[test]
    fn the_pepper_is_kept_beside_the_key_and_needed() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = keyed();
        let at = dir.path().join(PEPPER_NAME);
        let first = read_pepper(&at).unwrap();
        assert_eq!(first.len(), PEPPER_LEN);
        let mode = std::fs::metadata(&at).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let phc = read(&dir.path().join(KEY_NAME)).unwrap().unwrap();
        assert!(verify(b"correct horse battery", &phc, &first));
        keep(&dir.path().join(KEY_NAME), b"correct horse battery").unwrap();
        assert_ne!(read_pepper(&at).unwrap(), first, "the pepper was reused");
        std::fs::remove_file(&at).unwrap();
        let asked = ask_at(dir.path(), Some("correct horse battery"), 1000);
        let why = asked.out.unwrap_err();
        assert!(why.contains("run `ljos person-key set` again"), "{why}");
        assert!(asked.shown.is_empty(), "{}", asked.shown);
        assert!(!dir.path().join(FAILS_NAME).exists());
        std::fs::write(&at, [1u8; PEPPER_LEN - 1]).unwrap();
        assert!(read_pepper(&at).is_err());
    }

    /// The hash made in zeroed memory is the one the hash crate makes, so
    /// a key kept before verifies the same way.
    #[test]
    fn a_hash_from_the_hash_crate_verifies() {
        use argon2::password_hash::PasswordHasher as _;
        let salt = SaltString::encode_b64(&[7u8; 16]).unwrap();
        let params = Params::new(M_COST_KIB, T_COST, P_COST, None).unwrap();
        let theirs = Argon2::new_with_secret(PEPPER, Algorithm::Argon2id, Version::V0x13, params)
            .unwrap()
            .hash_password(b"correct horse battery", &salt)
            .unwrap()
            .to_string();
        assert!(verify(b"correct horse battery", &theirs, PEPPER));
        assert!(!verify(b"correct horse batterz", &theirs, PEPPER));
    }

    /// A new hash names the full cost. One made at the hash crate's
    /// default, which an earlier ljos kept, is not a key, and the error
    /// says to set the passphrase again.
    #[test]
    fn a_hash_below_the_cost_is_not_a_key() {
        use argon2::password_hash::PasswordHasher as _;
        let phc = hash(b"correct horse battery", PEPPER).unwrap();
        let params = Params::try_from(&parse(&phc).unwrap()).unwrap();
        assert_eq!(
            (params.m_cost(), params.t_cost(), params.p_cost()),
            (64 * 1024, 3, 4)
        );
        let salt = SaltString::encode_b64(&[7u8; 16]).unwrap();
        let old = Argon2::default()
            .hash_password(b"correct horse battery", &salt)
            .unwrap()
            .to_string();
        let why = format!("{:#}", parse(&old).unwrap_err());
        assert!(why.contains("run `ljos person-key set` again"), "{why}");
        assert!(!verify(b"correct horse battery", &old, PEPPER));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_NAME);
        std::fs::write(&path, format!("{old}\n")).unwrap();
        assert!(read(&path).is_err());
    }

    /// A line as long as the terminal reader takes is read into the
    /// buffer it started in, so no moved copy of it is left unzeroed. A
    /// longer line is refused, and its buffer dropped and zeroed.
    #[test]
    fn a_long_line_stays_in_the_buffer_it_started_in() {
        use crate::person_tty::MAX_LINE;
        let long: &'static str = "a".repeat(MAX_LINE).leak();
        for typed in ["x", long] {
            let (master, mut slave) = pty_pair();
            let shown = typist(master, vec![("Line: ", typed)]);
            let line = crate::person_tty::ask(&mut slave, "Line: ")
                .unwrap()
                .unwrap();
            assert_eq!(line.bytes(), typed.as_bytes());
            assert_eq!(line.capacity(), MAX_LINE, "{} bytes", typed.len());
            drop(slave);
            shown.join().unwrap();
        }
        let longer: &'static str = "a".repeat(MAX_LINE + 1).leak();
        let (master, mut slave) = pty_pair();
        let shown = typist(master, vec![("Line: ", longer)]);
        assert!(crate::person_tty::ask(&mut slave, "Line: ").is_err());
        drop(slave);
        shown.join().unwrap();
    }

    #[test]
    fn only_an_argon2id_phc_string_parses() {
        let phc = hash(b"correct horse battery", PEPPER).unwrap();
        let parsed = parse(&phc).unwrap();
        assert_eq!(parsed.algorithm.as_str(), "argon2id");
        assert!(parsed.salt.is_some());
        let argon2i = phc.replacen("$argon2id$", "$argon2i$", 1);
        assert!(parse(&argon2i).is_err());
        assert!(!verify(b"correct horse battery", &argon2i, PEPPER));
        for junk in [
            "",
            "correct horse battery",
            "$argon2id$",
            "$argon2id$v=19$m=1",
        ] {
            assert!(parse(junk).is_err(), "{junk:?}");
            assert!(!verify(b"correct horse battery", junk, PEPPER));
        }
    }

    #[test]
    fn the_stored_key_is_mode_0600_and_reads_back() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ljos").join(KEY_NAME);
        assert!(read(&path).unwrap().is_none());
        let phc = hash(b"correct horse battery", PEPPER).unwrap();
        store(&path, &phc).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(read(&path).unwrap().as_deref(), Some(phc.as_str()));
        let next = hash(b"another long phrase", PEPPER).unwrap();
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
        store(&real, &hash(b"correct horse battery", PEPPER).unwrap()).unwrap();
        let path = dir.path().join(KEY_NAME);
        std::os::unix::fs::symlink(&real, &path).unwrap();
        assert!(read(&path).is_err());
    }

    /// The key directory is opened once. Swapped for a symlink to another
    /// key after that, it is not followed: the key and pepper read, and
    /// the failure count read and written, are the ones the open found.
    /// Opened again, the link is refused, and so is a pepper swapped for
    /// a link.
    #[test]
    fn a_link_swapped_in_after_the_open_is_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("ljos");
        let evil = root.path().join("evil");
        let moved = root.path().join("moved");
        keep(&dir.join(KEY_NAME), b"correct horse battery").unwrap();
        keep(&evil.join(KEY_NAME), b"a runner's own phrase").unwrap();
        let phc = read(&dir.join(KEY_NAME)).unwrap().unwrap();
        let swap = || {
            std::fs::rename(&dir, &moved).unwrap();
            std::os::unix::fs::symlink(&evil, &dir).unwrap();
        };
        let unswap = || {
            std::fs::remove_file(&dir).unwrap();
            std::fs::rename(&moved, &dir).unwrap();
        };

        let at = KeyDir::open(&dir, false).unwrap().unwrap();
        swap();
        assert_eq!(
            key_in(&at, KEY_NAME).unwrap().as_deref(),
            Some(phc.as_str())
        );
        let pepper = pepper_in(&at).unwrap();
        assert!(verify(b"correct horse battery", &phc, &pepper));
        let why = format!("{:#}", KeyDir::open(&dir, false).err().unwrap());
        assert!(why.contains("is a link"), "{why}");
        assert!(read(&dir.join(KEY_NAME)).is_err());
        assert!(read_pepper(&dir.join(PEPPER_NAME)).is_err());
        let refused = ask_at(&dir, Some("a runner's own phrase"), 1000);
        assert!(refused.out.unwrap_err().contains("is a link"));
        assert!(refused.shown.is_empty(), "{}", refused.shown);
        unswap();

        let (master, slave) = pty_pair();
        let shown = typist(master, vec![(PROMPT, "a runner's own phrase")]);
        let swapped = std::cell::Cell::new(false);
        let out = ask_with(
            &dir,
            Some(slave),
            &["CURSOR_AGENT".to_string()],
            "git push",
            || {
                if !swapped.replace(true) {
                    swap();
                }
                1000
            },
            |_| {},
        );
        shown.join().unwrap();
        assert_eq!(out, Err("the passphrase did not match".into()));
        assert_eq!(
            std::fs::read_to_string(moved.join(FAILS_NAME)).unwrap(),
            "1000\n"
        );
        assert!(!evil.join(FAILS_NAME).exists(), "the link was followed");
        unswap();

        std::fs::remove_file(dir.join(PEPPER_NAME)).unwrap();
        std::os::unix::fs::symlink(evil.join(PEPPER_NAME), dir.join(PEPPER_NAME)).unwrap();
        let why = format!("{:#}", read_pepper(&dir.join(PEPPER_NAME)).unwrap_err());
        assert!(why.contains("not a regular file"), "{why}");
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
