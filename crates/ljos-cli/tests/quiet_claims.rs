//! A seat is shown the claims it holds and has not touched for a day.
//! Another seat's claim is not in that list, and nothing is released.

ljos_test_boot::harness!();

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

use chrono::{Duration as Days, NaiveDate};
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
fn empty_pack() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            thread::spawn(move || {
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
    format!("http://127.0.0.1:{port}")
}

fn stamp(day: NaiveDate) -> String {
    day.format("[%Y-%m-%d %a 09:00]").to_string()
}

fn age(day: NaiveDate, today: NaiveDate) -> String {
    ljos_cli::age_of(
        Some(&day.format("%Y-%m-%d").to_string()),
        &today.format("%Y-%m-%d").to_string(),
    )
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
    org: PathBuf,
    today: NaiveDate,
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
        let today = chrono::Local::now().date_naive();
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
                let when = stamp(today - Days::days(days));
                heading
                    .properties
                    .insert("CLAIMED_BY".into(), claim.holder.to_string());
                heading.properties.insert("CLAIMED_AT".into(), when);
            }
            if let Some(days) = claim.note_days {
                heading.logbook.insert(
                    0,
                    LogEntry {
                        timestamp: stamp(today - Days::days(days)),
                        from_state: None,
                        to_state: None,
                        note: Some("still on it".into()),
                        raw: None,
                    },
                );
            }
            if let Some(days) = claim.deed_days {
                let when = stamp(today - Days::days(days));
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
                let when = stamp(today - Days::days(days));
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
            pack: empty_pack(),
            org: path,
            today,
            root,
        };
        (world, ids)
    }

    fn ljos(&self, seat: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
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
            .env("PACKSET_AUTH", "off")
            .output()
            .unwrap()
    }

    fn ok(&self, seat: &str, args: &[&str]) -> String {
        let out = self.ljos(seat, args);
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
            &age(world.today - Days::days(3), world.today),
        ),
        line(
            stale_long,
            long,
            &age(world.today - Days::days(2), world.today),
        ),
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
        sat.contains("2 quiet claims (`ljos claims --mine`)"),
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
            &age(world.today - Days::days(4), world.today)
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
        line(
            &ids[0],
            "oldest",
            &age(world.today - Days::days(5), world.today),
        ),
        line(
            &ids[1],
            "older",
            &age(world.today - Days::days(4), world.today),
        ),
        line(
            &ids[2],
            "old",
            &age(world.today - Days::days(3), world.today),
        ),
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
        sat.contains("4 quiet claims (`ljos claims --mine`)"),
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
}
