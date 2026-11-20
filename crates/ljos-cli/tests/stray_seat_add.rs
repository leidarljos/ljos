//! `ljos seat add` and `ljos seat grant` run in a product checkout while no
//! tracker is named.
//!
//! vissue takes the working directory for the tracker as a last guess, so
//! the roster write used to create `vissue.toml` and a commit lock in the
//! product. The call now stops with the one line every tracker verb prints
//! when nothing names a tracker, and the checkout stays as it was.

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
    let out = Command::new("git")
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

fn tree(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            out.push(path.strip_prefix(root).unwrap().display().to_string());
            if path.is_dir() {
                walk(&path, root, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
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
fn seat_add_with_no_named_tracker_writes_nothing_into_the_product() {
    let place = Place::new();
    let before = tree(&place.product);
    let out = place.ljos(&["seat", "add", "saddle"]);
    let said = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(said.starts_with("No tracker found"), "{said}");
    assert_eq!(said.lines().count(), 1, "{said}");
    assert_eq!(tree(&place.product), before, "the product was written");
    assert!(!place.product.join("vissue.toml").exists());
}

#[test]
fn seat_grant_with_no_named_tracker_writes_nothing_into_the_product() {
    let place = Place::new();
    let before = tree(&place.product);
    let out = place.ljos(&[
        "seat",
        "grant",
        "bea",
        "close=own",
        "--quote",
        "yes, close it",
        "--at",
        "2026-01-02T03:04:05Z",
        "--via",
        "saddle",
    ]);
    let said = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.starts_with("No tracker found"), "{said}");
    assert_eq!(tree(&place.product), before, "{said}");
}
