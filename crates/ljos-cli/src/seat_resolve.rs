//! One seat resolver.
//!
//! A command used to stop when no seat was exported. The seat is now the
//! first match in a fixed order, and a miss still runs the command.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

/// The seat name a read uses when no rule matched. Search, recall, status
/// and doctor then read every seat.
pub const EVERY_SEAT: &str = "every-seat";

/// Entity added to a memory written with no seat resolved.
pub const UNRESOLVED_LABEL: &str = "seat:unresolved";

/// What a command should do when the six rules miss.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Keep the older resolution, including the login user.
    Keep,
    /// Read every seat.
    Read,
    /// Write as `unknown-<hostname>`.
    Write,
}

thread_local! {
    static FLAG: RefCell<Option<String>> = const { RefCell::new(None) };
    static KIND: Cell<Kind> = const { Cell::new(Kind::Keep) };
    static NOTICED: Cell<bool> = const { Cell::new(false) };
}

/// Remember `--seat` and whether this command reads or writes.
/// A write with no seat prints one line.
pub fn prepare(flag: Option<String>, kind: Kind) {
    let flag = flag.and_then(|name| usable(&name));
    FLAG.with(|slot| *slot.borrow_mut() = flag);
    KIND.with(|cell| cell.set(kind));
    if kind == Kind::Write && named_from_env().is_none() {
        let seat = unknown_for(&crate::sync::host());
        if !NOTICED.with(|cell| cell.replace(true)) {
            eprintln!("{}", notice(&seat));
        }
    }
}

/// The seat and the rule phrase, when one of the six rules matches.
pub(crate) fn named_from_env() -> Option<(String, &'static str)> {
    let flag = FLAG.with(|slot| slot.borrow().clone());
    let ljos_seat = std::env::var("LJOS_SEAT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let env_file = read_files("env");
    let map = resolved_map();
    let parent = parent_record();
    let write_log = writes_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let host = crate::sync::host();
    let inputs = Inputs {
        flag: flag.as_deref(),
        ljos_seat: ljos_seat.as_deref(),
        env_file: &env_file,
        map_seat: map.as_ref().map(|(name, rule)| (name.as_str(), *rule)),
        parent_seat: parent.as_deref(),
        write_log: &write_log,
        host: &host,
    };
    pick(&inputs)
}

/// The seat a read or a write uses when [`named_from_env`] missed.
/// `None` for [`Kind::Keep`].
pub(crate) fn command_fallback() -> Option<crate::Seat> {
    let kind = KIND.with(|cell| cell.get());
    fallback_seat(kind, &crate::sync::host())
}

/// A seat for a call that used to return `seat-unresolved`.
pub(crate) fn unresolved_seat() -> crate::Seat {
    command_fallback().unwrap_or_else(|| unknown_seat(&crate::sync::host()))
}

/// True when `mine` is the read-every-seat sentinel.
#[must_use]
pub fn reads_every(mine: &str) -> bool {
    mine == EVERY_SEAT
}

/// One plain line for a write that had to pick a seat.
#[must_use]
pub fn notice(seat: &str) -> String {
    format!(
        "ljos: no seat was named; this write uses {seat}. Set one with --seat NAME or export LJOS_SEAT=NAME."
    )
}

/// Append one write so a later command can see the most recent seat.
pub fn note_seat_write(seat: &str) {
    if usable(seat).is_none() || reads_every(seat) {
        return;
    }
    let Some(path) = writes_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let line = format!("{}\t{seat}\t{}\n", crate::now_utc(), crate::sync::host());
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write as _;
            file.write_all(line.as_bytes())
        });
}

/// Inputs the six rules read. Tests build this directly.
pub(crate) struct Inputs<'a> {
    pub flag: Option<&'a str>,
    pub ljos_seat: Option<&'a str>,
    pub env_file: &'a str,
    /// A seats.map or seats.toml hit, already keyed by the agent or
    /// conversation id.
    pub map_seat: Option<(&'a str, &'static str)>,
    pub parent_seat: Option<&'a str>,
    pub write_log: &'a str,
    pub host: &'a str,
}

/// The first matching rule: the seat and the phrase doctor prints.
pub(crate) fn pick(inputs: &Inputs<'_>) -> Option<(String, &'static str)> {
    if let Some(name) = inputs.flag.and_then(usable) {
        return Some((name, "--seat"));
    }
    if let Some(name) = inputs.ljos_seat.and_then(usable) {
        return Some((name, "LJOS_SEAT"));
    }
    if let Some(name) = seat_line(inputs.env_file) {
        return Some((name, "seat= line in the env file"));
    }
    if let Some((name, rule)) = inputs.map_seat {
        if let Some(name) = usable(name) {
            return Some((name, rule));
        }
    }
    if let Some(name) = inputs.parent_seat.and_then(usable) {
        return Some((name, "parent process seat record"));
    }
    if let Some(name) = recent_line(inputs.write_log, inputs.host) {
        return Some((name, "most recent write from this machine"));
    }
    None
}

