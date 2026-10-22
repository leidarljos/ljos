//! HTML reduced to the text a mail client would show.
//!
//! Not a full parser: tags, comments and raw-text elements are skipped,
//! entities decoded, and block elements become line breaks. Attribute
//! values are entity-decoded before use, and CSS has its comments
//! removed and its escapes decoded outside strings. As in HTML, `/>`
//! closes only a void element or one inside `<svg>` or `<math>`; any
//! other element stays open.
//!
//! Text the reducer can tell is concealed is dropped: inside an element
//! with the `hidden` attribute, a class or id rule that conceals, or a
//! style that sets `display:none`, `visibility:hidden` or `collapse`,
//! `content-visibility:hidden`, `mso-hide:all`, a width or height of
//! 1px or less, an offset or indent that is negative or 1000px or more,
//! `clip`, `clip-path`, or any transform; under an opacity (a number or
//! a percentage, multiplied down the tree) of 0.05 or less or a font
//! size (resolved down the tree) of 1px or less; or drawn in a color
//! whose alpha is 0.05 or less, or within 16 of its background in every
//! channel (black text on white when nothing sets them). A value of one
//! of those properties the reducer cannot read, or any `var()`, counts
//! as concealing.
//!
//! Every `<style>` block, wherever it sits, is read before the body, and
//! any block or concealment marks the part as hiding content. Rules
//! inside `@media`, `@supports` and other block at-rules are read as if
//! they apply. When the HTML does what the reducer cannot follow
//! (`@import`, a concealing rule on any selector other than a class or
//! id, a rule nested in a style rule or blocks nested more than 8 deep,
//! a stylesheet color within 16 of a background set anywhere or, when
//! its rule sets no background, of white, a stylesheet background within
//! 16 of a color set anywhere or, when its rule sets no color, of black,
//! a linked stylesheet, or a `style`, `class`, `id` or `hidden`
//! attribute holding a reference the reducer cannot decode), the text is
//! withheld rather than guessed. A link keeps its text and gains its
//! target as plain text, so a mismatched link is visible.

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
    paint: Paint,
}

/// How the text inside an element is drawn, from it and its ancestors.
#[derive(Clone)]
struct Paint {
    fg: Color,
    bg: Color,
    opacity: f32,
    font_px: f32,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            fg: BLACK,
            bg: WHITE,
            opacity: 1.0,
            font_px: 16.0,
        }
    }
}

