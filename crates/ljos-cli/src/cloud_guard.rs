//! What a cloud seat refuses once `ljos cloud up` installed its rules,
//! read from the words each command runs rather than matched as text.
//! A ref write may name only the seat's branches and `develop`. Anything
//! else, and any force or delete, is refused. A `gh` command that opens,
//! readies, reopens or merges a pull request is refused too. The source
//! side of a refspec, a remote URL, an option's value and the body of a
//! comment are never what is refused.

use std::cell::OnceCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

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

/// The pull-request actions `line` runs. The same walk reads a ref write,
/// so a `git` alias whose script runs `gh` is not a separate pass.
pub(crate) fn gh_acts(line: &str, cwd: Option<&Path>) -> GhActs {
    judge(line, cwd).acts
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
    let mut gh_aliases = incoming.gh_aliases.clone();
    let mut tainted = incoming.tainted;
    let mut gh_unreadable = incoming.gh_unreadable;
    let mut verdict = Verdict::default();
    for l in crate::nested_lines(line, 0) {
        let mut names: Vec<(String, String)> = Vec::new();
        for seg in crate::raw_segments(&l) {
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
                "gh" => {
                    if let Some(set) = gh_alias_set(&words) {
                        if unreadable_token(&set.0) || unreadable_token(&set.1) || set.1.is_empty()
                        {
                            gh_unreadable = true;
                        } else {
                            let exp = if set.2 { format!("!{}", set.1) } else { set.1 };
                            gh_aliases.push((set.0, exp));
                        }
                        continue;
                    }
                    let child = walk_child(
                        incoming.depth,
                        tainted,
                        gh_unreadable,
                        &git_aliases,
                        &gh_aliases,
                    );
                    verdict = verdict.either(gh_command(
                        &words,
                        &prefix_env(seg),
                        here.as_deref(),
                        &child,
                        run.via_xargs,
                    ));
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
                "curl" | "wget" | "http" | "https" => {
                    // A GET the walk resolved is a read, so a script that
                    // only does that is not failed closed for naming curl.
                    if http_targets_github(&words) {
                        verdict.saw_ref = true;
                    }
                    if let Some(why) = http_ref_write(&words, &run.env, here.as_deref()) {
                        if verdict.push.is_none() {
                            verdict.push = Some(why);
                        }
                    }
                }
                _ => {
                    if cloud_repoint(cmd, &words) {
                        verdict.saw_ref = true;
                        if verdict.push.is_none() {
                            verdict.push = Some(format!(
                                "`ljos cloud up --repoint` would retarget the repository this seat may push; {ALLOW_RULE}"
                            ));
                        }
                    }
                }
            }
        }
    }
    verdict
}

/// `ljos cloud up --repoint` rewrites the repository the hook allows.
fn cloud_repoint(cmd: &str, words: &[String]) -> bool {
    if cmd.rsplit('/').next().unwrap_or(cmd) != "ljos" {
        return false;
    }
    let mut cloud = false;
    let mut up = false;
    let mut repoint = false;
    for word in words.iter().skip(1) {
        match word.as_str() {
            "cloud" => cloud = true,
            "up" => up = true,
            "--repoint" => repoint = true,
            _ => {}
        }
    }
    cloud && up && repoint
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

fn gh_command(
    words: &[String],
    env: &[(String, String)],
    cwd: Option<&Path>,
    incoming: &WalkIn,
    via_xargs: bool,
) -> Verdict {
    if incoming.depth > ALIAS_DEPTH {
        return Verdict {
            push: Some(format!(
                "gh alias expansion could not be read; {ALLOW_RULE}"
            )),
            acts: GhActs {
                open: true,
                merge: true,
            },
            saw_ref: true,
        };
    }
    let mut rest = words.iter().map(String::as_str).skip(1);
    let Some(noun) = gh_word(&mut rest) else {
        return Verdict::default();
    };
    if unreadable_token(noun) || (via_xargs && noun == "api") {
        return Verdict::closed(format!(
            "`gh {noun}` takes arguments the line does not show; {ALLOW_RULE}"
        ));
    }
    match noun {
        "help" | "completion" | "status" | "search" | "browse" | "alias" | "config" => {
            Verdict::default()
        }
        "auth" => gh_read_verb(noun, gh_word(&mut rest), &["status", "token"]),
        "pr" => gh_pr(rest),
        "issue" => gh_read_verb(noun, gh_word(&mut rest), &["view", "list", "status"]),
        "repo" => gh_read_verb(noun, gh_word(&mut rest), &["view", "list", "clone"]),
        "release" => gh_release(rest),
        "run" => gh_read_verb(
            noun,
            gh_word(&mut rest),
            &["view", "list", "watch", "download"],
        ),
        "label" => gh_read_verb(noun, gh_word(&mut rest), &["list"]),
        "cache" => gh_read_verb(noun, gh_word(&mut rest), &["list"]),
        "workflow" => gh_read_verb(noun, gh_word(&mut rest), &["view", "list"]),
        "gist" => gh_read_verb(noun, gh_word(&mut rest), &["view", "list"]),
        "api" => gh_api(rest, cwd),
        other => {
            let exp = incoming
                .gh_aliases
                .iter()
                .rev()
                .find(|(name, _)| name == other)
                .map(|(_, exp)| exp.clone())
                .or_else(|| alias_expansion(env, other));
            let Some(exp) = exp else {
                if incoming.gh_unreadable {
                    return Verdict::closed(format!(
                        "a gh alias set on this line could not be read; {ALLOW_RULE}"
                    ));
                }
                return gh_write(other, "");
            };
            let extra: Vec<&str> = rest.collect();
            expand_alias(&exp, &extra, env, cwd, incoming)
        }
    }
}

/// GitHub writes this program runs. There are none. The reads it does run
/// are `gh api user`, `repos/{owner}/{repo}`, that repo's collaborators,
/// and its releases, each a GET.
const GH_WRITES_LJOS_NEEDS: &[&str] = &[];

fn ljos_needs_write(noun: &str, verb: &str) -> bool {
    GH_WRITES_LJOS_NEEDS
        .iter()
        .any(|need| *need == noun || *need == verb)
}

fn gh_write(noun: &str, verb: &str) -> Verdict {
    if ljos_needs_write(noun, verb) {
        return Verdict::default();
    }
    let what = if verb.is_empty() {
        format!("`gh {noun}`")
    } else {
        format!("`gh {noun} {verb}`")
    };
    Verdict::closed(format!(
        "{what} is not a GitHub read this seat runs; {ALLOW_RULE}"
    ))
}

/// `verb` is allowed when it is absent or one of `reads`. Anything else
/// is a write. Opening and merging a pull request are not reads, and they
/// are not this function's: [`gh_pr`] names those two on their own rules.
fn gh_read_verb(noun: &str, verb: Option<&str>, reads: &[&str]) -> Verdict {
    match verb {
        Some(verb) if reads.contains(&verb) => Verdict::default(),
        None => Verdict::default(),
        Some(verb) => gh_write(noun, verb),
    }
}

fn gh_pr<'a>(mut words: impl Iterator<Item = &'a str>) -> Verdict {
    let verb = gh_word(&mut words);
    let acts = pr_verb(verb);
    if acts.any() {
        return Verdict {
            acts,
            ..Verdict::default()
        };
    }
    gh_read_verb(
        "pr",
        verb,
        &["view", "list", "status", "checks", "diff", "checkout"],
    )
}

fn pr_verb(verb: Option<&str>) -> GhActs {
    match verb {
        Some("create" | "new" | "ready" | "reopen") => GhActs {
            open: true,
            merge: false,
        },
        Some("merge") => GhActs {
            open: false,
            merge: true,
        },
        _ => GhActs::default(),
    }
}

fn expand_alias(
    exp: &str,
    extra: &[&str],
    env: &[(String, String)],
    cwd: Option<&Path>,
    incoming: &WalkIn,
) -> Verdict {
    // A shell alias is `!script`. gh appends the words typed after the
    // alias. The script is the same walk as the line that named the alias,
    // so a script that does not push is allowed.
    if let Some(script) = exp.strip_prefix('!') {
        let mut line = script.trim().to_string();
        if !extra.is_empty() {
            line.push(' ');
            line.push_str(&extra.join(" "));
        }
        let child = walk_child(
            incoming.depth,
            incoming.tainted,
            incoming.gh_unreadable,
            &incoming.git_aliases,
            &incoming.gh_aliases,
        );
        return judge_depth(&line, cwd, &child);
    }
    let mut words = vec!["gh".to_string()];
    words.extend(crate::shell_words(exp));
    words.extend(extra.iter().map(|word| (*word).to_string()));
    let child = walk_child(
        incoming.depth,
        incoming.tainted,
        incoming.gh_unreadable,
        &incoming.git_aliases,
        &incoming.gh_aliases,
    );
    gh_command(&words, env, cwd, &child, false)
}