/// The six rules plus the read and write fallbacks. `None` when `kind`
/// is [`Kind::Keep`] and no rule matched.
#[cfg(test)]
pub(crate) fn choose(inputs: &Inputs<'_>, kind: Kind) -> Option<(String, &'static str)> {
    if let Some(found) = pick(inputs) {
        return Some(found);
    }
    fallback_seat(kind, inputs.host).map(|seat| {
        let rule = if seat.seat == EVERY_SEAT {
            "no seat rule matched; reading every seat"
        } else {
            "no seat rule matched; using an unknown seat"
        };
        (seat.seat, rule)
    })
}

fn fallback_seat(kind: Kind, host: &str) -> Option<crate::Seat> {
    match kind {
        Kind::Keep => None,
        Kind::Read => Some(whole(
            EVERY_SEAT,
            "no seat rule matched; reading every seat",
        )),
        Kind::Write => {
            let name = unknown_for(host);
            Some(whole(&name, "no seat rule matched; using an unknown seat"))
        }
    }
}

fn unknown_seat(host: &str) -> crate::Seat {
    let name = unknown_for(host);
    whole(&name, "no seat rule matched; using an unknown seat")
}

fn whole(name: &str, source: &str) -> crate::Seat {
    crate::Seat {
        seat: name.to_string(),
        holder: name.to_string(),
        source: source.to_string(),
    }
}

fn unknown_for(host: &str) -> String {
    let host = host.trim();
    if host.is_empty() {
        "unknown-host".to_string()
    } else {
        format!("unknown-{host}")
    }
}

/// A name that can be a seat. A subagent id, a pronoun and an empty
/// string are not.
pub(crate) fn usable(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || crate::omitted_actor_name(name) || crate::is_raw_subagent_name(name) {
        return None;
    }
    let mapped = crate::seat_for_env(name);
    if mapped.is_empty()
        || mapped == "runner"
        || crate::omitted_actor_name(&mapped)
        || crate::is_raw_subagent_name(&mapped)
    {
        return None;
    }
    Some(mapped)
}

fn read_files(name: &str) -> String {
    let mut out = String::new();
    for path in config_files(name) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            out.push_str(&text);
            if !text.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

fn config_files(name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(base) = crate::config_base() {
        out.push(base.join("ljos").join(name));
    }
    // `~/.config/ljos` is the file `load_seat_env` reads. Unit tests keep
    // the isolated config directory and do not read the login's files.
    #[cfg(not(test))]
    if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home).join(".config/ljos").join(name);
        if !out.iter().any(|have| have == &path) {
            out.push(path);
        }
    }
    out
}

fn resolved_map() -> Option<(String, &'static str)> {
    let text = read_files("seats.map");
    let ids = lookup_ids();
    if let Some(name) = map_hit(&text, &ids) {
        return Some((name, "seats.map"));
    }
    crate::mapped_box_seat()
        .and_then(|name| usable(&name))
        .map(|name| (name, "seats.toml"))
}

fn parent_record() -> Option<String> {
    for (pid, _) in crate::ancestry() {
        let path = crate::runtime_dir().join(format!("seat-{pid}"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(line) = text.lines().next() else {
            continue;
        };
        if let Some(name) = usable(line) {
            return Some(name);
        }
    }
    None
}

fn writes_path() -> Option<PathBuf> {
    #[cfg(test)]
    std::env::var_os("XDG_STATE_HOME")?;
    Some(crate::state_base_dir().join("seat-writes.log"))
}

fn seat_line(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "seat" {
            continue;
        }
        if let Some(name) = usable(value) {
            return Some(name);
        }
    }
    None
}

pub(crate) fn map_hit(text: &str, ids: &[String]) -> Option<String> {
    if text.is_empty() || ids.is_empty() {
        return None;
    }
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = if let Some((key, value)) = line.split_once('=') {
            (key.trim(), value.trim())
        } else {
            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap_or("");
            let value = parts.next().unwrap_or("");
            (key, value)
        };
        if key.is_empty() || !ids.iter().any(|id| id == key) {
            continue;
        }
        if let Some(name) = usable(value) {
            return Some(name);
        }
    }
    None
}

fn lookup_ids() -> Vec<String> {
    let mut ids = Vec::new();
    let mut push = |value: String| {
        let value = value.trim().to_string();
        if value.len() >= 8 && !ids.iter().any(|have| have == &value) {
            ids.push(value);
        }
    };
    for (_, id) in crate::stamped_sessions() {
        push(id);
    }
    for key in [
        "CURSOR_AGENT_ID",
        "CLOUD_AGENT_ID",
        "AGENT_ID",
        "CURSOR_CONVERSATION_ID",
        "CODESPACE_NAME",
        "GITHUB_RUN_ID",
    ] {
        if let Ok(value) = std::env::var(key) {
            push(value);
        }
    }
    ids
}

