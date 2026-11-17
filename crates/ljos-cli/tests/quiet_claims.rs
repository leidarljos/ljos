//! A seat is shown the claims it holds and has not touched for a day.
//! Another seat's claim is not in that list, and nothing is released.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ljos_cli::clock::{self, Date};
use vissue_core::LogEntry;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn respond(sock: &mut impl Write, body: &str) {
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = sock.write_all(resp.as_bytes());
}

/// A pack that answers at once with nothing stored.
///
/// The count is every request the process made. A command that does not
/// open the pack leaves it at zero.
fn empty_pack() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&hits);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            let count = Arc::clone(&count);
            thread::spawn(move || {
                count.fetch_add(1, Ordering::SeqCst);
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf);
                let req = String::from_utf8_lossy(&buf);
                let line = req.lines().next().unwrap_or("");
                if line.contains("/v1/status") || line.contains("/health") {
                    respond(&mut sock, r#"{"version":"0.13.0"}"#);
                } else if line.contains("/v1/atoms") {
                    respond(&mut sock, r#"{"atoms":[]}"#);
                } else {
                    respond(&mut sock, r#"{"hits":[],"island":[]}"#);
                }
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), hits)
}

fn stamp(day: Date) -> String {
    clock::org_stamp(day, 9, 0)
}

fn back(day: Date, days: i64) -> Date {
    clock::days_before(day, days).unwrap()
}

fn age(day: Date, today: Date) -> String {
    ljos_cli::age_of(Some(&clock::day_text(day)), &clock::day_text(today))
}

/// Characters kept from a title, matching the line a seat reads.
fn short_title(title: &str) -> String {
    const SHOWN: usize = 48;
    const ELLIPSIS: &str = "...";
    if title.chars().count() <= SHOWN {
        return title.to_string();
    }
    let keep = SHOWN - ELLIPSIS.chars().count();
    let mut cut: String = title.chars().take(keep).collect();
    cut.push_str(ELLIPSIS);
    cut
}

fn line(id: &str, title: &str, age: &str) -> String {
    format!(
        "{id}  {}  {age}  ljos finish {id} / ljos note {id} \"...\"",
        short_title(title)
    )
}

fn quiet_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.contains("ljos note ") && line.contains("\"...\""))
        .map(str::to_string)
        .collect()
}

struct Claim {
    title: &'static str,
    holder: &'static str,
    /// Days before today of the claim stamp. `None` leaves the ticket unclaimed.
    claim_days: Option<i64>,
    state: &'static str,
    note_days: Option<i64>,
    deed_days: Option<i64>,
    finished_days: Option<i64>,
}

struct World {
    root: tempfile::TempDir,
    tracker: PathBuf,
    home: PathBuf,
    path: String,
    pack: String,
    pack_hits: Arc<AtomicUsize>,
    org: PathBuf,
    today: Date,
}

