//! The tracker's `vissue.toml` seat list, and a close grant recorded there.
//!
//! `ljos seat add` writes a seat into that list. `ljos seat grant` records
//! the owner's quoted yes that a seat may close tickets it filed or holds.
//! A grant written into the file by hand does not count: the commit named
//! in the row has to be the one this command made, and the quote, the time
//! and the recording seat have to match that commit.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::PathCommit;

/// What `vissue.toml` says about who may write.
pub(crate) enum SeatList {
    /// No tracker, or a tracker whose file names no `seats` key.
    Absent,
    /// The file names a list. It may be empty.
    Listed {
        path: PathBuf,
        seats: std::collections::BTreeSet<String>,
    },
    /// The file is there and does not read.
    Broken { why: String },
}

/// The roster [`crate::registered_seat`] checks.
pub(crate) fn seat_list() -> SeatList {
    let Some(path) = config_path() else {
        return SeatList::Absent;
    };
    match read_list(&path) {
        Ok(None) => SeatList::Absent,
        Ok(Some(seats)) => SeatList::Listed { path, seats },
        Err(why) => SeatList::Broken { why },
    }
}

/// Add `name` to the tracker's seat list and commit that row when the
/// tracker is a git checkout.
///
/// # Errors
///
/// No seat is named, this seat may not write, `name` is not a seat, or
/// the tracker file cannot be written.
pub fn add(name: &str) -> Result<String> {
    let _caller = crate::write_seat("seat add")?;
    let slug = seat_slug_of(name).context("seat add")?;
    let path = config_path().context("seat add: no tracker; set VISSUE_ROOT")?;
    let mut doc = load_doc(&path).context("seat add")?;
    let fresh = insert_seat(&mut doc, &slug);
    write_doc(&path, &doc).context("seat add")?;
    if let Err(why) = commit_if_repo(&path, &commit_subject("add", &slug)) {
        bail!("seat add: {why}");
    }
    if fresh {
        Ok(format!("added {slug} to {}\n", path.display()))
    } else {
        Ok(format!("{slug} is already in {}\n", path.display()))
    }
}

/// Record a `close=own` grant for `seat`.
///
/// `spec` is `close=own`. `quote` is the owner's words, `at` is when they
/// were said, and `via` is the seat writing them down. That seat is this
/// process, and it is not `seat`.
///
/// # Errors
///
/// The arguments do not name that grant, this seat is the one being
/// granted, the tracker is not a git checkout, or the file cannot be written.
pub fn grant(seat: &str, spec: &str, quote: &str, at: &str, via: &str) -> Result<String> {
    if spec.trim() != "close=own" {
        bail!("seat grant: the only grant is close=own. Nothing was written");
    }
    let quote = quote.trim();
    if quote.is_empty() {
        bail!("seat grant: pass the owner's words with --quote. Nothing was written");
    }
    if chrono::DateTime::parse_from_rfc3339(at.trim()).is_err() {
        bail!("seat grant: --at {at:?} is not RFC 3339. Nothing was written");
    }
    let target = seat_slug_of(seat).context("seat grant")?;
    let via = seat_slug_of(via).context("seat grant")?;
    if via == target {
        bail!("seat grant: a seat cannot grant itself. Nothing was written");
    }
    let caller = crate::write_seat("seat grant")?;
    let caller_slug = crate::canonical_seat_name(&caller.seat);
    if caller_slug == target {
        bail!("seat grant: a seat cannot grant itself. Nothing was written");
    }
    if caller_slug != via {
        bail!("seat grant: --via names {via}, and this seat is {caller_slug}. Nothing was written");
    }
    let path = config_path().context("seat grant: no tracker; set VISSUE_ROOT")?;
    let mut doc = load_doc(&path).context("seat grant")?;
    if !seat_known(&doc, &target) {
        bail!(
            "seat grant: no seat named {target} in {}. Add it with `ljos seat add {target}`. Nothing was written",
            path.display()
        );
    }
    let root = path.parent().unwrap_or(Path::new("."));
    if !is_repo(root) {
        bail!(
            "seat grant: the tracker is not a git checkout, so the grant has no commit. Nothing was written"
        );
    }
    set_grant(&mut doc, &target, quote, at.trim(), &via, None);
    write_doc(&path, &doc).context("seat grant")?;
    commit_if_repo(&path, &commit_subject("grant close own", &target))?;
    let sha = head_sha(root).context("seat grant")?;
    set_grant(&mut doc, &target, quote, at.trim(), &via, Some(&sha));
    write_doc(&path, &doc).context("seat grant")?;
    commit_if_repo(&path, "seat: record grant commit")?;
    Ok(format!(
        "granted {target} close of its own tickets, commit {sha}, recorded by {via}\n"
    ))
}

