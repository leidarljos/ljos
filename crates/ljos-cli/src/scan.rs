//! Human pages for search, recall, ready, and the hook.
//!
//! Each line fits [`COLS`]. The pages name the place and say shown N of M.
//! A search page names the command that lists the rest. A ready page that
//! leaves tickets off ends with `--limit`, `--all`, `-p` and `--mine`.
//! The ranker score stays off the page. `--json` and the MCP rows do not
//! use this module. A pipe, and `NO_COLOR`, carry no ANSI.

/// Columns a human page fits, including a pipe read in an 80-column terminal.
pub const COLS: usize = 80;

/// What a text page prints when a ticket has no title.
/// JSON keeps the empty string.
pub const NO_TITLE: &str = "(no title)";

const TITLE_CUT: &str = "...";

/// A title that stays on one ticket line.
///
/// A blank title is [`NO_TITLE`]. A title longer than `room` columns is cut
/// with `...`. The cut counts characters, so a letter such as `Þ` is one
/// column. `room` of zero yields an empty string.
#[must_use]
pub fn fit_title(title: &str, room: usize) -> String {
    let flat = title.replace(['\n', '\r', '\t'], " ");
    let flat = flat.trim();
    if flat.is_empty() {
        if NO_TITLE.chars().count() <= room {
            return NO_TITLE.to_string();
        }
        return NO_TITLE.chars().take(room).collect();
    }
    if flat.chars().count() <= room {
        return flat.to_string();
    }
    let dots = TITLE_CUT.chars().count();
    if room <= dots {
        return flat.chars().take(room).collect();
    }
    let mut cut: String = flat.chars().take(room - dots).collect();
    cut.push_str(TITLE_CUT);
    cut
}

/// `lead` and the title on one line of at most `cols` columns.
///
/// `lead` includes the spaces before the title. The title is not wrapped
/// onto the next line. A blank title is [`NO_TITLE`].
#[must_use]
pub fn line_with_title(lead: &str, title: &str, cols: usize) -> String {
    let room = cols.saturating_sub(lead.chars().count());
    format!("{lead}{}", fit_title(title, room))
}

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

/// Break a token that does not fit. A URL breaks after `/`, `?`, `&`, `=`
/// or `#`. A hyphen, underscore or dot is the next place. Only a run with
/// none of those ends a line with `\`, so the rest is visibly the same word.
fn break_word(text: &str, cols: usize) -> String {
    if text.chars().count() <= cols {
        return text.to_string();
    }
    let indent_len = text.chars().take_while(|ch| *ch == ' ').count();
    let indent = " ".repeat(indent_len);
    let room = cols.saturating_sub(indent_len).max(1);
    let mut lines = Vec::new();
    let mut current = indent.clone();
    for piece in soft_pieces(text.trim_start_matches(' ')) {
        place_piece(&mut lines, &mut current, &indent, room, &piece);
    }
    if current.chars().count() > indent_len {
        lines.push(current);
    }
    lines.join("\n")
}

