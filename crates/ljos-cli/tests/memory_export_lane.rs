//! `LJOS_SYNC_HOST` names an export, and `--as-machine` names an import.

ljos_test_boot::harness!();

use std::path::Path;
use std::process::Command;

fn run(
    root: &Path,
    home: &Path,
    seat: &str,
    sync_host: Option<&str>,
    args: &[&str],
) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.join("tmp"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("VISSUE_ROOT", root)
        .env("LJOS_SEAT", seat)
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .env("LJOS_OFF", "1");
    if let Some(host) = sync_host {
        cmd.env("LJOS_SYNC_HOST", host);
    }
    cmd.output().expect("ljos")
}

fn text(out: &std::process::Output) -> String {
    format!(
        "status {}\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn seat_home() -> (tempfile::TempDir, std::path::PathBuf) {
    let home = tempfile::tempdir().expect("home");
    let root = home.path().join("tracker");
    let kept = root.join(".ljos/memory/kept");
    std::fs::create_dir_all(&kept).unwrap();
    std::fs::create_dir_all(home.path().join("tmp")).unwrap();
    let row = serde_json::json!({
        "id": "local:lane",
        "text": "the lane keeps its own kiln",
        "kind": "lesson",
        "entities": ["seat:lane-seat"],
        "machine": "cursor",
        "seat": "lane-seat",
    });
    std::fs::write(
        kept.join("lane.json"),
        serde_json::to_vec_pretty(&row).unwrap(),
    )
    .unwrap();
    (home, root)
}

fn header_machine(body: &str) -> String {
    let line = body.lines().next().expect("header");
    let value: serde_json::Value = serde_json::from_str(line).expect("header json");
    value["machine"].as_str().unwrap_or("").to_string()
}

#[test]
fn help_names_machine_and_as_machine() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("tracker");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(home.path().join("tmp")).unwrap();
    let export = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &["memory", "export", "--help"],
    );
    assert!(export.status.success(), "{}", text(&export));
    let export_help = String::from_utf8(export.stdout).unwrap();
    assert!(export_help.contains("--machine"), "{export_help}");
    assert!(export_help.contains("LJOS_SYNC_HOST"), "{export_help}");
    let import = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &["memory", "import", "--help"],
    );
    assert!(import.status.success(), "{}", text(&import));
    let import_help = String::from_utf8(import.stdout).unwrap();
    assert!(import_help.contains("--as-machine"), "{import_help}");
    let top = run(&root, home.path(), "lane-seat", None, &["export", "--help"]);
    assert!(top.status.success(), "{}", text(&top));
    assert!(String::from_utf8(top.stdout).unwrap().contains("--machine"));
}

#[test]
fn sync_host_names_the_export_and_as_machine_names_the_import() {
    let (home, root) = seat_home();
    let out = root.join("mem.jsonl");
    let exported = run(
        &root,
        home.path(),
        "lane-seat",
        Some("lane-a"),
        &["memory", "export", "--out", out.to_str().unwrap()],
    );
    assert!(exported.status.success(), "{}", text(&exported));
    let bytes = std::fs::read(&out).unwrap();
    let body = String::from_utf8(bytes.clone()).unwrap();
    assert_eq!(header_machine(&body), "lane-a", "{body}");
    assert!(body.contains("the lane keeps its own kiln"), "{body}");

    let named = root.join("named.jsonl");
    let flagged = run(
        &root,
        home.path(),
        "lane-seat",
        Some("lane-a"),
        &[
            "memory",
            "export",
            "--machine",
            "lane-c",
            "--out",
            named.to_str().unwrap(),
        ],
    );
    assert!(flagged.status.success(), "{}", text(&flagged));
    let named_body = std::fs::read_to_string(&named).unwrap();
    assert_eq!(header_machine(&named_body), "lane-c", "{named_body}");

    let imported = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &[
            "memory",
            "import",
            "--as-machine",
            "lane-b",
            out.to_str().unwrap(),
        ],
    );
    assert!(imported.status.success(), "{}", text(&imported));
    let said = String::from_utf8(imported.stdout).unwrap();
    assert!(
        said.starts_with("imported 1, unchanged 0, skipped 0\n"),
        "{said}"
    );
    let digest = Command::new("sha256sum").arg(&out).output().unwrap();
    let digest = String::from_utf8(digest.stdout).unwrap();
    let import_id = digest.split_whitespace().next().unwrap().to_string();

    let slot = root.join(".ljos/memory/imports/lane-seat").join(&import_id);
    assert_eq!(std::fs::read(slot.join("payload.jsonl")).unwrap(), bytes);
    let index = std::fs::read_to_string(slot.join("index.jsonl")).unwrap();
    let row: serde_json::Value = serde_json::from_str(index.lines().next().unwrap()).unwrap();
    assert_eq!(row["machine"], "lane-b");

    let listed = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &["memory", "imports"],
    );
    assert!(listed.status.success(), "{}", text(&listed));
    let list = String::from_utf8(listed.stdout).unwrap();
    assert!(
        list.contains(&format!("{import_id} lane-b/lane-seat ")),
        "{list}"
    );
}

