//! Wrap human-facing prose to terminal width on a TTY.
//!
//! Valid `COLUMNS` overrides the TTY width. Non-TTY output is unchanged,
//! and so is output once `crate::output::set_output_kind` selects it.

use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A narrower `COLUMNS` or terminal is wrapped at this width.
pub const MIN_COLS: usize = 20;
/// A wider `COLUMNS` or terminal is wrapped at this width.
pub const MAX_COLS: usize = 240;

/// The width a human page is built to.
///
/// There is no default. A page builder takes the `Width` of the stream its
/// text is written to: [`Width::stdout`], [`Width::stderr`], or
/// [`Width::FIXED`] for a destination that is never a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Width(Option<usize>);

impl Width {
    /// The fixed 80-column layout that pipes, files, hooks and the MCP
    /// server get.
    pub const FIXED: Self = Self(None);

    /// The width of stdout: its terminal width, or [`Width::FIXED`] when
    /// stdout is not a terminal or the process writes machine output.
    #[must_use]
    pub fn stdout() -> Self {
        Self(prose_width_for_stdout().filter(|_| !crate::output::machine_output()))
    }

    /// The width of stderr: its terminal width, or [`Width::FIXED`] when
    /// stderr is not a terminal or the process writes machine output.
    #[must_use]
    pub fn stderr() -> Self {
        Self(prose_width_for_stderr().filter(|_| !crate::output::machine_output()))
    }

    /// A terminal `cols` columns wide.
    #[must_use]
    pub fn columns(cols: usize) -> Self {
        Self(Some(cols.max(1)))
    }

    /// The terminal width, or `None` for [`Width::FIXED`].
    #[must_use]
    pub fn terminal(self) -> Option<usize> {
        self.0
    }
}

/// Width for human prose output.
///
/// On a TTY, `COLUMNS` wins when it is a positive integer. Without it, the
/// width comes from the terminal. Either width is clamped to
/// `MIN_COLS..=MAX_COLS`. Non-TTY output and a terminal that reports zero
/// columns return `None` so bytes stay stable.
#[must_use]
pub fn prose_width_for_stdout() -> Option<usize> {
    prose_width(
        crate::output::stdout_is_terminal(),
        std::env::var("COLUMNS").ok(),
    )
}

/// Width for human prose sent to stderr.
#[must_use]
pub fn prose_width_for_stderr() -> Option<usize> {
    prose_width_with(
        crate::output::stderr_is_terminal(),
        std::env::var("COLUMNS").ok(),
        tty_columns_for_fd(libc::STDERR_FILENO),
    )
}

/// Width from TTY state and optional `COLUMNS`, mainly for tests.
#[must_use]
pub fn prose_width(stdout_is_tty: bool, columns_env: Option<String>) -> Option<usize> {
    prose_width_with(
        stdout_is_tty,
        columns_env,
        tty_columns_for_fd(libc::STDOUT_FILENO),
    )
}

#[must_use]
fn prose_width_with(
    stdout_is_tty: bool,
    columns_env: Option<String>,
    tty_width: Option<usize>,
) -> Option<usize> {
    if !stdout_is_tty {
        return None;
    }
    match columns_env.as_deref().and_then(parse_columns) {
        Some(width) => Some(width),
        None => tty_width.and_then(sanitize_width),
    }
}

/// Wrap prose lines to `width`.
///
/// Doctor rows (`ok|info|warn|fail<TAB>name<TAB>state`) keep their first two
/// fields as a hanging prefix. Bare command lines stay whole. URLs and paths
/// are unbreakable tokens inside wrapped lines.
#[must_use]
pub fn wrap_prose(text: &str, width: Option<usize>) -> String {
    let Some(cols) = width else {
        return text.to_string();
    };
    text.split_inclusive('\n')
        .map(|chunk| wrap_chunk(chunk, cols))
        .collect()
}

/// Wrap one prose body line under a hanging prefix.
#[must_use]
pub fn wrap_hanging_prose(body: &str, cols: usize, first: &str, cont: &str) -> String {
    wrap_with_prefix(body, cols.max(1), first, cont, true)
}

/// Display width used by prose wrapping.
#[must_use]
pub fn prose_display_width(text: &str) -> usize {
    visible_width(text)
}

/// Remove ANSI escape sequences while keeping plain text.
#[must_use]
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for ch in chars.by_ref() {
                    if ('@'..='~').contains(&ch) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(ch) = chars.next() {
                    if ch == '\u{7}' {
                        break;
                    }
                    if ch == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {}
            None => {}
        }
    }
    out
}

fn wrap_chunk(chunk: &str, cols: usize) -> String {
    let (line, had_nl) = if let Some(stripped) = chunk.strip_suffix('\n') {
        (stripped, true)
    } else {
        (chunk, false)
    };
    let wrapped = wrap_line(line, cols);
    if had_nl {
        format!("{wrapped}\n")
    } else {
        wrapped
    }
}

fn wrap_line(line: &str, cols: usize) -> String {
    if line.trim().is_empty() {
        return line.to_string();
    }
    if let Some((left, name, state)) = split_doctor_row(line) {
        let first = format!("{left}  {name}  ");
        let cont = " ".repeat(visible_width(&first));
        return wrap_with_prefix(state, cols, &first, &cont, true);
    }
    if visible_width(line) <= cols || keep_whole_line(line) {
        return line.to_string();
    }
    let (indent, body) = split_indent(line);
    wrap_with_prefix(body, cols, indent, indent, false)
}

fn split_doctor_row(line: &str) -> Option<(&str, &str, &str)> {
    let mut parts = line.splitn(3, '\t');
    let left = parts.next()?;
    if !crate::is_doctor_word(left) {
        return None;
    }
    let name = parts.next()?;
    let state = parts.next()?;
    Some((left, name, state))
}

fn wrap_with_prefix(
    body: &str,
    cols: usize,
    first: &str,
    cont: &str,
    always_render_prefix: bool,
) -> String {
    let mut out = String::new();
    for (index, line) in body.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let lead = if index == 0 { first } else { cont };
        out.push_str(&wrap_with_prefix_line(
            line,
            cols,
            lead,
            cont,
            always_render_prefix,
        ));
    }
    if out.is_empty() {
        return first.trim_end_matches([' ', '\t']).to_string();
    }
    out
}

