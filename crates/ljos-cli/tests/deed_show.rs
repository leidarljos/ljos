//! `ljos deed --show` prints a stored file's bytes once they check out.
//!
//! The bytes are the blob in the deed store. A blob that was altered or
//! removed is refused, and nothing reaches stdout.

ljos_test_boot::harness!();

#[path = "common/deed_world.rs"]
mod deed_world;

use deed_world::{answer, mcp, said, stderr, stdout, World};

const CSV: &str = "model,score\nalpha,0.91\nbeta,0.87\n";

fn stored(world: &World) -> String {
    let path = world.write("scores.csv", CSV);
    let made = world
        .command("deedar")
        .args([
            "create", "file", "--name", "scores", "--path", &path, "--agent", "alice",
        ])
        .output()
        .unwrap();
    assert!(made.status.success(), "{}", said(&made));
    stdout(&made)
        .lines()
        .find_map(|line| line.strip_prefix("accession="))
        .unwrap()
        .to_string()
}

fn blob(world: &World) -> std::path::PathBuf {
    let dir = world.store().join("bytes");
    let entry = std::fs::read_dir(&dir).unwrap().flatten().next().unwrap();
    entry.path()
}

#[test]
fn show_prints_the_stored_bytes_and_out_writes_them() {
    let world = World::new();
    let accession = stored(&world);
    let shown = world.ljos(&["deed", "--show", &accession]);
    assert!(shown.status.success(), "{}", said(&shown));
    assert_eq!(stdout(&shown), CSV);

    let out = world.dir().join("copy.csv");
    let wrote = world.ljos(&[
        "deed",
        "--show",
        &accession,
        "--out",
        &out.display().to_string(),
    ]);
    assert!(wrote.status.success(), "{}", said(&wrote));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), CSV);
    let line = format!("wrote {} bytes of deed-file-scores", CSV.len());
    assert!(stdout(&wrote).starts_with(&line), "{}", said(&wrote));
}

#[test]
fn a_tampered_blob_is_refused_and_nothing_is_printed() {
    let world = World::new();
    let accession = stored(&world);
    let blob = blob(&world);
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&blob).unwrap().permissions());
    ljos_cli::change_mode(&blob, mode | 0o200).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&blob, CSV.replace("0.87", "0.99")).unwrap();

    let shown = world.ljos(&["deed", "--show", &accession]);
    assert_eq!(shown.status.code(), Some(1), "{}", said(&shown));
    assert!(stdout(&shown).is_empty(), "{}", said(&shown));
    let err = stderr(&shown);
    assert!(err.contains("deed-file-scores"), "{err}");
    assert!(err.contains("nothing was printed"), "{err}");
}

#[test]
fn a_missing_blob_is_refused_by_name() {
    let world = World::new();
    let accession = stored(&world);
    std::fs::remove_file(blob(&world)).unwrap();
    let shown = world.ljos(&["deed", "--show", &accession]);
    assert_eq!(shown.status.code(), Some(1), "{}", said(&shown));
    assert!(stdout(&shown).is_empty(), "{}", said(&shown));
    assert!(
        stderr(&shown).contains("nothing was printed"),
        "{}",
        said(&shown)
    );
}

#[test]
fn the_mcp_tool_reads_the_same_bytes() {
    let world = World::new();
    let accession = stored(&world);
    let answers = mcp(
        &world,
        &[
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": "ljos_deed_show", "arguments": {"accession": accession}}}),
        ],
    );
    let (failed, text) = answer(&answers, 1);
    assert!(!failed, "{text}");
    let csv = serde_json::json!(CSV).to_string();
    assert!(text.contains(&format!("\"text\":{csv}")), "{text}");
}
