//! Source tests that do not need a compositor.

#[test]
fn ljos_cli_never_links_iced() {
    let toml = include_str!("../../ljos-cli/Cargo.toml");
    assert!(!toml.contains("iced"), "musl CLI must not link iced");
    assert!(!toml.contains("icedtea"), "musl CLI must not link icedtea");
}

#[test]
fn hud_dist_never_musl() {
    let toml = include_str!("../Cargo.toml");
    assert!(toml.contains("aarch64-apple-darwin"));
    assert!(toml.contains("x86_64-unknown-linux-gnu"));
    assert!(
        !toml.contains("unknown-linux-musl"),
        "HUD dist targets must not include musl"
    );
}

#[test]
fn activation_wiring_calls_wlactivate() {
    let app = include_str!("../src/app.rs");
    let prod = app.split("#[cfg(test)]").next().unwrap();
    assert!(prod.contains("wlactivate::activate"));
    assert!(prod.contains("window::run"));
    assert!(prod.contains("ActivationApplied"));
    assert!(prod.contains("iced::daemon"));
    assert!(!prod.contains("let _ = tok"));
    assert!(!prod.contains("iced::application"));
}

#[test]
fn overlay_and_popout_ids_differ_in_source() {
    let place = include_str!("../src/place.rs");
    let app = include_str!("../src/app.rs");
    let desktop = include_str!("../src/install_desktop.rs");
    assert!(place.contains("me.rgoswami.ljos-hud"));
    assert!(app.contains("me.rgoswami.ljos-hud.window"));
    assert!(desktop.contains("ljos hud --toggle"));
    assert!(desktop.contains("crate::place::OVERLAY_APP_ID"));
}
