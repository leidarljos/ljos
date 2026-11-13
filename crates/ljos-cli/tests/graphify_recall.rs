//! `ljos recall` adds a code context section from a graphify graph.
//!
//! The fixture is a minimised graph.json. With no file, recall's text does
//! not gain a section. `--no-graph` and `LJOS_GRAPHIFY=off` do the same
//! when the file is present.

ljos_test_boot::harness!();

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

const FIXTURE: &str = include_str!("fixtures/graphify/graph.json");

fn tracker(root: &Path) {
    let project = root.join("Software/cue");
    std::fs::create_dir_all(&project).expect("project dir");
    std::fs::write(
        project.join("issues.org"),
        "\
#+TITLE: cue issues
#+VISSUE: 1
#+CATEGORY: cue
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#B] MultiHeadAttention and LayerNorm
:PROPERTIES:
:ID:         cue-attn
:CREATED:    [2026-09-20 Sat]
:END:

* TODO [#B] Quay schedule
:PROPERTIES:
:ID:         cue-bare
:CREATED:    [2026-09-20 Sat]
:END:
",
    )
    .expect("issues.org");
}

fn stub_vissue(bin: &Path) {
    std::fs::create_dir_all(bin).expect("bin");
    let path = bin.join("vissue");
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("vissue stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

fn write_graph(dir: &Path) {
    std::fs::create_dir_all(dir).expect("graph dir");
    std::fs::write(dir.join("graph.json"), FIXTURE).expect("graph.json");
}

struct Seat {
    home: tempfile::TempDir,
}

impl Seat {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("seat home");
        tracker(home.path());
        stub_vissue(&home.path().join("bin"));
        std::fs::create_dir(home.path().join(".git")).expect(".git");
        Self { home }
    }

    fn recall(&self, args: &[&str], extra: &[(&str, &str)]) -> (bool, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.home.path())
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.home.path().join("bin").display()),
            )
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_STATE_HOME", self.home.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.home.path().join("run"))
            .env("VISSUE_ROOT", self.home.path())
            .env("ISSUE_ROOT", self.home.path())
            .env("VISSUE_NO_ROUTE", "1")
            .env("PACKSET_URL", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_CAPTURE", "off")
            .stdin(Stdio::null());
        for (key, value) in extra {
            cmd.env(key, value);
        }
        let out = cmd.output().expect("ljos recall");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), text)
    }
}

fn assert_code(text: &str) {
    assert!(text.contains("Code context"), "{text}");
    assert!(text.contains("MultiHeadAttention  model.py:10"), "{text}");
    assert!(text.contains("LayerNorm  model.py:20"), "{text}");
    assert!(text.contains("[EXTRACTED]"), "{text}");
    assert!(text.contains("[INFERRED]"), "{text}");
    assert!(text.contains("community 0"), "{text}");
    assert!(text.contains("degree 2"), "{text}");
}

#[test]
fn recall_without_a_graph_has_no_code_section() {
    let seat = Seat::new();
    let (ok, text) = seat.recall(&["recall", "cue-attn"], &[]);
    assert!(ok, "{text}");
    assert!(!text.contains("Code context"), "{text}");
    assert!(!text.contains("model.py"), "{text}");
}

#[test]
fn recall_with_the_fixture_lists_matching_nodes() {
    let seat = Seat::new();
    write_graph(&seat.home.path().join("graphify-out"));
    let (ok, text) = seat.recall(&["recall", "cue-attn"], &[]);
    assert!(ok, "{text}");
    assert!(text.contains("Code context  shown"), "{text}");
    assert_code(&text);
    let (ok, bare) = seat.recall(&["recall", "cue-bare"], &[]);
    assert!(ok, "{bare}");
    assert!(
        !bare.contains("Code context"),
        "an unmatched issue gained a section:\n{bare}"
    );
    let (ok, json) = seat.recall(&["recall", "--json", "cue-attn"], &[]);
    assert!(ok, "{json}");
    assert!(json.contains("model.py:10"), "{json}");
    assert!(json.contains("EXTRACTED"), "{json}");
    assert!(json.contains("INFERRED"), "{json}");
}

#[test]
fn recall_no_graph_and_env_off_leave_the_section_out() {
    let seat = Seat::new();
    write_graph(&seat.home.path().join("graphify-out"));
    let (ok, flagged) = seat.recall(&["recall", "--no-graph", "cue-attn"], &[]);
    assert!(ok, "{flagged}");
    assert!(!flagged.contains("Code context"), "{flagged}");
    let (ok, off) = seat.recall(&["recall", "cue-attn"], &[("LJOS_GRAPHIFY", "off")]);
    assert!(ok, "{off}");
    assert!(!off.contains("Code context"), "{off}");
}

#[test]
fn recall_reads_a_graph_path_from_the_environment_or_ljos_toml() {
    let seat = Seat::new();
    let custom = seat.home.path().join("custom");
    write_graph(&custom);
    let path = custom.join("graph.json");
    let (ok, from_env) = seat.recall(
        &["recall", "cue-attn"],
        &[("LJOS_GRAPHIFY", path.to_str().expect("utf8 path"))],
    );
    assert!(ok, "{from_env}");
    assert_code(&from_env);

    let seat = Seat::new();
    let custom = seat.home.path().join("custom");
    write_graph(&custom);
    std::fs::write(
        seat.home.path().join("ljos.toml"),
        "graphify = \"custom/graph.json\"\n",
    )
    .expect("ljos.toml");
    let (ok, from_toml) = seat.recall(&["recall", "cue-attn"], &[]);
    assert!(ok, "{from_toml}");
    assert_code(&from_toml);
}

#[test]
fn recall_refuses_an_unknown_schema_major() {
    let seat = Seat::new();
    let dir = seat.home.path().join("graphify-out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("graph.json"),
        r#"{"graph":{"schema_version":2,"graphify_version":"9.0.0"},"nodes":[],"links":[]}"#,
    )
    .unwrap();
    let (ok, text) = seat.recall(&["recall", "cue-attn"], &[]);
    assert!(ok, "{text}");
    assert!(text.contains("schema major 2 is not supported"), "{text}");
    assert!(text.contains("this ljos reads major 1"), "{text}");
    assert!(!text.contains("model.py"), "{text}");
}

#[test]
fn mcp_recall_includes_the_same_section() {
    let seat = Seat::new();
    write_graph(&seat.home.path().join("graphify-out"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
        .current_dir(seat.home.path())
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", seat.home.path().join("bin").display()),
        )
        .env("HOME", seat.home.path())
        .env("XDG_CONFIG_HOME", seat.home.path().join(".config"))
        .env("XDG_STATE_HOME", seat.home.path().join("state"))
        .env("XDG_RUNTIME_DIR", seat.home.path().join("run"))
        .env("VISSUE_ROOT", seat.home.path())
        .env("ISSUE_ROOT", seat.home.path())
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", "off")
        .env("LJOS_USER_RULES_SYNC", "off")
        .env("LJOS_CAPTURE", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("ljos-mcp");
    let pid = child.id();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        let _ = Command::new("kill").arg(pid.to_string()).status();
    });
    let mut stdin = child.stdin.take().unwrap();
    for line in [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"probe","version":"0"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"ljos_recall","arguments":{"issue":"cue-attn"}}}),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        !stderr.contains("panicked"),
        "stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert_code(&stdout);
}
