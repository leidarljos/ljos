//! Launch slots. The seat that launches a cloud run leaves a `slot:` note
//! on the issue it hands the run: the seat the run will vote as, the model
//! it asked for, the run id and the public key the run minted on its
//! machine, signed under the launcher's pinned key. The slot names the
//! seat and its key before the tracker pins any key for it, so a writer
//! that pins the seat first under another key does not vote as the run.
//!
//! A pin alone is what any tracker writer can make, and the author date on
//! that commit is whatever the writer sets. A seat stands when its pinned
//! key was already committed when slots began in this tracker ([`stood`]):
//! in the parents of the commit that brought the first slot note, by
//! commit order. On a tracker that is not a git repository the names on a
//! list in this binary stand instead ([`standing`]). Any other seat counts
//! toward a push cite only under a slot on that issue ([`problems`]). The
//! slot has to already be in that issue's own entry in the commit that
//! introduced the pin. A copy of the line on another issue does not
//! count, and a git date is not read.

use anyhow::{bail, Context, Result};
use ed25519_dalek::SigningKey;
use std::collections::BTreeMap;
use std::path::Path;

use crate::ballot_sig;

/// The note prefix a slot rides under.
pub const SLOT_NOTE: &str = "slot: ";

/// When the standing list was frozen: the slot change, 2026-10-21T23:48:25Z.
/// The list is the policy. This instant is not compared with a git time.
pub const SLOTS_SINCE: i64 = 1_792_626_505;

/// Seats that stood at [`SLOTS_SINCE`]. One name per line. A `#` line is a
/// comment.
const STANDING_SEATS: &str = include_str!("standing_seats.txt");

/// Whether `seat` is on the list that stands on a tracker that is not a
/// git repository ([`stands`]).
#[must_use]
pub fn standing(seat: &str) -> bool {
    let seat = seat.trim();
    !seat.is_empty()
        && STANDING_SEATS.lines().any(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#') && line == seat
        })
}

/// The text a slot note carries in a tracker file.
const SLOT_LINE: &str = "Note: \"slot: ";

/// Where slots began in this tracker, by commit order: the parents of each
/// commit that brought a slot note into a tree none of its parents held
/// one in. Empty when no slot is committed yet. `Ok(None)` when the
/// tracker is not a git repository.
///
/// # Errors
///
/// The tracker's history is incomplete after a deepen was tried
/// ([`require_tracker_history`]). Missing parents are not "no slots".
pub fn starts() -> std::result::Result<Option<Vec<Vec<String>>>, String> {
    match ballot_sig::env_root() {
        Some(root) => starts_in(&root),
        None => Ok(None),
    }
}

