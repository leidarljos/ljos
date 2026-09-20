//! Cockpit face: due and claims columns around a trust-graph canvas.

use iced::mouse;
use iced::widget::canvas::{self, stroke, Geometry, Path, Stroke};
use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Length, Rectangle, Renderer};
use icedtea::a11y::A11y;
use icedtea::icon::Icons;
use icedtea::theme::Tokens;
use icedtea::variant::Variant;
use icedtea::widget;

use crate::app::Message;
use crate::data::Snapshot;
use crate::graph::{GraphLayout, PaintOp};
use crate::theme;

/// Skip chips in layout order. Always visible; not `:focus-visible`-only.
pub const SKIP_CHIPS: [(&str, Pane); 3] = [
    ("graph", Pane::Trust),
    ("due", Pane::Due),
    ("claims", Pane::Claims),
];

const DASH: [f32; 2] = [6.0, 4.0];

/// Which region owns the cursor.
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

/// Paint due, the trust canvas, and claims. Skip chips sit in the layout.
pub fn view<'a>(snap: &'a Snapshot, pane: Pane, selected: usize) -> Element<'a, Message> {
    let tea = theme::tokens();
    let header = if snap.banner.is_empty() {
        text("ljos  graph  due  claims   j/k  h/l  1 2 3  P  r  q")
            .size(theme::SIZE_META)
            .color(theme::SUBTEXT)
    } else {
        text(format!("ljos  {}", snap.banner))
            .size(theme::SIZE_META)
            .color(theme::PEACH)
    };

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

    column![
        container(header).padding(8).width(Length::Fill),
        container(skip_chips(pane, tea)).padding([0, 8]),
        row![due, graph_pane(snap, pane == Pane::Trust, selected), claims,]
            .spacing(8)
            .padding(8)
            .height(Length::Fill),
    ]
    .into()
}

fn skip_chips(pane: Pane, tea: Tokens) -> Element<'static, Message> {
    let mut row = row![].spacing(6);
    for (label, target) in SKIP_CHIPS {
        row = row.push(skip_chip(label, pane == target, Message::Pane(target), tea));
    }
    row.into()
}

fn skip_chip(label: &str, active: bool, msg: Message, tea: Tokens) -> Element<'static, Message> {
    let title = label.to_string();
    widget::chip(
        title.clone(),
        Some(msg),
        None,
        tea,
        if active {
            Variant::Primary
        } else {
            Variant::Chip
        },
        widget::ChipKind::Assist,
        Icons::NONE,
        A11y::button(format!("skip to {label}")),
    )
}

fn graph_pane(snap: &Snapshot, focused: bool, selected: usize) -> Element<'_, Message> {
    let title_color = if focused { theme::BLUE } else { theme::SUBTEXT };
    let mut col = column![text("graph").size(theme::SIZE_TITLE).color(title_color)].spacing(4);
    if snap.graph.is_empty() {
        if snap.banner.is_empty() {
            col = col.push(text("(none)").size(theme::SIZE_META).color(theme::SUBTEXT));
        }
    } else {
        let canvas_el = iced::widget::canvas(TrustCanvas {
            layout: &snap.graph,
            selected,
            focused,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        col = col.push(canvas_el);
        let readout = snap.graph.readout(selected);
        col = col.push(text(readout).size(theme::SIZE_META).color(if focused {
            theme::TEXT
        } else {
            theme::SUBTEXT
        }));
    }
    container(col)
        .padding(8)
        .width(Length::FillPortion(2))
        .height(Length::Fill)
        .style(if focused {
            container::bordered_box
        } else {
            container::rounded_box
        })
        .into()
}

struct TrustCanvas<'a> {
    layout: &'a GraphLayout,
    selected: usize,
    focused: bool,
}

impl canvas::Program<Message> for TrustCanvas<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for op in self
            .layout
            .paint_ops(self.selected, self.focused, bounds.width, bounds.height)
        {
            paint(&mut frame, &op);
        }
        vec![frame.into_geometry()]
    }
}

