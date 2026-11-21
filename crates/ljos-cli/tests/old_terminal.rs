//! Terminal writes from v0.17 through v0.29, and lessons a runner held.
//!
//! Those releases left no `source.seat`. Harness and session are the login
//! user, via is `ljos`, and there is no turn. The current binary does not
//! stamp `source.basis` `person` on those atoms.
//!
//! Packset is seeded from `fixtures/old_terminal/legacy_atoms.json`. See
//! `fixtures/old_terminal/README.md` for how each payload was derived.

#![allow(
    clippy::disallowed_macros,
    reason = "a test writes its notes to the test harness, not to a person"
)]

ljos_test_boot::harness!();
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct LegacyFixture {
    terminal_by_version: Vec<TerminalVersion>,
    held_workspace: String,
    runner_lesson: HeldLesson,
    agent_lesson: HeldLesson,
}

#[derive(Deserialize)]
struct TerminalVersion {
    version: String,
    workspace: String,
    preference_text: String,
    lesson_text: String,
}

#[derive(Deserialize)]
struct HeldLesson {
    text: String,
    origin: String,
    harness: String,
    session: String,
}

struct Seat {
    home: tempfile::TempDir,
    run: tempfile::TempDir,
    url: String,
}

impl Seat {
    fn on(home: tempfile::TempDir, url: String) -> Self {
        Self {
            home,
            run: tempfile::tempdir().unwrap(),
            url,
        }
    }

    fn command(&self, bin: &str, args: &[&str]) -> Command {
        let path = format!("{}:/usr/bin:/bin", self.home.path().join("bin").display());
        let mut cmd = Command::new(bin);
        cmd.args(args)
            .current_dir(self.home.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.home.path())
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.run.path())
            .env("PACKSET_URL", &self.url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "8000")
            .env("LJOS_USER_RULES_SYNC", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn doctor(&self, bin: &str, workspace: &str) -> Output {
        stage_doctor_bins(self.home.path());
        self.command(bin, &["doctor", "--all"])
            .env("PACKSET_WORKSPACE", workspace)
            .env("PATH", doctor_path(self.home.path()))
            .output()
            .unwrap()
    }
}

fn fixture() -> LegacyFixture {
    serde_json::from_str(include_str!("fixtures/old_terminal/legacy_atoms.json"))
        .expect("legacy_atoms.json")
}

fn stage_doctor_bins(home: &Path) {
    use std::os::unix::fs::symlink;
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let vissue = bin.join("vissue");
    ljos_cli::install_executable(
        &vissue,
        "#!/bin/sh\nif [ \"$1\" = vote ]; then echo '--used --confidence'; else echo 'vissue 0.21.0'; fi\n",
    )
    .unwrap();
    let claimdag = bin.join("claimdag");
    ljos_cli::install_executable(&claimdag, "#!/bin/sh\necho 'claimdag 0.8.0'\n").unwrap();
    let curl = bin.join("curl");
    ljos_cli::install_executable(&curl, "#!/bin/sh\nexit 1\n").unwrap();
    for (name, src) in [
        ("ljos", env!("CARGO_BIN_EXE_ljos")),
        ("ljos-mcp", env!("CARGO_BIN_EXE_ljos-mcp")),
    ] {
        let dest = bin.join(name);
        if !dest.exists() {
            symlink(src, dest).unwrap();
        }
    }
}

fn doctor_path(home: &Path) -> String {
    let parent = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());
    format!("{}:{parent}", home.join("bin").display())
}

fn text_of(out: &Output) -> String {
    format!(
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn atoms(url: &str, workspace: &str) -> Vec<Value> {
    let got: Value = ureq::get(&format!("{url}/v1/atoms"))
        .query("workspace", workspace)
        .query("embedding", "omit")
        .call()
        .expect("GET /v1/atoms")
        .into_json()
        .expect("atoms json");
    got["atoms"].as_array().cloned().unwrap_or_default()
}

fn by_text<'a>(atoms: &'a [Value], needle: &str) -> &'a Value {
    atoms
        .iter()
        .find(|a| {
            a.get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains(needle))
                && a.get("tombstone") != Some(&Value::Bool(true))
        })
        .unwrap_or_else(|| panic!("no atom containing {needle}: {atoms:?}"))
}

