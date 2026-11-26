//! Long notes: a file kept as an org page beside its ticket.
//!
//! `ljos note ID --file PATH` writes the file to
//! `<board>/notes/<leaf>/<slug>.org`, next to the board's `issues.org` or
//! `issues/<leaf>.org`, and adds one stub line to the ticket's logbook: an
//! org link to that page and its size. The page is plain org, so Emacs opens
//! and edits it like any other tracker file, and git compresses it. Its path
//! is its identity. An edit is a new git version of the same page. The page
//! is published by writing a temporary in the same directory, syncing it and
//! renaming it into place. A failed write removes the temporary and leaves
//! no page and no stub.
//!
//! Markdown is converted with pandoc when it is on `PATH`
//! (`LJOS_PANDOC=off` skips it), else with the small converter below. A
//! log or a text file goes inside one example block. Org is kept as it is.
//!
//! The search hot path never opens these pages. A local index under
//! `.ljos/cache/` in the tracker checkout, ignored by git, holds their text.
//! A write or an import updates it. `ljos search --long` refreshes the
//! entries whose size or modification time changed, then reads it.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// A long note past this many bytes is kept whole, with a warning.
pub const LONG_NOTE_WARN_BYTES: usize = 256 * 1024;

/// A long note past this many bytes keeps its head and tail only. The full
/// body goes to the local deed store.
pub const LONG_NOTE_MAX_BYTES: usize = 1024 * 1024;

/// How many bytes of the head, and of the tail, a trimmed note keeps.
pub const TRIM_KEEP_BYTES: usize = 128 * 1024;

/// The most bytes of the stub text. vissue writes it inside
/// `- Note: "..." [YYYY-MM-DD Ddd HH:MM]`, so the logbook line stays under
/// 200 bytes.
pub const STUB_MAX_BYTES: usize = 160;

const SLUG_MAX_BYTES: usize = 40;

/// The file names `ljos notes import` takes.
const IMPORT_EXTENSIONS: &[&str] = &["md", "txt", "log"];

/// What a note is, for the stub and the listing.
pub const KINDS: &[&str] = &["land", "report", "gate", "log"];

/// The markup a body arrives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Markup {
    Markdown,
    Org,
    /// A log or plain text, kept verbatim in an example block.
    Verbatim,
}

/// One `ljos note ID --file` request.
pub struct Request<'a> {
    pub issue: &'a str,
    /// A path, or `-` for stdin.
    pub file: &'a str,
    /// The body itself, read in place of `file`: a short note's text past
    /// a tracker note's caps. `file` is then `-`.
    pub body: Option<&'a str>,
    pub title: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub refuse_trim: bool,
}

/// A page ready for the logbook, or the page already there.
struct Planned {
    /// The page, written unless this is a dry run or a duplicate.
    path: PathBuf,
    /// The page from the tracker checkout's root, for the commit.
    rel: String,
    stub: String,
    /// The page already on the ticket with these bytes.
    duplicate: bool,
    /// The deed that holds the full body of a trimmed note.
    deed: Option<String>,
    /// Warnings for stderr.
    lines: String,
    bytes: usize,
    trimmed: bool,
}

/// Where a ticket's long notes live.
struct Place {
    /// `<board>/notes/<leaf>`.
    dir: PathBuf,
    /// The link from the ticket's org file to `dir`.
    link: String,
    /// The tracker checkout, or the directory above the board when it is
    /// not a git checkout.
    root: PathBuf,
}

fn place(issue: &str) -> Result<Place> {
    let file = crate::tracker_issue_path(issue)
        .map_err(|e| anyhow::anyhow!("note: could not find {issue}: {e}"))?;
    let parent = file
        .parent()
        .context("note: the ticket's file has no directory")?
        .to_path_buf();
    let split = parent.file_name().is_some_and(|n| n == "issues")
        && parent
            .parent()
            .is_some_and(|board| board.join("issues.org").is_file());
    let (board, up) = if split {
        (
            parent
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| parent.clone()),
            "../",
        )
    } else {
        (parent.clone(), "")
    };
    let leaf = crate::ledger_file_name(issue);
    let leaf = leaf.strip_suffix(".org").unwrap_or(&leaf).to_string();
    Ok(Place {
        dir: board.join("notes").join(&leaf),
        link: format!("{up}notes/{leaf}"),
        root: index_root(&board),
    })
}

/// The checkout that holds `dir`, else `dir` itself.
fn index_root(dir: &Path) -> PathBuf {
    match crate::git_ok_stdout(dir, &["rev-parse", "--show-toplevel"]) {
        Some(top) if Path::new(top.trim()).is_dir() => PathBuf::from(top.trim()),
        _ => dir.to_path_buf(),
    }
}

fn rel_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// `1234567` as `1,234,567`.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (at, ch) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn deed_route(issue: &str) -> String {
    format!(
        "keep it with `deedar create file --path FILE --name NAME --agent SEAT` and cite it with `ljos deed {issue} --add ACCESSION`"
    )
}

/// The body as text, or why it is not a note: a NUL byte, bytes that are
/// not UTF-8, or a run past the short-note line cap with no space in it,
/// which reads as encoded bytes.
fn text_body<'b>(issue: &str, what: &str, bytes: &'b [u8]) -> Result<&'b str> {
    let refuse = |why: String| -> anyhow::Error {
        anyhow::anyhow!(
            "note: {what} {why}, so it is not a note; {}. Nothing was noted",
            deed_route(issue)
        )
    };
    if bytes.contains(&0) {
        return Err(refuse("holds a NUL byte".into()));
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(refuse("is not UTF-8 text".into()));
    };
    let longest_run = |line: &str| {
        line.split(char::is_whitespace)
            .map(str::len)
            .max()
            .unwrap_or(0)
    };
    if let Some((at, run)) = text
        .lines()
        .enumerate()
        .map(|(at, line)| (at, longest_run(line)))
        .find(|(_, run)| *run > crate::NOTE_LINE_MAX_BYTES)
    {
        return Err(refuse(format!(
            "has a run of {} bytes with no space on line {}, which reads as encoded bytes",
            grouped(run),
            at + 1
        )));
    }
    Ok(text)
}

