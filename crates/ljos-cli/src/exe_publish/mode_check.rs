//! A source lint for common executable-publication mistakes.
//!
//! This is not a proof. Clippy's resolved disallowed-method and
//! disallowed-type checks enforce the filesystem API boundary. This lint
//! reads production Rust sources and `build.rs` in every workspace member and
//! catches common mistakes, including mode changes, shell programs, and a
//! `Command` program built from a non-literal. It cannot see generated code,
//! native code, or commands hidden inside shell strings.
//!
//! - `set_permissions` and `.permissions(..)` unless the mode is a literal
//!   without an execute bit, or is masked by one;
//! - `.mode(..)` and a two-argument `::mode(..)` on the same terms, except
//!   at the end of a `DirBuilder::new()` chain, which makes only
//!   directories;
//! - the mode argument of `libc` and `rustix::fs` `open`, `openat`, `creat`
//!   and `mknod` on the same terms;
//! - `chmod`, `fchmod`, `fchmodat`, `fchmodat2` and `lchmod` in any form;
//! - `fs::copy`, which copies the source's execute bits;
//! - a glob import of `fs` or `libc`, `fs` renamed, and anything else named
//!   `DirBuilder`, any of which would hide a form above;
//! - `paste` and `concat_idents`, which can build any name;
//! - `Command::new` and `program` with non-literal programs, writer
//!   programs, or shell programs.

use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

/// Narrow lint exceptions, matched by workspace-relative file, item path,
/// and exact number of hits in that item.
const ALLOWED: &[(&str, &str, usize, &str)] = &[
    (
        "crates/ljos-hud/src/summon.rs",
        "install_unix",
        1,
        "the HUD creates its private socket directory before changing its mode",
    ),
    (
        "crates/ljos-cli/src/book_queue.rs",
        "kick",
        1,
        "runs the queued executable",
    ),
    (
        "crates/ljos-cli/src/book_queue.rs",
        "run_batch",
        1,
        "runs the queued executable",
    ),
    (
        "crates/ljos-cli/src/cloud.rs",
        "shell_command_path",
        1,
        "probes a configured shell",
    ),
    (
        "crates/ljos-cli/src/cloud.rs",
        "binary_version",
        1,
        "probes a downloaded binary",
    ),
    (
        "crates/ljos-cli/src/cloud.rs",
        "run_installer",
        1,
        "runs the bundled installer script",
    ),
    (
        "crates/ljos-cli/src/hookd.rs",
        "start_server",
        1,
        "starts the resolved hook server",
    ),
    (
        "crates/ljos-cli/src/hud.rs",
        "run",
        1,
        "starts the resolved HUD binary",
    ),
    (
        "crates/ljos-cli/src/jev.rs",
        "key_by_command",
        1,
        "runs a configured key command",
    ),
    (
        "crates/ljos-cli/src/jev.rs",
        "command_post",
        1,
        "runs a configured post command",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "exec_server",
        1,
        "executes the resolved server",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "mcp_forward_with_requests",
        1,
        "starts the selected MCP program",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "responsible_comm",
        1,
        "runs the selected process viewer",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "detach",
        1,
        "starts the selected detached binary",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "file_yes_no",
        1,
        "runs the located vissue binary",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "spawn_member",
        1,
        "starts an argv-selected member",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "spawn_judge_worker",
        1,
        "starts the current executable as the judge worker",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "warm_command",
        1,
        "starts the selected warmer",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "probe_lists_ljos_run",
        1,
        "probes a configured binary",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "probe_version",
        1,
        "probes a resolved binary",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "check_vissue_ballot_protocol",
        1,
        "probes the resolved vissue binary",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "git_parents",
        1,
        "runs the selected process viewer",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "ljos_mcp_servers",
        1,
        "runs the selected process viewer",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "vouch_sign",
        1,
        "runs the configured signing binary",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "run_fed",
        1,
        "runs the configured federation binary",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "git_runs_in",
        1,
        "runs the selected process viewer",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "tcb_answer_in_with_env",
        1,
        "runs the configured TCB binary",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "run_as",
        1,
        "runs a resolved testable program",
    ),
    (
        "crates/ljos-cli/src/lib.rs",
        "run_captured_within",
        1,
        "runs a resolved testable program",
    ),
    (
        "crates/ljos-cli/src/mailbox/notmuch.rs",
        "command",
        1,
        "runs the configured notmuch binary",
    ),
    (
        "crates/ljos-cli/src/main.rs",
        "prompt_as_text",
        1,
        "restarts the current executable",
    ),
    (
        "crates/ljos-cli/src/mesh.rs",
        "tailscale_cmd",
        1,
        "runs the installed tailscale binary",
    ),
    (
        "crates/ljos-cli/src/mesh.rs",
        "start_daemon",
        1,
        "runs the installed tailscaled binary",
    ),
    (
        "crates/ljos-cli/src/panel_gate.rs",
        "run_job",
        1,
        "starts an argv-selected panel job",
    ),
    (
        "crates/ljos-cli/src/runner_hooks.rs",
        "relay",
        1,
        "starts the resolved relay binary",
    ),
    (
        "crates/ljos-cli/src/sync.rs",
        "age_cmd",
        1,
        "runs the selected age-compatible binary",
    ),
    (
        "crates/ljos-cli/src/sync.rs",
        "keygen_cmd",
        1,
        "runs the selected key generator",
    ),
    (
        "crates/ljos-cli/src/tools.rs",
        "run",
        1,
        "runs a resolved sibling binary",
    ),
    (
        "crates/ljos-cli/src/tools.rs",
        "answers",
        1,
        "runs a resolved sibling binary",
    ),
    (
        "crates/ljos-cli/src/upgrade.rs",
        "run_ok",
        1,
        "runs a selected upgrade helper",
    ),
    (
        "crates/ljos-cli/src/version_skew.rs",
        "version_of",
        1,
        "probes a resolved binary",
    ),
    (
        "crates/ljos-cli/src/wake.rs",
        "run_command",
        1,
        "runs a configured shell command",
    ),
    (
        "crates/ljos-hud/src/detach.rs",
        "start_detached",
        1,
        "restarts the current HUD executable",
    ),
    (
        "crates/ljos-test-boot/src/packsetd.rs",
        "command",
        1,
        "constructs a test boot command",
    ),
    (
        "crates/ljos-test-boot/src/packsetd.rs",
        "spawn",
        1,
        "starts a test boot command",
    ),
];

