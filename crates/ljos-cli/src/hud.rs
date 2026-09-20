//! `ljos hud` — exec the separate `ljos-hud` binary.
//!
//! The musl CLI stays thin: this module never depends on iced. The HUD is a
//! workspace crate (`crates/ljos-hud`) and is not in a musl tarball.

use std::env;
use std::path::PathBuf;

/// Env override for the HUD binary this launcher execs.
pub const HUD_BIN_ENV: &str = "LJOS_HUD_BIN";

/// Flags named in `--help` so a missing HUD binary still documents the pane.
pub const HELP: &str = "\
Usage: ljos hud [OPTION]...
Launch the ljos-hud icedtea overlay (due / claims / trust).

The HUD is a separate glibc binary (crates/ljos-hud, icedtea 0.17).
It is not in a musl CLI tarball. This command execs `ljos-hud`
(sibling of this binary, $LJOS_HUD_BIN, or PATH) and forwards flags.

  --show / --hide / --toggle
                       talk to a running HUD (hide-miss exits 0)
  --install-desktop    write .desktop + Sway overlay rules
  --foreground         stay on the terminal (default detaches)
  -h, --help           show this help
  -V, --version        print the HUD version via the HUD binary

Examples:
  ljos hud --help
  ljos hud --toggle
";

/// Locate `ljos-hud`. Missing means exit 127 (except `--hide`).
pub fn resolve_hud_bin() -> Option<PathBuf> {
    if let Ok(raw) = env::var(HUD_BIN_ENV) {
        let t = raw.trim();
        if !t.is_empty() {
            return Some(PathBuf::from(t));
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("ljos-hud");
            if sibling.is_file() {
                return Some(sibling);
            }
        }
    }
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
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

/// Dispatch `ljos hud`. `--help` is answered here so a missing HUD still
/// names the overlay.
pub fn run(prog: &str, args: &[String]) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return 0;
    }
    // Hide with no HUD binary is success (compositor bind miss).
    if args.iter().any(|a| a == "--hide") && !resolve_hud_bin().is_some_and(|p| p.is_file()) {
        return 0;
    }
    let Some(bin) = resolve_hud_bin().filter(|p| p.is_file()) else {
        eprintln!("{prog}: {}", missing_bin_message());
        return 127;
    };
    let mut cmd = std::process::Command::new(&bin);
    cmd.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        eprintln!("{prog}: exec {}: {err}", bin.display());
        1
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(st) => st.code().unwrap_or(1),
            Err(e) => {
                eprintln!("{prog}: spawn {}: {e}", bin.display());
                1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_text_names_the_hud() {
        assert!(HELP.contains("ljos-hud"));
        assert!(HELP.contains("icedtea"));
        assert!(HELP.contains("--toggle"));
        assert!(HELP.contains("--install-desktop"));
        assert!(HELP.contains("--foreground"));
        assert!(missing_bin_message().contains("ljos-hud"));
    }

    #[test]
    fn ljos_cli_cargo_toml_never_links_iced() {
        let toml = include_str!("../Cargo.toml");
        assert!(
            !toml.contains("iced"),
            "musl CLI must not link iced: {toml}"
        );
        assert!(
            !toml.contains("icedtea"),
            "musl CLI must not link icedtea: {toml}"
        );
    }

    #[test]
    fn hide_without_binary_is_zero() {
        let _guard = env_lock();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(
                HUD_BIN_ENV,
                format!("/nonexistent/ljos-hud-{}", std::process::id()),
            );
        }
        let code = run("ljos", &["--hide".into()]);
        #[allow(unused_unsafe)]
        unsafe {
            std::env::remove_var(HUD_BIN_ENV);
        }
        assert_eq!(code, 0);
    }

    #[test]
    fn missing_binary_is_127() {
        let _guard = env_lock();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(
                HUD_BIN_ENV,
                format!("/nonexistent/ljos-hud-{}", std::process::id()),
            );
        }
        let code = run("ljos", &["--toggle".into()]);
        #[allow(unused_unsafe)]
        unsafe {
            std::env::remove_var(HUD_BIN_ENV);
        }
        assert_eq!(code, 127);
    }

    #[test]
    fn help_without_binary_is_zero() {
        let _guard = env_lock();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(
                HUD_BIN_ENV,
                format!("/nonexistent/ljos-hud-{}", std::process::id()),
            );
        }
        let code = run("ljos", &["--help".into()]);
        #[allow(unused_unsafe)]
        unsafe {
            std::env::remove_var(HUD_BIN_ENV);
        }
        assert_eq!(code, 0);
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }
}
