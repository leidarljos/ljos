//! The one cloud rule, read from the words each command runs.
//! A ref write may name only the seat's branches and `develop`. Anything
//! else, and any force or delete, is refused. The source side of a
//! refspec, a remote URL, an option's value and the body of a comment
//! are never what is refused.

use std::cell::OnceCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// git's own options before the subcommand that take the next word.
const GIT_VALUED: &[&str] = &[
    "-C",
    "-c",
    "--attr-source",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
];

/// Substitutions that print the branch checked out, so pushing one is
/// pushing `HEAD`.
const CURRENT_BRANCH: &[&str] = &[
    "$(git branch --show-current)",
    "$(git rev-parse --abbrev-ref HEAD)",
    "$(git symbolic-ref --short HEAD)",
    "`git branch --show-current`",
    "`git rev-parse --abbrev-ref HEAD`",
    "`git symbolic-ref --short HEAD`",
];

const UNREAD: &str = "this push goes where the checkout says, and the checkout could not be read";

/// Why a destination is not one this seat may update.
const ALLOW_RULE: &str =
    "a cloud seat may push only its named branches and develop, and may not force or delete a ref";

/// Branches this seat may update: `develop`, plus each branch named in the
/// seat's `ljos.toml` under `[cloud] branches`. `Err` when that file is
/// present and cannot be read, so a ref write fails closed.
pub(crate) fn seat_allowlist() -> Result<HashSet<String>, String> {
    let mut allow = HashSet::new();
    allow.insert("develop".to_string());
    let Some(path) = seat_branch_file() else {
        return Ok(allow);
    };
    if !path.is_file() {
        return Ok(allow);
    }
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "the seat config {} could not be read; {ALLOW_RULE}",
            path.display()
        )
    })?;
    if text.trim().is_empty() {
        return Ok(allow);
    }
    let value: toml::Value = toml::from_str(&text).map_err(|_| {
        format!(
            "the seat config {} does not parse; {ALLOW_RULE}",
            path.display()
        )
    })?;
    let Some(root) = value.as_table() else {
        return Err(format!(
            "the seat config {} is not a table; {ALLOW_RULE}",
            path.display()
        ));
    };
    let branches = root
        .get("cloud")
        .and_then(toml::Value::as_table)
        .and_then(|cloud| cloud.get("branches"))
        .or_else(|| root.get("branches"));
    let Some(branches) = branches else {
        return Ok(allow);
    };
    let Some(items) = branches.as_array() else {
        return Err(format!(
            "the seat config {} key `branches` must be a list of strings; {ALLOW_RULE}",
            path.display()
        ));
    };
    for item in items {
        let Some(name) = item.as_str() else {
            return Err(format!(
                "the seat config {} key `branches` must be a list of strings; {ALLOW_RULE}",
                path.display()
            ));
        };
        let name = short_branch(name);
        if !name.is_empty() {
            allow.insert(name);
        }
    }
    Ok(allow)
}

/// `~/.config/ljos/seats/$LJOS_SEAT/ljos.toml`, or the same file under
/// `$XDG_CONFIG_HOME`. `LJOS_SEAT` is the process environment, then the
/// env file `ljos cloud up` writes.
fn seat_branch_file() -> Option<PathBuf> {
    let base = crate::config_base()?;
    let seat = std::env::var("LJOS_SEAT")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| seat_from_env_file(&base))?;
    let seat = seat.trim();
    if seat.is_empty() || seat.contains('/') || seat.contains('\\') || seat.contains("..") {
        return None;
    }
    Some(base.join("ljos").join("seats").join(seat).join("ljos.toml"))
}