/// `gh alias set [-s] NAME EXPANSION`. `None` when this is not that command.
/// The bool is `--shell` / `-s`.
fn gh_alias_set(words: &[String]) -> Option<(String, String, bool)> {
    let strs: Vec<&str> = words.iter().skip(1).map(String::as_str).collect();
    let mut i = 0;
    let mut shell = false;
    let mut noun = None;
    while i < strs.len() {
        let word = strs[i];
        if word == "-R" || word == "--repo" {
            i += 2;
            continue;
        }
        if word == "-s" || word == "--shell" {
            shell = true;
            i += 1;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        noun = Some(word);
        i += 1;
        break;
    }
    if noun != Some("alias") {
        return None;
    }
    while i < strs.len() {
        let word = strs[i];
        if word == "-s" || word == "--shell" {
            shell = true;
            i += 1;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        break;
    }
    if strs.get(i) != Some(&"set") {
        return None;
    }
    i += 1;
    while i < strs.len() {
        let word = strs[i];
        if word == "-s" || word == "--shell" {
            shell = true;
            i += 1;
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            i += 1;
            continue;
        }
        break;
    }
    let name = strs.get(i)?.to_string();
    i += 1;
    if i >= strs.len() {
        return Some((name, String::new(), shell));
    }
    Some((name, strs[i..].join(" "), shell))
}

fn gh_release<'a>(mut words: impl Iterator<Item = &'a str>) -> Verdict {
    let verb = gh_word(&mut words);
    if verb == Some("create") {
        return Verdict::closed(format!("`gh release create` writes a tag; {ALLOW_RULE}"));
    }
    gh_read_verb("release", verb, &["view", "list", "download"])
}

/// `NAME=value` words in front of `gh`, so `gh alias list` sees the config
/// directory the command itself sets.
fn prefix_env(segment: &str) -> Vec<(String, String)> {
    let mut env = Vec::new();
    for word in crate::shell_words(segment) {
        if word == "gh" || word.rsplit('/').next() == Some("gh") {
            break;
        }
        let Some((key, value)) = word.split_once('=') else {
            continue;
        };
        if assign_key(key) && !value.contains('\0') {
            env.push((key.to_string(), value.to_string()));
        }
    }
    env
}

fn assign_key(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with(|c: char| c.is_ascii_digit())
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The expansion `gh alias list` gives `name`, without the quotes gh puts
/// around a shell alias. `None` when gh has no such alias or cannot be asked.
fn alias_expansion(env: &[(String, String)], name: &str) -> Option<String> {
    let mut cmd = Command::new("gh");
    cmd.args(["alias", "list"])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in env {
        cmd.env(key, value);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let Some((alias, exp)) = line.split_once(':') else {
            continue;
        };
        if alias.trim() != name {
            continue;
        }
        let exp = exp.trim();
        let exp = exp
            .strip_prefix('\'')
            .and_then(|inner| inner.strip_suffix('\''))
            .unwrap_or(exp);
        return Some(exp.to_string());
    }
    None
}

/// gh api flags that take the next word. A flag not listed here is boolean,
/// so its word is not the endpoint and not a body.
const API_VALUED: &[&str] = &[
    "-X",
    "--method",
    "-f",
    "--raw-field",
    "-F",
    "--field",
    "-H",
    "--header",
    "--hostname",
    "--input",
    "-q",
    "--jq",
    "-p",
    "--preview",
    "-t",
    "--template",
    "--cache",
];

const OPEN_MUTATIONS: &[&str] = &[
    "createPullRequest",
    "reopenPullRequest",
    "markPullRequestReadyForReview",
];

const MERGE_MUTATIONS: &[&str] = &["mergePullRequest", "enablePullRequestAutoMerge"];

struct ApiCall {
    method: Option<String>,
    endpoint: String,
    fields: Vec<(String, String)>,
    input: Option<String>,
    /// A `-F` `@file` or an `--input` body that could not be read.
    opaque: bool,
    method_unreadable: bool,
    endpoint_unreadable: bool,
}

fn gh_api<'a>(words: impl Iterator<Item = &'a str>, cwd: Option<&Path>) -> Verdict {
    let Some(call) = parse_api_call(words, cwd) else {
        return Verdict::closed(format!("`gh api` endpoint could not be read; {ALLOW_RULE}"));
    };
    if call.method_unreadable || call.endpoint_unreadable || unreadable_token(&call.endpoint) {
        return Verdict::closed(format!(
            "`gh api` method or endpoint could not be read; {ALLOW_RULE}"
        ));
    }
    let acts = classify_api(
        &call.endpoint,
        call.method.as_deref(),
        &call.fields,
        call.input.as_deref(),
        cwd,
    );
    // Open and merge keep their own rules. A push reason here would hide
    // them, because the push rule is the one read first.
    if acts.any() {
        return Verdict {
            acts,
            saw_ref: true,
            ..Verdict::default()
        };
    }
    if api_is_safe_read(&call, cwd) {
        return Verdict::default();
    }
    if let Some(why) = classify_api_cursor_ref(
        &call.endpoint,
        call.method.as_deref(),
        &call.fields,
        call.input.as_deref(),
        cwd,
        call.opaque,
    ) {
        return Verdict {
            push: Some(why),
            saw_ref: true,
            ..Verdict::default()
        };
    }
    Verdict::closed(format!(
        "`gh api` is not a GET or HEAD with no request body; {ALLOW_RULE}"
    ))
}

/// A resolved GET or HEAD. No method and no field is gh's GET. An explicit
/// GET may carry `-f` query fields. A GraphQL query is a read; a mutation
/// is not. `--input` and a field read from a file are not resolved.
fn api_is_safe_read(call: &ApiCall, cwd: Option<&Path>) -> bool {
    if call.opaque || call.input.is_some() {
        return false;
    }
    if graphql_path(&api_path(&call.endpoint)) {
        return graphql_is_query(call, cwd);
    }
    match call.method.as_deref() {
        Some(method)
            if method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD") =>
        {
            true
        }
        Some(_) => false,
        None => call.fields.is_empty(),
    }
}

fn graphql_is_query(call: &ApiCall, cwd: Option<&Path>) -> bool {
    if let Some(method) = call.method.as_deref() {
        let upper = method.to_ascii_uppercase();
        if upper != "GET" && upper != "HEAD" && upper != "POST" {
            return false;
        }
    }
    let (texts, opaque) = graphql_texts(&call.fields, call.input.as_deref(), cwd);
    if opaque || texts.is_empty() {
        return false;
    }
    texts
        .iter()
        .all(|text| !has_ident(&without_strings(text), "mutation"))
}

fn parse_api_call<'a>(words: impl Iterator<Item = &'a str>, cwd: Option<&Path>) -> Option<ApiCall> {
    let mut method: Option<String> = None;
    let mut endpoint: Option<String> = None;
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut input: Option<String> = None;
    let mut opaque = false;
    let mut method_unreadable = false;
    let mut endpoint_unreadable = false;
    let mut it = words;
    let note_method = |value: &str, method: &mut Option<String>, bad: &mut bool| {
        if unreadable_token(value) {
            *bad = true;
        }
        *method = Some(value.to_string());
    };
    while let Some(word) = it.next() {
        if word == "--" {
            if endpoint.is_none() {
                if let Some(value) = it.next() {
                    if unreadable_token(value) {
                        endpoint_unreadable = true;
                    }
                    endpoint = Some(value.to_string());
                }
            }
            continue;
        }
        if let Some(rest) = word.strip_prefix("--method=") {
            note_method(rest, &mut method, &mut method_unreadable);
            continue;
        }
        if let Some(rest) = word.strip_prefix("--raw-field=") {
            fields.push(field_pair(rest));
            continue;
        }
        if let Some(rest) = word.strip_prefix("--field=") {
            match resolve_typed_field(rest, cwd) {
                Ok(pair) => fields.push(pair),
                Err(()) => opaque = true,
            }
            continue;
        }
        if let Some(rest) = word.strip_prefix("--input=") {
            input = Some(rest.to_string());
            continue;
        }
        if let Some(inline) = word.strip_prefix("-X") {
            if !word.starts_with("--") && !inline.is_empty() {
                note_method(inline, &mut method, &mut method_unreadable);
                continue;
            }
        }
        if let Some(cluster) = word.strip_prefix('-') {
            if !word.starts_with("--") && !cluster.is_empty() && !API_VALUED.contains(&word) {
                take_api_cluster(
                    cluster,
                    &mut it,
                    cwd,
                    &mut method,
                    &mut method_unreadable,
                    &mut fields,
                    &mut opaque,
                );
                continue;
            }
        }
        if API_VALUED.contains(&word) {
            let Some(value) = it.next() else {
                continue;
            };
            match word {
                "-X" | "--method" => note_method(value, &mut method, &mut method_unreadable),
                "-f" | "--raw-field" => fields.push(field_pair(value)),
                "-F" | "--field" => match resolve_typed_field(value, cwd) {
                    Ok(pair) => fields.push(pair),
                    Err(()) => opaque = true,
                },
                "--input" => input = Some(value.to_string()),
                _ => {}
            }
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            continue;
        }
        if endpoint.is_none() {
            if unreadable_token(word) {
                endpoint_unreadable = true;
            }
            endpoint = Some(word.to_string());
        }
    }
    let endpoint = endpoint?;
    Some(ApiCall {
        method,
        endpoint,
        fields,
        input,
        opaque,
        method_unreadable,
        endpoint_unreadable,
    })
}