fn basis(atom: &Value) -> Option<&str> {
    atom.pointer("/source/basis")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|b| !b.is_empty())
}

fn assert_not_person_stored(atom: &Value, label: &str) {
    assert!(
        !ljos_cli::person_stored(atom),
        "{label} must stay unverified without source.basis person: {atom}"
    );
}

fn assert_doctor_preferences(stdout: &str, state: &str) {
    assert!(
        stdout.contains(&format!("preferences\t{state}")),
        "doctor must report preferences\t{state}\n{stdout}"
    );
}

fn assert_legacy_preference_search(stdout: &str, preference: &Value, needle: &str) {
    let pack_id = preference
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("preference has no id: {preference}"));
    let search_id = ljos_cli::search_hit_id("preference", Some(pack_id), &[pack_id])
        .unwrap_or_else(|| panic!("search id for {pack_id}"));
    assert!(
        stdout.contains(needle),
        "search must show the preference text\n{stdout}"
    );
    assert!(
        stdout.contains(&search_id),
        "search must name the pack id {search_id}\n{stdout}"
    );
    assert!(
        stdout.contains("preference, from an agent, not checked"),
        "legacy preference must stay unverified in search\n{stdout}"
    );
}

fn post_atom(url: &str, body: Value) {
    let resp = ureq::post(&format!("{url}/v1/atoms"))
        .send_json(body)
        .expect("POST /v1/atoms");
    assert!(resp.status() < 300, "POST /v1/atoms: {}", resp.status());
}

fn post_terminal_pair(url: &str, workspace: &str, preference_text: &str, lesson_text: &str) {
    for (kind, text) in [("preference", preference_text), ("lesson", lesson_text)] {
        post_atom(
            url,
            serde_json::json!({
                "schema": "inside.atom/v1",
                "workspace": workspace,
                "kind": kind,
                "level": "explicit",
                "text": text,
                "origin": "user-declared",
                "source": {
                    "harness": "person",
                    "session": "person",
                    "via": "ljos",
                },
            }),
        );
    }
}

fn post_held(url: &str, workspace: &str, lesson: &HeldLesson) {
    post_atom(
        url,
        serde_json::json!({
            "schema": "inside.atom/v1",
            "workspace": workspace,
            "kind": "lesson",
            "level": "explicit",
            "text": lesson.text,
            "origin": lesson.origin,
            "source": {
                "harness": lesson.harness,
                "session": lesson.session,
                "via": "ljos",
            },
        }),
    );
}

fn assert_terminal_shape(atom: &Value) {
    assert_eq!(atom["origin"], "user-declared", "{atom}");
    assert!(basis(atom).is_none(), "{atom}");
    let source = atom["source"].as_object().expect("source");
    assert!(
        source.get("seat").is_none() || source["seat"].as_str().unwrap_or("").is_empty(),
        "v0.17–v0.29 stamped a seat: {source:?}"
    );
    assert_eq!(source["harness"], source["session"], "{source:?}");
    assert_eq!(source["via"], "ljos", "{source:?}");
    assert!(source.get("turn").is_none(), "{source:?}");
    assert!(!source["session"].as_str().unwrap_or("").contains('@'));
}

