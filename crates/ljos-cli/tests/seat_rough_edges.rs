//! CLI regressions for seat rough edges (ljos-2qgi).

ljos_test_boot::harness!();

use std::process::Command;

use packset_client::Hit;

fn write_hold(home: &std::path::Path, issue: &str) {
    let runtime = home.join("run/ljos");
    std::fs::create_dir_all(&runtime).unwrap();
    let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        runtime.join("hold-ada"),
        format!(
            "ada\nada\n{}\n{}\n2026-11-16T12:00:00Z\n{issue}\n-\n",
            std::process::id(),
            comm.trim()
        ),
    )
    .unwrap();
}

#[test]
fn search_names_finish_with_a_lesson_when_the_top_ticket_is_yours() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    write_hold(&home, "demo-kiln");
    // SAFETY: scoped to this test.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_RUNTIME_DIR", home.join("run"));
        std::env::set_var("LJOS_SEAT", "ada");
        std::env::set_var("LJOS_HOLD", "ada");
    }
    let hit = Hit {
        id: Some("demo-kiln".to_string()),
        text: "Ship the kiln".to_string(),
        score: 1.0,
        kind: "ticket".to_string(),
        ts: None,
        entities: vec!["issue:demo-kiln".to_string()],
        ballots: None,
        of: None,
        rerank_score: None,
        labels: Vec::new(),
    };
    let hint = ljos_cli::search_finish_hint(std::slice::from_ref(&hit));
    assert!(hint.is_some(), "{hint:?}");
    let line = hint.unwrap();
    assert!(line.contains("ljos finish demo-kiln --lesson"), "{line}");
    assert!(!line.contains("--status done"), "{line}");
}

struct Seat {
    dir: tempfile::TempDir,
}

impl Seat {
    fn new(identity: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        let stub = dir.path().join("vissue");
        ljos_cli::install_executable(
            &stub,
            format!(
                "#!/bin/sh\necho \"vissue $*\" >> '{log}'\ncase \"$1\" in\n  identity) printf '%s\\n' {identity} ;;\n  create) echo kiln-ab12 ;;\nesac\nexit 0\n",
                log = log.display(),
                identity = identity,
            ),
        )
        .unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("run")).unwrap();
        Self { dir }
    }

    fn ljos(&self, args: &[&str]) -> std::process::Output {
        let home = self.dir.path().join("home");
        let path = format!(
            "{}:{}",
            self.dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .current_dir(self.dir.path())
            .output()
            .unwrap();
    }
}

#[test]
fn file_top_defaults_to_the_seat_project() {
    let seat = Seat::new("prefix=demo");
    let out = seat.ljos(&["file", "a ticket", "--top"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(id, "kiln-ab12");
    let creates = std::fs::read_to_string(seat.dir.path().join("calls.log")).unwrap();
    assert!(creates.contains("vissue create -p demo"), "{creates}");
}

#[test]
fn file_top_without_a_seat_project_names_p() {
    let seat = Seat::new("'vissue test'");
    let out = seat.ljos(&["file", "a ticket", "--top"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("no project"), "{err}");
    assert!(err.contains("-p"), "{err}");
    assert!(String::from_utf8_lossy(&out.stdout).trim().is_empty());
}

#[test]
fn sitting_plan_accepts_top() {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["sitting", "--help"])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .unwrap();
    let help = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(help.contains("--top"), "{help}");
    assert!(help.contains("--plan"), "{help}");
}