fn wrap_with_prefix_line(
    body: &str,
    cols: usize,
    first: &str,
    cont: &str,
    always_render_prefix: bool,
) -> String {
    if body.trim().is_empty() {
        return first.trim_end_matches([' ', '\t']).to_string();
    }
    let segs = prose_segments(body);
    if segs.is_empty() {
        return first.trim_end_matches([' ', '\t']).to_string();
    }
    let first_w = visible_width(first);
    let first_word_w = segs
        .iter()
        .find_map(|seg| {
            if seg.space {
                return None;
            }
            let w = visible_width(&seg.text);
            if w > cols && breakable_word(&seg.text) {
                return word_atoms(&seg.text)
                    .into_iter()
                    .map(|atom| atom.width)
                    .find(|w| *w > 0);
            }
            Some(w)
        })
        .unwrap_or(0);
    if first_w >= cols {
        let mut combined = String::from(first.trim_end_matches([' ', '\t']));
        if !combined.is_empty() && !body.is_empty() {
            combined.push(' ');
        }
        combined.push_str(body);
        return wrap_segments(&prose_segments(&combined), cols, "", "", false);
    }
    if first_w + first_word_w > cols {
        let mut fallback = first.trim_end_matches([' ', '\t']).to_string();
        if !fallback.is_empty() {
            fallback.push('\n');
        }
        fallback.push_str(&wrap_segments(&segs, cols, "", "", false));
        return fallback;
    }
    wrap_segments(&segs, cols, first, cont, always_render_prefix)
}

fn wrap_segments(
    segs: &[Seg],
    cols: usize,
    first: &str,
    cont: &str,
    always_render_prefix: bool,
) -> String {
    let mut out = String::new();
    let mut line = first.to_string();
    let mut line_w = visible_width(&line);
    let mut has_body = false;
    let cont_w = visible_width(cont);
    let push_line = |out: &mut String, line: &mut String, line_w: &mut usize| {
        while line.ends_with(' ') || line.ends_with('\t') {
            line.pop();
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        *line_w = visible_width(line);
    };
    for seg in segs {
        let w = visible_width_from(&seg.text, line_w);
        if seg.space {
            if !has_body && line_w + w <= cols {
                line.push_str(&seg.text);
                line_w += w;
                continue;
            }
            if !has_body {
                continue;
            }
            if line_w + w <= cols {
                line.push_str(&seg.text);
                line_w += w;
            } else {
                push_line(&mut out, &mut line, &mut line_w);
                line.clear();
                if cont_w + w <= cols {
                    line.push_str(cont);
                    line_w = cont_w;
                } else {
                    line_w = 0;
                }
                has_body = false;
            }
            continue;
        }
        if w > cols && line_w + w > cols && breakable_word(&seg.text) {
            let mut held = String::new();
            for atom in word_atoms(&seg.text) {
                let piece = &seg.text[atom.range];
                let gw = atom.width;
                if gw == 0 {
                    held.push_str(piece);
                    continue;
                }
                if has_body && line_w + gw > cols {
                    push_line(&mut out, &mut line, &mut line_w);
                    line.clear();
                    if cont_w + gw <= cols {
                        line.push_str(cont);
                        line_w = cont_w;
                    } else {
                        line_w = 0;
                    }
                }
                if !has_body && line_w + gw > cols {
                    line.clear();
                    line_w = 0;
                }
                line.push_str(&held);
                held.clear();
                line.push_str(piece);
                line_w += gw;
                has_body = true;
            }
            line.push_str(&held);
            continue;
        }
        if has_body && line_w + w > cols {
            push_line(&mut out, &mut line, &mut line_w);
            line.clear();
            if cont_w + w <= cols {
                line.push_str(cont);
                line_w = cont_w;
            } else {
                line_w = 0;
            }
        }
        if !has_body && line_w + w > cols {
            line.clear();
            line_w = 0;
        }
        line.push_str(&seg.text);
        line_w += w;
        has_body = true;
    }
    if has_body || always_render_prefix {
        push_line(&mut out, &mut line, &mut line_w);
    }
    out
}

#[derive(Clone)]
struct Seg {
    text: String,
    space: bool,
}

fn prose_segments(text: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < text.len() {
        let ch = text[i..].chars().next().unwrap_or('\0');
        if break_space(ch) {
            let start = i;
            while i < text.len() {
                let ch = text[i..].chars().next().unwrap_or('\0');
                if !break_space(ch) {
                    break;
                }
                i += ch.len_utf8();
            }
            out.push(Seg {
                text: text[start..i].to_string(),
                space: true,
            });
            continue;
        }
        let start = i;
        let mut open_ticks: Option<usize> = None;
        let mut in_osc8 = false;
        while i < text.len() {
            let ch = text[i..].chars().next().unwrap_or('\0');
            if in_osc8 && ch != '\x1b' {
                i += ch.len_utf8();
                continue;
            }
            if ch == '\x1b' {
                if let Some(tok) = escape_token_at(text, i) {
                    if let Some(close) = tok.osc8_close {
                        if in_osc8 {
                            if close {
                                in_osc8 = false;
                            }
                        } else if !close {
                            in_osc8 = true;
                        }
                    }
                    i = tok.end;
                    continue;
                }
            }
            if open_ticks.is_none() && ch == '`' {
                let run = tick_run(text, i);
                if has_closing_ticks(text, i + run, run) {
                    open_ticks = Some(run);
                    i += run;
                } else {
                    i += run;
                }
                continue;
            }
            if let Some(run) = open_ticks {
                if ch == '`' {
                    let got = tick_run(text, i);
                    if got == run {
                        i += run;
                        open_ticks = None;
                        continue;
                    }
                    i += got;
                    continue;
                }
                i += ch.len_utf8();
                continue;
            }
            if break_space(ch) {
                break;
            }
            i += ch.len_utf8();
        }
        while i < text.len() {
            let ch = text[i..].chars().next().unwrap_or('\0');
            if !matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']') {
                break;
            }
            i += ch.len_utf8();
        }
        out.push(Seg {
            text: text[start..i].to_string(),
            space: false,
        });
    }
    out
}