fn soft_pieces(word: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for ch in word.chars() {
        cur.push(ch);
        if matches!(ch, '/' | '?' | '&' | '=' | '#') {
            parts.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    let mut out = Vec::new();
    for part in parts {
        let mut cur = String::new();
        for ch in part.chars() {
            cur.push(ch);
            if matches!(ch, '-' | '_' | '.') && cur.chars().count() > 1 {
                out.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

fn place_piece(
    lines: &mut Vec<String>,
    current: &mut String,
    indent: &str,
    room: usize,
    piece: &str,
) {
    let indent_len = indent.chars().count();
    if current.chars().count() > indent_len
        && current.chars().count() + piece.chars().count() > indent_len + room
    {
        lines.push(std::mem::take(current));
        current.push_str(indent);
    }
    if piece.chars().count() <= room {
        current.push_str(piece);
        return;
    }
    let mut rest = piece;
    while !rest.is_empty() {
        if current.chars().count() > indent_len {
            lines.push(std::mem::take(current));
            current.push_str(indent);
        }
        let take = room.min(rest.chars().count());
        let more = rest.chars().count() > take;
        let cut = if more {
            take.saturating_sub(1).max(1)
        } else {
            take
        };
        let (head, tail) = split_at_chars(rest, cut);
        current.push_str(head);
        if more {
            current.push('\\');
            lines.push(std::mem::take(current));
            current.push_str(indent);
        }
        rest = tail;
    }
}

fn split_at_chars(text: &str, n: usize) -> (&str, &str) {
    match text.char_indices().nth(n) {
        Some((index, _)) => text.split_at(index),
        None => (text, ""),
    }
}

/// The first eight characters, which `ljos forget` accepts as an id.
#[must_use]
pub fn short_id(id: &str) -> &str {
    prefix_of(id, 8)
}

/// A prefix of `id`, at least eight characters when `id` is that long,
/// long enough that every id in the list has its own. `ljos forget`
/// accepts the eight-character prefix, and a longer one when several
/// memories share those eight.
#[must_use]
pub fn unique_prefixes<'a>(ids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let ids: Vec<&str> = ids.into_iter().map(str::trim).collect();
    let widest = ids.iter().map(|id| id.chars().count()).max().unwrap_or(0);
    let mut len = 8.min(widest);
    while len < widest {
        let mut seen = std::collections::BTreeSet::new();
        let clash = ids.iter().filter(|id| !id.is_empty()).any(|id| {
            let short = prefix_of(id, len);
            !seen.insert(short)
        });
        if !clash {
            break;
        }
        len += 1;
    }
    ids.iter()
        .map(|id| prefix_of(id, len).to_string())
        .collect()
}

fn prefix_of(id: &str, len: usize) -> &str {
    let id = id.trim();
    match id.char_indices().nth(len) {
        Some((index, _)) => &id[..index],
        None => id,
    }
}

/// The id `ljos search` prints.
///
/// The shortest prefix of `id` that starts one id in `store`. It keeps at
/// least eight hex digits of the trailing hex run when that run is long
/// enough. `local:0123abcd…` keeps `local:` and then eight hex digits. A
/// plain hex id takes eight digits. A shorter id is printed whole.
/// `ljos forget` accepts the result.
#[must_use]
pub fn shown_prefix(id: &str, store: &[&str]) -> String {
    let id = id.trim();
    if id.is_empty() {
        return String::new();
    }
    let width = id.chars().count();
    let mut len = prefix_floor(id).min(width);
    while len < width {
        let prefix = prefix_of(id, len);
        if !prefix_shared(prefix, id, store) {
            return prefix.to_string();
        }
        len += 1;
    }
    id.to_string()
}

/// Characters until the prefix holds eight hex digits of the trailing hex
/// run. The run is the hex digits at the end of `id`. No such run: eight
/// characters, or the whole id when it is shorter.
fn prefix_floor(id: &str) -> usize {
    let chars: Vec<char> = id.chars().collect();
    let mut tail = chars.len();
    while tail > 0 && chars[tail - 1].is_ascii_hexdigit() {
        tail -= 1;
    }
    let hex = chars.len() - tail;
    if hex == 0 {
        return chars.len().min(8);
    }
    tail + hex.min(8)
}

/// True when some id in `store` other than `id` starts with `prefix`.
fn prefix_shared(prefix: &str, id: &str, store: &[&str]) -> bool {
    store
        .iter()
        .map(|other| other.trim())
        .any(|other| !other.is_empty() && other != id && other.starts_with(prefix))
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
    head("Remembered", shown, matched, "fused order", shown < matched)
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
    /// Ranker number. The page does not print it. A meaning score and a
    /// word score use different scales, and a meaning score does not order
    /// the rows. `--json` keeps the number.
    pub score: f64,
    /// Scorers that named the hit, and how many ran.
    pub ballots: Option<(u32, u32)>,
    /// Full id. The page prints a prefix that names this row alone.
    pub id: Option<&'a str>,
    /// Ticket the memory was filed on, when it has one.
    pub ticket: Option<&'a str>,
    /// Place on this page. 1 is the first row.
    pub place: usize,
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
    /// Optional line above the count, before the rows.
    pub top_hint: Option<&'a str>,
    /// `--explain`: the order paragraph, instead of the one legend line.
    pub explain: bool,
    /// The query, for the command that lists the rest.
    pub query: &'a str,
    /// The paragraph `--explain` prints. Ignored when `explain` is false.
    pub reading: &'a str,
    /// This seat's rows come first. A word ranking leaves this false: the
    /// word score already includes a small boost, and a closer row stays ahead.
    pub seat_ahead: bool,
    /// Ids the printed prefix is unique among. Empty means the rows on
    /// this page.
    pub store: &'a [&'a str],
}

/// The human search page.
#[must_use]
pub fn search_page(page: &SearchPage<'_>) -> String {
    if page.rows.is_empty() {
        return nothing_matched_page(page.query);
    }
    let mut out = String::new();
    if let Some(hint) = page.top_hint.filter(|line| !line.trim().is_empty()) {
        out.push_str(hint.trim_end());
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(&shown_line(page));
    out.push('\n');
    if page.explain {
        let reading = page.reading.trim();
        if !reading.is_empty() {
            out.push_str(&wrap(reading, COLS));
            out.push('\n');
        }
    } else if page.seat_ahead {
        out.push_str("The first row is the closest match. This seat's rows come first.\n");
    } else {
        out.push_str("The first row is the closest match.\n");
    }
    out.push('\n');
    let page_ids: Vec<&str> = page.rows.iter().filter_map(|row| row.id).collect();
    let store = if page.store.is_empty() {
        page_ids.as_slice()
    } else {
        page.store
    };
    for row in page.rows {
        // A ticket id is what `ljos recall` takes. A cut of `Software-6mrw`
        // is not that id.
        let id_label = if row.label == "ticket" {
            row.id.unwrap_or("").to_string()
        } else {
            shown_prefix(row.id.unwrap_or(""), store)
        };
        out.push_str(&search_row(row, &id_label));
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
        let line = format!(
            "shown {shown} of {matched}, {}",
            search_hint(page.query, next_limit(matched, page.capped), page.capped,)
        );
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

/// One sentence when a search matched nothing and the store already holds
/// something. A shorter query is named when [`crate::word_rank::shorter_search`]
/// has one. The command is plain text, not a code span.
#[must_use]
pub fn nothing_matched_line(query: &str) -> String {
    let query = query.trim();
    let shown = crate::word_rank::quote_query(query);
    match crate::word_rank::shorter_search(query) {
        Some(cmd) => format!("Nothing matched {shown}. Try a shorter query, such as: {cmd}"),
        None => format!("Nothing matched {shown}."),
    }
}

/// [`nothing_matched_line`] for a terminal. The suggested command is not
/// wrapped. When the sentence fits, the command stays on that line. When
/// it does not, the command is the next line.
#[must_use]
pub fn nothing_matched_page(query: &str) -> String {
    let query = query.trim();
    let shown = crate::word_rank::quote_query(query);
    let head = format!("Nothing matched {shown}.");
    match crate::word_rank::shorter_search(query) {
        Some(cmd) => {
            let prose = format!("{head} Try a shorter query, such as:");
            plain_command_page(&prose, Some(&cmd))
        }
        None => plain_command_page(&head, None),
    }
}

/// The page when a relevance floor removed every row. The hint names
/// `--min-score 0`. It stays whole, on the next line when the sentence
/// does not fit.
#[must_use]
pub fn no_relevant_page(query: &str) -> String {
    let query = query.trim();
    let shown = crate::word_rank::quote_query(query);
    let head = format!("No relevant matches for {shown}.");
    let hint = "Pass --min-score 0 to see every row.";
    let one = format!("{head} {hint}");
    if one.chars().count() <= COLS && !one.contains('\n') {
        return ended(&one);
    }
    let mut out = wrap(&head, COLS);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(hint);
    out.push('\n');
    out
}

/// A search before anything has been stored. A shorter query would miss
/// too, so the sentence names the command that stores a lesson. The query
/// is not echoed into that command: a search term is not a lesson, so the
/// command carries the placeholder `LESSON`. The command is plain text.
#[must_use]
pub fn empty_store_line(query: &str) -> String {
    let shown = crate::word_rank::quote_query(query.trim());
    format!(
        "{} {}",
        empty_store_prose(&shown),
        store_lesson_command(query)
    )
}

/// [`empty_store_line`] for a terminal. The command is not wrapped.
#[must_use]
pub fn empty_store_page(query: &str) -> String {
    let shown = crate::word_rank::quote_query(query.trim());
    let cmd = store_lesson_command(query);
    plain_command_page(&empty_store_prose(&shown), Some(&cmd))
}

/// The sentences before the command of an empty-store search.
#[must_use]
pub fn empty_store_prose(shown: &str) -> String {
    format!(
        "Nothing matched {shown}. A shorter search would miss too. Try storing a lesson in your own words, such as:"
    )
}

/// `ljos remember` with the placeholder `LESSON`, whatever was searched.
#[must_use]
pub fn store_lesson_command(_query: &str) -> String {
    "ljos remember \"LESSON\"".to_string()
}

/// Prose stays within [`COLS`]. `cmd`, when set, is not wrapped. It stays
/// on the prose line when the whole sentence fits, and is the next line
/// when it does not.
fn plain_command_page(prose: &str, cmd: Option<&str>) -> String {
    let Some(cmd) = cmd else {
        return ended(&wrap(prose, COLS));
    };
    let one = format!("{prose} {cmd}");
    if one.chars().count() <= COLS && !one.contains('\n') {
        return ended(&one);
    }
    let mut out = wrap(prose, COLS);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(cmd);
    out.push('\n');
    out
}

fn ended(text: &str) -> String {
    let mut out = text.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
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

fn search_row(row: &SearchRow<'_>, id_label: &str) -> String {
    let ticket = row.ticket.filter(|ticket| !ticket.is_empty());
    let mut head = if row.label == "ticket" {
        match ticket {
            Some(id) => format!("ticket {id}"),
            None => row.label.to_string(),
        }
    } else {
        row.label.to_string()
    };
    if !row.age.is_empty() {
        head.push_str(", ");
        head.push_str(row.age);
    }
    if let Some(seat) = row.from.filter(|seat| !seat.is_empty()) {
        head.push_str(", from ");
        head.push_str(seat);
    }
    if row.label != "ticket" {
        if let Some(id) = ticket {
            head.push_str(", ticket ");
            head.push_str(id);
        }
    }
    // Place is the order a person sees. The ranker number is not that
    // order on a meaning page, and a word score is a different scale.
    let mut foot = format!("place {}", row.place.max(1));
    if row.local {
        foot.push_str(", local, not in the pack");
    } else if let Some((hit, of)) = row.ballots {
        foot.push_str(&format!(", {hit} of {of} checks"));
    }
    if !id_label.is_empty() {
        foot.push_str(", id ");
        foot.push_str(id_label);
    }
    claim_block(&head, row.text, &foot, 0)
}

/// `vissue ready` rows, relaid so the title is not pushed past column 80
/// by a padded id. JSON stays with the caller. Every row is printed.
#[must_use]
pub fn ready_text(raw: &str) -> String {
    ready_page(raw, None)
}

enum ReadyPiece {
    Row(ReadyRow),
    Loose(String),
}

/// The last line of a short ready page. It names the flags that show more
/// tickets or narrow the page.
const READY_HINT: &str = "See more with `--limit` or `--all`. Narrow with `-p` or `--mine`.";

/// The ready page. `limit` is how many tickets to print. `None` prints
/// every row. The header is `ready  shown X of Y`. A page that leaves
/// tickets off ends with [`READY_HINT`].
#[must_use]
pub fn ready_page(raw: &str, limit: Option<usize>) -> String {
    let raw = strip_ansi(raw);
    let mut pieces = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(row) = parse_ready(line) {
            pieces.push(ReadyPiece::Row(row));
        } else {
            pieces.push(ReadyPiece::Loose(line.to_string()));
        }
    }
    let total = pieces.len();
    let take = limit.unwrap_or(total).min(total);
    let mut out = count_line("ready", take, total, "");
    out.push('\n');
    if total == 0 {
        out.push_str("no open ticket is free of an open blocker, so run `ljos file TITLE`.\n");
        return out;
    }
    out.push_str("open and not waiting on an open ticket, so run `ljos show ID`.\n");
    for piece in pieces.into_iter().take(take) {
        out.push('\n');
        match piece {
            ReadyPiece::Row(row) => {
                out.push_str(&ready_row(&row));
                out.push('\n');
            }
            ReadyPiece::Loose(line) => {
                out.push_str(&wrap(&line, COLS));
                out.push('\n');
            }
        }
    }
    if take < total {
        out.push_str(&wrap(READY_HINT, COLS));
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
    // The id, state and priority are ASCII columns. The title may hold
    // any letter, so the rest of the line is not required to be ASCII.
    // A line that ends on the priority has a blank title. Trailing spaces
    // were already removed, so that line is 37 columns.
    if line.len() < 37
        || !line
            .as_bytes()
            .get(..37)
            .is_some_and(|prefix| prefix.is_ascii())
    {
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
    let rest = line
        .get(37..)
        .unwrap_or("")
        .strip_prefix("  ")
        .unwrap_or("");
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
    let mut out = line_with_title(&format!("{head}  "), &row.title, COLS);
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
    fn a_short_ready_page_names_how_to_see_more_or_narrow() {
        let mut raw = String::new();
        for n in 0..3 {
            raw.push_str(&format!(
                "{:<22} {:<9} [#{}]  ticket {n}\n",
                format!("demo-{n:04}"),
                "TODO",
                "B"
            ));
        }
        let text = ready_page(&raw, Some(2));
        assert!(text.contains("ready  shown 2 of 3"), "{text}");
        assert!(text.contains("demo-0000"), "{text}");
        assert!(text.contains("demo-0001"), "{text}");
        assert!(!text.contains("demo-0002"), "{text}");
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("ready  shown 2 of 3"));
        let hint = lines.last().unwrap_or("");
        assert_eq!(hint, READY_HINT, "{text}");
        assert!(hint.chars().count() <= COLS, "{hint}");
        assert!(hint.contains("`--limit`"), "{hint}");
        assert!(hint.contains("`--all`"), "{hint}");
        assert!(hint.contains("`-p`"), "{hint}");
        assert!(hint.contains("`--mine`"), "{hint}");
        assert_eq!(text.matches(READY_HINT).count(), 1, "{text}");
        assert!(text.trim_end().ends_with(READY_HINT), "{text}");
        let all = ready_page(&raw, None);
        assert!(all.starts_with("ready  shown 3 of 3\n"), "{all}");
        assert!(!all.contains(READY_HINT), "{all}");
        assert!(
            all.lines().all(|line| line.chars().count() <= COLS),
            "{all}"
        );
    }

    /// Headings taken from tickets whose titles used to leave the line.
    /// The first is an org heading whose title is everything after the
    /// priority and before the tag. A blank title is named. A letter
    /// outside ASCII stays one column.
    #[test]
    fn a_long_title_stays_on_the_ready_line() {
        let shapes = [
            (
                "ljos-633g",
                "TODO",
                "A",
                "guard: tmux by full path, renamed binary, -L or -S bypasses send-keys checks",
            ),
            (
                "ljos-tile",
                "STARTED",
                "A",
                "Glaze the tile after reading https://kiln.example/logs/2026/november/reykjavik/damper-and-thermocouple-readings/line-4/workshop-north/session-that-is-one-long-url",
            ),
            ("ljos-blank", "TODO", "C", ""),
            (
                "ljos-ice",
                "TODO",
                "B",
                "Þéttingin í ofninum í Reykjavík er gömul og þarf nýja þéttingu áður en morguneldurinn kviknar í verkstæðinu",
            ),
        ];
        let mut raw = String::new();
        for (id, state, priority, title) in shapes {
            raw.push_str(&format!("{id:<22} {state:<9} [#{priority}]  {title}\n"));
        }
        let text = ready_text(&raw);
        assert!(text.contains("ready  shown 4 of 4"), "{text}");
        for (id, state, priority, title) in shapes {
            let lead = format!("{id}  {state}  [#{priority}]  ");
            let want = line_with_title(&lead, title, COLS);
            let line = text
                .lines()
                .find(|line| line.starts_with(id))
                .unwrap_or_else(|| panic!("no ticket line for {id}\n{text}"));
            assert_eq!(line, want, "{text}");
            assert!(line.chars().count() <= COLS, "{line}");
            assert!(
                line.contains(priority) && !line.ends_with(&format!("[#{priority}]")),
                "the ticket line ends at the priority: {line}"
            );
        }
        assert!(
            !text.lines().any(|line| line.starts_with("  guard:")),
            "{text}"
        );
        assert!(
            text.lines()
                .any(|line| line.starts_with("ljos-blank") && line.ends_with(NO_TITLE)),
            "{text}"
        );
    }

    #[test]
    fn wrap_keeps_every_line_inside_eighty_columns() {
        let long = "Score is how the scorers ranked this query. The fraction is how many of them named the hit. Neither is whether the claim is true.";
        let wrapped = wrap(long, COLS);
        assert!(wrapped.lines().all(|line| line.chars().count() <= COLS));
        assert!(wrapped.lines().count() > 1);
        let url = "https://kiln.example/logs/2026/november/reykjavik/damper-and-thermocouple-readings/line-4";
        let broken = wrap(url, COLS);
        assert!(broken.lines().all(|line| line.chars().count() <= COLS));
        assert!(
            broken
                .lines()
                .all(|line| !line.contains("read\n") && !line.ends_with("read")),
            "{broken}"
        );
        assert!(broken.contains("readings/"), "{broken}");
        assert!(!broken.contains("read\nings"), "{broken}");
        let solid = "SupercalifragilisticexpialidociousKilnDamperReadingThatExceedsEightyColumnsWithoutABreak";
        let cut = wrap(solid, 20);
        assert!(cut.contains('\\'), "{cut}");
        assert!(cut.lines().all(|line| line.chars().count() <= 20), "{cut}");
    }

    #[test]
    fn a_search_page_is_one_legend_line_and_does_not_print_the_score() {
        let row = |place: usize, score: f64, id: &'static str| SearchRow {
            label: "lesson",
            age: "today",
            from: None,
            text: "The damper sticks.",
            score,
            ballots: Some((2, 3)),
            id: Some(id),
            ticket: None,
            place,
            local: false,
        };
        let rows = [row(1, 68.0, "idkiln01"), row(2, 101.0, "idkiln02")];
        let reading = "The first row is the closest match, and this seat's rows come first. A score from 0 to 100 is how well the sentence matched.";
        let page = |explain: bool, seat_ahead: bool| SearchPage {
            rows: &rows,
            shown: 2,
            matched: 2,
            capped: false,
            top_hint: None,
            explain,
            query: "kiln",
            reading,
            seat_ahead,
            store: &[],
        };
        let meaning = search_page(&page(false, true));
        assert_eq!(
            meaning.lines().nth(1),
            Some("The first row is the closest match. This seat's rows come first.")
        );
        assert!(
            !meaning.contains("68.00") && !meaning.contains("101.00"),
            "{meaning}"
        );
        assert!(meaning.contains("place 1, 2 of 3 checks"), "{meaning}");
        assert!(meaning.contains("place 2, 2 of 3 checks"), "{meaning}");
        let words = search_page(&page(false, false));
        assert_eq!(
            words.lines().nth(1),
            Some("The first row is the closest match.")
        );
        assert!(
            !words.contains("68.00") && !words.contains("101.00"),
            "{words}"
        );
        let explained = search_page(&page(true, true));
        assert!(explained.contains("0 to 100"), "{explained}");
        assert!(!explained.contains("68.00"), "{explained}");
        for page in [&meaning, &words, &explained] {
            assert!(
                page.lines().all(|line| line.chars().count() <= COLS),
                "{page}"
            );
        }
    }

    #[test]
    fn a_capped_search_page_names_the_next_page_once() {
        let row = SearchRow {
            label: "lesson",
            age: "today",
            from: None,
            text: "The damper sticks.",
            score: 68.0,
            ballots: None,
            id: Some("idkiln01"),
            ticket: None,
            place: 1,
            local: false,
        };
        let rows = [row];
        let page = SearchPage {
            rows: &rows,
            shown: 1,
            matched: 19,
            capped: true,
            top_hint: None,
            explain: false,
            query: "kiln",
            reading: "",
            seat_ahead: false,
            store: &[],
        };
        let page_text = search_page(&page);
        let first = page_text.lines().next().unwrap_or("");
        assert_eq!(
            first.matches("and the next page asks for more").count(),
            1,
            "{first}"
        );
        assert!(!first.contains(", or more"), "{first}");
    }

    #[test]
    fn a_shown_prefix_is_eight_hex_digits_and_unique_in_the_store() {
        let shared = format!("45{}", "a".repeat(40));
        let local_a = format!("local:{shared}1111111111111111111111");
        let local_b = format!("local:{shared}2222222222222222222222");
        let local_c = format!("local:45{}3333333333333333333333", "b".repeat(40));
        let plain_a = format!("{}{}", "a".repeat(8), "1".repeat(56));
        let plain_b = format!("{}{}", "a".repeat(8), "2".repeat(56));
        let plain_c = format!("{}{}", "b".repeat(8), "3".repeat(56));
        let ids = [
            local_a.as_str(),
            local_b.as_str(),
            local_c.as_str(),
            plain_a.as_str(),
            plain_b.as_str(),
            plain_c.as_str(),
        ];
        let want = [
            format!("local:{shared}1"),
            format!("local:{shared}2"),
            "local:45bbbbbb".to_string(),
            format!("{}1", "a".repeat(8)),
            format!("{}2", "a".repeat(8)),
            "b".repeat(8),
        ];
        for (id, prefix) in ids.iter().zip(want) {
            let shown = shown_prefix(id, &ids);
            assert_eq!(shown, prefix, "{id}");
            assert!(id.starts_with(&shown));
            let hex = shown
                .chars()
                .rev()
                .take_while(|ch| ch.is_ascii_hexdigit())
                .count();
            assert!(hex >= 8, "{shown}");
            let hits = ids.iter().filter(|other| other.starts_with(&shown)).count();
            assert_eq!(hits, 1, "{shown}");
        }
        assert_eq!(shown_prefix("id-24", &["id-24"]), "id-24");
        assert_eq!(shown_prefix("local:ab", &["local:ab"]), "local:ab");
    }

    #[test]
    fn a_miss_names_a_shorter_query_without_splitting_the_command() {
        let short = nothing_matched_page("which fusion");
        assert_eq!(
            short,
            "Nothing matched \"which fusion\". Try a shorter query, such as: ljos search fusion\n"
        );
        assert!(
            short.lines().all(|line| line.chars().count() <= COLS),
            "{short}"
        );
        assert!(!short.contains('`'), "{short}");

        let long = nothing_matched_page("usability friction confusing error");
        assert!(
            long.lines().any(|line| line == "ljos search usability"),
            "{long}"
        );
        assert!(!long.contains('`'), "{long}");
        assert!(
            long.lines()
                .filter(|line| *line != "ljos search usability")
                .all(|line| line.chars().count() <= COLS),
            "{long}"
        );
        let wide = "x".repeat(90);
        let query = format!("note {wide}");
        let page = nothing_matched_page(&query);
        let cmd = format!("ljos search {wide}");
        assert!(page.lines().any(|line| line == cmd), "{page}");
        assert!(!page.contains('`'), "{page}");

        assert_eq!(
            nothing_matched_line("usability"),
            "Nothing matched usability."
        );
        assert_eq!(nothing_matched_line("the"), "Nothing matched the.");
        assert_eq!(nothing_matched_line("a\"b"), "Nothing matched this query.");
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
