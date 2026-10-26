#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::Command;

/// The reproducible release recipe places `--remap-path-prefix=${SRCDIR}=/build/ljos`
/// after `--remap-path-prefix=${HOME}=/home/build`. Because `rustc` applies
/// the last matching `--remap-path-prefix`, a checkout inside `HOME` maps
/// its source files to `/build/ljos` rather than `/home/build/...`, and any
/// recipe-built release binaries in `target/release` embed neither the
/// checkout path nor `HOME`.
#[test]
fn release_remap_order_maps_checkout_inside_home_and_strips_host_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home/user");
    let src_in_home = home.join("work/ljos");
    let src_outside = tmp.path().join("outside/ljos");
    std::fs::create_dir_all(&src_in_home).unwrap();
    std::fs::create_dir_all(&src_outside).unwrap();

    for src in [&src_in_home, &src_outside] {
        let rs = src.join("probe.rs");
        let out = src.join("probe");
        std::fs::write(&rs, "fn main() { println!(\"{}\", file!()); }\n").unwrap();
        let st = Command::new("rustc")
            .arg(&rs)
            .arg("-o")
            .arg(&out)
            .arg(format!(
                "--remap-path-prefix={}=/home/build",
                home.display()
            ))
            .arg(format!("--remap-path-prefix={}=/build/ljos", src.display()))
            .status()
            .unwrap();
        assert!(st.success());
        let printed = Command::new(&out).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&printed.stdout).trim(),
            "/build/ljos/probe.rs"
        );
    }

    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let recipe = root.join("docs/reproducible-build.md");
    assert!(
        recipe.is_file(),
        "missing reproducible build recipe at {}",
        recipe.display()
    );
    let root_str = root.display().to_string();
    let target = root.join("target/release");
    for name in ["ljos", "ljos-mcp", "ljos-hud"] {
        let bin = target.join(name);
        if !bin.is_file() {
            continue;
        }
        let bytes = std::fs::read(&bin).unwrap();
        let hay = String::from_utf8_lossy(&bytes);
        if hay.contains("/build/ljos") {
            assert!(
                !hay.contains(&root_str),
                "{} embeds checkout path {root_str}",
                bin.display()
            );
        }
    }
}