/// gh shorts stuck to their values: `-fbase=main`, `-Fk=@file`, `-if title=x`.
/// `i` is boolean. `f`, `F`, and `X` take the rest of the cluster or the next word.
fn take_api_cluster<'a>(
    cluster: &str,
    it: &mut impl Iterator<Item = &'a str>,
    cwd: Option<&Path>,
    method: &mut Option<String>,
    method_unreadable: &mut bool,
    fields: &mut Vec<(String, String)>,
    opaque: &mut bool,
) {
    const VALUE: &str = "fFXHqpt";
    const BOOL: &str = "i";
    let chars: Vec<char> = cluster.chars().collect();
    let mut k = 0usize;
    while k < chars.len() {
        let c = chars[k];
        if BOOL.contains(c) {
            k += 1;
            continue;
        }
        if VALUE.contains(c) {
            let attached: String = chars[k + 1..].iter().collect();
            let value = if attached.is_empty() {
                it.next().unwrap_or("").to_string()
            } else {
                attached
            };
            match c {
                'X' => {
                    if unreadable_token(&value) {
                        *method_unreadable = true;
                    }
                    *method = Some(value);
                }
                'f' => fields.push(field_pair(&value)),
                'F' => match resolve_typed_field(&value, cwd) {
                    Ok(pair) => fields.push(pair),
                    Err(()) => *opaque = true,
                },
                _ => {}
            }
            return;
        }
        k += 1;
    }
}

/// `-F` / `--field` reads `@file` as the field's value. `-f` does not.
/// `Err` when that file is missing, unreadable, or itself a substitution.
fn resolve_typed_field(raw: &str, cwd: Option<&Path>) -> Result<(String, String), ()> {
    let (key, value) = field_pair(raw);
    let Some(path) = value.strip_prefix('@') else {
        return Ok((key, value));
    };
    if path.is_empty() || path == "-" || unreadable_token(path) {
        return Err(());
    }
    let text = read_query(cwd, path).ok_or(())?;
    Ok((key, text.trim_end_matches('\n').to_string()))
}

fn field_pair(raw: &str) -> (String, String) {
    match raw.split_once('=') {
        Some((key, value)) => (key.to_string(), value.to_string()),
        None => (raw.to_string(), String::new()),
    }
}

fn classify_api(
    endpoint: &str,
    method: Option<&str>,
    fields: &[(String, String)],
    input: Option<&str>,
    cwd: Option<&Path>,
) -> GhActs {
    let path = api_path(endpoint);
    if graphql_path(&path) {
        return graphql_acts(fields, input, cwd);
    }
    if !api_writes(method, !fields.is_empty() || input.is_some()) {
        return GhActs::default();
    }
    rest_acts(&path)
}

fn classify_api_cursor_ref(
    endpoint: &str,
    method: Option<&str>,
    fields: &[(String, String)],
    input: Option<&str>,
    cwd: Option<&Path>,
    opaque: bool,
) -> Option<String> {
    let has_body = !fields.is_empty() || input.is_some() || opaque;
    if !api_writes(method, has_body) {
        return None;
    }
    let path = api_path(endpoint);
    if graphql_path(&path) {
        return graphql_cursor_ref(fields, input, cwd, opaque);
    }
    rest_ref_write(endpoint, &path, method, fields, input, cwd, opaque)
}

/// gh's default is GET, and POST when a field or a body was given. An
/// explicit method wins, including GET with fields, which sends a query
/// string rather than a pull-request write.
fn api_writes(method: Option<&str>, has_body: bool) -> bool {
    match method {
        Some(method) => matches!(
            method.to_ascii_uppercase().as_str(),
            "POST" | "PUT" | "PATCH" | "DELETE"
        ),
        None => has_body,
    }
}

fn api_path(endpoint: &str) -> String {
    let endpoint = endpoint.split(['?', '#']).next().unwrap_or(endpoint);
    let path = if let Some(rest) = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))
    {
        rest.split_once('/').map(|(_, path)| path).unwrap_or("")
    } else {
        endpoint
    };
    path.trim_matches('/').to_string()
}

fn graphql_path(path: &str) -> bool {
    path == "graphql" || path.ends_with("/graphql")
}

fn rest_acts(path: &str) -> GhActs {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let n = parts.len();
    if n >= 3 && parts[n - 1] == "merge" && parts[n - 3] == "pulls" {
        return GhActs {
            open: false,
            merge: true,
        };
    }
    if parts.last() == Some(&"pulls") {
        return GhActs {
            open: true,
            merge: false,
        };
    }
    GhActs::default()
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum ForceFlag {
    #[default]
    Absent,
    False,
    True,
    Unknown,
}

fn merge_force(left: ForceFlag, right: ForceFlag) -> ForceFlag {
    use ForceFlag::{Absent, False, True, Unknown};
    match (left, right) {
        (True, _) | (_, True) => True,
        (Unknown, _) | (_, Unknown) => Unknown,
        (False, _) | (_, False) => False,
        (Absent, Absent) => Absent,
    }
}

fn force_literal(value: &str) -> ForceFlag {
    let value = value.trim();
    if value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("yes")
        || value.eq_ignore_ascii_case("on")
        || value == "1"
    {
        ForceFlag::True
    } else if value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("no")
        || value.eq_ignore_ascii_case("off")
        || value == "0"
    {
        ForceFlag::False
    } else if value.is_empty() {
        ForceFlag::Absent
    } else {
        ForceFlag::Unknown
    }
}

#[derive(Default)]
struct RefBody {
    refs: Vec<String>,
    force: ForceFlag,
    opaque: bool,
}

fn collect_ref_body_from_json(value: &Value, body: &mut RefBody) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "ref" {
                    if let Some(name) = value.as_str() {
                        body.refs.push(name.to_string());
                    } else {
                        body.opaque = true;
                    }
                } else if key == "force" {
                    let next = match value {
                        Value::Bool(v) => {
                            if *v {
                                ForceFlag::True
                            } else {
                                ForceFlag::False
                            }
                        }
                        Value::String(v) => force_literal(v),
                        _ => ForceFlag::Unknown,
                    };
                    body.force = merge_force(body.force, next);
                }
                collect_ref_body_from_json(value, body);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_ref_body_from_json(item, body);
            }
        }
        _ => {}
    }
}

fn read_ref_body(fields: &[(String, String)], input: Option<&str>, cwd: Option<&Path>) -> RefBody {
    let mut body = RefBody::default();
    for (key, value) in fields {
        if key == "ref" {
            body.refs.push(value.clone());
        } else if key == "force" {
            body.force = merge_force(body.force, force_literal(value));
        }
    }
    let Some(path) = input else {
        return body;
    };
    if path == "-" {
        body.opaque = true;
        return body;
    }
    let Some(text) = read_query(cwd, path) else {
        body.opaque = true;
        return body;
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => collect_ref_body_from_json(&value, &mut body),
        Err(_) => body.opaque = true,
    }
    body
}

fn git_refs_tail(path: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let at = parts
        .windows(2)
        .position(|window| window[0] == "git" && window[1] == "refs")?;
    Some(parts[at + 2..].to_vec())
}

fn rest_ref_write(
    endpoint: &str,
    path: &str,
    method: Option<&str>,
    fields: &[(String, String)],
    input: Option<&str>,
    cwd: Option<&Path>,
    opaque: bool,
) -> Option<String> {
    let has_body = !fields.is_empty() || input.is_some() || opaque;
    let method = method.map(|m| m.to_ascii_uppercase()).unwrap_or_else(|| {
        if has_body {
            "POST".into()
        } else {
            "GET".into()
        }
    });
    if let Some(why) = rest_other_ref(endpoint, path, &method, fields, input, cwd, opaque) {
        return Some(why);
    }
    let tail = git_refs_tail(path)?;
    let mut body = read_ref_body(fields, input, cwd);
    if opaque {
        body.opaque = true;
    }
    match method.as_str() {
        "DELETE" => Some("`gh api` can delete refs through /git/refs".into()),
        "POST" if tail.is_empty() => {
            if body.opaque || body.refs.is_empty() {
                return Some(format!(
                    "`gh api` can create refs through /git/refs and the destination could not be read; {ALLOW_RULE}"
                ));
            }
            body.refs
                .into_iter()
                .find_map(|name| match branch_permitted(&name) {
                    Err(why) => Some(why),
                    Ok(false) => Some(format!("`gh api` can create {name} through /git/refs")),
                    Ok(true) => None,
                })
        }
        "PATCH" if !tail.is_empty() => {
            if body.opaque {
                return Some(format!(
                    "`gh api` can force-update refs through /git/refs with an unread request body; {ALLOW_RULE}"
                ));
            }
            match body.force {
                ForceFlag::True => Some("`gh api` can force-update refs through /git/refs".into()),
                ForceFlag::Unknown => Some(
                    "`gh api` can force-update refs through /git/refs with an unread force value"
                        .into(),
                ),
                _ => {
                    let name = tail.join("/");
                    match branch_permitted(&name) {
                        Err(why) => Some(why),
                        Ok(false) => Some(format!(
                            "`gh api` can update {name} through /git/refs; {ALLOW_RULE}"
                        )),
                        Ok(true) => None,
                    }
                }
            }
        }
        _ => None,
    }
}

