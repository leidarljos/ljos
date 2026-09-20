//! Cockpit face: clock, leases, trust canvas, island, deed rail.

use iced::mouse;
use iced::widget::canvas::{self, stroke, Geometry, Path, Stroke};
use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Length, Rectangle, Renderer};
use icedtea::a11y::{A11y, Role};
use icedtea::icon::Icons;
use icedtea::theme::Tokens;
use icedtea::toast::ToastKind;
use icedtea::variant::Variant;
use icedtea::widget;

use crate::app::Message;
use crate::data::{claim_lease_label, ClockState, IslandSnap, Snapshot};
use crate::graph::{GraphLayout, PaintOp};
use crate::theme;

/// Skip chips in layout order. Keys 1–5.
pub const SKIP_CHIPS: [(&str, Pane); 5] = [
    ("due", Pane::Due),
    ("claims", Pane::Claims),
    ("graph", Pane::Trust),
    ("island", Pane::Island),
    ("timeline", Pane::Timeline),
];

/// What the graph pane paints. Pack-down is not honest-empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphInstrument {
    PackDown,
    Empty,
    Canvas,
}

/// Pack-down vs empty-up must differ. A claims banner must not blank an
/// honest-empty graph.
pub fn graph_instrument(snap: &Snapshot) -> GraphInstrument {
    if !snap.pack_ok {
        GraphInstrument::PackDown
    } else if snap.graph.is_empty() {
        GraphInstrument::Empty
    } else {
        GraphInstrument::Canvas
    }
}

const DASH: [f32; 2] = [6.0, 4.0];

/// Which region owns the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Due,
    Claims,
    Trust,
    Island,
    Timeline,
}

impl Pane {
    pub fn next(self) -> Self {
        match self {
            Self::Due => Self::Claims,
            Self::Claims => Self::Trust,
            Self::Trust => Self::Island,
            Self::Island => Self::Timeline,
            Self::Timeline => Self::Due,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Due => Self::Timeline,
            Self::Claims => Self::Due,
            Self::Trust => Self::Claims,
            Self::Island => Self::Trust,
            Self::Timeline => Self::Island,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Due => "due",
            Self::Claims => "claims",
            Self::Trust => "graph",
            Self::Island => "island",
            Self::Timeline => "timeline",
        }
    }
}

/// Paint the cockpit. Skip chips sit in the layout. `cue` is the draft
/// island field; enter activates a read, not a write.
pub fn view<'a>(
    snap: &'a Snapshot,
    pane: Pane,
    selected: usize,
    cue: &'a str,
) -> Element<'a, Message> {
    let tea = theme::tokens();
    let header: Element<'a, Message> = if snap.banner.is_empty() {
        let chrome = text("ljos  due  claims  graph  island  timeline   j/k  h/l  1-5  P  r  q")
            .size(theme::SIZE_META)
            .color(theme::SUBTEXT);
        if snap.review.is_empty() {
            chrome.into()
        } else {
            column![
                chrome,
                text(snap.review.clone())
                    .size(theme::SIZE_META)
                    .color(theme::TEXT),
            ]
            .spacing(2)
            .into()
        }
    } else {
        widget::banner(
            snap.banner.clone(),
            None,
            Some(ToastKind::Warning),
            tea,
            A11y::new("banner", Role::Status),
        )
    };

    column![
        container(header).padding(8).width(Length::Fill),
        container(skip_chips(pane, tea)).padding([0, 8]),
        row![
            due_pane(snap, pane == Pane::Due, selected, tea),
            graph_pane(snap, pane == Pane::Trust, selected, tea),
            claims_pane(snap, pane == Pane::Claims, selected, tea),
        ]
        .spacing(8)
        .padding([0, 8])
        .height(Length::FillPortion(3)),
        row![
            island_pane(snap, pane == Pane::Island, selected, cue, tea),
            timeline_pane(snap, pane == Pane::Timeline, selected, tea),
        ]
        .spacing(8)
        .padding(8)
        .height(Length::FillPortion(2)),
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

fn display_chip(label: &str, tea: Tokens, variant: Variant) -> Element<'static, Message> {
    widget::chip(
        label.to_string(),
        None,
        None,
        tea,
        variant,
        widget::ChipKind::Assist,
        Icons::NONE,
        A11y::new(label.to_string(), Role::Status),
    )
}

