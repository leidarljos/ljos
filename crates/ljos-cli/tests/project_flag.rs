//! `ljos search` and `ljos remember` take `-p`, the same project a ticket lives in.
//!
//! Search keeps that project's tickets and lessons, and a lesson that names
//! no project. Remember records the project on the lesson. An unknown name
//! exits 1 and names the projects. A verb with no project names the verbs
//! that take `-p`.

ljos_test_boot::harness!();

#[path = "common/refused_port.rs"]
mod refused_port;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const FINANCE_TICKET: &str = "finance-6mrw";
const GARDEN_TICKET: &str = "garden-6mrw";
const VERBS: &str = "The verbs that take -p are calibrate, claims, file, handbook, list, prefer, ready, remember, rule, search and sitting.";

struct Seat {
    root: PathBuf,
    url: String,
    posts: Arc<Mutex<Vec<String>>>,
}

impl Seat {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        for sub in [
            "home/.config/ljos",
            "home/.local/state/ljos",
            "home/.local/share",
            "run",
            "work",
            "tracker/Software/finance",
            "tracker/Software/garden",
        ] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        write_issue(
            &root.join("tracker/Software/finance/issues.org"),
            "finance",
            FINANCE_TICKET,
            "kiln finance ledger",
        );
        write_issue(
            &root.join("tracker/Software/garden/issues.org"),
            "garden",
            GARDEN_TICKET,
            "kiln garden ledger",
        );
        let posts = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let recorded = Arc::clone(&posts);
        thread::spawn(move || serve(listener, recorded));
        Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
            posts,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with_url(args, &self.url)
    }

    fn run_with_url(&self, args: &[&str], url: &str) -> Output {
        self.launch(args, url, &[])
    }

    fn run_in_project(&self, args: &[&str], project: &str) -> Output {
        self.launch(args, &self.url, &[("LJOS_PROJECT", project)])
    }

    fn launch(&self, args: &[&str], url: &str, extra: &[(&str, &str)]) -> Output {
        let home = self.root.join("home");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_SEARCH_WAIT", "0")
            .env("LJOS_SEARCH_WARMUP_WAIT", "0")
            .env("LJOS_SEARCH_BUSY_WAIT", "0")
            .env("PACKSET_URL", url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("NO_COLOR", "1")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .envs(extra.iter().copied())
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap_or_else(|err| panic!("ljos {}: {err}", args.join(" ")));
    }

    fn mcp(&self, name: &str, arguments: serde_json::Value, url: &str) -> serde_json::Value {
        let home = self.root.join("home");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
            .current_dir(self.root.join("work"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", &self.root)
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "alice")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_SEARCH_WAIT", "0")
            .env("LJOS_SEARCH_WARMUP_WAIT", "0")
            .env("LJOS_SEARCH_BUSY_WAIT", "0")
            .env("PACKSET_URL", url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("LJOS_MCP_TOOLS", "all")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(30));
            #[expect(
                clippy::disallowed_methods,
                reason = "starts a program outside git hook code"
            )]
            let _ = Command::new("kill").arg(pid.to_string()).status();
        });
        let mut stdin = child.stdin.take().unwrap();
        for line in [
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "probe", "version": "0"}}
            }),
            serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": name, "arguments": arguments}
            }),
        ] {
            writeln!(stdin, "{line}").unwrap();
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|v| v["id"] == 2)
            .unwrap_or_else(|| panic!("no id=2 in {stdout}"))
    }
}