fn graphql_acts(fields: &[(String, String)], input: Option<&str>, cwd: Option<&Path>) -> GhActs {
    let (texts, opaque) = graphql_texts(fields, input, cwd);
    let mut acts = GhActs::default();
    for text in &texts {
        acts = acts.either(mutation_acts(text));
    }
    // The body is not on the line and could not be read. It may be any of
    // the mutations below, so it is not treated as a query.
    if opaque && !acts.any() {
        acts.open = true;
        acts.merge = true;
    }
    acts
}

fn graphql_texts(
    fields: &[(String, String)],
    input: Option<&str>,
    cwd: Option<&Path>,
) -> (Vec<String>, bool) {
    let mut texts = Vec::new();
    let mut opaque = false;
    for (key, value) in fields {
        if key != "query" {
            continue;
        }
        match value.strip_prefix('@') {
            Some("-" | "") => opaque = true,
            Some(path) => match read_query(cwd, path) {
                Some(body) => texts.push(body),
                None => opaque = true,
            },
            None => texts.push(value.clone()),
        }
    }
    if let Some(path) = input {
        if path == "-" {
            opaque = true;
        } else {
            match read_query(cwd, path) {
                Some(body) => texts.push(body),
                None => opaque = true,
            }
        }
    }
    (texts, opaque)
}

fn quoted_strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '"' {
            i += 1;
            continue;
        }
        i += 1;
        let mut value = String::new();
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' {
                i += 1;
                if i < chars.len() {
                    value.push(chars[i]);
                    i += 1;
                }
                continue;
            }
            if c == '"' {
                i += 1;
                break;
            }
            value.push(c);
            i += 1;
        }
        out.push(value);
    }
    out
}

fn force_in_text(text: &str) -> ForceFlag {
    let mut force = ForceFlag::Absent;
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut at = 0usize;
    while let Some(found) = text[at..].find("force") {
        let start = at + found;
        let before = text[..start].chars().next_back();
        let end = start + "force".len();
        let after = text[end..].chars().next();
        if before.is_some_and(ident) || after.is_some_and(ident) {
            at = end;
            continue;
        }
        let mut rest = text[end..].chars().peekable();
        while rest.peek().is_some_and(|c| c.is_whitespace()) {
            rest.next();
        }
        if rest.peek() != Some(&':') {
            at = end;
            continue;
        }
        rest.next();
        while rest.peek().is_some_and(|c| c.is_whitespace()) {
            rest.next();
        }
        if rest.peek() == Some(&'$') {
            force = merge_force(force, ForceFlag::Unknown);
            at = end;
            continue;
        }
        let mut literal = String::new();
        while rest
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        {
            literal.push(rest.next().unwrap_or_default());
        }
        let next = force_literal(&literal);
        force = merge_force(force, next);
        at = end;
    }
    force
}

fn rest_other_ref(
    endpoint: &str,
    path: &str,
    method: &str,
    fields: &[(String, String)],
    input: Option<&str>,
    cwd: Option<&Path>,
    opaque: bool,
) -> Option<String> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let ends = |name: &str| parts.last() == Some(&name);
    if method == "POST" && ends("releases") {
        return Some(format!("`gh api` can create a release tag; {ALLOW_RULE}"));
    }
    if method == "POST" && ends("rename") && parts.contains(&"branches") {
        return Some(format!(
            "`gh api` can rename a branch, which deletes the old ref; {ALLOW_RULE}"
        ));
    }
    if method == "POST" && ends("merges") {
        return named_branch_field(fields, input, cwd, opaque, "base", "`gh api` merge");
    }
    if method == "PUT" && parts.contains(&"contents") {
        if let Some(branch) =
            query_param(endpoint, "branch").or_else(|| query_param(endpoint, "ref"))
        {
            return judge_named_branch(&branch, "`gh api` contents");
        }
        return named_branch_field(fields, input, cwd, opaque, "branch", "`gh api` contents");
    }
    None
}

fn named_branch_field(
    fields: &[(String, String)],
    input: Option<&str>,
    cwd: Option<&Path>,
    opaque: bool,
    key: &str,
    what: &str,
) -> Option<String> {
    match body_string(fields, input, cwd, key) {
        Some(name) if !unreadable_token(&name) => judge_named_branch(&name, what),
        _ if opaque => Some(format!(
            "{what} does not show the branch it writes; {ALLOW_RULE}"
        )),
        Some(name) => Some(format!(
            "{what} names `{name}`, which is only known once the shell expands it; {ALLOW_RULE}"
        )),
        None => Some(format!(
            "{what} does not show the branch it writes; {ALLOW_RULE}"
        )),
    }
}

fn judge_named_branch(name: &str, what: &str) -> Option<String> {
    match branch_permitted(name) {
        Err(why) => Some(why),
        Ok(true) => None,
        Ok(false) => Some(format!("{what} updates {name}; {ALLOW_RULE}")),
    }
}

fn body_string(
    fields: &[(String, String)],
    input: Option<&str>,
    cwd: Option<&Path>,
    key: &str,
) -> Option<String> {
    if let Some((_, value)) = fields.iter().rev().find(|(k, _)| k == key) {
        if !value.is_empty() {
            return Some(value.clone());
        }
    }
    let path = input?;
    if path == "-" {
        return None;
    }
    let text = read_query(cwd, path)?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn query_param(endpoint: &str, key: &str) -> Option<String> {
    let query = endpoint.split_once('?')?.1.split('#').next()?;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=')?;
        if name == key && !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn graphql_cursor_ref(
    fields: &[(String, String)],
    input: Option<&str>,
    cwd: Option<&Path>,
    extra_opaque: bool,
) -> Option<String> {
    let (texts, opaque) = graphql_texts(fields, input, cwd);
    if extra_opaque || (opaque && texts.is_empty()) {
        return Some(format!(
            "`gh api graphql` body could write refs and could not be read; {ALLOW_RULE}"
        ));
    }
    for text in &texts {
        let code = without_strings(text);
        if has_ident(&code, "deleteRef") {
            return Some("`gh api graphql` can delete refs with deleteRef".into());
        }
        if has_ident(&code, "createRef") {
            if let Some(why) = graphql_destinations(text, "name", "createRef") {
                return Some(why);
            }
        }
        if has_ident(&code, "createCommitOnBranch") {
            if let Some(why) = graphql_destinations(text, "branchName", "createCommitOnBranch") {
                return Some(why);
            }
        }
        if has_ident(&code, "mergeBranch") {
            if let Some(why) = graphql_destinations(text, "base", "mergeBranch") {
                return Some(why);
            }
        }
        if has_ident(&code, "updateRef") || has_ident(&code, "updateRefs") {
            match force_in_text(&code) {
                ForceFlag::True => {
                    return Some("`gh api graphql` can force-update refs with force: true".into());
                }
                ForceFlag::Unknown => {
                    return Some(
                        "`gh api graphql` can force-update refs with an unread force value".into(),
                    );
                }
                _ => {}
            }
            if !has_literal_oid(&code) {
                return Some(format!(
                    "`gh api graphql` updateRefs shows no oid, so the ref it writes cannot be read; {ALLOW_RULE}"
                ));
            }
            if let Some(why) = graphql_destinations(text, "name", "updateRef") {
                return Some(why);
            }
        }
    }
    if opaque {
        return Some(format!(
            "`gh api graphql` body could write refs and could not be read; {ALLOW_RULE}"
        ));
    }
    None
}

fn graphql_destinations(text: &str, key: &str, what: &str) -> Option<String> {
    let fields = string_fields(text, key);
    if fields.is_empty() {
        return Some(format!(
            "`gh api graphql` {what} does not name a branch; {ALLOW_RULE}"
        ));
    }
    for field in fields {
        match field {
            FieldRead::Unknown => {
                return Some(format!(
                    "`gh api graphql` {what} names a branch the line does not show; {ALLOW_RULE}"
                ));
            }
            FieldRead::Value(name) => match branch_permitted(&name) {
                Err(why) => return Some(why),
                Ok(false) => {
                    return Some(format!(
                        "`gh api graphql` {what} updates {name}; {ALLOW_RULE}"
                    ));
                }
                Ok(true) => {}
            },
        }
    }
    None
}

enum FieldRead {
    Unknown,
    Value(String),
}

fn string_fields(text: &str, key: &str) -> Vec<FieldRead> {
    let mut out = Vec::new();
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut at = 0usize;
    while let Some(found) = text[at..].find(key) {
        let start = at + found;
        let before = text[..start].chars().next_back();
        let end = start + key.len();
        let after = text[end..].chars().next();
        if before.is_some_and(ident) || after.is_some_and(ident) {
            at = end;
            continue;
        }
        let rest = text[end..].trim_start();
        if !rest.starts_with(':') {
            at = end;
            continue;
        }
        let rest = rest[1..].trim_start();
        if rest.starts_with('$') || rest.starts_with('`') {
            out.push(FieldRead::Unknown);
        } else if rest.starts_with('"') {
            let value = quoted_strings(rest).into_iter().next().unwrap_or_default();
            out.push(FieldRead::Value(value));
        } else {
            out.push(FieldRead::Unknown);
        }
        at = end;
    }
    out
}

/// Query text with each double-quoted string emptied, so an identifier
/// inside a comment is not a mutation. Escapes keep their character.
fn without_strings(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '"' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        out.push('"');
        out.push('"');
        i += 1;
        while i < chars.len() {
            if chars[i] == '\\' {
                i += 1;
                if i < chars.len() {
                    i += 1;
                }
                continue;
            }
            if chars[i] == '"' {
                i += 1;
                break;
            }
            i += 1;
        }
    }
    out
}

/// An `oid: "..."` argument, not `afterOid` or `beforeOid`, and not a
/// variable. `updateRefs` with only `afterOid` has no literal oid.
fn has_literal_oid(text: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut at = 0usize;
    while let Some(found) = text[at..].find("oid") {
        let start = at + found;
        let before = text[..start].chars().next_back();
        let end = start + "oid".len();
        let after = text[end..].chars().next();
        if before.is_some_and(ident) || after.is_some_and(ident) {
            at = end;
            continue;
        }
        let rest = text[end..].trim_start();
        if let Some(rest) = rest.strip_prefix(':') {
            if rest.trim_start().starts_with('"') {
                return true;
            }
        }
        at = end;
    }
    false
}

fn read_query(cwd: Option<&Path>, path: &str) -> Option<String> {
    if path.is_empty() || path.contains('\0') {
        return None;
    }
    let path = Path::new(path);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let base = cwd
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())?;
        base.join(path)
    };
    std::fs::read_to_string(full).ok()
}

