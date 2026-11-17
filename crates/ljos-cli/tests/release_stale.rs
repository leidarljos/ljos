//! `ljos release --stale` drops this seat's quiet claims in one command.
//!
//! A fresh claim, another seat's claim, and a claim whose conversation is
//! still running stay. A dry run writes nothing. A claim graph that does
//! not answer is one refusal line, inside the call deadline.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn actor(fill: char) -> String {
    fill.to_string().repeat(32)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The short command name a hold check compares. Linux reads `/proc`.
/// macOS reads `/bin/ps`.
fn self_comm() -> String {
    let pid = std::process::id();
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let open = stat.find('(').unwrap();
        let close = stat.rfind(')').unwrap();
        stat[open + 1..close].to_string()
    }
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("/bin/ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        let comm = text.trim();
        std::path::Path::new(comm)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(comm)
            .to_string()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        String::from("test")
    }
}

fn write_hold(run: &Path, actor: &str, seat: &str, pid: u32, comm: &str, ticket: &str) {
    let dir = run.join("ljos");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("hold-{actor}")),
        format!("{seat}\n{seat}\n{pid}\n{comm}\n2026-01-01T00:00:00Z\n{ticket}\n-\n"),
    )
    .unwrap();
}

struct Seat {
    root: tempfile::TempDir,
    org: PathBuf,
    bin: PathBuf,
    run: PathBuf,
    log: PathBuf,
    ids: Vec<String>,
}

