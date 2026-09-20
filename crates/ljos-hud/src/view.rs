//! Cockpit: due and claims as rails, trust as an iced canvas.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::{column, container, row, scrollable, text, Canvas};
use iced::{Color, Element, Length, Padding, Point, Rectangle, Renderer};

use icedtea::a11y::{A11y, Role};
use icedtea::icon::Icons;
use icedtea::toast::ToastKind;
use icedtea::variant::Variant;
use icedtea::widget;

use crate::app::Message;
use crate::data::Snapshot;
use crate::graph::TrustGraph;
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
            Self::Trust => "graph",
        }
    }
}

/// Paint skip chips, rails, and the trust canvas. The active pane is highlighted.
pub fn view<'a>(snap: &'a Snapshot, pane: Pane, selected: usize) -> Element<'a, Message> {
    let tea = theme::tokens();
    let skip = skip_chips(pane, tea);
    let header = if snap.banner.is_empty() {
        widget::label(
            "ljos  due  claims  graph   j/k  h/l  1 2 3  P  r  q",
            widget::LabelFace::Meta,
            tea,
            A11y::new("hints", Role::Status),
        )
    } else {
        widget::banner(
            snap.banner.clone(),
            None,
            Some(ToastKind::Warning),
            tea,
            A11y::new("banner", Role::Status),
        )
    };

    let due = column_pane(
        "due",
        pane == Pane::Due,
        selected,
        snap.due.iter().map(|r| {
            let when = if r.due_at.is_empty() {
                "-"
            } else {
                r.due_at.as_str()
            };
            format!("{}  {}  {}  {}", r.kind, r.id, when, r.text)
        }),
    );
    let claims = column_pane(
        "claims",
        pane == Pane::Claims,
        selected,
        snap.claims
            .iter()
            .map(|r| format!("{}  {}  {}", r.status, r.id, r.summary)),
    );
    let graph_pane = trust_pane(snap, pane, selected, tea);

    column![
        skip,
        container(header).padding(8).width(Length::Fill),
        row![due, graph_pane, claims]
            .spacing(8)
            .padding(8)
            .height(Length::Fill),
    ]
    .into()
}

/// Skip chips sit in the layout (y >= 0).
fn skip_chips(pane: Pane, tea: icedtea::theme::Tokens) -> Element<'static, Message> {
    let chip = |p: Pane| {
        widget::chip(
            p.label().to_string(),
            Some(Message::Skip(p)),
            None,
            tea,
            if pane == p {
                Variant::Primary
            } else {
                Variant::Chip
            },
            widget::ChipKind::Filter,
            Icons::NONE,
            A11y::button(p.label()),
        )
    };
    row![
        widget::label(
            "skip",
            widget::LabelFace::Meta,
            tea,
            A11y::new("skip", Role::Status),
        ),
        chip(Pane::Due),
        chip(Pane::Claims),
        chip(Pane::Trust),
    ]
    .spacing(6)
    .padding(Padding {
        top: 8.0,
        right: 8.0,
        bottom: 0.0,
        left: 8.0,
    })
    .into()
}

fn trust_pane<'a>(
    snap: &'a Snapshot,
    pane: Pane,
    selected: usize,
    tea: icedtea::theme::Tokens,
) -> Element<'a, Message> {
    let active = pane == Pane::Trust;
    let title_color = if active { theme::BLUE } else { theme::SUBTEXT };
    let title = text("graph").size(theme::SIZE_TITLE).color(title_color);

    let body: Element<'a, Message> = if !snap.pack_ok {
        icedtea::pattern::status_page(
            "Pack is down",
            if snap.banner.is_empty() {
                "the pack did not answer".to_string()
            } else {
                snap.banner.clone()
            },
            None,
            tea,
            A11y::new("pack-down", Role::Status),
        )
    } else if snap.graph.nodes.is_empty() {
        icedtea::pattern::status_page(
            "No trust graph",
            "This pack has no personas and no trust rows.",
            None,
            tea,
            A11y::new("graph-empty", Role::Status),
        )
    } else {
        let canvas: Element<'a, Message> = Canvas::new(GraphCanvas {
            graph: &snap.graph,
            focused: selected,
            ring: active,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
        let framed = if active {
            icedtea::focus::target(canvas, tea, true)
        } else {
            canvas
        };
        let readout = text(snap.graph.readout(selected))
            .size(theme::SIZE_META)
            .color(theme::TEXT);
        column![framed, readout]
            .spacing(4)
            .height(Length::Fill)
            .into()
    };

    let col = column![title, body].spacing(4).height(Length::Fill);
    container(col)
        .padding(8)
        .width(Length::FillPortion(2))
        .style(if active {
            container::bordered_box
        } else {
            container::rounded_box
        })
        .into()
}