/// The largest index at or below `at` on a char boundary of `s`.
fn floor_char(s: &str, at: usize) -> usize {
    let mut at = at.min(s.len());
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The head and the tail of `text`, each about [`TRIM_KEEP_BYTES`], cut at
/// line ends where one is near.
fn head_and_tail(text: &str) -> (&str, &str) {
    let head_end = floor_char(text, TRIM_KEEP_BYTES);
    let head_end = text[..head_end].rfind('\n').map_or(head_end, |at| at + 1);
    let tail_start = floor_char(text, text.len().saturating_sub(TRIM_KEEP_BYTES));
    let tail_start = text[tail_start..]
        .find('\n')
        .map_or(tail_start, |at| tail_start + at + 1);
    (&text[..head_end], &text[tail_start.max(head_end)..])
}

fn markup_of(file: &str, kind: Option<&str>) -> Markup {
    if kind == Some("log") {
        return Markup::Verbatim;
    }
    if file == "-" {
        return Markup::Markdown;
    }
    let ext = Path::new(file)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "md" | "markdown" => Markup::Markdown,
        "org" => Markup::Org,
        _ => Markup::Verbatim,
    }
}

/// A line inside an org block, with the comma org puts before a line that
/// would otherwise start a heading or a keyword.
fn block_line(line: &str) -> String {
    let body = line.trim_start();
    let indent = &line[..line.len() - body.len()];
    let needs = body.starts_with('*')
        || body.starts_with("#+")
        || body.starts_with(",*")
        || body.starts_with(",#+");
    if needs {
        format!("{indent},{body}")
    } else {
        line.to_string()
    }
}

fn example_block(text: &str) -> String {
    let mut out = String::from("#+begin_example\n");
    for line in text.lines() {
        out.push_str(&block_line(line));
        out.push('\n');
    }
    out.push_str("#+end_example\n");
    out
}

/// Markdown as org with pandoc, when it is on `PATH` and answers.
fn pandoc(text: &str) -> Option<String> {
    let bin = std::env::var("LJOS_PANDOC").unwrap_or_default();
    let bin = match bin.trim() {
        "off" | "0" | "false" => return None,
        "" => "pandoc",
        named => named,
    };
    let bin = which::which(bin).ok()?;
    let dir = tempfile::tempdir().ok()?;
    let input = dir.path().join("note.md");
    crate::write_file(&input, text).ok()?;
    let mut cmd = crate::program(&bin).ok()?;
    cmd.args([
        "--from",
        "markdown-auto_identifiers",
        "--to",
        "org",
        "--wrap=preserve",
    ])
    .arg(&input);
    let out = crate::bounded_output(&mut cmd).ok()?;
    if !out.status.success() {
        return None;
    }
    let org = String::from_utf8(out.stdout).ok()?;
    (!org.trim().is_empty()).then_some(org)
}

/// `[text](url)` as `[[url][text]]` and `![alt](url)` as `[[url]]`, outside
/// code spans. A bracket that is not a link is left.
fn md_links(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let image = open > 0 && rest[..open].ends_with('!');
        let Some(close) = rest[open..].find("](").map(|at| open + at) else {
            break;
        };
        let Some(end) = rest[close + 2..].find(')').map(|at| close + 2 + at) else {
            break;
        };
        let text = &rest[open + 1..close];
        let url = &rest[close + 2..end];
        if url.contains(char::is_whitespace) || text.contains('[') {
            out.push_str(&rest[..=open]);
            rest = &rest[open + 1..];
            continue;
        }
        let lead = if image {
            &rest[..open - 1]
        } else {
            &rest[..open]
        };
        out.push_str(lead);
        if image || text.is_empty() {
            out.push_str(&format!("[[{url}]]"));
        } else {
            out.push_str(&format!("[[{url}][{text}]]"));
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// One markdown line's inline markup as org: code spans, links and bold.
fn md_inline(line: &str) -> String {
    let mut out = String::new();
    for (at, piece) in line.split('`').enumerate() {
        if at % 2 == 1 {
            out.push_str(&format!("~{piece}~"));
        } else {
            out.push_str(&md_links(piece).replace("**", "*"));
        }
    }
    if line.matches('`').count() % 2 == 1 {
        return md_links(line);
    }
    out
}

/// Markdown as org without pandoc: headings, fenced code, quotes, bullets,
/// rules, code spans, links and bold. Anything else passes as it is.
fn markdown_to_org(text: &str) -> String {
    let mut out = String::new();
    let mut fence: Option<&'static str> = None;
    let mut quote = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(end) = fence {
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                out.push_str(end);
                out.push('\n');
                fence = None;
            } else {
                out.push_str(&block_line(line));
                out.push('\n');
            }
            continue;
        }
        let quoted = trimmed.strip_prefix('>');
        if quoted.is_some() != quote {
            out.push_str(if quote {
                "#+end_quote\n"
            } else {
                "#+begin_quote\n"
            });
            quote = quoted.is_some();
        }
        if let Some(body) = quoted {
            out.push_str(&md_inline(body.strip_prefix(' ').unwrap_or(body)));
            out.push('\n');
            continue;
        }
        if let Some(lang) = trimmed
            .strip_prefix("```")
            .or_else(|| trimmed.strip_prefix("~~~"))
        {
            let lang = lang.trim();
            if lang.is_empty() {
                out.push_str("#+begin_example\n");
                fence = Some("#+end_example");
            } else {
                let word = lang.split_whitespace().next().unwrap_or(lang);
                out.push_str(&format!("#+begin_src {word}\n"));
                fence = Some("#+end_src");
            }
            continue;
        }
        let hashes = line.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
            let title = line[hashes..].trim().trim_end_matches('#').trim_end();
            out.push_str(&format!("{} {}\n", "*".repeat(hashes), md_inline(title)));
            continue;
        }
        let rule = trimmed.len() >= 3
            && (trimmed.chars().all(|c| c == '-')
                || trimmed.chars().all(|c| c == '*')
                || trimmed.chars().all(|c| c == '_'));
        if rule {
            out.push_str("-----\n");
            continue;
        }
        let indent = &line[..line.len() - trimmed.len()];
        if let Some(item) = trimmed
            .strip_prefix("* ")
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            out.push_str(&format!("{indent}- {}\n", md_inline(item)));
            continue;
        }
        if trimmed.starts_with("#+") {
            out.push_str(&block_line(line));
            out.push('\n');
            continue;
        }
        out.push_str(&md_inline(line));
        out.push('\n');
    }
    if let Some(end) = fence {
        out.push_str(end);
        out.push('\n');
    }
    if quote {
        out.push_str("#+end_quote\n");
    }
    out
}

