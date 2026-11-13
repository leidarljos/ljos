//! Human pages for search, recall, ready, and the hook.
//!
//! Each line fits [`COLS`]. The pages name the score, say shown N of M,
//! and name the command that lists the rest. `--json` and the MCP rows
//! do not use this module. A pipe, and `NO_COLOR`, carry no ANSI.

/// Columns a human page fits, including a pipe read in an 80-column terminal.
pub const COLS: usize = 80;

/// Whether a human page may keep ANSI. `NO_COLOR` set to a non-empty
/// value turns color off, and so does a stdout that is not a terminal.
#[must_use]
pub fn color_allowed(stdout_is_tty: bool) -> bool {
    if std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
        return false;
    }
    stdout_is_tty
}

/// `text` as a reader of a pipe or a `NO_COLOR` terminal should see it.
#[must_use]
pub fn for_reader(text: &str, allow_color: bool) -> String {
    if allow_color {
        text.to_string()
    } else {
        strip_ansi(text)
    }
}

/// Drop CSI and OSC sequences. Other characters stay.
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

/// Wrap `text` so each line is at most `cols` characters. A tab is a
/// space. A word longer than the line breaks. Continuation keeps a
/// leading run of spaces.
#[must_use]
pub fn wrap(text: &str, cols: usize) -> String {
    let cols = cols.max(1);
    let mut out = String::new();
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let flat = line.replace('\t', " ");
        push_wrapped(&mut out, &flat, cols);
    }
    out
}

/// Wrap `line` under a hanging prefix. The prefix counts toward `cols`.
#[must_use]
pub fn wrap_hang(line: &str, cols: usize, first: &str, cont: &str) -> String {
    let cols = cols.max(1);
    let room = cols.saturating_sub(first.chars().count()).max(1);
    let body = wrap(&line.replace('\t', " "), room);
    let mut out = String::new();
    for (index, part) in body.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
            out.push_str(cont);
        } else {
            out.push_str(first);
        }
        out.push_str(part);
    }
    out
}

