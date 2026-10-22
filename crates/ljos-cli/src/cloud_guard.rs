//! What a cloud seat refuses once `ljos cloud up` installed its rules,
//! read from the words each command runs rather than matched as text: a
//! `git push` that updates a `cursor/*` branch or tag, and a `gh` command
//! that opens, readies, reopens or merges a pull request. The source side
//! of a refspec, a remote URL, an option's value and the body of a comment
//! are never what is refused.

use std::cell::OnceCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The branch and tag prefix a cloud seat never pushes to.
const CURSOR: &str = "cursor/";

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

/// Why a `git push` in `line` would update a `cursor/*` ref, or `None`
/// when none would. `cwd` is where the line runs, the process directory
/// when absent; a `cd DIR` before the push and `git -C DIR` move it.
/// A `$NAME` bound by an earlier assignment on the same line is the
/// destination the shell pushes, so that name is what is judged.
pub(crate) fn cursor_push(line: &str, cwd: Option<&Path>) -> Option<String> {
    let mut here = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());
    let mut line_aliases: Vec<(String, String)> = Vec::new();
    for l in crate::nested_lines(line, 0) {
        let mut names: Vec<(String, String)> = Vec::new();
        for seg in crate::raw_segments(&l) {
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
            let run = prefix_context(seg, here.clone());
            match words.first().map(String::as_str) {
                Some("cd" | "pushd") => here = moved(here, &words[1..]),
                Some("popd") => here = None,
                Some("git") => {
                    if let Some(alias) = git_alias_write(&words[1..]) {
                        line_aliases.push(alias);
                    }
                    if let Some(why) =
                        git_push(&words[1..], run.dir, &line_aliases, &run.env, run.via_xargs)
                            .and_then(|p| p.cursor_ref())
                    {
                        return Some(why);
                    }
                }
                _ => {}
            }
        }
    }
    None
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

/// The pull-request actions `line` runs. gh's flags before the noun and
/// between `pr` and the verb are skipped, `-R` and `--repo` with their
/// value. A name `gh alias list` expands is read as that expansion, so
/// `gh pc` is whatever `pc` stands for. Words after a `pr` verb are its
/// arguments and are never read.
pub(crate) fn gh_acts(line: &str, cwd: Option<&Path>) -> GhActs {
    if !line.contains("gh") {
        return GhActs::default();
    }
    let mut here = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());
    let mut acts = GhActs::default();
    for l in crate::nested_lines(line, 0) {
        for seg in crate::raw_segments(&l) {
            let words = argv(&seg);
            match words.first().map(String::as_str) {
                Some("cd" | "pushd") => here = moved(here, &words[1..]),
                Some("popd") => here = None,
                Some("gh") => {
                    acts = acts.either(gh_command(&words, &prefix_env(&seg), here.as_deref(), 0));
                }
                _ => {}
            }
            if acts.open && acts.merge {
                return acts;
            }
        }
    }
    acts
}

/// How many times an alias may expand into another alias before the line
/// is refused. A real alias is one step. A loop would hide the verb.
const ALIAS_DEPTH: usize = 8;

fn gh_command(
    words: &[String],
    env: &[(String, String)],
    cwd: Option<&Path>,
    depth: usize,
) -> GhActs {
    if depth > ALIAS_DEPTH {
        return GhActs {
            open: true,
            merge: true,
        };
    }
    let mut rest = words.iter().map(String::as_str).skip(1);
    let Some(noun) = gh_word(&mut rest) else {
        return GhActs::default();
    };
    match noun {
        "pr" => pr_verb(gh_word(&mut rest)),
        "api" => gh_api(rest, cwd),
        other => {
            let Some(exp) = alias_expansion(env, other) else {
                return GhActs::default();
            };
            let extra: Vec<&str> = rest.collect();
            expand_alias(&exp, &extra, env, cwd, depth)
        }
    }
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
    depth: usize,
) -> GhActs {
    // A shell alias is `!script`. gh appends the words typed after the
    // alias, then the script runs. Anything that is not a gh pull request
    // is left alone.
    if let Some(script) = exp.strip_prefix('!') {
        let mut line = script.trim().to_string();
        if !extra.is_empty() {
            line.push(' ');
            line.push_str(&extra.join(" "));
        }
        return gh_script(&line, env, cwd, depth + 1);
    }
    let mut words = vec!["gh".to_string()];
    words.extend(crate::shell_words(exp));
    words.extend(extra.iter().map(|word| (*word).to_string()));
    gh_command(&words, env, cwd, depth + 1)
}

