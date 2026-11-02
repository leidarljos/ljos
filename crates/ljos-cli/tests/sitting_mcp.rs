//! A sitting claims when the MCP client names a binary that is not there.
//! The doctor still says so. Only the tracker, the claim graph and the
//! deed store can refuse, and a refusal exits nonzero.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn pack() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf);
            let line = req.lines().next().unwrap_or("");
            let body = if line.contains("/v1/atoms") {
                r#"{"atoms":[]}"#
            } else if line.contains("/v1/activate") {
                r#"{"island":[],"fired":0}"#
            } else if line.contains("/v1/status") || line.contains("/health") {
                r#"{"embedder":{"available":false}}"#
            } else {
                "{}"
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });
    port
}

struct Host {
    root: tempfile::TempDir,
    log: std::path::PathBuf,
    issue: String,
}

impl Host {
    fn open(deedar_ok: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("calls.log");
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        std::fs::write(
            home.join(".cursor/mcp.json"),
            r#"{"mcpServers":{"ljos":{"command":"/no/such/ljos-mcp"}}}"#,
        )
        .unwrap();
        let tracker = root.path().join("tracker");
        std::fs::create_dir_all(tracker.join("Software")).unwrap();
        script(
            root.path(),
            "vissue",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"vissue $*\" >> '{}'\ncase \"$1\" in\n  show) printf '%s\\n' '{{\"id\":\"probe-sit\",\"title\":\"probe\",\"state\":\"TODO\"}}' ;;\n  recall) echo noted ;;\n  identity) printf '%s\\n' 'vissue test' 'root={}' 'prefix=Software' ;;\nesac\nexit 0\n",
                log.display(),
                tracker.display()
            ),
        );
        script(
            root.path(),
            "claimdag",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"claimdag $*\" >> '{}'\ncase \"$1\" in\n  get) exit 1 ;;\nesac\necho '[]'\nexit 0\n",
                log.display()
            ),
        );
        let deed = if deedar_ok {
            "echo head\nexit 0\n"
        } else {
            "echo 'deedar down' >&2\nexit 1\n"
        };
        script(
            root.path(),
            "deedar",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"deedar $*\" >> '{}'\n{deed}",
                log.display()
            ),
        );
        let layout = vissue_core::Layout::new(&tracker, "Software");
        vissue_core::ops::create(&layout, "probe", "probe work", Default::default()).unwrap();
        let doc = vissue_core::IssueDoc::parse_file("probe", &layout.project_issues_path("probe"))
            .unwrap();
        let issue = doc.headings[0].id.clone();
        Self { root, log, issue }
    }

    fn ljos(&self, args: &[&str]) -> std::process::Output {
        let home = self.root.path().join("home");
        let path = format!("{}:/usr/bin:/bin", self.root.path().display());
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.root.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("VISSUE_ROOT", self.root.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_URL", format!("http://127.0.0.1:{}", pack()))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

#[test]
fn a_sitting_claims_when_mcp_json_names_a_missing_binary() {
    let host = Host::open(true);
    std::fs::create_dir_all(host.root.path().join("run")).unwrap();
    let out = host.ljos(&["sitting", &host.issue]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "sitting refused:\n{said}\ncalls:\n{}",
        host.calls()
    );
    assert!(said.contains("sitting opened without MCP tools"), "{said}");
    assert!(!said.contains("nothing was claimed"), "{said}");
    assert!(
        host.calls().contains("claimdag claim"),
        "the issue was not claimed:\n{}",
        host.calls()
    );
}

#[test]
fn a_refused_sitting_exits_nonzero() {
    let host = Host::open(false);
    std::fs::create_dir_all(host.root.path().join("run")).unwrap();
    let out = host.ljos(&["sitting", &host.issue]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "a refused sitting exited 0:\n{said}");
    assert!(said.contains("nothing was claimed"), "{said}");
    assert!(said.contains("deed store"), "{said}");
    assert!(
        !host.calls().contains("claimdag claim"),
        "a refused sitting claimed:\n{}",
        host.calls()
    );
}
