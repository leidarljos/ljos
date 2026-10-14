//! Reap idle claims: a sitting nobody finished goes back to ready, and its
//! seat hears why. A claim is idle when neither its claim time nor the
//! newest tracker commit naming it falls inside the window.

use anyhow::{bail, Result};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde_json::Value;
use std::time::Duration;

/// One claim the reaper looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub id: String,
    pub holder: String,
    pub title: String,
    /// Newest sign of life: claim time or last tracker commit, Unix seconds.
    pub last: Option<i64>,
}

/// `6h`, `90m`, `2d`, `45s`.
pub fn parse_idle(s: &str) -> Result<Duration> {
    let s = s.trim();
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: u64 = n
        .parse()
        .map_err(|_| anyhow::anyhow!("reap: --idle {s:?} wants a number and s, m, h or d"))?;
    let secs = match unit {
        "s" => n,
        "m" | "" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        _ => bail!("reap: --idle {s:?} wants a number and s, m, h or d"),
    };
    Ok(Duration::from_secs(secs))
}

/// The tracker's `[2026-10-13 Tue 16:01]`, read in this machine's zone.
pub fn org_ts(s: &str) -> Option<i64> {
    let t = s
        .trim()
        .trim_matches(|c| c == '[' || c == '<' || c == ']' || c == '>');
    let mut parts = t.split_whitespace();
    let date = parts.next()?;
    let time = parts.find(|p| p.contains(':')).unwrap_or("00:00");
    let naive = NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M").ok()?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|d| d.timestamp())
}

/// The seat a holder names: `eessi@main` is the seat `eessi`.
pub fn seat_of(holder: &str) -> &str {
    holder.split('@').next().unwrap_or(holder)
}

/// Claims from `vissue claims --json`, `last` set from the claim time.
pub fn held_from_json(json: &str) -> Result<Vec<Held>> {
    let v: Value = serde_json::from_str(json)?;
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(Held {
                id: c["id"].as_str()?.to_string(),
                holder: c["holder"].as_str()?.to_string(),
                title: c["title"].as_str().unwrap_or("").to_string(),
                last: c["claimed_at"].as_str().and_then(org_ts),
            })
        })
        .collect())
}

/// The claims idle past `idle` at `now`. A claim with no time is idle.
pub fn idle_claims(held: &[Held], idle: Duration, now: i64) -> Vec<Held> {
    let cut = now - idle.as_secs() as i64;
    held.iter()
        .filter(|h| h.last.is_none_or(|t| t < cut))
        .cloned()
        .collect()
}

/// Newest tracker commit whose subject names `id`, Unix seconds.
fn last_commit(id: &str) -> Option<i64> {
    let root = std::env::var("VISSUE_ROOT").ok()?;
    let out = std::process::Command::new("git")
        .args(["-C", &root, "log", "-1", "--format=%ct", "-F", "--grep", id])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn when(t: Option<i64>) -> String {
    t.and_then(|t| DateTime::from_timestamp(t, 0))
        .map(|d| d.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "never".to_string())
}

/// Release every claim idle past `idle` and mail its seat. Without
/// `apply`, print what would happen and write nothing.
pub fn reap(idle: Duration, apply: bool) -> Result<String> {
    let said = crate::run_captured("vissue", &["claims", "--json"])?;
    let mut held = held_from_json(&said.stdout)?;
    for h in &mut held {
        if let Some(c) = last_commit(&h.id) {
            h.last = Some(h.last.map_or(c, |t| t.max(c)));
        }
    }
    let now = Local::now().timestamp();
    let idle_ones = idle_claims(&held, idle, now);
    let mut out = String::new();
    let hours = idle.as_secs() as f64 / 3600.0;
    for h in &idle_ones {
        let seat = seat_of(&h.holder);
        let note = format!(
            "auto-released {}: idle since {} (over {hours:.1}h). Run `ljos claim {}` to pick it up again, or `ljos finish {} --status done` when it is done.",
            h.id, when(h.last), h.id, h.id
        );
        if !apply {
            out.push_str(&format!(
                "would release {} held by {} (last {}) and mail {seat}\n",
                h.id,
                h.holder,
                when(h.last)
            ));
            continue;
        }
        match crate::release(&h.id, &h.holder) {
            Ok(s) => {
                out.push_str(&s);
                out.push_str(&crate::persist_tracker(&h.id, "released"));
            }
            Err(e) => {
                out.push_str(&format!(
                    "reap: {} held by {}: release refused: {e}\n",
                    h.id, h.holder
                ));
                continue;
            }
        }
        let mail = crate::mail::send(&crate::mail::Send {
            seat: Some(seat),
            group: None,
            text: &note,
            interrupt: false,
            issue: Some(&h.id),
            new_seat: true,
        });
        match mail {
            Ok(s) => out.push_str(&s),
            Err(e) => out.push_str(&format!("reap: {}: mail to {seat} failed: {e}\n", h.id)),
        }
    }
    out.push_str(&format!(
        "reap: {} of {} claims idle over {hours:.1}h{}\n",
        idle_ones.len(),
        held.len(),
        if apply {
            ""
        } else {
            "; dry run, pass --apply to release"
        }
    ));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_parses_units() {
        assert_eq!(parse_idle("6h").unwrap().as_secs(), 21600);
        assert_eq!(parse_idle("90m").unwrap().as_secs(), 5400);
        assert_eq!(parse_idle("2d").unwrap().as_secs(), 172800);
        assert!(parse_idle("6x").is_err());
        assert!(parse_idle("h").is_err());
    }

    #[test]
    fn holder_names_its_seat() {
        assert_eq!(seat_of("eessi@main"), "eessi");
        assert_eq!(seat_of("inky"), "inky");
    }

    #[test]
    fn only_old_claims_are_idle() {
        let json = r#"[
          {"id":"a-1","holder":"inky","title":"old","claimed_at":"[2026-10-13 Tue 06:00]"},
          {"id":"a-2","holder":"eessi@main","title":"new","claimed_at":"[2026-10-13 Tue 16:00]"},
          {"id":"a-3","holder":"travel","title":"no time"}
        ]"#;
        let held = held_from_json(json).unwrap();
        let now = org_ts("[2026-10-13 Tue 19:00]").unwrap();
        let ids: Vec<_> = idle_claims(&held, parse_idle("6h").unwrap(), now)
            .into_iter()
            .map(|h| h.id)
            .collect();
        assert_eq!(ids, ["a-1", "a-3"]);
    }
}

#[cfg(test)]
mod log_tests {
    #[test]
    fn recall_log_appends_one_line_per_call() {
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var_os("XDG_STATE_HOME");
        std::env::set_var("XDG_STATE_HOME", dir.path());
        crate::log_recall("recall", "a-1");
        crate::log_recall("search", "two\nlines");
        match old {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
        let log = std::fs::read_to_string(dir.path().join("ljos/recall.log")).unwrap();
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(" recall a-1"));
        assert!(lines[1].ends_with(" search two lines"));
    }
}
