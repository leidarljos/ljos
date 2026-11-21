//! A new ticket with no `-p`, no `--parent` and no held issue goes in the
//! seat's home project, the `project` key of its row in `vissue.toml`.
//! With none, the command refuses and names `-p PROJECT`. On a shared
//! tracker whose prefix is `Software`, `--top` and `--plan` used to file
//! into a project named after the prefix, `Software/Software`, which
//! every seat then piled its tickets into.

ljos_test_boot::harness!();
use std::io::{BufRead, BufReader, Write as _};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const HOME_ROW: &str = "\n[seats.rohit]\nproject = \"inkwell\"\n";

struct Seat {
    dir: tempfile::TempDir,
}

impl Seat {
    /// A tracker with prefix `Software`, one ticket `kiln-aaaa`, and the
    /// extra `vissue.toml` text `row`.
    fn new(row: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let stub = bin.join("vissue");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\necho \"vissue $*\" >> '{log}'\ncase \"$1\" in\n  identity) printf '%s\\n' 'vissue test' 'prefix=Software' ;;\n  create) echo stub-id ;;\nesac\nexit 0\n",
                log = log.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir_all(dir.path().join("home/run")).unwrap();
        let tracker = dir.path().join("tracker");
        let kiln = tracker.join("Software/kiln");
        std::fs::create_dir_all(&kiln).unwrap();
        std::fs::write(
            tracker.join("vissue.toml"),
            format!("prefix = \"Software\"\n{row}"),
        )
        .unwrap();
        std::fs::write(
            kiln.join("issues.org"),
            "\
#+TITLE: kiln
#+VISSUE: 1
#+CATEGORY: kiln
#+TODO: TODO STARTED | DONE CANCELLED

* TODO [#C] Fire the kiln
:PROPERTIES:
:ID:         kiln-aaaa
:END:
",
        )
        .unwrap();
        Self { dir }
    }

    fn tracker(&self) -> PathBuf {
        self.dir.path().join("tracker")
    }

    /// Put a second ticket, `kiln-bbbb`, on the kiln board.
    fn second_kiln_ticket(&self) {
        let board = self.tracker().join("Software/kiln/issues.org");
        let mut text = std::fs::read_to_string(&board).unwrap();
        text.push_str("\n* TODO [#C] Cool the kiln\n:PROPERTIES:\n:ID:         kiln-bbbb\n:END:\n");
        std::fs::write(&board, text).unwrap();
    }

    /// A hold record that names `issue` for a child of `pid`, whose
    /// command name is `comm`.
    fn hold_for(&self, issue: &str, pid: u32, comm: &str) {
        let runtime = self.dir.path().join("home/run/ljos");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::write(
            runtime.join("hold-rohit"),
            format!("rohit\nrohit\n{pid}\n{comm}\n2026-11-03T00:00:00Z\n{issue}\n-\n"),
        )
        .unwrap();
    }

    /// A hold this test process left, so the `ljos` it runs is its child.
    fn hold(&self, issue: &str) {
        let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
        self.hold_for(issue, std::process::id(), comm.trim());
    }

    fn path(&self) -> String {
        let ljos = Path::new(env!("CARGO_BIN_EXE_ljos")).parent().unwrap();
        format!(
            "{}:{}:{}",
            self.dir.path().join("bin").display(),
            ljos.display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    fn env(&self, cmd: &mut Command) {
        let home = self.dir.path().join("home");
        cmd.env_clear()
            .env("PATH", self.path())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("VISSUE_ROOT", self.tracker())
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "rohit")
            .env("LJOS_HOLD", "rohit")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .current_dir(self.tracker());
    }

    fn ljos(&self, args: &[&str]) -> std::process::Output {
        self.ljos_with(args, &[])
    }

    fn ljos_with(&self, args: &[&str], extra: &[(&str, &str)]) -> std::process::Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args);
        self.env(&mut cmd);
        cmd.envs(extra.iter().copied());
        cmd.output().unwrap()
    }

    /// Remove the kiln board, so the tracker has no `issues.org` at all.
    fn without_boards(&self) {
        std::fs::remove_dir_all(self.tracker().join("Software/kiln")).unwrap();
    }

    fn hold_path(&self) -> PathBuf {
        self.dir.path().join("home/run/ljos/hold-rohit")
    }

    fn creates(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("calls.log"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.starts_with("vissue create"))
            .map(str::to_string)
            .collect()
    }

    /// Whether the kiln board has a heading `title` whose `:PARENT:` is
    /// `kiln-aaaa`.
    fn held_plan_in_kiln(&self, title: &str) -> bool {
        let board =
            std::fs::read_to_string(self.tracker().join("Software/kiln/issues.org")).unwrap();
        board.split("\n* ").any(|heading| {
            heading
                .lines()
                .next()
                .is_some_and(|line| line.contains(title))
                && heading
                    .lines()
                    .any(|line| line.trim() == ":PARENT:     kiln-aaaa")
        })
    }

