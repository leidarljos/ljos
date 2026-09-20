//! Three columns: due, claims, trust.

use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Length};

use crate::data::Snapshot;
use crate::theme;

/// Which column owns the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Due,
    Claims,
    Trust,
}

impl Pane {
    pub fn next(self) -> Self {
        match self {
            Self::Due => Self::Claims,
            Self::Claims => Self::Trust,
            Self::Trust => Self::Due,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Due => Self::Trust,
            Self::Claims => Self::Due,
            Self::Trust => Self::Claims,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Due => "due",
            Self::Claims => "claims",
            Self::Trust => "trust",
        }
    }
}

/// Paint the three columns. The active pane is highlighted.
pub fn view<'a>(
    snap: &'a Snapshot,
    pane: Pane,
    selected: usize,
) -> Element<'a, crate::app::Message> {
    let header = text(if snap.banner.is_empty() {
        "ljos  due  claims  trust   j/k  h/l  1 2 3  P  r  q".into()
    } else {
        format!("ljos  {}", snap.banner)
    })
    .size(theme::SIZE_META)
    .color(theme::SUBTEXT);

    let due = column_pane(
        "due",
        pane == Pane::Due,
        selected,
        snap.due
            .iter()
            .map(|r| format!("{}  {}  {}", r.kind, r.id, r.text)),
    );
    let claims = column_pane(
        "claims",
        pane == Pane::Claims,
        selected,
        snap.claims
            .iter()
            .map(|r| format!("{}  {}  {}", r.status, r.id, r.summary)),
    );
    let trust = column_pane(
        "trust",
        pane == Pane::Trust,
        selected,
        snap.trust.iter().map(|r| {
            format!(
                "{} → {}  {:.3}{}",
                r.from,
                r.to,
                r.weight,
                if r.about.is_empty() {
                    String::new()
                } else {
                    format!("  {}", r.about)
                }
            )
        }),
    );

    column![
        container(header).padding(8).width(Length::Fill),
        row![due, claims, trust]
            .spacing(8)
            .padding(8)
            .height(Length::Fill),
    ]
    .into()
}

fn column_pane<'a, I, S>(
    title: &'static str,
    active: bool,
    selected: usize,
    rows: I,
) -> Element<'a, crate::app::Message>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    let title_color = if active { theme::BLUE } else { theme::SUBTEXT };
    let mut col = column![text(title).size(theme::SIZE_TITLE).color(title_color)].spacing(4);
    let mut empty = true;
    for (i, line) in rows.enumerate() {
        empty = false;
        let color = if active && i == selected {
            theme::TEXT
        } else {
            theme::SUBTEXT
        };
        let prefix = if active && i == selected {
            "▸ "
        } else {
            "  "
        };
        col = col.push(
            text(format!("{prefix}{}", line.as_ref()))
                .size(theme::SIZE_BODY)
                .color(color),
        );
    }
    if empty {
        col = col.push(text("(none)").size(theme::SIZE_META).color(theme::SUBTEXT));
    }
    let body = scrollable(col).height(Length::Fill);
    container(body)
        .padding(8)
        .width(Length::Fill)
        .style(if active {
            container::bordered_box
        } else {
            container::rounded_box
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_cycles() {
        assert_eq!(Pane::Due.next(), Pane::Claims);
        assert_eq!(Pane::Trust.next(), Pane::Due);
        assert_eq!(Pane::Due.prev(), Pane::Trust);
    }
}