/// A fresh `ljos onboard` names the tracker in vissue's config and does not
/// write `vissue.toml`. The import still belongs under that tracker, which
/// is the path the memory pages name.
#[test]
fn an_onboarded_tracker_keeps_the_import() {
    let home = tempfile::tempdir().expect("home");
    let root = home.path().join("tracker");
    std::fs::create_dir_all(root.join("Software")).unwrap();
    std::fs::create_dir_all(home.path().join(".config/vissue")).unwrap();
    std::fs::create_dir_all(home.path().join(".local/state")).unwrap();
    std::fs::create_dir_all(home.path().join("tmp")).unwrap();
    std::fs::write(
        home.path().join(".config/vissue/config.toml"),
        format!("root = \"{}\"\n", root.display()),
    )
    .unwrap();
    let file = home.path().join("lesson.jsonl");
    std::fs::write(
        &file,
        concat!(
            "{\"format\":\"ljos-memory-1\",\"record\":\"export\",\"machine\":\"desk\",\"seat\":\"alice\",\"exported_at\":\"2026-11-13T00:00:00Z\",\"exporter_revision\":\"0.41.11\"}\n",
            "{\"format\":\"ljos-memory-1\",\"kind\":\"lesson\",\"text\":\"kiln brick stays dry\",\"origin\":\"agent-derived\",\"level\":\"explicit\",\"entities\":[],\"source\":{},\"atom_id\":\"abc\",\"content_sha256\":\"cd\",\"origin_hash\":\"ef\",\"exported_at\":\"2026-11-13T00:00:00Z\"}\n",
        ),
    )
    .unwrap();
    let imported = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["memory", "import", file.to_str().unwrap()])
        .current_dir(home.path())
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.path().join("tmp"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("LJOS_SEAT", "bob")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .env("LJOS_OFF", "1")
        .output()
        .expect("ljos");
    assert!(imported.status.success(), "{}", text(&imported));
    let slot = root.join(".ljos/memory/imports/bob");
    assert!(
        slot.is_dir(),
        "import landed outside the tracker\nstate: {}\n{}",
        home.path().join(".local/state/ljos/memory").display(),
        text(&imported)
    );
    assert!(
        !home
            .path()
            .join(".local/state/ljos/memory/imports")
            .exists(),
        "a second copy was written under the state directory"
    );
}

/// A path that is not there is one line, and it names the command that writes the file.
#[test]
fn a_missing_import_names_export() {
    let home = tempfile::tempdir().expect("home");
    std::fs::create_dir_all(home.path().join("tmp")).unwrap();
    let missing = home.path().join("no-such.jsonl");
    let imported = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["memory", "import", missing.to_str().unwrap()])
        .current_dir(home.path())
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.path().join("tmp"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("LJOS_SEAT", "bob")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .env("LJOS_OFF", "1")
        .output()
        .expect("ljos");
    assert!(!imported.status.success());
    let said = text(&imported);
    assert!(
        !said.contains("Caused by"),
        "the failure is a chain:\n{said}"
    );
    assert!(
        said.contains("ljos memory export --out"),
        "no next step:\n{said}"
    );
    assert!(said.contains("no-such.jsonl"), "{said}");
}

/// The words this system uses for `code`, without the `os error` number.
fn os_words(code: i32) -> String {
    let text = std::io::Error::from_raw_os_error(code).to_string();
    match text.rfind(" (os error ") {
        Some(at) if text.ends_with(')') => text[..at].to_string(),
        _ => text,
    }
}

fn fault(out: &std::process::Output) -> String {
    format!(
        "status {}\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn integrate_without_an_id_names_imports_and_all() {
    let (home, root) = seat_home();
    let out = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &["memory", "integrate"],
    );
    assert_eq!(out.status.code(), Some(1), "{}", fault(&out));
    let said = fault(&out);
    assert!(said.contains("pass an import id or --all."), "{said}");
    assert!(
        said.contains("`ljos memory imports` lists the ids"),
        "{said}"
    );
    assert!(
        said.contains("`ljos memory integrate --all` maps every import"),
        "{said}"
    );
    assert!(!said.contains("Caused by"), "{said}");
}

#[test]
fn an_unknown_import_names_imports() {
    let (home, root) = seat_home();
    let out = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &["memory", "integrate", "deadbeef"],
    );
    assert_eq!(out.status.code(), Some(1), "{}", fault(&out));
    let said = fault(&out);
    assert!(
        said.contains("no import starts with deadbeef. `ljos memory imports` lists the ids"),
        "{said}"
    );
    assert!(!said.contains("Caused by"), "{said}");
}