    fn projects(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.tracker().join("Software"))
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| entry.path().is_dir())
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn no_p_and_no_hold_files_in_the_seat_home_project() {
    let seat = Seat::new(HOME_ROW);
    for args in [
        &["file", "a ticket"][..],
        &["file", "a top ticket", "--top"],
    ] {
        let out = seat.ljos(args);
        assert!(out.status.success(), "{args:?}: {}", text(&out));
    }
    let creates = seat.creates();
    assert_eq!(creates.len(), 2, "{creates:?}");
    for create in &creates {
        assert!(
            create.starts_with("vissue create -p inkwell "),
            "{creates:?}"
        );
        assert!(!create.contains("--parent"), "{creates:?}");
    }
}

#[test]
fn no_home_project_refuses_and_names_p() {
    let seat = Seat::new("");
    for args in [
        &["file", "a ticket"][..],
        &["file", "a top ticket", "--top"],
    ] {
        let out = seat.ljos(args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {err}");
        assert!(out.stdout.is_empty(), "{args:?}: {}", text(&out));
        assert_eq!(err.trim_end().lines().count(), 1, "{args:?}: {err}");
        assert!(err.contains("-p PROJECT"), "{args:?}: {err}");
        assert!(err.contains("[seats.rohit]"), "{args:?}: {err}");
    }
    assert!(seat.creates().is_empty(), "{:?}", seat.creates());
}

#[test]
fn a_plan_with_no_p_goes_in_the_home_project_or_is_refused() {
    let none = Seat::new("");
    let out = none.ljos(&["sitting", "--plan", "a plan", "--", "an item"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("-p PROJECT"), "{err}");
    assert!(
        none.projects().iter().all(|p| p == "kiln"),
        "{:?}",
        none.projects()
    );

    let homed = Seat::new(HOME_ROW);
    let out = homed.ljos(&["sitting", "--plan", "a plan", "--", "an item"]);
    let said = text(&out);
    assert!(said.contains("inkwell-"), "{said}");
    assert_eq!(homed.projects(), ["inkwell", "kiln"], "{said}");
}

#[test]
fn a_parent_in_kiln_files_in_kiln() {
    let seat = Seat::new(HOME_ROW);
    let out = seat.ljos(&["file", "a child", "--parent", "kiln-aaaa"]);
    assert!(out.status.success(), "{}", text(&out));
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].starts_with("vissue create -p kiln --parent kiln-aaaa "),
        "{creates:?}"
    );
}

#[test]
fn a_held_issue_in_kiln_files_in_kiln() {
    let seat = Seat::new(HOME_ROW);
    seat.hold("kiln-aaaa");
    let out = seat.ljos(&["file", "a child"]);
    assert!(out.status.success(), "{}", text(&out));
    let creates = seat.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].starts_with("vissue create -p kiln --parent kiln-aaaa "),
        "{creates:?}"
    );
    let out = seat.ljos(&["sitting", "--plan", "a held plan", "--", "an item"]);
    let said = text(&out);
    assert_eq!(seat.projects(), ["kiln"], "{said}");
    assert!(seat.held_plan_in_kiln("a held plan"), "{said}");
}