/// An org body's own `#+title:`, and the body without it.
fn split_org_title(text: &str) -> (Option<String>, String) {
    let mut title = None;
    let mut body = String::new();
    for line in text.lines() {
        if title.is_none() {
            if let Some(rest) = line
                .strip_prefix("#+title:")
                .or_else(|| line.strip_prefix("#+TITLE:"))
            {
                title = Some(rest.trim().to_string());
                continue;
            }
        }
        body.push_str(line);
        body.push('\n');
    }
    (title, body)
}

fn to_org(text: &str, markup: Markup) -> String {
    match markup {
        Markup::Verbatim => example_block(text),
        Markup::Org => split_org_title(text).1,
        Markup::Markdown => pandoc(text).unwrap_or_else(|| markdown_to_org(text)),
    }
}

/// A title on one line, with no bracket or double quote to break the link
/// or vissue's quoting.
fn clean_title(raw: &str) -> String {
    let words: Vec<&str> = raw.split_whitespace().collect();
    words
        .join(" ")
        .replace('[', "(")
        .replace(']', ")")
        .replace('"', "'")
}

fn slug_of(title: &str) -> String {
    let mut slug = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let mut slug = slug.trim_matches('-').to_string();
    if slug.len() > SLUG_MAX_BYTES {
        slug.truncate(SLUG_MAX_BYTES);
        if let Some(cut) = slug.rfind('-').filter(|at| *at > SLUG_MAX_BYTES / 2) {
            slug.truncate(cut);
        }
        slug = slug.trim_matches('-').to_string();
    }
    if slug.is_empty() || slug == "issues" {
        slug = format!("{}note", if slug.is_empty() { "" } else { "issues-" });
    }
    slug
}

/// The stub vissue writes on the logbook: the link, the title, the size.
/// A long title is cut to keep it under [`STUB_MAX_BYTES`].
fn stub_line(
    kind: Option<&str>,
    link: &str,
    title: &str,
    bytes: usize,
    lines: usize,
    trimmed: bool,
) -> String {
    let lead = match kind {
        Some(kind) => format!("long note {kind}: "),
        None => "long note: ".to_string(),
    };
    let tail = format!(
        " {} B {} lines{}",
        grouped(bytes),
        grouped(lines),
        if trimmed { " trimmed" } else { "" }
    );
    let frame = lead.len() + "[[file:".len() + link.len() + "][".len() + "]]".len() + tail.len();
    let room = STUB_MAX_BYTES.saturating_sub(frame).max(8);
    let title = if title.len() > room {
        let cut = floor_char(title, room.saturating_sub(3));
        format!("{}...", title[..cut].trim_end())
    } else {
        title.to_string()
    };
    format!("{lead}[[file:{link}][{title}]]{tail}")
}

/// The `#+key: value` lines at the top of a page, keys in lower case.
fn header_of(path: &Path) -> Vec<(String, String)> {
    use std::io::BufRead as _;
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in std::io::BufReader::new(file)
        .lines()
        .take(40)
        .map_while(std::result::Result::ok)
    {
        let Some(rest) = line.strip_prefix("#+") else {
            if line.trim().is_empty() && out.is_empty() {
                continue;
            }
            break;
        };
        if let Some((key, value)) = rest.split_once(':') {
            out.push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    out
}

fn field<'h>(header: &'h [(String, String)], key: &str) -> Option<&'h str> {
    header
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// The org pages in `dir`, regular files only, sorted by name.
fn pages(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "org"))
        .collect();
    out.sort();
    out
}

fn same_source(dir: &Path, digest: &str) -> Option<PathBuf> {
    pages(dir)
        .into_iter()
        .find(|page| field(&header_of(page), "source_sha256") == Some(digest))
}

