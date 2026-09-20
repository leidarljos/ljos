//! Process flags for `ljos-hud`. The `ljos hud` launcher lives in ljos-cli.

use std::path::PathBuf;

use clap::Parser;

use crate::detach;
use crate::summon::{self, SummonAction, SummonCli};

/// Read-only pane over due, claims, and trust.
#[derive(Debug, Clone, Parser)]
#[command(name = "ljos-hud", version, about = "Read-only ljos seat pane")]
pub struct HudCli {
    /// Control socket path. Falls back to LJOS_HUD_SOCKET, then
    /// $XDG_RUNTIME_DIR/ljos/hud.sock.
    #[arg(short = 's', long)]
    pub socket: Option<PathBuf>,
    /// Stay on the terminal. Default detaches.
    #[arg(long)]
    pub foreground: bool,
    /// Show or hide a running HUD (compositor bind target).
    #[arg(long, group = "summon")]
    pub toggle: bool,
    /// Show a running HUD.
    #[arg(long, group = "summon")]
    pub show: bool,
    /// Hide a running HUD.
    #[arg(long, group = "summon")]
    pub hide: bool,
    /// Write a user-local .desktop launcher and Sway overlay include.
    #[arg(long)]
    pub install_desktop: bool,
}

impl HudCli {
    /// Summon verb from flags, if any.
    pub fn summon_action(&self) -> Option<SummonAction> {
        if self.toggle {
            Some(SummonAction::Toggle)
        } else if self.show {
            Some(SummonAction::Show)
        } else if self.hide {
            Some(SummonAction::Hide)
        } else {
            None
        }
    }

    /// Whether the overlay starts mapped. `--hide` starts hidden.
    pub fn initial_visible(&self) -> bool {
        !self.hide || self.show || self.toggle
    }
}

/// Parse args and run: summon bounce, detach, or the iced loop.
pub fn run_cli() -> anyhow::Result<i32> {
    let cli = HudCli::parse();
    run_with(cli)
}

/// Run already-parsed flags: bounce off a live HUD, detach, or own the loop.
pub fn run_with(cli: HudCli) -> anyhow::Result<i32> {
    if cli.install_desktop {
        crate::install_desktop::run_cli()?;
        return Ok(0);
    }
    if let Some(path) = cli.socket.as_ref() {
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(summon::SOCKET_ENV, path);
        }
    }
    if let Some(action) = cli.summon_action() {
        match summon::plan_summon_cli(action, summon::send_command(action)) {
            Ok(SummonCli::Done) => return Ok(0),
            Ok(SummonCli::StartShown) => {}
            Err(err) => return Err(err.into()),
        }
    }
    if !cli.foreground {
        return detach::start_detached(&cli);
    }
    run_owner(cli)
}

pub(crate) fn run_owner(cli: HudCli) -> anyhow::Result<i32> {
    let _summon = match summon::install() {
        Ok(server) => Some(server),
        Err(summon::SummonError::AlreadyRunning(_)) => {
            if let Some(action) = cli.summon_action() {
                let _ = summon::send_command(action);
            }
            return Ok(0);
        }
        Err(summon::SummonError::Unsupported) => None,
        Err(err) => {
            crate::log::error(&format!("summon install: {err}"));
            return Err(err.into());
        }
    };
    crate::app::run(crate::app::BootOpts {
        visible: cli.initial_visible(),
        summon: _summon,
    })?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn parse_foreground_and_summon_flags() {
        let cli = HudCli::parse_from(["ljos-hud", "--foreground", "--toggle"]);
        assert!(cli.foreground);
        assert_eq!(cli.summon_action(), Some(SummonAction::Toggle));
        assert!(cli.initial_visible());
    }

    #[test]
    fn hide_starts_hidden() {
        let cli = HudCli::parse_from(["ljos-hud", "--hide", "--foreground"]);
        assert!(!cli.initial_visible());
        assert_eq!(cli.summon_action(), Some(SummonAction::Hide));
    }

    #[test]
    fn help_is_in_the_cli() {
        let help = HudCli::command().render_help().to_string();
        assert!(help.contains("--foreground"));
        assert!(help.contains("--toggle"));
        assert!(help.contains("--install-desktop"));
        assert!(help.contains("--hide"));
    }

    #[cfg(unix)]
    #[test]
    fn hide_without_socket_is_zero() {
        let _guard = crate::env_lock();
        let path = format!("/tmp/ljos-hud-none-{}", std::process::id());
        let _ = std::fs::remove_file(&path);
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(summon::SOCKET_ENV, &path);
        }
        let code = run_with(HudCli {
            socket: None,
            foreground: true,
            toggle: false,
            show: false,
            hide: true,
            install_desktop: false,
        })
        .unwrap();
        assert_eq!(code, 0);
        #[allow(unused_unsafe)]
        unsafe {
            std::env::remove_var(summon::SOCKET_ENV);
        }
    }
}