fn mutation_acts(text: &str) -> GhActs {
    let code = without_strings(text);
    GhActs {
        open: OPEN_MUTATIONS.iter().any(|name| has_ident(&code, name)),
        merge: MERGE_MUTATIONS.iter().any(|name| has_ident(&code, name)),
    }
}

fn has_ident(hay: &str, ident: &str) -> bool {
    hay.match_indices(ident).any(|(at, _)| {
        let before = hay[..at].chars().next_back();
        let after = hay[at + ident.len()..].chars().next();
        let ident_char = |c: char| c.is_ascii_alphanumeric() || c == '_';
        !before.is_some_and(ident_char) && !after.is_some_and(ident_char)
    })
}

/// The next word of a gh command that is not a flag or a flag's value.
fn gh_word<'a>(words: &mut impl Iterator<Item = &'a str>) -> Option<&'a str> {
    while let Some(word) = words.next() {
        if word == "-R" || word == "--repo" {
            words.next();
        } else if !(word.starts_with('-') && word.len() > 1) {
            return Some(word);
        }
    }
    None
}

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
    lower.contains("git push")
        || lower.contains("git send-pack")
        || lower.contains("gh api")
        || lower.contains("gh release create")
        || lower.contains("gh repo sync")
        || ((lower.contains("curl") || lower.contains("wget") || lower.contains("http"))
            && lower.contains("github.com"))
}

/// A hidden host counts as GitHub: the line does not show where it goes.
/// A visible other host does not, so cargo and npm stay allowed.
fn http_targets_github(words: &[String]) -> bool {
    let cmd = words
        .first()
        .map(|word| word.rsplit('/').next().unwrap_or(word.as_str()))
        .unwrap_or("");
    let proto = if cmd == "curl" {
        curl_proto_default(words)
    } else {
        None
    };
    let httpie = cmd == "http" || cmd == "https";
    let mut github = false;
    let mut other = false;
    let mut hidden = false;
    for word in words.iter().skip(1) {
        if word.starts_with('-') {
            continue;
        }
        if let Some(host) = literal_http_host(word) {
            if is_github_host(&host) || address_host(&host) {
                github = true;
            } else {
                other = true;
            }
            continue;
        }
        if httpie || proto.is_some() {
            if let Some(host) = schemeless_host(word) {
                if is_github_host(&host) || address_host(&host) {
                    github = true;
                } else {
                    other = true;
                }
                continue;
            }
        }
        // `$URL` and `https://$HOST/...` do not name a host. A header that
        // happens to hold a token has a colon or a space and is not a URL.
        if unreadable_token(word) && (word.contains("://") || !word.contains([' ', ':', '='])) {
            hidden = true;
        }
    }
    // A visible host that is not GitHub stays out of this allowlist, even
    // when the path or a header still has a substitution.
    github || (hidden && !other)
}

fn curl_proto_default(words: &[String]) -> Option<String> {
    let mut i = 1usize;
    while i < words.len() {
        let word = words[i].as_str();
        if let Some(value) = word.strip_prefix("--proto-default=") {
            return (!value.is_empty()).then(|| value.to_string());
        }
        if word == "--proto-default" {
            return words.get(i + 1).cloned();
        }
        i += 1;
    }
    None
}

fn literal_http_host(word: &str) -> Option<String> {
    host_in_url(word)
}

fn schemeless_host(word: &str) -> Option<String> {
    if word.contains("://") || word.contains('=') || word.contains([' ', '\t']) {
        return None;
    }
    let host = host_in_url(&format!("https://{word}"))?;
    host.contains('.').then_some(host)
}

/// Authority host. The scheme's case does not matter. Userinfo is the last
/// `@` before the path, so a token is not the host and a port stays with
/// the host. One trailing dot is not a different host.
fn host_in_url(word: &str) -> Option<String> {
    let word = word
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>');
    let idx = word.find("://")?;
    let scheme = &word[..idx];
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let rest = &word[idx + 3..];
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    if auth.is_empty() {
        return None;
    }
    let hostport = auth.rsplit_once('@').map(|(_, host)| host).unwrap_or(auth);
    if hostport.is_empty() || unreadable_token(hostport) {
        return None;
    }
    let host = strip_host_port(hostport).trim_end_matches('.');
    if host.is_empty() || unreadable_token(host) {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn strip_host_port(hostport: &str) -> &str {
    if let Some(inner) = hostport.strip_prefix('[') {
        return inner.split(']').next().unwrap_or(inner);
    }
    if let Some((host, port)) = hostport.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            return host;
        }
    }
    hostport
}

fn is_github_host(host: &str) -> bool {
    host == "github.com"
        || host == "github.ghe.com"
        || host == "ghcr.io"
        || host == "githubusercontent.com"
        || host.ends_with(".github.com")
        || host.ends_with(".github.ghe.com")
        || host.ends_with(".ghcr.io")
        || host.ends_with(".githubusercontent.com")
        || host.ends_with(".pkg.github.com")
}

/// An IP address in a form curl and wget take: dotted, one number, hex
/// parts, or IPv6. A DNS name does not end in a number, so a peer named
/// this way may be GitHub behind a `Host:` header.
fn address_host(host: &str) -> bool {
    if host.contains(':') {
        return true;
    }
    let last = host.rsplit('.').next().unwrap_or(host);
    let hex = last
        .strip_prefix("0x")
        .is_some_and(|digits| digits.chars().all(|c| c.is_ascii_hexdigit()));
    !last.is_empty() && (hex || last.chars().all(|c| c.is_ascii_digit()))
}

fn http_ref_write(
    words: &[String],
    env: &[(String, String)],
    cwd: Option<&Path>,
) -> Option<String> {
    let cmd = words.first()?.rsplit('/').next()?;
    if !matches!(cmd, "curl" | "wget" | "http" | "https") {
        return None;
    }
    let (shape, config_github) = http_shape(cmd, words, env, cwd);
    // A host that is not GitHub is left alone, including a clustered
    // DELETE. Cargo and npm talk to those hosts. A config file can name
    // the host when the command line does not.
    if !http_targets_github(words) && !config_github {
        return None;
    }
    match shape {
        HttpShape::Get => None,
        HttpShape::Write | HttpShape::Opaque => Some(format!(
            "an HTTP client can write refs on GitHub and the destination is not limited to this seat; {ALLOW_RULE}"
        )),
    }
}

enum HttpShape {
    Get,
    Write,
    Opaque,
}

fn http_shape(
    cmd: &str,
    words: &[String],
    env: &[(String, String)],
    cwd: Option<&Path>,
) -> (HttpShape, bool) {
    match cmd {
        "curl" => curl_shape(words, env, cwd),
        "wget" => wget_shape(words, env, cwd),
        "http" | "https" => (httpie_shape(words), false),
        _ => (HttpShape::Opaque, false),
    }
}

/// How curl reads an option, on the command line and in a curlrc alike.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CurlOpt {
    /// Changes neither the request nor where it goes.
    Flag,
    /// Takes a value that changes neither.
    Value,
    /// Takes a proxy, a resolver, a socket, or an address map, so the URL
    /// host is not where the request goes.
    Route,
    /// Judged by name in `apply_curl_opt`. True when it takes a value.
    Own(bool),
}

impl CurlOpt {
    fn takes_value(self) -> bool {
        matches!(self, Self::Value | Self::Route | Self::Own(true))
    }
}

/// Curl options by kind, each `long` or `long/short`. A name that is a
/// prefix of a listed name and is itself a curl option must be listed
/// too, or its abbreviation would resolve to the wrong option.
const CURL_FLAG: &str = "\
    silent/s show-error/S fail/f fail-with-body insecure/k verbose/v compressed \
    progress-bar/# remote-name/O remote-name-all remote-header-name/J http1.1 http2 \
    path-as-is no-progress-meter create-dirs ipv4/4 ipv6/6 retry-all-errors include/i \
    disable/q use-ascii/B list-only/l append/a help/h version/V manual/M \
    junk-session-cookies/j proxytunnel/p location/L location-trusted";