fn free_page(dir: &Path, slug: &str) -> PathBuf {
    let first = dir.join(format!("{slug}.org"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{slug}-{n}.org")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

fn org_now() -> String {
    chrono::Local::now()
        .format("[%Y-%m-%d %a %H:%M]")
        .to_string()
}

/// Store the full body of a trimmed note in the local deed store.
fn store_full(issue: &str, what: &str, path: &Path, digest: &str) -> Result<String> {
    let seat = crate::tracker_seat();
    let file = path.display().to_string();
    let name = format!("long note {issue} {}", &digest[..16]);
    let made = crate::run_captured(
        "deedar",
        &[
            "create", "file", "--path", &file, "--name", &name, "--agent", &seat,
        ],
    )
        .with_context(|| {
            format!(
                "note: {what} is past the {} hard cap and the full body goes to the local deed store, which did not take it; pass --refuse-trim to refuse instead. Nothing was noted",
                grouped(LONG_NOTE_MAX_BYTES)
            )
        })?;
    let accession = made
        .stdout
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("accession="))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .with_context(|| format!("note: deedar printed no accession for {what} on {issue}"))?;
    Ok(accession.to_string())
}

fn read_source(file: &str) -> Result<Vec<u8>> {
    if file == "-" {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .context("note: could not read stdin")?;
        return Ok(bytes);
    }
    std::fs::read(crate::cwd_probe::from_start(Path::new(file)))
        .with_context(|| format!("note: could not read {file}"))
}

/// Check, convert and place one body. A dry run, or a body the ticket
/// already has, writes nothing.
fn plan(req: &Request<'_>, bytes: &[u8], dry_run: bool) -> Result<Planned> {
    let issue = req.issue;
    let what = match (req.body, req.file) {
        (Some(_), _) => "the note",
        (None, "-") => "stdin",
        (None, file) => file,
    };
    if let Some(kind) = req.kind {
        if !KINDS.contains(&kind) {
            bail!("note: --kind is one of {}, not {kind}", KINDS.join(", "));
        }
    }
    let text = text_body(issue, what, bytes)?;
    if text.trim().is_empty() {
        bail!("note: {what} is empty. Nothing was noted");
    }
    let mut lines = String::new();
    let trimmed = text.len() > LONG_NOTE_MAX_BYTES;
    if trimmed && req.refuse_trim {
        bail!(
            "note: {what} is {} B, past the {} B hard cap, and --refuse-trim keeps it whole; split it, or drop --refuse-trim to keep the first and last {} B. Nothing was noted",
            grouped(text.len()),
            grouped(LONG_NOTE_MAX_BYTES),
            grouped(TRIM_KEEP_BYTES)
        );
    }
    if text.len() > LONG_NOTE_WARN_BYTES && !trimmed {
        lines.push_str(&format!(
            "note: {what} is {} B, past the {} B soft cap for a long note; it is kept whole\n",
            grouped(text.len()),
            grouped(LONG_NOTE_WARN_BYTES)
        ));
    }
    let place = place(issue)?;
    let digest = crate::deed_bytes::sha256_hex(bytes);
    let markup = markup_of(req.file, req.kind);
    let own_title = match markup {
        Markup::Org => split_org_title(text).0,
        _ => None,
    };
    let title = req
        .title
        .map(str::to_string)
        .or(own_title)
        .unwrap_or_else(|| {
            if req.file == "-" {
                text.lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("note")
                    .trim_start_matches('#')
                    .chars()
                    .take(60)
                    .collect()
            } else {
                Path::new(req.file)
                    .file_name()
                    .map_or_else(|| req.file.to_string(), |n| n.to_string_lossy().into())
            }
        });
    let title = clean_title(&title);
    let title = if title.is_empty() {
        "note".to_string()
    } else {
        title
    };
    let line_count = text.lines().count();
    if let Some(page) = same_source(&place.dir, &digest) {
        let name = page.file_name().map(|n| n.to_string_lossy().into_owned());
        return Ok(Planned {
            rel: rel_to(&place.root, &page),
            stub: stub_line(
                req.kind,
                &format!("{}/{}", place.link, name.unwrap_or_default()),
                &title,
                text.len(),
                line_count,
                trimmed,
            ),
            path: page,
            duplicate: true,
            deed: None,
            lines,
            bytes: text.len(),
            trimmed,
        });
    }
    let page = free_page(&place.dir, &slug_of(&title));
    let name = page
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stub = stub_line(
        req.kind,
        &format!("{}/{name}", place.link),
        &title,
        text.len(),
        line_count,
        trimmed,
    );
    let rel = rel_to(&place.root, &page);
    if dry_run {
        return Ok(Planned {
            path: page,
            rel,
            stub,
            duplicate: false,
            deed: None,
            lines,
            bytes: text.len(),
            trimmed,
        });
    }
    let (body, deed) = if trimmed {
        let scratch;
        let full = if req.file == "-" {
            scratch = tempfile::tempdir().context("note: no scratch directory")?;
            let path = scratch.path().join("note.txt");
            crate::write_file(&path, bytes).context("note: could not keep stdin")?;
            path
        } else {
            crate::cwd_probe::from_start(Path::new(req.file))
        };
        let accession = store_full(issue, what, &full, &digest)?;
        let (head, tail) = head_and_tail(text);
        let elision = format!(
            "[ljos: trimmed: the full body is {} B, sha256:{digest}, deed {accession}; this note keeps the first and last {} B]\n",
            grouped(text.len()),
            grouped(TRIM_KEEP_BYTES)
        );
        let kept = format!(
            "{head}{}{elision}{tail}",
            if head.ends_with('\n') { "" } else { "\n" }
        );
        (example_block(&kept), Some(accession))
    } else {
        (to_org(text, markup), None)
    };
    let mut page_text = format!("#+title: {title}\n#+ticket: {issue}\n");
    if let Some(kind) = req.kind {
        page_text.push_str(&format!("#+kind: {kind}\n"));
    }
    page_text.push_str(&format!(
        "#+seat: {}\n#+created: {}\n",
        crate::tracker_seat(),
        org_now()
    ));
    if req.file != "-" {
        if let Some(name) = Path::new(req.file).file_name() {
            page_text.push_str(&format!("#+source: {}\n", name.to_string_lossy()));
        }
    }
    page_text.push_str(&format!(
        "#+source_sha256: {digest}\n#+source_bytes: {}\n",
        text.len()
    ));
    if let Some(accession) = &deed {
        page_text.push_str(&format!(
            "#+trimmed: the full body is {} B in deed {accession}\n",
            grouped(text.len())
        ));
    }
    page_text.push('\n');
    page_text.push_str(&body);
    if !page_text.ends_with('\n') {
        page_text.push('\n');
    }
    std::fs::create_dir_all(&place.dir)
        .with_context(|| format!("note: could not make {}", place.dir.display()))?;
    publish_page(&page, &page_text)?;
    Ok(Planned {
        path: page,
        rel,
        stub,
        duplicate: false,
        deed,
        lines,
        bytes: text.len(),
        trimmed,
    })
}

/// Fail the publish after the temporary is synced, when a test asks.
fn refuse_page_publish(tmp: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if tests::page_write_fault(tmp) {
        return Err(std::io::Error::other("injected"));
    }
    let _ = tmp;
    Ok(())
}

/// Write `text` to `page` through a temporary in the same directory.
///
/// The temporary is synced and then renamed into place. Any failure removes
/// the temporary and leaves `page` as it was, so the caller writes no stub.
///
/// # Errors
///
/// The temporary cannot be written or synced, or the rename fails.
fn publish_page(page: &Path, text: &str) -> Result<()> {
    crate::tracker_tmp::replace_file_with(page, text, refuse_page_publish)
        .with_context(|| format!("note: could not write {}", page.display()))
}

/// Put the stub on the logbook and cite a trimmed note's deed. The page is
/// removed again when the tracker refuses the stub, so no page is left
/// that no ticket names.
fn attach(issue: &str, planned: &Planned) -> Result<String> {
    let noted = crate::run_for(
        "vissue",
        &["note", issue, &planned.stub],
        crate::tracker_verb_wait(),
    );
    let noted = match noted {
        Ok(said) => said,
        Err(e) => {
            let _ = std::fs::remove_file(&planned.path);
            return Err(e);
        }
    };
    if let Some(accession) = &planned.deed {
        crate::run_captured("vissue", &["deed", issue, "--add", accession])
            .with_context(|| format!("note: could not cite {accession} on {issue}"))?;
    }
    Ok(noted.stderr)
}

/// `ljos note ID --file PATH`: write the page, put the stub on the
/// logbook, and commit both.
///
/// # Errors
///
/// The body is binary or encoded, empty, past the hard cap with
/// `--refuse-trim`, the ticket is unknown, or the tracker refused the stub.
pub fn note_file(req: &Request<'_>) -> Result<String> {
    let bytes = match req.body {
        Some(body) => body.as_bytes().to_vec(),
        None => read_source(req.file)?,
    };
    let planned = plan(req, &bytes, false)?;
    say_raw!("{}", planned.lines);
    if planned.duplicate {
        return Ok(format!(
            "{}: already noted as {}; nothing written\n",
            req.issue, planned.rel
        ));
    }
    let stderr = attach(req.issue, &planned)?;
    say_raw!("{stderr}");
    index_pages(std::slice::from_ref(&planned.path));
    let mut said = format!(
        "{}: noted {} B as {}\n",
        req.issue,
        grouped(planned.bytes),
        planned.rel
    );
    if let Some(accession) = &planned.deed {
        said.push_str(&format!(
            "{}: kept the first and last {} B; the full body is deed {accession}\n",
            req.issue,
            grouped(TRIM_KEEP_BYTES)
        ));
    }
    said.push_str(&crate::persist_tracker_with(
        req.issue,
        "noted",
        std::slice::from_ref(&planned.rel),
    ));
    Ok(said)
}

/// What `ljos notes import` did.
pub struct Imported {
    pub text: String,
    pub refused: usize,
}

fn import_kind(name: &str) -> Option<&'static str> {
    let upper = name.to_ascii_uppercase();
    if upper.starts_with("LAND") {
        Some("land")
    } else if upper.starts_with("REPORT") {
        Some("report")
    } else if upper.starts_with("GATE") {
        Some("gate")
    } else if upper.ends_with(".LOG") {
        Some("log")
    } else {
        None
    }
}

/// `ljos notes import DIR --ticket ID`: each `*.md`, `*.txt` and `*.log`
/// file directly in DIR becomes a long note on ID, in one tracker commit.
/// A file whose bytes the ticket already has is left. Every other file is
/// named and skipped.
///
/// # Errors
///
/// DIR cannot be read, or the ticket is unknown.
pub fn import(dir: &Path, issue: &str, dry_run: bool) -> Result<Imported> {
    place(issue)?;
    let entries = std::fs::read_dir(crate::cwd_probe::from_start(dir))
        .with_context(|| format!("import: could not read {}", dir.display()))?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| entry.path())
        .collect();
    files.sort();
    let mut out = String::new();
    let (mut noted, mut already, mut skipped, mut refused) = (0, 0, 0, 0);
    let mut rels = Vec::new();
    let mut written = Vec::new();
    for file in &files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = file
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !IMPORT_EXTENSIONS.contains(&ext.as_str()) {
            skipped += 1;
            out.push_str(&format!("skipped {name}: not .md, .txt or .log\n"));
            continue;
        }
        let path = file.display().to_string();
        let req = Request {
            issue,
            file: &path,
            body: None,
            title: None,
            kind: import_kind(&name),
            refuse_trim: false,
        };
        let planned = std::fs::read(file)
            .with_context(|| format!("could not read {name}"))
            .and_then(|bytes| plan(&req, &bytes, dry_run));
        let planned = match planned {
            Ok(planned) => planned,
            Err(e) => {
                refused += 1;
                out.push_str(&format!("refused {name}: {e:#}\n"));
                continue;
            }
        };
        out.push_str(&planned.lines);
        if planned.duplicate {
            already += 1;
            out.push_str(&format!("already noted {name} as {}\n", planned.rel));
            continue;
        }
        let trimmed = if planned.trimmed { ", trimmed" } else { "" };
        if dry_run {
            noted += 1;
            out.push_str(&format!(
                "would note {name} as {} ({} B{trimmed})\n",
                planned.rel,
                grouped(planned.bytes)
            ));
            continue;
        }
        match attach(issue, &planned) {
            Ok(_) => {
                noted += 1;
                out.push_str(&format!(
                    "noted {name} as {} ({} B{trimmed})\n",
                    planned.rel,
                    grouped(planned.bytes)
                ));
                rels.push(planned.rel.clone());
                written.push(planned.path.clone());
            }
            Err(e) => {
                refused += 1;
                out.push_str(&format!("refused {name}: {e:#}\n"));
            }
        }
    }
    let verb = if dry_run { "would note" } else { "noted" };
    out.push_str(&format!(
        "import: {verb} {noted}, already noted {already}, skipped {skipped}, refused {refused}\n"
    ));
    if !written.is_empty() {
        index_pages(&written);
        out.push_str(&crate::persist_tracker_with(issue, "noted", &rels));
    }
    Ok(Imported { text: out, refused })
}

