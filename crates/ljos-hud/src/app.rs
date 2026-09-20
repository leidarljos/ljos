//! iced daemon: overlay Boot, pop-out Boot, summon, guest activation.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use iced::event::{self, Event};
use iced::keyboard::{self, key::Named, Key};
use iced::window;
use iced::{time, Element, Font, Pixels, Subscription, Task};

use crate::data::Snapshot;
use crate::summon::{self, SummonAction, SummonRequest};
use crate::theme;
use crate::view::{self, Pane};

const HUD_W: f32 = 1100.0;
const HUD_H: f32 = 720.0;

/// The app id of the pop-out, so a compositor rule on the overlay's id
/// leaves it a normal window.
pub const POPOUT_APP_ID: &str = "me.rgoswami.ljos-hud.window";

/// iced messages.
#[derive(Debug, Clone)]
pub enum Message {
    /// Native key after mapping.
    Key(Key),
    /// 50 ms poll: summon, tray quit, Sway place.
    Tick,
    /// Off-thread habitat refresh.
    Refresh,
    /// Snapshot produced off the UI thread.
    Loaded(Snapshot),
    /// Latest iced window id, once the shell reports one.
    WindowId(Option<window::Id>),
    /// Window close request: hide; the daemon stays.
    Close(window::Id),
    /// Compositor deleted the surface.
    Closed(window::Id),
    /// Guest xdg-activation result. True clears the pending token.
    ActivationApplied(bool),
    /// Retry guest activation; iced often has no Wayland handles on attempt 0.
    ActivateRetry(u8),
}

/// iced application state.
#[derive(Debug)]
pub struct HudApp {
    snap: Snapshot,
    pane: Pane,
    selected: usize,
    help: bool,
    visible: bool,
    window_id: Option<window::Id>,
    opening: Option<window::Id>,
    popout_id: Option<window::Id>,
    opening_popout: Option<window::Id>,
    pending_token: Option<String>,
    tray_quit: Option<Arc<AtomicBool>>,
    place_tries: u8,
}

impl HudApp {
    fn new(visible: bool) -> Self {
        Self {
            snap: Snapshot::load(),
            pane: Pane::Due,
            selected: 0,
            help: false,
            visible,
            window_id: None,
            opening: None,
            popout_id: None,
            opening_popout: None,
            pending_token: if visible {
                summon::take_env_token()
            } else {
                None
            },
            tray_quit: None,
            place_tries: 0,
        }
    }

    fn clamp_selected(&mut self) {
        let n = match self.pane {
            Pane::Due => self.snap.due.len(),
            Pane::Claims => self.snap.claims.len(),
            Pane::Trust => self.snap.trust.len(),
        };
        if n == 0 {
            self.selected = 0;
        } else if self.selected >= n {
            self.selected = n - 1;
        }
    }

    fn mapped(&self) -> bool {
        self.window_id.is_some()
    }

    fn hide(&mut self) {
        self.visible = false;
        self.pending_token = None;
        self.help = false;
    }

    fn show(&mut self) {
        self.visible = true;
    }

    fn apply_summon(&mut self, req: SummonRequest) {
        if req.action != SummonAction::Hide {
            self.pending_token = req.token;
        } else {
            self.pending_token = None;
        }
        match req.action {
            SummonAction::Show => self.show(),
            SummonAction::Hide => self.hide(),
            SummonAction::Toggle => {
                if self.visible {
                    self.hide();
                } else {
                    self.show();
                }
            }
        }
    }

    fn try_place(&mut self) {
        if self.place_tries == 0 {
            return;
        }
        if crate::place::place_overlay() || !crate::place::sway_available() {
            self.place_tries = 0;
        } else {
            self.place_tries = self.place_tries.saturating_sub(1);
        }
    }

    /// Guest activate when a token is pending. Token-less toggle does not
    /// steal the keyboard.
    fn try_activate(&self, attempt: u8) -> Task<Message> {
        let Some(id) = self.window_id else {
            return Task::none();
        };
        let Some(tok) = self.pending_token.clone() else {
            return Task::none();
        };
        let activate = window::run(id, move |win| crate::wlactivate::activate(win, &tok))
            .map(Message::ActivationApplied);
        if attempt < 6 {
            let wait_ms = if attempt == 0 {
                30
            } else {
                40 + u64::from(attempt) * 20
            };
            let wait = Task::perform(
                async move {
                    tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                },
                move |_| Message::ActivateRetry(attempt.saturating_add(1)),
            );
            Task::batch([activate, wait])
        } else {
            activate
        }
    }

    fn open_overlay(&mut self) -> Task<Message> {
        self.place_tries = 20;
        let (id, open) = window::open(overlay_window());
        self.window_id = Some(id);
        self.opening = Some(id);
        open.map(|id| Message::WindowId(Some(id)))
    }

