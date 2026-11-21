//! A sitting on an issue id the tracker does not hold leaves every file as
//! it was, `vissue.toml` included.
//!
//! The CLI and the `ljos mcp` tool both refuse in one line that names the
//! id and `ljos ready`. Each case compares the bytes of every file in the
//! product checkout, the tracker and the seat's config directory before and
//! after. A `vissue` stand-in on `PATH` rewrites `vissue.toml` in the
//! directory it runs from, so a probe that reaches it shows up as a change.

ljos_test_boot::harness!();

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const MISSING: &str = "demo-zzzz";

type Snapshot = BTreeMap<String, Option<Vec<u8>>>;

fn snapshot(dir: &Path) -> Snapshot {
    fn walk(dir: &Path, root: &Path, out: &mut Snapshot) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            let name = path.strip_prefix(root).unwrap().display().to_string();
            if path.is_dir() {
                out.insert(name, None);
                walk(&path, root, out);
            } else {
                out.insert(name, Some(std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Snapshot::new();
    if dir.exists() {
        walk(dir, dir, &mut out);
    }
    out
}

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    ljos_cli::install_executable(&path, body).unwrap();
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

struct Place {
    _root: tempfile::TempDir,
    product: PathBuf,
    tracker: PathBuf,
    bin: PathBuf,
    home: PathBuf,
    run: PathBuf,
    /// Where a command runs and what names the tracker.
    cwd: PathBuf,
    named_root: bool,
}

impl Place {
    /// A product checkout, an empty tracker with one ticket, and tools that
    /// rewrite `vissue.toml` where they run.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let product = root.path().join("product");
        let tracker = root.path().join("tracker");
        let bin = root.path().join("bin");
        let home = root.path().join("home");
        let run = root.path().join("run");
        for dir in [&product, &tracker, &bin, &home, &run] {
            std::fs::create_dir_all(dir).unwrap();
        }
        git(&product, &["init", "-q", "-b", "main"]);
        std::fs::write(product.join("README"), "a product\n").unwrap();
        git(&product, &["add", "README"]);
        git(&product, &["commit", "-q", "-m", "product"]);
        let layout = vissue_core::Layout::new(&tracker, "Software");
        std::fs::create_dir_all(layout.projects_dir()).unwrap();
        vissue_core::ops::create(
            &layout,
            "demo",
            "an existing ticket",
            vissue_core::CreateOpts {
                quiet: true,
                ..vissue_core::CreateOpts::default()
            },
        )
        .unwrap();
        let canary = "#!/bin/sh\nprintf 'seats = [\"intruder\"]\\n' >> vissue.toml\nexit 0\n";
        script(&bin, "vissue", canary);
        script(
            &bin,
            "claimdag",
            "#!/bin/sh\nprintf 'seats = [\"intruder\"]\\n' >> vissue.toml\ncase \"$1\" in\n  get) exit 1 ;;\n  list) echo '[]' ;;\n  *) echo 'gen=1' ;;\nesac\nexit 0\n",
        );
        script(&bin, "deedar", "#!/bin/sh\necho head\nexit 0\n");
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos"), bin.join("ljos")).unwrap();
        Self {
            _root: root,
            cwd: product.clone(),
            product,
            tracker,
            bin,
            home,
            run,
            named_root: false,
        }
    }

    fn name_the_tracker(mut self) -> Self {
        self.named_root = true;
        self
    }

    fn stand_in(mut self, dir: &Path) -> Self {
        self.cwd = dir.to_path_buf();
        self
    }

    fn command(&self, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(&self.cwd)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("VISSUE_NO_ROUTE", "1");
        if self.named_root {
            cmd.env("VISSUE_ROOT", &self.tracker);
        }
        cmd
    }

    /// Every file the call could write, as bytes.
    fn files(&self) -> (Snapshot, Snapshot, Snapshot) {
        (
            snapshot(&self.product),
            snapshot(&self.tracker),
            snapshot(&self.home.join(".config")),
        )
    }

    fn write_toml(&self, dir: &Path, text: &str) {
        std::fs::write(dir.join("vissue.toml"), text).unwrap();
    }

    fn cli(&self, args: &[&str]) -> Output {
        self.command(&self.bin.join("ljos"))
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// One `tools/call` to a fresh `ljos-mcp`. Returns `(isError, text)`.
    fn mcp(&self, name: &str, arguments: serde_json::Value) -> (bool, String) {
        let mut child = self
            .command(Path::new(env!("CARGO_BIN_EXE_ljos-mcp")))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(40));
            let _ = Command::new("kill").arg(pid.to_string()).status();
        });
        let mut stdin = child.stdin.take().unwrap();
        for line in [
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"probe","version":"0"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name": name, "arguments": arguments}}),
        ] {
            writeln!(stdin, "{line}").unwrap();
        }
        stdin.flush().unwrap();
        drop(stdin);
        let mut answer = None;
        for line in BufReader::new(child.stdout.take().unwrap()).lines() {
            let Ok(line) = line else { break };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if value["id"] == 2 {
                answer = Some(value);
                break;
            }
        }
        let _ = child.wait();
        let answer = answer.unwrap_or_else(|| panic!("no answer from {name}"));
        let text = answer["result"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let failed = answer["result"]["isError"] == true || answer.get("error").is_some();
        (failed, text)
    }
}

fn said(out: &Output) -> String {
    format!("status {}\n{}", out.status, spoken(out))
}

fn spoken(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The refusal is one line, names the id and says how to list ids.
fn assert_one_line_naming_the_id(text: &str) {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with("next:"))
        .collect();
    assert_eq!(lines.len(), 1, "the refusal was not one line:\n{text}");
    assert!(lines[0].contains(MISSING), "the id is not named:\n{text}");
    assert!(
        lines[0].contains("`ljos ready`") || lines[0].contains("`ljos search`"),
        "no way to find ids:\n{text}"
    );
}

fn assert_unchanged(place: &Place, before: &(Snapshot, Snapshot, Snapshot), what: &str) {
    let after = place.files();
    assert_eq!(after.0, before.0, "{what} wrote the product checkout");
    assert_eq!(after.1, before.1, "{what} wrote the tracker");
    assert_eq!(after.2, before.2, "{what} wrote the seat's config");
}

/// [`assert_unchanged`] for a refusal that records the refused attempt as a
/// memory row under the tracker's `.ljos/memory/attempts`. That row is the
/// attempt log, not tracker state, so it is left out of the comparison.
fn assert_unchanged_but_for_attempts(
    place: &Place,
    before: &(Snapshot, Snapshot, Snapshot),
    what: &str,
) {
    let mut after = place.files();
    after.1.retain(|name, _| {
        !(name == ".ljos" || name == ".ljos/memory" || name.starts_with(".ljos/memory/attempts"))
    });
    assert_eq!(after.0, before.0, "{what} wrote the product checkout");
    assert_eq!(after.1, before.1, "{what} wrote the tracker");
    assert_eq!(after.2, before.2, "{what} wrote the seat's config");
}

#[test]
fn a_tracker_checkout_keeps_its_vissue_toml_bytes() {
    let place = Place::new();
    let toml = "prefix = \"Software\"\nseats = [\"saddle\"]\n";
    place.write_toml(&place.tracker, toml);
    git(&place.tracker, &["init", "-q", "-b", "main"]);
    git(&place.tracker, &["add", "-A"]);
    git(&place.tracker, &["commit", "-q", "-m", "tracker"]);
    let tracker = place.tracker.clone();
    let place = place.stand_in(&tracker);
    let before = place.files();

    let out = place.cli(&["sitting", MISSING]);
    assert!(!out.status.success(), "{}", said(&out));
    assert_one_line_naming_the_id(&spoken(&out));
    assert_unchanged(&place, &before, "ljos sitting");

    let (failed, text) = place.mcp(
        "ljos_sitting",
        serde_json::json!({"seat":"saddle","issue": MISSING}),
    );
    assert!(failed, "{text}");
    assert_one_line_naming_the_id(&text);
    assert_unchanged(&place, &before, "ljos_sitting");
    assert_eq!(
        std::fs::read_to_string(place.tracker.join("vissue.toml")).unwrap(),
        toml
    );
}

#[test]
fn a_named_tracker_without_vissue_toml_gets_none() {
    let place = Place::new().name_the_tracker();
    assert!(!place.tracker.join("vissue.toml").exists());
    let before = place.files();

    let out = place.cli(&["sitting", MISSING]);
    assert!(!out.status.success(), "{}", said(&out));
    assert_one_line_naming_the_id(&spoken(&out));
    assert_unchanged(&place, &before, "ljos sitting");

    let (failed, text) = place.mcp(
        "ljos_sitting",
        serde_json::json!({"seat":"saddle","issue": MISSING}),
    );
    assert!(failed, "{text}");
    assert_one_line_naming_the_id(&text);
    assert_unchanged(&place, &before, "ljos_sitting");
    assert!(!place.tracker.join("vissue.toml").exists());
    assert!(!place.product.join("vissue.toml").exists());
}

#[test]
fn a_named_tracker_keeps_a_roster_that_lists_the_seat() {
    let place = Place::new().name_the_tracker();
    let toml = "seats = [\"saddle\", \"alice\"]\n";
    place.write_toml(&place.tracker, toml);
    let before = place.files();

    let out = place.cli(&["sitting", MISSING]);
    assert!(!out.status.success(), "{}", said(&out));
    assert_one_line_naming_the_id(&spoken(&out));
    assert_unchanged(&place, &before, "ljos sitting");

    let (failed, text) = place.mcp(
        "ljos_sitting",
        serde_json::json!({"seat":"saddle","issue": MISSING}),
    );
    assert!(failed, "{text}");
    assert_one_line_naming_the_id(&text);
    assert_unchanged(&place, &before, "ljos_sitting");
    assert_eq!(
        std::fs::read_to_string(place.tracker.join("vissue.toml")).unwrap(),
        toml
    );
}

#[test]
fn a_roster_that_leaves_the_seat_out_is_not_extended() {
    let place = Place::new().name_the_tracker();
    let toml = "seats = [\"alice\"]\n";
    place.write_toml(&place.tracker, toml);
    let before = place.files();

    let out = place.cli(&["sitting", MISSING]);
    assert!(!out.status.success(), "{}", said(&out));
    assert_unchanged_but_for_attempts(&place, &before, "ljos sitting");

    let _ = place.mcp(
        "ljos_sitting",
        serde_json::json!({"seat":"saddle","issue": MISSING}),
    );
    assert_unchanged_but_for_attempts(&place, &before, "ljos_sitting");
    assert_eq!(
        std::fs::read_to_string(place.tracker.join("vissue.toml")).unwrap(),
        toml
    );
}

#[test]
fn a_plan_on_a_missing_issue_writes_nothing() {
    let place = Place::new().name_the_tracker();
    let toml = "seats = [\"saddle\"]\n";
    place.write_toml(&place.tracker, toml);
    let before = place.files();

    let (failed, text) = place.mcp(
        "ljos_sitting",
        serde_json::json!({"seat":"saddle","issue": MISSING,"items":["one","two"]}),
    );
    assert!(failed, "{text}");
    assert!(text.contains(MISSING), "the id is not named:\n{text}");
    assert_unchanged(&place, &before, "ljos_sitting with items");
}

#[test]
fn a_stray_vissue_toml_in_the_product_checkout_is_left_alone() {
    let place = Place::new();
    let toml = "prefix = \"Software\"\n";
    place.write_toml(&place.product, toml);
    let before = place.files();

    let out = place.cli(&["sitting", MISSING]);
    assert!(!out.status.success(), "{}", said(&out));
    assert_unchanged(&place, &before, "ljos sitting");

    let (failed, text) = place.mcp(
        "ljos_sitting",
        serde_json::json!({"seat":"saddle","issue": MISSING}),
    );
    assert!(failed, "{text}");
    assert_eq!(
        text.lines()
            .filter(|line| !line.is_empty() && !line.starts_with("next:"))
            .count(),
        1,
        "the refusal was not one line:\n{text}"
    );
    assert_unchanged(&place, &before, "ljos_sitting");
    assert_eq!(
        std::fs::read_to_string(place.product.join("vissue.toml")).unwrap(),
        toml
    );
}