const CURL_VALUE: &str = "\
    output/o retry retry-delay retry-max-time connect-timeout max-time/m max-redirs \
    user-agent/A referer/e cookie/b cookie-jar/c proxy-user/U cacert capath cert/E key \
    speed-limit/Y speed-time/y limit-rate output-dir write-out/w user/u oauth2-bearer \
    dump-header/D continue-at/C range/r time-cond/z telnet-option/t ftp-port/P quote/Q \
    proto proto-redir proto-default";
const CURL_ROUTE: &str = "\
    proxy/x preproxy proxy1.0 socks4 socks4a socks5 socks5-hostname resolve connect-to \
    doh-url dns-servers unix-socket abstract-unix-socket alt-svc";
const CURL_OWN_FLAG: &str = "get/G head/I globoff/g netrc/n netrc-optional next/:";
const CURL_OWN_VALUE: &str = "\
    request/X data/d data-ascii data-binary data-raw data-urlencode json upload-file/T \
    form/F form-string header/H url config/K netrc-file variable";

#[derive(Clone, Copy)]
struct CurlHit {
    long: &'static str,
    kind: CurlOpt,
    /// `--no-` before the name.
    off: bool,
}

fn curl_opts() -> impl Iterator<Item = (&'static str, Option<char>, CurlOpt)> {
    [
        (CURL_FLAG, CurlOpt::Flag),
        (CURL_VALUE, CurlOpt::Value),
        (CURL_ROUTE, CurlOpt::Route),
        (CURL_OWN_FLAG, CurlOpt::Own(false)),
        (CURL_OWN_VALUE, CurlOpt::Own(true)),
    ]
    .into_iter()
    .flat_map(|(names, kind)| {
        names.split_ascii_whitespace().map(move |entry| {
            let (long, short) = entry.split_once('/').unwrap_or((entry, ""));
            (long, short.chars().next(), kind)
        })
    })
}

fn curl_short_opt(c: char) -> Option<CurlHit> {
    curl_opts()
        .find(|(_, short, _)| *short == Some(c))
        .map(|(long, _, kind)| CurlHit {
            long,
            kind,
            off: false,
        })
}

/// The option curl takes `name` for. Curl takes any prefix that names one
/// option, and `no-` before a flag turns it off. curl 8.3 fills
/// `{{name}}` in an `--expand-` option from a `--variable`, so both are
/// judged as `variable`.
fn curl_long_opt(name: &str) -> Option<CurlHit> {
    let name = name.to_ascii_lowercase();
    if name.starts_with("expand-") {
        return curl_prefix_opt("variable");
    }
    if let Some(hit) = curl_prefix_opt(&name) {
        return Some(hit);
    }
    let hit = curl_prefix_opt(name.strip_prefix("no-")?)?;
    (!hit.kind.takes_value()).then_some(CurlHit { off: true, ..hit })
}

fn curl_prefix_opt(word: &str) -> Option<CurlHit> {
    if word.is_empty() {
        return None;
    }
    let hits: Vec<_> = curl_opts()
        .filter(|(long, _, _)| long.starts_with(word))
        .collect();
    let (long, _, kind) = match hits.iter().find(|(long, _, _)| *long == word) {
        Some(hit) => *hit,
        None if hits.len() == 1 => hits[0],
        None => return None,
    };
    Some(CurlHit {
        long,
        kind,
        off: false,
    })
}

/// Method, body and host learned from a curl or wget config file.
#[derive(Default)]
struct Tune {
    method: Option<String>,
    body: bool,
    upload: bool,
    query: bool,
    /// A config line named a GitHub host.
    github: bool,
    /// A config that applies could not be read.
    unreadable: bool,
    /// URLs curl will request, from the command line and its configs.
    urls: Vec<String>,
    /// `-g`: curl takes `{}[]` in a URL literally.
    globoff: bool,
}

fn curl_shape(words: &[String], env: &[(String, String)], cwd: Option<&Path>) -> (HttpShape, bool) {
    let mut tune = Tune::default();
    let mut seen = HashSet::new();
    let mut opaque = false;
    // `-q` and `--disable` skip the default curlrc only as the first
    // argument. A later one still loads the file, which can set the method.
    // The command line is read after an opaque file too: it can still
    // move the target.
    if !curl_rc_skipped(words) {
        opaque |= curl_default_files(env, cwd, &mut tune, &mut seen).is_some();
    }
    let mut i = 1usize;
    while i < words.len() {
        let word = words[i].as_str();
        i += 1;
        if word == "--" {
            tune.urls.extend(words[i..].iter().cloned());
            break;
        }
        if let Some(long) = word.strip_prefix("--") {
            let (name, attached) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (long, None),
            };
            let Some(hit) = curl_long_opt(name) else {
                tune.github = true;
                continue;
            };
            let value = if !hit.kind.takes_value() {
                None
            } else if let Some(value) = attached {
                Some(value.to_string())
            } else {
                i += 1;
                words.get(i - 1).cloned()
            };
            opaque |= apply_curl_opt(hit, value.as_deref(), cwd, &mut tune, &mut seen).is_some();
            continue;
        }
        let Some(cluster) = word.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
            tune.urls.push(word.to_string());
            continue;
        };
        for (at, c) in cluster.char_indices() {
            let Some(hit) = curl_short_opt(c) else {
                tune.github = true;
                opaque = true;
                break;
            };
            if !hit.kind.takes_value() {
                opaque |= apply_curl_opt(hit, None, cwd, &mut tune, &mut seen).is_some();
                continue;
            }
            let rest = &cluster[at + c.len_utf8()..];
            let value = if rest.is_empty() {
                i += 1;
                words.get(i - 1).cloned()
            } else {
                Some(rest.to_string())
            };
            opaque |= apply_curl_opt(hit, value.as_deref(), cwd, &mut tune, &mut seen).is_some();
            break;
        }
    }
    let shape = if opaque {
        HttpShape::Opaque
    } else {
        finish_http(tune.method.as_deref(), tune.body, tune.query, tune.upload)
    };
    (shape, curl_config_github(&tune))
}

/// A config that applies and could not be read may add any URL, whatever
/// host the command line shows.
fn curl_config_github(tune: &Tune) -> bool {
    tune.github
        || tune.unreadable
        || tune
            .urls
            .iter()
            .any(|url| curl_url_github(url, tune.globoff))
}

/// Curl skips its default curlrc when `-q` or `--disable` is argv[1].
/// `-qs` counts: curl compares the first argument with the prefix `-q`.
fn curl_rc_skipped(words: &[String]) -> bool {
    match words.get(1).map(String::as_str) {
        Some("--disable") => true,
        Some(word) => word.starts_with("-q"),
        None => false,
    }
}

fn finish_http(method: Option<&str>, body: bool, query: bool, upload: bool) -> HttpShape {
    // `-G` moves `-d` into the query string. An upload (`-T`, `-F`) stays a body.
    if upload {
        return HttpShape::Write;
    }
    match method {
        Some("GET" | "HEAD") if !body || query => HttpShape::Get,
        Some("GET" | "HEAD") => HttpShape::Write,
        Some(_) => HttpShape::Write,
        None if body && !query => HttpShape::Write,
        None => HttpShape::Get,
    }
}

fn prefixed_or_process(env: &[(String, String)], key: &str) -> Option<String> {
    if let Some((_, value)) = env.iter().rev().find(|(name, _)| name == key) {
        return Some(value.clone());
    }
    std::env::var(key).ok()
}

fn curl_named_file(
    value: &str,
    cwd: Option<&Path>,
    tune: &mut Tune,
    seen: &mut HashSet<PathBuf>,
) -> Option<HttpShape> {
    if value.is_empty() || value == "-" {
        tune.unreadable = true;
        return Some(HttpShape::Opaque);
    }
    read_curl_file(&resolve_cfg(value, cwd), cwd, tune, seen, true)
}