fn column_pane<'a, I, S>(
    title: &'static str,
    active: bool,
    selected: usize,
    rows: I,
) -> Element<'a, Message>
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

/// Paints stored layout. Does not run the spring.
struct GraphCanvas<'a> {
    graph: &'a TrustGraph,
    focused: usize,
    ring: bool,
}

impl canvas::Program<Message> for GraphCanvas<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width.max(1.0);
        let h = bounds.height.max(1.0);
        let pad = 28.0_f32.min(w.min(h) * 0.12);
        let map = |x: f32, y: f32| Point::new(pad + x * (w - 2.0 * pad), pad + y * (h - 2.0 * pad));

        if self.ring {
            let focus = Path::rectangle(
                Point::new(2.0, 2.0),
                iced::Size::new((w - 4.0).max(0.0), (h - 4.0).max(0.0)),
            );
            frame.stroke(
                &focus,
                Stroke::default().with_width(3.0).with_color(theme::BLUE),
            );
        }

        for e in &self.graph.edges {
            let a = &self.graph.nodes[e.from_idx];
            let b = &self.graph.nodes[e.to_idx];
            let path = Path::line(map(a.x, a.y), map(b.x, b.y));
            let color = Color {
                a: e.stroke_alpha(),
                ..theme::BLUE
            };
            frame.stroke(
                &path,
                Stroke {
                    width: e.stroke_width(),
                    line_dash: canvas::LineDash {
                        segments: e.dash(),
                        offset: 0,
                    },
                    ..Stroke::default().with_color(color)
                },
            );
        }

        for (i, n) in self.graph.nodes.iter().enumerate() {
            let p = map(n.x, n.y);
            let r = 10.0;
            let ring = Path::circle(p, r);
            if !n.hollow() {
                frame.fill(&ring, theme::SURFACE0);
            }
            frame.stroke(
                &ring,
                Stroke::default()
                    .with_width(n.ring_width())
                    .with_color(if i == self.focused {
                        theme::BLUE
                    } else {
                        theme::TEXT
                    }),
            );
            if i == self.focused {
                let outer = Path::circle(p, r + 5.0);
                frame.stroke(
                    &outer,
                    Stroke::default().with_width(2.0).with_color(theme::BLUE),
                );
            }
            frame.fill_text(canvas::Text {
                content: n.name.clone(),
                position: Point::new(p.x, p.y + r + 2.0),
                color: theme::TEXT,
                size: iced::Pixels(12.0),
                ..canvas::Text::default()
            });
        }

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_cycles() {
        assert_eq!(Pane::Due.next(), Pane::Claims);
        assert_eq!(Pane::Trust.next(), Pane::Due);
        assert_eq!(Pane::Due.prev(), Pane::Trust);
        assert_eq!(Pane::Trust.label(), "graph");
    }

    #[test]
    fn skip_chips_sit_in_layout() {
        let src = include_str!("view.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("skip_chips"));
        assert!(prod.contains("widget::chip"));
        assert!(prod.contains("top: 8.0"));
        assert!(prod.contains("bottom: 0.0"));
        assert!(
            !prod.contains("y: -"),
            "skip chips must not sit at negative y"
        );
        assert!(!prod.contains("-999"));
    }

    #[test]
    fn trust_is_a_canvas_not_a_column() {
        let src = include_str!("view.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("Canvas::new"));
        assert!(prod.contains("GraphCanvas"));
        assert!(prod.contains("status_page"));
        assert!(prod.contains("focus::target"));
        assert!(!prod.contains("layout_fr"), "layout is not a paint step");
        assert!(
            !prod.contains("from → to  {:.3}"),
            "a from→to column is not a graph"
        );
    }
}