#[test]
fn an_id_and_all_together_names_both() {
    let (home, root) = seat_home();
    let out = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &["memory", "integrate", "deadbeef", "--all"],
    );
    assert_eq!(out.status.code(), Some(1), "{}", fault(&out));
    let said = fault(&out);
    assert!(
        said.contains("pass an import id or --all, not both. `ljos memory imports` lists the ids"),
        "{said}"
    );
    assert!(!said.contains("Caused by"), "{said}");
}

#[test]
fn integrate_all_with_nothing_names_import() {
    let (home, root) = seat_home();
    let out = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &["memory", "integrate", "--all"],
    );
    assert!(out.status.success(), "{}", fault(&out));
    let said = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        said,
        "no imports. `ljos memory import FILE` stores a file `ljos memory export --out` writes\n"
    );
}

#[test]
fn import_of_a_directory_names_export() {
    let (home, root) = seat_home();
    let dir = root.join("handoff");
    std::fs::create_dir(&dir).unwrap();
    let out = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &["memory", "import", dir.to_str().unwrap()],
    );
    assert_eq!(out.status.code(), Some(1), "{}", fault(&out));
    let said = fault(&out);
    assert!(
        said.contains("is a directory. Pass the file `ljos memory export --out` wrote"),
        "{said}"
    );
    assert!(!said.contains("Caused by"), "{said}");
}

#[test]
fn export_onto_a_directory_names_a_file() {
    let (home, root) = seat_home();
    let dir = root.join("notfile");
    std::fs::create_dir(&dir).unwrap();
    let out = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &["memory", "export", "--out", dir.to_str().unwrap()],
    );
    assert_eq!(out.status.code(), Some(1), "{}", fault(&out));
    let said = fault(&out);
    assert!(
        said.contains("is a directory. `ljos memory export --out FILE` writes a file"),
        "{said}"
    );
    assert!(!said.contains("Caused by"), "{said}");
}

#[test]
fn integrate_help_does_not_name_apply() {
    let (home, root) = seat_home();
    let out = run(
        &root,
        home.path(),
        "lane-seat",
        None,
        &["memory", "integrate", "--help"],
    );
    assert!(out.status.success(), "{}", fault(&out));
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(help.contains("--all"), "{help}");
    assert!(help.contains("--dry-run"), "{help}");
    assert!(help.contains("ljos memory imports"), "{help}");
    assert!(!help.contains("--apply"), "{help}");
}

#[test]
fn a_dead_share_stops_export_and_import_without_a_count() {
    let (home, root) = seat_home();
    let share = root.join("share");
    std::fs::create_dir(&share).unwrap();
    let out_path = share.join("lane.jsonl");
    let exported = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["memory", "export", "--out", out_path.to_str().unwrap()])
        .current_dir(&root)
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.path().join("tmp"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("VISSUE_ROOT", &root)
        .env("LJOS_SEAT", "lane-seat")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .env("LJOS_OFF", "1")
        .env("LJOS_SHARE_FAULT", "eio")
        .output()
        .expect("ljos");
    assert_eq!(exported.status.code(), Some(1), "{}", fault(&exported));
    let said = fault(&exported);
    assert!(said.contains(&os_words(libc::EIO)), "{said}");
    assert!(said.contains("Check the mount, then rerun"), "{said}");
    assert!(
        said.contains(&format!("ljos memory export --out {}", out_path.display())),
        "{said}"
    );
    assert!(said.contains(&share.display().to_string()), "{said}");
    assert!(!said.contains("os error"), "{said}");
    assert!(!said.contains("Caused by"), "{said}");
    assert!(!said.contains("exported "), "{said}");
    assert!(!out_path.exists());

    std::fs::write(&out_path, "{}\n").unwrap();
    let imported = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["memory", "import", out_path.to_str().unwrap()])
        .current_dir(&root)
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.path().join("tmp"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("VISSUE_ROOT", &root)
        .env("LJOS_SEAT", "lane-seat")
        .env("LJOS_TRACKER_GIT", "off")
        .env("PACKSET_URL", "off")
        .env("LJOS_OFF", "1")
        .env("LJOS_SHARE_FAULT", "estale")
        .output()
        .expect("ljos");
    assert_eq!(imported.status.code(), Some(1), "{}", fault(&imported));
    let said = fault(&imported);
    assert!(said.contains(&os_words(libc::ESTALE)), "{said}");
    assert!(
        said.contains(&format!("ljos memory import {}", out_path.display())),
        "{said}"
    );
    assert!(!said.contains("imported "), "{said}");
    assert!(!said.contains("os error"), "{said}");
}