impl Seat {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap();
        let tracker = root.path().join("tracker");
        let home = root.path().join("home");
        let bin = root.path().join("bin");
        let run = root.path().join("run");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&run).unwrap();
        let layout = vissue_core::Layout::new(&tracker, "Software");
        std::fs::create_dir_all(layout.projects_dir()).unwrap();
        let titles = [
            "quiet old",
            "quiet new",
            "fresh",
            "other seat",
            "still running",
        ];
        let holders = ["ada", "ada", "ada", "bea", "ada"];
        let mut ids = Vec::new();
        for (title, holder) in titles.iter().zip(holders) {
            let made = vissue_core::ops::create(
                &layout,
                "work",
                title,
                vissue_core::CreateOpts {
                    quiet: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let id = made.trim().to_string();
            vissue_core::ops::claim_as(&layout, &id, false, holder).unwrap();
            ids.push(id);
        }
        let path = layout.project_issues_path("work");
        let mut doc = vissue_core::IssueDoc::parse_file("work", &path).unwrap();
        let today = chrono::Local::now().date_naive();
        let ages = [3_i64, 3, 0, 3, 3];
        for ((heading_id, holder), days) in ids.iter().zip(holders).zip(ages) {
            let heading = doc
                .headings
                .iter_mut()
                .find(|heading| heading.id == *heading_id)
                .unwrap();
            heading.state = "STARTED".to_string();
            let when = (today - chrono::Duration::days(days))
                .format("[%Y-%m-%d %a 09:00]")
                .to_string();
            heading
                .properties
                .insert("CLAIMED_BY".into(), holder.to_string());
            heading.properties.insert("CLAIMED_AT".into(), when);
        }
        doc.write().unwrap();

        let now = now_unix();
        let nodes = [
            (actor('1'), actor('a'), &ids[0], now.saturating_sub(80_000)),
            (actor('2'), actor('b'), &ids[1], now.saturating_sub(50_000)),
            (actor('3'), actor('c'), &ids[2], now.saturating_sub(5)),
            (actor('4'), actor('d'), &ids[3], now.saturating_sub(80_000)),
            (actor('5'), actor('e'), &ids[4], now.saturating_sub(40_000)),
        ];
        let rows: Vec<serde_json::Value> = nodes
            .iter()
            .map(|(id, who, ticket, updated)| {
                serde_json::json!({
                    "id": id,
                    "status": "claimed",
                    "assignee": who,
                    "updated_unix": updated,
                    "summary": ticket,
                })
            })
            .collect();
        let graph = root.path().join("graph.json");
        std::fs::write(&graph, serde_json::to_string(&rows).unwrap()).unwrap();
        let log = root.path().join("claimdag.log");
        script(
            &bin,
            "claimdag",
            &format!(
                "#!/bin/sh\necho \"claimdag $*\" >> '{}'\ncase \"$1\" in\n  list) cat '{}'\n ;;\n  release) echo gen=3 ;;\nesac\nexit 0\n",
                log.display(),
                graph.display(),
            ),
        );
        script(&bin, "vissue", "#!/bin/sh\nexit 0\n");
        let me = std::process::id();
        let comm = self_comm();
        write_hold(
            &run,
            &actor('a'),
            "ada",
            4_194_303,
            "no-such-runner",
            &ids[0],
        );
        write_hold(
            &run,
            &actor('b'),
            "ada",
            4_194_303,
            "no-such-runner",
            &ids[1],
        );
        write_hold(
            &run,
            &actor('d'),
            "bea",
            4_194_303,
            "no-such-runner",
            &ids[3],
        );
        write_hold(&run, &actor('e'), "ada", me, &comm, &ids[4]);
        Self {
            org: path,
            bin,
            run,
            log,
            ids,
            root,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        let home = self.root.path().join("home");
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(self.root.path())
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("HOME", &home)
            .env("TMPDIR", self.root.path())
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("VISSUE_ROOT", self.root.path().join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("PACKSET_URL", "off")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        let text = format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{args:?} failed\n{text}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

fn state(path: &Path, id: &str) -> (String, Option<String>) {
    let doc = vissue_core::IssueDoc::parse_file("work", path).unwrap();
    let heading = doc
        .headings
        .iter()
        .find(|heading| heading.id == id)
        .unwrap();
    (
        heading.state.clone(),
        heading.claimed_by().map(str::to_string),
    )
}

#[test]
fn release_stale_drops_quiet_claims_and_leaves_the_rest() {
    let seat = Seat::open();
    let (old, new, fresh, other, live) = (
        seat.ids[0].clone(),
        seat.ids[1].clone(),
        seat.ids[2].clone(),
        seat.ids[3].clone(),
        seat.ids[4].clone(),
    );
    let before = std::fs::read(&seat.org).unwrap();

    let both = seat.run(&["release", &old, "--stale"]);
    let err = String::from_utf8_lossy(&both.stderr);
    assert!(!both.status.success(), "{err}");
    assert!(
        err.contains("release: pass an id or --stale, not both"),
        "{err}"
    );
    assert!(both.stdout.is_empty(), "{both:?}");
    assert_eq!(std::fs::read(&seat.org).unwrap(), before);

    let dry = seat.ok(&["release", "--stale", "--dry-run"]);
    assert_eq!(
        dry, "would release 2 quiet claims; kept 1 held by a running conversation\n",
        "{dry}"
    );
    assert_eq!(std::fs::read(&seat.org).unwrap(), before);
    let logged = std::fs::read_to_string(&seat.log).unwrap();
    assert!(logged.contains("list"), "{logged}");
    assert!(!logged.contains("release "), "{logged}");

    let verbose = seat.ok(&["release", "--stale", "--dry-run", "--verbose"]);
    assert!(
        verbose.contains(&format!("would release {old}\n")),
        "{verbose}"
    );
    assert!(
        verbose.contains(&format!("would release {new}\n")),
        "{verbose}"
    );
    assert!(
        verbose.contains(&format!("kept {live}: held by a running conversation\n")),
        "{verbose}"
    );
    assert!(!verbose.contains(&fresh), "{verbose}");
    assert!(!verbose.contains(&other), "{verbose}");
    assert!(!verbose.contains("quiet claims;"), "{verbose}");
    assert_eq!(std::fs::read(&seat.org).unwrap(), before);

    let wide = seat.ok(&["release", "--stale", "--older-than", "2d", "--dry-run"]);
    assert_eq!(wide, "would release 0 quiet claims\n", "{wide}");
    assert_eq!(std::fs::read(&seat.org).unwrap(), before);

    std::fs::write(&seat.log, "").unwrap();
    let said = seat.ok(&["release", "--stale"]);
    assert_eq!(
        said, "released 2 quiet claims; kept 1 held by a running conversation\n",
        "{said}"
    );
    let calls = std::fs::read_to_string(&seat.log).unwrap();
    assert!(
        calls.contains(&format!("release {} --actor {}", actor('1'), actor('a'))),
        "{calls}"
    );
    assert!(
        calls.contains(&format!("release {} --actor {}", actor('2'), actor('b'))),
        "{calls}"
    );
    assert!(!calls.contains(&actor('c')), "{calls}");
    assert!(!calls.contains(&actor('d')), "{calls}");
    assert!(!calls.contains(&actor('e')), "{calls}");

    for id in [&old, &new] {
        let (st, who) = state(&seat.org, id);
        assert_eq!(st, "TODO", "{id} {st}");
        assert!(who.is_none(), "{id} still claimed by {who:?}");
    }
    for id in [&fresh, &live] {
        let (st, who) = state(&seat.org, id);
        assert_eq!(st, "STARTED", "{id} {st}");
        assert_eq!(who.as_deref(), Some("ada"), "{id} {who:?}");
    }
    let (st, who) = state(&seat.org, &other);
    assert_eq!(st, "STARTED", "{st}");
    assert_eq!(who.as_deref(), Some("bea"), "{who:?}");
    let org = String::from_utf8(std::fs::read(&seat.org).unwrap()).unwrap();
    assert!(!org.contains("* DONE"), "{org}");
    assert!(
        seat.run
            .join("ljos")
            .join(format!("hold-{}", actor('e')))
            .is_file(),
        "the running conversation's hold was dropped"
    );
    assert!(
        !seat
            .run
            .join("ljos")
            .join(format!("hold-{}", actor('a')))
            .exists(),
        "the gone conversation's hold was kept"
    );

    let one = seat.ok(&["release", &fresh]);
    let (st, who) = state(&seat.org, &fresh);
    assert_eq!(st, "TODO", "{one}\n{st}");
    assert!(who.is_none(), "{one}");
    let org = String::from_utf8(std::fs::read(&seat.org).unwrap()).unwrap();
    assert!(!org.contains("* DONE"), "{org}");
    let (st, who) = state(&seat.org, &other);
    assert_eq!(
        who.as_deref(),
        Some("bea"),
        "release ISSUE touched another seat: {st}"
    );
}

#[test]
fn a_silent_claim_graph_is_one_line_inside_the_deadline() {
    let seat = Seat::open();
    let before = std::fs::read(&seat.org).unwrap();
    let pidfile = seat.root.path().join("sleep.pid");
    script(
        &seat.bin,
        "claimdag",
        &format!(
            "#!/bin/sh\nsleep 30 &\necho $! > '{}'\nwait\n",
            pidfile.display()
        ),
    );
    let home = seat.root.path().join("home");
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["release", "--stale"])
        .current_dir(seat.root.path())
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", seat.bin.display()))
        .env("HOME", &home)
        .env("TMPDIR", seat.root.path())
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &seat.run)
        .env("VISSUE_ROOT", seat.root.path().join("tracker"))
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_ASK_PACK", "off")
        .env("PACKSET_URL", "off")
        .env("LJOS_CALL_TIMEOUT", "2")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let took = started.elapsed();
    let err = String::from_utf8_lossy(&out.stderr);
    let out_text = String::from_utf8_lossy(&out.stdout);
    assert!(
        took < Duration::from_secs(8),
        "release --stale waited {took:?}\n{err}{out_text}"
    );
    assert!(!out.status.success(), "{err}{out_text}");
    let lines: Vec<_> = err.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines,
        ["claim graph did not answer in 2s; nothing was written"],
        "{err}"
    );
    assert!(out.stdout.is_empty(), "{out_text}");
    assert_eq!(std::fs::read(&seat.org).unwrap(), before);
    let pid = std::fs::read_to_string(&pidfile).unwrap_or_default();
    assert!(!pid.trim().is_empty(), "the sleeper did not start\n{err}");
    let alive = Command::new("kill")
        .args(["-0", pid.trim()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!alive.success(), "grandchild {} still running", pid.trim());
}

#[test]
fn release_stale_drops_older_holders_of_same_seat_and_keeps_live_and_other() {
    let root = tempfile::tempdir().unwrap();
    let tracker = root.path().join("tracker");
    let home = root.path().join("home");
    let bin = root.path().join("bin");
    let run = root.path().join("run");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&run).unwrap();
    let layout = vissue_core::Layout::new(&tracker, "Software");
    std::fs::create_dir_all(layout.projects_dir()).unwrap();
    let titles = [
        "dead hold older holder",
        "gone hold older holder",
        "tracker only older holder",
        "still running older holder",
        "other seat",
    ];
    let holders = [
        "dseams-paper@dorx1f91mq",
        "dseams-paper@dorx1f91mq",
        "dseams-paper@dorx1f91mq",
        "dseams-paper@live111111",
        "other-seat@dorx1f91mq",
    ];
    let mut ids = Vec::new();
    for (title, holder) in titles.iter().zip(holders) {
        let made = vissue_core::ops::create(
            &layout,
            "work",
            title,
            vissue_core::CreateOpts {
                quiet: true,
                ..Default::default()
            },
        )
        .unwrap();
        let id = made.trim().to_string();
        vissue_core::ops::claim_as(&layout, &id, false, holder).unwrap();
        ids.push(id);
    }
    let org = layout.project_issues_path("work");
    let mut doc = vissue_core::IssueDoc::parse_file("work", &org).unwrap();
    let old_when = (chrono::Local::now().date_naive() - chrono::Duration::days(3))
        .format("[%Y-%m-%d %a 09:00]")
        .to_string();
    for (id, holder) in ids.iter().zip(holders) {
        let heading = doc.headings.iter_mut().find(|h| h.id == *id).unwrap();
        heading.state = "STARTED".to_string();
        heading
            .properties
            .insert("CLAIMED_BY".into(), holder.to_string());
        heading
            .properties
            .insert("CLAIMED_AT".into(), old_when.clone());
    }
    doc.write().unwrap();

    let now = now_unix();
    let rows = vec![
        serde_json::json!({
            "id": actor('1'),
            "status": "claimed",
            "assignee": actor('a'),
            "updated_unix": now.saturating_sub(80_000),
            "summary": &ids[0],
        }),
        serde_json::json!({
            "id": actor('2'),
            "status": "claimed",
            "assignee": actor('b'),
            "updated_unix": now.saturating_sub(70_000),
            "summary": &ids[1],
        }),
        serde_json::json!({
            "id": actor('3'),
            "status": "claimed",
            "assignee": actor('c'),
            "updated_unix": now.saturating_sub(60_000),
            "summary": &ids[3],
        }),
        serde_json::json!({
            "id": actor('4'),
            "status": "claimed",
            "assignee": actor('d'),
            "updated_unix": now.saturating_sub(80_000),
            "summary": &ids[4],
        }),
    ];
    let graph = root.path().join("graph.json");
    std::fs::write(&graph, serde_json::to_string(&rows).unwrap()).unwrap();
    script(
        &bin,
        "claimdag",
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  list) cat '{}' ;;\n  release) echo gen=3 ;;\nesac\nexit 0\n",
            graph.display(),
        ),
    );
    script(&bin, "vissue", "#!/bin/sh\nexit 0\n");

    write_hold(
        &run,
        &actor('a'),
        "dseams-paper",
        4_194_303,
        "no-such-runner",
        &ids[0],
    );
    // ids[1] and ids[2] deliberately have no hold file (old session is gone).
    write_hold(
        &run,
        &actor('c'),
        "dseams-paper",
        std::process::id(),
        &self_comm(),
        &ids[3],
    );
    write_hold(
        &run,
        &actor('d'),
        "other-seat",
        4_194_303,
        "no-such-runner",
        &ids[4],
    );

    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["release", "--stale"])
        .current_dir(root.path())
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("TMPDIR", root.path())
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", &run)
        .env("VISSUE_ROOT", &tracker)
        .env("VISSUE_NO_ROUTE", "1")
        .env("LJOS_SEAT", "dseams-paper")
        .env("CURSOR_CONVERSATION_ID", "li41adshxv-current-session")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_ASK_PACK", "off")
        .env("PACKSET_URL", "off")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{said}");
    for id in [&ids[0], &ids[1], &ids[2]] {
        assert!(
            said.contains(&format!("took over {id} from dseams-paper@dorx1f91mq")),
            "missing takeover line for {id}:\n{said}"
        );
        let (st, who) = state(&org, id);
        assert_eq!(st, "TODO", "{id} {st}");
        assert!(who.is_none(), "{id} still claimed by {who:?}");
    }
    assert!(
        said.contains("released 3 quiet claims; kept 1 held by a running conversation"),
        "{said}"
    );
    let (st, who) = state(&org, &ids[3]);
    assert_eq!(st, "STARTED");
    assert_eq!(who.as_deref(), Some("dseams-paper@live111111"));
    let (st, who) = state(&org, &ids[4]);
    assert_eq!(st, "STARTED");
    assert_eq!(who.as_deref(), Some("other-seat@dorx1f91mq"));
}