impl World {
    fn new(claims: &[Claim]) -> (Self, Vec<String>) {
        let root = tempfile::tempdir().unwrap();
        let tracker = root.path().join("tracker");
        let home = root.path().join("home");
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        let layout = vissue_core::Layout::new(&tracker, "Software");
        std::fs::create_dir_all(layout.projects_dir()).unwrap();
        let mut ids = Vec::new();
        for claim in claims {
            let made = vissue_core::ops::create(
                &layout,
                "work",
                claim.title,
                vissue_core::CreateOpts {
                    quiet: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let id = made.trim().to_string();
            if !claim.holder.is_empty() && claim.claim_days.is_some() {
                vissue_core::ops::claim_as(&layout, &id, false, claim.holder).unwrap();
            }
            ids.push(id);
        }
        let today = clock::local_today();
        let (pack, pack_hits) = empty_pack();
        let path = layout.project_issues_path("work");
        let mut doc = vissue_core::IssueDoc::parse_file("work", &path).unwrap();
        for (claim, id) in claims.iter().zip(ids.iter()) {
            let heading = doc
                .headings
                .iter_mut()
                .find(|heading| heading.id == *id)
                .unwrap();
            heading.state = claim.state.to_string();
            if let Some(days) = claim.claim_days {
                let when = stamp(back(today, days));
                heading
                    .properties
                    .insert("CLAIMED_BY".into(), claim.holder.to_string());
                heading.properties.insert("CLAIMED_AT".into(), when);
            }
            if let Some(days) = claim.note_days {
                heading.logbook.insert(
                    0,
                    LogEntry {
                        timestamp: stamp(back(today, days)),
                        from_state: None,
                        to_state: None,
                        note: Some("still on it".into()),
                        raw: None,
                    },
                );
            }
            if let Some(days) = claim.deed_days {
                let when = stamp(back(today, days));
                heading.logbook.insert(
                    0,
                    LogEntry {
                        timestamp: when.clone(),
                        from_state: None,
                        to_state: None,
                        note: None,
                        raw: Some(format!("- cited deed-file-old {when}")),
                    },
                );
            }
            if let Some(days) = claim.finished_days {
                let when = stamp(back(today, days));
                heading.logbook.insert(
                    0,
                    LogEntry {
                        timestamp: when.clone(),
                        from_state: None,
                        to_state: None,
                        note: None,
                        raw: Some(format!("- Finished by {} {when}", claim.holder)),
                    },
                );
            }
        }
        doc.write().unwrap();
        let tracker_s = tracker.display().to_string();
        script(
            &bin,
            "vissue",
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  identity) printf 'vissue test\\nroot={tracker_s}\\nprefix=Software\\n' ;;\n  show) printf '%s\\n' \"{{\\\"id\\\":\\\"$2\\\",\\\"title\\\":\\\"$2\\\",\\\"state\\\":\\\"TODO\\\"}}\" ;;\n  recall) echo noted ;;\nesac\nexit 0\n"
            ),
        );
        script(
            &bin,
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  list) echo '[]' ;;\nesac\nexit 0\n",
        );
        script(&bin, "deedar", "#!/bin/sh\necho head\nexit 0\n");
        let world = Self {
            tracker,
            home,
            path: format!("{}:/usr/bin:/bin", bin.display()),
            pack,
            pack_hits,
            org: path,
            today,
            root,
        };
        (world, ids)
    }

    fn ljos_env(&self, seat: &str, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.root.path())
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("TMPDIR", "/tmp")
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", seat)
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("LJOS_LEASE_SECS", "0")
            .env("PACKSET_URL", &self.pack)
            .env("PACKSET_AUTH", "off");
        for (key, value) in extra {
            cmd.env(key, value);
        }
        cmd.output().unwrap()
    }

    fn ok(&self, seat: &str, args: &[&str]) -> String {
        self.ok_env(seat, args, &[])
    }

    fn ok_env(&self, seat: &str, args: &[&str], extra: &[(&str, &str)]) -> String {
        let out = self.ljos_env(seat, args, extra);
        let text = format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{args:?} as {seat} failed\n{text}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

fn claim(
    title: &'static str,
    holder: &'static str,
    claim_days: i64,
    note_days: Option<i64>,
    deed_days: Option<i64>,
    finished_days: Option<i64>,
    state: &'static str,
) -> Claim {
    Claim {
        title,
        holder,
        claim_days: Some(claim_days),
        state,
        note_days,
        deed_days,
        finished_days,
    }
}

#[test]
fn a_seat_sees_its_two_stale_claims_and_not_another_seats() {
    let long =
        "a very long title that a seat should see cut short before the commands crowd it out";
    let (world, ids) = World::new(&[
        claim("stale alpha", "ada", 5, None, Some(3), None, "STARTED"),
        claim(long, "ada", 2, None, None, None, "STARTED"),
        claim("fresh today", "ada", 0, None, None, None, "STARTED"),
        claim("bea holds this", "bea", 4, None, None, None, "STARTED"),
        claim("noted today", "ada", 6, Some(0), None, None, "STARTED"),
        claim("deed today", "ada", 6, None, Some(0), None, "STARTED"),
        claim("finished today", "ada", 6, None, None, Some(0), "STARTED"),
        claim("already closed", "ada", 6, None, None, None, "DONE"),
        Claim {
            title: "the sitting",
            holder: "",
            claim_days: None,
            state: "TODO",
            note_days: None,
            deed_days: None,
            finished_days: None,
        },
    ]);
    let stale_deed = &ids[0];
    let stale_long = &ids[1];
    let fresh = &ids[2];
    let bea = &ids[3];
    let noted = &ids[4];
    let deed_today = &ids[5];
    let finished = &ids[6];
    let closed = &ids[7];
    let sitting = &ids[8];
    let expected = vec![
        line(
            stale_deed,
            "stale alpha",
            &age(back(world.today, 3), world.today),
        ),
        line(stale_long, long, &age(back(world.today, 2), world.today)),
    ];
    let before = std::fs::read(&world.org).unwrap();

    let mine = world.ok("ada", &["claims", "--mine"]);
    assert_eq!(quiet_lines(&mine), expected, "{mine}");
    assert!(!mine.contains("more (ljos claims --mine)"), "{mine}");
    assert!(!mine.contains(fresh), "{mine}");
    assert!(!mine.contains(bea), "{mine}");
    assert!(!mine.contains(noted), "{mine}");
    assert!(!mine.contains(deed_today), "{mine}");
    assert!(!mine.contains(finished), "{mine}");
    assert!(!mine.contains(closed), "{mine}");
    assert_eq!(std::fs::read(&world.org).unwrap(), before);

    let inbox = world.ok("ada", &["inbox"]);
    assert!(inbox.contains("inbox: nothing unread"), "{inbox}");
    assert_eq!(quiet_lines(&inbox), expected, "{inbox}");
    assert!(!inbox.contains(bea), "{inbox}");
    assert_eq!(std::fs::read(&world.org).unwrap(), before);

    let sat = world.ok("ada", &["sitting", sitting]);
    assert!(
        sat.contains("2 quiet claims are still yours (ljos release --stale)"),
        "{sat}"
    );
    assert!(quiet_lines(&sat).is_empty(), "{sat}");
    assert!(sat.contains("is yours"), "{sat}");
    assert!(!sat.contains(fresh), "{sat}");
    assert!(!sat.contains(bea), "{sat}");
    assert!(!sat.contains(noted), "{sat}");
    assert_eq!(std::fs::read(&world.org).unwrap(), before);

    let theirs = world.ok("bea", &["claims", "--mine"]);
    assert_eq!(
        quiet_lines(&theirs),
        vec![line(
            bea,
            "bea holds this",
            &age(back(world.today, 4), world.today)
        )],
        "{theirs}"
    );
    assert!(!theirs.contains(stale_deed), "{theirs}");
    assert!(!theirs.contains(stale_long), "{theirs}");
    assert_eq!(std::fs::read(&world.org).unwrap(), before);
}

#[test]
fn claims_past_the_handful_end_on_one_more_line() {
    let (world, ids) = World::new(&[
        claim("oldest", "ada", 5, None, None, None, "STARTED"),
        claim("older", "ada", 4, None, None, None, "STARTED"),
        claim("old", "ada", 3, None, None, None, "STARTED"),
        claim("least old", "ada", 2, None, None, None, "STARTED"),
        Claim {
            title: "the sitting",
            holder: "",
            claim_days: None,
            state: "TODO",
            note_days: None,
            deed_days: None,
            finished_days: None,
        },
    ]);
    let shown = vec![
        line(&ids[0], "oldest", &age(back(world.today, 5), world.today)),
        line(&ids[1], "older", &age(back(world.today, 4), world.today)),
        line(&ids[2], "old", &age(back(world.today, 3), world.today)),
    ];
    let mine = world.ok("ada", &["claims", "--mine"]);
    assert_eq!(quiet_lines(&mine).len(), 4, "{mine}");
    assert!(!mine.contains("more (ljos claims --mine)"), "{mine}");
    assert!(mine.contains(&ids[3]), "{mine}");

    let inbox = world.ok("ada", &["inbox"]);
    assert_eq!(quiet_lines(&inbox), shown, "{inbox}");
    assert!(inbox.contains("+1 more (ljos claims --mine)"), "{inbox}");
    assert!(!inbox.contains(&ids[3]), "{inbox}");

    let sat = world.ok("ada", &["sitting", &ids[4]]);
    assert!(
        sat.contains("4 quiet claims are still yours (ljos release --stale)"),
        "{sat}"
    );
    assert!(quiet_lines(&sat).is_empty(), "{sat}");
    assert!(!sat.contains(&ids[3]), "{sat}");

    let verbose = world.ok("ada", &["sitting", &ids[4], "--verbose"]);
    assert_eq!(quiet_lines(&verbose), shown, "{verbose}");
    assert!(
        verbose.contains("+1 more (ljos claims --mine)"),
        "{verbose}"
    );
    assert!(
        !verbose.contains("untouched for"),
        "a claim under 7 days was called stale:\n{verbose}"
    );
}

/// The three banner rows are the claims still inside the window.
/// A claim untouched for 7 days or more is one line.
#[test]
fn the_banner_keeps_fresh_claims_and_folds_stale_ones() {
    let (world, ids) = World::new(&[
        claim("still in hand", "ada", 3, None, None, None, "STARTED"),
        claim("left last month", "ada", 12, None, None, None, "STARTED"),
        claim("touched today", "ada", 0, None, None, None, "STARTED"),
        Claim {
            title: "the sitting",
            holder: "",
            claim_days: None,
            state: "TODO",
            note_days: None,
            deed_days: None,
            finished_days: None,
        },
    ]);
    let fresh = &ids[0];
    let stale = &ids[1];
    let today = &ids[2];
    let sitting = &ids[3];
    let inbox = world.ok("ada", &["inbox"]);
    assert_eq!(
        quiet_lines(&inbox),
        vec![line(
            fresh,
            "still in hand",
            &age(back(world.today, 3), world.today)
        )],
        "{inbox}"
    );
    assert!(
        inbox.contains("1 claim untouched for 7+ days: ljos claims --stale"),
        "{inbox}"
    );
    assert!(!inbox.contains(stale), "{inbox}");
    assert!(!inbox.contains(today), "{inbox}");
    assert!(!inbox.contains("more (ljos claims --mine)"), "{inbox}");

    let verbose = world.ok("ada", &["sitting", sitting, "--verbose"]);
    assert!(verbose.contains(fresh), "{verbose}");
    assert!(
        verbose.contains("1 claim untouched for 7+ days: ljos claims --stale"),
        "{verbose}"
    );
    assert!(!verbose.contains(stale), "{verbose}");

    let short = world.ok("ada", &["sitting", sitting]);
    assert!(
        short.contains("1 quiet claim is still yours (ljos release --stale)"),
        "{short}"
    );
    assert!(
        short.contains("1 claim untouched for 7+ days: ljos claims --stale"),
        "{short}"
    );
    assert!(quiet_lines(&short).is_empty(), "{short}");
}

/// A wider window puts the same claim back in the rows.
#[test]
fn a_wider_stale_window_leaves_the_banner_as_it_was() {
    let (world, ids) = World::new(&[
        claim("ten days", "ada", 10, None, None, None, "STARTED"),
        Claim {
            title: "the sitting",
            holder: "",
            claim_days: None,
            state: "TODO",
            note_days: None,
            deed_days: None,
            finished_days: None,
        },
    ]);
    let inbox = world.ok_env("ada", &["inbox"], &[("LJOS_STALE_DAYS", "14")]);
    assert_eq!(
        quiet_lines(&inbox),
        vec![line(
            &ids[0],
            "ten days",
            &age(back(world.today, 10), world.today)
        )],
        "{inbox}"
    );
    assert!(!inbox.contains("untouched for"), "{inbox}");
}

#[test]
fn claims_stale_lists_the_oldest_first_and_json_matches() {
    let (world, ids) = World::new(&[
        claim("middle", "ada", 12, None, None, None, "STARTED"),
        claim("oldest", "ada", 20, None, None, None, "STARTED"),
        claim("newer", "ada", 8, None, None, None, "STARTED"),
        claim("recent", "ada", 2, None, None, None, "STARTED"),
    ]);
    let listed = world.ok("ada", &["claims", "--stale"]);
    let lines = quiet_stale_lines(&listed);
    assert_eq!(lines.len(), 3, "{listed}");
    assert!(lines[0].contains(&ids[1]), "{listed}");
    assert!(lines[1].contains(&ids[0]), "{listed}");
    assert!(lines[2].contains(&ids[2]), "{listed}");
    assert!(!listed.contains(&ids[3]), "{listed}");
    for (line, id) in lines.iter().zip([&ids[1], &ids[0], &ids[2]]) {
        assert!(line.contains(&format!("ljos finish {id}")), "{line}");
        assert!(line.contains(&format!("ljos release {id}")), "{line}");
        assert!(line.contains(&format!("ljos note {id} \"...\"")), "{line}");
    }
    let json = world.ok("ada", &["claims", "--stale", "--json"]);
    let rows: Vec<serde_json::Value> = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(rows.len(), 3, "{json}");
    assert_eq!(rows[0]["id"], ids[1], "{json}");
    assert_eq!(rows[1]["id"], ids[0], "{json}");
    assert_eq!(rows[2]["id"], ids[2], "{json}");
    assert_eq!(rows[0]["age_days"], 20, "{json}");
    assert_eq!(rows[0]["title"], "oldest", "{json}");
    assert_eq!(
        rows[0]["finish"],
        format!("ljos finish {}", ids[1]),
        "{json}"
    );
    assert_eq!(
        rows[0]["release"],
        format!("ljos release {}", ids[1]),
        "{json}"
    );
    let narrowed = world.ok("ada", &["claims", "--stale", "--days", "15", "--json"]);
    let few: Vec<serde_json::Value> = serde_json::from_str(narrowed.trim()).unwrap();
    assert_eq!(few.len(), 1, "{narrowed}");
    assert_eq!(few[0]["id"], ids[1], "{narrowed}");
}

#[test]
fn release_stale_prints_the_list_until_yes_and_then_returns_the_ticket_to_todo() {
    let (world, ids) = World::new(&[claim("old work", "ada", 11, None, None, None, "STARTED")]);
    let before = std::fs::read(&world.org).unwrap();
    let dry = world.ok("ada", &["release", "--stale", "--days", "7"]);
    assert!(dry.contains(&ids[0]), "{dry}");
    assert!(dry.contains(&format!("ljos release {}", ids[0])), "{dry}");
    assert!(
        dry.contains(
            "1 claim would go back to TODO. ljos release --stale --days 7 --yes releases them."
        ),
        "{dry}"
    );
    assert_eq!(std::fs::read(&world.org).unwrap(), before);
    assert!(!dry.contains("--close"), "{dry}");

    let yes = world.ok("ada", &["release", "--stale", "--yes"]);
    let org = String::from_utf8(std::fs::read(&world.org).unwrap()).unwrap();
    assert!(
        org.contains("* TODO") && !org.contains("CLAIMED_BY") && !org.contains("* DONE"),
        "{org}\n{yes}"
    );
    assert!(yes.contains("back to TODO"), "{yes}");
    assert!(
        yes.contains("released 1 claim. Each ticket is TODO again."),
        "{yes}"
    );
    assert!(!yes.contains("--close"), "{yes}");
    assert!(!yes.contains("finish"), "{yes}");
}

#[test]
fn release_stale_with_nothing_to_hand_back_writes_nothing_and_skips_the_pack() {
    let (world, ids) = World::new(&[claim(
        "touched today",
        "ada",
        0,
        None,
        None,
        None,
        "STARTED",
    )]);
    let before = std::fs::read(&world.org).unwrap();
    assert_eq!(world.pack_hits.load(Ordering::SeqCst), 0);
    let said = world.ok("ada", &["release", "--stale", "--yes"]);
    assert!(said.contains("no claims untouched for 7+ days"), "{said}");
    assert_eq!(std::fs::read(&world.org).unwrap(), before);
    assert_eq!(
        world.pack_hits.load(Ordering::SeqCst),
        0,
        "an empty stale release called the pack"
    );
    let bin = PathBuf::from(world.path.split(':').next().unwrap());
    script(&bin, "claimdag", "#!/bin/sh\necho called >&2\nexit 1\n");
    let again = world.ok("ada", &["release", "--stale", "--days", "7"]);
    assert!(again.contains("no claims untouched"), "{again}");
    assert_eq!(std::fs::read(&world.org).unwrap(), before);
    assert!(!again.contains(&ids[0]), "{again}");
}

/// Lines of `ljos claims --stale`: the id, the age and the three commands.
fn quiet_stale_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.contains("ljos finish ") && line.contains("ljos release "))
        .map(str::to_string)
        .collect()
}