/// Whitespace a line may break at. A no-break space (U+00A0), a narrow
/// no-break space (U+202F) and a figure space (U+2007) join their words.
fn break_space(ch: char) -> bool {
    ch.is_whitespace() && !matches!(ch, '\u{a0}' | '\u{202f}' | '\u{2007}')
}

fn tick_run(text: &str, start: usize) -> usize {
    let mut n = 0usize;
    while start + n < text.len() && text.as_bytes()[start + n] == b'`' {
        n += 1;
    }
    n.max(1)
}

fn has_closing_ticks(text: &str, mut start: usize, run: usize) -> bool {
    while start < text.len() {
        if text.as_bytes()[start] == b'`' {
            let got = tick_run(text, start);
            if got == run {
                return true;
            }
            start += got;
            continue;
        }
        let ch = text[start..].chars().next().unwrap_or('\0');
        start += ch.len_utf8();
    }
    false
}

#[derive(Copy, Clone)]
struct EscapeToken {
    end: usize,
    osc8_close: Option<bool>,
}

/// The escape sequence that starts at `start`. `None` when the byte there
/// is not ESC, or when a CSI, OSC or string sequence has no terminator
/// before the line ends: the ESC is then a control of no width and the
/// text after it is visible, so it is measured and wraps.
fn escape_token_at(text: &str, start: usize) -> Option<EscapeToken> {
    let bytes = text.as_bytes();
    if bytes.get(start) != Some(&0x1b) {
        return None;
    }
    match bytes.get(start + 1).copied() {
        Some(b'[') => Some(EscapeToken {
            end: csi_end(bytes, start + 2)?,
            osc8_close: None,
        }),
        Some(b']') => {
            let (end, payload_end) = osc_term_end(bytes, start + 2)?;
            let mut semi = start + 2;
            while semi < payload_end && bytes[semi] != b';' {
                semi += 1;
            }
            if semi >= payload_end {
                return Some(EscapeToken {
                    end,
                    osc8_close: None,
                });
            }
            if &text[start + 2..semi] != "8" {
                return Some(EscapeToken {
                    end,
                    osc8_close: None,
                });
            }
            let mut second = semi + 1;
            while second < payload_end && bytes[second] != b';' {
                second += 1;
            }
            if second >= payload_end {
                return Some(EscapeToken {
                    end,
                    osc8_close: None,
                });
            }
            let uri_start = second + 1;
            Some(EscapeToken {
                end,
                osc8_close: Some(uri_start == payload_end),
            })
        }
        Some(b'P' | b'X' | b'^' | b'_') => {
            let (end, _) = osc_term_end(bytes, start + 2)?;
            Some(EscapeToken {
                end,
                osc8_close: None,
            })
        }
        Some(_) => Some(EscapeToken {
            end: esc_end(bytes, start + 1),
            osc8_close: None,
        }),
        None => Some(EscapeToken {
            end: start + 1,
            osc8_close: None,
        }),
    }
}

fn csi_end(bytes: &[u8], mut i: usize) -> Option<usize> {
    while i < bytes.len() && bytes[i] != b'\n' {
        let b = bytes[i];
        i += 1;
        if (0x40..=0x7e).contains(&b) {
            return Some(i);
        }
    }
    None
}

fn esc_end(bytes: &[u8], mut i: usize) -> usize {
    if i >= bytes.len() {
        return i;
    }
    while i < bytes.len() && (0x20..=0x2f).contains(&bytes[i]) {
        i += 1;
    }
    if i < bytes.len() && (0x30..=0x7e).contains(&bytes[i]) {
        i + 1
    } else {
        i
    }
}

fn osc_term_end(bytes: &[u8], mut i: usize) -> Option<(usize, usize)> {
    while i < bytes.len() && bytes[i] != b'\n' {
        if bytes[i] == 0x07 {
            return Some((i + 1, i));
        }
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\') {
            return Some((i + 2, i));
        }
        i += 1;
    }
    None
}

fn split_indent(line: &str) -> (&str, &str) {
    let mut chars = 0usize;
    for ch in line.chars() {
        if ch == ' ' {
            chars += 1;
        } else {
            break;
        }
    }
    let at = byte_at_char(line, chars);
    line.split_at(at)
}

fn byte_at_char(text: &str, n: usize) -> usize {
    match text.char_indices().nth(n) {
        Some((i, _)) => i,
        None => text.len(),
    }
}

fn visible_width(text: &str) -> usize {
    visible_width_from(text, 0)
}

#[derive(Copy, Clone)]
enum Piece {
    Text,
    Escape(EscapeToken),
    Control(char),
}

/// Split `text` into printable runs, escape sequences and other control
/// characters. Escape sequences are atomic and have no width.
fn pieces(text: &str) -> Vec<(Range<usize>, Piece)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut run = 0usize;
    while i < text.len() {
        let ch = text[i..].chars().next().unwrap_or('\0');
        if !ch.is_control() {
            i += ch.len_utf8();
            continue;
        }
        if run < i {
            out.push((run..i, Piece::Text));
        }
        let escape = if ch == '\x1b' {
            escape_token_at(text, i)
        } else {
            None
        };
        i = match escape {
            Some(tok) => {
                out.push((i..tok.end, Piece::Escape(tok)));
                tok.end
            }
            None => {
                let end = i + ch.len_utf8();
                out.push((i..end, Piece::Control(ch)));
                end
            }
        };
        run = i;
    }
    if run < text.len() {
        out.push((run..text.len(), Piece::Text));
    }
    out
}

fn span_width(visible: &str) -> usize {
    visible.graphemes(true).map(UnicodeWidthStr::width).sum()
}

fn visible_width_from(text: &str, mut col: usize) -> usize {
    let start = col;
    let mut visible = String::new();
    for (range, piece) in pieces(text) {
        match piece {
            Piece::Text => visible.push_str(&text[range]),
            Piece::Escape(_) => {}
            Piece::Control(ch) => {
                col += span_width(&visible);
                visible.clear();
                if ch == '\t' {
                    col += 8 - (col % 8);
                }
            }
        }
    }
    col += span_width(&visible);
    col.saturating_sub(start)
}

