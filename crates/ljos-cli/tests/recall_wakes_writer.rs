//! Recall and a new session start the memory store themselves.
//!
//! A down writer used to be handed back as `packset ensure` for the agent
//! to run. Doctor already starts it. These two do too.

ljos_test_boot::harness!();
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "common/seat_root.rs"]
mod seat_root;

struct SeatHome(PathBuf);

impl SeatHome {
    fn new(name: &str) -> Self {
        let dir = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp)
            .join(format!("ljos-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("seat home");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for SeatHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fake_bin(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, body).expect(name);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

fn seat() -> (SeatHome, PathBuf, u16) {
    let home = SeatHome::new("recall-wake");
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let marker = home.path().join("ensure-called");
    fake_bin(
        &bin,
        "packset",
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 0\n", marker.display()),
    );
    fake_bin(&bin, "vissue", "#!/bin/sh\nexit 0\n");
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("probe")
        .local_addr()
        .expect("addr")
        .port();
    (home, marker, port)
}

fn ljos(home: &SeatHome, port: u16) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.env_clear()
        .current_dir(home.path())
        .env("HOME", home.path())
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", home.path().join("bin").display()),
        )
        .env("PACKSET_PORT", port.to_string())
        .env("PACKSET_TIMEOUT_MS", "300")
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("VISSUE_ROOT", home.path().join("tracker"))
        .env("VISSUE_NO_ROUTE", "1");
    cmd
}

#[test]
fn a_session_and_a_recall_start_the_writer_without_naming_the_command() {
    let (home, marker, port) = seat();
    std::fs::create_dir_all(home.path().join("run")).expect("run");
    let payload = format!(
        r#"{{"hook_event_name":"SessionStart","session_id":"wake-1","cwd":"{}"}}"#,
        home.path().display()
    );
    let mut hook = ljos(&home, port);
    hook.arg("hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = hook.spawn().expect("hook");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.as_bytes())
        .expect("write");
    let hook_out = child.wait_with_output().expect("hook wait");
    let hook_text = format!(
        "{}{}",
        String::from_utf8_lossy(&hook_out.stdout),
        String::from_utf8_lossy(&hook_out.stderr)
    );
    let calls = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(
        calls.lines().any(|l| l.trim() == "ensure"),
        "session start did not start the writer; calls: {calls:?}\n{hook_text}"
    );
    assert!(
        hook_text.contains("memory store does not answer"),
        "{hook_text}"
    );
    assert!(!hook_text.contains("packset ensure"), "{hook_text}");

    std::fs::write(&marker, "").expect("clear");
    let recall = ljos(&home, port)
        .args(["recall", "probe-0001"])
        .output()
        .expect("recall");
    let recall_text = format!(
        "{}{}",
        String::from_utf8_lossy(&recall.stdout),
        String::from_utf8_lossy(&recall.stderr)
    );
    let calls = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(
        calls.lines().any(|l| l.trim() == "ensure"),
        "recall did not start the writer; calls: {calls:?}\n{recall_text}"
    );
    assert!(!recall_text.contains("packset ensure"), "{recall_text}");
}
