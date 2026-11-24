//! Every install line ljos prints is `RELEASE_INSTALL_COMMAND`. A copy of
//! the URL written by hand in a source or a test drifts from it.

use std::path::{Path, PathBuf};

use ljos_cli::{RELEASE_INSTALL_COMMAND, RELEASE_INSTALL_URL};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_install_url_is_written_once_in_the_rust_sources() {
    let mut files = Vec::new();
    rust_files(&repo().join("crates"), &mut files);
    assert!(files.len() > 10, "no sources under crates/: {files:?}");
    // Split so this file is not a copy of what it looks for.
    let needle = ["releases/latest", "/download/install.sh"].concat();
    let mut found = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        for (n, line) in text.lines().enumerate() {
            if line.contains(&needle) {
                let rel = path
                    .strip_prefix(repo())
                    .unwrap_or(path)
                    .display()
                    .to_string();
                found.push(format!("{rel}:{}", n + 1));
            }
        }
    }
    let defining = "crates/ljos-cli/src/lib.rs";
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].starts_with(defining), "{found:#?}");
    assert!(
        RELEASE_INSTALL_COMMAND.contains(RELEASE_INSTALL_URL),
        "{RELEASE_INSTALL_COMMAND}"
    );
}

#[test]
fn the_readme_and_getting_started_name_the_printed_install() {
    for rel in [
        "README.md",
        "docs/source/getting-started.rst",
        "docs/orgmode/getting-started.org",
    ] {
        let text = std::fs::read_to_string(repo().join(rel)).unwrap();
        assert!(
            text.lines()
                .any(|line| line.trim().trim_start_matches("$ ") == RELEASE_INSTALL_COMMAND),
            "{rel} does not name `{RELEASE_INSTALL_COMMAND}`"
        );
    }
}

#[test]
fn the_uninstall_stub_names_the_printed_install() {
    assert!(
        ljos_cli::uninstall::STUB.contains(&format!("Reinstall: {RELEASE_INSTALL_COMMAND}\"")),
        "{}",
        ljos_cli::uninstall::STUB
    );
}