/// A byte range of a word that a line break may not enter.
struct Atom {
    range: Range<usize>,
    width: usize,
}

/// One atom per visible grapheme, with any escape inside that grapheme.
/// Escapes and controls between graphemes are atoms of width 0.
fn word_atoms(text: &str) -> Vec<Atom> {
    let mut atoms = Vec::new();
    let mut visible = String::new();
    let mut raw_at = Vec::new();
    let mut run_start = 0usize;
    for (range, piece) in pieces(text) {
        match piece {
            Piece::Text => {
                raw_at.extend(range.clone());
                visible.push_str(&text[range]);
            }
            Piece::Escape(_) => {}
            Piece::Control(_) => {
                push_run_atoms(&mut atoms, run_start..range.start, &visible, &raw_at);
                visible.clear();
                raw_at.clear();
                run_start = range.end;
                atoms.push(Atom { range, width: 0 });
            }
        }
    }
    push_run_atoms(&mut atoms, run_start..text.len(), &visible, &raw_at);
    atoms
}

fn push_run_atoms(atoms: &mut Vec<Atom>, run: Range<usize>, visible: &str, raw_at: &[usize]) {
    let mut pos = run.start;
    for (at, grapheme) in visible.grapheme_indices(true) {
        let start = raw_at[at];
        let end = raw_at[at + grapheme.len() - 1] + 1;
        if pos < start {
            atoms.push(Atom {
                range: pos..start,
                width: 0,
            });
        }
        atoms.push(Atom {
            range: start..end,
            width: UnicodeWidthStr::width(grapheme),
        });
        pos = end;
    }
    if pos < run.end {
        atoms.push(Atom {
            range: pos..run.end,
            width: 0,
        });
    }
}

/// Words that may break between graphemes when no line can hold them.
/// URLs, paths, code spans and OSC-8 hyperlinks stay whole.
fn breakable_word(text: &str) -> bool {
    !has_scheme(&strip_ansi(text))
        && pieces(text).into_iter().all(|(range, piece)| match piece {
            Piece::Text => !text[range].contains(['`', '/', '\\']),
            Piece::Escape(tok) => tok.osc8_close.is_none(),
            Piece::Control(_) => true,
        })
}

/// Whether `word` holds a URI with a scheme, as in `mailto:a@b.org` or
/// `urn:isbn:0451450523`: a letter, then letters, digits, `+`, `-` or
/// `.`, then `:` and something more than closing punctuation.
fn has_scheme(word: &str) -> bool {
    let word = word.trim_start_matches(['(', '[', '<', '"', '\'']);
    let Some((scheme, rest)) = word.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
        && rest
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '>', '"', '\''])
            .chars()
            .next()
            .is_some()
}

fn keep_whole_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    command_line(trimmed)
}

fn command_line(line: &str) -> bool {
    let starts =
        line.starts_with("ljos ") || line.starts_with("vissue ") || line.starts_with("git ");
    if !starts {
        return false;
    }
    if line.contains(", ") || line.contains(": ") || line.trim_end().ends_with('.') {
        return false;
    }
    line.split_whitespace().all(|word| {
        word.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '-' | '_' | '/' | '.' | ':' | '=' | '"' | '\'' | '$' | '`'
                )
        })
    })
}

fn parse_columns(raw: &str) -> Option<usize> {
    let width: usize = raw.trim().parse().ok()?;
    sanitize_width(width)
}

fn sanitize_width(width: usize) -> Option<usize> {
    if width == 0 {
        None
    } else {
        Some(width.clamp(MIN_COLS, MAX_COLS))
    }
}