fn recent_line(text: &str, host: &str) -> Option<String> {
    let host = host.trim();
    if host.is_empty() || text.is_empty() {
        return None;
    }
    let mut best: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let Some(ts) = parts.next() else {
            continue;
        };
        let Some(seat) = parts.next() else {
            continue;
        };
        let Some(line_host) = parts.next() else {
            continue;
        };
        if line_host.trim() != host {
            continue;
        }
        let Some(seat) = usable(seat) else {
            continue;
        };
        let ts = ts.trim().to_string();
        match best.get(&seat) {
            Some(have) if have.as_str() >= ts.as_str() => {}
            _ => {
                best.insert(seat, ts);
            }
        }
    }
    let max = best.values().max().cloned()?;
    let mut seats: Vec<String> = best
        .into_iter()
        .filter(|(_, ts)| ts == &max)
        .map(|(seat, _)| seat)
        .collect();
    if seats.len() == 1 {
        seats.pop()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Inputs<'static> {
        Inputs {
            flag: None,
            ljos_seat: None,
            env_file: "",
            map_seat: None,
            parent_seat: None,
            write_log: "",
            host: "box",
        }
    }

    #[test]
    fn each_rule_stops_at_the_first_match_and_the_flag_wins() {
        let mut inputs = blank();
        inputs.flag = Some("saddle");
        inputs.ljos_seat = Some("envy");
        inputs.env_file = "seat=filed\n";
        inputs.map_seat = Some(("saddle-map", "seats.map"));
        inputs.parent_seat = Some("parent-seat");
        inputs.write_log = "2026-11-11T00:00:00Z\trecent\tbox\n";
        assert_eq!(pick(&inputs).unwrap(), ("saddle".into(), "--seat"));

        inputs.flag = None;
        assert_eq!(pick(&inputs).unwrap(), ("envy".into(), "LJOS_SEAT"));

        inputs.ljos_seat = None;
        assert_eq!(
            pick(&inputs).unwrap(),
            ("filed".into(), "seat= line in the env file")
        );

        inputs.env_file = "";
        assert_eq!(pick(&inputs).unwrap(), ("saddle-map".into(), "seats.map"));

        inputs.map_seat = Some(("toml-seat", "seats.toml"));
        assert_eq!(pick(&inputs).unwrap(), ("toml-seat".into(), "seats.toml"));

        inputs.map_seat = None;
        assert_eq!(
            pick(&inputs).unwrap(),
            ("parent-seat".into(), "parent process seat record")
        );

        inputs.parent_seat = None;
        assert_eq!(
            pick(&inputs).unwrap(),
            ("recent".into(), "most recent write from this machine")
        );
    }

    #[test]
    fn a_map_line_needs_the_agent_or_conversation_id() {
        let text = "# note\nbc-1234abcd = saddle\nother-id other\n";
        assert_eq!(
            map_hit(text, &["bc-1234abcd".into()]).as_deref(),
            Some("saddle")
        );
        assert!(map_hit(text, &["missing-id".into()]).is_none());
        assert!(map_hit("bc-1234abcd = sand-subagent-001\n", &["bc-1234abcd".into()]).is_none());
    }

    #[test]
    fn two_recent_seats_are_not_one_candidate() {
        let log = "\
2026-11-11T00:00:00Z\talice\tbox
2026-11-11T00:00:01Z\tbob\tbox
2026-11-11T00:00:01Z\talice\tbox
";
        assert!(recent_line(log, "box").is_none());
        let one = "\
2026-11-10T00:00:00Z\talice\tbox
2026-11-11T00:00:00Z\tbob\tbox
2026-11-11T00:00:00Z\tbob\tother
";
        assert_eq!(recent_line(one, "box").as_deref(), Some("bob"));
        assert!(recent_line(one, "elsewhere").is_none());
    }

    #[test]
    fn a_subagent_name_is_not_a_seat_and_the_next_rule_runs() {
        let mut inputs = blank();
        inputs.flag = Some("sand-subagent-001");
        inputs.ljos_seat = Some("example-host-sand-sub");
        inputs.env_file = "seat=saddle\n";
        assert_eq!(
            pick(&inputs).unwrap(),
            ("saddle".into(), "seat= line in the env file")
        );
    }

    #[test]
    fn reads_fall_back_to_every_seat_and_writes_to_unknown() {
        let inputs = blank();
        assert!(pick(&inputs).is_none());
        assert_eq!(
            choose(&inputs, Kind::Read).unwrap(),
            (
                EVERY_SEAT.into(),
                "no seat rule matched; reading every seat"
            )
        );
        let (seat, rule) = choose(&inputs, Kind::Write).unwrap();
        assert_eq!(seat, "unknown-box");
        assert_eq!(rule, "no seat rule matched; using an unknown seat");
        assert!(choose(&inputs, Kind::Keep).is_none());
        let line = notice("unknown-box");
        assert!(line.contains("unknown-box"), "{line}");
        assert!(line.contains("--seat"), "{line}");
        assert!(line.contains("LJOS_SEAT"), "{line}");
        assert!(!line.contains('\n'), "{line}");
    }

    #[test]
    fn an_explicit_seat_wins_over_a_later_rule() {
        let mut inputs = blank();
        inputs.flag = Some("saddle");
        inputs.ljos_seat = Some("other");
        let found = choose(&inputs, Kind::Write).unwrap();
        assert_eq!(found.0, "saddle");
        assert_eq!(found.1, "--seat");
    }
}