/// What one element's declarations and color attributes set; `None`
/// for each part left to the parent.
struct Own {
    fg: Option<Color>,
    bg: Option<Color>,
    opacity: Option<f32>,
    font_px: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
enum Color {
    Rgb([u8; 3], f32),
    /// A name the reducer has no value for, compared by name.
    Named(String),
}

const BLACK: Color = Color::Rgb([0, 0, 0], 1.0);
const WHITE: Color = Color::Rgb([255, 255, 255], 1.0);

#[derive(Default)]
struct HiddenRules {
    classes: Vec<String>,
    ids: Vec<String>,
    colors: Vec<Color>,
    backgrounds: Vec<Color>,
    styled: bool,
    withheld: bool,
}

#[must_use]
pub fn visible_text(html: &str) -> Visible {
    let mut out = String::new();
    let mut dropped = false;
    let mut concealed = false;
    let mut stack: Vec<Open> = Vec::new();
    let mut rules = HiddenRules::of(html);
    let (mut seen_fg, mut seen_bg) = (Vec::new(), Vec::new());
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
        if tag.odd && !tag.closing {
            rules.withheld = true;
        }
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
        if RAW.contains(&tag.name.as_str()) {
            let close = format!("</{}", tag.name);
            rest = &rest[find_ascii_ci(rest, &close).unwrap_or(rest.len())..];
            continue;
        }
        if tag.name == "link"
            && tag
                .attr("rel")
                .is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet"))
        {
            rules.withheld = true;
        }
        let foreign = ["svg", "math"].contains(&tag.name.as_str())
            || stack.iter().any(|o| o.name == "svg" || o.name == "math");
        if VOID.contains(&tag.name.as_str()) || (tag.self_closing && foreign) {
            continue;
        }
        let style = declarations(&css_text(tag.attr("style").unwrap_or("")));
        let parent = stack
            .last()
            .map_or_else(Paint::default, |o| o.paint.clone());
        let set = own(&style, Some(&tag), parent.font_px);
        if let Some(set) = &set {
            seen_fg.extend(set.fg.clone());
            seen_bg.extend(set.bg.clone());
        }
        let paint = set.as_ref().map(|set| Paint {
            fg: set.fg.clone().unwrap_or_else(|| parent.fg.clone()),
            bg: set.bg.clone().unwrap_or_else(|| parent.bg.clone()),
            opacity: parent.opacity * set.opacity.unwrap_or(1.0),
            font_px: set.font_px.unwrap_or(parent.font_px),
        });
        let hidden = tag.attr("hidden").is_some()
            || hides(&style)
            || paint
                .as_ref()
                .is_none_or(|p| p.opacity <= 0.05 || p.font_px <= 1.0)
            || tag
                .attr("class")
                .is_some_and(|c| c.split_whitespace().any(|c| rules.has_class(c)))
            || tag.attr("id").is_some_and(|id| rules.has_id(id));
        let href = (tag.name == "a")
            .then(|| tag.attr("href").map(|h| h.trim().to_string()))
            .flatten()
            .filter(|h| !h.is_empty());
        concealed |= hidden;
        stack.push(Open {
            name: tag.name,
            hidden,
            href,
            paint: paint.unwrap_or(parent),
        });
    }
    let clash = rules.colors.iter().any(|c| {
        rules
            .backgrounds
            .iter()
            .chain(&seen_bg)
            .any(|b| close(c, b))
    }) || rules
        .backgrounds
        .iter()
        .any(|b| seen_fg.iter().any(|c| close(c, b)));
    let withheld = rules.withheld || clash;
    Visible {
        text: tidy(&out),
        hidden_content: dropped || concealed || rules.styled || withheld,
        withheld,
    }
}

fn text(raw: &str, stack: &[Open], out: &mut String, dropped: &mut bool) {
    if raw.is_empty() {
        return;
    }
    let decoded = decode(raw);
    if stack.iter().any(|o| o.hidden) || stack.last().is_some_and(|o| invisible(&o.paint)) {
        *dropped |= !decoded.trim().is_empty();
        return;
    }
    out.push_str(&decoded);
}

/// CSS as a client reads it: comments removed and escapes such as `\3a`
/// decoded, both outside strings.
fn css_text(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    let mut quote = None;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), _) => {
                out.push(c);
                if c == '\\' {
                    out.extend(chars.next());
                } else if c == q || matches!(c, '\n' | '\r' | '\x0c') {
                    quote = None;
                }
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                out.push(c);
            }
            (None, '/') if chars.peek() == Some(&'*') => {
                chars.next();
                let mut star = false;
                for c in chars.by_ref() {
                    if star && c == '/' {
                        break;
                    }
                    star = c == '*';
                }
            }
            (None, '\\') => {
                let mut hex = String::new();
                while hex.len() < 6 && chars.peek().is_some_and(char::is_ascii_hexdigit) {
                    hex.extend(chars.next());
                }
                if hex.is_empty() {
                    out.push(chars.next().unwrap_or('\u{FFFD}'));
                } else {
                    chars.next_if(|c| c.is_whitespace());
                    let code = u32::from_str_radix(&hex, 16).ok().filter(|&c| c != 0);
                    out.push(code.and_then(char::from_u32).unwrap_or('\u{FFFD}'));
                }
            }
            (None, _) => out.push(c),
        }
    }
    out
}

/// `property: value` pairs in lower case, runs of whitespace as one
/// space, `!important` removed.
fn declarations(css: &str) -> Vec<(String, String)> {
    css.split(';')
        .filter_map(|d| d.split_once(':'))
        .map(|(k, v)| {
            let mut v = v.split_whitespace().collect::<Vec<_>>().join(" ");
            if let Some(bang) = v.find('!') {
                v.truncate(bang);
            }
            (k.trim().to_ascii_lowercase(), v.trim().to_ascii_lowercase())
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

/// A CSS number and the unit after it: `(-12.0, "em")` for `-12em`,
/// `(0.001, "%")` for `1e-3%`.
fn number(v: &str) -> Option<(f32, &str)> {
    let mut end = v
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '+' | '-')))
        .unwrap_or(v.len());
    if let Some(exp) = v[end..].strip_prefix(['e', 'E']) {
        let digits = exp.trim_start_matches(['+', '-']);
        let run = digits
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(digits.len());
        if run > 0 {
            end += 1 + exp.len() - digits.len() + run;
        }
    }
    let n: f32 = v[..end].parse().ok().filter(|n: &f32| n.is_finite())?;
    Some((n, &v[end..]))
}

