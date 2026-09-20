//! `ljos hud --install-desktop` writes a user-local launcher and Sway overlay rules.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Overlay app_id (Sway float/sticky). Same as [`crate::place::OVERLAY_APP_ID`].
pub const OVERLAY_APP_ID: &str = crate::place::OVERLAY_APP_ID;
/// Decorated / pop-out app_id.
pub const APP_ID: &str = "me.rgoswami.ljos-hud.window";
/// Human name.
pub const APP_NAME: &str = "ljos HUD";

/// Sway fragment: float/sticky overlay + bindsym toggle.
#[must_use]
pub fn sway_overlay_rules() -> String {
    format!(
        "# ljos overlay ({OVERLAY_APP_ID}). Include from sway config:\n\
         #   include ~/.config/ljos/sway-hud.conf\n\
         for_window [app_id=\"{OVERLAY_APP_ID}\"] floating enable\n\
         for_window [app_id=\"{OVERLAY_APP_ID}\"] border pixel 0\n\
         for_window [app_id=\"{OVERLAY_APP_ID}\"] sticky enable\n\
         bindsym $mod+Shift+l exec ljos hud --toggle\n"
    )
}

/// Paths written by a successful install.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Files created or replaced.
    pub wrote: Vec<PathBuf>,
}

impl Report {
    /// Lines for stdout.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.wrote
            .iter()
            .map(|p| format!("wrote {}", p.display()))
            .collect()
    }
}

/// Install under `xdg_data` / `xdg_config` using `exe` as Exec=.
pub fn install(home: &Path, exe: &Path, xdg_data: Option<&Path>) -> io::Result<Report> {
    let data = xdg_data
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .unwrap_or_else(|| home.join(".local/share"));
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let mut report = Report::default();

    let apps = data.join("applications");
    fs::create_dir_all(&apps)?;
    let desktop = apps.join("ljos-hud.desktop");
    let exec = exe.display();
    fs::write(
        &desktop,
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name={APP_NAME}\n\
             Comment=Read-only ljos seat pane\n\
             Exec={exec}\n\
             TryExec={exec}\n\
             Icon=ljos-hud\n\
             Terminal=false\n\
             Categories=Utility;\n\
             StartupWMClass={OVERLAY_APP_ID}\n\
             Keywords=ljos;seat;hud;\n"
        ),
    )?;
    report.wrote.push(desktop);

    let ljos_cfg = config.join("ljos");
    fs::create_dir_all(&ljos_cfg)?;
    let sway = ljos_cfg.join("sway-hud.conf");
    fs::write(&sway, sway_overlay_rules())?;
    report.wrote.push(sway);
    Ok(report)
}

/// CLI: install using this process executable.
pub fn run_cli() -> io::Result<Report> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME unset"))?;
    let exe = std::env::current_exe()?;
    let report = install(&home, &exe, None)?;
    for line in report.lines() {
        println!("{line}");
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sway_rules_name_overlay_and_toggle() {
        let s = sway_overlay_rules();
        assert!(s.contains(OVERLAY_APP_ID));
        assert!(s.contains("ljos hud --toggle"));
        assert!(s.contains("floating enable"));
        assert_ne!(OVERLAY_APP_ID, APP_ID);
        assert_eq!(APP_ID, "me.rgoswami.ljos-hud.window");
    }

    #[test]
    fn install_writes_desktop_and_sway() {
        let root = std::env::temp_dir().join(format!("ljos-hud-install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let exe = root.join("bin/ljos-hud");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"x").unwrap();
        let data = root.join("share");
        let _guard = crate::env_lock();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        }
        let report = install(&root, &exe, Some(&data)).unwrap();
        assert!(report.wrote.iter().any(|p| p.ends_with("ljos-hud.desktop")));
        assert!(report.wrote.iter().any(|p| p.ends_with("sway-hud.conf")));
        let desktop = fs::read_to_string(data.join("applications/ljos-hud.desktop")).unwrap();
        assert!(desktop.contains("Exec="));
        assert!(desktop.contains(OVERLAY_APP_ID));
        let sway = fs::read_to_string(root.join("config/ljos/sway-hud.conf")).unwrap();
        assert!(sway.contains("ljos hud --toggle"));
        assert!(sway.contains(OVERLAY_APP_ID));
        #[allow(unused_unsafe)]
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        let _ = fs::remove_dir_all(&root);
    }
}
