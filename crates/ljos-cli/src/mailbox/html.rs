//! HTML reduced to the text a mail client would show.
//!
//! Not a full parser: tags, comments and raw-text elements are skipped,
//! entities decoded, and block elements become line breaks. Text inside an
//! element the reducer can tell is concealed is dropped: the `hidden`
//! attribute, or a style that sets `display:none`, `visibility:hidden`,
//! `opacity` near 0, a zero `font-size`, a zero width or height, a
//! transparent color or one equal to the background, a negative offset
//! or indent, `clip`, `clip-path` or `mso-hide:all`. The style may be
//! inline or a class or id rule in a `<style>` block seen earlier.
//!
//! Any `<style>` block or concealment marks the part as hiding content.
//! When a stylesheet does what the reducer cannot follow (an `@` rule
//! such as `@media`, a concealing rule on any selector other than a
//! class or id, a color in one rule equal to a background in another,
//! or a linked stylesheet), the text is withheld rather than guessed.
//! A link keeps its text and gains its target as plain text, so a
//! mismatched link is visible.

/// The visible text and what the HTML hid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visible {
    pub text: String,
    /// A style block or concealment was present, so the part hides, or
    /// may hide, text from a reader.
    pub hidden_content: bool,
    /// The reducer could not tell what is hidden; `text` must not be
    /// shown.
    pub withheld: bool,
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
    background: Option<String>,
}

#[derive(Default)]
struct HiddenRules {
    classes: Vec<String>,
    ids: Vec<String>,
    styled: bool,
    withheld: bool,
}

#[must_use]
pub fn visible_text(html: &str) -> Visible {
    let mut out = String::new();
    let mut dropped = false;
    let mut concealed = false;
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
        if tag.name == "link"
            && tag
                .attr("rel")
                .is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet"))
        {
            rules.withheld = true;
        }
        if VOID.contains(&tag.name.as_str()) || tag.self_closing {
            continue;
        }
        let style = declarations(tag.attr("style").unwrap_or(""));
        let background = value(&style, "background-color")
            .or_else(|| value(&style, "background"))
            .or_else(|| tag.attr("bgcolor"))
            .map(color)
            .or_else(|| stack.last().and_then(|o| o.background.clone()));
        let fg = value(&style, "color")
            .or_else(|| (tag.name == "font").then(|| tag.attr("color")).flatten())
            .map(color);
        let hidden = tag.attr("hidden").is_some()
            || conceals(&style)
            || fg.is_some_and(|c| background.as_deref() == Some(c.as_str()))
            || tag
                .attr("class")
                .is_some_and(|c| c.split_whitespace().any(|c| rules.has_class(c)))
            || tag.attr("id").is_some_and(|id| rules.has_id(id));
        let href = (tag.name == "a")
            .then(|| tag.attr("href").map(|h| decode(h).trim().to_string()))
            .flatten()
            .filter(|h| !h.is_empty());
        concealed |= hidden;
        stack.push(Open {
            name: tag.name,
            hidden,
            href,
            background,
        });
    }
    Visible {
        text: tidy(&out),
        hidden_content: dropped || concealed || rules.styled || rules.withheld,
        withheld: rules.withheld,
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

/// `property: value` pairs, lower case, whitespace and `!important`
/// removed.
fn declarations(css: &str) -> Vec<(String, String)> {
    css.split(';')
        .filter_map(|d| d.split_once(':'))
        .map(|(k, v)| {
            let squash = |s: &str| {
                s.chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
                    .to_ascii_lowercase()
            };
            (squash(k), squash(v).replace("!important", ""))
        })
        .collect()
}

fn value<'a>(decls: &'a [(String, String)], name: &str) -> Option<&'a str> {
    decls
        .iter()
        .rev()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// The leading number of a CSS length, `0` for `0px`, `-12` for `-12em`.
fn number(v: &str) -> Option<f32> {
    let end = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(v.len());
    v[..end].parse().ok()
}

/// Declarations that hide the element's text on their own.
fn conceals(decls: &[(String, String)]) -> bool {
    decls.iter().any(|(k, v)| match k.as_str() {
        "display" => v == "none",
        "visibility" => v == "hidden" || v == "collapse",
        "opacity" => number(v).is_some_and(|n| n <= 0.05),
        "font-size" => number(v)
            .is_some_and(|n| n <= 0.0 || (n <= 1.0 && (v.ends_with("px") || v.ends_with("pt")))),
        "width" | "height" | "max-width" | "max-height" => number(v) == Some(0.0),
        "left" | "top" | "right" | "bottom" | "text-indent" | "margin-left" | "margin-top" => {
            v.starts_with('-')
        }
        "clip" => v.starts_with("rect("),
        "clip-path" => v != "none",
        "transform" => v.contains("scale(0"),
        "mso-hide" => v == "all",
        "color" => v == "transparent" || v.ends_with(",0)") || v.ends_with(",0.0)"),
        _ => false,
    })
}