/// Default curlrc search. `CURL_HOME` replaces the others: a missing file
/// there is opaque, because the variable says a config applies and it
/// could not be read. Otherwise the first file that exists, in curl's
/// order, is the one curl would load: the XDG file, then `HOME`, then the
/// Windows `USERPROFILE`, `APPDATA`, and `USERPROFILE\Application Data`.
/// Windows curl also takes `_curlrc` after `.curlrc` in each. The Windows
/// variables are read wherever they are set, so a seat that sets one is
/// judged as Windows curl would be.
fn curl_default_files(
    env: &[(String, String)],
    cwd: Option<&Path>,
    tune: &mut Tune,
    seen: &mut HashSet<PathBuf>,
) -> Option<HttpShape> {
    let dir_of = |key: &str| {
        prefixed_or_process(env, key)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = dir_of("CURL_HOME") {
        let found = first_curl_file(&curl_rc_names(&dir, cfg!(windows)), cwd, tune, seen);
        return found.unwrap_or_else(|| {
            tune.unreadable = true;
            Some(HttpShape::Opaque)
        });
    }
    let mut paths = Vec::new();
    if let Some(xdg) = dir_of("XDG_CONFIG_HOME") {
        paths.push(xdg.join("curlrc"));
    }
    if let Some(home) = dir_of("HOME") {
        paths.extend(curl_rc_names(&home, cfg!(windows)));
    }
    if let Some(profile) = dir_of("USERPROFILE") {
        paths.extend(curl_rc_names(&profile, true));
    }
    if let Some(appdata) = dir_of("APPDATA") {
        paths.extend(curl_rc_names(&appdata, true));
    }
    if let Some(profile) = dir_of("USERPROFILE") {
        paths.extend(curl_rc_names(&profile.join("Application Data"), true));
    }
    first_curl_file(&paths, cwd, tune, seen).flatten()
}

fn curl_rc_names(dir: &Path, windows: bool) -> Vec<PathBuf> {
    let mut names = vec![dir.join(".curlrc")];
    if windows {
        names.push(dir.join("_curlrc"));
    }
    names
}

/// The judgement of the first of `paths` that exists, or None when none
/// does.
fn first_curl_file(
    paths: &[PathBuf],
    cwd: Option<&Path>,
    tune: &mut Tune,
    seen: &mut HashSet<PathBuf>,
) -> Option<Option<HttpShape>> {
    for path in paths {
        if let Some(shape) = read_curl_file(path, cwd, tune, seen, false) {
            return Some(Some(shape));
        }
        if tune.unreadable || seen.contains(&cfg_key(path)) {
            return Some(None);
        }
    }
    None
}

/// `required` makes a missing file opaque. A default curlrc that is simply
/// absent does not apply.
fn read_curl_file(
    path: &Path,
    cwd: Option<&Path>,
    tune: &mut Tune,
    seen: &mut HashSet<PathBuf>,
    required: bool,
) -> Option<HttpShape> {
    if seen.len() >= 8 {
        tune.unreadable = true;
        return Some(HttpShape::Opaque);
    }
    let key = cfg_key(path);
    if !seen.insert(key.clone()) {
        return None;
    }
    match open_cfg(&key) {
        Ok(None) if !required => {
            seen.remove(&key);
            None
        }
        Ok(None) | Err(()) => {
            tune.unreadable = true;
            Some(HttpShape::Opaque)
        }
        Ok(Some(text)) => apply_curl_text(&text, cwd, tune, seen),
    }
}

fn apply_curl_text(
    text: &str,
    cwd: Option<&Path>,
    tune: &mut Tune,
    seen: &mut HashSet<PathBuf>,
) -> Option<HttpShape> {
    let mut opaque = false;
    for line in text.lines() {
        match curl_cfg_line(line) {
            CfgLine::Blank => {}
            CfgLine::Bad => {
                opaque = true;
                tune.github = true;
            }
            CfgLine::Item { name, value } => {
                let mut chars = name.chars();
                let hit = match (chars.next(), chars.next()) {
                    (Some(c), None) => curl_short_opt(c),
                    _ => curl_long_opt(&name),
                };
                // An option this seat does not know, or a line spilled
                // from the one before, may name the target.
                let Some(hit) = hit else {
                    tune.github = true;
                    opaque = true;
                    continue;
                };
                opaque |= apply_curl_opt(hit, value.as_deref(), cwd, tune, seen).is_some();
            }
        }
    }
    opaque.then_some(HttpShape::Opaque)
}

enum CfgLine {
    Blank,
    Bad,
    Item { name: String, value: Option<String> },
}

fn curl_cfg_line(line: &str) -> CfgLine {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return CfgLine::Blank;
    }
    if line.starts_with('-') {
        return curl_dashed_line(line);
    }
    let mut name_end = line.len();
    for (i, c) in line.char_indices() {
        if c == '=' || c == ':' || c.is_whitespace() {
            name_end = i;
            break;
        }
    }
    let name = &line[..name_end];
    if name.is_empty() {
        return CfgLine::Bad;
    }
    let rest = line[name_end..].trim_start();
    let rest = if let Some(rest) = rest.strip_prefix('=').or_else(|| rest.strip_prefix(':')) {
        rest.trim()
    } else {
        rest
    };
    if rest.is_empty() {
        return CfgLine::Item {
            name: name.to_ascii_lowercase(),
            value: None,
        };
    }
    match unquote(rest) {
        Some(value) => CfgLine::Item {
            name: name.to_ascii_lowercase(),
            value: Some(value),
        },
        None => CfgLine::Bad,
    }
}

fn curl_dashed_line(line: &str) -> CfgLine {
    let long = line.starts_with("--");
    let body = if long { &line[2..] } else { &line[1..] };
    if body.is_empty() {
        return CfgLine::Bad;
    }
    if !long {
        let mut chars = body.chars();
        let Some(flag) = chars.next() else {
            return CfgLine::Bad;
        };
        let attached = chars.as_str().trim();
        let value = if attached.is_empty() {
            None
        } else {
            match unquote(attached) {
                Some(value) => Some(value),
                None => return CfgLine::Bad,
            }
        };
        return CfgLine::Item {
            name: flag.to_string(),
            value,
        };
    }
    let (name, raw) = if let Some((name, value)) = body.split_once('=') {
        (name, Some(value))
    } else if let Some((name, value)) = body.split_once(char::is_whitespace) {
        (name, Some(value.trim()))
    } else {
        (body, None)
    };
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return CfgLine::Bad;
    }
    let value = match raw {
        None | Some("") => None,
        Some(value) => match unquote(value.trim()) {
            Some(value) => Some(value),
            None => return CfgLine::Bad,
        },
    };
    CfgLine::Item {
        name: name.to_ascii_lowercase(),
        value,
    }
}

fn unquote(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if !raw.starts_with('"') {
        return Some(raw.to_string());
    }
    let mut out = String::new();
    let mut chars = raw[1..].chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('v') => out.push('\u{0b}'),
                Some(other) => out.push(other),
                None => return None,
            }
            continue;
        }
        if c == '"' {
            return Some(out);
        }
        out.push(c);
    }
    None
}

fn apply_curl_opt(
    hit: CurlHit,
    value: Option<&str>,
    cwd: Option<&Path>,
    tune: &mut Tune,
    seen: &mut HashSet<PathBuf>,
) -> Option<HttpShape> {
    let value = value.unwrap_or("");
    // `--no-globoff` turns globbing back on, and `--no-get` can turn a
    // query back into a body.
    if hit.off {
        if hit.long == "globoff" {
            tune.globoff = false;
            return None;
        }
        return (hit.kind != CurlOpt::Flag).then_some(HttpShape::Opaque);
    }
    match hit.kind {
        CurlOpt::Flag | CurlOpt::Value => return None,
        CurlOpt::Route => {
            tune.github = true;
            return None;
        }
        CurlOpt::Own(_) => {}
    }
    match hit.long {
        "request" if value.is_empty() => return Some(HttpShape::Opaque),
        "request" => tune.method = Some(value.to_ascii_uppercase()),
        "head" => tune.method = Some("HEAD".to_string()),
        "get" => tune.query = true,
        "globoff" => tune.globoff = true,
        "data" | "data-ascii" | "data-binary" | "data-raw" | "data-urlencode" | "json"
        | "upload-file" | "form" | "form-string" | "header"
            if value.starts_with('@') =>
        {
            return Some(HttpShape::Opaque);
        }
        "data" | "data-ascii" | "data-binary" | "data-raw" | "data-urlencode" | "json" => {
            tune.body = true;
        }
        "upload-file" | "form" | "form-string" => tune.upload = true,
        "header" if header_sets_host(value) => tune.github = true,
        "url" => tune.urls.push(value.to_string()),
        "config" => return curl_named_file(value, cwd, tune, seen),
        "netrc" | "netrc-optional" | "netrc-file" | "next" => return Some(HttpShape::Opaque),
        "variable" => {
            tune.github = true;
            return Some(HttpShape::Opaque);
        }
        _ => {}
    }
    None
}

/// A `Host:` header sends the request to whatever host it names on the
/// peer the URL reaches.
fn header_sets_host(value: &str) -> bool {
    value
        .trim_start()
        .get(..5)
        .is_some_and(|name| name.eq_ignore_ascii_case("host:"))
}

/// Whether a URL curl requests may reach GitHub: its host is GitHub, an
/// address, or a name the shell has not expanded yet. Curl takes a word
/// without a scheme as a URL. Unless `-g` is given, curl expands `{a,b}`
/// and `[1-3]` into several URLs, so `git{hub,lab}.com` is two hosts.
fn curl_url_github(url: &str, globoff: bool) -> bool {
    let url = url
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>');
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    if unreadable_token(auth) || auth.contains("{{") {
        return true;
    }
    if !globoff && auth.contains(['{', '}', '[', ']']) {
        return true;
    }
    let hostport = auth.rsplit_once('@').map_or(auth, |(_, host)| host);
    let host = strip_host_port(hostport)
        .trim_end_matches('.')
        .to_ascii_lowercase();
    is_github_host(&host) || address_host(&host)
}

fn resolve_cfg(value: &str, cwd: Option<&Path>) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        cwd.map(|dir| dir.join(path))
            .unwrap_or_else(|| PathBuf::from(value))
    }
}

fn cfg_key(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Only a missing path is no config. A symlink is followed, as curl and
/// wget follow it. What it reaches must be a regular file this user or
/// root owns; a directory, a fifo, a device, or another user's file is
/// opaque. `O_NONBLOCK` keeps a fifo from waiting for a writer, and
/// `fstat` judges what was opened, not what the path named a moment ago.
fn open_cfg(path: &Path) -> Result<Option<String>, ()> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    // SAFETY: geteuid has no arguments and only reads the process identity.
    let me = unsafe { libc::geteuid() };
    let owned = |meta: &std::fs::Metadata| meta.uid() == me || meta.uid() == 0;
    if !file
        .metadata()
        .is_ok_and(|meta| meta.is_file() && owned(&meta))
    {
        return Err(());
    }
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|_| ())?;
    Ok(Some(text))
}

