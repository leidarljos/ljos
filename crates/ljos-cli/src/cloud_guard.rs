//! What a cloud seat refuses once `ljos cloud up` installed its rules,
//! read from the words each command runs rather than matched as text: a
//! `git push` that updates a `cursor/*` branch or tag, and `gh pr create`
//! or `gh pr merge`. The source side of a refspec, a remote URL, an
//! option's value and the body of a comment are never what is refused.

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
pub(crate) fn cursor_push(line: &str, cwd: Option<&Path>) -> Option<String> {
    let mut here = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());
    let mut line_aliases: Vec<(String, String)> = Vec::new();
    for seg in crate::nested_lines(line, 0)
        .iter()
        .flat_map(|l| crate::raw_segments(l))
    {
        let words = argv(&seg);
        let run = prefix_context(&seg, here.clone());
        match words.first().map(String::as_str) {
            Some("cd" | "pushd") => here = moved(here, &words[1..]),
            Some("popd") => here = None,
            Some("git") => {
                if let Some(alias) = git_alias_write(&words[1..]) {
                    line_aliases.push(alias);
                }
                if let Some(why) = git_push(&words[1..], run.dir, &line_aliases, &run.env)
                    .and_then(|p| p.cursor_ref())
                {
                    return Some(why);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether `line` runs `gh pr` with one of `verbs`. gh's flags before the
/// noun and between the noun and the verb are skipped, `-R` and `--repo`
/// with their value; the words after the verb are its arguments and are
/// never read.
pub(crate) fn gh_pr(line: &str, verbs: &[&str]) -> bool {
    crate::nested_lines(line, 0)
        .iter()
        .flat_map(|l| crate::raw_segments(l))
        .any(|seg| {
            let words = argv(&seg);
            if words.first().map(String::as_str) != Some("gh") {
                return false;
            }
            let mut rest = words[1..].iter().map(String::as_str);
            gh_word(&mut rest) == Some("pr")
                && gh_word(&mut rest).is_some_and(|verb| verbs.contains(&verb))
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
fn moved(here: Option<PathBuf>, args: &[String]) -> Option<PathBuf> {
    let home = || crate::home().ok();
    let Some(to) = args.iter().find(|a| !a.starts_with('-') || *a == "-") else {
        return home();
    };
    if to == "-" || to.contains(['$', '`']) {
        return None;
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

#[derive(Default)]
struct PrefixContext {
    dir: Option<PathBuf>,
    env: Vec<(String, String)>,
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
    if words.first().map(String::as_str) != Some("push") {
        return None;
    }
    Some(Push::read(&words[1..], at))
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

/// A `git push` as its words give it.
struct Push {
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

    /// Why this push would update a `cursor/*` ref.
    fn cursor_ref(&self) -> Option<String> {
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
        let mirrors_or_all = self
            .bulk
            .iter()
            .any(|f| matches!(f.as_str(), "--all" | "--branches" | "--mirror"));
        if self.words.len() <= 1 && !mirrors_or_all {
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
        // bare `HEAD` or `@` pushes the branch checked out.
        let (src, dst, bare) = match spec.rsplit_once(':') {
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
        if spec.contains(['$', '`']) && dst.contains(['$', '`', '(', ')', '{', '}']) {
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
