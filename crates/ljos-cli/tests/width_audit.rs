//! A page builder takes a [`ljos_cli::output_wrap::Width`], which has no
//! default, so a caller has to name one. These checks cover what clippy
//! cannot name: no `Option<usize>` width, the fixed layout only where the
//! output does not go to a terminal, no wrap that skips the page width,
//! and no terminal width read outside `Width`. They read `ljos-cli/src`
//! and `ljos-hud/src`.
//!
//! The print macros, `dbg!`, `std::io::stdout`, `std::io::stderr`, and the
//! raw writers `FromRawFd::from_raw_fd`, `libc::write`, `libc::writev`,
//! `rustix::io::write` and `rustix::io::writev` are refused by the
//! workspace `clippy.toml`, under
//! `cargo clippy --workspace --all-targets -- -D warnings`. Those bans
//! keep new code on the output macros. They are hygiene: the wrap rule
//! itself is `ljos_cli::output::shape`.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};

/// The items that may name `Width::FIXED`, by path under `crates/`, and why.
const FIXED_ITEMS: &[(&str, &str)] = &[
    // Hook JSON goes to the runner.
    ("ljos-cli/src/lib.rs", "HOOK_WIDTH"),
    // The sitting block is parsed line by line and is the `--json` text.
    ("ljos-cli/src/lib.rs", "SITTING_WIDTH"),
    // A brief is written to a file and handed to a subagent.
    ("ljos-cli/src/lib.rs", "brief"),
    // The close check reads one-line `label` records.
    ("ljos-cli/src/lib.rs", "finish"),
    ("ljos-cli/src/lib.rs", "open_child_close_refusal"),
    // `ljos sitting --plan` compares the plan's `next:` line whole.
    ("ljos-cli/src/plan.rs", "render"),
    // The digest the MCP tool returns.
    ("ljos-cli/src/digest.rs", "digest_for_tool"),
    // `--json` output.
    ("ljos-cli/src/main.rs", "page_width"),
    // JSON-RPC replies.
    ("ljos-cli/src/bin/ljos-mcp/server.rs", "TOOL_WIDTH"),
    // The pane takes the banner lines from the island page; it is not a terminal.
    ("ljos-hud/src/data.rs", "island_notices_of"),
];

fn sources() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut dirs = vec![crates.join("ljos-cli/src"), crates.join("ljos-hud/src")];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The modules declared only under `#[cfg(test)]`, by file stem.
fn test_only_modules(files: &[(PathBuf, String)]) -> Vec<String> {
    let mut test_mods = Vec::new();
    let mut plain_mods = Vec::new();
    for (_, text) in files {
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            let item = line.trim();
            let Some(name) = item
                .strip_prefix("pub mod ")
                .or_else(|| item.strip_prefix("pub(crate) mod "))
                .or_else(|| item.strip_prefix("mod "))
                .and_then(|rest| rest.strip_suffix(';'))
            else {
                continue;
            };
            let gated = lines[..at]
                .iter()
                .rev()
                .take_while(|above| above.trim().starts_with("#["))
                .any(|above| above.trim() == "#[cfg(test)]");
            if gated {
                test_mods.push(name.to_string());
            } else {
                plain_mods.push(name.to_string());
            }
        }
    }
    test_mods.retain(|name| !plain_mods.contains(name));
    test_mods
}

/// Each line outside `#[cfg(test)]` items and comments, with its number
/// and the top-level item it sits in.
fn shipped_lines(text: &str) -> Vec<(usize, String, &str)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut item = String::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if line.trim() == "#[cfg(test)]" {
            let depth = indent(line);
            let mut end = at + 1;
            while end < lines.len() && lines[end].trim().starts_with("#[") {
                end += 1;
            }
            if end < lines.len() && !lines[end].trim_end().ends_with(';') {
                let close = format!("{}}}", " ".repeat(depth));
                while end < lines.len() && lines[end].trim_end() != close {
                    end += 1;
                }
                assert!(
                    end < lines.len(),
                    "no {close:?} ends the item at line {}",
                    at + 1
                );
            }
            at = end + 1;
            continue;
        }
        if indent(line) == 0 {
            if let Some(name) = item_name(line) {
                item = name;
            }
        }
        if !line.trim_start().starts_with("//") {
            out.push((at + 1, item.clone(), line));
        }
        at += 1;
    }
    out
}

