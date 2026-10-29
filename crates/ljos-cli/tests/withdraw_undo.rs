//! ljos-3j43: `ljos withdraw` takes a note back without deleting it.
//!
//! The logbook keeps what was said. A withdrawal is another dated note
//! under it, so a reader sees the claim and that the seat no longer
//! stands behind it. A second withdrawal of the same note writes nothing.

ljos_test_boot::harness!();
use std::path::Path;
use std::process::{Command, Output};

const ISSUE: &str = "probe-aaaa";
const NOTE: &str = "the parser keeps the hyphen";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Hab {
    dir: tempfile::TempDir,
}

impl Hab {
    /// A stub tracker: `show --json` answers from `record.json`, `note`
    /// appends to the logbook in that file and to a call log.
    fn new(logbook: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("record.json");
        std::fs::write(
            &record,
            format!(
                r#"{{"id":"{ISSUE}","title":"a ticket","state":"STARTED","logbook":[{logbook}]}}"#
            ),
        )
        .unwrap();
        let log = dir.path().join("calls.log");
        script(
            dir.path(),
            "vissue",
            &format!(
                r#"#!/bin/sh
printf '%s\n' "vissue $*" >> '{log}'
if [ "$1" = show ]; then cat '{record}'; exit 0; fi
if [ "$1" = note ]; then
  python3 - "$3" <<'PY'
import json, sys
path = "{record}"
v = json.load(open(path))
v["logbook"].insert(0, {{"note": sys.argv[1], "timestamp": "[2026-10-20 Tue 09:00]"}})
json.dump(v, open(path, "w"))
PY
  exit 0
fi
exit 0
"#,
                log = log.display(),
                record = record.display()
            ),
        );
        script(dir.path(), "claimdag", "#!/bin/sh\nexit 1\n");
        for name in ["home", "run"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        Self { dir }
    }

    fn withdraw(&self, args: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .arg("withdraw")
            .args(args)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", self.dir.path().join("home"))
            .env(
                "XDG_CONFIG_HOME",
                self.dir.path().join("home").join(".config"),
            )
            .env(
                "XDG_STATE_HOME",
                self.dir.path().join("home").join(".state"),
            )
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("VISSUE_ROOT", self.dir.path().join("tracker"))
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("calls.log")).unwrap_or_default()
    }

    fn record(&self) -> serde_json::Value {
        let text = std::fs::read_to_string(self.dir.path().join("record.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn notes(v: &serde_json::Value) -> Vec<String> {
    v["logbook"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["note"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn a_withdrawn_note_stays_on_the_logbook_under_its_withdrawal() {
    let hab = Hab::new(&format!(r#"{{"note":"{NOTE}"}}"#));
    let out = hab.withdraw(&[ISSUE, "--note", "keeps the hyphen"]);
    assert!(out.status.success(), "{}", said(&out));
    assert!(
        hab.calls().contains(&format!(
            "vissue note {ISSUE} note-withdrawn: saddle {NOTE}"
        )),
        "{}",
        hab.calls()
    );
    let kept = notes(&hab.record());
    assert!(
        kept.iter().any(|n| n == NOTE),
        "the note was deleted: {kept:?}"
    );
    assert!(
        kept.iter()
            .any(|n| n == &format!("note-withdrawn: saddle {NOTE}")),
        "{kept:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("the note stays on the logbook"), "{stdout}");
}

#[test]
fn a_note_taken_back_twice_is_written_once() {
    let hab = Hab::new(&format!(
        r#"{{"note":"note-withdrawn: saddle {NOTE}"}},{{"note":"{NOTE}"}}"#
    ));
    let out = hab.withdraw(&[ISSUE, "--note", NOTE]);
    assert!(out.status.success(), "{}", said(&out));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("taken back already"),
        "{}",
        said(&out)
    );
    assert!(
        !hab.calls().contains("vissue note"),
        "a second withdrawal was written:\n{}",
        hab.calls()
    );
}

#[test]
fn a_text_that_names_two_notes_withdraws_neither() {
    let hab =
        Hab::new(r#"{"note":"the parser keeps the hyphen"},{"note":"the parser drops the dash"}"#);
    let out = hab.withdraw(&[ISSUE, "--note", "the parser"]);
    assert!(!out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("2 notes read"), "{}", said(&out));
    assert!(!hab.calls().contains("vissue note"), "{}", hab.calls());
}

#[test]
fn another_seats_note_is_not_this_seats_to_take_back() {
    let hab = Hab::new(&format!(
        r#"{{"note":"{NOTE}","raw":"- Noted by mallory [2026-10-20 Tue 09:00]"}}"#
    ));
    let out = hab.withdraw(&[ISSUE, "--note", NOTE]);
    assert!(!out.status.success(), "{}", said(&out));
    assert!(
        said(&out).contains("mallory wrote that note"),
        "{}",
        said(&out)
    );
    assert!(!hab.calls().contains("vissue note"), "{}", hab.calls());
}