/// One page as `ljos notes` lists it.
struct Listed {
    path: PathBuf,
    rel: String,
    title: String,
    kind: String,
    seat: String,
    created: String,
    bytes: u64,
    lines: usize,
    sha: String,
}

fn listed(place: &Place) -> Vec<Listed> {
    let mut out: Vec<Listed> = pages(&place.dir)
        .into_iter()
        .map(|path| {
            let header = header_of(&path);
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let get = |key| field(&header, key).unwrap_or("").to_string();
            Listed {
                rel: rel_to(&place.root, &path),
                title: get("title"),
                kind: get("kind"),
                seat: get("seat"),
                created: get("created"),
                bytes: std::fs::metadata(&path).map_or(0, |m| m.len()),
                lines: text.lines().count(),
                sha: get("source_sha256"),
                path,
            }
        })
        .collect();
    out.sort_by(|a, b| (&a.created, &a.rel).cmp(&(&b.created, &b.rel)));
    out
}

/// `ljos notes ID`: the ticket's long notes, oldest first.
///
/// # Errors
///
/// The ticket is unknown.
pub fn list(issue: &str, json: bool) -> Result<String> {
    let place = place(issue)?;
    let rows = listed(&place);
    if json {
        let rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "path": row.rel,
                    "title": row.title,
                    "kind": row.kind,
                    "seat": row.seat,
                    "created": row.created,
                    "bytes": row.bytes,
                    "lines": row.lines,
                    "source_sha256": row.sha,
                })
            })
            .collect();
        return Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        ));
    }
    if rows.is_empty() {
        return Ok(format!(
            "{issue}: no long notes; `ljos note {issue} --file PATH` adds one\n"
        ));
    }
    let mut out = format!(
        "{issue}: {} long note{}\n",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" }
    );
    for row in &rows {
        let kind = if row.kind.is_empty() {
            "note"
        } else {
            &row.kind
        };
        out.push_str(&format!(
            "{}\n  {kind}, {} B, {} lines, {}, {}\n  {}\n",
            row.title,
            grouped(row.bytes as usize),
            grouped(row.lines),
            if row.seat.is_empty() { "-" } else { &row.seat },
            if row.created.is_empty() {
                "-"
            } else {
                &row.created
            },
            row.rel
        ));
    }
    Ok(out)
}

