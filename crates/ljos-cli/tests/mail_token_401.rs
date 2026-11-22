//! Mail commands report HTTP 401 as a refused seat token with the
//! `PACKSET_PORT` recovery line instead of saying the pack did not answer.

ljos_test_boot::harness!();

#[path = "common/seat_env.rs"]
mod seat_env;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn serve_401() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            sock.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let body = r#"{"error":"packsetd answers its owner only"}"#;
            let resp = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    port
}

#[test]
fn mail_commands_report_token_refusal_instead_of_silent_pack_on_401() {
    let port = serve_401();
    let home = tempfile::tempdir().expect("home");
    for sub in [
        "tmp",
        ".config",
        ".local/share",
        ".local/state",
        ".cache",
        "run",
    ] {
        std::fs::create_dir_all(home.path().join(sub)).expect("dirs");
    }
    let url = format!("http://127.0.0.1:{port}");
    let id = "00000000000000000000000000000000";
    for args in [
        vec!["inbox"],
        vec!["read", id],
        vec!["reply", id, "ack"],
        vec!["send", "bob", "hi"],
    ] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(&args)
            .env_clear()
            .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
            .current_dir(home.path())
            .env("HOME", home.path())
            .env("PATH", "/usr/bin:/bin")
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .env("XDG_DATA_HOME", home.path().join(".local/share"))
            .env("XDG_STATE_HOME", home.path().join(".local/state"))
            .env("XDG_CACHE_HOME", home.path().join(".cache"))
            .env("XDG_RUNTIME_DIR", home.path().join("run"))
            .env("TMPDIR", home.path().join("tmp"))
            .env("PACKSET_URL", &url)
            .stdin(Stdio::null());
        seat_env::clear(&mut cmd);
        cmd.env("LJOS_SEAT", "alice");
        let out = cmd.output().expect("ljos");
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            !out.status.success(),
            "ljos {args:?} succeeded unexpectedly:\nstdout:{stdout}\nstderr:{stderr}"
        );
        assert!(
            stderr.contains("refused this seat's token") && stderr.contains("PACKSET_PORT"),
            "ljos {args:?} missed token refusal:\nstdout:{stdout}\nstderr:{stderr}"
        );
        assert!(
            !stderr.contains("the pack did not answer"),
            "ljos {args:?} wrongly said the pack did not answer:\n{stderr}"
        );
    }
}
