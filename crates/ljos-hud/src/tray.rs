//! A StatusNotifier tray for the long-lived HUD. Left click toggles the
//! overlay through the summon socket, the menu offers the same and Quit.
//! `LJOS_HUD_TRAY=0` leaves the tray out. Linux only.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// The tray's title.
pub const TITLE: &str = "ljos";
/// A themed icon name every Freedesktop theme carries.
pub const ICON: &str = "view-list-symbolic";
/// Menu labels, in order.
pub const MENU_TOGGLE: &str = "Show or hide the pane";
/// The item that leaves the HUD.
pub const MENU_QUIT: &str = "Quit the HUD";

/// `LJOS_HUD_TRAY=0` is the one way off.
#[must_use]
pub fn enabled() -> bool {
    match std::env::var("LJOS_HUD_TRAY") {
        Ok(v) => v.trim() != "0",
        Err(_) => true,
    }
}

/// The menu as it reads, top to bottom.
#[must_use]
pub fn menu_labels() -> [&'static str; 2] {
    [MENU_TOGGLE, MENU_QUIT]
}

/// Start the tray on its own thread. The flag it returns is set by Quit;
/// the app reads it on its tick and exits. `None` when the tray is off.
#[must_use]
pub fn start() -> Option<Arc<AtomicBool>> {
    if !enabled() {
        return None;
    }
    let quit = Arc::new(AtomicBool::new(false));
    #[cfg(target_os = "linux")]
    linux::start(Arc::clone(&quit));
    Some(quit)
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use crate::summon::{self, SummonAction};

    struct LjosTray {
        quit: Arc<AtomicBool>,
    }

    fn toggle_pane() {
        let _ = summon::send_command(SummonAction::Toggle);
    }

    impl ksni::Tray for LjosTray {
        fn id(&self) -> String {
            "ljos-hud".into()
        }

        fn title(&self) -> String {
            super::TITLE.into()
        }

        fn icon_name(&self) -> String {
            super::ICON.into()
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            toggle_pane();
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::{MenuItem, StandardItem};
            vec![
                StandardItem {
                    label: super::MENU_TOGGLE.into(),
                    activate: Box::new(|_: &mut Self| toggle_pane()),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: super::MENU_QUIT.into(),
                    activate: Box::new(|this: &mut Self| this.quit.store(true, Ordering::Relaxed)),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    pub fn start(quit: Arc<AtomicBool>) {
        let _ = std::thread::Builder::new()
            .name("ljos-tray".into())
            .spawn(move || {
                use ksni::TrayMethods;
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                rt.block_on(async move {
                    if let Ok(_handle) = (LjosTray { quit }).spawn().await {
                        std::future::pending::<()>().await;
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_toggles_then_quits() {
        assert_eq!(menu_labels(), [MENU_TOGGLE, MENU_QUIT]);
        assert!(MENU_QUIT.contains("HUD"), "Quit names what it leaves");
    }
}