/// `ljos show ID --long [NOTE]`: one page as it is on disk. NOTE is the
/// file name, its stem, its path, or a prefix of its source digest; none
/// is the newest.
///
/// # Errors
///
/// The ticket is unknown, has no long note, or NOTE names none or several.
pub fn show(issue: &str, which: Option<&str>) -> Result<String> {
    let place = place(issue)?;
    let rows = listed(&place);
    let Some(newest) = rows.last() else {
        bail!("{issue}: no long notes; `ljos note {issue} --file PATH` adds one");
    };
    let chosen = match which.map(str::trim).filter(|w| !w.is_empty()) {
        None => newest,
        Some(want) => {
            let want_hex = want.strip_prefix("sha256:").unwrap_or(want);
            let hits: Vec<&Listed> = rows
                .iter()
                .filter(|row| {
                    let name = row
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    name == want
                        || name.strip_suffix(".org") == Some(want)
                        || row.rel == want
                        || row.rel.ends_with(&format!("/{want}"))
                        || (want_hex.len() >= 6
                            && want_hex.bytes().all(|b| b.is_ascii_hexdigit())
                            && row.sha.starts_with(want_hex))
                })
                .collect();
            match hits.as_slice() {
                [one] => *one,
                [] => bail!("{issue}: no long note is {want}; `ljos notes {issue}` lists them"),
                many => bail!(
                    "{issue}: {want} names {} long notes: {}",
                    many.len(),
                    many.iter()
                        .map(|row| row.rel.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
    };
    std::fs::read_to_string(&chosen.path)
        .with_context(|| format!("show: could not read {}", chosen.rel))
}

// ---- the local index ------------------------------------------------------

/// Where the index of `root` lives. The directory holds a `.gitignore` of
/// `*`, so git never sees it.
fn index_file(root: &Path) -> PathBuf {
    root.join(".ljos").join("cache").join("notes.idx")
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Entry {
    path: String,
    mtime_ns: u128,
    size: u64,
    ticket: String,
    title: String,
    text: String,
}

fn stamp(path: &Path) -> Option<(u128, u64)> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((mtime, meta.len()))
}

fn load_index(root: &Path) -> Vec<Entry> {
    std::fs::read_to_string(index_file(root))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn save_index(root: &Path, entries: &[Entry]) {
    let file = index_file(root);
    let Some(dir) = file.parent() else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        let _ = crate::write_file(&ignore, "*\n");
    }
    let mut text = String::new();
    for entry in entries {
        if let Ok(line) = serde_json::to_string(entry) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    let _ = crate::tracker_tmp::replace_file(&file, &text);
}

fn entry_for(root: &Path, path: &Path) -> Option<Entry> {
    let (mtime_ns, size) = stamp(path)?;
    let text = std::fs::read_to_string(path).ok()?;
    let header = header_of(path);
    Some(Entry {
        path: rel_to(root, path),
        mtime_ns,
        size,
        ticket: field(&header, "ticket").unwrap_or("").to_string(),
        title: field(&header, "title").unwrap_or("").to_string(),
        text,
    })
}

/// Put `paths` in the index of the checkout that holds them.
fn index_pages(paths: &[PathBuf]) {
    let Some(first) = paths.first().and_then(|p| p.parent()) else {
        return;
    };
    let root = index_root(first);
    let mut entries = load_index(&root);
    for path in paths {
        let Some(entry) = entry_for(&root, path) else {
            continue;
        };
        entries.retain(|e| e.path != entry.path);
        entries.push(entry);
    }
    save_index(&root, &entries);
}

/// Every long-note page under the boards of the tracker at `layout`.
fn all_pages(layout: &vissue_core::Layout) -> Vec<PathBuf> {
    let projects = vissue_core::store::list_projects(layout).unwrap_or_default();
    let base = layout.projects_dir();
    let mut out = Vec::new();
    for project in projects {
        let notes = base.join(&project).join("notes");
        let Ok(tickets) = std::fs::read_dir(&notes) else {
            continue;
        };
        for ticket in tickets.flatten() {
            if ticket.file_type().is_ok_and(|t| t.is_dir()) {
                out.extend(pages(&ticket.path()));
            }
        }
    }
    out
}

/// The index brought up to date: a page whose size or modification time
/// differs from its entry is read again, a new page is added, and a page
/// that is gone is dropped. Only `ljos search --long` calls this.
fn refreshed(root: &Path, pages: &[PathBuf]) -> Vec<Entry> {
    let old = load_index(root);
    let mut changed = old.len() != pages.len();
    let mut entries = Vec::with_capacity(pages.len());
    for path in pages {
        let rel = rel_to(root, path);
        let Some((mtime_ns, size)) = stamp(path) else {
            continue;
        };
        match old.iter().find(|e| e.path == rel) {
            Some(e) if e.mtime_ns == mtime_ns && e.size == size => entries.push(e.clone()),
            _ => {
                changed = true;
                if let Some(entry) = entry_for(root, path) {
                    entries.push(entry);
                }
            }
        }
    }
    if changed {
        save_index(root, &entries);
    }
    entries
}

/// `ljos search --long QUERY`: long notes whose title or text holds every
/// word of the query, case folded. Each hit names its ticket, title and
/// path, and the first lines that match.
///
/// # Errors
///
/// No tracker is configured.
pub fn search(query: &str, limit: usize) -> Result<String> {
    let layout = vissue_core::Layout::resolve(None, None)
        .map_err(|e| anyhow::anyhow!("search: no tracker: {e}"))?;
    let root = index_root(layout.root());
    let entries = refreshed(&root, &all_pages(&layout));
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        bail!("search: the query is empty, so run `ljos search --long \"QUERY\"`.");
    }
    let mut out = String::new();
    let mut shown = 0;
    for entry in &entries {
        let haystack = format!("{}\n{}", entry.title, entry.text).to_lowercase();
        if !words.iter().all(|w| haystack.contains(w.as_str())) {
            continue;
        }
        shown += 1;
        if shown > limit {
            continue;
        }
        out.push_str(&format!(
            "{}  {}\n  {}\n",
            entry.ticket, entry.title, entry.path
        ));
        for (at, line) in entry
            .text
            .lines()
            .enumerate()
            .filter(|(_, l)| {
                let l = l.to_lowercase();
                words.iter().any(|w| l.contains(w.as_str()))
            })
            .take(2)
        {
            let line = line.trim();
            let cut = floor_char(line, 120);
            out.push_str(&format!(
                "  {}: {}{}\n",
                at + 1,
                &line[..cut],
                if cut < line.len() { "..." } else { "" }
            ));
        }
    }
    if shown == 0 {
        return Ok(format!(
            "No long note holds {query:?}. `ljos notes ID` lists a ticket's long notes.\n"
        ));
    }
    if shown > limit {
        out.push_str(&format!("{} more; pass -n to show them\n", shown - limit));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        static FAIL_PAGE_WRITE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static PAGE_TEMP_SEEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Whether this publish should fail. Records that the temporary was a
    /// file, which is what the failure test asserts was removed.
    pub(super) fn page_write_fault(tmp: &Path) -> bool {
        if !FAIL_PAGE_WRITE.with(std::cell::Cell::get) {
            return false;
        }
        PAGE_TEMP_SEEN.with(|seen| seen.set(tmp.is_file()));
        true
    }

    #[test]
    fn grouped_puts_commas_every_three_digits() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(9412), "9,412");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    #[test]
    fn a_stub_stays_under_its_cap_with_a_long_title() {
        let _env_read = crate::test_env_read_lock();
        let title = "a very long title ".repeat(20);
        let stub = stub_line(
            Some("report"),
            "../notes/Software%2Fljos-3n59/a-very-long-title-a-very-long-title.org",
            title.trim(),
            1_048_576,
            123_456,
            true,
        );
        assert!(stub.len() <= STUB_MAX_BYTES, "{} {stub}", stub.len());
        assert!(
            stub.starts_with("long note report: [[file:../notes/"),
            "{stub}"
        );
        assert!(
            stub.ends_with("1,048,576 B 123,456 lines trimmed"),
            "{stub}"
        );
    }

    #[test]
    fn a_slug_is_short_lowercase_and_never_issues() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(slug_of("LAND r3"), "land-r3");
        assert_eq!(slug_of("REPORT.md"), "report-md");
        assert_eq!(slug_of("!!!"), "note");
        assert_eq!(slug_of("issues"), "issues-note");
        assert!(slug_of(&"word ".repeat(30)).len() <= SLUG_MAX_BYTES);
    }

    #[test]
    fn binary_and_encoded_bodies_are_refused_with_the_deed_route() {
        let _env_read = crate::test_env_read_lock();
        let nul = text_body("p-1", "x.bin", b"a\0b").unwrap_err().to_string();
        assert!(
            nul.contains("NUL") && nul.contains("deedar create file"),
            "{nul}"
        );
        let bad = text_body("p-1", "x", &[0xff, 0xfe])
            .unwrap_err()
            .to_string();
        assert!(bad.contains("not UTF-8"), "{bad}");
        let b64 = format!("head\n{}\n", "QUJD".repeat(1_000));
        let enc = text_body("p-1", "x", b64.as_bytes())
            .unwrap_err()
            .to_string();
        assert!(
            enc.contains("line 2") && enc.contains("ljos deed p-1 --add"),
            "{enc}"
        );
        let labelled = format!("snapshot b64z:{}\n", "QUJD".repeat(1_000));
        let enc = text_body("p-1", "x", labelled.as_bytes())
            .unwrap_err()
            .to_string();
        assert!(enc.contains("a run of 4,005 bytes"), "{enc}");
        let prose = format!("{}\n", "word ".repeat(1_000));
        assert!(text_body("p-1", "x", prose.as_bytes()).is_ok());
    }

    #[test]
    fn head_and_tail_keep_whole_lines_from_each_end() {
        let _env_read = crate::test_env_read_lock();
        let line = "0123456789abcdef\n";
        let text = line.repeat(2 * LONG_NOTE_MAX_BYTES / line.len());
        let (head, tail) = head_and_tail(&text);
        assert!(head.len() <= TRIM_KEEP_BYTES && head.len() > TRIM_KEEP_BYTES - line.len());
        assert!(tail.len() <= TRIM_KEEP_BYTES && tail.len() > TRIM_KEEP_BYTES - line.len());
        assert!(head.ends_with('\n') && tail.starts_with('0'));
    }

    #[test]
    fn an_example_block_escapes_lines_org_would_read() {
        let _env_read = crate::test_env_read_lock();
        let block = example_block("* not a heading\n#+end_example\nplain\n");
        assert_eq!(
            block,
            "#+begin_example\n,* not a heading\n,#+end_example\nplain\n#+end_example\n"
        );
    }

    #[test]
    fn markdown_becomes_org_without_pandoc() {
        let _env_read = crate::test_env_read_lock();
        let md = "# Title\n\nSome **bold** and `code` and [a link](https://example.org).\n\n* item\n  * nested\n\n```rust\nfn main() {}\n* inside\n```\n\n> quoted\n\n---\n";
        let org = markdown_to_org(md);
        assert_eq!(
            org,
            "* Title\n\nSome *bold* and ~code~ and [[https://example.org][a link]].\n\n- item\n  - nested\n\n#+begin_src rust\nfn main() {}\n,* inside\n#+end_src\n\n#+begin_quote\nquoted\n#+end_quote\n\n-----\n"
        );
    }

    #[test]
    fn an_org_title_is_taken_from_the_body() {
        let _env_read = crate::test_env_read_lock();
        let (title, body) = split_org_title("#+title: Gate r2\n* Findings\n");
        assert_eq!(title.as_deref(), Some("Gate r2"));
        assert_eq!(body, "* Findings\n");
    }

    #[test]
    fn import_kinds_follow_kit_file_names() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(import_kind("LAND.md"), Some("land"));
        assert_eq!(import_kind("REPORT-r2.md"), Some("report"));
        assert_eq!(import_kind("GATE-gpt.md"), Some("gate"));
        assert_eq!(import_kind("build.log"), Some("log"));
        assert_eq!(import_kind("README.md"), None);
    }

    /// The publish fails after the temporary is synced. The page, the
    /// temporary and the logbook stub are all absent.
    #[test]
    fn a_failed_page_write_leaves_no_page_no_temp_and_no_stub() {
        let _env = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("Software/demo");
        std::fs::create_dir_all(&project).unwrap();
        let issues = project.join("issues.org");
        let ticket = "\
#+TITLE: demo issues
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#B] Scoreboard
:PROPERTIES:
:ID:         demo-a1b2
:END:
";
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&issues, ticket).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let init = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(init.success(), "git init");
        crate::with_env_mutation(|| unsafe {
            std::env::set_var("VISSUE_ROOT", dir.path());
            std::env::set_var("ISSUE_ROOT", dir.path());
            std::env::set_var("VISSUE_NO_ROUTE", "1");
            std::env::set_var("LJOS_SEAT", "alice");
            std::env::set_var("LJOS_TRACKER_GIT", "off");
            std::env::set_var("LJOS_HOOKD", "off");
            std::env::set_var("PACKSET_URL", "off");
            std::env::set_var("HOME", dir.path());
        });
        struct Arm;
        impl Arm {
            fn new() -> Self {
                FAIL_PAGE_WRITE.with(|cell| cell.set(true));
                PAGE_TEMP_SEEN.with(|cell| cell.set(false));
                Self
            }
        }
        impl Drop for Arm {
            fn drop(&mut self) {
                FAIL_PAGE_WRITE.with(|cell| cell.set(false));
            }
        }
        let _arm = Arm::new();
        let err = note_file(&Request {
            issue: "demo-a1b2",
            file: "-",
            body: Some("The landing is green and the gate holds.\n"),
            title: Some("Gate"),
            kind: Some("gate"),
            refuse_trim: false,
        })
        .unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("could not write"), "{err}");
        assert!(
            PAGE_TEMP_SEEN.with(std::cell::Cell::get),
            "the temporary was never written"
        );
        let notes = project.join("notes/demo-a1b2");
        let names: Vec<String> = std::fs::read_dir(&notes)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            names.iter().all(|name| !name.ends_with(".org")),
            "a page remained: {names:?}"
        );
        assert!(
            names
                .iter()
                .all(|name| !crate::tracker_tmp::is_temp_name(name)),
            "a temporary remained: {names:?}"
        );
        assert!(!notes.join("gate.org").exists());
        assert_eq!(std::fs::read_to_string(&issues).unwrap(), ticket);
    }
}