/// A length in px, `em` against `em` px and `%` against `percent` px.
fn px(v: &str, em: f32, percent: f32) -> Option<f32> {
    let (n, unit) = number(v)?;
    let scale = match unit {
        "" | "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        "em" => em,
        "rem" => 16.0,
        "ex" | "ch" => em / 2.0,
        "%" => percent,
        "vw" | "vh" | "vmin" | "vmax" => 3.2,
        _ => return None,
    };
    Some(n * scale)
}

/// A keyword that leaves the value to the parent or the default.
fn inherits(v: &str) -> bool {
    matches!(
        v,
        "inherit" | "initial" | "unset" | "revert" | "revert-layer" | "currentcolor"
    )
}

/// Declarations that hide the element's text whatever its parent is.
fn hides(decls: &[(String, String)]) -> bool {
    decls.iter().any(|(k, v)| {
        let keyword = inherits(v)
            || matches!(
                v.as_str(),
                "auto" | "none" | "normal" | "fit-content" | "max-content" | "min-content"
            );
        let len = || px(v, 16.0, 6.0);
        v.contains("var(")
            || match k.as_str() {
                "display" => v == "none",
                "visibility" => v == "hidden" || v == "collapse",
                "content-visibility" => v == "hidden",
                "width" | "height" | "max-width" | "max-height" => {
                    !keyword && len().is_none_or(|n| n <= 1.0)
                }
                "left" | "top" | "right" | "bottom" | "text-indent" | "margin-left"
                | "margin-top" => !keyword && len().is_none_or(|n| !(0.0..1000.0).contains(&n)),
                "clip" => v.starts_with("rect("),
                "clip-path" => v != "none",
                "transform" | "translate" | "scale" | "rotate" => v != "none",
                "mso-hide" => v == "all",
                _ => false,
            }
    })
}

/// What `decls` and, for an element, its color attributes set. `None`
/// when a color, opacity or font size cannot be read.
fn own(decls: &[(String, String)], tag: Option<&Tag>, parent_font: f32) -> Option<Own> {
    let attr =
        |element: &str, name: &str| tag.filter(|t| t.name == element).and_then(|t| t.attr(name));
    let fg = value(decls, "-webkit-text-fill-color")
        .filter(|v| !inherits(v))
        .or_else(|| value(decls, "color"))
        .or_else(|| attr("font", "color"))
        .or_else(|| attr("body", "text"))
        .filter(|v| !inherits(v));
    let fg = match fg {
        Some(v) => Some(color(v)?),
        None => None,
    };
    let bg = decls
        .iter()
        .rev()
        .find(|(k, _)| k == "background" || k == "background-color");
    let bg = match bg {
        Some((_, v)) => background(v)?,
        None => tag.and_then(|t| t.attr("bgcolor")).and_then(color),
    };
    let opacity = match value(decls, "opacity").filter(|v| !inherits(v)) {
        Some(v) => Some(match number(v)? {
            (n, "") => n,
            (n, "%") => n / 100.0,
            _ => return None,
        }),
        None => None,
    };
    let size = value(decls, "font-size").or_else(|| value(decls, "font").and_then(font_size));
    let font_px = match size {
        Some(v) => Some(font_px(v, parent_font)?),
        None => None,
    };
    Some(Own {
        fg,
        bg: bg.filter(|c| alpha(c) > 0.05),
        opacity,
        font_px,
    })
}