/// v0.17–v0.29 terminal writes stay unverified; doctor reports one preference
/// per workspace and does not promote them.
#[test]
fn legacy_terminal_atoms_stay_unverified_through_doctor() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let data = fixture();
    let home = tempfile::tempdir().unwrap();
    let mut daemon_builder = ljos_test_boot::Packsetd::build();
    daemon_builder.home_at(home.path());
    daemon_builder.command().env("PACKSET_AUTH", "off");
    let daemon = daemon_builder.spawn();
    let seat = Seat::on(home, daemon.url().to_string());
    let new = env!("CARGO_BIN_EXE_ljos");

    for row in &data.terminal_by_version {
        post_terminal_pair(
            daemon.url(),
            &row.workspace,
            &row.preference_text,
            &row.lesson_text,
        );

        let stored = atoms(daemon.url(), &row.workspace);
        let preference = by_text(&stored, &format!("{} notes", row.version));
        let remembered = by_text(&stored, &format!("{} staging", row.version));
        assert_eq!(preference["kind"], "preference", "{preference}");
        assert_eq!(remembered["kind"], "lesson", "{remembered}");
        assert_terminal_shape(preference);
        assert_terminal_shape(remembered);
        assert!(
            basis(preference).is_none(),
            "v{} preference was promoted",
            row.version
        );
        assert!(
            basis(remembered).is_none(),
            "v{} lesson was promoted",
            row.version
        );
        assert_not_person_stored(preference, "legacy preference");
        assert_not_person_stored(remembered, "legacy lesson");

        let doctor = seat.doctor(new, &row.workspace);
        assert!(
            doctor.status.success(),
            "v{} doctor must pass on legacy terminal atoms\n{}",
            row.version,
            text_of(&doctor)
        );
        let said = String::from_utf8_lossy(&doctor.stdout);
        assert_doctor_preferences(&said, "0 of 1 preference enforced by a rule");

        let after = atoms(daemon.url(), &row.workspace);
        let preference = by_text(&after, &format!("{} notes", row.version));
        let remembered = by_text(&after, &format!("{} staging", row.version));
        assert!(basis(preference).is_none(), "doctor promoted preference");
        assert!(basis(remembered).is_none(), "doctor promoted lesson");
        assert_not_person_stored(preference, "legacy preference after doctor");
        assert_not_person_stored(remembered, "legacy lesson after doctor");

        let needle = &format!("{} notes", row.version);
        let search = seat
            .command(new, &["search", needle])
            .env("PACKSET_WORKSPACE", &row.workspace)
            .output()
            .unwrap();
        let search_out = String::from_utf8_lossy(&search.stdout);
        assert!(
            search.status.success(),
            "legacy preference must be searchable\n{}",
            text_of(&search)
        );
        assert_legacy_preference_search(&search_out, preference, needle);
        assert_not_person_stored(preference, "legacy preference for search");
    }

    post_held(daemon.url(), &data.held_workspace, &data.runner_lesson);
    post_held(daemon.url(), &data.held_workspace, &data.agent_lesson);

    let before = atoms(daemon.url(), &data.held_workspace);
    let held = by_text(&before, "lint job");
    assert_eq!(held["kind"], "lesson", "{held}");
    assert_eq!(held["origin"], "user-declared", "{held}");
    assert!(basis(held).is_none(), "{held}");
    assert!(held["source"]["session"]
        .as_str()
        .unwrap_or("")
        .contains('@'));
    let agent_atom = by_text(&before, "deploy window");
    assert_eq!(agent_atom["origin"], "agent-derived", "{agent_atom}");

    let doctor = seat.doctor(new, &data.held_workspace);
    assert!(
        doctor.status.success(),
        "held lessons must not fail the doctor\n{}",
        text_of(&doctor)
    );
    let said = String::from_utf8_lossy(&doctor.stdout);
    assert_doctor_preferences(&said, "no preferences stored");

    let after = atoms(daemon.url(), &data.held_workspace);
    assert!(basis(by_text(&after, "lint job")).is_none());
    assert_eq!(by_text(&after, "deploy window")["origin"], "agent-derived");
    assert_ne!(basis(by_text(&after, "deploy window")), Some("person"));
}

/// Invariants the integration test above relies on; broken if `person_basis`
/// or `preferences_of` is relaxed.
#[test]
fn legacy_fixture_shape_is_not_person_stored() {
    let preference = serde_json::json!({
        "id": "legacy-pref",
        "origin": "user-declared",
        "kind": "preference",
        "text": "v0.21.0 notes",
        "source": { "harness": "person", "session": "person", "via": "ljos" },
    });
    assert!(!ljos_cli::person_stored(&preference));
    let prefs = ljos_cli::preferences_of(&[preference]);
    let (enforced, could, total) = ljos_cli::preferences_enforced(&prefs, &[]);
    assert_eq!(
        ljos_cli::preferences_line(enforced, could, total),
        "0 of 1 preference enforced by a rule"
    );
}