fn seat_from_env_file(base: &Path) -> Option<String> {
    let text = std::fs::read_to_string(base.join("ljos").join("env")).ok()?;
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "LJOS_SEAT" {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// The branch `name` names, with `refs/heads/` or `heads/` taken off.
/// A tag or any other ref is left unchanged, so it is not a branch name.
fn short_branch(name: &str) -> String {
    let name = name.trim();
    name.strip_prefix("refs/heads/")
        .or_else(|| name.strip_prefix("heads/"))
        .unwrap_or(name)
        .to_string()
}

/// Whether `name` is a branch this seat may update. A tag or a ref outside
/// `refs/heads/` is not.
fn branch_permitted(name: &str) -> Result<bool, String> {
    let allow = seat_allowlist()?;
    if is_tag_ref(name) || is_other_ref(name) {
        return Ok(false);
    }
    Ok(allow.contains(&short_branch(name)))
}

fn is_tag_ref(name: &str) -> bool {
    let name = name.trim();
    name.starts_with("refs/tags/") || name.starts_with("tags/")
}

fn is_other_ref(name: &str) -> bool {
    let name = name.trim();
    name.starts_with("refs/") && !name.starts_with("refs/heads/")
}

/// Why `name` may not be the destination of a ref write, or `None` when
/// it may. The reason states the allowlist rule.
fn dest_refused(name: &str) -> Option<String> {
    match branch_permitted(name) {
        Err(why) => Some(why),
        Ok(true) => None,
        Ok(false) => Some(format!("this push updates {name}; {ALLOW_RULE}")),
    }
}

/// Why a `git push` in `line` would update a ref this seat may not write,
/// or `None` when none would. `cwd` is where the line runs, the process
/// directory when absent; a `cd DIR` before the push and `git -C DIR` move
/// it. A `$NAME` bound by an earlier assignment on the same line is the
/// destination the shell pushes, so that name is what is judged.
pub(crate) fn cursor_push(line: &str, cwd: Option<&Path>) -> Option<String> {
    judge(line, cwd).push
}

/// Names a bare assignment binds, in order. A `NAME=value` prefix on the
/// command itself is that command's environment, not a shell assignment,
/// so it does not name a later `$NAME` on the same command.
fn bound_names(names: &mut Vec<(String, String)>, seg: &str) {
    let words = crate::shell_words(seg);
    let bare = if words.first().is_some_and(|w| w == "export") {
        &words[1..]
    } else {
        &words[..]
    };
    for w in bare {
        if let Some((k, v)) = w.split_once('=') {
            names.push((k.to_string(), v.to_string()));
        }
    }
}

/// Which pull-request actions `line` runs through `gh`. `open` is create,
/// its alias `new`, ready, reopen, or an API call that does one of those.
/// `merge` is `gh pr merge` or an API call that merges or turns auto-merge
/// on. A comment, a list, or a GET is neither.
#[derive(Clone, Copy, Default)]
pub(crate) struct GhActs {
    pub open: bool,
    pub merge: bool,
}

impl GhActs {
    fn either(self, other: Self) -> Self {
        Self {
            open: self.open || other.open,
            merge: self.merge || other.merge,
        }
    }

    fn any(self) -> bool {
        self.open || self.merge
    }
}

/// What one walk of a line found. `push` is a ref write the seat may not
/// make. `saw_ref` is a ref-writing command the walk did read, allowed or
/// not, so a script that only pushes `develop` is not failed closed for
/// mentioning `git push`.
#[derive(Clone, Default)]
struct Verdict {
    push: Option<String>,
    acts: GhActs,
    saw_ref: bool,
}

impl Verdict {
    fn either(mut self, other: Self) -> Self {
        if self.push.is_none() {
            self.push = other.push;
        }
        self.acts = self.acts.either(other.acts);
        self.saw_ref |= other.saw_ref;
        self
    }

    fn closed(why: impl Into<String>) -> Self {
        Self {
            push: Some(why.into()),
            saw_ref: true,
            ..Self::default()
        }
    }
}

/// Aliases and config already seen on the way into this line. A `!` script
/// is judged by the same walk, with these carried in.
#[derive(Clone, Default)]
struct WalkIn {
    depth: usize,
    tainted: bool,
    gh_unreadable: bool,
    git_aliases: Vec<(String, String)>,
    gh_aliases: Vec<(String, String)>,
}

fn judge(line: &str, cwd: Option<&Path>) -> Verdict {
    judge_depth(line, cwd, &WalkIn::default())
}

fn judge_depth(line: &str, cwd: Option<&Path>, incoming: &WalkIn) -> Verdict {
    if incoming.depth > ALIAS_DEPTH {
        return Verdict {
            push: Some(format!("alias expansion could not be read; {ALLOW_RULE}")),
            acts: GhActs {
                open: true,
                merge: true,
            },
            saw_ref: true,
        };
    }
    let mut here = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());
    let mut git_aliases = incoming.git_aliases.clone();
    let gh_aliases = incoming.gh_aliases.clone();
    let mut tainted = incoming.tainted;
    let gh_unreadable = incoming.gh_unreadable;
    let mut verdict = Verdict::default();
    for l in crate::nested_lines(line, 0) {
        let mut names: Vec<(String, String)> = Vec::new();
        let piped = piped_segments(&l);
        for seg in crate::raw_segments(&l) {
            let fed = piped.iter().any(|p| p == seg.trim()) || stdin_redirected(&seg);
            // An assignment on its own is not a git command, and it is
            // skipped below, but `export GIT_CONFIG_KEY_0=core.hooksPath`
            // still disarms the hook for every later push.
            if let Some(why) = hook_disarm(&seg, here.as_deref()) {
                verdict.saw_ref = true;
                if verdict.push.is_none() {
                    verdict.push = Some(why);
                }
            }
            if crate::assignment_segment(&seg) {
                bound_names(&mut names, &seg);
                continue;
            }
            let ifs = names
                .iter()
                .rev()
                .find(|(k, _)| k == "IFS")
                .map(|(_, v)| v.clone());
            let expanded = crate::expand_names(&seg, &names, ifs.as_deref());
            let seg = expanded.as_deref().unwrap_or(&seg);
            let words = argv(seg);
            let Some(cmd) = words.first().map(String::as_str) else {
                continue;
            };
            let run = prefix_context(seg, here.clone());
            if cmd == "bash" {
                if let Some(path) = env_value(&run.env, &names, "BASH_ENV") {
                    let child = walk_child(
                        incoming.depth,
                        tainted,
                        gh_unreadable,
                        &git_aliases,
                        &gh_aliases,
                    );
                    verdict = verdict.either(script_verdict(&path, here.as_deref(), &child));
                }
            }
            match cmd {
                "cd" | "pushd" => here = moved(here, &words[1..]),
                "popd" => here = None,
                "git" => {
                    if let Some(alias) = git_alias_write(&words[1..]) {
                        git_aliases.push(alias);
                    }
                    if config_affects_push(&words[1..]) {
                        tainted = true;
                    }
                    let Some(outcome) =
                        git_push(&words[1..], run.dir, &git_aliases, &run.env, run.via_xargs)
                    else {
                        continue;
                    };
                    match outcome {
                        GitOutcome::Shell(script) => {
                            let child = walk_child(
                                incoming.depth,
                                tainted,
                                gh_unreadable,
                                &git_aliases,
                                &gh_aliases,
                            );
                            verdict = verdict.either(judge_depth(&script, here.as_deref(), &child));
                        }
                        GitOutcome::Push(push) => {
                            verdict.saw_ref = true;
                            if push.dry_run {
                                continue;
                            }
                            let why = if tainted {
                                Some(format!(
                                    "a git config on this line changes where a push writes; {ALLOW_RULE}"
                                ))
                            } else {
                                push.cursor_ref()
                            };
                            if let Some(why) = why {
                                if verdict.push.is_none() {
                                    verdict.push = Some(why);
                                }
                            }
                        }
                    }
                }
                "source" | "." | "bash" | "sh" | "dash" | "zsh" | "ksh" => {
                    if let Some(file) = shell_script_file(&words) {
                        let child = walk_child(
                            incoming.depth,
                            tainted,
                            gh_unreadable,
                            &git_aliases,
                            &gh_aliases,
                        );
                        verdict = verdict.either(script_verdict(&file, here.as_deref(), &child));
                    }
                }
                _ => {
                    let _ = (cmd, fed);
                }
            }
        }
    }
    verdict
}

/// Why `seg` disarms the pre-push hook, or `None` when it does not.
///
/// The hook is the destination check. `--no-verify`, a `core.hooksPath`
/// override, and a write to the hook file or the gitconfig that names it
/// would let the next push skip that check. `GIT_CONFIG_COUNT` is not one
/// of these: a push refspec in that env is judged as a destination.
fn hook_disarm(seg: &str, cwd: Option<&Path>) -> Option<String> {
    let words = crate::shell_words(seg);
    if words.is_empty() {
        return None;
    }
    if let Some(why) = redirect_disarm(&words, cwd) {
        return Some(why);
    }
    let (at, _) = crate::prefix_end(&words);
    let cmd = words
        .get(at)
        .map(|word| word.rsplit('/').next().unwrap_or(word));
    if let Some(why) = file_tool_disarm(cmd, &words[at..], cwd) {
        return Some(why);
    }
    if let Some(why) = protected_path_write(cmd, &words[at..], cwd) {
        return Some(why);
    }
    let shell_wide = cmd == Some("export") || at == words.len();
    let git = cmd == Some("git");
    // A prefix assignment applies to this command. `export`, and a segment
    // that is only assignments, applies to every later command.
    if shell_wide || git {
        if let Some(why) = config_env_disarm(&words[..at]) {
            return Some(why);
        }
    }
    if cmd == Some("export") {
        if let Some(why) = config_env_disarm(&words[at + 1..]) {
            return Some(why);
        }
    }
    if git {
        return git_hook_disarm(&words[at..], cwd);
    }
    None
}

fn protected_path_write(cmd: Option<&str>, words: &[String], cwd: Option<&Path>) -> Option<String> {
    let cmd = cmd?;
    if command_is_pure_read(cmd, words) || (cmd == "git" && git_invocation_is_read(words)) {
        return None;
    }
    let hit = words
        .iter()
        .find(|word| crate::cloud_hook::word_hits_protected(word, cwd))?;
    Some(disarm_why(&format!(
        "`{cmd}` writes {hit}, which can replace the pre-push hook or the config that names it"
    )))
}

fn command_is_pure_read(cmd: &str, words: &[String]) -> bool {
    let cmd = cmd.rsplit('/').next().unwrap_or(cmd);
    if matches!(
        cmd,
        "cat"
            | "head"
            | "tail"
            | "less"
            | "more"
            | "wc"
            | "stat"
            | "ls"
            | "file"
            | "grep"
            | "rg"
            | "diff"
            | "cmp"
            | "od"
            | "hexdump"
            | "strings"
            | "echo"
            | "printf"
            | "true"
            | "false"
            | "export"
    ) {
        return true;
    }
    if matches!(cmd, "sed" | "perl" | "awk" | "ruby") {
        return !inplace_edit(cmd, words);
    }
    false
}

fn inplace_edit(cmd: &str, words: &[String]) -> bool {
    words.iter().any(|word| {
        if word == "--in-place" || word.starts_with("--in-place=") {
            return true;
        }
        if cmd == "perl" {
            if let Some(cluster) = word.strip_prefix('-') {
                if !cluster.is_empty() && !word.starts_with("--") && cluster.contains('i') {
                    return true;
                }
            }
            return false;
        }
        word == "-i" || (word.starts_with("-i") && !word.starts_with("--"))
    })
}

fn git_invocation_is_read(words: &[String]) -> bool {
    let mut i = 1usize;
    while i < words.len() {
        let word = words[i].as_str();
        if GIT_VALUED.contains(&word) {
            i += 2;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        break;
    }
    let sub = words.get(i).map(String::as_str).unwrap_or("");
    if matches!(
        sub,
        "status"
            | "log"
            | "diff"
            | "show"
            | "rev-parse"
            | "grep"
            | "blame"
            | "ls-files"
            | "cat-file"
            | "rev-list"
            | "merge-base"
            | "symbolic-ref"
            | "describe"
            | "fetch"
            | "version"
            | "help"
    ) {
        return true;
    }
    sub == "config" && git_config_is_read(&words[i + 1..])
}

fn git_config_is_read(args: &[String]) -> bool {
    let mut positional = 0usize;
    let mut i = 0usize;
    while i < args.len() {
        let word = args[i].as_str();
        if matches!(
            word,
            "--get"
                | "--get-all"
                | "--get-regexp"
                | "--get-urlmatch"
                | "--list"
                | "-l"
                | "--show-origin"
                | "--show-scope"
                | "--get-color"
                | "--get-colorbool"
                | "--global"
                | "--system"
                | "--local"
        ) {
            i += 1;
            continue;
        }
        if matches!(
            word,
            "--file" | "-f" | "--blob" | "--default" | "--comment" | "--type" | "--name" | "--id"
        ) {
            i += 2;
            continue;
        }
        if matches!(
            word,
            "--unset"
                | "--unset-all"
                | "--replace-all"
                | "--add"
                | "--edit"
                | "--remove-section"
                | "--rename-section"
        ) {
            return false;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        if word.contains('=') {
            return false;
        }
        positional += 1;
        i += 1;
    }
    positional < 2
}

fn disarm_why(what: &str) -> String {
    format!("{what}; {ALLOW_RULE}")
}

fn redirect_disarm(words: &[String], cwd: Option<&Path>) -> Option<String> {
    for (i, word) in words.iter().enumerate() {
        if word != ">" {
            continue;
        }
        let Some(target) = words.get(i + 1) else {
            continue;
        };
        if crate::cloud_hook::names_protected(target, cwd) {
            return Some(disarm_why(&format!(
                "a redirect writes {target}, which can replace the pre-push hook or the config that names it"
            )));
        }
    }
    None
}

fn file_tool_disarm(cmd: Option<&str>, words: &[String], cwd: Option<&Path>) -> Option<String> {
    let cmd = cmd?;
    let each = matches!(cmd, "rm" | "unlink" | "truncate" | "chmod" | "tee");
    let dest = matches!(cmd, "mv" | "cp" | "install" | "ln");
    if !each && !dest {
        return None;
    }
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 1;
    while i < words.len() {
        let word = words[i].as_str();
        if word == "--" {
            positionals.extend(words[i + 1..].iter().cloned());
            break;
        }
        if matches!(word, "-t" | "--target-directory" | "--target") {
            if let Some(dir) = words.get(i + 1) {
                if crate::cloud_hook::names_protected(dir, cwd) {
                    return Some(disarm_why(&format!(
                        "`{cmd}` writes {dir}, which can replace the pre-push hook or the config that names it"
                    )));
                }
            }
            i += 2;
            continue;
        }
        if let Some(dir) = word
            .strip_prefix("--target-directory=")
            .or_else(|| word.strip_prefix("--target="))
        {
            if crate::cloud_hook::names_protected(dir, cwd) {
                return Some(disarm_why(&format!(
                    "`{cmd}` writes {dir}, which can replace the pre-push hook or the config that names it"
                )));
            }
            i += 1;
            continue;
        }
        if valued_file_flag(cmd, word) {
            i += 2;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        positionals.push(word.to_string());
        i += 1;
    }
    let hit = if dest {
        positionals.last().cloned()
    } else {
        positionals
            .iter()
            .find(|path| crate::cloud_hook::names_protected(path, cwd))
            .cloned()
    };
    let hit = hit.filter(|path| crate::cloud_hook::names_protected(path, cwd))?;
    Some(disarm_why(&format!(
        "`{cmd}` writes {hit}, which can replace the pre-push hook or the config that names it"
    )))
}

fn valued_file_flag(cmd: &str, word: &str) -> bool {
    match cmd {
        "install" => matches!(
            word,
            "-m" | "-o"
                | "-g"
                | "-S"
                | "--mode"
                | "--owner"
                | "--group"
                | "--suffix"
                | "--strip-program"
                | "--compare"
        ),
        "chmod" => matches!(word, "--reference"),
        "cp" | "mv" | "ln" => matches!(
            word,
            "--suffix" | "-S" | "--backup" | "--context" | "--no-target-directory"
        ),
        "truncate" => matches!(word, "-s" | "--size" | "-r" | "--reference"),
        "tee" => false,
        _ => false,
    }
}

/// `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` replace the file that names
/// the hook. `GIT_CONFIG_KEY_*` and `GIT_CONFIG_PARAMETERS` can set
/// `core.hooksPath` without a `-c`. The ambient values are not read: a
/// test isolates git by pointing those variables at an empty file.
fn assign_key(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with('-')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn config_env_disarm(words: &[String]) -> Option<String> {
    for word in words {
        let Some((key, value)) = word.split_once('=') else {
            continue;
        };
        if !assign_key(key) {
            continue;
        }
        if key == "GIT_CONFIG_GLOBAL" || key == "GIT_CONFIG_SYSTEM" {
            return Some(disarm_why(&format!(
                "`{key}` replaces the config that names the pre-push hook"
            )));
        }
        if key.starts_with("GIT_CONFIG_KEY_") && value.eq_ignore_ascii_case("core.hooksPath") {
            return Some(disarm_why(
                "`GIT_CONFIG_KEY_*` sets core.hooksPath and skips the pre-push hook",
            ));
        }
        if key == "GIT_CONFIG_PARAMETERS" && value.to_ascii_lowercase().contains("core.hookspath") {
            return Some(disarm_why(
                "`GIT_CONFIG_PARAMETERS` sets core.hooksPath and skips the pre-push hook",
            ));
        }
    }
    None
}

fn git_hook_disarm(words: &[String], cwd: Option<&Path>) -> Option<String> {
    // `--no-verify` is a push option, so it follows the subcommand.
    // `-c core.hooksPath` is refused wherever it sits on the git command.
    if let Some(why) = git_hook_switches(words) {
        return Some(why);
    }
    let mut i = 1;
    while i < words.len() {
        let word = words[i].as_str();
        if word == "--no-verify" || word.starts_with("--no-verify=") {
            return Some(disarm_why("`git --no-verify` skips the pre-push hook"));
        }
        if word == "-c" || word == "--config-env" {
            let value = words.get(i + 1).map(String::as_str).unwrap_or("");
            if hooks_path_setting(value) {
                return Some(disarm_why(
                    "`git -c` names core.hooksPath and skips the pre-push hook",
                ));
            }
            i += 2;
            continue;
        }
        if let Some(value) = word.strip_prefix("--config-env=") {
            if hooks_path_setting(value) {
                return Some(disarm_why(
                    "`git -c` names core.hooksPath and skips the pre-push hook",
                ));
            }
            i += 1;
            continue;
        }
        if let Some(value) = word.strip_prefix("-c") {
            if !value.is_empty() && !word.starts_with("--") && hooks_path_setting(value) {
                return Some(disarm_why(
                    "`git -c` names core.hooksPath and skips the pre-push hook",
                ));
            }
        }
        if GIT_VALUED.contains(&word) {
            i += 2;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        break;
    }
    if words.get(i).map(String::as_str) == Some("config") {
        return git_config_hooks_disarm(words, &words[i + 1..], cwd);
    }
    None
}

fn git_hook_switches(words: &[String]) -> Option<String> {
    let mut i = 1;
    while i < words.len() {
        let word = words[i].as_str();
        if word == "--no-verify" || word.starts_with("--no-verify=") {
            return Some(disarm_why("`git --no-verify` skips the pre-push hook"));
        }
        if word == "-c" || word == "--config-env" || word == "--config" {
            let value = words.get(i + 1).map(String::as_str).unwrap_or("");
            if hooks_path_setting(value) {
                return Some(disarm_why(
                    "`git -c` names core.hooksPath and skips the pre-push hook",
                ));
            }
            i += 2;
            continue;
        }
        if let Some(value) = word.strip_prefix("--config=") {
            if hooks_path_setting(value) {
                return Some(disarm_why(
                    "`git -c` names core.hooksPath and skips the pre-push hook",
                ));
            }
        }
        if let Some(value) = word.strip_prefix("--config-env=") {
            if hooks_path_setting(value) {
                return Some(disarm_why(
                    "`git -c` names core.hooksPath and skips the pre-push hook",
                ));
            }
        }
        if let Some(value) = word.strip_prefix("-c") {
            if !value.is_empty() && !word.starts_with("--") && hooks_path_setting(value) {
                return Some(disarm_why(
                    "`git -c` names core.hooksPath and skips the pre-push hook",
                ));
            }
        }
        i += 1;
    }
    None
}

fn hooks_path_setting(value: &str) -> bool {
    let key = value.split_once('=').map(|(key, _)| key).unwrap_or(value);
    key.eq_ignore_ascii_case("core.hooksPath")
}

fn git_config_hooks_disarm(
    git_words: &[String],
    args: &[String],
    cwd: Option<&Path>,
) -> Option<String> {
    let mut i = 0;
    let mut read = false;
    let mut write = false;
    let mut edit = false;
    let mut global = false;
    let mut file: Option<String> = None;
    let mut section: Option<String> = None;
    let mut positional: Vec<&str> = Vec::new();
    while i < args.len() {
        let word = args[i].as_str();
        if matches!(word, "--global" | "--system") {
            global = true;
            i += 1;
            continue;
        }
        if word == "--local" {
            i += 1;
            continue;
        }
        if matches!(
            word,
            "--get"
                | "--get-all"
                | "--get-regexp"
                | "--get-urlmatch"
                | "--list"
                | "-l"
                | "--show-origin"
                | "--show-scope"
                | "--get-color"
                | "--get-colorbool"
        ) {
            read = true;
            i += 1;
            continue;
        }
        if matches!(word, "--unset" | "--unset-all" | "--replace-all" | "--add") {
            write = true;
            i += 1;
            continue;
        }
        if word == "--edit" {
            edit = true;
            i += 1;
            continue;
        }
        if word == "--remove-section" || word == "--rename-section" {
            section = args.get(i + 1).cloned();
            i += 2;
            continue;
        }
        if word == "--file" || word == "-f" {
            file = args.get(i + 1).cloned();
            i += 2;
            continue;
        }
        if let Some(rest) = word.strip_prefix("--file=") {
            file = Some(rest.to_string());
            i += 1;
            continue;
        }
        if matches!(
            word,
            "--blob" | "--default" | "--comment" | "--type" | "--name" | "--id"
        ) {
            i += 2;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        positional.push(word);
        i += 1;
    }
    if !config_scope_guarded(global, file.as_deref(), git_words, cwd) {
        return None;
    }
    if edit {
        return Some(disarm_why(
            "`git config --edit` can set core.hooksPath and skip the pre-push hook",
        ));
    }
    if let Some(section) = section.as_deref() {
        if section_disarms(section) {
            return Some(disarm_why(
                "`git config --remove-section` can drop core.hooksPath and skip the pre-push hook",
            ));
        }
    }
    let key = positional.first().copied().unwrap_or("");
    let key = key.split_once('=').map(|(key, _)| key).unwrap_or(key);
    if !key_disarms(key) {
        return None;
    }
    let sets =
        write || positional.len() >= 2 || positional.first().is_some_and(|w| w.contains('='));
    if read && !sets {
        return None;
    }
    if !sets {
        return None;
    }
    Some(disarm_why(
        "`git config` writes a key that skips the pre-push hook",
    ))
}

/// Global and system config, a named gitconfig, and the recorded seat's
/// local config can turn the hook off. A throwaway repository's local
/// `core.hooksPath` is how the bookkeeping fixtures run.
fn config_scope_guarded(
    global: bool,
    file: Option<&str>,
    git_words: &[String],
    cwd: Option<&Path>,
) -> bool {
    if global {
        return true;
    }
    if let Some(file) = file {
        return crate::cloud_hook::names_protected(file, cwd);
    }
    let dir = git_dash_c(git_words).unwrap_or_else(|| PathBuf::from("."));
    let dir = if dir.is_absolute() {
        dir
    } else {
        match cwd {
            Some(cwd) => cwd.join(dir),
            None => dir,
        }
    };
    crate::cloud_hook::dir_is_recorded_seat(&dir)
}

fn git_dash_c(words: &[String]) -> Option<PathBuf> {
    let mut i = 1usize;
    while i < words.len() {
        let word = words.get(i).map(String::as_str)?;
        if word == "-C" {
            return words.get(i + 1).map(PathBuf::from);
        }
        if let Some(rest) = word.strip_prefix("-C") {
            if !rest.is_empty() && !word.starts_with("--") {
                return Some(PathBuf::from(rest));
            }
        }
        if GIT_VALUED.contains(&word) {
            i += 2;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        break;
    }
    None
}

fn key_disarms(key: &str) -> bool {
    if key.eq_ignore_ascii_case("core.hooksPath") {
        return true;
    }
    let lower = key.to_ascii_lowercase();
    lower == "include.path" || lower.starts_with("include.") || lower.starts_with("includeif.")
}

fn section_disarms(section: &str) -> bool {
    let lower = section.to_ascii_lowercase();
    lower == "core" || lower == "include" || lower.starts_with("includeif")
}

fn walk_child(
    depth: usize,
    tainted: bool,
    gh_unreadable: bool,
    git_aliases: &[(String, String)],
    gh_aliases: &[(String, String)],
) -> WalkIn {
    WalkIn {
        depth: depth + 1,
        tainted,
        gh_unreadable,
        git_aliases: git_aliases.to_vec(),
        gh_aliases: gh_aliases.to_vec(),
    }
}

fn env_value(env: &[(String, String)], names: &[(String, String)], key: &str) -> Option<String> {
    env.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .or_else(|| {
            names
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        })
}

fn unreadable_token(word: &str) -> bool {
    word.contains(['$', '`'])
}

/// How many times an alias may expand into another alias before the line
/// is refused. A real alias is one step. A loop would hide the verb.
const ALIAS_DEPTH: usize = 8;

/// A command's words as it runs them: prefixes and quotes off, the
/// command name without its directory, and redirections dropped with
/// their targets.
fn argv(segment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut target = false;
    for word in crate::plain_words(segment) {
        if std::mem::take(&mut target) {
            continue;
        }
        if word.starts_with('>') || word.starts_with('<') {
            target = word == ">" || word.chars().all(|c| c == '<');
            continue;
        }
        out.push(word);
    }
    out
}

enum GitOutcome {
    Push(Box<Push>),
    /// A `!` alias. The whole script is judged by the same walk.
    Shell(String),
}

enum Expanded {
    Argv(Vec<String>),
    Shell(String),
}

/// Where `cd ARGS` leaves the line, `None` when it cannot be told: `-`,
/// a variable, or a directory that was already unknown.
/// `cd "$(git rev-parse --show-toplevel)"` is this checkout's top.
fn moved(here: Option<PathBuf>, args: &[String]) -> Option<PathBuf> {
    let home = || crate::home().ok();
    let Some(to) = args.iter().find(|a| !a.starts_with('-') || *a == "-") else {
        return home();
    };
    if to == "-" {
        return None;
    }
    if to.contains(['$', '`']) {
        return toplevel_cd(here, to);
    }
    if to == "~" {
        return home();
    }
    if let Some(rest) = to.strip_prefix("~/") {
        return home().map(|h| h.join(rest));
    }
    if to.starts_with('~') {
        return None;
    }
    Some(here?.join(to))
}

/// The checkout top `cd` named with `git rev-parse --show-toplevel`, or
/// `None` for any other substitution. The command is run in `here`, so a
/// subdirectory and the top resolve to the same repository.
fn toplevel_cd(here: Option<PathBuf>, to: &str) -> Option<PathBuf> {
    if to != "$(git rev-parse --show-toplevel)" && to != "`git rev-parse --show-toplevel`" {
        return None;
    }
    let here = here?;
    let out = Command::new("git")
        .current_dir(here)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

#[derive(Default)]
struct PrefixContext {
    dir: Option<PathBuf>,
    env: Vec<(String, String)>,
    via_xargs: bool,
}

fn assignment(word: &str) -> Option<(&str, &str)> {
    let (name, value) = word.split_once('=')?;
    let valid = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then_some((name, value))
}

fn prefix_context(segment: &str, here: Option<PathBuf>) -> PrefixContext {
    let words = crate::shell_words(segment);
    let mut ctx = PrefixContext {
        dir: here,
        ..PrefixContext::default()
    };
    let mut i = 0;
    while let Some(word) = words.get(i).map(String::as_str) {
        if let Some((name, value)) = assignment(word) {
            ctx.env.push((name.to_string(), value.to_string()));
            i += 1;
            continue;
        }
        if word == "xargs" {
            ctx.via_xargs = true;
            break;
        }
        if word != "env" {
            break;
        }
        i += 1;
        while let Some(arg) = words.get(i).map(String::as_str) {
            if arg == "--" {
                i += 1;
                break;
            }
            if arg == "-C" || arg == "--chdir" {
                let next = words.get(i + 1).cloned();
                ctx.dir = next.and_then(|value| moved(ctx.dir, &[value]));
                i += 2;
                continue;
            }
            if let Some(path) = arg.strip_prefix("--chdir=") {
                ctx.dir = moved(ctx.dir, &[path.to_string()]);
                i += 1;
                continue;
            }
            if matches!(arg, "-u" | "--unset") {
                i += 2;
                continue;
            }
            if let Some((name, value)) = assignment(arg) {
                ctx.env.push((name.to_string(), value.to_string()));
                i += 1;
                continue;
            }
            if arg.starts_with('-') && arg.len() > 1 {
                i += 1;
                continue;
            }
            break;
        }
    }
    ctx
}

/// The push `args` (the words after `git`) make, or `None` when they run
/// another subcommand. An alias set with `-c alias.NAME=push` is a push.
fn git_push(
    args: &[String],
    dir: Option<PathBuf>,
    line_aliases: &[(String, String)],
    env: &[(String, String)],
    via_xargs: bool,
) -> Option<GitOutcome> {
    let mut at = Checkout {
        dir,
        env: env.to_vec(),
        ..Checkout::default()
    };
    let mut i = 0;
    while let Some(word) = args.get(i).map(String::as_str) {
        if GIT_VALUED.contains(&word) {
            let value = args.get(i + 1)?;
            match word {
                "-c" => {
                    at.config.extend(["-c".to_string(), value.clone()]);
                }
                "--config-env" => at.config.extend([word.to_string(), value.clone()]),
                "-C" | "--git-dir" | "--work-tree" => {
                    at.place.extend([word.to_string(), value.clone()]);
                }
                _ => {}
            }
            i += 2;
        } else if word.starts_with("--config-env=") {
            at.config.push(word.to_string());
            i += 1;
        } else if word.starts_with("--git-dir=") || word.starts_with("--work-tree=") {
            at.place.push(word.to_string());
            i += 1;
        } else if word.starts_with('-') {
            i += 1;
        } else {
            break;
        }
    }
    let sub = args.get(i)?;
    let mut words: Vec<String> = vec![sub.clone()];
    words.extend(args[i + 1..].iter().cloned());
    words = match at.expand_aliases(words, line_aliases) {
        Ok(Expanded::Argv(words)) => words,
        Ok(Expanded::Shell(script)) => return Some(GitOutcome::Shell(script)),
        Err(why) => return Some(GitOutcome::Push(Box::new(Push::refuse(at, why)))),
    };
    let sub = words.first().map(String::as_str)?;
    if unresolved_subcommand(sub) {
        return Some(GitOutcome::Push(Box::new(Push::refuse(
            at,
            format!("`{sub}` is only known once the shell expands it; name the git subcommand"),
        ))));
    }
    match sub {
        "push" => {
            if via_xargs {
                return Some(GitOutcome::Push(Box::new(Push::refuse(
                    at,
                    "xargs fills git push arguments at run time; name each refspec directly"
                        .to_string(),
                ))));
            }
            Some(GitOutcome::Push(Box::new(Push::read(&words[1..], at))))
        }
        "send-pack" => Some(GitOutcome::Push(Box::new(send_pack_push(&words, at)))),
        "subtree" => subtree_push_cursor_ref(&words, &at)
            .map(|why| GitOutcome::Push(Box::new(Push::refuse(at, why)))),
        "submodule" => submodule_foreach_cursor_ref(&words, &at)
            .map(|why| GitOutcome::Push(Box::new(Push::refuse(at, why)))),
        _ => None,
    }
}

/// A `git config` write whose key changes where a later push on this line
/// sends refs. An alias assignment is not one: that alias is judged.
fn config_affects_push(args: &[String]) -> bool {
    let mut i = 0;
    while let Some(word) = args.get(i).map(String::as_str) {
        if GIT_VALUED.contains(&word) {
            i += 2;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        break;
    }
    if args.get(i).map(String::as_str) != Some("config") {
        return false;
    }
    i += 1;
    let mut read = false;
    let mut positional: Vec<&str> = Vec::new();
    while let Some(word) = args.get(i).map(String::as_str) {
        if matches!(
            word,
            "--get"
                | "--get-all"
                | "--get-regexp"
                | "--get-urlmatch"
                | "--list"
                | "-l"
                | "--show-origin"
                | "--show-scope"
                | "--get-color"
        ) {
            read = true;
            i += 1;
            continue;
        }
        if matches!(
            word,
            "--file" | "-f" | "--blob" | "--default" | "--comment" | "--type"
        ) {
            i += 2;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        positional.push(word);
        i += 1;
    }
    if read || positional.is_empty() {
        return false;
    }
    let key = positional[0];
    let key = key.split_once('=').map(|(key, _)| key).unwrap_or(key);
    if unreadable_token(key) {
        return true;
    }
    affects_push_key(key)
}

fn affects_push_key(key: &str) -> bool {
    if key == "push.default" || key == "remote.pushdefault" {
        return true;
    }
    let Some((head, tail)) = key.split_once('.') else {
        return false;
    };
    match head {
        "remote" => tail.ends_with(".push") || tail.ends_with(".mirror"),
        "branch" => {
            tail.ends_with(".merge")
                || tail.ends_with(".remote")
                || tail.ends_with(".pushRemote")
                || tail.ends_with(".pushremote")
        }
        _ => false,
    }
}

/// A `git config alias.NAME VALUE` assignment from one command line segment.
fn git_alias_write(args: &[String]) -> Option<(String, String)> {
    let mut i = 0;
    while let Some(word) = args.get(i).map(String::as_str) {
        if GIT_VALUED.contains(&word) {
            i += 2;
        } else if word.starts_with('-') {
            i += 1;
        } else {
            break;
        }
    }
    if args.get(i).map(String::as_str) != Some("config") {
        return None;
    }
    let mut key_at = i + 1;
    while args
        .get(key_at)
        .is_some_and(|word| word.starts_with('-') && word.len() > 1)
    {
        key_at += 1;
    }
    let key = args.get(key_at)?;
    let (name, value) = match key.split_once('=') {
        Some((name, value)) => (name, value.to_string()),
        None => (key.as_str(), args.get(key_at + 1)?.clone()),
    };
    let name = name.strip_prefix("alias.")?;
    Some((name.to_string(), value))
}

fn unresolved_subcommand(sub: &str) -> bool {
    sub.contains(['$', '`', '(', ')', '{', '}', '[', ']', '*', '?'])
}

/// `git send-pack`, flags included after the remote. `--stdin` names no
/// refspec on the line, so it fails closed. The result is judged as a push.
fn send_pack_push(words: &[String], at: Checkout) -> Push {
    let mut remote: Option<String> = None;
    let mut specs: Vec<String> = Vec::new();
    let mut bulk: Vec<String> = Vec::new();
    let mut dry = false;
    let mut force = false;
    let mut stdin = false;
    let mut i = 1usize;
    while i < words.len() {
        let word = words[i].as_str();
        if word == "--" {
            let rest = &words[i + 1..];
            if remote.is_none() {
                if let Some(name) = rest.first() {
                    remote = Some(name.clone());
                    specs.extend(rest.iter().skip(1).cloned());
                }
            } else {
                specs.extend(rest.iter().cloned());
            }
            break;
        }
        if matches!(word, "--receive-pack" | "--exec") {
            i += 2;
            continue;
        }
        if word.starts_with("--receive-pack=") || word.starts_with("--exec=") {
            i += 1;
            continue;
        }
        if matches!(
            word,
            "--all" | "--branches" | "--mirror" | "--tags" | "--stdin" | "--dry-run"
        ) || is_force_flag(word)
        {
            match word {
                "--all" | "--branches" | "--mirror" | "--tags" => bulk.push(word.to_string()),
                "--stdin" => stdin = true,
                "--dry-run" => dry = true,
                _ => force = true,
            }
            i += 1;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        if remote.is_none() {
            remote = Some(word.to_string());
        } else {
            specs.push(word.to_string());
        }
        i += 1;
    }
    if stdin {
        return Push::refuse(
            at,
            format!("git send-pack --stdin reads refspecs the line does not show; {ALLOW_RULE}"),
        );
    }
    let mut pushed = Vec::new();
    let repo_why = remote.as_deref().map(|name| send_pack_repo_why(&at, name));
    if let Some(name) = remote {
        pushed.push(name);
    }
    pushed.extend(specs);
    let mut push = Push {
        reason: None,
        repo_why: None,
        at,
        words: pushed,
        repo: None,
        bulk,
        prune: false,
        dry_run: dry,
        force,
        delete: false,
    };
    if let Some(Err(why)) = repo_why {
        push.repo_why = Some(why);
    }
    push
}

/// `git send-pack` does not run the pre-push hook, so the destination has
/// to be on the allowlist here. A name git cannot resolve fails closed.
fn send_pack_repo_why(at: &Checkout, remote: &str) -> Result<(), String> {
    let url = if remote_is_explicit(remote) {
        remote.to_string()
    } else {
        let out = at.git(&[], &["remote", "get-url", remote]).ok_or_else(|| {
            format!("git send-pack destination {remote} could not be read; {ALLOW_RULE}")
        })?;
        if !out.status.success() {
            return Err(format!(
                "git send-pack destination {remote} could not be read; {ALLOW_RULE}"
            ));
        }
        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if url.is_empty() {
            return Err(format!(
                "git send-pack destination {remote} could not be read; {ALLOW_RULE}"
            ));
        }
        url
    };
    match crate::cloud_hook::repo_on_allowlist(&url) {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "git send-pack destination is not a repository this seat may push; {ALLOW_RULE}"
        )),
        Err(why) => Err(format!("git send-pack {why}; {ALLOW_RULE}")),
    }
}

fn remote_is_explicit(remote: &str) -> bool {
    remote.contains("://")
        || remote.starts_with('/')
        || remote.starts_with('.')
        || remote.starts_with('~')
        || remote.contains(':')
}

fn subtree_push_cursor_ref(words: &[String], at: &Checkout) -> Option<String> {
    if words.first().map(String::as_str) != Some("subtree")
        || words.get(1).map(String::as_str) != Some("push")
    {
        return None;
    }
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 2;
    while let Some(word) = words.get(i).map(String::as_str) {
        if word == "--" {
            positional.extend(words[i + 1..].iter().map(String::as_str));
            break;
        }
        if matches!(word, "--prefix" | "-P") {
            i += 2;
            continue;
        }
        if word.starts_with("--prefix=") || (word.starts_with('-') && word.len() > 1) {
            i += 1;
            continue;
        }
        positional.push(word);
        i += 1;
    }
    let remote = positional.first().copied()?;
    let spec = positional.get(1).copied()?;
    at.spec(spec, Some(remote))
}

fn submodule_foreach_cursor_ref(words: &[String], at: &Checkout) -> Option<String> {
    if words.first().map(String::as_str) != Some("submodule")
        || words.get(1).map(String::as_str) != Some("foreach")
    {
        return None;
    }
    let mut i = 2;
    while words
        .get(i)
        .is_some_and(|word| word.starts_with('-') && word.len() > 1)
    {
        i += 1;
    }
    let script = words.get(i..)?.join(" ");
    let why = cursor_push(&script, at.dir.as_deref())?;
    Some(format!(
        "`git submodule foreach` runs a command where {why}"
    ))
}

/// A `git push` as its words give it.
struct Push {
    /// A refusal reason for non-`git push` forms that still write refs.
    reason: Option<String>,
    /// `git send-pack` names a repository the allowlist does not have.
    /// Ordinary `git push` leaves this empty: the hook is the backstop.
    repo_why: Option<String>,
    at: Checkout,
    /// The words past the options: the remote, then the refspecs.
    words: Vec<String>,
    /// The `--repo` value.
    repo: Option<String>,
    /// `--all`, `--branches`, `--mirror`, `--tags` and `--follow-tags`,
    /// which push local refs the words do not name.
    bulk: Vec<String>,
    /// `--prune`, which deletes remote refs with no local counterpart under
    /// its destination patterns.
    prune: bool,
    /// `--dry-run` or `-n`: the command writes nothing.
    dry_run: bool,
    /// `--force`, a short cluster containing `f`, or a lease force.
    force: bool,
    /// `--delete` or `-d`.
    delete: bool,
}

impl Push {
    fn read(args: &[String], at: Checkout) -> Self {
        let mut push = Push {
            reason: None,
            repo_why: None,
            at,
            words: Vec::new(),
            repo: None,
            bulk: Vec::new(),
            prune: false,
            dry_run: false,
            force: false,
            delete: false,
        };
        let mut it = args.iter();
        while let Some(word) = it.next() {
            let word = word.as_str();
            if word == "--" {
                push.words.extend(it.by_ref().cloned());
                break;
            }
            if word == "--repo" {
                push.repo = it.next().cloned();
            } else if let Some(value) = word.strip_prefix("--repo=") {
                push.repo = Some(value.to_string());
            } else if word == "--prune" {
                push.prune = true;
            } else if word == "--dry-run" || short_dry_run(word) {
                push.dry_run = true;
            } else if word == "--delete" || short_delete(word) {
                push.delete = true;
            } else if is_force_flag(word) {
                push.force = true;
            } else if crate::PUSH_VALUED.contains(&word) || short_cluster_takes_value(word) {
                it.next();
            } else if matches!(
                word,
                "--all" | "--branches" | "--mirror" | "--tags" | "--follow-tags"
            ) {
                push.bulk.push(word.to_string());
            } else if !(word.starts_with('-') && word.len() > 1) {
                push.words.push(word.to_string());
            }
        }
        push
    }

    fn refuse(at: Checkout, reason: String) -> Self {
        Self {
            reason: Some(reason),
            repo_why: None,
            at,
            words: Vec::new(),
            repo: None,
            bulk: Vec::new(),
            prune: false,
            dry_run: false,
            force: false,
            delete: false,
        }
    }

    /// Why this push would write a ref the seat may not update.
    fn cursor_ref(&self) -> Option<String> {
        if self.dry_run {
            return None;
        }
        if let Some(why) = &self.repo_why {
            return Some(why.clone());
        }
        if let Some(reason) = &self.reason {
            return Some(reason.clone());
        }
        if self.delete {
            return Some("this push deletes a ref; a cloud seat may not delete a ref".to_string());
        }
        if self.prune {
            return Some(
                "`--prune` can delete remote refs; a cloud seat may not delete a ref".to_string(),
            );
        }
        // `--follow-tags` also pushes the branch checked out. Judge that
        // branch before the tags, so a `cursor/*` checkout still says so
        // when a tag is present too.
        let follow_tags = self.bulk.iter().any(|f| f == "--follow-tags");
        for flag in &self.bulk {
            if flag == "--follow-tags" {
                continue;
            }
            let under: &[&str] = match flag.as_str() {
                "--all" | "--branches" => &["refs/heads/"],
                "--mirror" => &["refs/heads/", "refs/tags/"],
                _ => &["refs/tags/"],
            };
            if let Some(why) = self.at.local(under, &format!("`{flag}`")) {
                return Some(why);
            }
        }
        if self.bulk.iter().any(|f| f == "--mirror") {
            return Some(
                "`--mirror` can delete remote branches and tags; a cloud seat may not delete a ref"
                    .to_string(),
            );
        }
        // git reads the first word as the remote even beside `--repo`, and
        // a writer may mean every word as a refspec; both readings are
        // judged.
        let (remote, specs) = if self.repo.is_some() {
            (self.words.first().or(self.repo.as_ref()), &self.words[..])
        } else {
            (self.words.first(), self.words.get(1..).unwrap_or(&[]))
        };
        let mut words = specs.iter();
        while let Some(word) = words.next() {
            let spec = match (word.as_str(), words.clone().next()) {
                ("tag", Some(name)) => {
                    words.next();
                    format!("refs/tags/{name}")
                }
                _ => word.clone(),
            };
            if let Some(why) = self.at.spec(&spec, remote.map(String::as_str)) {
                return Some(why);
            }
        }
        // `--all`, `--branches` and `--mirror` already judged the local
        // branches. `--tags` pushes tags and not the branch checked out.
        // `--follow-tags` pushes that branch as well, so it is read below.
        let covers_heads = self
            .bulk
            .iter()
            .any(|f| matches!(f.as_str(), "--all" | "--branches" | "--mirror"));
        let tags_only = self.bulk.iter().any(|f| f == "--tags") && !covers_heads && !follow_tags;
        if self.words.len() <= 1 && !covers_heads && !tags_only {
            if let Some(why) = self.at.implicit(remote.map(String::as_str)) {
                return Some(why);
            }
        }
        if follow_tags {
            if let Some(why) = self.at.local(&["refs/tags/"], "`--follow-tags`") {
                return Some(why);
            }
        }
        if self.force {
            return Some(format!("a cloud seat may not force a ref; {ALLOW_RULE}"));
        }
        None
    }
}

/// A short option cluster (`-n`, `-fu`), not a long option.
fn short_cluster(word: &str) -> Option<&str> {
    let flags = word.strip_prefix('-')?;
    if flags.is_empty() || flags.starts_with('-') {
        return None;
    }
    flags
        .chars()
        .all(|c| c.is_ascii_alphabetic())
        .then_some(flags)
}

fn short_dry_run(word: &str) -> bool {
    short_cluster(word).is_some_and(|flags| flags.contains('n'))
}

fn short_delete(word: &str) -> bool {
    short_cluster(word).is_some_and(|flags| flags.contains('d'))
}

fn is_force_flag(word: &str) -> bool {
    word == "--force"
        || word == "--force-with-lease"
        || word.starts_with("--force-with-lease=")
        || word == "--force-if-includes"
        || short_cluster(word).is_some_and(|flags| flags.contains('f'))
}

/// `-fo VALUE`: a short cluster whose last letter is `-o`, which takes
/// the next word as its value.
fn short_cluster_takes_value(word: &str) -> bool {
    word.strip_prefix('-').is_some_and(|flags| {
        flags.len() > 1 && flags.chars().all(|c| c.is_ascii_alphabetic()) && flags.ends_with('o')
    })
}

/// The last `:` that splits a refspec into `src:dst`. Colons inside
/// `$(...)`, `${...}` and backticks stay inside that side, so
/// `$(echo HEAD:cursor/x)` is one destination and not `cursor/x)`.
fn refspec_sides(spec: &str) -> Option<(&str, &str)> {
    let mut paren = 0i32;
    let mut brace = 0i32;
    let mut tick = false;
    let mut colon = None;
    let mut chars = spec.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '`' => tick = !tick,
            '$' if !tick => match chars.peek().map(|(_, n)| *n) {
                Some('(') => {
                    paren += 1;
                    chars.next();
                }
                Some('{') => {
                    brace += 1;
                    chars.next();
                }
                _ => {}
            },
            ')' if !tick && paren > 0 => paren -= 1,
            '}' if !tick && brace > 0 => brace -= 1,
            ':' if !tick && paren == 0 && brace == 0 => colon = Some(i),
            _ => {}
        }
    }
    let at = colon?;
    Some((&spec[..at], &spec[at + 1..]))
}

/// A full refname is allowed when it is `refs/heads/<branch>` and that
/// branch is on the seat allowlist. Tags and other namespaces are not.
fn local_ref_allowed(allow: &HashSet<String>, full: &str) -> bool {
    full.strip_prefix("refs/heads/")
        .is_some_and(|name| allow.contains(name))
}

/// Where `name` lands under the pattern pair `src:dst`, each holding one
/// `*`, or `None` when `src` does not match it.
fn land(src: &str, dst: &str, name: &str) -> Option<String> {
    let (src_head, src_tail) = src.split_once('*')?;
    let (dst_head, dst_tail) = dst.split_once('*')?;
    let middle = name.strip_prefix(src_head)?.strip_suffix(src_tail)?;
    Some(format!("{dst_head}{middle}{dst_tail}"))
}

/// The repository a push runs against, each part read once and only when
/// a refspec needs it.
#[derive(Default)]
struct Checkout {
    /// Where the line is when the push runs; `None` when a `cd` on the
    /// way cannot be followed.
    dir: Option<PathBuf>,
    /// Environment assignments the shell applies to this git command.
    env: Vec<(String, String)>,
    /// The push's `-C`, `--git-dir` and `--work-tree`, given to each read.
    place: Vec<String>,
    /// The push's `-c NAME=VALUE`, given to the config read.
    config: Vec<String>,
    head: OnceCell<Option<Option<String>>>,
    settings: OnceCell<Option<Vec<(String, String)>>>,
    refs: OnceCell<Option<Vec<String>>>,
}

impl Checkout {
    fn git(&self, extra: &[String], args: &[&str]) -> Option<Output> {
        Command::new("git")
            .current_dir(self.dir.as_ref()?)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .args(&self.place)
            .args(extra)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
    }

    fn read(&self, extra: &[String], args: &[&str]) -> Option<String> {
        let out = self.git(extra, args)?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// The branch checked out: `Some(None)` on a detached `HEAD`, `None`
    /// when the checkout cannot be read.
    fn branch(&self) -> Option<Option<&str>> {
        self.head
            .get_or_init(|| {
                let out = self.git(&[], &["symbolic-ref", "-q", "HEAD"])?;
                match out.status.code() {
                    Some(0) => Some(
                        String::from_utf8_lossy(&out.stdout)
                            .trim()
                            .strip_prefix("refs/heads/")
                            .map(str::to_string),
                    ),
                    Some(1) => Some(None),
                    _ => None,
                }
            })
            .as_ref()
            .map(Option::as_deref)
    }

    /// Every config entry git reads for the push, `-c` included, in order.
    fn settings(&self) -> Option<&[(String, String)]> {
        self.settings
            .get_or_init(|| {
                let text = self.read(&self.config, &["config", "-z", "--list"])?;
                Some(
                    text.split('\0')
                        .filter(|entry| !entry.is_empty())
                        .map(|entry| match entry.split_once('\n') {
                            Some((key, value)) => (key.to_string(), value.to_string()),
                            None => (entry.to_string(), "true".to_string()),
                        })
                        .collect(),
                )
            })
            .as_deref()
    }

    fn alias_setting(&self, sub: &str) -> Option<String> {
        let key = format!("alias.{sub}");
        self.settings()?
            .iter()
            .rev()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
    }

    fn expand_aliases(
        &self,
        words: Vec<String>,
        line_aliases: &[(String, String)],
    ) -> Result<Expanded, String> {
        let mut words = words;
        let mut seen = HashSet::new();
        while let Some(sub) = words.first().cloned() {
            if !seen.insert(sub.clone()) {
                break;
            }
            let value = line_aliases
                .iter()
                .rev()
                .find(|(name, _)| *name == sub)
                .map(|(_, value)| value.clone())
                .or_else(|| self.alias_setting(&sub));
            let Some(value) = value else { break };
            if let Some(script) = value.strip_prefix('!') {
                let mut line = script.trim().to_string();
                let extra: Vec<String> = words.iter().skip(1).cloned().collect();
                if !extra.is_empty() {
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(&extra.join(" "));
                }
                return Ok(Expanded::Shell(line));
            }
            let mut expanded: Vec<String> = value.split_whitespace().map(str::to_string).collect();
            if expanded.is_empty() {
                break;
            }
            expanded.extend(words.into_iter().skip(1));
            words = expanded;
        }
        Ok(Expanded::Argv(words))
    }

    /// Every ref in the checkout, by its full name.
    fn refs(&self) -> Option<&[String]> {
        self.refs
            .get_or_init(|| {
                let text = self.read(&[], &["for-each-ref", "--format=%(refname)"])?;
                Some(text.lines().map(str::to_string).collect())
            })
            .as_deref()
    }

    /// Why a push of local refs under one of `under` leaves the allowlist.
    fn local(&self, under: &[&str], what: &str) -> Option<String> {
        let Some(refs) = self.refs() else {
            return Some(UNREAD.into());
        };
        let allow = match seat_allowlist() {
            Ok(allow) => allow,
            Err(why) => return Some(why),
        };
        refs.iter()
            .find(|r| under.iter().any(|p| r.starts_with(p)) && !local_ref_allowed(&allow, r))
            .map(|r| format!("{what} pushes {r}"))
    }

    /// Why the refspec `spec` would write a ref outside the allowlist, or
    /// force or delete one. The destination is `dst` of `src:dst`, else
    /// the refspec itself, a leading `+` off. `HEAD` is the branch checked
    /// out. A wildcard is each local ref it maps. A destination the shell
    /// still has to expand cannot be told, and is refused.
    fn spec(&self, spec: &str, remote: Option<&str>) -> Option<String> {
        let forced = spec.starts_with('+');
        let spec = spec.strip_prefix('+').unwrap_or(spec);
        if spec == ":" {
            return Some(format!("a matching push can delete refs; {ALLOW_RULE}"));
        }
        // A destination of `HEAD` is a branch named `HEAD` to git; only a
        // bare `HEAD` or `@` pushes the branch checked out. A colon inside
        // `$(...)`, `${...}` or backticks belongs to that substitution.
        let (src, dst, bare) = match refspec_sides(spec) {
            Some((src, dst)) => (src, dst, false),
            None => (spec, spec, true),
        };
        if src.is_empty() {
            return Some(format!(
                "this push deletes {dst}; a cloud seat may not delete a ref"
            ));
        }
        if bare {
            if let Some(landed) = self.map_bare_destination(src, remote) {
                if landed != dst {
                    if dest_refused(&landed).is_some() {
                        return Some(format!("`{spec}` pushes {src} to {landed}"));
                    }
                    return forced.then(|| {
                        format!("this push updates {landed}; a cloud seat may not force a ref")
                    });
                }
            }
        }
        if CURRENT_BRANCH.contains(&dst) || (bare && matches!(dst, "HEAD" | "@")) {
            return match self.branch() {
                None => Some(UNREAD.into()),
                Some(Some(branch)) => match branch_permitted(branch) {
                    Err(why) => Some(why),
                    Ok(false) => Some(format!(
                        "this push updates {branch}, the branch checked out"
                    )),
                    Ok(true) if forced => Some(format!(
                        "this push updates {branch}; a cloud seat may not force a ref"
                    )),
                    Ok(true) => None,
                },
                Some(None) if forced => {
                    Some(format!("a cloud seat may not force a ref; {ALLOW_RULE}"))
                }
                Some(None) => None,
            };
        }
        // An earlier assignment has already been substituted. What remains
        // is a destination the shell has not expanded, named in full.
        if dst.contains(['$', '`']) {
            return Some(format!(
                "`{dst}` is only known once the shell expands it; push a named branch"
            ));
        }
        if dst.contains('*') {
            let Some(refs) = self.refs() else {
                return Some(UNREAD.into());
            };
            let hit = refs.iter().find_map(|r| {
                let landed = land(src, dst, r).or_else(|| {
                    (!src.starts_with("refs/"))
                        .then(|| land(&format!("refs/heads/{src}"), dst, r))
                        .flatten()
                })?;
                dest_refused(&landed).map(|_| format!("`{spec}` pushes {r} to {landed}"))
            });
            if hit.is_some() {
                return hit;
            }
            return forced.then(|| format!("a wildcard push may not be forced; {ALLOW_RULE}"));
        }
        if let Some(why) = dest_refused(dst) {
            return Some(why);
        }
        forced.then(|| format!("this push updates {dst}; a cloud seat may not force a ref"))
    }

    /// Why a push with no refspec would update a `cursor/*` ref, read as
    /// git reads it: the remote's `push` refspecs or `mirror`, else
    /// `push.default` over the branch checked out. `remote` is the one
    /// the words name.
    fn implicit(&self, remote: Option<&str>) -> Option<String> {
        let Some(branch) = self.branch() else {
            return Some(UNREAD.into());
        };
        let Some(settings) = self.settings() else {
            return Some(UNREAD.into());
        };
        let get = |key: &str| {
            settings
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let of_branch = |name: &str| branch.and_then(|b| get(&format!("branch.{b}.{name}")));
        let remote = remote
            .or_else(|| of_branch("pushremote"))
            .or_else(|| get("remote.pushdefault"))
            .or_else(|| of_branch("remote"))
            .unwrap_or("origin");
        if get(&format!("remote.{remote}.mirror"))
            .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"))
        {
            return self.local(&["refs/heads/", "refs/tags/"], "a mirror remote");
        }
        let key = format!("remote.{remote}.push");
        let pushes: Vec<&str> = settings
            .iter()
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
            .collect();
        if !pushes.is_empty() {
            return pushes.iter().find_map(|spec| self.spec(spec, Some(remote)));
        }
        match get("push.default").unwrap_or("simple") {
            "nothing" => None,
            "matching" => self.local(&["refs/heads/"], "a matching push"),
            "upstream" | "tracking" => {
                of_branch("merge").and_then(|merge| match branch_permitted(merge) {
                    Err(why) => Some(why),
                    Ok(false) => Some(format!(
                        "this push updates {merge}, the upstream of the branch checked out"
                    )),
                    Ok(true) => None,
                })
            }
            _ => match branch {
                Some(b) => match branch_permitted(b) {
                    Err(why) => Some(why),
                    Ok(false) => Some(format!("this push updates {b}, the branch checked out")),
                    Ok(true) => None,
                },
                None => None,
            },
        }
    }

    /// Where a bare source-only refspec lands once git applies remapping.
    fn map_bare_destination(&self, src: &str, remote: Option<&str>) -> Option<String> {
        let settings = self.settings()?;
        let get = |key: &str| {
            settings
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let branch = src.strip_prefix("refs/heads/").unwrap_or(src);
        let remote = remote
            .or_else(|| get(&format!("branch.{branch}.pushremote")))
            .or_else(|| get("remote.pushdefault"))
            .or_else(|| get(&format!("branch.{branch}.remote")))
            .unwrap_or("origin");
        let key = format!("remote.{remote}.push");
        for push in settings.iter().filter(|(k, _)| *k == key) {
            if let Some(dst) = map_push_spec(&push.1, src) {
                return Some(dst);
            }
        }
        match get("push.default").unwrap_or("simple") {
            "upstream" | "tracking" => {
                let branch_remote = get(&format!("branch.{branch}.remote"));
                if branch_remote.is_none_or(|r| r == remote) {
                    return get(&format!("branch.{branch}.merge")).map(str::to_string);
                }
            }
            _ => {}
        }
        None
    }
}

/// Where `src` lands under one push refspec from `remote.<name>.push`.
fn map_push_spec(spec: &str, src: &str) -> Option<String> {
    let spec = spec.strip_prefix('+').unwrap_or(spec);
    let (from, to) = spec.split_once(':')?;
    let matched = land(from, to, src).or_else(|| {
        (!src.starts_with("refs/")).then(|| land(from, to, &format!("refs/heads/{src}")))?
    });
    if matched.is_some() {
        return matched;
    }
    let from = from.strip_prefix("refs/heads/").unwrap_or(from);
    (from == src).then(|| to.to_string())
}

fn shell_script_file(words: &[String]) -> Option<String> {
    let cmd = words.first()?.rsplit('/').next()?;
    match cmd {
        "source" | "." => words
            .iter()
            .skip(1)
            .find(|word| !word.starts_with('-'))
            .cloned(),
        "bash" | "sh" | "dash" | "zsh" | "ksh" => {
            if words.iter().any(|word| word == "-c" || word == "--command") {
                return None;
            }
            let mut it = words.iter().skip(1);
            while let Some(word) = it.next() {
                if word == "--" {
                    return it.next().cloned();
                }
                if matches!(word.as_str(), "-o" | "--rcfile" | "--init-file") {
                    it.next();
                    continue;
                }
                if word.starts_with('-') && word.len() > 1 {
                    continue;
                }
                return Some(word.clone());
            }
            None
        }
        _ => None,
    }
}

fn script_verdict(file: &str, cwd: Option<&Path>, state: &WalkIn) -> Verdict {
    if unreadable_token(file) {
        return Verdict::closed(format!(
            "`{file}` is only known once the shell expands it; a script it names can write refs; {ALLOW_RULE}"
        ));
    }
    match read_script(cwd, file) {
        ScriptBody::Missing => Verdict::default(),
        ScriptBody::Unreadable => {
            Verdict::closed(format!("the script {file} could not be read; {ALLOW_RULE}"))
        }
        ScriptBody::Text(text) => {
            let inner = judge_depth(&text, cwd, state);
            if inner.push.is_some() || inner.acts.any() {
                return inner;
            }
            if !inner.saw_ref && script_mentions_ref_write(&text) {
                return Verdict::closed(format!(
                    "the script {file} mentions a ref write the guard could not resolve; {ALLOW_RULE}"
                ));
            }
            inner
        }
    }
}

enum ScriptBody {
    Missing,
    Unreadable,
    Text(String),
}

fn read_script(cwd: Option<&Path>, file: &str) -> ScriptBody {
    if file.is_empty() || file.contains('\0') {
        return ScriptBody::Unreadable;
    }
    let path = Path::new(file);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let Some(base) = cwd
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
        else {
            return ScriptBody::Unreadable;
        };
        base.join(path)
    };
    match std::fs::metadata(&full) {
        Ok(meta) if meta.is_file() => match std::fs::read_to_string(&full) {
            Ok(text) => ScriptBody::Text(text),
            Err(_) => ScriptBody::Unreadable,
        },
        Ok(_) => ScriptBody::Unreadable,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => ScriptBody::Missing,
        Err(_) => ScriptBody::Unreadable,
    }
}

fn script_mentions_ref_write(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("git push") || lower.contains("git send-pack")
}

/// The segments of `line` that read a pipe, trimmed: each stage of a
/// pipeline after the first. Another segment with the same text counts
/// as piped too, which only refuses.
fn piped_segments(line: &str) -> Vec<String> {
    crate::pipelines(line)
        .iter()
        .flat_map(|pipe| crate::raw_segments(pipe).into_iter().skip(1))
        .map(|seg| seg.trim().to_string())
        .collect()
}

/// A `<` redirect, here-document, or here-string gives the command a
/// stdin that is not a terminal.
fn stdin_redirected(seg: &str) -> bool {
    crate::shell_words(seg).iter().any(|word| {
        word.trim_start_matches(|c: char| c.is_ascii_digit())
            .starts_with('<')
    })
}