fn due_pane<'a>(
    snap: &'a Snapshot,
    active: bool,
    selected: usize,
    tea: Tokens,
) -> Element<'a, Message> {
    let title_color = if active { theme::BLUE } else { theme::SUBTEXT };
    let mut col = column![text("due").size(theme::SIZE_TITLE).color(title_color)].spacing(4);
    col = col.push(
        row![
            display_chip(
                &format!("{} unreviewed", snap.clock.unreviewed),
                tea,
                Variant::Chip,
            ),
            display_chip(&format!("{} due", snap.clock.due), tea, Variant::Primary),
            display_chip(
                &format!("{} overdue", snap.clock.overdue),
                tea,
                Variant::Chip,
            ),
        ]
        .spacing(4),
    );
    if snap.due.is_empty() {
        col = col.push(text("(none)").size(theme::SIZE_META).color(theme::SUBTEXT));
    } else {
        for (i, r) in snap.due.iter().enumerate() {
            let focused = active && i == selected;
            let color = if focused { theme::TEXT } else { theme::SUBTEXT };
            let prefix = if focused { "▸ " } else { "  " };
            let when = if r.due_at.is_empty() {
                "-"
            } else {
                r.due_at.as_str()
            };
            let clock_var = match r.clock {
                ClockState::Overdue => Variant::Chip,
                ClockState::Due => Variant::Primary,
                _ => Variant::Chip,
            };
            col = col.push(
                column![
                    row![
                        display_chip(r.clock.as_str(), tea, clock_var),
                        display_chip("recalled", tea, Variant::Chip),
                        display_chip("lapsed", tea, Variant::Chip),
                    ]
                    .spacing(4),
                    text(format!(
                        "{prefix}{}  {}  {}  {}",
                        r.kind, r.id, when, r.text
                    ))
                    .size(theme::SIZE_BODY)
                    .color(color),
                ]
                .spacing(2),
            );
        }
    }
    pane_box(scrollable(col).height(Length::Fill), active, 1)
}

fn claims_pane<'a>(
    snap: &'a Snapshot,
    active: bool,
    selected: usize,
    _tea: Tokens,
) -> Element<'a, Message> {
    let title_color = if active { theme::BLUE } else { theme::SUBTEXT };
    let mut col = column![text("claims").size(theme::SIZE_TITLE).color(title_color)].spacing(4);
    if snap.claims.is_empty() {
        col = col.push(text("(none)").size(theme::SIZE_META).color(theme::SUBTEXT));
    } else {
        for (i, r) in snap.claims.iter().enumerate() {
            let focused = active && i == selected;
            let color = if focused { theme::TEXT } else { theme::SUBTEXT };
            let prefix = if focused { "▸ " } else { "  " };
            let lease = claim_lease_label(r);
            col = col.push(
                text(format!(
                    "{prefix}{}  {}  as {}  gen {}  {}  {}  {}",
                    r.status, r.id, r.assignee, r.cas_gen, r.occupancy, lease, r.summary
                ))
                .size(theme::SIZE_BODY)
                .color(color),
            );
        }
    }
    pane_box(scrollable(col).height(Length::Fill), active, 1)
}

fn island_pane<'a>(
    snap: &'a Snapshot,
    active: bool,
    selected: usize,
    cue: &'a str,
    tea: Tokens,
) -> Element<'a, Message> {
    let title_color = if active { theme::BLUE } else { theme::SUBTEXT };
    let mut col = column![text("island").size(theme::SIZE_TITLE).color(title_color)].spacing(4);
    col = col.push(widget::text_input(
        "cue: enter activates a read",
        cue,
        Message::CueChanged,
        Some(Message::CueActivate),
        widget::FieldOpts::NONE,
        tea,
        A11y::new("island-cue", Role::TextBox),
        None,
    ));
    col = col.push(island_banners(&snap.island, tea));
    if snap.island.hits.is_empty() && snap.island.rows.is_empty() {
        col = col.push(
            text("type a cue, enter to activate")
                .size(theme::SIZE_META)
                .color(theme::SUBTEXT),
        );
    } else {
        for (i, h) in snap.island.hits.iter().enumerate() {
            let focused = active && i == selected;
            let color = if focused { theme::TEXT } else { theme::SUBTEXT };
            let prefix = if focused { "▸ " } else { "  " };
            col = col.push(
                text(format!(
                    "{prefix}{:.3}  {}  {}  {}",
                    h.score, h.kind, h.id, h.text
                ))
                .size(theme::SIZE_BODY)
                .color(color),
            );
        }
        let hit_n = snap.island.hits.len();
        for (i, r) in snap.island.rows.iter().enumerate() {
            let focused = active && i + hit_n == selected;
            let color = if focused { theme::TEXT } else { theme::SUBTEXT };
            let prefix = if focused { "▸ " } else { "  " };
            let seed = if r.seed { "seed" } else { "    " };
            col = col.push(
                text(format!(
                    "{prefix}{:.3}  {seed}  {}  {}  {}",
                    r.activation, r.kind, r.id, r.text
                ))
                .size(theme::SIZE_BODY)
                .color(color),
            );
        }
    }
    pane_box(scrollable(col).height(Length::Fill), active, 1)
}