const CHMODS: &[&str] = &["chmod", "fchmod", "fchmodat", "fchmodat2", "lchmod"];

/// Programs that write the paths they are given or change their modes.
const WRITERS: &[&str] = &["cp", "mv", "install", "ln", "chmod"];
const SHELLS: &[&str] = &["sh", "bash", "dash", "fish", "ksh", "zsh"];

/// A call the check refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Hit {
    pub line: usize,
    pub call: String,
    pub why: &'static str,
    pub item: String,
}

/// The mode changes in `src` that could make a file executable.
pub(super) fn mode_changes(src: &str) -> Result<Vec<Hit>, String> {
    let stream: TokenStream = src.parse().map_err(|err| format!("{err:?}"))?;
    let toks: Vec<TokenTree> = stream.into_iter().collect();
    let mut hits = Vec::new();
    scan(&toks, "", &mut hits);
    hits.sort_by_key(|hit| hit.line);
    Ok(hits)
}

fn scan(toks: &[TokenTree], item: &str, hits: &mut Vec<Hit>) {
    let mut i = 0;
    while i < toks.len() {
        let tok = &toks[i];
        if is_punct(tok, '#') {
            if let Some(TokenTree::Group(attribute)) = toks.get(i + 1) {
                let attr = attribute
                    .stream()
                    .to_string()
                    .split_whitespace()
                    .collect::<String>();
                let test_only = attr == "test"
                    || attr.starts_with("cfg(test)")
                    || attr.starts_with("cfg(all(test,")
                    || attr.starts_with("cfg(any(test,");
                if test_only {
                    if let Some((body_at, _)) =
                        toks.iter().enumerate().skip(i + 2).find(|(_, tok)| {
                            matches!(tok, TokenTree::Group(group) if group.delimiter() == Delimiter::Brace)
                        })
                    {
                        i = body_at + 1;
                        continue;
                    }
                }
            }
        }
        if ident(tok).as_deref() == Some("fn") {
            if let (Some(name), Some((body_at, TokenTree::Group(body)))) =
                (toks.get(i + 1).and_then(ident), toks.iter().enumerate().skip(i + 2).find(|(_, tok)| {
                    matches!(tok, TokenTree::Group(group) if group.delimiter() == Delimiter::Brace)
                }))
            {
                let child = if item.is_empty() {
                    name
                } else {
                    format!("{item}::{name}")
                };
                scan(
                    &body.stream().into_iter().collect::<Vec<_>>(),
                    &child,
                    hits,
                );
                i = body_at + 1;
                continue;
            }
        }
        match tok {
            TokenTree::Group(group) => {
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                scan(&inner, item, hits);
            }
            TokenTree::Ident(_) => {
                if let Some(why) = refused(toks, i) {
                    hits.push(Hit {
                        line: tok.span().start().line,
                        call: call_text(toks, i),
                        why,
                        item: item.to_string(),
                    });
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Why the name at `toks[i]` is refused, if it is.
fn refused(toks: &[TokenTree], i: usize) -> Option<&'static str> {
    let name = ident(toks.get(i)?)?;
    let call = paren_after(toks, i);
    let method = i > 0 && is_punct(&toks[i - 1], '.');
    let pathed = path_sep_before(toks, i);
    let owner = pathed
        .then(|| toks.get(i.checked_sub(3)?).and_then(ident))
        .flatten();
    match name.as_str() {
        "set_permissions" => match call {
            Some(args) if args.last().is_some_and(|mode| no_exec(mode)) => None,
            Some(_) => Some("set_permissions with a mode that may hold an execute bit"),
            None => Some("set_permissions named without a call"),
        },
        "permissions" if method => match call {
            Some(args) if args.is_empty() => None,
            Some(args) if args.last().is_some_and(|mode| no_exec(mode)) => None,
            _ => Some("permissions set from a mode that may hold an execute bit"),
        },
        name if CHMODS.contains(&name) => Some("a chmod call"),
        "copy" if owner.as_deref() == Some("fs") => Some("fs::copy, which copies execute bits"),
        "fs" | "libc" => glob_or_alias(toks, i, &name),
        "mode" if method || pathed => match call {
            Some(args) if args.is_empty() => None,
            Some(args) if pathed && args.len() == 1 => None,
            Some(args) if args.last().is_some_and(|mode| no_exec(mode)) => None,
            Some(_) if method && rooted_at_dir_builder(toks, i) => None,
            Some(_) => Some("a mode that may hold an execute bit"),
            None if pathed => Some("mode named without a call"),
            None => None,
        },
        "open" | "open64" | "openat" | "openat64" | "creat" | "creat64" | "mknod" | "mknodat"
            if owner.as_deref() == Some("libc") || rustix_fs(toks, i) =>
        {
            let at = match name.as_str() {
                "open" | "open64" | "mknodat" => 2,
                "openat" | "openat64" => 3,
                _ => 1,
            };
            match call {
                Some(args) if args.get(at).is_none_or(|mode| no_exec(mode)) => None,
                Some(_) => Some("an open or create with a mode that may hold an execute bit"),
                None => Some("an open or create named without a call"),
            }
        }
        "DirBuilder" => {
            let before = i.checked_sub(1).and_then(|at| ident(&toks[at]));
            let defines = before.as_deref().is_some_and(|word| {
                matches!(
                    word,
                    "as" | "struct" | "enum" | "type" | "trait" | "union" | "fn" | "mod"
                )
            });
            let foreign = pathed && owner.as_deref() != Some("fs");
            (defines || foreign).then_some("a DirBuilder that may not be std::fs::DirBuilder")
        }
        "paste" | "concat_idents" => Some("a macro that can build any name"),
        "new" if owner.as_deref() == Some("Command") => writer_program(call.as_deref(), true),
        "program" if !method => writer_program(call.as_deref(), false),
        _ => None,
    }
}

/// `fs::*`, `libc::*`, `fs as ..`, and the forms above inside
/// `fs::{..}` or `libc::{..}`.
fn glob_or_alias(toks: &[TokenTree], i: usize, name: &str) -> Option<&'static str> {
    let next = toks.get(i + 1);
    if name == "fs" && next.and_then(ident).as_deref() == Some("as") {
        return Some("fs renamed, which hides fs::copy");
    }
    if !(next.is_some_and(|t| is_punct(t, ':'))
        && toks.get(i + 2).is_some_and(|t| is_punct(t, ':')))
    {
        return None;
    }
    match toks.get(i + 3)? {
        TokenTree::Punct(p) if p.as_char() == '*' => {
            Some("a glob import that hides the names it brings")
        }
        TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
            let inner: Vec<TokenTree> = group.stream().into_iter().collect();
            let hidden = inner.iter().enumerate().any(|(at, tok)| match tok {
                TokenTree::Punct(p) => p.as_char() == '*',
                TokenTree::Ident(id) => {
                    let word = id.to_string();
                    let first = at == 0 || is_punct(&inner[at - 1], ',');
                    first
                        && match name {
                            "fs" => word == "copy",
                            _ => matches!(
                                word.as_str(),
                                "open"
                                    | "open64"
                                    | "openat"
                                    | "openat64"
                                    | "creat"
                                    | "creat64"
                                    | "mknod"
                                    | "mknodat"
                            ),
                        }
                }
                _ => false,
            });
            hidden.then_some("an import that hides a call the check reads")
        }
        _ => None,
    }
}

fn writer_program(args: Option<&[Vec<TokenTree>]>, require_literal: bool) -> Option<&'static str> {
    let Some(args) = args else {
        return require_literal.then_some("a command program that is not one literal");
    };
    let [arg] = args else {
        return require_literal.then_some("a command program that is not one literal");
    };
    let [TokenTree::Literal(lit)] = arg.as_slice() else {
        return require_literal.then_some("a command program that is not one literal");
    };
    let text = lit.to_string();
    let Some(program) = text
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
    else {
        return require_literal.then_some("a command program that is not one literal");
    };
    let base = program.rsplit('/').next().unwrap_or(program);
    if WRITERS.contains(&base) {
        Some("a program that writes paths or changes modes")
    } else if SHELLS.contains(&base) {
        Some("a shell invocation")
    } else {
        None
    }
}

/// `toks` is a mode with no execute bit: a literal without one, possibly
/// cast, parenthesised or wrapped in `from_mode`, or a mask by one.
fn no_exec(toks: &[TokenTree]) -> bool {
    let mut toks = toks;
    if toks.last().is_some_and(|t| is_punct(t, ',')) {
        toks = &toks[..toks.len() - 1];
    }
    if let Some(cast) = toks.iter().rposition(|t| ident(t).as_deref() == Some("as")) {
        let typed = toks[cast + 1..]
            .iter()
            .all(|t| ident(t).is_some() || is_punct(t, ':'));
        if cast > 0 && typed && cast + 1 < toks.len() {
            toks = &toks[..cast];
        }
    }
    match toks {
        [TokenTree::Literal(lit)] => int_of(&lit.to_string()).is_some_and(|mode| mode & 0o111 == 0),
        [TokenTree::Group(group)] if group.delimiter() == Delimiter::Parenthesis => {
            no_exec(&group.stream().into_iter().collect::<Vec<_>>())
        }
        [.., TokenTree::Ident(id), TokenTree::Group(group)]
            if (id == "from_mode" || id == "from_raw_mode")
                && group.delimiter() == Delimiter::Parenthesis =>
        {
            no_exec(&group.stream().into_iter().collect::<Vec<_>>())
        }
        _ => masked(toks),
    }
}

/// `a & b & ..` with no other binary operator at the top level, where one
/// operand is a literal without an execute bit, or `a | b | ..` where each
/// operand is one or a `S_IF..` file type.
fn masked(toks: &[TokenTree]) -> bool {
    let ors = split_top(toks, '|');
    if ors.len() >= 2 {
        return ors.iter().all(|operand| {
            no_exec(operand)
                || operand
                    .last()
                    .and_then(ident)
                    .is_some_and(|name| name.starts_with("S_IF"))
        });
    }
    let mut operands: Vec<&[TokenTree]> = Vec::new();
    let mut start = 0;
    for (at, tok) in toks.iter().enumerate() {
        let TokenTree::Punct(p) = tok else { continue };
        let binary = at > start && !matches!(toks[at - 1], TokenTree::Punct(_));
        match p.as_char() {
            '&' if binary => {
                if p.spacing() == Spacing::Joint
                    && toks.get(at + 1).is_some_and(|t| is_punct(t, '&'))
                {
                    return false;
                }
                operands.push(&toks[start..at]);
                start = at + 1;
            }
            '|' | '^' | '+' | '-' | '*' | '/' | '%' | '<' | '>' | '=' => return false,
            _ => {}
        }
    }
    operands.push(&toks[start..]);
    operands.len() >= 2
        && operands.iter().any(|operand| match operand {
            [TokenTree::Literal(lit)] => {
                int_of(&lit.to_string()).is_some_and(|mode| mode & 0o111 == 0)
            }
            _ => false,
        })
}

/// `toks` split at each top-level binary `op` that is not doubled.
fn split_top(toks: &[TokenTree], op: char) -> Vec<&[TokenTree]> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (at, tok) in toks.iter().enumerate() {
        let TokenTree::Punct(p) = tok else { continue };
        let doubled = (p.spacing() == Spacing::Joint
            && toks.get(at + 1).is_some_and(|t| is_punct(t, op)))
            || (at > 0 && is_punct(&toks[at - 1], op));
        if p.as_char() == op && at > start && !doubled {
            parts.push(&toks[start..at]);
            start = at + 1;
        }
    }
    parts.push(&toks[start..]);
    parts
}