fn wget_shape(words: &[String], env: &[(String, String)], cwd: Option<&Path>) -> (HttpShape, bool) {
    let mut tune = Tune::default();
    // `--no-config` before any `--config` reads nothing. A named
    // `--config` replaces the system and user files. Otherwise both
    // the system wgetrc and the user file apply, and either can set
    // the method. The command line is read after an opaque file too.
    let opaque = wget_files(words, env, cwd, &mut tune).is_some();
    let mut method = tune.method.clone();
    let mut body = tune.body;
    for (name, value) in wget_args(words) {
        match name {
            "method" => method = Some(value.to_ascii_uppercase()),
            "execute" => note_wget_exec(&value, &mut method, &mut body, &mut tune.github),
            "input-file" | "input-metalink" | "base" => tune.github = true,
            "header" if header_sets_host(&value) => tune.github = true,
            "post-data" | "post-file" | "body-data" | "body-file" => body = true,
            _ => {}
        }
    }
    let shape = if opaque {
        HttpShape::Opaque
    } else {
        finish_http(method.as_deref(), body, false, false)
    };
    (shape, curl_config_github(&tune))
}

/// Wget options the guard reads, each `long` or `long/short`. Wget takes
/// any prefix that names one option, so a wget option that is a prefix
/// of one listed here must be listed too.
const WGET_READ: &str = "\
    method execute/e input-file/i input-metalink base/B config no-config header post-data \
    post-file body-data body-file";
/// Wget shorts that take a value the guard does not read. `-n` takes the
/// letters after it.
const WGET_SHORT_VALUE: &str = "oatOTwQPUlARDIXn";

fn wget_opts() -> impl Iterator<Item = (&'static str, Option<char>)> {
    WGET_READ.split_ascii_whitespace().map(|entry| {
        let (long, short) = entry.split_once('/').unwrap_or((entry, ""));
        (long, short.chars().next())
    })
}

fn wget_long_name(word: &str) -> Option<&'static str> {
    let hits: Vec<_> = wget_opts()
        .map(|(long, _)| long)
        .filter(|long| !word.is_empty() && long.starts_with(word))
        .collect();
    match hits.iter().find(|long| **long == word) {
        Some(long) => Some(long),
        None if hits.len() == 1 => Some(hits[0]),
        None => None,
    }
}

/// The options in `words` the guard reads, by long name, each with its
/// value, split or attached, in a short cluster or abbreviated.
fn wget_args(words: &[String]) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let mut i = 1usize;
    while i < words.len() {
        let word = words[i].as_str();
        i += 1;
        if word == "--" {
            break;
        }
        if let Some(long) = word.strip_prefix("--") {
            let (name, attached) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (long, None),
            };
            let Some(name) = wget_long_name(&name.to_ascii_lowercase()) else {
                continue;
            };
            let value = match attached {
                _ if name == "no-config" => String::new(),
                Some(value) => value.to_string(),
                None => {
                    i += 1;
                    words.get(i - 1).cloned().unwrap_or_default()
                }
            };
            out.push((name, value));
            continue;
        }
        let Some(cluster) = word.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
            continue;
        };
        for (at, c) in cluster.char_indices() {
            let read = wget_opts().find(|(_, short)| *short == Some(c));
            if read.is_none() && !WGET_SHORT_VALUE.contains(c) {
                continue;
            }
            let rest = &cluster[at + c.len_utf8()..];
            let value = if rest.is_empty() {
                i += 1;
                words.get(i - 1).cloned().unwrap_or_default()
            } else {
                rest.to_string()
            };
            if let Some((long, _)) = read {
                out.push((long, value));
            }
            break;
        }
    }
    out
}

enum WgetFiles {
    Off,
    Defaults,
    Named(Vec<PathBuf>),
}

/// The first of `--no-config` and `--config` wins, which is what wget
/// does when it scans the command before reading a file.
fn wget_files_plan(words: &[String]) -> WgetFiles {
    let mut named = Vec::new();
    for (name, value) in wget_args(words) {
        match name {
            "no-config" if named.is_empty() => return WgetFiles::Off,
            "config" => named.push(PathBuf::from(value)),
            _ => {}
        }
    }
    if named.is_empty() {
        WgetFiles::Defaults
    } else {
        WgetFiles::Named(named)
    }
}

fn wget_files(
    words: &[String],
    env: &[(String, String)],
    cwd: Option<&Path>,
    tune: &mut Tune,
) -> Option<HttpShape> {
    match wget_files_plan(words) {
        WgetFiles::Off => None,
        WgetFiles::Named(paths) => {
            let mut opaque = false;
            for path in paths {
                let path = resolve_cfg(&path.to_string_lossy(), cwd);
                if read_wget_file(&path, tune, true).is_some() {
                    opaque = true;
                }
            }
            opaque.then_some(HttpShape::Opaque)
        }
        WgetFiles::Defaults => {
            if let Some(shape) = wget_system(env, tune) {
                return Some(shape);
            }
            wget_user(env, cwd, tune)
        }
    }
}

fn wget_system(env: &[(String, String)], tune: &mut Tune) -> Option<HttpShape> {
    if let Some(path) = prefixed_or_process(env, "SYSTEM_WGETRC") {
        if !path.is_empty() {
            let path = PathBuf::from(path);
            match open_cfg(&path) {
                Ok(None) => {}
                Err(()) => {
                    tune.unreadable = true;
                    return Some(HttpShape::Opaque);
                }
                Ok(Some(_)) => return read_wget_file(&path, tune, true),
            }
        }
    }
    read_wget_file(Path::new("/etc/wgetrc"), tune, false)
}

fn wget_user(env: &[(String, String)], cwd: Option<&Path>, tune: &mut Tune) -> Option<HttpShape> {
    if let Some(path) = prefixed_or_process(env, "WGETRC") {
        if !path.is_empty() {
            return read_wget_file(&resolve_cfg(&path, cwd), tune, true);
        }
    }
    if let Some(home) = prefixed_or_process(env, "HOME") {
        if !home.is_empty() {
            return read_wget_file(&PathBuf::from(home).join(".wgetrc"), tune, false);
        }
    }
    None
}

fn read_wget_file(path: &Path, tune: &mut Tune, required: bool) -> Option<HttpShape> {
    match open_cfg(path) {
        Ok(None) if !required => None,
        Ok(None) | Err(()) => {
            tune.unreadable = true;
            Some(HttpShape::Opaque)
        }
        Ok(Some(text)) => apply_wget_text(&text, tune),
    }
}

fn apply_wget_text(text: &str, tune: &mut Tune) -> Option<HttpShape> {
    let mut opaque = false;
    for line in text.lines() {
        match curl_cfg_line(line) {
            CfgLine::Blank => {}
            CfgLine::Bad => {
                opaque = true;
                tune.github = true;
            }
            CfgLine::Item { name, value } => {
                let key = name.replace(['_', '-'], "");
                if wget_key_moves_target(&key, value.as_deref().unwrap_or("")) {
                    tune.github = true;
                }
                match key.as_str() {
                    "method" => {
                        tune.method = Some(value.unwrap_or_default().trim().to_ascii_uppercase());
                    }
                    "postdata" | "postfile" | "bodydata" | "bodyfile" => tune.body = true,
                    "header" if value.as_deref().unwrap_or("").trim_start().starts_with('@') => {
                        opaque = true;
                    }
                    _ => {}
                }
            }
        }
    }
    opaque.then_some(HttpShape::Opaque)
}

/// A wgetrc command that can supply or change where the request goes. The
/// target is then not the one on the command line, so it may be GitHub.
fn wget_key_moves_target(key: &str, value: &str) -> bool {
    match key {
        "input" | "inputmetalink" | "base" | "httpproxy" | "httpsproxy" | "ftpproxy"
        | "useproxy" => true,
        "header" => header_sets_host(value),
        _ => false,
    }
}

fn note_wget_exec(value: &str, method: &mut Option<String>, body: &mut bool, github: &mut bool) {
    let lower = value.to_ascii_lowercase();
    let (key, val) = lower.split_once('=').unwrap_or((lower.as_str(), ""));
    let key = key.trim();
    if wget_key_moves_target(&key.replace(['_', '-'], ""), val) {
        *github = true;
    }
    match key {
        "method" => *method = Some(val.trim().to_ascii_uppercase()),
        "body_data" | "body_file" | "post_data" | "post_file" | "header" => *body = true,
        _ => {}
    }
}

fn httpie_shape(words: &[String]) -> HttpShape {
    let mut method: Option<String> = None;
    let mut saw_method = false;
    let mut body = false;
    for word in words.iter().skip(1) {
        if word.starts_with('@') {
            return HttpShape::Opaque;
        }
        if word.starts_with('-') {
            continue;
        }
        let upper = word.to_ascii_uppercase();
        if !saw_method
            && matches!(
                upper.as_str(),
                "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
            )
        {
            saw_method = true;
            method = Some(upper);
            continue;
        }
        if word.contains('=') && !word.starts_with("http") {
            body = true;
        }
    }
    finish_http(method.as_deref(), body, false, false)
}