fn paint(frame: &mut canvas::Frame, op: &PaintOp) {
    match op {
        PaintOp::Edge {
            from,
            to,
            width,
            dashed,
        } => {
            let line = Path::line(pt(*from), pt(*to));
            let dash = if *dashed {
                stroke::LineDash {
                    segments: &DASH,
                    offset: 0,
                }
            } else {
                stroke::LineDash::default()
            };
            frame.stroke(
                &line,
                Stroke {
                    style: stroke::Style::Solid(if *dashed {
                        theme::PEACH
                    } else {
                        theme::SUBTEXT
                    }),
                    width: *width,
                    line_cap: stroke::LineCap::Round,
                    line_join: stroke::LineJoin::Round,
                    line_dash: dash,
                },
            );
        }
        PaintOp::Node { at, r, persona } => {
            let circle = Path::circle(pt(*at), *r);
            frame.fill(
                &circle,
                if *persona {
                    theme::BLUE
                } else {
                    theme::SURFACE0
                },
            );
        }
        PaintOp::Ring { at, r, width } => {
            let circle = Path::circle(pt(*at), *r);
            frame.stroke(
                &circle,
                Stroke::default()
                    .with_width(*width)
                    .with_color(theme::GREEN),
            );
        }
        PaintOp::Focus { at, r, width } => {
            let circle = Path::circle(pt(*at), *r);
            frame.stroke(
                &circle,
                Stroke::default().with_width(*width).with_color(theme::BLUE),
            );
        }
        PaintOp::Label { at, text } => {
            frame.fill_text(canvas::Text {
                content: text.clone(),
                position: pt(*at),
                color: theme::TEXT,
                size: iced::Pixels::from(theme::SIZE_META),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Top,
                ..canvas::Text::default()
            });
        }
    }
}

fn pt(xy: (f32, f32)) -> iced::Point {
    iced::Point::new(xy.0, xy.1)
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
        .width(Length::FillPortion(1))
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
    use crate::graph::GraphLayout;
    use ljos_cli::{Persona, Trust};

    #[test]
    fn pane_cycles() {
        assert_eq!(Pane::Due.next(), Pane::Claims);
        assert_eq!(Pane::Trust.next(), Pane::Due);
        assert_eq!(Pane::Due.prev(), Pane::Trust);
    }

    #[test]
    fn skip_chips_live_in_layout() {
        let src = include_str!("view.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("widget::chip"));
        assert!(prod.contains("skip to"));
        assert!(prod.contains("SKIP_CHIPS"));
        assert_eq!(SKIP_CHIPS[0], ("graph", Pane::Trust));
        assert!(prod.contains("canvas("));
        assert!(
            !prod.contains(" → "),
            "trust pane must not be a from→to column"
        );
    }

    #[test]
    fn habitat_down_canvas_stays_empty() {
        let snap = Snapshot::banner_only("pack: down".into());
        assert!(snap.graph.is_empty());
        assert!(snap.graph.paint_ops(0, true, 640.0, 480.0).is_empty());
    }

    #[test]
    fn graph_pane_readout_follows_the_walk() {
        let layout = GraphLayout::from_pack(
            &[Persona {
                name: "seat-cockpit".into(),
                anchor: 0.4,
                view: "graph".into(),
                entities: vec![],
            }],
            &[Trust {
                from: "seat-cockpit".into(),
                to: "library-api-hud".into(),
                weight: 0.25,
                about: vec!["hud".into()],
            }],
        );
        assert!(layout.readout(0).contains("seat-cockpit"));
        assert!(layout.readout(0).contains("library-api-hud"));
        let selected = layout.walk_next(0);
        assert_eq!(selected, 1);
        assert!(layout.readout(selected).contains("library-api-hud"));
    }
}