fn int_of(text: &str) -> Option<u32> {
    let text = text.replace('_', "");
    let text = ["u16", "u32", "u64", "usize", "i16", "i32", "i64", "isize"]
        .iter()
        .find_map(|suffix| text.strip_suffix(suffix))
        .unwrap_or(&text);
    if let Some(digits) = text.strip_prefix("0o") {
        u32::from_str_radix(digits, 8).ok()
    } else if let Some(digits) = text.strip_prefix("0x") {
        u32::from_str_radix(digits, 16).ok()
    } else if let Some(digits) = text.strip_prefix("0b") {
        u32::from_str_radix(digits, 2).ok()
    } else {
        text.parse().ok()
    }
}

/// `.mode` at `toks[i]` ends `DirBuilder::new()` followed by method calls.
fn rooted_at_dir_builder(toks: &[TokenTree], i: usize) -> bool {
    let mut j = i - 1;
    while j >= 3
        && is_paren(&toks[j - 1])
        && ident(&toks[j - 2]).is_some()
        && is_punct(&toks[j - 3], '.')
    {
        j -= 3;
    }
    j >= 5
        && is_paren(&toks[j - 1])
        && ident(&toks[j - 2]).as_deref() == Some("new")
        && path_sep_before(toks, j - 2)
        && ident(&toks[j - 5]).as_deref() == Some("DirBuilder")
}