/// [`starts`] for the tracker at `root`.
///
/// # Errors
///
/// As [`starts`].
pub fn starts_in(root: &Path) -> std::result::Result<Option<Vec<Vec<String>>>, String> {
    if ballot_sig::git_in(root, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true")
    {
        return Ok(None);
    }
    require_tracker_history(root)?;
    let pick = format!("-S{SLOT_LINE}");
    let log = ballot_sig::git_in(
        root,
        &[
            "--no-pager",
            "log",
            "--full-history",
            "--format=%H %P",
            pick.as_str(),
        ],
    )
    .unwrap_or_default();
    let held = |commit: &str| {
        ballot_sig::git_in(
            root,
            &["--no-pager", "grep", "-q", "-F", "-e", SLOT_LINE, commit],
        )
        .is_some()
    };
    let mut out = Vec::new();
    for line in log.lines() {
        let mut parts = line.split_whitespace();
        let Some(commit) = parts.next() else {
            continue;
        };
        let parents: Vec<String> = parts.map(str::to_string).collect();
        if held(commit) && !parents.iter().any(|p| held(p)) {
            out.push(parents);
        }
    }
    Ok(Some(out))
}

/// Whether `seat`'s pin `key` was committed before slots began: its line
/// is in the pin file of every parent in `starts`. A slot in a root commit
/// leaves nothing before it. With no slot committed yet, a pin committed at
/// `HEAD` stood. A missing parent is not a pin that was absent.
///
/// # Errors
///
/// [`require_tracker_history`] could not make the history readable.
pub fn stood(starts: &[Vec<String>], seat: &str, key: &str) -> std::result::Result<bool, String> {
    match ballot_sig::env_root() {
        Some(root) => stood_in(&root, starts, seat, key),
        None => Ok(false),
    }
}

/// [`stood`] in the tracker at `root`.
///
/// # Errors
///
/// As [`stood`].
pub fn stood_in(
    root: &Path,
    starts: &[Vec<String>],
    seat: &str,
    key: &str,
) -> std::result::Result<bool, String> {
    require_tracker_history(root)?;
    if starts.is_empty() {
        return pin_line_at_in(root, "HEAD", seat, key);
    }
    for parents in starts {
        if parents.is_empty() {
            return Ok(false);
        }
        for parent in parents {
            if !pin_line_at_in(root, parent, seat, key)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Whether `seat`, pinned to `key`, needs no slot and may sign one: its
/// pin [`stood`]. A name alone does not stand on a git tracker, since the
/// first writer to sign as an unpinned name pins it. `starts` is `None` on
/// a tracker that is not a git repository, which keeps no order, and there
/// the list stands.
///
/// # Errors
///
/// [`require_tracker_history`] could not make the history readable.
pub fn stands(
    seat: &str,
    key: Option<&str>,
    starts: Option<&[Vec<String>]>,
) -> std::result::Result<bool, String> {
    match starts {
        None => Ok(standing(seat)),
        Some(at) => match key {
            Some(key) => stood(at, seat, key),
            None => Ok(false),
        },
    }
}

/// The refusal when pin order cannot be read. Names the cause: the
/// fetch a caller runs, a `refs/replace` ref, or a grafts file.
pub const HISTORY_INCOMPLETE: &str = "tracker history incomplete";

/// Make `root`'s history readable for pin order. A shallow checkout, or
/// one whose objects are missing, is deepened with `git fetch --unshallow`
/// when it has a remote, then with a deepen of that remote if it is still
/// shallow. `refs/replace` and a grafts file rewrite parents without
/// removing objects, so those are refused instead of deepened. A tracker
/// that is not a git repository is left alone.
///
/// # Errors
///
/// History is still incomplete, or parents are rewritten. The error is
/// [`HISTORY_INCOMPLETE`]. A shallow checkout names `git fetch
/// --unshallow`. A checkout that is not shallow and still misses
/// objects, as a partial clone does, names `git fetch --refetch
/// --no-filter`. A replace ref or a grafts file names that cause.
/// Missing history is not "stood".
pub fn require_tracker_history(root: &Path) -> std::result::Result<(), String> {
    if ballot_sig::git_in(root, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true")
    {
        return Ok(());
    }
    // A deepen does not delete replace refs or a grafts file. Refuse
    // before the ready cache: a checkout that was complete can gain
    // either later, and the cached answer would then be the rewrite.
    if let Some(why) = parent_rewrite(root) {
        return Err(format!("{HISTORY_INCOMPLETE}: {why}"));
    }
    let key = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // A second caller waits for the first one's fetch, then reads the
    // cache and the checkout again, rather than losing git's shallow.lock.
    let gate = history_gate(&key);
    let _held = gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (ready, deepened) = {
        let cache = history_cache();
        (
            cache.ready.iter().any(|path| path == &key),
            cache.deepened.iter().any(|path| path == &key),
        )
    };
    if ready {
        return Ok(());
    }
    if !history_gap(root) {
        history_cache().ready.push(key);
        return Ok(());
    }
    // Only a fetch that ran to the end is remembered, so a remote that
    // did not answer is tried again on the next check.
    if !deepened {
        if let Some(remote) = remote_name(root) {
            if deepen(root, &remote) {
                history_cache().deepened.push(key.clone());
            }
            // A fetch that failed may have lost to another process that
            // completed the checkout, so read it again either way.
            if !history_gap(root) {
                history_cache().ready.push(key);
                return Ok(());
            }
        }
    }
    if shallow_repo(root) == Some(false) && missing_objects(root) == Some(true) {
        return Err(format!(
            "{HISTORY_INCOMPLETE}: this checkout is not shallow, but objects its history names are missing, \
             as a partial clone leaves them. Run `git fetch --refetch --no-filter` in the tracker, \
             or clone the tracker again without --filter, then retry"
        ));
    }
    Err(format!(
        "{HISTORY_INCOMPLETE}: this checkout does not hold the commits a pin's order is read from. \
         Add the tracker remote if it has none and run `git fetch --unshallow` in the tracker, then retry"
    ))
}

struct HistoryCache {
    ready: Vec<std::path::PathBuf>,
    deepened: Vec<std::path::PathBuf>,
    gates: Vec<(std::path::PathBuf, std::sync::Arc<std::sync::Mutex<()>>)>,
}

fn history_cache() -> std::sync::MutexGuard<'static, HistoryCache> {
    static CACHE: std::sync::Mutex<HistoryCache> = std::sync::Mutex::new(HistoryCache {
        ready: Vec::new(),
        deepened: Vec::new(),
        gates: Vec::new(),
    });
    CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The lock held across the gap check and deepen of the tracker at `key`.
fn history_gate(key: &Path) -> std::sync::Arc<std::sync::Mutex<()>> {
    let mut cache = history_cache();
    if let Some((_, gate)) = cache.gates.iter().find(|(path, _)| path == key) {
        return std::sync::Arc::clone(gate);
    }
    let gate = std::sync::Arc::default();
    cache
        .gates
        .push((key.to_path_buf(), std::sync::Arc::clone(&gate)));
    gate
}

/// Shallow history, missing objects, or parents rewritten by a replace
/// ref or a grafts file. The rewrite is also refused up front in
/// [`require_tracker_history`]: fetching does not remove it. A git that
/// could not say whether the checkout is shallow or missing objects is a
/// gap too.
fn history_gap(root: &Path) -> bool {
    shallow_repo(root) != Some(false)
        || missing_objects(root) != Some(false)
        || parent_rewrite(root).is_some()
}

/// Why `root`'s parent list is not the one in the commit objects, or
/// `None` when pin order can be read from those objects.
fn parent_rewrite(root: &Path) -> Option<String> {
    let replace = replace_refs(root);
    let grafts = grafts_file(root);
    let why = match (replace, grafts) {
        (false, false) => return None,
        (true, false) => {
            "refs/replace rewrites the parents a pin's order is read from. \
             Remove the replace refs and retry"
        }
        (false, true) => {
            ".git/info/grafts rewrites the parents a pin's order is read from. \
             Remove the grafts file and retry"
        }
        (true, true) => {
            "refs/replace and .git/info/grafts rewrite the parents a pin's order is read from. \
             Remove the replace refs and the grafts file and retry"
        }
    };
    Some(why.to_string())
}

fn replace_refs(root: &Path) -> bool {
    ballot_sig::git_in(
        root,
        &["for-each-ref", "--format=%(refname)", "refs/replace"],
    )
    .is_some_and(|text| text.lines().any(|line| line.starts_with("refs/replace/")))
}

fn grafts_file(root: &Path) -> bool {
    let mut paths = Vec::new();
    if let Some(path) = git_abs(
        root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/grafts",
        ],
    ) {
        paths.push(path);
    }
    if let Some(dir) = git_abs(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) {
        paths.push(dir.join("info").join("grafts"));
    }
    paths.push(root.join(".git").join("info").join("grafts"));
    paths.iter().any(|path| path.is_file())
}

fn git_abs(root: &Path, args: &[&str]) -> Option<std::path::PathBuf> {
    let text = ballot_sig::git_in(root, args)?;
    (!text.is_empty()).then(|| std::path::PathBuf::from(text))
}

/// Whether `root` is shallow; `None` when git did not answer.
fn shallow_repo(root: &Path) -> Option<bool> {
    match ballot_sig::git_in(root, &["rev-parse", "--is-shallow-repository"])?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Whether `root` lacks an object reachable from a ref; `None` when the
/// walk failed.
fn missing_objects(root: &Path) -> Option<bool> {
    ballot_sig::git_in(root, &["rev-list", "--objects", "--all", "--missing=print"])
        .map(|text| text.lines().any(|line| line.starts_with('?')))
}

fn remote_name(root: &Path) -> Option<String> {
    ballot_sig::git_in(root, &["remote"]).and_then(|text| {
        text.lines()
            .find(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_string())
    })
}

/// Fetch `remote`'s whole history into `root`. False when a fetch failed
/// or the checkout is still shallow, or not known not to be.
fn deepen(root: &Path, remote: &str) -> bool {
    if !git_fetch(root, &["fetch", "--unshallow", remote]) {
        return false;
    }
    match shallow_repo(root) {
        Some(false) => true,
        Some(true) => {
            git_fetch(root, &["fetch", "--deepen=2147483647", remote])
                && shallow_repo(root) == Some(false)
        }
        None => false,
    }
}

fn git_fetch(root: &Path, args: &[&str]) -> bool {
    let mut cmd = std::process::Command::new("git");
    crate::clear_git_repo_env(&mut cmd);
    cmd.arg("-c")
        .arg("protocol.file.allow=always")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// A slot note's fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub seat: String,
    pub model: String,
    pub run: String,
    /// The public key the run minted, as hex. Its ballots count only when
    /// the tracker pins this key for `seat`.
    pub run_key: String,
    pub launcher: String,
    pub key: String,
    pub sig: String,
}

/// This tracker as a slot names it: its root commits, sorted and joined
/// with `,`. `-` on a tracker that is not a git repository.
#[must_use]
pub fn tracker_id() -> String {
    let roots = ballot_sig::tracker_git(&["rev-list", "--max-parents=0", "HEAD"]);
    let mut roots: Vec<&str> = roots.as_deref().unwrap_or("").split_whitespace().collect();
    roots.sort_unstable();
    if roots.is_empty() {
        "-".into()
    } else {
        roots.join(",")
    }
}

/// The bytes a slot signature covers. The tracker and the issue are in
/// them, so a slot copied to another issue, or to the same issue id on
/// another tracker, does not verify there.
#[must_use]
pub fn message(
    tracker: &str,
    issue: &str,
    seat: &str,
    model: &str,
    run: &str,
    run_key: &str,
    launcher: &str,
) -> Vec<u8> {
    format!(
        "ljos-slot/v3\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        tracker.trim(),
        issue.trim(),
        seat.trim(),
        model.trim(),
        run.trim(),
        run_key.trim(),
        launcher.trim()
    )
    .into_bytes()
}

/// `slot: SEAT model=MODEL run=RUN key=RUNKEY LAUNCHER KEYHEX SIGHEX`. A
/// slot without the run's key does not parse.
#[must_use]
pub fn parse(note: &str) -> Option<Slot> {
    let rest = note.trim().strip_prefix(SLOT_NOTE)?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let [seat, model, run, run_key, launcher, key, sig] = parts.as_slice() else {
        return None;
    };
    Some(Slot {
        seat: (*seat).to_string(),
        model: model.strip_prefix("model=")?.to_string(),
        run: run.strip_prefix("run=")?.to_string(),
        run_key: run_key.strip_prefix("key=")?.to_string(),
        launcher: (*launcher).to_string(),
        key: (*key).to_string(),
        sig: (*sig).to_string(),
    })
}

/// Whether `slot` is a valid signature by its key over this issue's slot
/// on `tracker` ([`tracker_id`]).
#[must_use]
pub fn verifies(tracker: &str, issue: &str, slot: &Slot) -> bool {
    ballot_sig::signature_ok(
        &slot.key,
        &slot.sig,
        &message(
            tracker,
            issue,
            &slot.seat,
            &slot.model,
            &slot.run,
            &slot.run_key,
            &slot.launcher,
        ),
    )
}

/// The note for a slot on `tracker` ([`tracker_id`]) signed by `key`.
/// Each field is one token.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn sign(
    key: &SigningKey,
    tracker: &str,
    issue: &str,
    seat: &str,
    model: &str,
    run: &str,
    run_key: &str,
    launcher: &str,
) -> String {
    let [seat, model, run, run_key, launcher] =
        [seat, model, run, run_key, launcher].map(ballot_sig::field);
    let (public, sig) = ballot_sig::signed_hex(
        key,
        &message(tracker, issue, &seat, &model, &run, &run_key, &launcher),
    );
    format!("{SLOT_NOTE}{seat} model={model} run={run} key={run_key} {launcher} {public} {sig}")
}

/// Sign a slot on `issue` for run `run` on `model`, as this process's
/// seat, and leave it on the issue. `seat` is the `LJOS_SEAT` the run
/// boots with; the run id is appended the way `ljos cloud up` appends it.
/// `run_key` is the public key `ljos cloud up` printed on the run.
///
/// # Errors
///
/// No seat names the launcher, the launcher has no pinned key or holds
/// another one, `seat` already ends with the run id, the run's seat is
/// the launcher or is already pinned, `run_key` is not a public key, or
/// the tracker did not take the note. A launcher that is not a standing
/// seat is refused too: no slot it signs would count.
pub fn write(issue: &str, seat: &str, model: &str, run: &str, run_key: &str) -> Result<String> {
    let launcher = ballot_sig::own_voter()
        .context("slot: no seat to sign as; set LJOS_SEAT to the launching seat")?;
    let run_key = run_key.trim().to_ascii_lowercase();
    let public = hex::decode(&run_key)
        .ok()
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .and_then(|b| ed25519_dalek::VerifyingKey::from_bytes(&b).ok());
    if public.is_none() {
        bail!("slot: --key {run_key} is not a public seat key; pass the hex `ljos cloud up` printed on the run");
    }
    let seat = crate::cloud::run_seat(seat, run)?;
    if seat == launcher {
        bail!("slot: {seat} is the launching seat, and a seat does not slot itself");
    }
    let pins = ballot_sig::pins();
    let Some(pinned) = pins.get(&launcher) else {
        bail!(
            "slot: {launcher} has no pinned key on this tracker; a slot is signed under the key its ballots pinned"
        );
    };
    if pins.contains_key(&seat) {
        bail!(
            "slot: {seat} is already pinned, so its key is not one the run minted; slot a seat before its first ballot"
        );
    }
    let path = ballot_sig::seat_key_path(&launcher)
        .context("slot: no place for a seat key: HOME and XDG_DATA_HOME are unset")?;
    let key = ballot_sig::key_from_file(&path)?;
    let mine = hex::encode(key.verifying_key().to_bytes());
    if ballot_sig::key_fingerprint(&mine) != ballot_sig::key_fingerprint(pinned) {
        bail!(
            "slot: {launcher} is pinned to key {}…, and this key is {}…",
            &pinned[..12.min(pinned.len())],
            &mine[..12]
        );
    }
    let began = starts().map_err(anyhow::Error::msg)?;
    if !stands(&launcher, Some(pinned), began.as_deref()).map_err(anyhow::Error::msg)? {
        bail!(
            "slot: {launcher} is not a standing seat, so no slot it signs counts; a standing seat launches"
        );
    }
    let note = sign(
        &key,
        &tracker_id(),
        issue,
        &seat,
        model,
        run,
        &run_key,
        &launcher,
    );
    if ballot_sig::write_sig_note(issue, &note).is_err() {
        crate::run_captured("vissue", &["note", issue, &note])?;
    }
    Ok(format!(
        "slot: {seat} on {issue} for run {}, model {}, run key {}…, signed by {launcher} with key {}…\n",
        ballot_sig::field(run),
        ballot_sig::field(model),
        &run_key[..12],
        &mine[..12]
    ))
}

/// Why the run seats among `seats` do not count on `issue`. A seat that
/// `stands` needs no slot. Any other needs a slot in `notes` that names it
/// and its pinned key and verifies, signed under the pin of a seat that
/// stands.
/// `before_pin(note, voter)` is whether `note` was already in this
/// issue's own entry when `voter`'s pin was introduced. `false` refuses
/// the slot: the key it vouches for is not one the run minted after it.
/// A slot verifies only on `tracker` ([`tracker_id`]).
#[must_use]
pub fn problems(
    tracker: &str,
    issue: &str,
    seats: &[(&str, &str)],
    notes: &[String],
    pins: &BTreeMap<String, String>,
    stands: &dyn Fn(&str) -> bool,
    before_pin: &dyn Fn(&str, &str) -> bool,
) -> Vec<String> {
    let mut out = Vec::new();
    for (voter, _) in seats {
        if stands(voter) {
            continue;
        }
        let mut why = None;
        let mut counted = false;
        for (note, slot) in notes.iter().filter_map(|n| Some((n, parse(n)?))) {
            if slot.seat != *voter {
                continue;
            }
            let launcher = &slot.launcher;
            let refused = match pins.get(launcher) {
                _ if !verifies(tracker, issue, &slot) => {
                    Some(format!("{voter}'s slot does not verify"))
                }
                None => Some(format!(
                    "{voter}'s slot is from {launcher}, which has no pinned key on this tracker"
                )),
                Some(key)
                    if ballot_sig::key_fingerprint(key)
                        != ballot_sig::key_fingerprint(&slot.key) =>
                {
                    Some(format!(
                        "{voter}'s slot is signed by a key other than {launcher}'s pinned one"
                    ))
                }
                Some(_) if !stands(launcher) => Some(format!(
                    "{voter}'s slot is from {launcher}, which is not a standing seat"
                )),
                Some(_)
                    if pins.get(*voter).map(|k| ballot_sig::key_fingerprint(k))
                        != Some(ballot_sig::key_fingerprint(&slot.run_key)) =>
                {
                    Some(format!(
                        "{voter} is pinned to a key other than the one its slot names"
                    ))
                }
                Some(_) if !before_pin(note, voter) => {
                    Some(format!("{voter}'s key was pinned before its slot"))
                }
                Some(_) => None,
            };
            match refused {
                None => {
                    counted = true;
                    break;
                }
                Some(line) => {
                    why.get_or_insert(line);
                }
            }
        }
        if !counted {
            out.push(why.unwrap_or_else(|| {
                format!("{voter} is not a standing seat, and no slot on {issue} names it")
            }));
        }
    }
    out
}

/// Whether `note` was already in `issue`'s own entry when `seat`'s pin
/// `key` was introduced. `Ok(None)` when the tracker is not a git
/// repository. `Ok(Some(false))` when the pin is not committed, or when
/// any commit that introduced it lacks `note` in this issue's entry. The
/// same text on another issue is not this entry. No author or committer
/// date is read.
///
/// # Errors
///
/// [`require_tracker_history`] could not make the history readable. A
/// missing parent is not "the note was absent".
pub fn recorded_before_pin(
    issue: &str,
    note: &str,
    seat: &str,
    key: &str,
) -> std::result::Result<Option<bool>, String> {
    if ballot_sig::tracker_git(&["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return Ok(None);
    }
    if let Some(root) = ballot_sig::env_root() {
        require_tracker_history(&root)?;
    }
    let note = note.trim();
    if note.is_empty() || seat.trim().is_empty() || key.trim().is_empty() {
        return Ok(Some(false));
    }
    let found = pin_introductions(seat, key)?;
    // No introduction means the pin is not in history. `all` of an empty
    // set is true, and an uncommitted pin would then count.
    if found.is_empty() {
        return Ok(Some(false));
    }
    Ok(Some(
        found
            .iter()
            .all(|commit| note_on_issue_at(commit, issue, note)),
    ))
}

/// Commits that added the exact pin line `seat key`, whose parent trees
/// do not already hold it. Every branch is read: a merge that kept the
/// same line from both sides would otherwise hide one introduction.
fn pin_introductions(seat: &str, key: &str) -> std::result::Result<Vec<String>, String> {
    if let Some(root) = ballot_sig::env_root() {
        require_tracker_history(&root)?;
    }
    let pick = format!("-S{seat} {key}");
    let log = ballot_sig::tracker_git(&[
        "--no-pager",
        "log",
        "--full-history",
        "--format=%H %P",
        pick.as_str(),
        "--",
        ballot_sig::PIN_FILE,
    ])
    .unwrap_or_default();
    let mut out = Vec::new();
    for line in log.lines() {
        let mut parts = line.split_whitespace();
        let Some(commit) = parts.next() else {
            continue;
        };
        let mut held_by_parent = false;
        for parent in parts {
            if pin_line_at(parent, seat, key)? {
                held_by_parent = true;
                break;
            }
        }
        if pin_line_at(commit, seat, key)? && !held_by_parent {
            out.push(commit.to_string());
        }
    }
    Ok(out)
}

/// The pin lines some commit in this tracker's history held that `pins`
/// no longer does, as `seat key…`. A pin once committed stays: a line
/// changed in place re-pins a seat, and a line removed drops its ballots.
/// Empty on a tracker that is not a git repository.
#[must_use]
pub fn dropped_pins(pins: &BTreeMap<String, String>) -> Vec<String> {
    let log = ballot_sig::tracker_git(&[
        "--no-pager",
        "log",
        "--full-history",
        "--format=%H",
        "--",
        ballot_sig::PIN_FILE,
    ])
    .unwrap_or_default();
    let mut out = std::collections::BTreeSet::new();
    for commit in log.lines() {
        let Some(text) = file_at(commit.trim(), ballot_sig::PIN_FILE) else {
            continue;
        };
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let (Some(seat), Some(key)) = (words.next(), words.next()) else {
                continue;
            };
            if pins.get(seat).map(String::as_str) != Some(key) {
                let short: String = key.chars().take(12).collect();
                out.insert(format!("{seat} {short}…"));
            }
        }
    }
    out.into_iter().collect()
}

fn pin_line_at(commit: &str, seat: &str, key: &str) -> std::result::Result<bool, String> {
    match ballot_sig::env_root() {
        Some(root) => pin_line_at_in(&root, commit, seat, key),
        None => Ok(false),
    }
}

fn pin_line_at_in(
    root: &Path,
    commit: &str,
    seat: &str,
    key: &str,
) -> std::result::Result<bool, String> {
    require_tracker_history(root)?;
    let Some(text) = file_at_in(root, commit, ballot_sig::PIN_FILE) else {
        return Ok(false);
    };
    Ok(text.lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some(seat) && words.next() == Some(key) && words.next().is_none()
    }))
}

fn file_at(commit: &str, path: &str) -> Option<String> {
    file_at_in(&ballot_sig::env_root()?, commit, path)
}

pub(crate) fn file_at_in(root: &Path, commit: &str, path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    ballot_sig::git_in(root, &["--no-pager", "show", &format!("{commit}:{path}")])
}

fn note_on_issue_at(commit: &str, issue: &str, note: &str) -> bool {
    ballot_sig::env_root().is_some_and(|root| note_on_issue_at_in(&root, commit, issue, note))
}

pub(crate) fn note_on_issue_at_in(root: &Path, commit: &str, issue: &str, note: &str) -> bool {
    let pattern = format!(":ID:[[:space:]]+{}[[:space:]]*$", ere_escape(issue));
    let Some(hits) = ballot_sig::git_in(
        root,
        &["--no-pager", "grep", "-l", "-E", "-e", &pattern, commit],
    ) else {
        return false;
    };
    let prefix = format!("{commit}:");
    hits.lines().any(|hit| {
        let hit = hit.trim();
        let path = hit.strip_prefix(&prefix).unwrap_or(hit);
        file_at_in(root, commit, path)
            .is_some_and(|text| issue_entry(&text, issue).is_some_and(|block| block.contains(note)))
    })
}

/// The org entry whose `:ID:` is `issue`, from its heading through the
/// line before the next heading. A line of the same text in another entry
/// is not this one.
fn issue_entry(text: &str, issue: &str) -> Option<String> {
    let mut block = String::new();
    let mut matched = false;
    let mut in_entry = false;
    for line in text.lines() {
        if is_heading(line) {
            if matched {
                return Some(block);
            }
            block.clear();
            matched = false;
            in_entry = true;
        }
        if !in_entry {
            continue;
        }
        if !block.is_empty() {
            block.push('\n');
        }
        block.push_str(line);
        if id_line(line, issue) {
            matched = true;
        }
    }
    matched.then_some(block)
}

fn is_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('*') else {
        return false;
    };
    rest.is_empty() || rest.starts_with(['*', ' ', '\t'])
}

fn id_line(line: &str, issue: &str) -> bool {
    let Some(rest) = line.trim().strip_prefix(":ID:") else {
        return false;
    };
    rest.trim() == issue
}

/// `git grep -E` metacharacters, so an issue id is matched as itself.
fn ere_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if matches!(
            c,
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> SigningKey {
        SigningKey::from_bytes(&[b; 32])
    }

    #[test]
    fn a_slot_parses_whole_and_verifies_on_its_issue_only() {
        let note = sign(
            &key(1),
            "t-1",
            "d-1",
            "review-cafebabe0001abcd",
            "grok 4",
            "bc-cafebabe0001abcd",
            &public(&key(4)),
            "ljosbot",
        );
        let slot = parse(&note).expect("a signed slot parses");
        assert_eq!(slot.seat, "review-cafebabe0001abcd");
        assert_eq!(slot.model, "grok_4");
        assert_eq!(slot.run, "bc-cafebabe0001abcd");
        assert_eq!(slot.run_key, public(&key(4)));
        assert_eq!(slot.launcher, "ljosbot");
        assert!(verifies("t-1", "d-1", &slot));
        assert!(
            !verifies("t-1", "d-2", &slot),
            "a slot lifted to another issue verified"
        );
        assert!(
            !verifies("t-2", "d-1", &slot),
            "a slot lifted to another tracker verified"
        );
        let mut moved = slot.clone();
        moved.model = "opus".into();
        assert!(
            !verifies("t-1", "d-1", &moved),
            "a rewritten model still verified"
        );
        let mut rekeyed = slot.clone();
        rekeyed.run_key = public(&key(3));
        assert!(
            !verifies("t-1", "d-1", &rekeyed),
            "a rewritten run key still verified"
        );
        let mut renamed = slot;
        renamed.seat = "review-other".into();
        assert!(
            !verifies("t-1", "d-1", &renamed),
            "a rewritten seat still verified"
        );
    }

    fn public(k: &SigningKey) -> String {
        hex::encode(k.verifying_key().to_bytes())
    }

    /// A slot for `seat` on `issue` of tracker `t-1` naming the run key
    /// [`check`] pins for `review-r1`, signed by `key(k)` as `launcher`.
    fn slotted(k: u8, issue: &str, seat: &str, launcher: &str) -> String {
        sign(
            &key(k),
            "t-1",
            issue,
            seat,
            "grok",
            "r1",
            &public(&key(4)),
            launcher,
        )
    }

    /// `ljosbot` and `inky` are standing seats. `mallory` and the run seat
    /// are not. `before_pin` is false only for the note `late` names: that
    /// line was not in this issue's entry when the run pin was introduced.
    fn check(notes: &[String], late: Option<&str>) -> Vec<String> {
        let pins: BTreeMap<String, String> = [
            ("ljosbot", public(&key(1))),
            ("inky", public(&key(2))),
            ("mallory", public(&key(3))),
            ("review-r1", public(&key(4))),
        ]
        .into_iter()
        .map(|(seat, k)| (seat.to_string(), k))
        .collect();
        let before_pin = |note: &str, _voter: &str| late != Some(note);
        problems(
            "t-1",
            "d-1",
            &[("inky", "ship"), ("review-r1", "ship")],
            notes,
            &pins,
            &standing,
            &before_pin,
        )
    }

    #[test]
    fn a_run_seat_counts_under_a_slot_from_a_standing_seat() {
        let slot = slotted(1, "d-1", "review-r1", "ljosbot");
        assert!(check(std::slice::from_ref(&slot), None).is_empty());
        let none = check(&[], None);
        assert_eq!(
            none,
            vec!["review-r1 is not a standing seat, and no slot on d-1 names it"]
        );
        let other = slotted(1, "d-1", "review-r2", "ljosbot");
        assert_eq!(check(&[other], None), none);
    }

    #[test]
    fn a_slot_that_does_not_stand_does_not_count() {
        let refused = |note: String, late: bool| {
            let p = check(std::slice::from_ref(&note), late.then_some(note.as_str()));
            assert_eq!(p.len(), 1, "{p:?}");
            p[0].clone()
        };
        let lifted = slotted(1, "d-2", "review-r1", "ljosbot");
        assert!(refused(lifted, false).contains("does not verify"));
        let forged = slotted(3, "d-1", "review-r1", "ljosbot");
        assert!(refused(forged, false).contains("other than ljosbot's pinned one"));
        let minted = slotted(3, "d-1", "review-r1", "mallory");
        assert!(refused(minted, false).contains("not a standing seat"));
        let own = slotted(4, "d-1", "review-r1", "review-r1");
        assert!(refused(own, false).contains("not a standing seat"));
        let stranger = slotted(5, "d-1", "review-r1", "nobody");
        assert!(refused(stranger, false).contains("no pinned key"));
        let rekeyed = sign(
            &key(1),
            "t-1",
            "d-1",
            "review-r1",
            "grok",
            "r1",
            &public(&key(3)),
            "ljosbot",
        );
        assert!(refused(rekeyed, false).contains("other than the one its slot names"));
        let late = slotted(1, "d-1", "review-r1", "ljosbot");
        assert!(refused(late, true).contains("pinned before its slot"));
    }

    #[test]
    fn a_run_pinned_in_upper_case_hex_counts_under_its_slot() {
        let pins: BTreeMap<String, String> = [
            ("ljosbot".to_string(), public(&key(1))),
            (
                "review-r1".to_string(),
                public(&key(4)).to_ascii_uppercase(),
            ),
        ]
        .into();
        let slot = slotted(1, "d-1", "review-r1", "ljosbot");
        let p = problems(
            "t-1",
            "d-1",
            &[("review-r1", "ship")],
            &[slot],
            &pins,
            &standing,
            &|_, _| true,
        );
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn one_good_slot_beside_a_forged_one_counts() {
        let forged = slotted(3, "d-1", "review-r1", "ljosbot");
        let good = slotted(2, "d-1", "review-r1", "inky");
        assert!(check(&[forged, good], None).is_empty());
    }

    #[test]
    fn a_seat_off_the_list_needs_a_slot_when_the_tracker_has_no_clock() {
        let pins: BTreeMap<String, String> = [("review-r1".to_string(), public(&key(4)))].into();
        let p = problems(
            "t-1",
            "d-1",
            &[("review-r1", "ship")],
            &[],
            &pins,
            &standing,
            &|_, _| true,
        );
        assert_eq!(
            p,
            vec!["review-r1 is not a standing seat, and no slot on d-1 names it"]
        );
    }

    #[test]
    fn the_standing_list_names_those_seats_and_skips_comments() {
        assert!(standing("ljosbot"));
        assert!(standing("inky"));
        assert!(standing("disk-saver"));
        assert!(!standing("mallory"));
        assert!(!standing("# Seats that stood"));
        assert!(!standing(""));
    }

    #[test]
    fn a_slot_line_in_another_issues_block_is_not_on_this_one() {
        let note = slotted(1, "d-1", "review-r1", "ljosbot");
        let other =
            format!("* TODO other\n:PROPERTIES:\n:ID: d-2\n:END:\n- Note: \"{note}\" [2020]\n");
        let this = "* TODO this\n:PROPERTIES:\n:ID: d-1\n:END:\n";
        let text = format!("{other}{this}");
        assert!(
            issue_entry(&text, "d-1").is_none_or(|block| !block.contains(&note)),
            "a line on d-2 counted as d-1"
        );
        assert!(
            issue_entry(&text, "d-2").is_some_and(|block| block.contains(&note)),
            "the line was not in d-2's own entry"
        );
        let with = format!("{text}- Note: \"{note}\" [2021]\n");
        assert!(issue_entry(&with, "d-1").is_some_and(|block| block.contains(&note)));
    }

    #[test]
    fn an_issue_id_is_a_literal_in_the_grep() {
        assert_eq!(ere_escape("a.b*"), r"a\.b\*");
        assert_eq!(ere_escape("probe-slot"), "probe-slot");
    }

    #[test]
    fn a_note_that_is_not_a_slot_does_not_parse() {
        assert_eq!(parse("slot: review model=m run=r ljosbot ab"), None);
        assert_eq!(
            parse("slot: review model=m run=r ljosbot ab cd"),
            None,
            "a slot that names no run key parsed"
        );
        assert_eq!(parse("slot: review m r ljosbot ab cd"), None);
        assert_eq!(parse("ballot-sig: a ship ab cd"), None);
    }

    fn git_ok(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} in {}: {}\n{}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn init_repo(dir: &std::path::Path) {
        std::fs::create_dir_all(dir).unwrap();
        git_ok(dir, &["init", "-q", "-b", "main"]);
        git_ok(dir, &["config", "user.email", "seat@example.invalid"]);
        git_ok(dir, &["config", "user.name", "seat"]);
        git_ok(dir, &["config", "commit.gpgsign", "false"]);
        let hooks = dir.join("no-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        git_ok(dir, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    }

    fn commit_all(dir: &std::path::Path, message: &str) {
        git_ok(dir, &["add", "."]);
        git_ok(dir, &["commit", "-q", "-m", message]);
    }

    /// A ballot, then `eessi`'s pin, then the first slot note. A depth-1
    /// clone can see the slot and the pin at `HEAD`, and not the parent
    /// the pin was introduced in.
    fn pinned_before_slots(dir: &std::path::Path) {
        init_repo(dir);
        let board = "\
* TODO [#C] old
:PROPERTIES:
:ID:         probe-old
:END:
- [X] elder: rail
";
        std::fs::write(dir.join("issues.org"), board).unwrap();
        commit_all(dir, "ballot");
        std::fs::write(dir.join(".ljos-seat-keys"), "eessi aa\n").unwrap();
        commit_all(dir, "pin");
        let with_slot =
            format!("{board}- Note: \"slot: eessi model=m run=r key=k ljosbot aa bb\"\n");
        std::fs::write(dir.join("issues.org"), with_slot).unwrap();
        commit_all(dir, "slot");
    }

    fn shallow_of(root: &std::path::Path, origin: &std::path::Path) -> std::path::PathBuf {
        let dest = root.join("shallow");
        let url = format!("file://{}", origin.display());
        git_ok(
            root,
            &["clone", "-q", "--depth", "1", &url, dest.to_str().unwrap()],
        );
        let hooks = dest.join("no-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        git_ok(&dest, &["config", "commit.gpgsign", "false"]);
        git_ok(
            &dest,
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        );
        dest
    }

    fn quiet_git_config(root: &std::path::Path) {
        let cfg = root.join("gitconfig");
        std::fs::write(&cfg, b"").unwrap();
        std::env::set_var("GIT_CONFIG_GLOBAL", &cfg);
        std::env::set_var("GIT_CONFIG_SYSTEM", &cfg);
    }

    #[test]
    fn a_shallow_clone_deepens_and_a_seat_pinned_before_slots_stands() {
        let _lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        quiet_git_config(root.path());
        let origin = root.path().join("origin");
        pinned_before_slots(&origin);
        let shallow = shallow_of(root.path(), &origin);
        assert_eq!(
            git_ok(&shallow, &["rev-parse", "--is-shallow-repository"]).trim(),
            "true"
        );
        let at = starts_in(&shallow).unwrap().expect("a git tracker");
        assert!(
            stood_in(&shallow, &at, "eessi", "aa").unwrap(),
            "a seat pinned before slots did not stand after deepen: {at:?}"
        );
        assert_eq!(
            git_ok(&shallow, &["rev-parse", "--is-shallow-repository"]).trim(),
            "false",
            "the clone stayed shallow"
        );
        assert!(
            crate::ballot_sig::recorded_before_signing_in(&shallow, "probe-old", "elder", "rail")
                .unwrap(),
            "a ballot recorded before the pin was not dated"
        );
    }

    /// A deepen whose fetch failed is not remembered: once the remote
    /// answers, the next check deepens.
    #[test]
    fn a_failed_deepen_is_tried_again() {
        let _lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin");
        pinned_before_slots(&origin);
        let shallow = shallow_of(root.path(), &origin);
        let away = root.path().join("away");
        std::fs::rename(&origin, &away).unwrap();
        let err = require_tracker_history(&shallow).unwrap_err();
        assert!(err.contains(HISTORY_INCOMPLETE), "{err}");
        std::fs::rename(&away, &origin).unwrap();
        require_tracker_history(&shallow).unwrap();
        assert_eq!(
            git_ok(&shallow, &["rev-parse", "--is-shallow-repository"]).trim(),
            "false",
            "the clone stayed shallow"
        );
    }

    /// `GIT_DIR` names a repository in place of `-C`. Exported toward a
    /// complete repository, it must not make a shallow tracker ready. It
    /// is set only on a child run of this binary, so no other test
    /// thread's git follows it.
    #[test]
    fn a_git_dir_naming_a_complete_repo_does_not_make_a_shallow_tracker_ready() {
        let _lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin");
        pinned_before_slots(&origin);
        let shallow = shallow_of(root.path(), &origin);
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "slot::tests::history_check_in_a_child",
                "--test-threads=1",
            ])
            .env("GIT_DIR", origin.join(".git"))
            .env("LJOS_TEST_HISTORY_ROOT", &shallow)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{said}");
        assert!(said.contains("1 passed"), "{said}");
        assert_eq!(
            git_ok(&shallow, &["rev-parse", "--is-shallow-repository"]).trim(),
            "false",
            "the shallow tracker was ready without a deepen"
        );
    }

    /// The history check on the tracker a parent test names. A run with
    /// no tracker named checks nothing.
    #[test]
    fn history_check_in_a_child() {
        if let Some(root) = std::env::var_os("LJOS_TEST_HISTORY_ROOT") {
            require_tracker_history(Path::new(&root)).unwrap();
        }
    }

    /// Two checks of one shallow tracker at once fetch one at a time, and
    /// the one that waited finds the checkout whole. The remote's
    /// upload-pack marks a fetch that starts while another is in flight.
    #[test]
    fn two_checks_of_one_tracker_do_not_deepen_at_once() {
        let _lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin");
        pinned_before_slots(&origin);
        let shallow = shallow_of(root.path(), &origin);
        let busy = root.path().join("busy");
        let overlap = root.path().join("overlap");
        // git appends the repository path to this line.
        let upload_pack = format!(
            "mkdir '{}' 2>/dev/null || touch '{}'; sleep 1; rmdir '{}'; git-upload-pack",
            busy.display(),
            overlap.display(),
            busy.display()
        );
        git_ok(
            &shallow,
            &["config", "remote.origin.uploadpack", &upload_pack],
        );
        let start = std::sync::Barrier::new(2);
        let results: Vec<_> = std::thread::scope(|s| {
            let checks: Vec<_> = (0..2)
                .map(|_| {
                    s.spawn(|| {
                        start.wait();
                        require_tracker_history(&shallow)
                    })
                })
                .collect();
            checks.into_iter().map(|c| c.join().unwrap()).collect()
        });
        assert!(!overlap.exists(), "two fetches of one tracker ran at once");
        for result in results {
            result.unwrap();
        }
        assert_eq!(
            git_ok(&shallow, &["rev-parse", "--is-shallow-repository"]).trim(),
            "false",
            "the clone stayed shallow"
        );
    }

    /// A deepen whose fetch failed reads the checkout again. Here the
    /// remote's upload-pack completes the checkout, as another process's
    /// fetch would, and then fails this one.
    #[test]
    fn a_checkout_completed_while_the_deepen_failed_is_ready() {
        let _lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin");
        pinned_before_slots(&origin);
        let shallow = shallow_of(root.path(), &origin);
        let git = shallow.join(".git");
        // git appends the repository path, which `false` ignores.
        let upload_pack = format!(
            "echo '{}' > '{}'; rm '{}'; false",
            origin.join(".git").join("objects").display(),
            git.join("objects")
                .join("info")
                .join("alternates")
                .display(),
            git.join("shallow").display()
        );
        git_ok(
            &shallow,
            &["config", "remote.origin.uploadpack", &upload_pack],
        );
        require_tracker_history(&shallow).unwrap();
    }

    /// A depth-1 partial clone unshallows and still misses the blobs its
    /// filter left out. The refusal names those objects and a fetch that
    /// brings them, and that fetch makes the checkout ready.
    #[test]
    fn a_partial_clone_names_its_missing_objects_and_not_unshallow() {
        let _lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin");
        pinned_before_slots(&origin);
        git_ok(&origin, &["config", "uploadpack.allowFilter", "true"]);
        let partial = root.path().join("partial");
        let url = format!("file://{}", origin.display());
        git_ok(
            root.path(),
            &[
                "clone",
                "-q",
                "--depth",
                "1",
                "--filter=blob:none",
                &url,
                partial.to_str().unwrap(),
            ],
        );
        let err = require_tracker_history(&partial).unwrap_err();
        assert!(err.contains(HISTORY_INCOMPLETE), "{err}");
        assert!(err.contains("missing"), "{err}");
        assert!(err.contains("git fetch --refetch --no-filter"), "{err}");
        assert!(!err.contains("git fetch --unshallow"), "{err}");
        assert_eq!(
            git_ok(&partial, &["rev-parse", "--is-shallow-repository"]).trim(),
            "false",
            "the partial clone was not unshallowed"
        );
        git_ok(
            &partial,
            &["fetch", "-q", "--refetch", "--no-filter", "origin"],
        );
        require_tracker_history(&partial).unwrap();
    }

    #[test]
    fn a_shallow_clone_with_no_remote_names_incomplete_history() {
        let _lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        quiet_git_config(root.path());
        let origin = root.path().join("origin");
        pinned_before_slots(&origin);
        let shallow = shallow_of(root.path(), &origin);
        git_ok(&shallow, &["remote", "remove", "origin"]);
        std::env::set_var("VISSUE_ROOT", &shallow);

        let err = starts().unwrap_err();
        assert!(err.contains("tracker history incomplete"), "{err}");
        assert!(err.contains("git fetch --unshallow"), "{err}");
        let err = stood(&[], "eessi", "aa").unwrap_err();
        assert!(
            err.contains("tracker history incomplete"),
            "missing history stood: {err}"
        );
        let err =
            crate::ballot_sig::recorded_before_signing_in(&shallow, "probe-old", "elder", "rail")
                .unwrap_err();
        assert!(err.contains("tracker history incomplete"), "{err}");
        assert_eq!(
            git_ok(&shallow, &["rev-parse", "--is-shallow-repository"]).trim(),
            "true",
            "a clone with no remote was deepened"
        );
    }

    /// A pin, then the pin removed, then the first slot. The slot's parent
    /// does not hold the pin, so the seat does not stand.
    fn pin_removed_before_slots(dir: &std::path::Path) -> (String, String) {
        init_repo(dir);
        let board = "\
* TODO [#C] old
:PROPERTIES:
:ID:         probe-old
:END:
- [X] elder: rail
";
        std::fs::write(dir.join("issues.org"), board).unwrap();
        std::fs::write(dir.join(".ljos-seat-keys"), "eessi aa\n").unwrap();
        commit_all(dir, "root");
        let root = git_ok(dir, &["rev-parse", "HEAD"]).trim().to_string();
        std::fs::write(dir.join(".ljos-seat-keys"), "").unwrap();
        commit_all(dir, "middle");
        let middle = git_ok(dir, &["rev-parse", "HEAD"]).trim().to_string();
        let with_slot =
            format!("{board}- Note: \"slot: eessi model=m run=r key=k ljosbot aa bb\"\n");
        std::fs::write(dir.join("issues.org"), with_slot).unwrap();
        commit_all(dir, "tip");
        (root, middle)
    }

    fn assert_real_order(dir: &std::path::Path) {
        std::env::set_var("VISSUE_ROOT", dir);
        let at = starts().unwrap().expect("a git tracker");
        assert!(
            !stood(&at, "eessi", "aa").unwrap(),
            "a pin removed before slots stood: {at:?}"
        );
    }

    fn assert_rewrite_refused(dir: &std::path::Path, cause: &str) {
        let err = starts().unwrap_err();
        assert!(err.contains("tracker history incomplete"), "{err}");
        assert!(err.contains(cause), "{err}");
        let err = stood(&[], "eessi", "aa").unwrap_err();
        assert!(err.contains(cause), "rewritten history stood: {err}");
        let err = recorded_before_pin(
            "probe-old",
            "slot: eessi model=m run=r key=k ljosbot aa bb",
            "eessi",
            "aa",
        )
        .unwrap_err();
        assert!(err.contains(cause), "{err}");
        let err = crate::ballot_sig::recorded_before_signing_in(dir, "probe-old", "elder", "rail")
            .unwrap_err();
        assert!(err.contains(cause), "{err}");
    }

    #[test]
    fn a_replace_ref_does_not_make_a_removed_pin_stand() {
        let _lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        quiet_git_config(root.path());
        let repo = root.path().join("repo");
        let (root_sha, middle) = pin_removed_before_slots(&repo);
        assert_real_order(&repo);

        git_ok(&repo, &["replace", &middle, &root_sha]);
        let shown =
            crate::ballot_sig::git_in(&repo, &["show", &format!("{middle}:.ljos-seat-keys")])
                .expect("pin file at the middle commit");
        assert!(
            !shown.split_whitespace().any(|word| word == "eessi"),
            "a replace ref changed the pin git show reads: {shown:?}"
        );
        assert_rewrite_refused(&repo, "refs/replace");
    }

    #[test]
    fn a_grafts_file_does_not_make_a_removed_pin_stand() {
        let _lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        quiet_git_config(root.path());
        let repo = root.path().join("repo");
        let (root_sha, middle) = pin_removed_before_slots(&repo);
        assert_real_order(&repo);
        let tip = git_ok(&repo, &["rev-parse", "HEAD"]).trim().to_string();
        let grafts = repo.join(".git").join("info").join("grafts");
        std::fs::create_dir_all(grafts.parent().unwrap()).unwrap();
        // The slot commit's parent becomes the root, which still holds
        // the pin the middle commit removed.
        std::fs::write(&grafts, format!("{tip} {root_sha}\n")).unwrap();

        let parents = crate::ballot_sig::git_in(&repo, &["log", "-1", "--format=%P"]).unwrap();
        assert_eq!(
            parents.trim(),
            middle,
            "a grafts file changed the parent pin order reads: {parents}"
        );
        assert_rewrite_refused(&repo, "grafts");
    }
}