fn island_banners(island: &IslandSnap, tea: Tokens) -> Element<'static, Message> {
    let mut col = column![].spacing(4);
    if island.weak {
        col = col.push(widget::banner(
            "weak island: seeds two scorers did not agree on; read it as the pack's best-connected cluster, not as what the cue is about; it will not fire",
            None,
            Some(ToastKind::Warning),
            tea,
            A11y::new("island-weak", Role::Status),
        ));
    }
    if !island.dense {
        col = col.push(widget::banner(
            "encoder is down; ranking is lexical only",
            None,
            Some(ToastKind::Warning),
            tea,
            A11y::new("island-dense", Role::Status),
        ));
    }
    if !island.banner.is_empty() {
        col = col.push(widget::banner(
            island.banner.clone(),
            None,
            Some(ToastKind::Warning),
            tea,
            A11y::new("island-banner", Role::Status),
        ));
    }
    col.into()
}

fn timeline_pane<'a>(
    snap: &'a Snapshot,
    active: bool,
    selected: usize,
    _tea: Tokens,
) -> Element<'a, Message> {
    let title_color = if active { theme::BLUE } else { theme::SUBTEXT };
    let title = if snap.timeline_issue.is_empty() {
        "timeline".to_string()
    } else {
        format!("timeline  {}", snap.timeline_issue)
    };
    let mut col = column![text(title).size(theme::SIZE_TITLE).color(title_color)].spacing(4);
    if !snap.timeline_banner.is_empty() {
        col = col.push(
            text(snap.timeline_banner.clone())
                .size(theme::SIZE_META)
                .color(theme::PEACH),
        );
    }
    if snap.timeline.is_empty() {
        col = col.push(
            text("(activate an issue cue, or hold a live claim)")
                .size(theme::SIZE_META)
                .color(theme::SUBTEXT),
        );
    } else {
        for (i, e) in snap.timeline.iter().enumerate() {
            let focused = active && i == selected;
            let color = if focused { theme::TEXT } else { theme::SUBTEXT };
            let prefix = if focused { "▸ " } else { "  " };
            col = col.push(
                text(format!("{prefix}{}  {}  {}", e.source, e.clock, e.text))
                    .size(theme::SIZE_BODY)
                    .color(color),
            );
        }
    }
    pane_box(scrollable(col).height(Length::Fill), active, 1)
}

fn pane_box<'a>(
    body: impl Into<Element<'a, Message>>,
    active: bool,
    portion: u16,
) -> Element<'a, Message> {
    container(body)
        .padding(8)
        .width(Length::FillPortion(portion))
        .height(Length::Fill)
        .style(if active {
            container::bordered_box
        } else {
            container::rounded_box
        })
        .into()
}