fn paren_after(toks: &[TokenTree], i: usize) -> Option<Vec<Vec<TokenTree>>> {
    let TokenTree::Group(group) = toks.get(i + 1)? else {
        return None;
    };
    if group.delimiter() != Delimiter::Parenthesis {
        return None;
    }
    let mut args = vec![Vec::new()];
    for tok in group.stream() {
        if is_punct(&tok, ',') {
            args.push(Vec::new());
        } else {
            args.last_mut()?.push(tok);
        }
    }
    if args.last().is_some_and(Vec::is_empty) {
        args.pop();
    }
    Some(args)
}

fn path_sep_before(toks: &[TokenTree], i: usize) -> bool {
    i >= 2 && is_punct(&toks[i - 1], ':') && is_punct(&toks[i - 2], ':')
}

/// `rustix::fs` immediately before the name at `toks[i]`.
fn rustix_fs(toks: &[TokenTree], i: usize) -> bool {
    let Some(fs_at) = i.checked_sub(3) else {
        return false;
    };
    let Some(root_at) = fs_at.checked_sub(3) else {
        return false;
    };
    path_sep_before(toks, i)
        && ident(&toks[fs_at]).as_deref() == Some("fs")
        && path_sep_before(toks, fs_at)
        && ident(&toks[root_at]).as_deref() == Some("rustix")
}

