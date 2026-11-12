//! A lesson exported from one seat is stored in another seat's pack and
//! comes back on a matching prompt.
//!
//! Seat A writes one lesson and `ljos export --out -` prints the file.
//! Seat B imports that file into a fresh workspace, integrates it, and the
//! prompt hook and a search both name the lesson. The materialised count
//! is the number of memories the file carried.

ljos_test_boot::harness!();

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const LESSON: &str = "Rebuild the docs site with zola before fixing the broken links.";
const PROMPT: &str = "fix the broken links on the docs site";

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

struct Seat {
    home: tempfile::TempDir,
    port: u16,
    path: String,
}

impl Seat {
    fn run(&self, workspace: &str, root: &Path, args: &[&str], stdin: Option<&str>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args).current_dir(root);
        self.env(&mut cmd, workspace, root);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn ljos");
        let mut pipe = child.stdin.take().expect("stdin");
        if let Some(text) = stdin {
            pipe.write_all(text.as_bytes()).expect("write stdin");
        }
        drop(pipe);
        child.wait_with_output().expect("wait ljos")
    }

    fn run_slot(&self, workspace: &str, root: &Path, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args).current_dir(root);
        self.env(&mut cmd, workspace, root);
        cmd.env("PACKSET_URL", "off");
        cmd.env_remove("PACKSET_PORT");
        cmd.env_remove("PACKSET_HOME");
        cmd.output().expect("ljos")
    }

    fn env(&self, cmd: &mut Command, workspace: &str, root: &Path) {
        let home = self.home.path();
        cmd.env_clear()
            .env("HOME", home)
            .env("PATH", &self.path)
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", home.join("tmp"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("PACKSET_PORT", self.port.to_string())
            .env("PACKSET_HOME", home.join(".local/share/packset"))
            .env("PACKSET_WORKSPACE", workspace)
            .env("VISSUE_ROOT", root)
            .env("LJOS_TRACKER_GIT", "off");
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn number_after(text: &str, marker: &str) -> Option<usize> {
    text.lines().find_map(|line| {
        let rest = line.split(marker).nth(1)?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

fn open_seat() -> (Seat, ljos_test_boot::Packsetd) {
    let packsetd = on_path("packsetd").expect("packsetd is not on PATH");
    let home = tempfile::tempdir().expect("home");
    for sub in [
        "tmp",
        "run",
        ".config",
        ".local/share/packset",
        ".local/state",
        ".cache",
    ] {
        std::fs::create_dir_all(home.path().join(sub)).expect("home dirs");
    }
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_ljos"))
        .parent()
        .expect("bin dir");
    let path = std::env::join_paths([
        bin_dir,
        packsetd.parent().expect("packsetd dir"),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .expect("PATH")
    .into_string()
    .expect("utf-8 PATH");
    let mut started = ljos_test_boot::Packsetd::build();
    started.bin(&packsetd);
    started.home_at(home.path());
    started.pack_at(home.path().join(".local/share/packset"));
    let daemon = started.spawn();
    let seat = Seat {
        port: daemon.port(),
        path,
        home,
    };
    (seat, daemon)
}

fn tracker(home: &Path, name: &str) -> PathBuf {
    let root = home.join(name);
    std::fs::create_dir_all(&root).expect("tracker");
    root
}

#[test]
fn export_import_integrate_round_trip_reaches_the_hook() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let (seat, _daemon) = open_seat();
    let from = tracker(seat.home.path(), "seat-a");
    let into = tracker(seat.home.path(), "seat-b");

    let remembered = seat.run("roundtrip-a", &from, &["remember", LESSON], None);
    assert!(
        remembered.status.success(),
        "remember: {}",
        text(&remembered)
    );

    let exported = seat.run("roundtrip-a", &from, &["export", "--out", "-"], None);
    assert!(exported.status.success(), "export: {}", text(&exported));
    assert!(
        !from.join("-").is_file(),
        "export --out - wrote a file named -"
    );
    let file = String::from_utf8_lossy(&exported.stdout);
    assert!(
        file.contains("ljos-memory-1") && file.contains("zola"),
        "stdout was not the memory file:\n{file}\n{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    let carried = number_after(&String::from_utf8_lossy(&exported.stderr), "exported ");
    assert!(carried.is_some_and(|n| n > 0), "{}", text(&exported));
    let carried = carried.expect("exported count");
    let path = into.join("mem.jsonl");
    std::fs::write(&path, exported.stdout).expect("write the export");

    let imported = seat.run(
        "roundtrip-b",
        &into,
        &["memory", "import", path.to_str().expect("utf-8")],
        None,
    );
    assert!(imported.status.success(), "import: {}", text(&imported));
    let rows = number_after(&text(&imported), " rows ");
    assert_eq!(rows, Some(carried), "import: {}", text(&imported));

    let integrated = seat.run(
        "roundtrip-b",
        &into,
        &["memory", "integrate", "--all"],
        None,
    );
    assert!(
        integrated.status.success(),
        "integrate: {}",
        text(&integrated)
    );
    assert!(
        text(&integrated).contains("zola"),
        "integrate did not name the lesson: {}",
        text(&integrated)
    );

    let listed = seat.run("roundtrip-b", &into, &["memory", "imports"], None);
    assert!(listed.status.success(), "imports: {}", text(&listed));
    let stored = number_after(&text(&listed), " materialised ");
    assert_eq!(
        stored,
        Some(carried),
        "materialised count: {}",
        text(&listed)
    );

    let hook = seat.run(
        "roundtrip-b",
        &into,
        &["hook", "--prompt", "--text"],
        Some(PROMPT),
    );
    assert!(hook.status.success(), "hook: {}", text(&hook));
    let heard = String::from_utf8_lossy(&hook.stdout);
    assert!(
        heard.contains("zola"),
        "the hook did not recall the imported lesson:\n{heard}\n{}",
        String::from_utf8_lossy(&hook.stderr)
    );

    let recalled = seat.run("roundtrip-b", &into, &["search", PROMPT], None);
    assert!(recalled.status.success(), "search: {}", text(&recalled));
    assert!(
        text(&recalled).contains("zola"),
        "recall did not find the imported lesson: {}",
        text(&recalled)
    );
}

/// Export from a scratch seat, import into a fresh seat, export again.
/// The memory sets match: ids, kinds, text and entity labels.
#[test]
fn export_import_export_keeps_the_same_memory_set() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let (seat, _daemon) = open_seat();
    let from = tracker(seat.home.path(), "seat-a");
    let into = tracker(seat.home.path(), "seat-b");
    let lesson = "The kiln fires at dusk on the north bench.";
    let preference = "Use the north bench for the kiln.";
    let remembered = seat.run("round-a", &from, &["remember", lesson], None);
    assert!(
        remembered.status.success(),
        "remember: {}",
        text(&remembered)
    );
    let preferred = seat.run("round-a", &from, &["prefer", preference], None);
    assert!(preferred.status.success(), "prefer: {}", text(&preferred));

    let first = from.join("first.jsonl");
    let exported = seat.run(
        "round-a",
        &from,
        &["memory", "export", "--out", first.to_str().unwrap()],
        None,
    );
    assert!(exported.status.success(), "export: {}", text(&exported));

    let imported = seat.run_slot(
        "round-b",
        &into,
        &["memory", "import", first.to_str().unwrap()],
    );
    assert!(imported.status.success(), "import: {}", text(&imported));

    let second = into.join("second.jsonl");
    let again = seat.run_slot(
        "round-b",
        &into,
        &["memory", "export", "--out", second.to_str().unwrap()],
    );
    assert!(again.status.success(), "second export: {}", text(&again));
    assert_eq!(
        memory_set(&first),
        memory_set(&second),
        "first:\n{}\nsecond:\n{}",
        std::fs::read_to_string(&first).unwrap(),
        std::fs::read_to_string(&second).unwrap()
    );
}

fn memory_set(path: &Path) -> std::collections::BTreeSet<(String, String, String, Vec<String>)> {
    let body = std::fs::read_to_string(path).expect("export file");
    let mut set = std::collections::BTreeSet::new();
    for line in body.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("export line");
        let Some(text) = value["text"].as_str() else {
            continue;
        };
        // Import tags the file stem as a lane. That is provenance of the
        // file, not of the memories the export carried.
        let mut labels: Vec<String> = value["entities"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .filter(|tag| !tag.starts_with("lane:"))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        labels.sort();
        set.insert((
            value["atom_id"].as_str().unwrap_or("").to_string(),
            value["kind"].as_str().unwrap_or("").to_string(),
            text.to_string(),
            labels,
        ));
    }
    set
}
