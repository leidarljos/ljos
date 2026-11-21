//! A pinned default tracker that was never made stays unmade.
//!
//! A product checkout with its own untracked `vissue.toml` is not the
//! tracker, so ljos names the seat's default directory under the data home
//! instead. That directory does not exist until `ljos onboard` makes it. A
//! roster write or a mesh pull used to create it, with `vissue.toml` or a
//! `.ljos/memory` tree inside, from a command the person ran in a checkout.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Place {
    _root: tempfile::TempDir,
    product: PathBuf,
    home: PathBuf,
    run: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let out = ljos_test_boot::git()
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=seat",
            "-c",
            "user.email=seat@example.test",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

impl Place {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let product = root.path().join("product");
        let home = root.path().join("home");
        let run = root.path().join("run");
        for dir in [&product, &home, &run] {
            std::fs::create_dir_all(dir).unwrap();
        }
        git(&product, &["init", "-q", "-b", "main"]);
        std::fs::write(product.join("README"), "a product\n").unwrap();
        git(&product, &["add", "README"]);
        git(&product, &["commit", "-q", "-m", "product"]);
        std::fs::write(product.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        Self {
            _root: root,
            product,
            home,
            run,
        }
    }

    fn ljos(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(&self.product)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .env("VISSUE_NO_ROUTE", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

#[test]
fn seat_add_does_not_make_the_default_tracker() {
    let place = Place::new();
    let out = place.ljos(&["seat", "add", "saddle"]);
    let said = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.starts_with("No tracker found"), "{said}");
    assert!(
        !place.home.join(".local/share/vissue").exists(),
        "the default tracker directory was made"
    );
}

#[test]
fn mesh_pull_does_not_make_the_default_tracker() {
    let place = Place::new();
    let from = place.home.join("handoff");
    std::fs::create_dir_all(&from).unwrap();
    let out = place.ljos(&[
        "mesh",
        "pull",
        "--seat",
        "saddle",
        "--from",
        "../home/handoff",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        !place.home.join(".local/share/vissue").exists(),
        "the default tracker directory was made"
    );
}