fn write_issue(path: &std::path::Path, project: &str, id: &str, title: &str) {
    let body = format!(
        "\
#+TITLE: {project} issues
#+VISSUE: 1
#+CATEGORY: {project}
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#B] {title}
:PROPERTIES:
:ID:         {id}
:CREATED:    [2026-11-01 Sat]
:END:
"
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(path, body).unwrap();
}

fn hit(id: &str, text: &str, entities: &[&str]) -> String {
    let entities = entities
        .iter()
        .map(|entity| format!("\"{entity}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"id":"{id}","kind":"lesson","text":"{text}","score":90,"ts":"2026-11-01T00:00:00.000Z","entities":[{entities}],"ballots":1,"of":1}}"#
    )
}

fn search_body() -> String {
    let hits = [
        hit(
            "lesson-finance",
            "kiln finance lesson",
            &["seat:alice", "project:finance"],
        ),
        hit(
            "lesson-garden",
            "kiln garden lesson",
            &["seat:alice", "project:garden"],
        ),
        hit("lesson-global", "kiln global lesson", &["seat:alice"]),
        hit(
            "lesson-finance-issue",
            "kiln finance issue lesson",
            &["seat:alice", "issue:finance-yyyy"],
        ),
        hit(
            "lesson-garden-issue",
            "kiln garden issue lesson",
            &["seat:alice", "issue:garden-zzzz"],
        ),
    ]
    .join(",");
    format!(r#"{{"hits":[{hits}],"rank":"relevance"}}"#)
}

fn serve(listener: TcpListener, posts: Arc<Mutex<Vec<String>>>) {
    let lessons = search_body();
    let status = r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#;
    for conn in listener.incoming().flatten() {
        let lessons = lessons.clone();
        let status = status.to_string();
        let posts = Arc::clone(&posts);
        thread::spawn(move || {
            let mut sock = conn;
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if request_complete(&buf) {
                            break;
                        }
                    }
                }
            }
            if buf.is_empty() {
                return;
            }
            let head_end = buf
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap_or(buf.len());
            let head = String::from_utf8_lossy(&buf[..head_end.min(buf.len())]).to_string();
            let request = head.split_whitespace().nth(1).unwrap_or("/");
            let path = request.split('?').next().unwrap_or("/");
            let method = head.split_whitespace().next().unwrap_or("");
            let raw_body = if head_end + 4 <= buf.len() {
                String::from_utf8_lossy(&buf[head_end + 4..]).into_owned()
            } else {
                String::new()
            };
            let body = if method == "POST" && path == "/v1/atoms" {
                posts.lock().unwrap().push(raw_body);
                r#"{"id":"atom-1","kind":"lesson","text":"stored"}"#.to_string()
            } else if path == "/v1/search" {
                lessons
            } else if path == "/v1/status" {
                status
            } else if path == "/v1/atoms" {
                r#"{"atoms":[]}"#.to_string()
            } else {
                r#"{"ok":true}"#.to_string()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        });
    }
}

fn request_complete(buf: &[u8]) -> bool {
    let Some(head_end) = buf.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&buf[..head_end]);
    let length = head.lines().find_map(|line| {
        let rest = line
            .split_once(':')
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))?
            .1
            .trim();
        rest.parse::<usize>().ok()
    });
    match length {
        Some(n) => buf.len() >= head_end + 4 + n,
        None => true,
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

fn page_keeps_finance(out: &Output) {
    let dump = said(out);
    assert!(out.status.success(), "{dump}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(FINANCE_TICKET), "{dump}");
    assert!(stdout.contains("kiln finance lesson"), "{dump}");
    assert!(stdout.contains("kiln global lesson"), "{dump}");
    assert!(stdout.contains("kiln finance issue lesson"), "{dump}");
    assert!(!stdout.contains(GARDEN_TICKET), "{dump}");
    assert!(!stdout.contains("kiln garden"), "{dump}");
}

#[test]
fn search_limits_rows_to_one_project() {
    let seat = Seat::open();
    let scoped = seat.run(&[
        "search",
        "kiln",
        "-p",
        "Finance",
        "--min-score",
        "0",
        "-n",
        "20",
    ]);
    page_keeps_finance(&scoped);
    let long = seat.run(&[
        "search",
        "--project",
        "finance",
        "--min-score",
        "0",
        "-n",
        "20",
        "kiln",
    ]);
    page_keeps_finance(&long);
    let open = seat.run(&["search", "kiln", "--min-score", "0", "-n", "20"]);
    let dump = said(&open);
    assert!(open.status.success(), "{dump}");
    let stdout = String::from_utf8_lossy(&open.stdout);
    assert!(stdout.contains(FINANCE_TICKET), "{dump}");
    assert!(stdout.contains(GARDEN_TICKET), "{dump}");
    assert!(stdout.contains("kiln garden lesson"), "{dump}");
}

#[test]
fn an_unknown_project_names_the_projects() {
    let seat = Seat::open();
    let line = "project: boats is not a project. The projects are finance, garden.";
    for args in [
        &["search", "kiln", "-p", "boats"][..],
        &[
            "remember",
            "The kiln door stays shut.",
            "--project",
            "boats",
        ][..],
    ] {
        let out = seat.run(args);
        let dump = said(&out);
        assert_eq!(out.status.code(), Some(1), "{dump}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.lines().any(|row| row == line),
            "expected one line {line:?}\n{dump}"
        );
    }
    assert!(
        seat.posts.lock().unwrap().is_empty(),
        "remember wrote a lesson for an unknown project"
    );
}

#[test]
fn remember_records_the_project_on_the_lesson() {
    let seat = Seat::open();
    let out = seat.run(&["remember", "-p", "Finance", "The kiln door stays shut."]);
    let dump = said(&out);
    assert!(out.status.success(), "{dump}");
    let posts = seat.posts.lock().unwrap();
    assert_eq!(posts.len(), 1, "{posts:?}\n{dump}");
    assert!(
        posts[0].contains("project:finance"),
        "the lesson has no project tag:\n{}\n{dump}",
        posts[0]
    );
    drop(posts);
    let plain = seat.run(&["remember", "The kiln door stays open."]);
    let dump = said(&plain);
    assert!(plain.status.success(), "{dump}");
    let posts = seat.posts.lock().unwrap();
    assert_eq!(posts.len(), 2, "{posts:?}");
    assert!(
        !posts[1].contains("project:"),
        "a lesson with no -p gained a project tag:\n{}",
        posts[1]
    );
}

fn user_rules(seat: &Seat) -> String {
    std::fs::read_to_string(seat.root.join("home/.config/ljos/user-rules.json")).unwrap_or_default()
}

#[test]
fn prefer_and_rule_record_a_project_and_tags_only_when_asked() {
    let seat = Seat::open();
    for args in [
        &["prefer", "Keep the kiln door shut."][..],
        &[
            "rule",
            "cargo publish *",
            "--verdict",
            "ask",
            "--why",
            "Ask first.",
        ][..],
    ] {
        let out = seat.run_in_project(args, "garden");
        assert!(out.status.success(), "{args:?}\n{}", said(&out));
    }
    {
        let posts = seat.posts.lock().unwrap();
        assert_eq!(posts.len(), 2, "{posts:?}");
        for body in posts.iter() {
            assert!(
                !body.contains("project:") && !body.contains("tag:"),
                "a preference or rule with no flag gained a scope:\n{body}"
            );
        }
    }
    let user = seat.run_in_project(
        &[
            "prefer",
            "--user",
            "--why",
            "The bench asked for it",
            "Write the kiln notes in the lab book.",
        ],
        "garden",
    );
    assert!(user.status.success(), "{}", said(&user));
    assert!(
        !user_rules(&seat).contains("project:"),
        "a user rule with no flag gained a project:\n{}",
        user_rules(&seat)
    );

    let scoped = seat.run(&[
        "prefer",
        "-p",
        "Finance",
        "--tag",
        "release",
        "Keep the kiln door open.",
    ]);
    assert!(scoped.status.success(), "{}", said(&scoped));
    let ruled = seat.run(&[
        "rule",
        "cargo test *",
        "--verdict",
        "ask",
        "--why",
        "Run the suite first.",
        "--project",
        "garden",
        "--tag",
        "ci",
    ]);
    assert!(ruled.status.success(), "{}", said(&ruled));
    let posts = seat.posts.lock().unwrap();
    assert_eq!(posts.len(), 4, "{posts:?}");
    assert!(
        posts[2].contains("project:finance") && posts[2].contains("tag:release"),
        "{}",
        posts[2]
    );
    assert!(
        posts[3].contains("project:garden") && posts[3].contains("tag:ci"),
        "{}",
        posts[3]
    );
    drop(posts);

    let user = seat.run(&[
        "prefer",
        "--user",
        "-p",
        "finance",
        "--tag",
        "release",
        "--why",
        "The bench asked for it",
        "Write the kiln logs in the lab book.",
    ]);
    assert!(user.status.success(), "{}", said(&user));
    let rules = user_rules(&seat);
    assert!(
        rules.contains("project:finance") && rules.contains("tag:release"),
        "{rules}"
    );
    let unknown = seat.run(&["prefer", "-p", "boats", "Keep the kiln door shut."]);
    assert_eq!(unknown.status.code(), Some(1), "{}", said(&unknown));
    let empty = seat.run(&["prefer", "--tag", " ", "Keep the kiln door shut."]);
    assert_eq!(empty.status.code(), Some(1), "{}", said(&empty));
    assert!(
        String::from_utf8_lossy(&empty.stderr).contains("prefer: --tag needs a name"),
        "{}",
        said(&empty)
    );
}

#[test]
fn queued_remember_keeps_the_project_when_the_pack_is_down() {
    let seat = Seat::open();
    let held = refused_port::RefusedPort::reserve();
    let dead = format!("http://127.0.0.1:{}", held.port());
    let queued = seat.run_with_url(
        &["remember", "-p", "Finance", "Queued kiln finance lesson."],
        &dead,
    );
    let dump = said(&queued);
    assert!(queued.status.success(), "{dump}");

    let mcp = seat.mcp(
        "ljos_remember",
        serde_json::json!({"seat": "alice", "project": "Garden", "text": "MCP queued garden lesson."}),
        &dead,
    );
    assert!(
        mcp.to_string().contains("queued for the next send"),
        "{mcp}"
    );

    let flushed = seat.run(&["search", "kiln"]);
    let dump = said(&flushed);
    assert!(flushed.status.success(), "{dump}");
    let posts = seat.posts.lock().unwrap();
    assert_eq!(posts.len(), 2, "{posts:?}\n{dump}");
    assert!(
        posts.iter().any(|body| {
            body.contains("Queued kiln finance lesson.") && body.contains("project:finance")
        }),
        "flushed CLI lesson lost project:finance:\n{posts:?}"
    );
    assert!(
        posts.iter().any(|body| {
            body.contains("MCP queued garden lesson.") && body.contains("project:garden")
        }),
        "flushed MCP lesson lost project:garden:\n{posts:?}"
    );
    drop(held);
}

fn flag_line(args: &[&str]) -> Output {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(args)
        .current_dir(root.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("LJOS_SEAT", "alice")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("VISSUE_NO_ROUTE", "1")
        .env("PACKSET_URL", "off")
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn project_flag_errors_cover_upper_p_long_project_and_start_alias() {
    let none = format!("ljos: {{verb}} has no flag {{shown}}. {VERBS}");
    let sitting = format!("ljos: sitting takes -p with --plan. {VERBS}");
    let mut rows: Vec<(Vec<&str>, String)> = Vec::new();
    for verb in ["note", "claim", "finish", "recall", "show"] {
        rows.push((
            vec![verb, "-p", "finance"],
            none.replace("{verb}", verb).replace("{shown}", "-p"),
        ));
        rows.push((
            vec![verb, "-P", "finance"],
            none.replace("{verb}", verb).replace("{shown}", "-P"),
        ));
        rows.push((
            vec![verb, "--project", "finance"],
            none.replace("{verb}", verb).replace("{shown}", "--project"),
        ));
        rows.push((
            vec![verb, "--project=finance"],
            none.replace("{verb}", verb).replace("{shown}", "--project"),
        ));
    }
    rows.push((
        vec!["sitting", FINANCE_TICKET, "-p", "finance"],
        sitting.clone(),
    ));
    rows.push((vec!["sitting", "-p", "finance"], sitting.clone()));
    rows.push((vec!["sitting", "-P", "finance"], sitting.clone()));
    rows.push((
        vec!["sitting", FINANCE_TICKET, "-P", "finance"],
        sitting.clone(),
    ));
    rows.push((vec!["start", "-p", "finance"], sitting.clone()));
    rows.push((vec!["start", FINANCE_TICKET, "-p", "finance"], sitting));
    let suggest = |verb: &str, run: &str| {
        format!("ljos: {verb} has no flag -P, and the closest is -p, so run `{run}`.")
    };
    rows.push((
        vec!["search", "-P", "finance", "kiln"],
        suggest("search", "ljos search -p <value>"),
    ));
    rows.push((
        vec!["remember", "-P", "finance", "The kiln."],
        suggest("remember", "ljos remember -p <value>"),
    ));
    rows.push((
        vec!["list", "-P", "finance"],
        suggest("list", "ljos list -p <value>"),
    ));
    rows.push((
        vec!["ready", "-P", "finance"],
        suggest("ready", "ljos ready -p <value>"),
    ));
    rows.push((
        vec!["file", "Title", "-P", "finance"],
        suggest("file", "ljos file <value> -p <value>"),
    ));
    rows.push((
        vec!["calibrate", "-P", "finance"],
        suggest("calibrate", "ljos calibrate -p <value>"),
    ));
    for (args, line) in rows {
        let out = flag_line(&args);
        let dump = said(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}\n{dump}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.lines().any(|row| row == line),
            "expected {line:?}\n{dump}"
        );
    }
}

#[test]
fn search_project_does_not_name_min_score_for_another_projects_weak_hit() {
    let mut seat = Seat::open();
    let weak_body = r#"{"hits":[{"id":"lesson-garden-weak","kind":"lesson","text":"The JOSS editor keeps review comments in document order beside the line they mark.","score":10.0,"cosine":0.3,"ts":"2026-11-01T00:00:00.000Z","entities":["seat:alice","project:garden"],"ballots":1,"of":1}],"rank":"relevance"}"#.to_string();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let status = r#"{"embedder":{"available":true,"answering":true,"enabled":true},"search":{"ranks":["fused","relevance"]}}"#;
        for conn in listener.incoming().flatten() {
            let weak_body = weak_body.clone();
            thread::spawn(move || {
                let mut sock = conn;
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if request_complete(&buf) {
                                break;
                            }
                        }
                    }
                }
                if buf.is_empty() {
                    return;
                }
                let head_end = buf
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .unwrap_or(buf.len());
                let head = String::from_utf8_lossy(&buf[..head_end]);
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/");
                let body = if path == "/v1/search" {
                    weak_body
                } else if path == "/v1/status" {
                    status.to_string()
                } else if path == "/v1/atoms" {
                    r#"{"atoms":[]}"#.to_string()
                } else {
                    r#"{"ok":true}"#.to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    seat.url = format!("http://127.0.0.1:{port}");

    let query = "ridley-core landing order rebase review";
    let finance = seat.run(&["search", query, "-p", "finance"]);
    let dump = said(&finance);
    assert!(finance.status.success(), "{dump}");
    let stdout = String::from_utf8_lossy(&finance.stdout);
    assert!(
        !stdout.contains("--min-score 0") && !stdout.contains("No relevant matches"),
        "finance search blamed min-score for garden's weak hit:\n{dump}"
    );

    let mcp = seat.mcp(
        "ljos_search",
        serde_json::json!({"query": query, "project": "finance"}),
        &seat.url,
    );
    let text = mcp.to_string();
    assert!(
        !text.contains("min_score 0") && !text.contains("No relevant matches"),
        "mcp finance search blamed min_score for garden's weak hit:\n{text}"
    );

    let garden = seat.run(&["search", query, "-p", "garden"]);
    let dump = said(&garden);
    assert!(garden.status.success(), "{dump}");
    let stdout = String::from_utf8_lossy(&garden.stdout);
    assert!(
        stdout.contains("No relevant matches") && stdout.contains("--min-score 0"),
        "garden search dropped its own weak hit without naming --min-score 0:\n{dump}"
    );
}