#[test]
fn a_hold_that_matches_two_tickets_files_no_plan() {
    let seat = Seat::new(HOME_ROW);
    seat.second_kiln_ticket();
    seat.hold("kil");
    let file = seat.ljos(&["file", "a child"]);
    let file_err = String::from_utf8_lossy(&file.stderr);
    assert_eq!(file.status.code(), Some(1), "{file_err}");
    assert!(file_err.contains("kiln-aaaa and kiln-bbbb"), "{file_err}");
    let out = seat.ljos(&["sitting", "--plan", "ambiguous plan", "--", "one item"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(err.contains("kiln-aaaa and kiln-bbbb"), "{err}");
    assert!(out.stdout.is_empty(), "{}", text(&out));
    assert_eq!(seat.projects(), ["kiln"], "{}", text(&out));
    let board = std::fs::read_to_string(seat.tracker().join("Software/kiln/issues.org")).unwrap();
    assert!(!board.contains("ambiguous plan"), "{board}");
    assert!(seat.creates().is_empty(), "{:?}", seat.creates());
}

/// A tracker with no `issues.org` cannot have the held id, so the hold
/// is named and not used: the home project takes the ticket or the plan,
/// and with no home project the refusal names the hold and keeps it.
#[test]
fn a_gone_hold_on_a_tracker_with_no_board_is_named_and_not_used() {
    let homed = Seat::new(HOME_ROW);
    homed.without_boards();
    homed.hold("gone-zzzz");
    let out = homed.ljos(&["file", "a ticket"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        err.contains("held issue gone-zzzz is gone; filed at the top of inkwell"),
        "{err}"
    );
    let creates = homed.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    assert!(
        creates[0].starts_with("vissue create -p inkwell "),
        "{creates:?}"
    );
    assert!(!creates[0].contains("--parent"), "{creates:?}");
    assert!(
        !homed.hold_path().exists(),
        "a filed ticket kept the gone hold"
    );

    homed.hold("gone-zzzz");
    let out = homed.ljos(&["sitting", "--plan", "a plan", "--", "an item"]);
    let said = text(&out);
    assert!(
        said.contains("held issue gone-zzzz is gone; filed at the top of inkwell"),
        "{said}"
    );
    assert_eq!(homed.projects(), ["inkwell"], "{said}");

    let none = Seat::new("");
    none.without_boards();
    none.hold("gone-zzzz");
    for args in [
        &["file", "a ticket"][..],
        &["sitting", "--plan", "a plan", "--", "an item"],
    ] {
        let out = none.ljos(args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {}", text(&out));
        assert!(
            err.starts_with("held issue gone-zzzz is gone, so it is not the parent; no project: "),
            "{args:?}: {err}"
        );
        assert!(none.hold_path().exists(), "{args:?}: the hold was cleared");
        assert!(
            none.projects().is_empty(),
            "{args:?}: {:?}",
            none.projects()
        );
    }
    assert!(none.creates().is_empty(), "{:?}", none.creates());
}

/// A bad name from `LJOS_PROJECT` or the seat row is refused after the
/// gone hold is named, as a bad `-p` is.
#[test]
fn a_bad_default_project_refusal_names_the_gone_hold() {
    let env = Seat::new("");
    env.hold("gone-zzzz");
    let row = Seat::new("\n[seats.rohit]\nproject = \"foo/bar\"\n");
    row.hold("gone-zzzz");
    for (seat, extra) in [(&env, &[("LJOS_PROJECT", "foo/bar")][..]), (&row, &[])] {
        let out = seat.ljos_with(&["file", "a ticket"], extra);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{extra:?}: {}", text(&out));
        assert_eq!(
            err.trim_end(),
            "held issue gone-zzzz is gone, so it is not the parent; project: pass -p bar, not -p foo/bar",
            "{extra:?}"
        );
        assert!(seat.hold_path().exists(), "{extra:?}: the hold was cleared");
        assert!(seat.creates().is_empty(), "{:?}", seat.creates());
    }
}

/// `ljos sitting --plan` names a gone hold in its refusal and keeps it,
/// and files nothing.
#[test]
fn a_plan_refusal_names_the_gone_hold() {
    let seat = Seat::new("");
    seat.hold("gone-zzzz");
    let out = seat.ljos(&["sitting", "--plan", "a plan", "--", "an item"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(out.stdout.is_empty(), "{}", text(&out));
    assert!(
        err.starts_with("held issue gone-zzzz is gone, so it is not the parent; no project: "),
        "{err}"
    );
    assert!(err.contains("-p PROJECT"), "{err}");
    assert!(seat.hold_path().exists(), "a refused plan cleared the hold");
    assert_eq!(seat.projects(), ["kiln"], "{err}");
}

/// `--top` does not read the hold: its refusal does not name it, a filed
/// ticket does not clear it, and the help says so.
#[test]
fn top_does_not_read_the_hold() {
    let seat = Seat::new("");
    seat.hold("gone-zzzz");
    let out = seat.ljos(&["file", "a ticket", "--top"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(!err.contains("gone-zzzz"), "{err}");
    let out = seat.ljos(&["file", "a ticket", "--top", "-p", "demo"]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(!said.contains("gone-zzzz"), "{said}");
    assert!(seat.hold_path().exists(), "--top cleared the hold");
    let help = seat.ljos(&["file", "--help"]);
    let help = String::from_utf8_lossy(&help.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        help.contains("`--top` does not read the held issue"),
        "{help}"
    );
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Session {
    fn open(seat: &Seat) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
        seat.env(&mut cmd);
        cmd.env("LJOS_MCP_ORPHAN_IDLE", "off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            let mut line = String::new();
            while stdout.read_line(&mut line).unwrap_or(0) > 0 {
                if tx.send(line.clone()).is_err() {
                    break;
                }
                line.clear();
            }
        });
        let mut session = Self {
            child,
            stdin,
            lines,
            next_id: 1,
        };
        session.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "probe", "version": "0"}
            }),
        );
        session.send(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        session
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn send(&mut self, value: &serde_json::Value) {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(30))
                .unwrap_or_else(|err| panic!("no answer to {method} ({err})"));
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            if value["id"] == id {
                return value;
            }
        }
    }

    /// The tool's text, and whether it was an error.
    fn call(&mut self, name: &str, arguments: serde_json::Value) -> (bool, String) {
        let value = self.request(
            "tools/call",
            serde_json::json!({"name": name, "arguments": arguments}),
        );
        let failed = value.get("error").is_some() || value["result"]["isError"] == true;
        let said = value["result"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        (failed, format!("{said}{}", value["error"]))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_file_with_no_project_goes_home_or_is_refused() {
    let homed = Seat::new(HOME_ROW);
    let mut session = Session::open(&homed);
    let (failed, said) = session.call(
        "ljos_file",
        serde_json::json!({"seat": "rohit", "title": "a ticket", "issue_kind": "task"}),
    );
    assert!(!failed, "{said}");
    let (failed, said) = session.call(
        "ljos_file",
        serde_json::json!({"seat": "rohit", "title": "a top ticket", "top": true}),
    );
    assert!(!failed, "{said}");
    let creates = homed.creates();
    assert_eq!(creates.len(), 2, "{creates:?}");
    for create in &creates {
        assert!(
            create.starts_with("vissue create -p inkwell "),
            "{creates:?}"
        );
    }

    let none = Seat::new("");
    let mut session = Session::open(&none);
    let (failed, said) = session.call(
        "ljos_file",
        serde_json::json!({"seat": "rohit", "title": "a top ticket", "top": true}),
    );
    assert!(failed, "{said}");
    assert!(said.contains("pass `project`"), "{said}");
    assert!(said.contains("vissue.toml"), "{said}");
    assert!(none.creates().is_empty(), "{:?}", none.creates());
}

#[test]
fn mcp_file_under_a_parent_or_a_hold_in_kiln_files_in_kiln() {
    let seat = Seat::new(HOME_ROW);
    let mut session = Session::open(&seat);
    let (failed, said) = session.call(
        "ljos_file",
        serde_json::json!({"seat": "rohit", "title": "a child", "parent": "kiln-aaaa"}),
    );
    assert!(!failed, "{said}");
    seat.hold_for("kiln-aaaa", session.pid(), "ljos-mcp");
    let (failed, said) = session.call(
        "ljos_file",
        serde_json::json!({"seat": "rohit", "title": "a held child", "issue_kind": "task"}),
    );
    assert!(!failed, "{said}");
    let creates = seat.creates();
    assert_eq!(creates.len(), 2, "{creates:?}");
    for create in &creates {
        assert!(
            create.starts_with("vissue create -p kiln --parent kiln-aaaa "),
            "{creates:?}"
        );
    }
    seat.hold("kiln-aaaa");
    let (failed, said) = session.call(
        "ljos_start",
        serde_json::json!({"seat": "rohit", "task": "a held plan", "items": ["an item"]}),
    );
    assert!(!failed, "{said}");
    assert_eq!(seat.projects(), ["kiln"], "{said}");
    assert!(seat.held_plan_in_kiln("a held plan"), "{said}");
}

#[test]
fn mcp_start_with_no_repo_goes_home_or_is_refused() {
    let homed = Seat::new(HOME_ROW);
    let mut session = Session::open(&homed);
    let (failed, said) = session.call(
        "ljos_start",
        serde_json::json!({"seat": "rohit", "task": "fire the kiln"}),
    );
    assert!(!failed, "{said}");
    assert!(said.contains("claimed inkwell-"), "{said}");
    assert_eq!(homed.projects(), ["inkwell", "kiln"], "{said}");

    for repo in [None, Some("Software"), Some("tracker")] {
        let none = Seat::new("");
        let mut session = Session::open(&none);
        let mut args = serde_json::json!({"seat": "rohit", "task": "fire the kiln"});
        if let Some(name) = repo {
            args["repo"] = serde_json::json!(none.dir.path().join(name).display().to_string());
        }
        let (failed, said) = session.call("ljos_start", args);
        assert!(failed, "{repo:?}: {said}");
        assert!(
            said.contains("no project to file the task in"),
            "{repo:?}: {said}"
        );
        assert!(said.contains("[seats.rohit]"), "{repo:?}: {said}");
        assert_eq!(none.projects(), ["kiln"], "{repo:?}: {said}");
    }
}

#[test]
fn mcp_file_keeps_the_gone_hold_in_its_refusal() {
    let seat = Seat::new("");
    let mut session = Session::open(&seat);
    seat.hold_for("gone-zzzz", session.pid(), "ljos-mcp");
    let (failed, said) = session.call(
        "ljos_file",
        serde_json::json!({"seat": "rohit", "title": "a ticket", "issue_kind": "task"}),
    );
    assert!(failed, "{said}");
    assert!(
        said.starts_with("ljos_file: held issue gone-zzzz is gone, so it is not the parent; no project to file in, so pass `project`"),
        "{said}"
    );
    assert!(seat.hold_path().exists(), "a refused file cleared the hold");
    assert!(seat.creates().is_empty(), "{:?}", seat.creates());
}
