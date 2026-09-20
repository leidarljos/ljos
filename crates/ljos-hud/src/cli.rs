//! Process flags for `ljos-hud` and the `ljos hud` launcher.

use clap::Parser;

use crate::detach;
use crate::summon::{self, SummonAction, SummonCli};

/// Env override for the HUD binary the launcher execs.
pub const HUD_BIN_ENV: &str = "LJOS_HUD_BIN";

/// Read-only pane over due, claims, and trust.
#[derive(Debug, Clone, Parser)]
#[command(name = "ljos-hud", version, about = "Read-only ljos seat pane")]
pub struct HudCli {
    /// Stay on the terminal. Default detach uses process_group(0) only.
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
    let mut visible = cli.initial_visible();
    if let Some(action) = cli.summon_action() {
        match summon::plan_summon_cli(action, summon::send_command(action)) {
            Ok(SummonCli::Done) => return Ok(0),
            Ok(SummonCli::StartShown) => visible = true,
            Err(err) => {
                eprintln!("ljos-hud: {err}");
                return Ok(1);
            }
        }
    }
    if !cli.foreground {
        return detach::start_detached(&cli);
    }
    run_owner(visible)
}

fn run_owner(visible: bool) -> anyhow::Result<i32> {
    let _summon = match summon::install() {
        Ok(server) => Some(server),
        Err(summon::SummonError::AlreadyRunning(_)) => {
            return Ok(0);
        }
        Err(summon::SummonError::Unsupported) => None,
        Err(err) => {
            return Err(err.into());
        }
    };
    crate::app::run(visible)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_summon_flags() {
        let cli = HudCli::parse_from(["ljos-hud", "--toggle"]);
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
    fn parse_foreground() {
        let cli = HudCli::parse_from(["ljos-hud", "--foreground"]);
        assert!(cli.foreground);
        assert!(cli.initial_visible());
    }

    #[test]
    fn hide_without_socket_is_zero() {
        let _guard = crate::env_lock();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(
                crate::summon::SOCKET_ENV,
                format!("/tmp/ljos-hud-none-{}", std::process::id()),
            );
        }
        let code = run_with(HudCli {
            foreground: true,
            toggle: false,
            show: false,
            hide: true,
            install_desktop: false,
        })
        .unwrap();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::remove_var(crate::summon::SOCKET_ENV);
        }
        assert_eq!(code, 0);
    }
}