/// The color a `background` value paints: `Some(None)` for none the
/// reducer can see (an image, a gradient, `none`), `None` when a part
/// cannot be read.
fn background(v: &str) -> Option<Option<Color>> {
    const WORDS: &str = "none repeat no-repeat repeat-x repeat-y space round scroll fixed local \
        center top bottom left right cover contain auto border-box padding-box content-box text";
    let mut paint = None;
    for part in parts(v) {
        if part.starts_with("url(")
            || part.contains("gradient(")
            || part.contains('/')
            || number(part).is_some()
            || WORDS.split(' ').any(|w| w == part)
            || (inherits(part) && part != "currentcolor")
        {
            continue;
        }
        paint = Some(color(part).filter(|_| part != "currentcolor")?);
    }
    Some(paint)
}

/// The space-separated parts of a value, a function call kept whole.
fn parts(v: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0usize, 0);
    for (i, c) in v.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ' ' if depth == 0 => {
                out.extend((i > start).then(|| &v[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.extend((start < v.len()).then(|| &v[start..]));
    out
}

/// The size in a `font` shorthand: the first part, before any `/`, that
/// is a keyword size or a length with a unit or zero.
fn font_size(v: &str) -> Option<&str> {
    parts(v).into_iter().find_map(|part| {
        let size = part.split('/').next().unwrap_or("");
        let length = number(size).is_some_and(|(n, unit)| !unit.is_empty() || n == 0.0);
        let keyword = number(size).is_none() && font_px(size, 16.0).is_some();
        (length || keyword).then_some(size)
    })
}

/// A `font-size` in px against the parent's.
fn font_px(v: &str, parent: f32) -> Option<f32> {
    Some(match v {
        "xx-small" => 9.0,
        "x-small" => 10.0,
        "small" => 13.0,
        "medium" => 16.0,
        "large" => 18.0,
        "x-large" => 24.0,
        "xx-large" => 32.0,
        "xxx-large" => 48.0,
        "smaller" => parent / 1.2,
        "larger" => parent * 1.2,
        _ if inherits(v) || v == "math" => parent,
        _ => px(v, parent, parent / 100.0)?,
    })
}

fn alpha(c: &Color) -> f32 {
    match c {
        Color::Rgb(_, a) => *a,
        Color::Named(_) => 1.0,
    }
}

fn close(a: &Color, b: &Color) -> bool {
    match (a, b) {
        (Color::Rgb(x, _), Color::Rgb(y, _)) => x.iter().zip(y).all(|(p, q)| p.abs_diff(*q) <= 16),
        (Color::Named(x), Color::Named(y)) => x == y,
        _ => false,
    }
}

/// Text in `paint` cannot be seen.
fn invisible(paint: &Paint) -> bool {
    alpha(&paint.fg) <= 0.05 || close(&paint.fg, &paint.bg)
}

/// A CSS color: hex with or without `#` and with an optional alpha,
/// `rgb()` or `rgba()` with commas or spaces and an optional alpha,
/// `transparent`, or a name. `None` for a function the reducer does not
/// read, such as `hsl()` or `var()`.
fn color(v: &str) -> Option<Color> {
    let v = v.trim().to_ascii_lowercase();
    let bare = matches!(v.len(), 3 | 6) && v.bytes().all(|b| b.is_ascii_hexdigit());
    if let Some(h) = v.strip_prefix('#').or(bare.then_some(v.as_str())) {
        let d: Vec<u8> = h
            .chars()
            .map(|c| c.to_digit(16).and_then(|d| u8::try_from(d).ok()))
            .collect::<Option<_>>()?;
        let ([r, g, b], a) = match d.as_slice() {
            [r, g, b] => ([r * 17, g * 17, b * 17], 255),
            [r, g, b, a] => ([r * 17, g * 17, b * 17], a * 17),
            [r1, r2, g1, g2, b1, b2] => ([r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2], 255),
            [r1, r2, g1, g2, b1, b2, a1, a2] => {
                ([r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2], a1 * 16 + a2)
            }
            _ => return None,
        };
        return Some(Color::Rgb([r, g, b], f32::from(a) / 255.0));
    }
    if let Some(args) = v
        .strip_prefix("rgba(")
        .or_else(|| v.strip_prefix("rgb("))
        .and_then(|a| a.strip_suffix(')'))
    {
        let fraction = |p: &str| match number(p)? {
            (n, "") => Some(n),
            (n, "%") => Some(n / 100.0),
            _ => None,
        };
        let channel = |p: &str| match number(p)? {
            (n, "") => Some(n.clamp(0.0, 255.0) as u8),
            (n, "%") => Some((n * 2.55).clamp(0.0, 255.0) as u8),
            _ => None,
        };
        let parts: Vec<&str> = args
            .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
            .filter(|p| !p.is_empty())
            .collect();
        let (rgb, a) = match parts.as_slice() {
            [r, g, b] => ([*r, *g, *b], 1.0),
            [r, g, b, a] => ([*r, *g, *b], fraction(a)?),
            _ => return None,
        };
        let [r, g, b] = rgb.map(channel);
        return Some(Color::Rgb([r?, g?, b?], a.clamp(0.0, 1.0)));
    }
    if v.contains('(') {
        return None;
    }
    let rgb = match v.as_str() {
        "transparent" => return Some(Color::Rgb([0, 0, 0], 0.0)),
        "white" | "canvas" | "window" => [255, 255, 255],
        "black" | "canvastext" | "windowtext" => [0, 0, 0],
        "snow" => [255, 250, 250],
        "ghostwhite" => [248, 248, 255],
        "whitesmoke" => [245, 245, 245],
        "ivory" => [255, 255, 240],
        "azure" => [240, 255, 255],
        "mintcream" => [245, 255, 250],
        "floralwhite" => [255, 250, 240],
        "honeydew" => [240, 255, 240],
        "aliceblue" => [240, 248, 255],
        "lavenderblush" => [255, 240, 245],
        "seashell" => [255, 245, 238],
        _ => return Some(Color::Named(v)),
    };
    Some(Color::Rgb(rgb, 1.0))
}

impl HiddenRules {
    /// Every `<style>` block in `html`, wherever it sits: a client
    /// applies a sheet to the whole document.
    fn of(html: &str) -> Self {
        let mut rules = Self::default();
        let mut rest = html;
        while let Some(at) = find_ascii_ci(rest, "<style") {
            rest = &rest[at..];
            match parse_tag(rest) {
                Some((tag, after)) if tag.name == "style" && !tag.closing => {
                    let end = find_ascii_ci(after, "</style").unwrap_or(after.len());
                    rules.read(&after[..end]);
                    rest = &after[end..];
                }
                _ => rest = &rest[1..],
            }
        }
        rules
    }

    /// Class and id selectors whose rule hides, and the colors and
    /// backgrounds, from one `<style>` block. What cannot be followed
    /// sets `withheld`.
    fn read(&mut self, css: &str) {
        self.styled = true;
        self.rules(&css_text(&css.to_ascii_lowercase()), 0);
    }

    /// The rules in `css`. Those inside `@media`, `@supports` or any
    /// other block at-rule are read as if they apply; `@import`, or
    /// blocks nested more than 8 deep, withhold.
    fn rules(&mut self, mut css: &str, depth: usize) {
        if depth > 8 {
            self.withheld = true;
            return;
        }
        while let Some((prelude, block, rest)) = next_rule(css) {
            css = rest;
            let mut prelude = prelude.trim();
            while let Some(p) = prelude
                .strip_prefix("<!--")
                .or_else(|| prelude.strip_prefix("-->"))
            {
                prelude = p.trim_start();
            }
            match (prelude.strip_prefix('@'), block) {
                (Some(at), _) if at.starts_with("import") => self.withheld = true,
                (Some(_), Some(block)) => self.rules(block, depth + 1),
                (None, Some(block)) => self.rule(prelude, block),
                (_, None) => {}
            }
        }
    }

    /// One style rule. A rule nested inside it is not followed.
    fn rule(&mut self, selectors: &str, body: &str) {
        if unquoted(body).any(|(_, c)| c == '{') {
            self.withheld = true;
            return;
        }
        let decls = declarations(body);
        let set = own(&decls, None, 16.0);
        if let Some(set) = &set {
            if let Some(fg) = &set.fg {
                self.withheld |= set.bg.is_none() && close(fg, &WHITE);
                self.colors.push(fg.clone());
            }
            if let Some(bg) = &set.bg {
                self.withheld |= set.fg.is_none() && close(bg, &BLACK);
                self.backgrounds.push(bg.clone());
            }
        }
        let conceals = hides(&decls)
            || set.is_none_or(|set| {
                set.opacity.is_some_and(|n| n <= 0.05)
                    || set.font_px.is_some_and(|n| n <= 1.0)
                    || set.fg.is_some_and(|c| alpha(&c) <= 0.05)
            });
        if !conceals {
            return;
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

    fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c.eq_ignore_ascii_case(class))
    }

    fn has_id(&self, id: &str) -> bool {
        self.ids.iter().any(|i| i.eq_ignore_ascii_case(id))
    }
}

/// The first rule or statement in `css`: its prelude, its block if it
/// has one, and the CSS after it. A block the sheet leaves open runs to
/// the end.
fn next_rule(css: &str) -> Option<(&str, Option<&str>, &str)> {
    if css.trim().is_empty() {
        return None;
    }
    let (mut depth, mut open) = (0usize, 0);
    for (i, c) in unquoted(css) {
        match c {
            ';' if depth == 0 => return Some((&css[..i], None, &css[i + 1..])),
            '{' => {
                if depth == 0 {
                    open = i;
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return Some((&css[..open], Some(&css[open + 1..i]), &css[i + 1..]));
                }
            }
            _ => {}
        }
    }
    Some(if depth > 0 {
        (&css[..open], Some(&css[open + 1..]), "")
    } else {
        (css, None, "")
    })
}

/// The characters of `css` outside strings, with their offsets. A
/// string ends at its quote or at a line break.
fn unquoted(css: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let (mut quote, mut escaped) = (None, false);
    css.char_indices().filter(move |&(_, c)| {
        let Some(q) = quote else {
            quote = matches!(c, '"' | '\'').then_some(c);
            return quote.is_none();
        };
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == q || matches!(c, '\n' | '\r' | '\x0c') {
            quote = None;
        }
        false
    })
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
    /// `style`, `class`, `id` or `hidden` held a reference `decode` does
    /// not know, so a client may read it otherwise.
    odd: bool,
    /// Values entity-decoded.
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
                    odd: false,
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
    let (mut self_closing, mut odd) = (false, false);
    loop {
        while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
            chars.next();
        }
        let (at, c) = chars.next()?;
        match c {
            '>' => {
                let tag = Tag {
                    name,
                    closing,
                    self_closing,
                    odd,
                    attrs,
                };
                return Some((tag, &s[1 + at + 1..]));
            }
            '/' => self_closing = matches!(chars.peek(), Some((_, '>'))),
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
                let (value, unknown) = decode_all(&value);
                odd |= unknown && matches!(key.as_str(), "style" | "class" | "id" | "hidden");
                attrs.push((key, value));
            }
        }
    }
}

fn find_ascii_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// The common named entities and every numeric one.
fn decode(s: &str) -> String {
    decode_all(s).0
}

/// `s` decoded, and whether an `&` that starts some other reference was
/// left as it is.
fn decode_all(s: &str) -> (String, bool) {
    let mut out = String::with_capacity(s.len());
    let mut unknown = false;
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let reference = rest[1..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '#');
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            unknown |= reference;
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
                unknown |= reference;
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    (out, unknown)
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
            "<td bgcolor=\"#000\" style=\"color:#fff\">Shown <font color=black>secret</font> end</td>",
        );
        assert_eq!(inherited.text, "Shown end");
    }

    #[test]
    fn the_concealment_the_review_found_is_dropped() {
        for span in [
            "<span style=\"display&#58;none\">",
            "<span style=\"display&#x3a;none\">",
            "<span/style=\"display:none\">",
            "<span style=\"display:none\"/>",
            "<span style=\"display:/**/none\">",
            "<span style=\"display:n\\6f ne\">",
            "<span style=\"opacity:1%\">",
            "<span style=\"opacity:1e-3\">",
            "<span style=\"font-size:0.01em\">",
            "<span style=\"font:0/0 a\">",
            "<span style=\"color:rgba(255,255,255,.0)\">",
            "<span style=\"color:#fff0\">",
            "<span style=\"color:#fefefe\">",
            "<span style=\"-webkit-text-fill-color:transparent\">",
            "<span style=\"transform:translateX(-9999px)\">",
            "<span style=\"position:absolute;left:9999px\">",
            "<span style=\"display:var(--h)\">",
            "<span style=\"color:hsl(0,0%,100%)\">",
            "<span style=\"opacity:.2\"><span style=\"opacity:.2\">",
            "<span style=\"font-size:10%\"><span style=\"font-size:10%\">",
        ] {
            let got = visible_text(&format!("<p>Shown {span}SECRET</span></span> end</p>"));
            assert_eq!(got.text, "Shown end", "{span}");
            assert!(got.hidden_content && !got.withheld, "{span}");
        }
        let got = visible_text("<p>Shown<div/hidden>SECRET</div>end</p>");
        assert!(
            !got.text.contains("SECRET") && got.hidden_content,
            "{got:?}"
        );
    }

    #[test]
    fn styling_that_hides_nothing_keeps_its_text() {
        for span in [
            "<span style=\"color:#333;opacity:.5;font-size:.8em;transform:none\">",
            "<span style=\"background:url(x.png) #06c no-repeat;color:#fff\">",
            "<span style=\"left:-0px;width:auto\">",
        ] {
            let got = visible_text(&format!("<p>Shown {span}text</span> end</p>"));
            assert_eq!(got.text, "Shown text end", "{span}");
            assert!(!got.hidden_content, "{span}");
        }
        let svg = visible_text("<svg><rect style=\"display:none\"/></svg><p>Shown<br/>end</p>");
        assert_eq!(svg.text, "Shown\nend");
    }

    #[test]
    fn what_the_reducer_cannot_decode_or_place_is_withheld() {
        for html in [
            "<p><span style=\"display&colon;none\">SECRET</span></p>",
            "<p><span class=\"&#x78\">SECRET</span></p>",
            "<style>.t{color:#fff;background:#000}</style>\
             <div style=\"background:#fff\"><p class=t>SECRET</p></div>",
            "<style>.b{background:#fff;color:#000}</style>\
             <div class=b><p style=\"color:#fff\">SECRET</p></div>",
            "<style>.t{color:#fff}</style><p class=t>SECRET</p>",
            "<p class=t>SECRET</p><style>.x{color:#000}</style><td bgcolor=black>",
        ] {
            let got = visible_text(html);
            assert!(got.hidden_content && got.withheld, "{html}");
        }
        let late = visible_text("<p class=x>SECRET</p><p>Shown</p><style>.x{display:none}</style>");
        assert_eq!(late.text, "Shown");
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
            "@media screen { div > p { display: none } }",
            "@keyframes k { to { opacity: 0 } }",
            ".a { .x { display: none } }",
            "@import url(https://example.test/x.css);",
            "@media screen { @IMPORT 'x.css'; }",
            ".a { color: #fff } body { background: white }",
        ] {
            let got = visible_text(&format!("<style>{css}</style><p>Hello</p><p>secret</p>"));
            assert!(got.hidden_content && got.withheld, "{css}");
        }
        let linked = visible_text("<link rel=stylesheet href=https://example.test/x.css><p>Hi</p>");
        assert!(linked.withheld);
        let deep = format!(
            "<style>{}.x{{display:none}}</style>",
            "@media screen{".repeat(9)
        );
        assert!(visible_text(&deep).withheld);
    }

    #[test]
    fn at_rules_apply_as_if_they_match() {
        for css in [
            "@media screen { .a { color: red } }",
            ".a::after { content: \"a@b\" }",
            "/* @import url(x.css); */ .a { font-family: '@import' }",
            "@charset \"utf-8\"; @font-face { font-family: f; src: url(f.woff) }",
        ] {
            let got = visible_text(&format!("<style>{css}</style><p class=a>Hello</p>"));
            assert_eq!(got.text, "Hello", "{css}");
            assert!(got.hidden_content && !got.withheld, "{css}");
        }
        let got = visible_text(
            "<style><!-- @media (max-width: 600px) { .x { display: none } }\
             @supports (display: grid) { @media print { #y { font-size: 0 } } } -->\
             .z { content: \"a\n}.w{display:/**/none}</style>\
             <p>Shown</p><p class=x>SECRET</p><p id=y>SECRET</p><p class=w>SECRET</p>",
        );
        assert_eq!(got.text, "Shown");
        assert!(got.hidden_content && !got.withheld, "{got:?}");
    }

    #[test]
    fn a_stray_angle_bracket_is_text() {
        assert_eq!(visible_text("1 < 2 and 3 > 2").text, "1 < 2 and 3 > 2");
    }
}