fn gh_script(line: &str, env: &[(String, String)], cwd: Option<&Path>, depth: usize) -> GhActs {
    let mut acts = GhActs::default();
    for l in crate::nested_lines(line, 0) {
        for seg in crate::raw_segments(&l) {
            let words = argv(&seg);
            if words.first().map(String::as_str) == Some("gh") {
                acts = acts.either(gh_command(&words, env, cwd, depth));
            }
        }
    }
    acts
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

fn gh_api<'a>(words: impl Iterator<Item = &'a str>, cwd: Option<&Path>) -> GhActs {
    let mut method: Option<String> = None;
    let mut endpoint: Option<String> = None;
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut input: Option<String> = None;
    let mut it = words;
    while let Some(word) = it.next() {
        if word == "--" {
            if endpoint.is_none() {
                endpoint = it.next().map(str::to_string);
            }
            continue;
        }
        if let Some(rest) = word.strip_prefix("--method=") {
            method = Some(rest.to_string());
            continue;
        }
        if let Some(rest) = word
            .strip_prefix("--raw-field=")
            .or_else(|| word.strip_prefix("--field="))
        {
            fields.push(field_pair(rest));
            continue;
        }
        if let Some(rest) = word.strip_prefix("--input=") {
            input = Some(rest.to_string());
            continue;
        }
        if let Some(inline) = word.strip_prefix("-X") {
            if !word.starts_with("--") && !inline.is_empty() {
                method = Some(inline.to_string());
                continue;
            }
        }
        if API_VALUED.contains(&word) {
            let Some(value) = it.next() else {
                continue;
            };
            match word {
                "-X" | "--method" => method = Some(value.to_string()),
                "-f" | "--raw-field" | "-F" | "--field" => fields.push(field_pair(value)),
                "--input" => input = Some(value.to_string()),
                _ => {}
            }
            continue;
        }
        if word.starts_with('-') && word.len() > 1 {
            continue;
        }
        if endpoint.is_none() {
            endpoint = Some(word.to_string());
        }
    }
    let Some(endpoint) = endpoint else {
        return GhActs::default();
    };
    classify_api(&endpoint, method.as_deref(), &fields, input.as_deref(), cwd)
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

fn graphql_acts(fields: &[(String, String)], input: Option<&str>, cwd: Option<&Path>) -> GhActs {
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
    GhActs {
        open: OPEN_MUTATIONS.iter().any(|name| has_ident(text, name)),
        merge: MERGE_MUTATIONS.iter().any(|name| has_ident(text, name)),
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
) -> Option<Push> {
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
    words = at.expand_aliases(words, line_aliases);
    let sub = words.first().map(String::as_str)?;
    if unresolved_subcommand(sub) {
        return Some(Push::refuse(
            at,
            format!("`{sub}` is only known once the shell expands it; name the git subcommand"),
        ));
    }
    match sub {
        "push" => {
            if via_xargs {
                return Some(Push::refuse(
                    at,
                    "xargs fills git push arguments at run time; name each refspec directly"
                        .to_string(),
                ));
            }
            Some(Push::read(&words[1..], at))
        }
        "send-pack" => send_pack_cursor_ref(&words, &at).map(|why| Push::refuse(at, why)),
        "subtree" => subtree_push_cursor_ref(&words, &at).map(|why| Push::refuse(at, why)),
        "submodule" => submodule_foreach_cursor_ref(&words, &at).map(|why| Push::refuse(at, why)),
        _ => None,
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

fn send_pack_cursor_ref(words: &[String], at: &Checkout) -> Option<String> {
    if words.first().map(String::as_str) != Some("send-pack") {
        return None;
    }
    let remote = words.get(1).map(String::as_str);
    words.iter().skip(2).find_map(|spec| at.spec(spec, remote))
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
}

impl Push {
    fn read(args: &[String], at: Checkout) -> Self {
        let mut push = Push {
            reason: None,
            at,
            words: Vec::new(),
            repo: None,
            bulk: Vec::new(),
            prune: false,
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
            at,
            words: Vec::new(),
            repo: None,
            bulk: Vec::new(),
            prune: false,
        }
    }

    /// Why this push would update a `cursor/*` ref.
    fn cursor_ref(&self) -> Option<String> {
        if let Some(reason) = &self.reason {
            return Some(reason.clone());
        }
        for flag in &self.bulk {
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
            return Some("`--mirror` can delete remote cursor/* branches and tags".into());
        }
        // git reads the first word as the remote even beside `--repo`, and
        // a writer may mean every word as a refspec; both readings are
        // judged.
        let (remote, specs) = if self.repo.is_some() {
            (self.words.first().or(self.repo.as_ref()), &self.words[..])
        } else {
            (self.words.first(), self.words.get(1..).unwrap_or(&[]))
        };
        if self.prune && specs.iter().any(|spec| prune_covers_heads(spec)) {
            return Some("`--prune` can delete remote cursor/* branches under refs/heads/".into());
        }
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
        let follow_tags = self.bulk.iter().any(|f| f == "--follow-tags");
        let tags_only = self.bulk.iter().any(|f| f == "--tags") && !covers_heads && !follow_tags;
        if self.words.len() <= 1 && !covers_heads && !tags_only {
            return self.at.implicit(remote.map(String::as_str));
        }
        None
    }
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

fn prune_covers_heads(spec: &str) -> bool {
    let spec = spec.strip_prefix('+').unwrap_or(spec);
    let dst = spec.split_once(':').map(|(_, dst)| dst).unwrap_or(spec);
    dst.starts_with("refs/heads/") && dst.contains('*')
}

/// A ref name under `cursor/`, written short or under `refs/heads/` or
/// `refs/tags/`.
fn under_cursor(name: &str) -> bool {
    name.strip_prefix("refs/heads/")
        .or_else(|| name.strip_prefix("refs/tags/"))
        .or_else(|| name.strip_prefix("heads/"))
        .or_else(|| name.strip_prefix("tags/"))
        .unwrap_or(name)
        .starts_with(CURSOR)
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

    fn expand_aliases(&self, words: Vec<String>, line_aliases: &[(String, String)]) -> Vec<String> {
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
            let mut expanded = if let Some(script) = value.strip_prefix('!') {
                let mut script_words = argv(script);
                if script_words.first().map(String::as_str) == Some("git") {
                    script_words.remove(0);
                }
                script_words
            } else {
                value.split_whitespace().map(str::to_string).collect()
            };
            if expanded.is_empty() {
                break;
            }
            expanded.extend(words.into_iter().skip(1));
            words = expanded;
        }
        words
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

    /// Why a push of local refs under one of `under` reaches `cursor/`.
    fn local(&self, under: &[&str], what: &str) -> Option<String> {
        let Some(refs) = self.refs() else {
            return Some(UNREAD.into());
        };
        refs.iter()
            .find(|r| {
                under
                    .iter()
                    .any(|p| r.strip_prefix(p).is_some_and(|n| n.starts_with(CURSOR)))
            })
            .map(|r| format!("{what} pushes {r}"))
    }

    /// Why the refspec `spec` would update a `cursor/*` ref: its
    /// destination (`dst` of `src:dst`, else the refspec itself, a leading
    /// `+` off), the branch checked out for `HEAD`, or each local ref a
    /// wildcard maps there. A destination the shell still has to expand
    /// cannot be told, and is refused.
    fn spec(&self, spec: &str, remote: Option<&str>) -> Option<String> {
        let spec = spec.strip_prefix('+').unwrap_or(spec);
        if spec == ":" {
            return self.local(&["refs/heads/"], "a matching push");
        }
        // A destination of `HEAD` is a branch named `HEAD` to git; only a
        // bare `HEAD` or `@` pushes the branch checked out. A colon inside
        // `$(...)`, `${...}` or backticks belongs to that substitution.
        let (src, dst, bare) = match refspec_sides(spec) {
            Some((src, dst)) => (src, dst, false),
            None => (spec, spec, true),
        };
        if bare && !under_cursor(dst) {
            if let Some(landed) = self.map_bare_destination(src, remote) {
                if landed != dst && under_cursor(&landed) {
                    return Some(format!("`{spec}` pushes {src} to {landed}"));
                }
            }
        }
        if CURRENT_BRANCH.contains(&dst) || (bare && matches!(dst, "HEAD" | "@")) {
            return match self.branch() {
                None => Some(UNREAD.into()),
                Some(Some(branch)) if under_cursor(branch) => Some(format!(
                    "this push updates {branch}, the branch checked out"
                )),
                Some(_) => None,
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
            return refs.iter().find_map(|r| {
                let landed = land(src, dst, r).or_else(|| {
                    (!src.starts_with("refs/"))
                        .then(|| land(&format!("refs/heads/{src}"), dst, r))
                        .flatten()
                })?;
                under_cursor(&landed).then(|| format!("`{spec}` pushes {r} to {landed}"))
            });
        }
        under_cursor(dst).then(|| format!("this push updates {dst}"))
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
                of_branch("merge")
                    .filter(|merge| under_cursor(merge))
                    .map(|merge| {
                        format!("this push updates {merge}, the upstream of the branch checked out")
                    })
            }
            _ => branch
                .filter(|b| under_cursor(b))
                .map(|b| format!("this push updates {b}, the branch checked out")),
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
