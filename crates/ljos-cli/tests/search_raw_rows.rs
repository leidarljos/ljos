//! Search prints the formatted page.
//!
//! Several seats share one tracker and one pack. A papers lesson and
//! receipt rows are in that pack. A pipe and a terminal both stay free of
//! raw tab rows. `--porcelain` is the tab separated form. `--json` is the
//! other one.

ljos_test_boot::harness!();

#[path = "common/pty.rs"]
mod pty;
#[path = "common/seat_env.rs"]
mod seat_env;
#[path = "common/tools.rs"]
mod tools;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
const QUERY: &str = "anneal cov-floor review gate silently passes";
const LESSON: &str = "The anneal cov-floor review gate silently passes when the floor is met.";
const PAPER_ID: &str = "3fe074ed0123456789abcdef01234567";
const RECEIPT_ROW: &str = "receipt\tmsg-anneal\tpapers\t2026-11-16T00:00:00Z";

struct World {
    root: PathBuf,
    port: u16,
    path: String,
    _daemon: ljos_test_boot::Packsetd,
}

fn tool_dir(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .find(|dir| dir.join(name).is_file())
        .map(|dir| dir.to_path_buf())
}

fn world() -> Option<World> {
    if tools::halt(tools::Tool::Packsetd) || tools::halt(tools::Tool::Vissue) {
        return None;
    }
    let packsetd = tool_dir("packsetd").expect("packsetd");
    let vissue_dir = tool_dir("vissue").expect("vissue");
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_ljos"))
        .parent()
        .expect("ljos dir");
    let mut dirs = vec![bin_dir.to_path_buf(), packsetd.clone(), vissue_dir];
    if let Some(claimdag) = tool_dir("claimdag") {
        dirs.push(claimdag);
    }
    if let Some(deedar) = tool_dir("deedar") {
        dirs.push(deedar);
    }
    dirs.push(PathBuf::from("/usr/bin"));
    dirs.push(PathBuf::from("/bin"));
    let path = std::env::join_paths(dirs)
        .expect("PATH")
        .into_string()
        .expect("utf8");
    let root = tempfile::tempdir().expect("root").keep();
    for sub in [
        "tmp",
        "run",
        "work",
        "home/.config/ljos",
        "home/.local/share/packset",
        "home/.local/share/deedar/store",
        "home/.local/state/ljos",
        "tracker/Software",
    ] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
    let pack_home = root.join("home/.local/share/packset");
    let mut started = ljos_test_boot::Packsetd::build();
    started.bin(packsetd.join("packsetd"));
    started.pack_at(&pack_home);
    started
        .command()
        .env("PACKSET_AUTH", "off")
        .env("HOME", root.join("home"))
        .env("PATH", &path)
        .env("TMPDIR", root.join("tmp"));
    let daemon = started.spawn();
    let world = World {
        port: daemon.port(),
        path,
        root,
        _daemon: daemon,
    };
    assert_ne!(world.port, 8761, "the shared writer took the default port");
    Some(world)
}

impl World {
    fn ljos(&self, seat: &str, args: &[&str]) -> std::process::Output {
        let mut cmd = self.command(seat, args);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.output().expect("ljos")
    }

    fn command(&self, seat: &str, args: &[&str]) -> Command {
        let home = self.root.join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &home)
            .env("USER", "anneal")
            .env("LOGNAME", "anneal")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", seat)
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_SEARCH_WAIT", "0")
            .env("LJOS_SEARCH_WARMUP_WAIT", "0")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_PORT", self.port.to_string())
            .env("PACKSET_URL", format!("http://127.0.0.1:{}", self.port))
            .env("PACKSET_HOME", home.join(".local/share/packset"))
            .env("NO_COLOR", "1")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8");
        seat_env::clear(&mut cmd);
        cmd.env("LJOS_SEAT", seat);
        cmd
    }
}

fn post_atom(port: u16, body: &str) {
    let mut sock = TcpStream::connect(("127.0.0.1", port)).expect("pack");
    let req = format!(
        "POST /v1/atoms HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    sock.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = sock.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.contains("HTTP/1.1 200") || text.contains("HTTP/1.1 201"),
        "atom was not stored: {text}"
    );
}

