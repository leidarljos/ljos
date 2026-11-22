//! No source under `crates/` outside `exe_publish` sets a mode that can
//! make a file executable.
//!
//! Each source is read as tokens, so a call wrapped over many lines, a
//! comment, or an `#[expect]` around it changes nothing the check sees. A
//! file to be executed is published with [`super::publish`], and any other
//! mode change goes through [`super::change_mode`] or
//! [`super::change_mode_of`], which refuse to make a file executable. The
//! check refuses, outside this module:
//!
//! - `set_permissions` and `.permissions(..)` unless the mode is a literal
//!   without an execute bit, or is masked by one;
//! - `.mode(..)` and a two-argument `::mode(..)` on the same terms, except
//!   at the end of a `DirBuilder::new()` chain, which makes only
//!   directories;
//! - the mode argument of `libc::open`, `openat`, `creat` and `mknod` on the
//!   same terms;
//! - `chmod`, `fchmod`, `fchmodat`, `fchmodat2` and `lchmod` in any form;
//! - `fs::copy`, which copies the source's execute bits;
//! - a glob import of `fs` or `libc`, `fs` renamed, and anything else named
//!   `DirBuilder`, any of which would hide a form above;
//! - `paste` and `concat_idents`, which can build any name;
//! - `Command::new` and `program` of `cp`, `mv`, `install`, `ln` and
//!   `chmod`.
//!
//! Text passed to a shell is out of its reach.

use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

/// Calls outside this module the check lets through, by the file under
/// `crates/` and the call's tokens without spaces, each with its reason.
const ALLOWED: &[(&str, &str, &str)] = &[(
    "ljos-hud/src/summon.rs",
    "set_permissions(parent,std::fs::Permissions::from_mode(0o700))",
    "the socket's directory, which create_dir_all made or found just before; ljos-hud does not depend on ljos-cli",
)];

const CHMODS: &[&str] = &["chmod", "fchmod", "fchmodat", "fchmodat2", "lchmod"];

/// Programs that write the paths they are given or change their modes.
const WRITERS: &[&str] = &["cp", "mv", "install", "ln", "chmod"];

/// A call the check refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Hit {
    pub line: usize,
    pub call: String,
    pub why: &'static str,
}

/// The mode changes in `src` that could make a file executable.
pub(super) fn mode_changes(src: &str) -> Result<Vec<Hit>, String> {
    let stream: TokenStream = src.parse().map_err(|err| format!("{err:?}"))?;
    let toks: Vec<TokenTree> = stream.into_iter().collect();
    let mut hits = Vec::new();
    scan(&toks, &mut hits);
    hits.sort_by_key(|hit| hit.line);
    Ok(hits)
}

fn scan(toks: &[TokenTree], hits: &mut Vec<Hit>) {
    for (i, tok) in toks.iter().enumerate() {
        match tok {
            TokenTree::Group(group) => {
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                scan(&inner, hits);
            }
            TokenTree::Ident(_) => {
                if let Some(why) = refused(toks, i) {
                    hits.push(Hit {
                        line: tok.span().start().line,
                        call: call_text(toks, i),
                        why,
                    });
                }
            }
            _ => {}
        }
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
            if owner.as_deref() == Some("libc") =>
        {
            let at = match name.as_str() {
                "open" | "open64" | "mknodat" => 2,
                "openat" | "openat64" => 3,
                _ => 1,
            };
            match call {
                Some(args) if args.get(at).is_none_or(|mode| no_exec(mode)) => None,
                Some(_) => Some("a libc create with a mode that may hold an execute bit"),
                None => Some("a libc create named without a call"),
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
        "new" if owner.as_deref() == Some("Command") => writer_program(call.as_deref()),
        "program" if !method => writer_program(call.as_deref()),
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

fn writer_program(args: Option<&[Vec<TokenTree>]>) -> Option<&'static str> {
    let [arg] = args? else {
        return None;
    };
    let [TokenTree::Literal(lit)] = arg.as_slice() else {
        return None;
    };
    let text = lit.to_string();
    let program = text.strip_prefix('"')?.strip_suffix('"')?;
    let base = program.rsplit('/').next().unwrap_or(program);
    WRITERS
        .contains(&base)
        .then_some("a program that writes paths or changes modes")
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
            if id == "from_mode" && group.delimiter() == Delimiter::Parenthesis =>
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
fn no_source_outside_exe_publish_can_make_a_file_executable() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_sources(crates, &mut files);
    files.sort();
    assert!(files.len() > 100, "found {} sources", files.len());
    let mut found = Vec::new();
    let mut used = vec![false; ALLOWED.len()];
    for file in &files {
        let rel = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if rel == "ljos-cli/src/exe_publish.rs" || rel.starts_with("ljos-cli/src/exe_publish/") {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        let hits = mode_changes(&text).unwrap_or_else(|err| panic!("{rel}: {err}"));
        for hit in hits {
            let allowed = ALLOWED
                .iter()
                .position(|(path, call, _)| rel == *path && hit.call == *call);
            match allowed {
                Some(at) => used[at] = true,
                None => found.push(format!(
                    "crates/{rel}:{}: {}: {}",
                    hit.line, hit.why, hit.call
                )),
            }
        }
    }
    assert!(
        found.is_empty(),
        "publish a file to be executed with ljos_cli::publish, and change any other mode with change_mode or change_mode_of:\n{}",
        found.join("\n")
    );
    let stale: Vec<_> = ALLOWED
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|((path, call, _), _)| format!("{path}: {call}"))
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
}