/// The note a granted close should write, or `None` when a person is here
/// and no grant is required.
///
/// # Errors
///
/// No person is here and no standing grant covers this seat, or the grant
/// does not cover `issue`.
pub fn close_note(issue: &str) -> Result<Option<String>> {
    if crate::person_present() {
        return Ok(None);
    }
    let seat = crate::seat_name();
    let Some(grant) = honored_grant(&seat) else {
        let slug = crate::canonical_seat_name(&seat);
        bail!(
            "finish: a person closes a ticket. A standing grant is \
             `ljos seat grant {slug} close=own --quote TEXT --at TIME --via SEAT`. \
             Nothing was written"
        );
    };
    if !owns_issue(issue, &seat)? {
        bail!(
            "finish: {issue} is not {seat}'s to close. A grant of own covers a ticket this seat filed or holds. Nothing was written"
        );
    }
    Ok(Some(grant.note()))
}

struct Grant {
    quote: String,
    at: String,
    via: String,
    commit: String,
}

impl Grant {
    fn note(&self) -> String {
        format!(
            "standing grant allowed this close. quote: {}. at: {}. via: {}. commit: {}.",
            self.quote, self.at, self.via, self.commit
        )
    }
}

fn honored_grant(seat: &str) -> Option<Grant> {
    let path = config_path()?;
    let doc = load_doc(&path).ok()?;
    let slug = crate::canonical_seat_name(seat);
    let row = grant_row(&doc, &slug)?;
    if row.close != "own" || row.quote.is_empty() || row.at.is_empty() || row.via.is_empty() {
        return None;
    }
    if row.commit.is_empty() || row.via == slug {
        return None;
    }
    let root = path.parent()?;
    if !commit_is_ancestor(root, &row.commit) {
        return None;
    }
    let then = toml_at(root, &row.commit).ok()?;
    let then_doc = toml::from_str::<toml::Value>(&then).ok()?;
    let then_row = grant_row(&then_doc, &slug)?;
    if !seat_known(&then_doc, &row.via) {
        return None;
    }
    if then_row.close != row.close
        || then_row.quote != row.quote
        || then_row.at != row.at
        || then_row.via != row.via
    {
        return None;
    }
    Some(Grant {
        quote: row.quote,
        at: row.at,
        via: row.via,
        commit: row.commit,
    })
}

struct GrantRow {
    close: String,
    quote: String,
    at: String,
    via: String,
    commit: String,
}