fn raw_tab_line(text: &str) -> Option<&str> {
    text.lines().find(|line| {
        let line = line.trim();
        line.contains('\t')
            && (line.starts_with("receipt\t")
                || line.starts_with("receipt ")
                || line.matches('\t').count() >= 2
                || line.split('\t').nth(1) == Some("papers"))
    })
}

fn assert_clean(label: &str, stdout: &str, stderr: &str) {
    assert!(
        raw_tab_line(stdout).is_none(),
        "{label} stdout has a raw row:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        raw_tab_line(stderr).is_none(),
        "{label} stderr has a raw row:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("receipt\t") && !stderr.contains("receipt\t"),
        "{label} printed a receipt row:\n{stdout}\n{stderr}"
    );
}

fn seed(world: &World) {
    post_atom(
        world.port,
        &format!(
            r#"{{"schema":"inside.atom/v1","id":"{PAPER_ID}","kind":"lesson","level":"explicit","text":"{LESSON}","workspace":"seat","entities":["seat:papers","horizon:standing"]}}"#
        ),
    );
    post_atom(
        world.port,
        r#"{"schema":"inside.atom/v1","id":"aa11bb22cc33dd44ee55ff6677889900","kind":"receipt","level":"explicit","text":"read msg-anneal\nby anneal-paper\nat 2026-11-16T00:00:00Z\nmsg aa11bb22\nanneal cov-floor review gate silently passes","workspace":"seat","entities":["seat:papers","from:papers","to:anneal-paper"]}"#,
    );
    post_atom(
        world.port,
        r#"{"schema":"inside.atom/v1","id":"bb11bb22cc33dd44ee55ff6677889900","kind":"message","level":"explicit","text":"from papers\nto anneal-paper\nmsg bb11bb22\nThe anneal cov-floor review gate silently passes on the paper.","workspace":"seat","entities":["seat:papers","from:papers","to:anneal-paper"]}"#,
    );
    let scribe = world.ljos(
        "scribe",
        &[
            "remember",
            "A scribe keeps a note that the review gate can fail closed.",
        ],
    );
    assert!(
        scribe.status.success(),
        "scribe remember: {}",
        text(&scribe)
    );
    let filed = world.ljos(
        "anneal-paper",
        &[
            "file",
            "anneal cov-floor review gate silently passes",
            "--top",
            "-p",
            "anneal",
        ],
    );
    assert!(filed.status.success(), "file: {}", text(&filed));
}