fn push_wrapped(out: &mut String, line: &str, cols: usize) {
    if line.chars().count() <= cols {
        out.push_str(line);
        return;
    }
    let indent_len = line.chars().take_while(|ch| *ch == ' ').count();
    let indent_len = if indent_len + 8 > cols { 0 } else { indent_len };
    let indent = " ".repeat(indent_len);
    let words: Vec<&str> = line.split_whitespace().collect();
    if words.is_empty() {
        out.push_str(&break_word(line, cols));
        return;
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in words {
        let piece = if current.is_empty() {
            format!("{indent}{word}")
        } else {
            format!("{current} {word}")
        };
        if piece.chars().count() <= cols {
            current = piece;
        } else if current.is_empty() {
            lines.push(break_word(&piece, cols));
        } else {
            lines.push(std::mem::take(&mut current));
            let next = format!("{indent}{word}");
            if next.chars().count() <= cols {
                current = next;
            } else {
                lines.push(break_word(&next, cols));
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    out.push_str(&lines.join("\n"));
}

fn break_word(text: &str, cols: usize) -> String {
    if text.chars().count() <= cols {
        return text.to_string();
    }
    let mut out = String::new();
    let mut line = String::new();
    for ch in text.chars() {
        if line.chars().count() == cols {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&line);
            line.clear();
        }
        line.push(ch);
    }
    if !line.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&line);
    }
    out
}

/// The first eight characters, which `ljos forget` accepts as an id.
#[must_use]
pub fn short_id(id: &str) -> &str {
    match id.trim().char_indices().nth(8) {
        Some((index, _)) => &id[..index],
        None => id.trim(),
    }
}

/// One claim: a short head, the text under it, then an optional foot.
/// `indent` is the head's left margin. The text and the foot sit two
/// columns further in.
#[must_use]
pub fn claim_block(head: &str, text: &str, foot: &str, indent: usize) -> String {
    let cols = COLS;
    let pad = " ".repeat(indent.min(cols.saturating_sub(8)));
    let mut out = wrap(&format!("{pad}{head}"), cols);
    let text = text.trim();
    if !text.is_empty() {
        let body = wrap_hang(text, cols, &format!("{pad}  "), &format!("{pad}  "));
        out.push('\n');
        out.push_str(&body);
    }
    let foot = foot.trim();
    if !foot.is_empty() {
        let meta = wrap_hang(foot, cols, &format!("{pad}  "), &format!("{pad}  "));
        out.push('\n');
        out.push_str(&meta);
    }
    out
}

/// `Title  shown N of M` and an optional note, wrapped to [`COLS`].
#[must_use]
pub fn count_line(title: &str, shown: usize, total: usize, note: &str) -> String {
    let note = note.trim();
    let line = if note.is_empty() {
        format!("{title}  shown {shown} of {total}")
    } else {
        format!("{title}  shown {shown} of {total}, {note}")
    };
    wrap(&line, COLS)
}

/// The hook's pack header. One line at [`COLS`] for the counts a prompt shows.
#[must_use]
pub fn remembered_head(shown: usize, matched: usize) -> String {
    head(
        "Remembered",
        shown,
        matched,
        "oldest first",
        shown < matched,
    )
}

/// The hook's header for memories about a failed command.
#[must_use]
pub fn failure_head(shown: usize, matched: usize) -> String {
    head(
        "Remembered about this failure",
        shown,
        matched,
        "",
        shown < matched,
    )
}

fn head(title: &str, shown: usize, matched: usize, tail: &str, more: bool) -> String {
    let mut line = format!("{title}, shown {shown} of {matched}");
    if !tail.is_empty() {
        line.push_str(", ");
        line.push_str(tail);
    }
    if more {
        line.push_str(", and more remain, so run `ljos search`.");
    } else {
        line.push_str(", so run `ljos search`.");
    }
    wrap(&line, COLS)
}

/// One search hit a person reads. The id is the short prefix.
pub struct SearchRow<'a> {
    /// Kind and origin, as recall names them.
    pub label: &'a str,
    /// `today`, `5 months ago`, or empty.
    pub age: &'a str,
    /// Another seat, when the hit is not this seat's.
    pub from: Option<&'a str>,
    /// The stored sentence.
    pub text: &'a str,
    /// Scorer rank. Not whether the claim is true.
    pub score: f64,
    /// Scorers that named the hit, and how many ran.
    pub ballots: Option<(u32, u32)>,
    /// Full id. The page prints eight characters.
    pub id: Option<&'a str>,
    /// Kept on this machine, not in the pack.
    pub local: bool,
}

/// A search page: the rows after the limit cut, and how many matched.
pub struct SearchPage<'a> {
    pub rows: &'a [SearchRow<'a>],
    /// Rows printed.
    pub shown: usize,
    /// Rows that matched before the limit cut.
    pub matched: usize,
    /// The pack returned a full page, so more may exist past `matched`.
    pub capped: bool,
    /// `--explain`: the score paragraph, instead of the two legend lines.
    pub explain: bool,
    /// The query, for the command that lists the rest.
    pub query: &'a str,
    /// The paragraph `--explain` prints. Ignored when `explain` is false.
    pub reading: &'a str,
}

/// The human search page.
#[must_use]
pub fn search_page(page: &SearchPage<'_>) -> String {
    if page.rows.is_empty() {
        let mut out = wrap(&no_hits_line(page.query, page.explain), COLS);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        return out;
    }
    let mut out = shown_line(page);
    out.push('\n');
    if page.explain {
        let reading = page.reading.trim();
        if !reading.is_empty() {
            out.push_str(&wrap(reading, COLS));
            out.push('\n');
        }
    } else {
        out.push_str(
            "The number is the order in the list. It is not whether the sentence is true.\n",
        );
        out.push_str("Checks: how many of them named this row.\n");
    }
    out.push('\n');
    for row in page.rows {
        out.push_str(&search_row(row));
        out.push_str("\n\n");
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

fn shown_line(page: &SearchPage<'_>) -> String {
    let shown = page.shown;
    let matched = page.matched.max(shown);
    if shown < matched {
        let mut line = format!("shown {shown} of {matched}");
        if page.capped {
            line.push_str(", or more");
        }
        line.push_str(", ");
        line.push_str(&search_hint(
            page.query,
            next_limit(matched, page.capped),
            page.capped,
        ));
        return wrap(&line, COLS);
    }
    if page.capped && shown > 0 {
        let line = format!(
            "shown {shown} of {shown}, {}",
            search_hint(page.query, shown.saturating_mul(2).max(shown + 1), true)
        );
        return wrap(&line, COLS);
    }
    format!("shown {shown} of {matched}.")
}

fn next_limit(matched: usize, capped: bool) -> usize {
    if capped {
        matched.saturating_mul(2).max(matched + 1)
    } else {
        matched
    }
}

/// One sentence when a search matched nothing, and the command to run next.
#[must_use]
pub fn no_hits_line(query: &str, explain: bool) -> String {
    let query = query.trim();
    let next = next_search(query);
    let head = if query.is_empty()
        || query
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '"' | '`' | '\\'))
    {
        "No hits".to_string()
    } else if query.contains(char::is_whitespace) {
        format!("No hits for \"{query}\"")
    } else {
        format!("No hits for {query}")
    };
    if explain {
        format!("{head}, so the pack holds nothing on this query and the next command is `{next}`.")
    } else {
        format!("{head}, so run `{next}`.")
    }
}