fn graph_pane<'a>(
    snap: &'a Snapshot,
    focused: bool,
    selected: usize,
    tea: Tokens,
) -> Element<'a, Message> {
    let title_color = if focused { theme::BLUE } else { theme::SUBTEXT };
    let title = text("graph").size(theme::SIZE_TITLE).color(title_color);
    let body: Element<'a, Message> = match graph_instrument(snap) {
        GraphInstrument::PackDown => icedtea::pattern::status_page(
            "Pack is down",
            if snap.banner.is_empty() {
                "the pack did not answer".to_string()
            } else {
                snap.banner.clone()
            },
            None,
            tea,
            A11y::new("pack-down", Role::Status),
        ),
        GraphInstrument::Empty => icedtea::pattern::status_page(
            "No trust graph",
            "This pack has no personas and no trust rows.",
            None,
            tea,
            A11y::new("graph-empty", Role::Status),
        ),
        GraphInstrument::Canvas => {
            let canvas_el = iced::widget::canvas(TrustCanvas {
                layout: &snap.graph,
                selected,
                focused,
            })
            .width(Length::Fill)
            .height(Length::Fill);
            let readout = snap.graph.readout(selected);
            column![
                canvas_el,
                text(readout).size(theme::SIZE_META).color(if focused {
                    theme::TEXT
                } else {
                    theme::SUBTEXT
                })
            ]
            .spacing(4)
            .height(Length::Fill)
            .into()
        }
    };
    container(column![title, body].spacing(4).height(Length::Fill))
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
        PaintOp::Node { at, r, hollow } => {
            let circle = Path::circle(pt(*at), *r);
            if *hollow {
                frame.stroke(
                    &circle,
                    Stroke::default().with_width(1.5).with_color(theme::TEXT),
                );
            } else {
                frame.fill(&circle, theme::BLUE);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphLayout;
    use ljos_cli::{Persona, Trust};

    #[test]
    fn pane_cycles() {
        assert_eq!(Pane::Due.next(), Pane::Claims);
        assert_eq!(Pane::Trust.next(), Pane::Island);
        assert_eq!(Pane::Timeline.next(), Pane::Due);
        assert_eq!(Pane::Due.prev(), Pane::Timeline);
    }

    #[test]
    fn skip_chips_live_in_layout() {
        let src = include_str!("view.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("widget::chip"));
        assert!(prod.contains("skip to"));
        assert!(prod.contains("SKIP_CHIPS"));
        assert_eq!(SKIP_CHIPS[0], ("due", Pane::Due));
        assert_eq!(SKIP_CHIPS[1], ("claims", Pane::Claims));
        assert_eq!(SKIP_CHIPS[2], ("graph", Pane::Trust));
        assert_eq!(SKIP_CHIPS[3], ("island", Pane::Island));
        assert_eq!(SKIP_CHIPS[4], ("timeline", Pane::Timeline));
        assert!(prod.contains("canvas("));
        assert!(prod.contains("widget::banner"));
        assert!(prod.contains("status_page"));
        assert!(prod.contains("widget::text_input"));
        assert!(prod.contains("CueActivate"));
        assert!(prod.contains("\"recalled\""));
        assert!(prod.contains("\"lapsed\""));
        assert!(
            !prod.contains(" → "),
            "trust pane must not be a from→to column"
        );
        assert!(!prod.contains("graded("));
        assert!(!prod.contains("fire: true"));
    }

    #[test]
    fn grade_chips_are_display_only() {
        let src = include_str!("view.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("display_chip(\"recalled\""));
        assert!(prod.contains("display_chip(\"lapsed\""));
        assert!(prod.contains("fn display_chip"));
        let chip = prod
            .split("fn display_chip")
            .nth(1)
            .unwrap()
            .split("fn due_pane")
            .next()
            .unwrap();
        assert!(chip.contains("None,"), "display chips send no message");
        assert!(!chip.contains("Some("), "display chips send no message");
    }

    #[test]
    fn habitat_down_canvas_stays_empty() {
        let snap = Snapshot::banner_only("pack: down".into());
        assert!(!snap.pack_ok);
        assert!(snap.graph.is_empty());
        assert!(snap.graph.paint_ops(0, true, 640.0, 480.0).is_empty());
        assert_eq!(graph_instrument(&snap), GraphInstrument::PackDown);
    }

    #[test]
    fn pack_down_and_honest_empty_differ() {
        let down = Snapshot::banner_only("pack: down".into());
        assert_eq!(graph_instrument(&down), GraphInstrument::PackDown);

        let boot = Snapshot::banner_only(String::new());
        assert!(boot.pack_ok);
        assert_eq!(graph_instrument(&boot), GraphInstrument::Empty);

        let mut claims_banner = Snapshot::banner_only(String::new());
        claims_banner.banner = "claims: down".into();
        assert_eq!(
            graph_instrument(&claims_banner),
            GraphInstrument::Empty,
            "a claims banner must not blank an honest-empty graph"
        );
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