fn text(out: &std::process::Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn search_on_a_pipe_prints_the_page_and_no_raw_rows() {
    let Some(world) = world() else { return };
    seed(&world);
    let out = world.ljos("anneal-paper", &["search", QUERY]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{}", text(&out));
    assert_clean("pipe", &stdout, &stderr);
    assert!(
        stdout.contains("shown ") && stdout.contains(LESSON),
        "the formatted page lost the papers lesson:\n{stdout}\n{stderr}"
    );
    assert!(
        !stdout.contains("read msg-anneal") && !stderr.contains("read msg-anneal"),
        "a receipt line reached the page:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("3fe074ed") || stdout.contains("id "),
        "the page did not name an id:\n{stdout}"
    );

    let porcelain = world.ljos("anneal-paper", &["search", "--porcelain", QUERY]);
    let rows = String::from_utf8_lossy(&porcelain.stdout).into_owned();
    assert!(porcelain.status.success(), "{}", text(&porcelain));
    assert!(
        rows.contains('\t') && rows.contains(LESSON) && rows.contains("3fe074ed"),
        "porcelain did not print the papers row:\n{rows}"
    );
    assert!(
        !rows.contains("shown "),
        "porcelain printed the page:\n{rows}"
    );
    assert!(
        !rows.contains("receipt\t") && !rows.lines().any(|line| line.contains("\treceipt\t")),
        "porcelain printed a receipt:\n{rows}"
    );
    assert_clean(
        "porcelain stderr",
        "",
        &String::from_utf8_lossy(&porcelain.stderr),
    );

    let json = world.ljos("anneal-paper", &["search", "--json", QUERY]);
    let body = String::from_utf8_lossy(&json.stdout).into_owned();
    assert!(json.status.success(), "{}", text(&json));
    let parsed: serde_json::Value = serde_json::from_str(body.trim()).expect(&body);
    let hits = parsed["hits"].as_array().expect(&body);
    assert!(
        hits.iter().any(|hit| hit["text"].as_str() == Some(LESSON)),
        "{body}"
    );
    assert!(
        hits.iter()
            .all(|hit| hit["kind"].as_str() != Some("receipt")),
        "{body}"
    );
    assert_clean("json", &body, &String::from_utf8_lossy(&json.stderr));

    let mcp = mcp_search(&world);
    assert_clean("mcp", &mcp.0, &mcp.1);
    assert!(
        mcp.0.contains(LESSON) && !mcp.0.contains("read msg-anneal"),
        "mcp search leaked a receipt or dropped the lesson:\n{}\n{}",
        mcp.0,
        mcp.1
    );
}

fn mcp_search(world: &World) -> (String, String) {
    mcp_search_query(world, QUERY)
}

fn mcp_search_query(world: &World, query: &str) -> (String, String) {
    let home = world.root.join("home");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
    cmd.env_clear()
        .env("PATH", &world.path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", world.root.join("run"))
        .env("TMPDIR", world.root.join("tmp"))
        .env("LJOS_SEAT", "anneal-paper")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_SEARCH_WAIT", "0")
        .env("LJOS_SEARCH_WARMUP_WAIT", "0")
        .env("PACKSET_AUTH", "off")
        .env("PACKSET_PORT", world.port.to_string())
        .env("PACKSET_URL", format!("http://127.0.0.1:{}", world.port))
        .env("PACKSET_HOME", home.join(".local/share/packset"))
        .env("NO_COLOR", "1")
        .current_dir(world.root.join("work"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    seat_env::clear(&mut cmd);
    cmd.env("LJOS_SEAT", "anneal-paper");
    let mut child = cmd.spawn().expect("ljos-mcp");
    let mut stdin = child.stdin.take().unwrap();
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}"#;
    let noted = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let call = format!(
        "{init}\n{noted}\n{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"ljos_search\",\"arguments\":{{\"query\":\"{query}\",\"seat\":\"anneal-paper\"}}}}}}\n"
    );
    stdin.write_all(call.as_bytes()).unwrap();
    drop(stdin);
    let out = child.wait_with_output().expect("mcp");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn backend_receipt_rows_stay_off_page_and_mcp_but_json_keeps_them() {
    let Some(world) = world() else { return };
    seed(&world);
    let receipt_text = serde_json::to_string(RECEIPT_ROW).expect("receipt text");
    post_atom(
        world.port,
        &format!(
            r#"{{"schema":"inside.atom/v1","id":"cc11bb22cc33dd44ee55ff6677889900","kind":"lesson","level":"explicit","text":{receipt_text},"workspace":"seat","entities":["seat:papers","from:papers","to:anneal-paper"]}}"#
        ),
    );

    let page = world.ljos("anneal-paper", &["search", "anneal"]);
    let page_out = String::from_utf8_lossy(&page.stdout).into_owned();
    let page_err = String::from_utf8_lossy(&page.stderr).into_owned();
    assert!(page.status.success(), "{}", text(&page));
    assert_clean("page", &page_out, &page_err);
    assert!(
        !page_out.contains(RECEIPT_ROW) && !page_err.contains(RECEIPT_ROW),
        "search page leaked a backend receipt row:\n{page_out}\n{page_err}"
    );

    let json = world.ljos("anneal-paper", &["search", "--json", "anneal"]);
    let body = String::from_utf8_lossy(&json.stdout).into_owned();
    assert!(json.status.success(), "{}", text(&json));
    let parsed: serde_json::Value = serde_json::from_str(body.trim()).expect(&body);
    let rows = parsed["receipt_rows"].as_array().expect(&body);
    assert!(
        rows.iter().any(|line| line.as_str() == Some(RECEIPT_ROW)),
        "{body}"
    );
    let hits = parsed["hits"].as_array().expect(&body);
    assert!(
        hits.iter()
            .all(|hit| hit["text"].as_str() != Some(RECEIPT_ROW)),
        "{body}"
    );
    assert_clean(
        "json with receipt rows",
        &body,
        &String::from_utf8_lossy(&json.stderr),
    );

    let mcp = mcp_search_query(&world, "anneal");
    assert_clean("mcp with receipt rows", &mcp.0, &mcp.1);
    assert!(
        !mcp.0.contains(RECEIPT_ROW) && !mcp.1.contains(RECEIPT_ROW),
        "mcp search leaked a backend receipt row:\n{}\n{}",
        mcp.0,
        mcp.1
    );
}

#[test]
fn search_on_a_terminal_prints_the_page_and_no_raw_rows() {
    let Some(world) = world() else { return };
    seed(&world);
    let term = pty::Pty::open();
    let mut cmd = world.command("anneal-paper", &["search", QUERY]);
    cmd.stdin(Stdio::null());
    term.attach_output(&mut cmd);
    let child = term.spawn(&mut cmd);
    let out = pty::wait_output(child, 30);
    let shown = term.shown();
    assert!(out.status.success(), "{shown}\n{}", text(&out));
    assert_clean("terminal", &shown, "");
    assert!(
        shown.contains("shown ") && shown.contains("silently passes"),
        "the terminal page lost the lesson:\n{shown}"
    );
    assert!(
        !shown.contains("read msg-anneal"),
        "a receipt reached the terminal:\n{shown}"
    );
}

#[test]
fn recall_and_sitting_search_leave_raw_rows_off_the_page() {
    let Some(world) = world() else { return };
    seed(&world);
    let filed = String::from_utf8_lossy(
        &world
            .ljos(
                "anneal-paper",
                &["file", "anneal gate follow up", "--top", "-p", "anneal"],
            )
            .stdout,
    )
    .into_owned();
    let issue = filed.lines().next().unwrap_or("").trim();
    assert!(!issue.is_empty(), "no ticket id:\n{filed}");
    let recall = world.ljos("anneal-paper", &["recall", issue]);
    let stdout = String::from_utf8_lossy(&recall.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&recall.stderr).into_owned();
    assert!(recall.status.success(), "{}", text(&recall));
    assert_clean("recall", &stdout, &stderr);

    let sitting = world.ljos("anneal-paper", &["sitting", issue]);
    let stdout = String::from_utf8_lossy(&sitting.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&sitting.stderr).into_owned();
    assert!(sitting.status.success(), "{}", text(&sitting));
    let dump = stdout
        .lines()
        .find(|line| line.matches('\t').count() >= 3 || line.trim().starts_with("receipt\t"));
    assert!(
        dump.is_none(),
        "sitting search printed a raw row {dump:?}:\n{stdout}\n{stderr}"
    );
    assert!(
        !stderr.contains("receipt\t"),
        "sitting stderr printed a receipt row:\n{stderr}"
    );
}

#[test]
fn a_guard_line_is_not_a_raw_row_and_a_receipt_row_is() {
    let label = "allow\twarn\tcloud-cursor-push\ton a cloud seat the line skips or \
                 changes the git hooks, or writes a file the cloud guard reads";
    assert!(!ljos_cli::row_is_raw_tab(label));
    assert!(!ljos_cli::row_is_raw_tab("ok\tpack\tstate"));
    assert!(!ljos_cli::row_is_raw_tab("warn\tname\tstate"));
    assert!(!ljos_cli::row_is_raw_tab("info\tname\tstate"));
    assert!(ljos_cli::row_is_raw_tab(
        "receipt\tmsg-anneal\tpapers\t2026-01-01T00:00:00Z"
    ));
    assert!(ljos_cli::row_is_raw_tab(&format!(
        "{PAPER_ID}\tpapers\tunread\t{LESSON}"
    )));
    assert!(ljos_cli::row_is_raw_tab(
        "2026-01-01T00:00:00.000Z\tlesson\tx\tt"
    ));
}