fn next_search(query: &str) -> String {
    let words: Vec<&str> = query
        .split_whitespace()
        .filter(|word| word.len() >= 3 && word.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .collect();
    if words.len() > 1 {
        let mut best = words[0];
        for word in &words[1..] {
            if word.len() > best.len() {
                best = word;
            }
        }
        return format!("ljos search {best}");
    }
    "ljos remember \"TEXT\"".to_string()
}

fn search_hint(query: &str, limit: usize, asks: bool) -> String {
    let verb = if asks {
        "and the next page asks for more"
    } else {
        "and the next page lists the rest"
    };
    let query = query.trim();
    let usable = !query.is_empty()
        && !query
            .chars()
            .any(|ch| ch.is_control() || ch == '`' || ch == '"');
    let cmd = if usable && query.contains(char::is_whitespace) {
        format!("ljos search \"{query}\" -n {limit}")
    } else if usable {
        format!("ljos search {query} -n {limit}")
    } else {
        format!("ljos search -n {limit}")
    };
    format!("{verb}, so run `{cmd}`.")
}

fn search_row(row: &SearchRow<'_>) -> String {
    let mut head = row.label.to_string();
    if !row.age.is_empty() {
        head.push_str(", ");
        head.push_str(row.age);
    }
    if let Some(seat) = row.from.filter(|seat| !seat.is_empty()) {
        head.push_str(", from ");
        head.push_str(seat);
    }
    let mut foot = format!("order {:.2}", row.score);
    if row.local {
        foot.push_str(", local, not in the pack");
    } else if let Some((hit, of)) = row.ballots {
        foot.push_str(&format!(", checks {hit}/{of}"));
    }
    if let Some(id) = row.id.map(str::trim).filter(|id| !id.is_empty()) {
        foot.push_str(", id ");
        foot.push_str(short_id(id));
    }
    claim_block(&head, row.text, &foot, 0)
}

/// `vissue ready` rows, relaid so the title is not pushed past column 80
/// by a padded id. JSON stays with the caller.
#[must_use]
pub fn ready_text(raw: &str) -> String {
    let raw = strip_ansi(raw);
    let mut rows = Vec::new();
    let mut loose = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(row) = parse_ready(line) {
            rows.push(row);
        } else {
            loose.push(line.to_string());
        }
    }
    let total = rows.len() + loose.len();
    let mut out = count_line("ready", total, total, "");
    out.push('\n');
    if total == 0 {
        out.push_str("no open ticket is free of an open blocker, so run `ljos file TITLE`.\n");
        return out;
    }
    out.push_str("open and not waiting on an open ticket, so run `ljos show ID`.\n");
    for row in &rows {
        out.push('\n');
        out.push_str(&ready_row(row));
        out.push('\n');
    }
    for line in &loose {
        out.push('\n');
        out.push_str(&wrap(line, COLS));
        out.push('\n');
    }
    out
}

struct ReadyRow {
    id: String,
    state: String,
    priority: String,
    title: String,
    claim: String,
}