/// A color in one spelling, so `#FFF`, `#ffffff`, `white` and
/// `rgb(255,255,255)` compare equal.
fn color(v: &str) -> String {
    let v = v.to_ascii_lowercase().replace(' ', "");
    let hex = |r: u8, g: u8, b: u8| format!("#{r:02x}{g:02x}{b:02x}");
    if let Some(h) = v.strip_prefix('#') {
        let digits: Vec<u8> = h
            .chars()
            .filter_map(|c| c.to_digit(16).and_then(|d| u8::try_from(d).ok()))
            .collect();
        match digits.as_slice() {
            [r, g, b] if h.len() == 3 => return hex(r * 17, g * 17, b * 17),
            [r1, r2, g1, g2, b1, b2] if h.len() == 6 => {
                return hex(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2)
            }
            _ => {}
        }
    }
    if let Some(args) = v.strip_prefix("rgb(").and_then(|a| a.strip_suffix(')')) {
        let parts: Vec<u8> = args.split(',').filter_map(|p| p.parse().ok()).collect();
        if let [r, g, b] = parts.as_slice() {
            return hex(*r, *g, *b);
        }
    }
    match v.as_str() {
        "white" => hex(255, 255, 255),
        "black" => hex(0, 0, 0),
        _ => v,
    }
}

impl HiddenRules {
    /// Class and id selectors whose rule hides, from one `<style>` block.
    /// What cannot be followed sets `withheld`.
    fn read(&mut self, css: &str) {
        self.styled = true;
        let mut css = css.to_ascii_lowercase();
        while let Some(start) = css.find("/*") {
            let end = css[start..].find("*/").map_or(css.len(), |e| start + e + 2);
            css.replace_range(start..end, "");
        }
        if css.contains('@') {
            self.withheld = true;
        }
        let (mut colors, mut backgrounds) = (Vec::new(), Vec::new());
        for rule in css.split('}') {
            let Some((selectors, body)) = rule.split_once('{') else {
                continue;
            };
            let decls = declarations(body);
            colors.extend(value(&decls, "color").map(color));
            backgrounds.extend(
                ["background", "background-color"]
                    .iter()
                    .filter_map(|k| value(&decls, k))
                    .map(color),
            );
            if !conceals(&decls) {
                continue;
            }
            for selector in selectors.split(',') {
                let last = selector
                    .trim()
                    .rsplit(|c: char| c.is_whitespace() || ">+~".contains(c))
                    .next()
                    .unwrap_or("");
                match (last.rsplit_once('.'), last.rsplit_once('#')) {
                    (Some((_, class)), _) if simple(class) => self.classes.push(class.to_string()),
                    (None, Some((_, id))) if simple(id) => self.ids.push(id.to_string()),
                    _ => self.withheld = true,
                }
            }
        }
        if colors.iter().any(|c| backgrounds.contains(c)) {
            self.withheld = true;
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
        assert!(got.hidden_content && !got.withheld);
    }

    #[test]
    fn plain_html_drops_nothing_and_shows_link_targets() {
        let got = visible_text(
            "<head><title>t</title></head><div>A &amp; B</div><!-- c -->\
             <a href=\"https://example.test/x?a=1&amp;b=2\">here</a><script>evil()</script>",
        );
        assert_eq!(got.text, "A & B\nhere <https://example.test/x?a=1&b=2>");
        assert!(!got.hidden_content && !got.withheld);
    }

    #[test]
    fn inline_concealment_beyond_display_none_is_dropped() {
        for style in [
            "font-size:0",
            "font-size: 0px",
            "font-size:1px",
            "opacity:0",
            "opacity: 0.01",
            "color:#fff;background:#FFFFFF",
            "color:white; background-color: rgb(255, 255, 255)",
            "color: transparent",
            "position:absolute;left:-9999px",
            "text-indent:-9999px",
            "clip:rect(0,0,0,0)",
            "clip-path: inset(50%)",
            "max-height:0;overflow:hidden",
            "mso-hide:all",
            "visibility: collapse",
        ] {
            let html = format!("<p>Shown <span style=\"{style}\">secret</span> end</p>");
            let got = visible_text(&html);
            assert_eq!(got.text, "Shown end", "{style}");
            assert!(got.hidden_content && !got.withheld, "{style}");
        }
        let inherited = visible_text(
            "<div bgcolor=\"#000\"><p>Shown <font color=black>secret</font> end</p></div>",
        );
        assert_eq!(inherited.text, "Shown end");
    }

    #[test]
    fn a_style_block_marks_the_part_and_what_cannot_be_followed_is_withheld() {
        let plain = visible_text("<style>.a { color: red }</style><p>Hello</p>");
        assert_eq!(plain.text, "Hello");
        assert!(plain.hidden_content && !plain.withheld);
        for css in [
            "p { display: none }",
            "div > p { font-size: 0 }",
            "* { opacity: 0 }",
            "a:hover, .x { display: none }",
            "@media (max-width: 600px) { .x { display: none } }",
            "@import url(https://example.test/x.css);",
            ".a { color: #fff } body { background: white }",
        ] {
            let got = visible_text(&format!("<style>{css}</style><p>Hello</p><p>secret</p>"));
            assert!(got.hidden_content && got.withheld, "{css}");
        }
        let linked = visible_text("<link rel=stylesheet href=https://example.test/x.css><p>Hi</p>");
        assert!(linked.withheld);
    }

    #[test]
    fn a_stray_angle_bracket_is_text() {
        assert_eq!(visible_text("1 < 2 and 3 > 2").text, "1 < 2 and 3 > 2");
    }
}
