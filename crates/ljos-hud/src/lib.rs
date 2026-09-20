//! Read-only icedtea pane over the seat: due reviews, live claims, trust.
//!
//! The musl CLI does not link iced. `ljos hud` execs this binary.

pub mod app;
pub mod cli;
pub mod data;
pub mod detach;
pub mod graph;
pub mod install_desktop;
pub mod log;
pub mod place;
pub mod summon;
pub mod theme;
pub mod tray;
pub mod view;
pub mod wlactivate;

pub use cli::run_cli;
pub use summon::{parse_request, sanitize_token, SummonAction, SummonRequest};

#[cfg(test)]
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    #[test]
    fn dist_targets_are_gnu_and_darwin_only() {
        let cargo = include_str!("../Cargo.toml");
        assert!(cargo.contains("aarch64-apple-darwin"));
        assert!(cargo.contains("x86_64-unknown-linux-gnu"));
        assert!(!cargo.contains("musl"), "HUD must not ship a musl artifact");
    }

    #[test]
    fn release_yml_ships_hud_on_gnu_and_darwin_only() {
        let yml = include_str!("../../../.github/workflows/release.yml");
        assert!(yml.contains("-p ljos-hud"));
        assert!(yml.contains("x86_64-unknown-linux-gnu"));
        assert!(yml.contains("aarch64-apple-darwin"));
        assert!(
            !yml.contains("musl"),
            "release matrix must not build a musl HUD"
        );
    }

    #[test]
    fn ljos_cli_cargo_has_no_iced() {
        let cargo = include_str!("../../ljos-cli/Cargo.toml");
        assert!(
            !cargo.contains("iced"),
            "ljos-cli Cargo.toml must not name iced"
        );
        assert!(
            !cargo.contains("icedtea"),
            "ljos-cli Cargo.toml must not name icedtea"
        );
        assert!(
            !cargo.contains("ljos-hud"),
            "ljos-cli must not depend on the HUD crate"
        );
    }

    #[test]
    fn hud_sources_have_no_mutations() {
        let files = [
            include_str!("app.rs"),
            include_str!("bin.rs"),
            include_str!("cli.rs"),
            include_str!("data.rs"),
            include_str!("detach.rs"),
            include_str!("graph.rs"),
            include_str!("install_desktop.rs"),
            include_str!("log.rs"),
            include_str!("place.rs"),
            include_str!("summon.rs"),
            include_str!("theme.rs"),
            include_str!("tray.rs"),
            include_str!("view.rs"),
            include_str!("wlactivate.rs"),
            include_str!("lib.rs"),
        ];
        for src in files {
            let prod = src.split("#[cfg(test)]").next().unwrap();
            assert!(!prod.contains("graded("), "HUD must not grade");
            assert!(!prod.contains("post_atom"), "HUD must not write the pack");
            assert!(!prod.contains("sweep("), "HUD must not sweep");
            assert!(!prod.contains("fire: true"), "HUD must not fire an island");
            assert!(!prod.contains("fire=true"), "HUD must not fire an island");
            assert!(!prod.contains("due_report"), "HUD must not call due_report");
            assert!(!prod.contains("load_dir"), "HUD must not call load_dir");
        }
    }
}