fn parse_ready(line: &str) -> Option<ReadyRow> {
    let line = line.trim_end();
    if line.len() < 39 || !line.is_ascii() {
        return None;
    }
    let id = line.get(..22)?.trim();
    if id.is_empty() || line.as_bytes().get(22) != Some(&b' ') {
        return None;
    }
    let state = line.get(23..32)?.trim();
    if state.is_empty() || line.as_bytes().get(32) != Some(&b' ') {
        return None;
    }
    let mark = line.get(33..37)?;
    if !mark.starts_with("[#") || !mark.ends_with(']') {
        return None;
    }
    let priority = mark[2..3].to_string();
    let rest = line.get(37..)?.strip_prefix("  ")?;
    let (title, claim) = split_claim(rest);
    Some(ReadyRow {
        id: id.to_string(),
        state: state.to_string(),
        priority,
        title: title.to_string(),
        claim: claim.to_string(),
    })
}

fn split_claim(rest: &str) -> (&str, &str) {
    if let Some(index) = rest.rfind("  (claimed ") {
        let (title, claim) = rest.split_at(index);
        let claim = claim.trim().trim_start_matches('(').trim_end_matches(')');
        return (title.trim(), claim);
    }
    (rest.trim(), "")
}

fn ready_row(row: &ReadyRow) -> String {
    let head = format!("{}  {}  [#{}]", row.id, row.state, row.priority);
    let with_title = if row.title.is_empty() {
        head.clone()
    } else {
        format!("{head}  {}", row.title)
    };
    let mut out = if with_title.chars().count() <= COLS {
        with_title
    } else {
        let mut out = head;
        out.push('\n');
        out.push_str(&wrap_hang(&row.title, COLS, "  ", "  "));
        out
    };
    if !row.claim.is_empty() {
        out.push('\n');
        out.push_str(&wrap_hang(&row.claim, COLS, "  ", "  "));
    }
    out
}

#[cfg(test)]
static COLOR_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_at_eighty_columns_names_the_title_and_the_count() {
        let kiln = format!(
            "{:<22} {:<9} [#{}]  {}",
            "ufuv-kiln", "TODO", "B", "Kiln damper schedule"
        );
        let attn = format!(
            "{:<22} {:<9} [#{}]  {}  (claimed 3d by ada)",
            "ufuv-attn", "STARTED", "A", "MultiHeadAttention and LayerNorm"
        );
        let raw = format!("{kiln}\n{attn}\n");
        let text = ready_text(&raw);
        assert!(text.contains("ready  shown 2 of 2"), "{text}");
        assert!(text.contains("so run `ljos show ID`."), "{text}");
        assert!(
            text.contains("ufuv-kiln  TODO  [#B]  Kiln damper schedule"),
            "{text}"
        );
        assert!(text.contains("claimed 3d by ada"), "{text}");
        assert!(
            text.lines().all(|line| line.chars().count() <= COLS),
            "{text}"
        );
        assert!(!text.contains('\u{1b}'));
        let colored =
            "\u{1b}[31mufuv-kiln              TODO      [#B]  Kiln damper schedule\u{1b}[0m\n";
        let plain = ready_text(colored);
        assert!(!plain.contains('\u{1b}'), "{plain}");
        assert!(plain.contains("Kiln damper schedule"), "{plain}");
    }

    #[test]
    fn wrap_keeps_every_line_inside_eighty_columns() {
        let long = "Score is how the scorers ranked this query. The fraction is how many of them named the hit. Neither is whether the claim is true.";
        let wrapped = wrap(long, COLS);
        assert!(wrapped.lines().all(|line| line.chars().count() <= COLS));
        assert!(wrapped.lines().count() > 1);
    }

    #[test]
    fn no_color_strips_ansi_and_a_tty_without_it_keeps_color() {
        let _guard = COLOR_ENV.lock().unwrap_or_else(|err| err.into_inner());
        std::env::set_var("NO_COLOR", "1");
        assert!(!color_allowed(true));
        std::env::remove_var("NO_COLOR");
        assert!(color_allowed(true));
        assert!(!color_allowed(false));
        let colored = "\u{1b}[31mKiln\u{1b}[0m";
        assert_eq!(for_reader(colored, false), "Kiln");
        assert_eq!(for_reader(colored, true), colored);
    }
}
