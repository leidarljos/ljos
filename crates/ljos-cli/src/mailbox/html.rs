//! HTML reduced to the text a mail client would show.
//!
//! Not a full parser: tags, comments and raw-text elements are skipped,
//! entities decoded, and block elements become line breaks. Text inside an
//! element hidden by `display:none`, `visibility:hidden` or the `hidden`
//! attribute, inline or through a class or id rule in a `<style>` block
//! seen earlier, is dropped and reported. A link keeps its text and gains
//! its target as plain text, so a mismatched link is visible.

/// The visible text, and whether hidden text was dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visible {
    pub text: String,
    pub hidden_dropped: bool,
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];
const RAW: &[&str] = &[
    "script", "style", "title", "template", "noscript", "textarea",
];
const BLOCK: &[&str] = &[
    "p",
    "div",
    "br",
    "tr",
    "li",
    "ul",
    "ol",
    "table",
    "blockquote",
    "hr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "pre",
    "section",
    "article",
    "header",
    "footer",
];

struct Open {
    name: String,
    hidden: bool,
    href: Option<String>,
}

#[derive(Default)]
struct HiddenRules {
    classes: Vec<String>,
    ids: Vec<String>,
}

#[must_use]
pub fn visible_text(html: &str) -> Visible {
    let mut out = String::new();
    let mut dropped = false;
    let mut stack: Vec<Open> = Vec::new();
    let mut rules = HiddenRules::default();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            text(rest, &stack, &mut out, &mut dropped);
            break;
        };
        text(&rest[..lt], &stack, &mut out, &mut dropped);
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        let Some((tag, after)) = parse_tag(rest) else {
            text("<", &stack, &mut out, &mut dropped);
            rest = &rest[1..];
            continue;
        };
        rest = after;
        if tag.closing {
            if let Some(at) = stack.iter().rposition(|o| o.name == tag.name) {
                for open in stack.drain(at..).rev() {
                    if let Some(href) = open.href.filter(|_| !open.hidden) {
                        out.push_str(&format!(" <{href}>"));
                    }
                }
            }
            if BLOCK.contains(&tag.name.as_str()) {
                out.push('\n');
            }
            continue;
        }
        if BLOCK.contains(&tag.name.as_str()) {
            out.push('\n');
        }
        if RAW.contains(&tag.name.as_str()) && !tag.self_closing {
            let close = format!("</{}", tag.name);
            let end = find_ascii_ci(rest, &close).unwrap_or(rest.len());
            if tag.name == "style" {
                rules.read(&rest[..end]);
            }
            rest = &rest[end..];
            continue;
        }
        if VOID.contains(&tag.name.as_str()) || tag.self_closing {
            continue;
        }
        let hidden = tag.attr("hidden").is_some()
            || tag.attr("style").is_some_and(hides)
            || tag
                .attr("class")
                .is_some_and(|c| c.split_whitespace().any(|c| rules.has_class(c)))
            || tag.attr("id").is_some_and(|id| rules.has_id(id));
        let href = (tag.name == "a")
            .then(|| tag.attr("href").map(|h| decode(h).trim().to_string()))
            .flatten()
            .filter(|h| !h.is_empty());
        stack.push(Open {
            name: tag.name,
            hidden,
            href,
        });
    }
    Visible {
        text: tidy(&out),
        hidden_dropped: dropped,
    }
}

fn text(raw: &str, stack: &[Open], out: &mut String, dropped: &mut bool) {
    if raw.is_empty() {
        return;
    }
    let decoded = decode(raw);
    if stack.iter().any(|o| o.hidden) {
        *dropped |= !decoded.trim().is_empty();
        return;
    }
    out.push_str(&decoded);
}

/// Inline CSS that hides the element.
fn hides(style: &str) -> bool {
    let css: String = style
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    css.contains("display:none") || css.contains("visibility:hidden")
}

impl HiddenRules {
    /// Class and id selectors whose rule hides, from one `<style>` block.
    fn read(&mut self, css: &str) {
        let mut css = css.to_ascii_lowercase();
        while let Some(start) = css.find("/*") {
            let end = css[start..].find("*/").map_or(css.len(), |e| start + e + 2);
            css.replace_range(start..end, "");
        }
        for rule in css.split('}') {
            let Some((selectors, body)) = rule.split_once('{') else {
                continue;
            };
            if !hides(body) {
                continue;
            }
            for selector in selectors.split(',') {
                let last = selector
                    .trim()
                    .rsplit(|c: char| c.is_whitespace() || ">+~".contains(c))
                    .next()
                    .unwrap_or("");
                if let Some((_, class)) = last.rsplit_once('.') {
                    if simple(class) {
                        self.classes.push(class.to_string());
                    }
                } else if let Some((_, id)) = last.rsplit_once('#') {
                    if simple(id) {
                        self.ids.push(id.to_string());
                    }
                }
            }
        }
    }

    fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c.eq_ignore_ascii_case(class))
    }

    fn has_id(&self, id: &str) -> bool {
        self.ids.iter().any(|i| i.eq_ignore_ascii_case(id))
    }
}

fn simple(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

struct Tag {
    name: String,
    closing: bool,
    self_closing: bool,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// One tag at the start of `s`, and the text after it. `None` when `s`
/// does not start a tag, so the `<` is text.
fn parse_tag(s: &str) -> Option<(Tag, &str)> {
    let mut chars = s[1..].char_indices().peekable();
    let mut closing = false;
    if let Some((_, '/')) = chars.peek() {
        closing = true;
        chars.next();
    }
    if let Some((_, c)) = chars.peek() {
        if *c == '!' || *c == '?' {
            let end = s.find('>')?;
            return Some((
                Tag {
                    name: String::new(),
                    closing: true,
                    self_closing: true,
                    attrs: Vec::new(),
                },
                &s[end + 1..],
            ));
        }
    }
    let mut name = String::new();
    while let Some((_, c)) = chars.peek() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_lowercase());
            chars.next();
        } else {
            break;
        }
    }
    if name.is_empty() {
        return None;
    }
    let mut attrs = Vec::new();
    let mut self_closing = false;
    loop {
        while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
            chars.next();
        }
        let (at, c) = chars.next()?;
        match c {
            '>' => return Some((tag(name, closing, self_closing, attrs), &s[1 + at + 1..])),
            '/' => self_closing = true,
            _ => {
                let mut key = c.to_ascii_lowercase().to_string();
                while let Some((_, c)) = chars.peek() {
                    if c.is_whitespace() || *c == '=' || *c == '>' || *c == '/' {
                        break;
                    }
                    key.push(c.to_ascii_lowercase());
                    chars.next();
                }
                while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                    chars.next();
                }
                let mut value = String::new();
                if let Some((_, '=')) = chars.peek() {
                    chars.next();
                    while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                        chars.next();
                    }
                    match chars.peek().map(|(_, c)| *c) {
                        Some(q @ ('"' | '\'')) => {
                            chars.next();
                            for (_, c) in chars.by_ref() {
                                if c == q {
                                    break;
                                }
                                value.push(c);
                            }
                        }
                        _ => {
                            while let Some((_, c)) = chars.peek() {
                                if c.is_whitespace() || *c == '>' {
                                    break;
                                }
                                value.push(*c);
                                chars.next();
                            }
                        }
                    }
                }
                attrs.push((key, value));
            }
        }
    }
}

fn tag(name: String, closing: bool, self_closing: bool, attrs: Vec<(String, String)>) -> Tag {
    Tag {
        name,
        closing,
        self_closing,
        attrs,
    }
}

fn find_ascii_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// The common named entities and every numeric one.
fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..semi];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .map(|hex| u32::from_str_radix(hex, 16))
                .or_else(|| name.strip_prefix('#').map(str::parse::<u32>))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Runs of spaces become one, lines are trimmed, and no more than one
/// blank line is kept.
fn tidy(s: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in s.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() && lines.last().is_none_or(String::is_empty) {
            continue;
        }
        lines.push(line);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_text_is_dropped_and_reported() {
        let html = "<style>.x { display : none } #y{visibility:hidden}</style>\
            <p>Shown <span style='DISPLAY: none'>inline</span>\
            <b class='a x'>class</b><i id=y>id</i><u hidden>attr</u> end</p>";
        let got = visible_text(html);
        assert_eq!(got.text, "Shown end");
        assert!(got.hidden_dropped);
    }

    #[test]
    fn plain_html_drops_nothing_and_shows_link_targets() {
        let got = visible_text(
            "<head><title>t</title></head><div>A &amp; B</div><!-- c -->\
             <a href=\"https://example.test/x?a=1&amp;b=2\">here</a><script>evil()</script>",
        );
        assert_eq!(got.text, "A & B\nhere <https://example.test/x?a=1&b=2>");
        assert!(!got.hidden_dropped);
    }

    #[test]
    fn a_stray_angle_bracket_is_text() {
        assert_eq!(visible_text("1 < 2 and 3 > 2").text, "1 < 2 and 3 > 2");
    }
}