fn item_name(line: &str) -> Option<String> {
    let mut rest = line;
    for prefix in ["pub(crate) ", "pub ", "async ", "const fn ", "unsafe "] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest);
    }
    let rest = ["fn ", "const ", "static "]
        .iter()
        .find_map(|kind| rest.strip_prefix(kind))?;
    let name: String = rest
        .chars()
        .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn every_shipped_output_path_names_its_width() {
    let files: Vec<(PathBuf, String)> = sources()
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            (path, text)
        })
        .collect();
    let test_only = test_only_modules(&files);
    let mut faults = Vec::new();
    let mut used = Vec::new();
    for (path, text) in &files {
        let name = file_name(path);
        let stem = name.trim_end_matches(".rs");
        if test_only.iter().any(|module| module == stem) {
            continue;
        }
        let shown = path
            .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
            .unwrap_or(path)
            .display()
            .to_string();
        for (number, item, line) in shipped_lines(text) {
            let at = format!("{shown}:{number} in {item}: {}", line.trim());
            let wrapper = name == "output_wrap.rs";
            if !wrapper
                && (line.contains("width: Option<usize>") || line.contains("cols: Option<usize>"))
            {
                faults.push(format!("a width that may be left out, {at}"));
            }
            if line.contains("Width::FIXED") && !wrapper {
                match FIXED_ITEMS
                    .iter()
                    .find(|(file, it)| *file == shown && *it == item)
                {
                    Some(entry) => used.push(*entry),
                    None => {
                        faults.push(format!("the fixed layout outside the allowed items, {at}"))
                    }
                }
            }
            if line.contains("wrap_prose(") && !wrapper && name != "scan.rs" {
                faults.push(format!("a wrap that skips the page width, {at}"));
            }
            if line.contains("prose_width_for_std") && !wrapper {
                faults.push(format!("a terminal width read outside Width, {at}"));
            }
        }
    }
    for entry in FIXED_ITEMS {
        if !used.contains(entry) {
            faults.push(format!(
                "{entry:?} is allowed the fixed layout but does not use it"
            ));
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

#[test]
fn the_audit_sees_the_lines_it_checks() {
    let text = "pub fn shown() {\n    let a = 1;\n}\n\n#[cfg(test)]\nmod tests {\n    fn hidden() {}\n}\n\n/// doc Width::FIXED\nconst LAST: u8 = 1;\n";
    let lines = shipped_lines(text);
    let seen: Vec<(&str, &str)> = lines
        .iter()
        .map(|(_, item, line)| (item.as_str(), line.trim()))
        .collect();
    assert!(seen.contains(&("shown", "let a = 1;")), "{seen:?}");
    assert!(seen.contains(&("LAST", "const LAST: u8 = 1;")), "{seen:?}");
    assert!(
        !seen.iter().any(|(_, line)| line.contains("hidden")),
        "{seen:?}"
    );
    assert!(
        !seen.iter().any(|(_, line)| line.contains("FIXED")),
        "{seen:?}"
    );
}

/// The paths the workspace `clippy.toml` refuses. Taking one out of the
/// file lets a crate print past the wrappers again with clippy still green.
#[test]
fn clippy_refuses_the_direct_writers() {
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../clippy.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    let config: toml::Value = toml::from_str(&text).unwrap();
    let paths = |key: &str| -> Vec<String> {
        config[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap().to_string())
            .collect()
    };
    let macros = paths("disallowed-macros");
    for name in [
        "std::print",
        "std::println",
        "std::eprint",
        "std::eprintln",
        "std::dbg",
    ] {
        assert!(macros.iter().any(|path| path == name), "{name}: {macros:?}");
    }
    let methods = paths("disallowed-methods");
    for name in [
        "std::io::stdout",
        "std::io::stderr",
        "std::io::Stdout::lock",
        "std::io::Stderr::lock",
    ]
    .into_iter()
    .chain(RAW_WRITERS)
    {
        assert!(
            methods.iter().any(|path| path == name),
            "{name}: {methods:?}"
        );
    }
}

/// The paths that write to descriptor 1 or 2 without a `Stdout` or
/// `Stderr`. `nix` is not in the lockfile, so it has no entry.
const RAW_WRITERS: [&str; 6] = [
    "std::os::fd::FromRawFd::from_raw_fd",
    "std::os::unix::io::FromRawFd::from_raw_fd",
    "libc::write",
    "rustix::io::write",
    "libc::writev",
    "rustix::io::writev",
];

/// A crate that writes to the terminal through each raw writer.
const RAW_PROBE: &str = r#"#![allow(dead_code)]

use std::io::Write as _;

fn std_fd() {
    use std::os::fd::FromRawFd as _;
    let mut out = unsafe { std::fs::File::from_raw_fd(1) };
    let _ = out.write_all(b"unwrapped\n");
}

fn unix_io() {
    use std::os::unix::io::FromRawFd as _;
    let mut err = unsafe { std::fs::File::from_raw_fd(2) };
    let _ = err.write_all(b"unwrapped\n");
}

fn c_write() {
    let text = b"unwrapped\n";
    unsafe { libc::write(2, text.as_ptr().cast(), text.len()) };
}

fn rustix_write() {
    let _ = rustix::io::write(rustix::stdio::stdout(), b"unwrapped\n");
}

fn c_writev() {
    let text = b"unwrapped\n";
    let iov = libc::iovec {
        iov_base: text.as_ptr() as *mut libc::c_void,
        iov_len: text.len(),
    };
    unsafe { libc::writev(2, &iov, 1) };
}

fn rustix_writev() {
    let text = [std::io::IoSlice::new(b"unwrapped\n")];
    let _ = rustix::io::writev(rustix::stdio::stderr(), &text);
}
"#;

/// Runs clippy on [`RAW_PROBE`] with the `clippy.toml` in `conf_dir`.
fn clippy_on_raw_probe(root: &Path, conf_dir: &Path, target: &Path) -> std::process::Output {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let probe = root.join("probe");
    std::fs::create_dir_all(probe.join("src")).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        probe.join("Cargo.toml"),
        "[package]\nname = \"raw-probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [dependencies]\nlibc = \"0.2\"\nrustix = { version = \"1\", features = [\"stdio\"] }\n\n\
         [workspace]\n",
    )
    .unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(probe.join("src/lib.rs"), RAW_PROBE).unwrap();
    ljos_cli::copy_file(workspace.join("Cargo.lock"), probe.join("Cargo.lock")).unwrap();
    ljos_cli::copy_file(
        workspace.join("rust-toolchain.toml"),
        probe.join("rust-toolchain.toml"),
    )
    .unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    return std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["clippy", "--offline", "--quiet", "--", "-D", "warnings"])
        .current_dir(&probe)
        .env("CLIPPY_CONF_DIR", conf_dir)
        .env("CARGO_TARGET_DIR", target)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .unwrap();
}