    fn pop_out(&mut self) -> Task<Message> {
        if self.popout_id.is_some() {
            return Task::none();
        }
        let mut tasks = Vec::new();
        self.place_tries = 0;
        self.opening = None;
        if let Some(id) = self.window_id.take() {
            tasks.push(window::close(id));
        }
        self.show();
        let (id, open) = window::open(popout_window());
        self.popout_id = Some(id);
        self.opening_popout = Some(id);
        tasks.push(open.map(|id| Message::WindowId(Some(id))));
        Task::batch(tasks)
    }

    fn sync_window(&mut self) -> Task<Message> {
        if let Some(pop) = self.popout_id {
            if self.visible {
                return Task::none();
            }
            self.popout_id = None;
            self.opening_popout = None;
            return window::close(pop);
        }
        match (self.visible, self.mapped()) {
            (true, false) => {
                let open = self.open_overlay();
                let activate = self.try_activate(0);
                Task::batch([open, activate])
            }
            (false, true) => {
                self.place_tries = 0;
                self.opening = None;
                match self.window_id.take() {
                    Some(id) => window::close(id),
                    None => Task::none(),
                }
            }
            (true, true) => {
                self.place_tries = 20;
                self.try_place();
                self.try_activate(0)
            }
            (false, false) => Task::none(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowId(id) => {
                if id.is_some() && id == self.opening_popout {
                    self.opening_popout = None;
                    return Task::none();
                }
                if id != self.opening {
                    return Task::none();
                }
                self.opening = None;
                self.window_id = id;
                if self.visible {
                    self.place_tries = 20;
                    self.try_place();
                    return self.try_activate(0);
                }
                Task::none()
            }
            Message::Tick => {
                if self
                    .tray_quit
                    .as_ref()
                    .is_some_and(|q| q.load(std::sync::atomic::Ordering::Relaxed))
                {
                    return iced::exit();
                }
                self.try_place();
                if let Some(req) = summon::try_recv() {
                    let was = self.visible;
                    self.apply_summon(req);
                    if was != self.visible {
                        return self.sync_window();
                    }
                    if self.visible {
                        return self.try_activate(0);
                    }
                }
                Task::none()
            }
            Message::Refresh => load_snap(),
            Message::Loaded(snap) => {
                self.snap = snap;
                self.clamp_selected();
                Task::none()
            }
            Message::ActivationApplied(ok) => {
                if ok {
                    self.pending_token = None;
                }
                Task::none()
            }
            Message::ActivateRetry(attempt) => {
                if self.pending_token.is_some() && self.visible {
                    self.try_activate(attempt)
                } else {
                    Task::none()
                }
            }
            Message::Close(id) => {
                if Some(id) == self.popout_id {
                    self.popout_id = None;
                    self.opening_popout = None;
                    self.hide();
                    return window::close(id);
                }
                self.hide();
                self.opening = None;
                if let Some(oid) = self.window_id.take() {
                    window::close(oid)
                } else {
                    Task::none()
                }
            }
            Message::Closed(id) => {
                if Some(id) == self.popout_id {
                    self.popout_id = None;
                    return Task::none();
                }
                self.opening = None;
                if Some(id) == self.window_id {
                    self.window_id = None;
                }
                Task::none()
            }
            Message::Key(key) => self.on_key(key),
        }
    }

    fn on_key(&mut self, key: Key) -> Task<Message> {
        if !self.visible {
            return Task::none();
        }
        let ch = match &key {
            Key::Character(c) => Some(c.as_str()),
            _ => None,
        };
        match (&key, ch) {
            (Key::Named(Named::Tab), _) | (_, Some("l")) => {
                self.pane = self.pane.next();
                self.selected = 0;
            }
            (_, Some("h")) => {
                self.pane = self.pane.prev();
                self.selected = 0;
            }
            (Key::Named(Named::ArrowDown), _) | (_, Some("j")) => {
                self.selected = self.selected.saturating_add(1);
                self.clamp_selected();
            }
            (Key::Named(Named::ArrowUp), _) | (_, Some("k")) => {
                self.selected = self.selected.saturating_sub(1);
            }
            (_, Some("1")) => {
                self.pane = Pane::Due;
                self.selected = 0;
            }
            (_, Some("2")) => {
                self.pane = Pane::Claims;
                self.selected = 0;
            }
            (_, Some("3")) => {
                self.pane = Pane::Trust;
                self.selected = 0;
            }
            (_, Some("P") | Some("p")) => {
                return self.pop_out();
            }
            (_, Some("r")) => {
                return load_snap();
            }
            (_, Some("?")) => {
                self.help = !self.help;
            }
            (Key::Named(Named::Escape), _) | (_, Some("q")) => {
                self.hide();
                return self.sync_window();
            }
            _ => {}
        }
        Task::none()
    }
}

/// Undecorated always-on-top overlay. Sway is told to float it over IPC.
pub fn overlay_window() -> window::Settings {
    let boot = icedtea::app::Boot::new("ljos", crate::place::OVERLAY_APP_ID)
        .overlay()
        .size(HUD_W, HUD_H)
        .min_size(360.0, 420.0);
    icedtea::app::bootstrap(&boot).window
}

/// The pop-out: a decorated window at the compositor's own level.
pub fn popout_window() -> window::Settings {
    let boot = icedtea::app::Boot::new("ljos", POPOUT_APP_ID)
        .decorations(true)
        .size(HUD_W, HUD_H)
        .min_size(360.0, 420.0);
    icedtea::app::bootstrap(&boot).window
}

/// Run the iced daemon. `visible` maps the overlay on first paint.
pub fn run(visible: bool) -> anyhow::Result<()> {
    icedtea::typo::install_platform_faces();
    run_iced(visible).map_err(|err| anyhow::anyhow!("{err}"))
}

fn run_iced(visible: bool) -> iced::Result {
    iced::daemon(move || boot(visible), update, view)
        .subscription(subscription)
        .theme(|_: &HudApp, _| theme::theme())
        .title(|_: &HudApp, _| "ljos".to_string())
        .default_font(theme::FACE)
        .settings(iced::Settings {
            default_text_size: Pixels::from(theme::SIZE_BODY),
            default_font: theme::FACE,
            ..Default::default()
        })
        .run()
}

fn boot(visible: bool) -> (HudApp, Task<Message>) {
    let mut app = HudApp::new(visible);
    app.tray_quit = crate::tray::start();
    let task = if visible {
        app.open_overlay()
    } else {
        Task::none()
    };
    (app, task)
}

fn update(app: &mut HudApp, message: Message) -> Task<Message> {
    app.update(message)
}

fn view(app: &HudApp, _id: window::Id) -> Element<'_, Message> {
    view::view(&app.snap, app.pane, app.selected, app.help)
}