fn ident(tok: &TokenTree) -> Option<String> {
    match tok {
        TokenTree::Ident(id) => Some(id.to_string()),
        _ => None,
    }
}

fn is_punct(tok: &TokenTree, ch: char) -> bool {
    matches!(tok, TokenTree::Punct(p) if p.as_char() == ch)
}

fn is_paren(tok: &TokenTree) -> bool {
    matches!(tok, TokenTree::Group(g) if g.delimiter() == Delimiter::Parenthesis)
}

/// The name at `toks[i]` and its argument list, without spaces.
fn call_text(toks: &[TokenTree], i: usize) -> String {
    let mut text = toks[i].to_string();
    if let Some(TokenTree::Group(group)) = toks.get(i + 1) {
        text.push_str(&group.to_string());
    }
    text.split_whitespace().collect()
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
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn workspace_source_lint_catches_common_publication_mistakes() {
    let _env_read = crate::test_env_read_lock();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    #[expect(
        clippy::disallowed_methods,
        reason = "the lint invokes cargo metadata with a fixed literal program"
    )]
    let output = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--offline",
            "--format-version",
            "1",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let root = Path::new(metadata["workspace_root"].as_str().unwrap());
    let mut files = Vec::new();
    for package in metadata["packages"].as_array().unwrap() {
        let manifest = Path::new(package["manifest_path"].as_str().unwrap());
        let member = manifest.parent().unwrap();
        rust_sources(&member.join("src"), &mut files);
        let build = member.join("build.rs");
        if build.is_file() {
            files.push(build);
        }
    }
    files.sort();
    files.dedup();
    files.sort();
    assert!(files.len() > 100, "found {} sources", files.len());
    let mut found = Vec::new();
    let mut used = vec![0_usize; ALLOWED.len()];
    for file in &files {
        let rel = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if rel == "crates/ljos-cli/src/exe_publish.rs"
            || rel.starts_with("crates/ljos-cli/src/exe_publish/")
        {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        let hits = mode_changes(&text).unwrap_or_else(|err| panic!("{rel}: {err}"));
        for hit in hits {
            let allowed = ALLOWED
                .iter()
                .position(|(path, item, _, _)| rel == *path && hit.item == *item);
            match allowed {
                Some(at) if used[at] < ALLOWED[at].2 => used[at] += 1,
                None => found.push(format!(
                    "{rel}:{}: {} in {}: {}",
                    hit.line, hit.why, hit.item, hit.call
                )),
                Some(_) => found.push(format!(
                    "{rel}:{}: extra lint exception hit in {}: {}",
                    hit.line, hit.item, hit.call
                )),
            }
        }
    }
    assert!(
        found.is_empty(),
        "use the publication module and literal non-shell command programs:\n{}",
        found.join("\n")
    );
    let stale: Vec<_> = ALLOWED
        .iter()
        .zip(&used)
        .filter(|((_, _, count, _), used)| **used != *count)
        .map(|((path, item, count, _), used)| {
            format!("{path}::{item}: expected {count} hits, found {used}")
        })
        .collect();
    assert!(
        stale.is_empty(),
        "allowed calls no longer found:\n{}",
        stale.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::mode_changes;

    fn lines(src: &str) -> Vec<usize> {
        mode_changes(src)
            .unwrap()
            .iter()
            .map(|hit| hit.line)
            .collect()
    }

    /// The form the line scanner missed: a file written in this process
    /// and then made executable through a descriptor, with an `#[expect]`
    /// and the call wrapped over more lines than the scanner looked ahead.
    #[test]
    fn a_write_then_a_descriptor_chmod_is_refused_however_it_is_wrapped() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = r##"
fn stub(bin: &Path) {
    crate::write_file(bin.join("a"), "#!/bin/sh\n").unwrap();
    let file = std::fs::File::open(bin.join("a")).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "a reason long enough that rustfmt wraps the attribute over several lines"
    )]
    file
        // a comment between the receiver and the call
        .set_permissions(
            std::fs::Permissions::from_mode(
                0o755,
            ),
        )
        .unwrap();
}
"##;
        assert_eq!(lines(wrapped), vec![11]);
        let flat = r##"fn stub(bin: &Path) { crate::write_file(bin.join("a"), "x").unwrap(); std::fs::File::open(bin.join("a")).unwrap().set_permissions(std::fs::Permissions::from_mode(0o755)).unwrap(); }"##;
        assert_eq!(lines(flat), vec![1]);
    }

    #[test]
    fn a_mode_that_may_hold_an_execute_bit_is_refused() {
        let _env_read = crate::test_env_read_lock();
        let src = r#"
fn f(path: &Path, mode: u32, file: &File) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let perms = std::fs::Permissions::from_mode(0o644);
    std::fs::set_permissions(path, perms).unwrap();
    file.set_permissions(Permissions::from_mode(mode)).unwrap();
    file.set_permissions(Permissions::from_mode(mode | 0o100)).unwrap();
    file.set_permissions(Permissions::from_mode(mode & 0o666 | 0o100)).unwrap();
    OpenOptions::new().write(true).mode(0o755).open(path).unwrap();
    OpenOptions::new().write(true).mode(mode).open(path).unwrap();
    OpenOptionsExt::mode(&mut options, 0o600 | 0o100);
    tempfile::Builder::new().permissions(Permissions::from_mode(0o755)).tempfile().unwrap();
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
}
"#;
        assert_eq!(lines(src), vec![3, 5, 6, 7, 8, 9, 10, 11, 12, 14]);
    }

    #[test]
    fn a_mode_without_an_execute_bit_is_let_through() {
        let _env_read = crate::test_env_read_lock();
        let src = r#"
// std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
/// `chmod 755` in a doc comment
fn f(path: &Path, mode: u32, meta: &Metadata) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(path, PermissionsExt::from_mode(0o644 as u32)).unwrap();
    unsafe { libc::openat(dir, name, flags, 0o600 as libc::c_uint) };
    file.set_permissions(Permissions::from_mode(mode & 0o666)).unwrap();
    file.set_permissions(Permissions::from_mode((mode & 0o7777) & 0o644)).unwrap();
    OpenOptions::new().create(true).mode(0o600).open(path).unwrap();
    OpenOptions::new().create(true).mode(mode & 0o666).open(path).unwrap();
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path).unwrap();
    DirBuilder::new().mode(mode).create(path).unwrap();
    let exec = meta.permissions().mode() & 0o111 != 0;
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode();
    mode(path);
    let text = "chmod 755 bin/ljos";
    crate::program("true").unwrap();
    unsafe { libc::openat(dir, name, flags, 0o600) };
    unsafe { libc::open(name, flags) };
    std::io::copy(&mut from, &mut to).unwrap();
    let field = stat.mode;
    let bits = std::os::unix::fs::PermissionsExt::mode(&meta.permissions());
    unsafe { libc::mknod(path, libc::S_IFCHR | 0o600, dev) };
}
"#;
        assert_eq!(mode_changes(src).unwrap(), vec![]);
    }

    #[test]
    fn a_name_that_hides_a_mode_change_is_refused() {
        let _env_read = crate::test_env_read_lock();
        let src = r#"
use std::fs::set_permissions as sp;
use std::fs::{copy as c, write};
use std::fs as f;
use std::fs::*;
use libc::*;
use libc::fchmod as fc;
use libc::{openat as oa, close};
use libc::openat;
use std::fs::OpenOptions as DirBuilder;
type DirBuilder = OpenOptions;
paste::paste! { [<set_ perm>](path, mode) }
macro_rules! chmod_it { ($p:expr) => { std::fs::set_permissions($p, MODE) } }
fn g() { let set = std::fs::set_permissions; std::fs::copy(a, b).unwrap(); }
fn h() { crate::program("/bin/chmod"); std::process::Command::new("cp"); }
fn i() { unsafe { libc::chmod(p, 0o644); libc::openat(dir, name, flags, 0o755); } }
"#;
        assert_eq!(
            lines(src),
            vec![2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 12, 13, 14, 14, 15, 15, 16, 16]
        );
    }

    #[test]
    fn rustix_open_and_openat_with_an_execute_bit_are_refused() {
        let _env_read = crate::test_env_read_lock();
        let src = r#"
fn plant(dir: &File) {
    rustix::fs::openat(dir, "pre-commit", flags, 0o755.into());
    rustix::fs::open("pre-commit", flags, 0o755);
    rustix::fs::openat(dir, "note", flags, Mode::from_raw_mode(0o600));
    rustix::fs::open("note", flags, 0o644);
}
"#;
        assert_eq!(lines(src), vec![3, 4]);
    }

    #[test]
    fn nonliteral_programs_and_shells_are_refused() {
        let _env_read = crate::test_env_read_lock();
        let src = r#"
fn command(program: &str) {
    std::process::Command::new(program);
    std::process::Command::new(concat!("ch", "mod"));
    crate::program("sh");
    crate::program("/bin/bash");
    crate::program("true");
}
"#;
        assert_eq!(lines(src), vec![3, 4, 5, 6]);
    }
}