/// The revert proof for [`RAW_WRITERS`]: with the workspace `clippy.toml`
/// the probe fails clippy at each raw writer, and with those paths taken
/// out it passes.
#[test]
fn clippy_refuses_the_raw_writers_and_passes_them_without_the_paths() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = tempfile::tempdir().unwrap();

    let refused = clippy_on_raw_probe(root.path(), &workspace, &root.path().join("target-refused"));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{stderr}");
    for call in [
        "std::fs::File::from_raw_fd(1)",
        "std::fs::File::from_raw_fd(2)",
        "libc::write(",
        "rustix::io::write(",
        "libc::writev(",
        "rustix::io::writev(",
    ] {
        assert!(stderr.contains(call), "{call}: {stderr}");
    }
    assert_eq!(
        stderr
            .matches("route terminal output through output.rs")
            .count(),
        6,
        "{stderr}"
    );

    let text = std::fs::read_to_string(workspace.join("clippy.toml")).unwrap();
    let reverted: String = text
        .lines()
        .filter(|line| {
            !RAW_WRITERS
                .iter()
                .any(|path| line.contains(&format!("\"{path}\"")))
        })
        .map(|line| format!("{line}\n"))
        .collect();
    assert_ne!(reverted, text);
    let conf = root.path().join("reverted");
    std::fs::create_dir_all(&conf).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(conf.join("clippy.toml"), reverted).unwrap();
    let passed = clippy_on_raw_probe(root.path(), &conf, &root.path().join("target-reverted"));
    assert!(
        passed.status.success(),
        "{}",
        String::from_utf8_lossy(&passed.stderr)
    );
}

/// The output setup clippy cannot see. Command-line binaries select their
/// output kind before installing the panic hook. The graphical binary
/// installs the panic hook before its other work.
#[test]
fn every_binary_starts_with_its_output_setup() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for (file, prefix) in [
        (
            "ljos-cli/src/main.rs",
            &[
                "let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();",
                "ljos_cli::output::set_output_kind(output_kind(&argv));",
                "ljos_cli::output::install_panic_hook();",
            ][..],
        ),
        (
            "ljos-cli/src/bin/ljos-mcp/main.rs",
            &[
                "ljos_cli::output::set_output_kind(ljos_cli::output::Kind::Machine);",
                "ljos_cli::output::install_panic_hook();",
            ][..],
        ),
        (
            "ljos-hud/src/bin.rs",
            &["ljos_cli::output::install_panic_hook();"][..],
        ),
    ] {
        let text = std::fs::read_to_string(crates.join(file)).unwrap();
        let body = text
            .split_once("\nfn main() {\n")
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map(|(body, _)| body)
            .unwrap_or_else(|| panic!("{file}: no fn main"));
        let first: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .take(prefix.len())
            .collect();
        assert_eq!(
            first, prefix,
            "{file}: main does not start with its output setup"
        );
    }
}
