//! `ljos doctor --fix` repairs a missing home, a stale lock, a down writer
//! and a runner session with no seat recorded.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

const SCRUB: &[&str] = &[
    "CLAUDE_CODE_SESSION_ID",
    "CODEX_THREAD_ID",
    "CODEX_SESSION_ID",
    "CURSOR_CONVERSATION_ID",
    "LJOS_SEAT",
];

fn scrub() {
    for key in SCRUB {
        std::env::remove_var(key);
    }
}

fn root() -> PathBuf {
    tempfile::tempdir().unwrap().keep()
}

fn write_exe(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

struct Seat {
    home: PathBuf,
    bin: PathBuf,
}

impl Seat {
    fn new(name: &str) -> Self {
        scrub();
        let home = root().join(name);
        let bin = home.join("bin");
        for dir in [".config", ".local/share", ".cache", "run", "tmp", "bin"] {
            std::fs::create_dir_all(home.join(dir)).unwrap();
        }
        write_exe(
            &bin.join("curl"),
            "#!/bin/sh\necho 'curl: network is unreachable' >&2\nexit 6\n",
        );
        Self { home, bin }
    }

    fn state(&self) -> PathBuf {
        self.home.join(".local/state/ljos")
    }

    fn config(&self) -> PathBuf {
        self.home.join(".config/ljos/ljos.toml")
    }

    fn runtime(&self) -> PathBuf {
        self.home.join("run/ljos")
    }

    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> (i32, String, String) {
        scrub();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("HOME", &self.home)
            .env("USER", "ada")
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("TMPDIR", self.home.join("tmp"))
            .env("PACKSET_TIMEOUT_MS", "400")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off");
        for (key, value) in extra {
            cmd.env(key, value);
        }
        let out = cmd.stdin(Stdio::null()).output().unwrap();
        (
            out.status.code().unwrap_or(1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

fn row<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines()
        .find(|line| line.split('\t').nth(1) == Some(name))
}

fn dead_pid() -> u32 {
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    child.kill().unwrap();
    let _ = child.wait();
    pid
}

struct Live {
    child: Child,
    pid: u32,
}

impl Live {
    fn spawn() -> Self {
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        Self { child, pid }
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn doctor_fix_creates_a_missing_home_and_config() {
    let seat = Seat::new("home");
    let (_code, text, _) = seat.run(&["doctor"], &[]);
    let home = row(&text, "ljos home").unwrap_or("");
    let config = row(&text, "ljos config").unwrap_or("");
    assert!(
        home.starts_with("warn\t")
            && home.contains(&seat.state().display().to_string())
            && home.ends_with("run ljos doctor --fix"),
        "{text}"
    );
    assert!(
        config.starts_with("warn\t")
            && config.contains(&seat.config().display().to_string())
            && config.ends_with("run ljos doctor --fix"),
        "{text}"
    );
    assert!(text.contains("seat source"), "{text}");

    let (_, after, err) = seat.run(&["doctor", "--fix"], &[]);
    assert!(
        err.contains(&format!(
            "ljos: fixed ljos home: created {}",
            seat.state().display()
        )),
        "{err}"
    );
    assert!(
        err.contains(&format!(
            "ljos: fixed ljos config: wrote {} with defaults",
            seat.config().display()
        )),
        "{err}"
    );
    assert!(seat.state().is_dir(), "the ljos home was not created");
    let written = std::fs::read_to_string(seat.config()).unwrap();
    assert!(written.contains("keeps its default"), "{written}");
    assert!(!written.contains("email"), "{written}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(seat.config())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "config mode is {mode:04o}");
    }
    assert!(row(&after, "ljos home").is_none(), "{after}");
    assert!(row(&after, "ljos config").is_none(), "{after}");
    assert!(after.contains("seat source"), "{after}");
}

#[test]
fn doctor_fix_moves_a_stale_lock_aside_and_leaves_a_live_one() {
    let seat = Seat::new("locks");
    std::fs::create_dir_all(seat.state()).unwrap();
    std::fs::create_dir_all(seat.config().parent().unwrap()).unwrap();
    std::fs::write(seat.config(), "project = \"kept\"\n").unwrap();
    let gone = dead_pid();
    let live = Live::spawn();
    let pid_file = seat.state().join("writer.pid");
    let flight = seat.state().join("hookd.flight-ada");
    let live_file = seat.state().join("live.pid");
    let key = seat.state().join("age.key");
    let notes = seat.state().join("note-queue.jsonl");
    let outside = seat.home.join("outside.pid");
    let body = format!("{gone}\n");
    std::fs::write(&pid_file, &body).unwrap();
    std::fs::write(&flight, &body).unwrap();
    std::fs::write(&live_file, format!("{}\n", live.pid)).unwrap();
    std::fs::write(&key, &body).unwrap();
    std::fs::write(&notes, "{\"text\":\"keep\"}\n").unwrap();
    std::fs::write(&outside, &body).unwrap();
    std::os::unix::fs::symlink(&pid_file, seat.state().join("linked.pid")).unwrap();

    let (_, text, _) = seat.run(&["doctor"], &[]);
    let rows: Vec<_> = text
        .lines()
        .filter(|line| line.split('\t').nth(1) == Some("stale lock"))
        .collect();
    assert_eq!(rows.len(), 2, "{text}");
    assert!(
        rows.iter()
            .all(|line| line.ends_with("run ljos doctor --fix")),
        "{text}"
    );
    assert!(text.contains("seat source"), "{text}");

    let (_, after, err) = seat.run(&["doctor", "--fix"], &[]);
    assert!(err.contains("ljos: fixed stale lock: moved"), "{err}");
    assert!(err.matches("ljos: fixed stale lock:").count() == 2, "{err}");
    assert!(!pid_file.exists());
    assert_eq!(
        std::fs::read_to_string(seat.state().join("writer.pid.bak")).unwrap(),
        body
    );
    assert!(!flight.exists());
    assert_eq!(
        std::fs::read_to_string(seat.state().join("hookd.flight-ada.bak")).unwrap(),
        body
    );
    assert_eq!(
        std::fs::read_to_string(&live_file).unwrap(),
        format!("{}\n", live.pid)
    );
    assert_eq!(std::fs::read_to_string(&key).unwrap(), body);
    assert_eq!(
        std::fs::read_to_string(&notes).unwrap(),
        "{\"text\":\"keep\"}\n"
    );
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), body);
    assert!(seat
        .state()
        .join("linked.pid")
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        std::fs::read_to_string(seat.config()).unwrap(),
        "project = \"kept\"\n"
    );
    assert!(row(&after, "stale lock").is_none(), "{after}");
    assert!(after.contains("seat source"), "{after}");
}

#[test]
fn doctor_fix_starts_packsetd_with_packset_ensure() {
    let seat = Seat::new("pack");
    std::fs::create_dir_all(seat.state()).unwrap();
    std::fs::create_dir_all(seat.config().parent().unwrap()).unwrap();
    std::fs::write(seat.config(), CONFIG_KEEP).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let log = seat.home.join("packset.log");
    let pid = seat.home.join("pack.pid");
    let py = seat.home.join("pack.py");
    std::fs::write(&py, PACK_PY).unwrap();
    write_exe(&seat.bin.join("packset"), PACKSET_SH);
    write_exe(
        &seat.bin.join("packsetd"),
        "#!/bin/sh\necho 'packsetd 0.17.10'\nexit 0\n",
    );
    let run = |args: &[&str]| {
        scrub();
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", seat.bin.display()))
            .env("HOME", &seat.home)
            .env("USER", "ada")
            .env("XDG_CONFIG_HOME", seat.home.join(".config"))
            .env("XDG_DATA_HOME", seat.home.join(".local/share"))
            .env("XDG_STATE_HOME", seat.home.join(".local/state"))
            .env("XDG_CACHE_HOME", seat.home.join(".cache"))
            .env("XDG_RUNTIME_DIR", seat.home.join("run"))
            .env("TMPDIR", seat.home.join("tmp"))
            .env("PACKSET_TIMEOUT_MS", "400")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_PORT", port.to_string())
            .env("LJOS_TEST_PACK_LOG", &log)
            .env("LJOS_TEST_PACK_PID", &pid)
            .env("LJOS_TEST_PACK_PY", &py)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };

    let (text, err) = run(&["doctor"]);
    assert!(!log.exists(), "doctor started packset without --fix\n{err}");
    let pack = row(&text, "memory").unwrap_or("");
    assert!(
        pack.contains("packset ensure") && pack.ends_with("run ljos doctor --fix"),
        "{text}"
    );
    assert!(text.contains("seat source"), "{text}");

    let (after, err) = run(&["doctor", "--fix"]);
    if let Ok(text) = std::fs::read_to_string(&pid) {
        if let Ok(server) = text.trim().parse::<u32>() {
            let _ = Command::new("kill")
                .args(["-TERM", &server.to_string()])
                .status();
        }
    }
    assert!(
        err.contains("ljos: fixed pack: ran `packset ensure`"),
        "{err}"
    );
    assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "ensure");
    assert!(row(&after, "memory").is_none(), "{after}\n{err}");
    assert!(after.contains("seat source"), "{after}");
}

const CONFIG_KEEP: &str = "# kept\n";

#[test]
fn doctor_fix_records_a_runner_session_seat() {
    let seat = Seat::new("session");
    std::fs::create_dir_all(seat.state()).unwrap();
    std::fs::create_dir_all(seat.config().parent().unwrap()).unwrap();
    std::fs::write(seat.config(), CONFIG_KEEP).unwrap();
    let id = "cursor-conv-0199a1b2";
    let name = format!("cursor-{}", ljos_cli::session_tag(id));
    let (_, text, _) = seat.run(&["doctor"], &[("CURSOR_CONVERSATION_ID", id)]);
    let row_text = row(&text, "session seat").unwrap_or("");
    assert!(
        row_text.starts_with("warn\t")
            && row_text.contains("CURSOR_CONVERSATION_ID")
            && row_text.contains(&name)
            && row_text.ends_with("run ljos doctor --fix"),
        "{text}"
    );
    assert!(text.contains("seat source"), "{text}");
    let record_path = seat
        .runtime()
        .join(format!("session-{}", ljos_cli::session_tag(id)));
    if let Ok(before) = std::fs::read_to_string(&record_path) {
        assert!(
            !before.starts_with(&format!("{name}\n")),
            "doctor recorded the session seat without --fix:\n{before}"
        );
    }

    let (_, after, err) = seat.run(&["doctor", "--fix"], &[("CURSOR_CONVERSATION_ID", id)]);
    assert!(
        err.contains(&format!(
            "ljos: fixed session seat: recorded {name} for CURSOR_CONVERSATION_ID"
        )),
        "{err}"
    );
    let record = std::fs::read_to_string(
        seat.runtime()
            .join(format!("session-{}", ljos_cli::session_tag(id))),
    )
    .unwrap();
    assert!(record.starts_with(&format!("{name}\n")), "{record}");
    assert!(row(&after, "session seat").is_none(), "{after}\n{err}");
    assert!(after.contains("seat source"), "{after}");

    let (_, again, _) = seat.run(&["doctor"], &[("CURSOR_CONVERSATION_ID", id)]);
    assert!(row(&again, "session seat").is_none(), "{again}");
    let kept = std::fs::read_to_string(
        seat.runtime()
            .join(format!("session-{}", ljos_cli::session_tag(id))),
    )
    .unwrap();
    assert!(kept.starts_with(&format!("{name}\n")), "{kept}");
}

#[test]
fn doctor_fix_leaves_a_missing_packset_with_one_command() {
    let seat = Seat::new("nopack");
    std::fs::create_dir_all(seat.state()).unwrap();
    std::fs::create_dir_all(seat.config().parent().unwrap()).unwrap();
    std::fs::write(seat.config(), CONFIG_KEEP).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    scrub();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["doctor", "--fix"])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", seat.bin.display()))
        .env("HOME", &seat.home)
        .env("USER", "ada")
        .env("XDG_CONFIG_HOME", seat.home.join(".config"))
        .env("XDG_DATA_HOME", seat.home.join(".local/share"))
        .env("XDG_STATE_HOME", seat.home.join(".local/state"))
        .env("XDG_CACHE_HOME", seat.home.join(".cache"))
        .env("XDG_RUNTIME_DIR", seat.home.join("run"))
        .env("TMPDIR", seat.home.join("tmp"))
        .env("PACKSET_TIMEOUT_MS", "200")
        .env("PACKSET_PORT", port.to_string())
        .env("LJOS_HOOKD", "off")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(
            "ljos: left pack: packset is not on PATH, so run `cargo install --locked packset`"
        ),
        "{err}"
    );
    assert!(!err.contains("fixed pack"), "{err}");
}

const PACK_PY: &str = concat!(
    "import os\n",
    "from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer\n",
    "\n",
    "class Handler(BaseHTTPRequestHandler):\n",
    "    def do_GET(self):\n",
    "        body = b'ok'\n",
    "        if self.path.split('?')[0] == '/v1/status':\n",
    "            body = b'{\"embedder\":{\"available\":false},\"live\":0}'\n",
    "        self.send_response(200)\n",
    "        self.send_header('Content-Length', str(len(body)))\n",
    "        self.send_header('Connection', 'close')\n",
    "        self.end_headers()\n",
    "        self.wfile.write(body)\n",
    "\n",
    "    def log_message(self, fmt, *args):\n",
    "        return\n",
    "\n",
    "port = int(os.environ['PACKSET_PORT'])\n",
    "with open(os.environ['LJOS_TEST_PACK_PID'], 'w') as handle:\n",
    "    handle.write(str(os.getpid()))\n",
    "    handle.flush()\n",
    "ThreadingHTTPServer(('127.0.0.1', port), Handler).serve_forever()\n",
);

const PACKSET_SH: &str = concat!(
    "#!/bin/sh\n",
    "if [ \"$1\" = ensure ]; then\n",
    "  echo ensure >> \"$LJOS_TEST_PACK_LOG\"\n",
    "  setsid python3 \"$LJOS_TEST_PACK_PY\" >/dev/null 2>&1 &\n",
    "  i=0\n",
    "  while [ \"$i\" -lt 50 ]; do\n",
    "    python3 -c 'import os,socket; s=socket.create_connection((\"127.0.0.1\", int(os.environ[\"PACKSET_PORT\"])), 0.2); s.sendall(b\"GET /health HTTP/1.1\\r\\nHost: x\\r\\nConnection: close\\r\\n\\r\\n\"); raise SystemExit(0 if b\"200\" in s.recv(80) else 1)' && exit 0\n",
    "    i=$((i + 1))\n",
    "    sleep 0.05\n",
    "  done\n",
    "  exit 1\n",
    "fi\n",
    "echo 'packset 0.17.10'\n",
    "exit 0\n",
);