fn grant_row(doc: &toml::Value, slug: &str) -> Option<GrantRow> {
    let table = doc.get("seats")?.as_table()?;
    let (_, row) = table
        .iter()
        .find(|(key, _)| crate::canonical_seat_name(key) == slug)?;
    let row = row.as_table()?;
    let field = |name: &str| {
        row.get(name)
            .and_then(toml::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    Some(GrantRow {
        close: field("close"),
        quote: field("quote"),
        at: field("at"),
        via: crate::canonical_seat_name(&field("via")),
        commit: field("commit"),
    })
}

fn owns_issue(issue: &str, seat: &str) -> Result<bool> {
    let card = crate::issue_record(issue)?;
    let slug = crate::canonical_seat_name(seat);
    if let Some(by) = card.get("claimed_by").and_then(toml_str) {
        if crate::canonical_seat_name(by) == slug {
            return Ok(true);
        }
    }
    let Some(notes) = card.get("logbook").and_then(serde_json::Value::as_array) else {
        return Ok(false);
    };
    for entry in notes {
        let Some(note) = entry.get("note").and_then(toml_str) else {
            continue;
        };
        let Some(who) = note.strip_prefix("Filed by ") else {
            continue;
        };
        let who = who.split_whitespace().next().unwrap_or("");
        if !who.is_empty() && crate::canonical_seat_name(who) == slug {
            return Ok(true);
        }
    }
    Ok(false)
}

fn toml_str(value: &serde_json::Value) -> Option<&str> {
    value.as_str()
}

fn seat_slug_of(name: &str) -> Result<String> {
    let held = crate::claim_holder(name)?;
    let slug = crate::canonical_seat_name(&held);
    if slug.is_empty() || slug == "runner" {
        bail!("pass a seat name, not {name:?}");
    }
    Ok(slug)
}

fn config_path() -> Option<PathBuf> {
    if let Ok(layout) = vissue_core::Layout::resolve(None, None) {
        return Some(layout.root().join("vissue.toml"));
    }
    let root = std::env::var_os("VISSUE_ROOT")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("ISSUE_ROOT").filter(|v| !v.is_empty()))?;
    Some(PathBuf::from(root).join("vissue.toml"))
}

/// `None` when the file is missing or has no `seats` key.
fn read_list(
    path: &Path,
) -> std::result::Result<Option<std::collections::BTreeSet<String>>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            if text.trim().is_empty() {
                return Ok(None);
            }
            let value: toml::Value = toml::from_str(&text)
                .map_err(|e| format!("{} is not TOML: {e}", path.display()))?;
            Ok(names_of(&value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(path).is_ok() {
                return Err(format!(
                    "{} links to a file that is not there",
                    path.display()
                ));
            }
            Ok(None)
        }
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

fn names_of(value: &toml::Value) -> Option<std::collections::BTreeSet<String>> {
    let seats = value.get("seats")?;
    let mut names = std::collections::BTreeSet::new();
    match seats {
        toml::Value::Array(list) => {
            for item in list {
                if let Some(name) = item.as_str() {
                    insert_name(&mut names, name);
                }
            }
        }
        toml::Value::Table(table) => {
            for key in table.keys() {
                insert_name(&mut names, key);
            }
        }
        _ => return Some(names),
    }
    Some(names)
}

fn insert_name(names: &mut std::collections::BTreeSet<String>, raw: &str) {
    if crate::is_raw_subagent_name(raw) {
        return;
    }
    let slug = crate::canonical_seat_name(raw);
    if slug.is_empty() || slug == "runner" {
        return;
    }
    names.insert(slug);
}

fn load_doc(path: &Path) -> Result<toml::Value> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(empty_doc()),
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("{} is not TOML", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(path).is_ok() {
                bail!("{} links to a file that is not there", path.display());
            }
            Ok(empty_doc())
        }
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn empty_doc() -> toml::Value {
    toml::Value::Table(toml::Table::new())
}

fn write_doc(path: &Path, doc: &toml::Value) -> Result<()> {
    let mut text = toml::to_string_pretty(doc).context("vissue.toml")?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.new");
    std::fs::write(&tmp, &text).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

/// `true` when `slug` was not already a seat.
fn insert_seat(doc: &mut toml::Value, slug: &str) -> bool {
    let table = seats_mut(doc);
    let fresh = !table
        .keys()
        .any(|key| crate::canonical_seat_name(key) == slug);
    table
        .entry(slug.to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    fresh
}

fn seat_known(doc: &toml::Value, slug: &str) -> bool {
    names_of(doc).is_some_and(|names| names.contains(slug))
}

fn set_grant(
    doc: &mut toml::Value,
    slug: &str,
    quote: &str,
    at: &str,
    via: &str,
    commit: Option<&str>,
) {
    let table = seats_mut(doc);
    let row = table
        .entry(slug.to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !row.is_table() {
        *row = toml::Value::Table(toml::Table::new());
    }
    let Some(row) = row.as_table_mut() else {
        return;
    };
    row.insert("close".into(), toml::Value::String("own".into()));
    row.insert("quote".into(), toml::Value::String(quote.to_string()));
    row.insert("at".into(), toml::Value::String(at.to_string()));
    row.insert("via".into(), toml::Value::String(via.to_string()));
    if let Some(commit) = commit {
        row.insert("commit".into(), toml::Value::String(commit.to_string()));
    } else {
        row.remove("commit");
    }
}

fn seats_mut(doc: &mut toml::Value) -> &mut toml::Table {
    if !doc.is_table() {
        *doc = empty_doc();
    }
    let root = doc.as_table_mut().expect("doc is a table");
    let next = match root.remove("seats") {
        Some(toml::Value::Table(table)) => table,
        Some(toml::Value::Array(list)) => {
            let mut table = toml::Table::new();
            for item in list {
                if let Some(name) = item.as_str() {
                    let slug = crate::canonical_seat_name(name);
                    if !slug.is_empty() && slug != "runner" && !crate::is_raw_subagent_name(name) {
                        table
                            .entry(slug)
                            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                    }
                }
            }
            table
        }
        _ => toml::Table::new(),
    };
    root.insert("seats".into(), toml::Value::Table(next));
    root.get_mut("seats")
        .and_then(toml::Value::as_table_mut)
        .expect("seats is a table")
}

fn commit_subject(verb: &str, seat: &str) -> String {
    let full = format!("seat: {verb} {seat}");
    if full.chars().count() <= 50 {
        full
    } else {
        let bare = format!("seat: {verb}");
        if bare.chars().count() <= 50 {
            bare
        } else {
            "seat: update roster".to_string()
        }
    }
}

fn commit_if_repo(path: &Path, message: &str) -> Result<()> {
    let Some(root) = path.parent() else {
        return Ok(());
    };
    if !is_repo(root) {
        return Ok(());
    }
    let _held = crate::tracker_commit_lock(root);
    match crate::commit_paths(root, &["vissue.toml"], message) {
        PathCommit::Made | PathCommit::AlreadyIn => Ok(()),
        PathCommit::Refused(why) => bail!("{why}"),
    }
}

fn is_repo(root: &Path) -> bool {
    crate::git_at(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()
        .is_some_and(|out| out.status.success() && out.stdout.starts_with(b"true"))
}

fn head_sha(root: &Path) -> Result<String> {
    let out = crate::git_at(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("git rev-parse")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() {
        bail!("git rev-parse printed nothing");
    }
    Ok(sha)
}

fn commit_is_ancestor(root: &Path, sha: &str) -> bool {
    crate::git_at(root)
        .args(["merge-base", "--is-ancestor", sha, "HEAD"])
        .status()
        .is_ok_and(|status| status.success())
}

fn toml_at(root: &Path, sha: &str) -> Result<String> {
    let spec = format!("{sha}:vissue.toml");
    let out = crate::git_at(root)
        .args(["show", &spec])
        .output()
        .context("git show")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod grant_tests {
    use super::honored_grant;

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?} {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A grant commit that is not an ancestor of HEAD, or whose `via` was
    /// not a listed seat, is not a standing grant.
    #[test]
    fn a_grant_must_be_on_head_and_via_a_listed_seat() {
        let saved: Vec<(&str, Option<std::ffi::OsString>)> =
            ["VISSUE_ROOT", "ISSUE_ROOT", "VISSUE_NO_ROUTE"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect();
        struct PutBack(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for PutBack {
            fn drop(&mut self) {
                for (key, value) in &self.0 {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
        let _restore = PutBack(saved);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "ada@example.invalid"]);
        git(root, &["config", "user.name", "ada"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        let listed = "\
[seats.ada]
close = \"own\"
quote = \"yes ada may close its own\"
at = \"2026-10-29T00:00:00Z\"
via = \"bea\"

[seats.bea]
";
        std::fs::write(root.join("vissue.toml"), listed).unwrap();
        git(root, &["add", "vissue.toml"]);
        git(root, &["commit", "-q", "-m", "roster"]);
        let good = git(root, &["rev-parse", "HEAD"]);
        let on_head = listed.replace(
            "via = \"bea\"\n",
            &format!("via = \"bea\"\ncommit = \"{good}\"\n"),
        );
        std::fs::write(root.join("vissue.toml"), &on_head).unwrap();
        git(root, &["add", "vissue.toml"]);
        git(root, &["commit", "-q", "-m", "record grant"]);
        std::env::set_var("VISSUE_ROOT", root);
        std::env::set_var("ISSUE_ROOT", root);
        std::env::set_var("VISSUE_NO_ROUTE", "1");
        assert!(honored_grant("ada").is_some());

        let orphan = git(
            root,
            &[
                "commit-tree",
                &git(root, &["rev-parse", "HEAD^{tree}"]),
                "-m",
                "off to the side",
            ],
        );
        let side = on_head.replace(&good, &orphan);
        std::fs::write(root.join("vissue.toml"), side).unwrap();
        assert!(
            honored_grant("ada").is_none(),
            "a commit that is not an ancestor of HEAD is not a grant"
        );

        let ghost = on_head.replace("via = \"bea\"", "via = \"ghost\"");
        std::fs::write(root.join("vissue.toml"), ghost).unwrap();
        assert!(
            honored_grant("ada").is_none(),
            "via must be a seat listed in the grant commit"
        );
    }
}