fn tty_columns_for_fd(fd: libc::c_int) -> Option<usize> {
    #[cfg(unix)]
    {
        use std::mem::MaybeUninit;
        let mut winsz = MaybeUninit::<libc::winsize>::zeroed();
        // SAFETY: ioctl with TIOCGWINSZ only reads window size into `winsz`.
        let rc = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, winsz.as_mut_ptr()) };
        if rc != 0 {
            return None;
        }
        // SAFETY: ioctl succeeded and initialized `winsz`.
        let winsz = unsafe { winsz.assume_init() };
        let cols = usize::from(winsz.ws_col);
        if cols == 0 {
            None
        } else {
            Some(cols)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_tty_without_columns_keeps_bytes() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(prose_width(false, None), None);
    }

    #[test]
    fn prose_width_parses_columns_on_a_tty() {
        let _env_read = crate::test_env_read_lock();
        let tty = Some(111);
        assert_eq!(
            prose_width_with(true, Some("0".to_string()), tty),
            Some(111)
        );
        assert_eq!(prose_width_with(true, Some("1".to_string()), tty), Some(20));
        assert_eq!(
            prose_width_with(true, Some("19".to_string()), tty),
            Some(20)
        );
        assert_eq!(
            prose_width_with(true, Some("20".to_string()), tty),
            Some(20)
        );
        assert_eq!(
            prose_width_with(true, Some("240".to_string()), tty),
            Some(240)
        );
        assert_eq!(
            prose_width_with(true, Some("241".to_string()), tty),
            Some(240)
        );
        assert_eq!(
            prose_width_with(true, Some("999999".to_string()), tty),
            Some(240)
        );
        assert_eq!(
            prose_width_with(true, Some("garbage".to_string()), tty),
            Some(111)
        );
        assert_eq!(
            prose_width_with(true, Some("-5".to_string()), tty),
            Some(111)
        );
        assert_eq!(prose_width_with(true, Some(String::new()), tty), Some(111));
    }

    #[test]
    fn a_terminal_width_is_clamped_like_columns() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(prose_width_with(true, None, Some(1)), Some(20));
        assert_eq!(prose_width_with(true, None, Some(10)), Some(20));
        assert_eq!(prose_width_with(true, None, Some(19)), Some(20));
        assert_eq!(prose_width_with(true, None, Some(20)), Some(20));
        assert_eq!(prose_width_with(true, None, Some(111)), Some(111));
        assert_eq!(prose_width_with(true, None, Some(241)), Some(240));
        assert_eq!(prose_width_with(true, None, Some(0)), None);
        assert_eq!(prose_width_with(true, None, None), None);
        assert_eq!(
            prose_width_with(true, Some("0".to_string()), Some(10)),
            Some(20)
        );
    }

    #[test]
    fn non_tty_stays_off_even_with_columns() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(prose_width(false, Some("60".to_string())), None);
    }

    #[test]
    fn unindented_prose_wraps_exactly() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("alpha beta gamma", Some(12));
        assert_eq!(wrapped, "alpha beta\ngamma");
    }

    #[test]
    fn code_span_in_parens_stays_whole() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("run (`ljos show ID`), now", Some(18));
        assert_eq!(wrapped, "run\n(`ljos show ID`),\nnow");
    }

    #[test]
    fn double_backtick_span_stays_whole() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("Use ``ljos show ID`` now", Some(12));
        assert_eq!(wrapped, "Use\n``ljos show ID``\nnow");
    }

    #[test]
    fn ansi_sequences_do_not_count_for_width() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("\u{1b}[31mred\u{1b}[0m blue", Some(7));
        assert_eq!(wrapped, "\u{1b}[31mred\u{1b}[0m\nblue");
    }

    #[test]
    fn cjk_uses_display_width() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("你好 世界 再见", Some(8));
        assert_eq!(wrapped, "你好\n世界\n再见");
    }

    #[test]
    fn urls_stay_whole_tokens() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("visit https://example.com/a/very/long/path now", Some(20));
        assert_eq!(wrapped, "visit\nhttps://example.com/a/very/long/path\nnow");
    }

    #[test]
    fn no_break_spaces_are_not_break_points() {
        let _env_read = crate::test_env_read_lock();
        for space in ['\u{a0}', '\u{202f}', '\u{2007}'] {
            let wrapped = wrap_prose(&format!("walk of 10{space}km"), Some(10));
            assert_eq!(wrapped, format!("walk of\n10{space}km"), "{space:?}");
        }
    }

    #[test]
    fn urls_without_slashes_stay_whole() {
        let _env_read = crate::test_env_read_lock();
        for url in [
            "mailto:someone.with.a.long.name@example.org",
            "urn:isbn:0451450523-0451450523",
            "tel:+1-555-0100-5550100-55501",
            "(mailto:someone.with.a.long.name@example.org)",
        ] {
            let wrapped = wrap_prose(&format!("write {url} now"), Some(20));
            assert!(wrapped.lines().any(|line| line == url), "{wrapped:?}");
        }
        let plain = wrap_prose("a-word-that-is-far-too-long-for-one-line", Some(20));
        assert!(
            plain.lines().all(|line| line.chars().count() <= 20),
            "{plain:?}"
        );
    }

    #[test]
    fn doctor_rows_use_spaces_consistently_and_wrap_with_zero_room() {
        let _env_read = crate::test_env_read_lock();
        let line =
            "ok\t12345678901234\tcursor-x3jhes1i90 named by CURSOR_CONVERSATION_ID in this process";
        let wrapped = wrap_prose(line, Some(20));
        let mut lines = wrapped.lines();
        assert_eq!(lines.next(), Some("ok  12345678901234"));
        assert!(lines.next().is_some(), "{wrapped}");
        let unwrapped = wrap_prose("ok\tpack\tstate", Some(80));
        assert_eq!(unwrapped, "ok  pack  state");
    }

    #[test]
    fn punctuation_before_code_span_does_not_split() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("open and run (`ljos show ID`), then continue", Some(30));
        assert!(!wrapped.contains("(`ljos show\n"), "{wrapped}");
        assert!(!wrapped.contains("show\n ID`"), "{wrapped}");
        assert!(wrapped.contains("(`ljos show ID`),"), "{wrapped}");
    }

    #[test]
    fn table_driven_review_repros() {
        let _env_read = crate::test_env_read_lock();
        let cases = [
            ("no_spurious_newline", "  hello              ", 20, "  hello"),
            (
                "osc8_unbreakable",
                "See \u{1b}]8;;https://example.com/long\u{7}click here now please\u{1b}]8;;\u{7} end",
                20,
                "See\n\u{1b}]8;;https://example.com/long\u{7}click here now please\u{1b}]8;;\u{7}\nend",
            ),
            (
                "esc_zero_width",
                "\u{1b}(Bhello world foo bar",
                12,
                "\u{1b}(Bhello world\nfoo bar",
            ),
            (
                "unclosed_backtick_does_not_freeze",
                "oops `unclosed backtick followed by many separate words here",
                24,
                "oops `unclosed backtick\nfollowed by many\nseparate words here",
            ),
        ];
        for (name, input, cols, want) in cases {
            let got = wrap_prose(input, Some(cols));
            assert_eq!(got, want, "{name}");
        }
    }

    #[test]
    fn doctor_prefix_overflow_uses_small_indent_and_no_trailing_spaces() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose(
            "info\t12345678901234567890\talpha beta gamma delta",
            Some(20),
        );
        let lines: Vec<&str> = wrapped.lines().collect();
        assert_eq!(lines[0], "info");
        assert_eq!(lines[1], "12345678901234567890");
        assert!(!lines.iter().any(|line| line.ends_with(' ')), "{wrapped}");
        assert_eq!(lines[2], "alpha beta gamma");
        assert!(
            lines.iter().all(|line| visible_width(line) <= 20),
            "{wrapped}"
        );
    }

    #[test]
    fn grapheme_sequences_wrap_at_display_width() {
        let _env_read = crate::test_env_read_lock();
        let cases = [
            (
                "vs16",
                "aaaaaaaaaaaaaaaaaa ⚠\u{fe0f} next",
                "aaaaaaaaaaaaaaaaaa\n⚠\u{fe0f} next",
            ),
            (
                "zwj_family",
                "aaaaaaaaaaaaaaa 👨\u{200d}👩\u{200d}👧 next",
                "aaaaaaaaaaaaaaa 👨\u{200d}👩\u{200d}👧\nnext",
            ),
            (
                "skin_tone",
                "aaaaaaaaaaaaaaaa 👍🏽 next",
                "aaaaaaaaaaaaaaaa 👍🏽\nnext",
            ),
            (
                "keycap",
                "aaaaaaaaaaaaaaaaaa 1\u{fe0f}\u{20e3} next",
                "aaaaaaaaaaaaaaaaaa\n1\u{fe0f}\u{20e3} next",
            ),
        ];
        for (name, input, want) in cases {
            let got = wrap_prose(input, Some(20));
            assert_eq!(got, want, "{name}");
        }
    }

    #[test]
    fn grapheme_display_widths() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(visible_width("👨\u{200d}👩\u{200d}👧"), 2);
        assert_eq!(visible_width("1\u{fe0f}\u{20e3}"), 2);
        assert_eq!(visible_width("👍🏽"), 2);
        assert_eq!(visible_width("你好"), 4);
        assert_eq!(
            visible_width("\u{1b}[31m👨\u{200d}👩\u{200d}👧\u{1b}[0m"),
            2
        );
        assert_eq!(visible_width("a\rb\u{7}c"), 3);
        assert_eq!(visible_width("你\t好"), 10);
        assert_eq!(visible_width_from("\tx", 3), 6);
    }

    #[test]
    fn an_escape_inside_a_grapheme_keeps_its_width() {
        let _env_read = crate::test_env_read_lock();
        assert_eq!(visible_width("1\u{1b}[31m\u{fe0f}\u{20e3}"), 2);
        assert_eq!(visible_width("1\u{1b}[31m\u{fe0f}\u{20e3}\u{1b}[0m"), 2);
        assert_eq!(
            visible_width("👨\u{1b}[32m\u{200d}👩\u{200d}\u{1b}[0m👧"),
            2
        );
        assert_eq!(visible_width("你\u{1b}[1m好"), 4);

        let keycap = "aaaaaaaaaaaaaaaaaa 1\u{1b}[31m\u{fe0f}\u{20e3}\u{1b}[0m next";
        assert_eq!(
            wrap_prose(keycap, Some(20)),
            "aaaaaaaaaaaaaaaaaa\n1\u{1b}[31m\u{fe0f}\u{20e3}\u{1b}[0m next"
        );
    }

    #[test]
    fn a_coloured_overwide_word_splits_at_width() {
        let _env_read = crate::test_env_read_lock();
        let word = "\u{1b}[31mabcdefghijklmnop\u{1b}[0m";
        assert_eq!(
            wrap_prose(word, Some(8)),
            "\u{1b}[31mabcdefgh\nijklmnop\u{1b}[0m"
        );
        assert_eq!(
            wrap_prose(&format!("see {word} now"), Some(8)),
            "see \u{1b}[31mabcd\nefghijkl\nmnop\u{1b}[0m now"
        );
        assert_eq!(
            wrap_prose("abcd\u{1b}[1mefghijkl\u{1b}[0mmnop", Some(8)),
            "abcd\u{1b}[1mefgh\nijkl\u{1b}[0mmnop"
        );
        let keycap = "1\u{1b}[31m\u{fe0f}\u{20e3}";
        let keycaps = keycap.repeat(5);
        assert_eq!(
            wrap_prose(&keycaps, Some(4)),
            format!("{keycap}{keycap}\n{keycap}{keycap}\n{keycap}")
        );
        let grapheme = "👨\u{1b}[32m\u{200d}👩\u{200d}👧";
        let family = format!("{grapheme}\u{1b}[0m");
        assert_eq!(
            wrap_prose(&family.repeat(3), Some(4)),
            format!("{family}{grapheme}\n\u{1b}[0m{family}")
        );
    }

    #[test]
    fn overwide_cjk_splits_at_grapheme_boundaries() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("这是一个没有空格的很长的中文句子用来测试", Some(10));
        assert_eq!(wrapped, "这是一个没\n有空格的很\n长的中文句\n子用来测试");

        let odd = wrap_prose("注意 这是一个没有空格的很长的中文句子", Some(9));
        for line in odd.lines() {
            assert!(visible_width(line) <= 9, "{odd}");
        }
        assert_eq!(
            odd.replace('\n', ""),
            "注意 这是一个没有空格的很长的中文句子"
        );

        let emoji = wrap_prose(
            "👨\u{200d}👩\u{200d}👧👨\u{200d}👩\u{200d}👧👨\u{200d}👩\u{200d}👧",
            Some(4),
        );
        assert_eq!(
            emoji,
            "👨\u{200d}👩\u{200d}👧👨\u{200d}👩\u{200d}👧\n👨\u{200d}👩\u{200d}👧"
        );

        let doctor = wrap_prose("warn\tpack\t这是一个没有空格的很长的中文句子", Some(20));
        let lines: Vec<&str> = doctor.lines().collect();
        assert_eq!(lines[0], "warn  pack  这是一个");
        assert!(
            lines.iter().all(|line| visible_width(line) <= 20),
            "{doctor}"
        );
    }

    #[test]
    fn overwide_urls_paths_spans_and_hyperlinks_stay_whole() {
        let _env_read = crate::test_env_read_lock();
        let cases = [
            "https://example.com/a/very/long/path",
            "/usr/local/share/ljos/very/long/path",
            "C:\\Users\\someone\\AppData\\Local\\ljos",
            "`ljos show a-very-long-ticket-id`",
            "\u{1b}]8;;https://example.com\u{7}linklabelthatislong\u{1b}]8;;\u{7}",
            "\u{1b}[31mhttps://example.com/a/very/long/path\u{1b}[0m",
        ];
        for token in cases {
            let wrapped = wrap_prose(&format!("see {token} now"), Some(12));
            assert!(wrapped.contains(token), "{wrapped:?}");
        }
    }

    #[test]
    fn doctor_prefix_at_edge_does_not_repeat_or_trail() {
        let _env_read = crate::test_env_read_lock();
        let exact = wrap_prose("ok\t12345678901234\talpha beta gamma delta", Some(20));
        assert_eq!(exact, "ok  12345678901234\nalpha beta gamma\ndelta");
        assert!(
            exact.lines().all(|line| visible_width(line) <= 20),
            "{exact}"
        );
        assert!(!exact.lines().any(|line| line.ends_with(' ')), "{exact}");

        let over = wrap_prose(
            "warn\tgit identity\tgit has no author for a commit",
            Some(20),
        );
        assert!(over.starts_with("warn  git identity\n"), "{over}");
        assert!(!over.contains("warn  git identity  git"), "{over}");
        assert!(over.lines().all(|line| visible_width(line) <= 20), "{over}");

        let empty = wrap_prose("info\tpackset-embed\t", Some(20));
        assert_eq!(empty, "info  packset-embed");
        let spaces = wrap_prose("info\tpackset-embed\t   ", Some(20));
        assert_eq!(spaces, "info  packset-embed");
    }

    #[test]
    fn backtick_run_matching_uses_same_tick_count() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("Use `alpha `` beta` now", Some(12));
        assert_eq!(wrapped, "Use\n`alpha `` beta`\nnow");

        let unclosed = wrap_prose("Use `unclosed_word `` still moving", Some(15));
        assert_eq!(unclosed, "Use\n`unclosed_word\n`` still moving");
    }

    #[test]
    fn warming_rows_are_treated_as_doctor_rows() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose(
            "warming\tpack\twarming, about 4.5 s; http://127.0.0.1:9 workspace seat",
            Some(25),
        );
        assert!(!wrapped.contains('\t'), "{wrapped}");
        assert!(wrapped.starts_with("warming  pack  "), "{wrapped}");
    }

    #[test]
    fn osc8_with_params_stays_atomic() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose(
            "See \u{1b}]8;id=1;https://example.com/long\u{7}click here now please\u{1b}]8;;\u{7} end",
            Some(20),
        );
        assert_eq!(
            wrapped,
            "See\n\u{1b}]8;id=1;https://example.com/long\u{7}click here now please\u{1b}]8;;\u{7}\nend"
        );
    }

    #[test]
    fn osc8_single_char_uri_is_not_treated_as_close() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose(
            "See \u{1b}]8;;/\u{7}slash link text\u{1b}]8;;\u{7} now",
            Some(18),
        );
        assert!(wrapped.contains("\u{1b}]8;;/\u{7}slash link text\u{1b}]8;;\u{7}"));
        assert!(!wrapped.contains("\u{1b}]8;;/\u{7}\n"), "{wrapped}");
    }

    #[test]
    fn osc_with_spaces_stays_atomic() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("pre \u{1b}]0;my title\u{7} post words", Some(10));
        assert!(wrapped.contains("\u{1b}]0;my title\u{7}"), "{wrapped}");
        assert!(!wrapped.contains("\u{1b}]0;\n"), "{wrapped}");
    }

    #[test]
    fn multiline_hanging_keeps_prefix_blank_lines_and_inner_indent() {
        let _env_read = crate::test_env_read_lock();
        let body =
            "first line\n  Þetta er íslenskt: ofninn í Reykjavík þarf nýja þéttingu.\n\nsecond";
        let wrapped = wrap_hanging_prose(body, 28, "| ", "| ");
        let lines: Vec<&str> = wrapped.lines().collect();
        assert_eq!(lines.first().copied(), Some("| first line"));
        assert!(lines.contains(&"|"), "{wrapped}");
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("|   Þetta er íslenskt")),
            "{wrapped}"
        );
        assert!(lines.iter().all(|line| !line.ends_with(' ')), "{wrapped}");
    }

    #[test]
    fn tabs_measure_to_the_next_eight_column_stop() {
        let _env_read = crate::test_env_read_lock();
        let wrapped = wrap_prose("alpha\tbeta\tgamma\tdelta\tepsilon", Some(20));
        for line in wrapped.lines() {
            assert!(visible_width(line) <= 20, "{wrapped}");
        }
    }

    #[test]
    fn an_unterminated_escape_leaves_the_rest_of_the_line_visible() {
        let _env_read = crate::test_env_read_lock();
        for text in [
            "pre \u{1b}]0;my title with spaces that still has to wrap",
            "pre \u{1b}[1 22 333 4444 55555 666666 7777777 88888888 999",
            "pre \u{1b}Pmy title with spaces that still has to wrap",
        ] {
            assert_eq!(
                visible_width(text),
                visible_width(&text.replace('\u{1b}', "")),
                "{text:?}"
            );
            let wrapped = wrap_prose(text, Some(12));
            assert!(wrapped.lines().count() > 1, "{wrapped:?}");
            for line in wrapped.lines() {
                assert!(visible_width(line) <= 12, "{line:?} in {wrapped:?}");
            }
        }
        let next = wrap_prose("a \u{1b}[31\nthe next line stays its own", Some(80));
        assert_eq!(next, "a \u{1b}[31\nthe next line stays its own");
    }

    #[test]
    fn a_terminated_escape_still_has_no_width() {
        let _env_read = crate::test_env_read_lock();
        for text in [
            "pre \u{1b}]0;a title\u{7}word",
            "pre \u{1b}]0;a title\u{1b}\\word",
            "pre \u{1b}[1;31mword\u{1b}[0m",
        ] {
            assert_eq!(visible_width(text), 8, "{text:?}");
        }
    }

    #[test]
    fn esc_before_multibyte_never_splits_utf8_or_panics() {
        let _env_read = crate::test_env_read_lock();
        let text = "pre \u{1b}你好 world more words";
        let stripped = strip_ansi(text);
        assert_eq!(stripped, "pre 你好 world more words");
        let solid = |s: &str| {
            s.chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>()
        };
        for cols in [1usize, 10, 20] {
            let wrapped = wrap_prose(text, Some(cols));
            assert_eq!(solid(&wrapped), solid(text), "cols={cols}");
            if cols >= 4 {
                assert!(wrapped.contains("你好"), "cols={cols} wrapped={wrapped:?}");
            }
        }
    }

    #[test]
    fn esc_intermediate_sequence_keeps_trailing_text() {
        let _env_read = crate::test_env_read_lock();
        let text = "pre \u{1b}(Bhello world";
        let stripped = strip_ansi(text);
        assert_eq!(stripped, "pre (Bhello world");
        let wrapped = wrap_prose(text, Some(10));
        assert!(wrapped.contains("\u{1b}(Bhello"), "{wrapped}");
    }

    #[test]
    fn strip_ansi_matches_base_non_tty_algorithm_for_escape_corpus() {
        let _env_read = crate::test_env_read_lock();
        fn base_strip_ansi(text: &str) -> String {
            let mut out = String::new();
            let mut chars = text.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch != '\u{1b}' {
                    out.push(ch);
                    continue;
                }
                match chars.peek().copied() {
                    Some('[') => {
                        chars.next();
                        for ch in chars.by_ref() {
                            if ('@'..='~').contains(&ch) {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        chars.next();
                        while let Some(ch) = chars.next() {
                            if ch == '\u{7}' {
                                break;
                            }
                            if ch == '\u{1b}' && chars.peek() == Some(&'\\') {
                                chars.next();
                                break;
                            }
                        }
                    }
                    Some(_) => {}
                    None => {}
                }
            }
            out
        }

        let mut corpus = vec![
            "plain ascii".to_string(),
            "pre \u{1b}[31mred\u{1b}[0m post".to_string(),
            "pre \u{1b}]0;my title\u{7} post".to_string(),
            "pre \u{1b}]8;;https://example.com\u{7}label\u{1b}]8;;\u{7} post".to_string(),
            "pre \u{1b}]8;;https://example.com\u{1b}\\label\u{1b}]8;;\u{1b}\\ post".to_string(),
            "pre \u{1b}P+qpayload data\u{1b}\\ post".to_string(),
            "pre \u{1b}Xsos payload\u{7} post".to_string(),
            "pre \u{1b}^pm payload 你好".to_string(),
            "pre \u{1b}_apc payload 👨‍👩‍👧".to_string(),
            "pre \u{1b}(Bhello 你好 wrap me".to_string(),
            "pre \u{1b}7 hello".to_string(),
            "pre \u{1b}8 hello".to_string(),
            "pre \u{1b}c hello".to_string(),
            "pre \u{1b}  hello".to_string(),
            "pre \u{1b}你好 hello".to_string(),
        ];
        for final_byte in 0x30u8..=0x7e {
            let ch = final_byte as char;
            corpus.push(format!("pre \u{1b}{ch} hello"));
        }
        for text in corpus {
            assert_eq!(strip_ansi(&text), base_strip_ansi(&text), "{text:?}");
        }
    }

    #[test]
    fn seeded_prose_property_roundtrip_and_width() {
        let _env_read = crate::test_env_read_lock();
        fn strip_ws(s: &str) -> String {
            s.chars().filter(|ch| !ch.is_whitespace()).collect()
        }
        fn xorshift64(mut x: u64) -> u64 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        }
        fn is_lone_overwide(line: &str, cols: usize) -> bool {
            let segs = prose_segments(line);
            let mut parts = segs.into_iter().filter(|seg| !seg.space);
            let Some(one) = parts.next() else {
                return false;
            };
            let visible = word_atoms(&one.text)
                .into_iter()
                .filter(|atom| atom.width > 0)
                .count();
            parts.next().is_none()
                && visible_width(&one.text) > cols
                && (!breakable_word(&one.text) || visible == 1)
        }
        fn no_split_escapes_or_ticks(text: &str) -> bool {
            let mut i = 0usize;
            let mut open_ticks: Option<usize> = None;
            while i < text.len() {
                let ch = text[i..].chars().next().unwrap_or('\0');
                if ch == '\n' && open_ticks.is_some() {
                    return false;
                }
                if ch == '\x1b' {
                    let Some(tok) = escape_token_at(text, i) else {
                        return false;
                    };
                    if text[i..tok.end].contains('\n') {
                        return false;
                    }
                    i = tok.end;
                    continue;
                }
                if ch == '`' {
                    let run = tick_run(text, i);
                    if let Some(open) = open_ticks {
                        if run == open {
                            open_ticks = None;
                        }
                    } else if has_closing_ticks(text, i + run, run) {
                        open_ticks = Some(run);
                    }
                    i += run;
                    continue;
                }
                i += ch.len_utf8();
            }
            open_ticks.is_none()
        }

        let banks = [
            "alpha",
            "beta",
            "你好",
            "👨‍👩‍👧",
            "👍🏽",
            "1️⃣",
            "`code span`",
            "``double ticks``",
            "https://example.com/a/very/long/path",
            "\u{1b}[31mred\u{1b}[0m",
            "\u{1b}[31mabcdefghijklmnop\u{1b}[0m",
            "ab\u{1b}[1mcdefgh\u{1b}[0mijkl",
            "1\u{1b}[31m\u{fe0f}\u{20e3}\u{1b}[0m",
            "👨\u{1b}[32m\u{200d}👩\u{200d}👧\u{1b}[0m",
            "\u{1b}]0;my title\u{7}",
            "\u{1b}]8;;https://example.com/long\u{7}click here\u{1b}]8;;\u{7}",
            "\u{1b}]8;id=7;/\u{7}slash\u{1b}]8;;\u{7}",
        ];
        let mut seed = 0x5eed_1234_5678_9abcu64;
        for cols in 1..=120usize {
            for round in 0..24u64 {
                seed = xorshift64(seed ^ round ^ cols as u64);
                let mut text = String::new();
                let prefix = if round % 3 == 0 {
                    "warn  git identity  "
                } else {
                    ""
                };
                text.push_str(prefix);
                for n in 0..18usize {
                    seed = xorshift64(seed ^ n as u64);
                    if n > 0 {
                        text.push(if seed & 1 == 0 { ' ' } else { '\t' });
                    }
                    let pick = ((seed >> 3) as usize) % banks.len();
                    text.push_str(banks[pick]);
                }
                let wrapped = wrap_prose(&text, Some(cols));
                for line in wrapped.lines() {
                    assert!(!line.ends_with(' '), "cols={cols} line={line:?}");
                    if visible_width(line) > cols {
                        assert!(
                            is_lone_overwide(line, cols),
                            "cols={cols} line too wide: {line:?}"
                        );
                    }
                }
                assert_eq!(strip_ws(&wrapped), strip_ws(&text), "cols={cols}");
                assert!(
                    no_split_escapes_or_ticks(&wrapped),
                    "cols={cols} text={wrapped:?}"
                );
            }
        }
    }
}
