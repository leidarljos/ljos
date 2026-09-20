//! `ljos hud` — exec the separate `ljos-hud` binary.
//!
//! The musl CLI stays thin: this module never depends on iced. The HUD is a
//! glibc workspace crate (`crates/ljos-hud`) and is not in a musl tarball.

use std::path::PathBuf;

/// Env override for the HUD binary this launcher execs.
pub const HUD_BIN_ENV: &str = "LJOS_HUD_BIN";

/// Flags forwarded to `ljos-hud`.
#[derive(Debug, Clone, Default)]
pub struct HudLaunch {
    /// Stay on the terminal.
    pub foreground: bool,
    /// Show or hide a running HUD.
    pub toggle: bool,
    /// Show a running HUD.
    pub show: bool,
    /// Hide a running HUD.
    pub hide: bool,
    /// Write a user-local .desktop launcher and Sway overlay include.
    pub install_desktop: bool,
}

/// Locate `ljos-hud`. Missing means exit 127.
pub fn resolve_hud_bin() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var(HUD_BIN_ENV) {
        let t = raw.trim();
        if !t.is_empty() {
            return Some(PathBuf::from(t));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("ljos-hud");
            if sibling.is_file() {
                return Some(sibling);
            }
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("ljos-hud");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Hint printed when the launcher cannot find the HUD binary.
pub fn missing_bin_message() -> &'static str {
    "ljos-hud is not installed. Install it with:\n  cargo install ljos-hud"
}

/// Dispatch `ljos hud`. `--help` is answered by clap before this runs.
pub fn run(opts: HudLaunch) -> i32 {
    // Hide with no HUD binary is success (compositor bind miss).
    if opts.hide
        && !opts.show
        && !opts.toggle
        && !opts.install_desktop
        && !resolve_hud_bin().is_some_and(|p| p.is_file())
    {
        return 0;
    }
    let Some(bin) = resolve_hud_bin().filter(|p| p.is_file()) else {
        eprintln!("{}", missing_bin_message());
        return 127;
    };
    let mut cmd = std::process::Command::new(&bin);
    if opts.foreground {
        cmd.arg("--foreground");
    }
    if opts.toggle {
        cmd.arg("--toggle");
    } else if opts.show {
        cmd.arg("--show");
    } else if opts.hide {
        cmd.arg("--hide");
    }
    if opts.install_desktop {
        cmd.arg("--install-desktop");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        eprintln!("ljos: exec {}: {err}", bin.display());
        1
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(st) => st.code().unwrap_or(1),
            Err(e) => {
                eprintln!("ljos: spawn {}: {e}", bin.display());
                1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ENV.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn missing_bin_is_127() {
        let _g = env_lock();
        let _before = std::env::var_os(HUD_BIN_ENV);
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(HUD_BIN_ENV, "/tmp/ljos-hud-missing-bin");
        }
        let code = run(HudLaunch {
            foreground: true,
            ..HudLaunch::default()
        });
        match _before {
            Some(v) => unsafe { std::env::set_var(HUD_BIN_ENV, v) },
            None => unsafe { std::env::remove_var(HUD_BIN_ENV) },
        }
        assert_eq!(code, 127);
        assert!(missing_bin_message().contains("ljos-hud"));
    }

    #[test]
    fn hide_without_binary_is_zero() {
        let _g = env_lock();
        let _before = std::env::var_os(HUD_BIN_ENV);
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(HUD_BIN_ENV, "/tmp/ljos-hud-missing-bin");
        }
        let code = run(HudLaunch {
            hide: true,
            ..HudLaunch::default()
        });
        match _before {
            Some(v) => unsafe { std::env::set_var(HUD_BIN_ENV, v) },
            None => unsafe { std::env::remove_var(HUD_BIN_ENV) },
        }
        assert_eq!(code, 0);
    }

    #[test]
    fn launcher_lives_in_the_cli() {
        let src = include_str!("main.rs");
        assert!(src.contains("Cmd::Hud"));
        assert!(src.contains("ljos-hud"));
        assert!(src.contains("install_desktop"));
        let cargo = include_str!("../Cargo.toml");
        assert!(!cargo.contains("iced"), "ljos crate must not link iced");
        assert!(!cargo.contains("icedtea"));
        assert!(!cargo.contains("ljos-hud"));
    }

    #[test]
    fn resolve_honors_override() {
        let _g = env_lock();
        let _before = std::env::var_os(HUD_BIN_ENV);
        let path = PathBuf::from("/tmp/custom-ljos-hud");
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(HUD_BIN_ENV, path.to_str().unwrap());
        }
        assert_eq!(resolve_hud_bin(), Some(path));
        match _before {
            Some(v) => unsafe { std::env::set_var(HUD_BIN_ENV, v) },
            None => unsafe { std::env::remove_var(HUD_BIN_ENV) },
        }
    }
}
