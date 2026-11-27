//! A stale `ljos-mcp` forwards a tool call to the installed binary through
//! `mcp_forward`; this drives that exchange against the built binary.

ljos_test_boot::harness!();
use std::path::Path;

/// Publish the script through [`ljos_cli::install_executable_mode`].
fn write_program(path: &Path, body: &str) {
    ljos_cli::install_executable_mode(path, body, 0o700).unwrap();
}

#[test]
fn a_child_request_can_share_the_tool_response_id() {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("server");
    write_program(
        &program,
        r#"#!/bin/sh
read -r initialize
read -r initialized
read -r call
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"elicitation/create","params":{"message":"confirm"}}'
read -r answer
printf '{"jsonrpc":"2.0","id":1,"result":%s}\n' "$answer"
"#,
    );
    let params = serde_json::json!({"name": "consent", "arguments": {}});
    let mut requests = 0;
    let answer = ljos_cli::mcp_forward_with_requests(
        &program,
        "TEST_FORWARDED",
        None,
        params.clone(),
        |method, params| {
            requests += 1;
            assert_eq!(method, "elicitation/create");
            assert_eq!(params["message"], "confirm");
            Ok(serde_json::json!({"action":"decline"}))
        },
    )
    .unwrap();
    assert_eq!(requests, 1);
    assert_eq!(answer["result"]["id"], 1);
    assert_eq!(answer["result"]["result"]["action"], "decline");

    let refused = ljos_cli::mcp_forward(&program, "TEST_FORWARDED", None, params).unwrap();
    assert_eq!(refused["result"]["error"]["code"], -32601);
    assert!(refused["result"].get("result").is_none());
}

#[test]
fn a_forwarded_call_is_answered_by_the_fresh_server() {
    let runtime = tempfile::tempdir().unwrap();
    let cards = tempfile::tempdir().unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(cards.path().join("USER.md"), "frozen by the test\n").unwrap();
    let tracker = tempfile::tempdir().unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        tracker.path().join("vissue.toml"),
        "seats = [\"acme-cli\"]\n",
    )
    .unwrap();
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    let program = runtime.path().join("server");
    write_program(
        &program,
        &format!(
            "#!/bin/sh\nexport XDG_RUNTIME_DIR={}\nexport LJOS_CARDS_DIR={}\n\
         export VISSUE_ROOT={}\nexport ISSUE_ROOT={}\nexport LJOS_SEAT=acme-cli\nexec {}\n",
            quote(runtime.path().to_str().unwrap()),
            quote(cards.path().to_str().unwrap()),
            quote(tracker.path().to_str().unwrap()),
            quote(tracker.path().to_str().unwrap()),
            quote(env!("CARGO_BIN_EXE_ljos-mcp")),
        ),
    );
    let params = serde_json::json!({"name": "ljos_cards", "arguments": {}});
    let init = serde_json::json!({"protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "acme-cli", "version": "1"}});
    let answer = ljos_cli::mcp_forward(&program, "LJOS_MCP_FORWARDED", Some(init), params).unwrap();
    assert_eq!(answer["id"], 1, "{answer}");
    assert!(
        answer["result"].to_string().contains("frozen by the test"),
        "{answer}"
    );
}

/// Arm an inotify watch for the open an exec of `path` performs.
#[cfg(target_os = "linux")]
fn watch_exec_open(path: &Path) -> i32 {
    // SAFETY: a new inotify descriptor, or -1. Nothing else is touched.
    let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
    assert!(fd >= 0, "inotify: {}", std::io::Error::last_os_error());
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: `name` is NUL-terminated and lives until the call returns.
    let wd = unsafe { libc::inotify_add_watch(fd, name.as_ptr(), libc::IN_OPEN) };
    assert!(
        wd >= 0,
        "inotify watch: {}",
        std::io::Error::last_os_error()
    );
    fd
}

/// Block until [`watch_exec_open`] sees an open. The event is queued before
/// exec returns, so pause until that return has delivered ETXTBSY.
#[cfg(target_os = "linux")]
fn wait_exec_open(fd: i32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut buf = [0u8; 4096];
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!left.is_zero(), "the forward did not exec the held script");
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = i32::try_from(left.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX);
        // SAFETY: one pollfd for the inotify descriptor this test owns.
        let rc = unsafe { libc::poll(&mut pfd, 1, ms) };
        if rc == 0 {
            continue;
        }
        assert!(rc > 0, "inotify poll: {}", std::io::Error::last_os_error());
        // SAFETY: `buf` is ours and `fd` is the inotify descriptor.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 {
            break;
        }
        let err = std::io::Error::last_os_error();
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::Interrupted,
            "inotify read: {err}"
        );
    }
    // SAFETY: `fd` is the descriptor this test opened.
    unsafe { libc::close(fd) };
    std::thread::sleep(std::time::Duration::from_millis(50));
}

/// Drop `held` only after the forward has exec'd `path` and seen it busy.
/// A probe spawn in this test used to win that race and drop the writer
/// before `mcp_forward` ran. `watch` is armed before that exec.
fn release_after_busy_exec(
    watch: i32,
    held: std::fs::File,
    started: &std::sync::atomic::AtomicBool,
) {
    #[cfg(target_os = "linux")]
    {
        let _ = started;
        wait_exec_open(watch);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = watch;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !started.load(std::sync::atomic::Ordering::Acquire) {
            assert!(
                std::time::Instant::now() < deadline,
                "the forward did not start"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Spawn is the next call. One attempt returns well inside this
        // pause, and the retry budget is still open when it ends.
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    drop(held);
}

/// A writer still holds the script. Exec answers ETXTBSY until that handle
/// closes. The forward waits it out and still gets the answer.
#[test]
fn a_script_held_open_for_writing_still_answers() {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("server");
    write_program(
        &program,
        r#"#!/bin/sh
read -r initialize
read -r initialized
read -r call
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"busy-ok"}]}}'
"#,
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "opens a path that is not a git hook"
    )]
    let held = std::fs::OpenOptions::new()
        .write(true)
        .open(&program)
        .unwrap();
    #[cfg(target_os = "linux")]
    let watch = watch_exec_open(&program);
    #[cfg(not(target_os = "linux"))]
    let watch = -1;
    let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&started);
    let called = program.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        flag.store(true, std::sync::atomic::Ordering::Release);
        let answer = ljos_cli::mcp_forward(
            &called,
            "TEST_FORWARDED",
            None,
            serde_json::json!({"name": "ping", "arguments": {}}),
        );
        let _ = tx.send(answer);
    });
    release_after_busy_exec(watch, held, &started);
    let answer = rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .expect("the forward did not finish")
        .unwrap();
    assert!(answer.to_string().contains("busy-ok"), "{answer}");
    worker.join().unwrap();
}