fn subscription(_app: &HudApp) -> Subscription<Message> {
    Subscription::batch([
        event::listen_with(|event, _status, id| match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => Some(Message::Key(key)),
            Event::Window(window::Event::CloseRequested) => Some(Message::Close(id)),
            Event::Window(window::Event::Closed) => Some(Message::Closed(id)),
            _ => None,
        }),
        time::every(Duration::from_millis(50)).map(|_| Message::Tick),
        time::every(Duration::from_secs(2)).map(|_| Message::Refresh),
    ])
}

fn load_snap() -> Task<Message> {
    Task::perform(
        async {
            tokio::task::spawn_blocking(Snapshot::load)
                .await
                .unwrap_or_else(|_| Snapshot::empty())
        },
        Message::Loaded,
    )
}

const _: Font = theme::FACE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_and_popout_app_ids_differ() {
        assert_ne!(crate::place::OVERLAY_APP_ID, POPOUT_APP_ID);
        assert_eq!(crate::place::OVERLAY_APP_ID, "me.rgoswami.ljos-hud");
        assert_eq!(POPOUT_APP_ID, "me.rgoswami.ljos-hud.window");
        let overlay = overlay_window();
        let pop = popout_window();
        assert!(!overlay.decorations);
        assert!(pop.decorations, "the pop-out is a managed window");
        assert_eq!(overlay.level, iced::window::Level::AlwaysOnTop);
        assert_eq!(pop.level, iced::window::Level::Normal);
        assert!(!overlay.exit_on_close_request);
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                overlay.platform_specific.application_id,
                crate::place::OVERLAY_APP_ID
            );
            assert_eq!(pop.platform_specific.application_id, POPOUT_APP_ID);
            assert_ne!(
                overlay.platform_specific.application_id, pop.platform_specific.application_id,
                "a compositor rule on the overlay's id leaves the pop-out alone"
            );
            assert!(overlay.platform_specific.override_redirect);
        }
    }

    #[test]
    fn activation_is_wired_not_dropped() {
        let src = include_str!("app.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("iced::daemon"));
        assert!(prod.contains("wlactivate::activate"));
        assert!(prod.contains("window::run"));
        assert!(prod.contains("ActivationApplied"));
        assert!(
            !prod.contains("let _ = tok"),
            "k9f1 drop: never ignore the token"
        );
        assert!(!prod.contains("window::gain_focus"));
    }

    #[test]
    fn q_hides_and_pop_out_close_keeps_the_daemon() {
        let mut app = HudApp::new(true);
        app.visible = true;
        let id = window::Id::unique();
        app.opening = Some(id);
        let _ = app.update(Message::WindowId(Some(id)));
        assert!(app.mapped());
        let _ = app.on_key(Key::Character("q".into()));
        assert!(!app.visible, "q hides, does not exit");

        app.visible = true;
        let _ = app.pop_out();
        assert!(!app.mapped(), "the overlay goes down");
        let pop = app.popout_id.expect("a pop-out window");
        assert!(app.visible);
        let _ = app.pop_out();
        assert_eq!(app.popout_id, Some(pop));
        let _ = app.update(Message::Close(pop));
        assert!(app.popout_id.is_none());
        assert!(!app.visible, "closing the pop-out hides");
    }
}
