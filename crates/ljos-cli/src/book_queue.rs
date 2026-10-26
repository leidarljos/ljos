//! Bookkeeping off the hook's answer. Each tool call used to keep the
//! book inside the call, under the one `book.lock` every seat on the host
//! shares, and the first write of a session filed a claim and committed
//! the tracker there. That cost seconds and hundreds of megabytes per call,
//! with every other hook waiting on the lock. A call now leaves a job in
//! the runtime directory and answers. One drainer per user applies the
//! jobs in the order they came, one process at a time, and commits each
//! tracker file once per batch.
//!
//! A job carries what the call knew and the drainer cannot learn later:
//! the environment, the directory, the runner above the call and HEAD at
//! the time of the call.

use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Jobs one apply process takes at most.
const BATCH: usize = 64;
/// How long one batch may run before the drainer stops it.
const BATCH_DEADLINE: Duration = Duration::from_secs(120);
/// A job the book lock kept out this many times, or this old, is dropped
/// with a line in the book log.
const TRIES: u32 = 20;
const STALE: Duration = Duration::from_secs(600);
/// How long a reader that acts on the book waits for the jobs queued
/// before it.
const CATCH_UP: Duration = Duration::from_secs(5);
/// Variables the hook sets per call. Jobs that differ only in these share
/// a batch, and the apply process sets them again for each job.
const PER_CALL: &[&str] = &["LJOS_HARNESS_SESSION"];

#[derive(Serialize, Deserialize)]
struct Job {
    raw: String,
    event: String,
    cue: String,
    session: String,
    #[serde(default)]
    payload_session: Option<String>,
    #[serde(default)]
    ask: Option<String>,
    owner: (u32, u64),
    ancestry: Vec<(u32, String)>,
    head: Option<String>,
    cwd: String,
    env: Vec<(String, String)>,
}

fn spool() -> PathBuf {
    crate::runtime_dir().join("bookq")
}

fn drain_lock_path() -> PathBuf {
    crate::runtime_dir().join("bookq.lock")
}

/// Queue the book for this hook call and return the recall a session
/// start or a prompt hands the model, read from the book as it stands.
#[must_use]
pub fn book_later(raw: &str, call: &crate::HookCall) -> String {
    enqueue(raw, call);
    crate::bookkeep::recall_for(raw, call)
}

/// Queue the ask the guard put to this call.
pub fn enqueue_ask(raw: &str, call: &crate::HookCall, reason: &str) {
    write_job(raw, call, Some(reason));
}

/// Queue the book for this call, and start a drainer when none runs.
pub fn enqueue(raw: &str, call: &crate::HookCall) {
    if crate::bookkeep::books(&call.event) {
        write_job(raw, call, None);
    }
}

fn write_job(raw: &str, call: &crate::HookCall, ask: Option<&str>) {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let value: serde_json::Value = serde_json::from_str(raw.trim()).unwrap_or_default();
    let place = value["cwd"]
        .as_str()
        .or_else(|| value["workspacePaths"][0].as_str())
        .filter(|s| !s.is_empty())
        .map_or_else(|| PathBuf::from(&cwd), PathBuf::from);
    let job = Job {
        raw: raw.to_string(),
        event: call.event.clone(),
        cue: call.cue.clone(),
        session: crate::bookkeep::session_key(call),
        payload_session: call.session.clone(),
        ask: ask.map(str::to_string),
        owner: crate::bookkeep::hook_owner(),
        ancestry: crate::ancestry(),
        head: head_now(&place),
        cwd,
        env: std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .collect(),
    };
    let dir = spool();
    if mkdir_private(&dir).is_err() {
        crate::bookkeep::log_line("book queue: cannot make the spool; nothing booked");
        return;
    }
    let Ok(body) = serde_json::to_vec(&job) else {
        return;
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!("{stamp:024}-{:010}.job", std::process::id());
    let tmp = dir.join(format!(".{name}.tmp"));
    let written = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(&body));
    if written.is_err() || std::fs::rename(&tmp, dir.join(&name)).is_err() {
        let _ = std::fs::remove_file(&tmp);
        crate::bookkeep::log_line("book queue: a job could not be written; nothing booked");
        return;
    }
    kick();
}

fn mkdir_private(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// HEAD of the checkout at `dir`, read from the git files without running
/// git: a sha, or `None` when there is no checkout or HEAD has no commit.
fn head_now(dir: &Path) -> Option<String> {
    let mut at = dir;
    let dot = loop {
        let dot = at.join(".git");
        if dot.exists() {
            break dot;
        }
        at = at.parent()?;
    };
    let git_dir = if dot.is_file() {
        let text = std::fs::read_to_string(&dot).ok()?;
        let named = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
        if named.is_absolute() {
            named
        } else {
            at.join(named)
        }
    } else {
        dot
    };
    let common = std::fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|c| git_dir.join(c.trim()))
        .unwrap_or_else(|| git_dir.clone());
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    let sha = match head.strip_prefix("ref:") {
        None => head.to_string(),
        Some(name) => {
            let name = name.trim();
            match std::fs::read_to_string(common.join(name)) {
                Ok(sha) => sha.trim().to_string(),
                Err(_) => std::fs::read_to_string(common.join("packed-refs"))
                    .ok()?
                    .lines()
                    .find_map(|line| {
                        let (sha, refname) = line.split_once(' ')?;
                        (refname.trim() == name).then(|| sha.to_string())
                    })?,
            }
        }
    };
    (sha.len() >= 40 && sha.chars().all(|c| c.is_ascii_hexdigit())).then_some(sha)
}

fn open_lock(path: &Path) -> Option<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .ok()
}

fn flock(file: &std::fs::File, wait: bool) -> bool {
    let how = if wait {
        libc::LOCK_EX
    } else {
        libc::LOCK_EX | libc::LOCK_NB
    };
    // SAFETY: flock on a descriptor `file` owns; the lock goes with it.
    unsafe { libc::flock(file.as_raw_fd(), how) == 0 }
}

/// Start `ljos hook --drain` unless one runs. A drainer that runs looks
/// at the spool again before it exits, so a job queued meanwhile is not
/// left behind.
fn kick() {
    use std::os::unix::process::CommandExt as _;
    // A unit test's own binary is not ljos.
    if cfg!(test) {
        return;
    }
    let Some(lock) = open_lock(&drain_lock_path()) else {
        return;
    };
    if !flock(&lock, false) {
        return;
    }
    drop(lock);
    let Some(exe) = ljos_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["hook", "--drain", "--no-wait"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: setsid is async-signal-safe and touches only the child.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = cmd.spawn();
}

/// The `ljos` binary: this one, else the one installed beside it, as for
/// `ljos-mcp`, else the one on PATH.
fn ljos_exe() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    if me.file_name().is_some_and(|name| name == "ljos") {
        return Some(me);
    }
    let beside = me.with_file_name("ljos");
    if beside.is_file() {
        return Some(beside);
    }
    which::which("ljos").ok()
}

/// The inode, size and mtime of the binary this process runs, read
/// through its path. An install replaces the file, so the stamp changes
/// while the running process keeps the old one.
fn binary_stamp() -> Option<(u64, u64, i64)> {
    let exe = std::env::current_exe().ok()?;
    let meta = std::fs::metadata(exe).ok()?;
    Some((meta.ino(), meta.len(), meta.mtime()))
}

/// Let the jobs queued before this call apply first, for a reader that
/// acts on the book or the claims: a finish, a start, a commit's book.
/// Waits at most [`CATCH_UP`]; a job still queued then applies later.
/// Jobs queued after the call began are not waited for, so a burst
/// cannot hold the reader. Inside a hook or a drainer this returns at
/// once, since the job it would wait for may be its own.
pub fn catch_up() {
    if std::env::var_os("LJOS_IN_HOOK").is_some() {
        return;
    }
    let ahead = pending();
    if ahead.is_empty() {
        return;
    }
    kick();
    let until = Instant::now() + CATCH_UP;
    while ahead.iter().any(|job| job.exists()) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether jobs wait in the spool. A direct bookkeeper queues behind
/// them, so the book sees calls in the order they came.
#[must_use]
pub fn has_pending() -> bool {
    std::fs::read_dir(spool()).is_ok_and(|mut entries| {
        entries.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "job")))
    })
}

fn pending() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(spool()) else {
        return Vec::new();
    };
    let mut jobs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "job"))
        .collect();
    jobs.sort();
    jobs
}

fn read_job(path: &Path) -> Option<Job> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// `ljos hook --drain`: apply every queued job. With `wait`, wait for a
/// drainer that runs and return once the spool is empty; without, leave
/// the work to that drainer.
#[must_use]
pub fn drain(wait: bool) -> i32 {
    if mkdir_private(&spool()).is_err() {
        return 1;
    }
    loop {
        let Some(lock) = open_lock(&drain_lock_path()) else {
            return 1;
        };
        if !flock(&lock, wait) {
            return 0;
        }
        let drained = drain_held();
        drop(lock);
        if !drained {
            return 1;
        }
        if pending().is_empty() {
            return 0;
        }
    }
}

/// Apply jobs until the spool is empty (`true`), or stop early (`false`)
/// when the ljos binary was replaced or a batch cannot start: the next
/// hook's kick then starts a drainer from the binary that is installed,
/// instead of this one dropping every job after [`TRIES`].
fn drain_held() -> bool {
    let stamp = binary_stamp();
    let mut stuck: Option<(PathBuf, u32)> = None;
    loop {
        if binary_stamp() != stamp {
            crate::bookkeep::log_line(
                "book queue: the ljos binary changed; this drainer stops and the next job starts one",
            );
            return false;
        }
        let jobs = pending();
        let Some(first) = jobs.first().cloned() else {
            return true;
        };
        let Some(head) = read_job(&first) else {
            crate::bookkeep::log_line(&format!(
                "book queue: dropped unreadable job {}",
                first.display()
            ));
            let _ = std::fs::remove_file(&first);
            continue;
        };
        let mut batch = vec![first.clone()];
        for path in jobs.iter().skip(1).take(BATCH - 1) {
            match read_job(path) {
                Some(job) if job.cwd == head.cwd && same_env(&job, &head) => {
                    batch.push(path.clone());
                }
                _ => break,
            }
        }
        if !run_batch(&head, &batch) {
            return false;
        }
        if !first.exists() {
            stuck = None;
            continue;
        }
        let tries = match &stuck {
            Some((path, n)) if *path == first => n + 1,
            _ => 1,
        };
        let old = std::fs::metadata(&first)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > STALE);
        if tries >= TRIES || old {
            crate::bookkeep::log_line(&format!(
                "book queue: dropped {} {} after {tries} tries",
                head.event,
                first.display()
            ));
            let _ = std::fs::remove_file(&first);
            stuck = None;
            continue;
        }
        stuck = Some((first, tries));
        // Nothing is held here but the drainer's own lock.
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn same_env(a: &Job, b: &Job) -> bool {
    let kept = |job: &Job| {
        job.env
            .iter()
            .filter(|(k, _)| !PER_CALL.contains(&k.as_str()))
            .cloned()
            .collect::<Vec<_>>()
    };
    kept(a) == kept(b)
}

/// Set the variables of [`PER_CALL`] and the payload's session as the
/// call that left `job` had them.
fn wear_call(job: &Job) {
    crate::with_env_mutation(|| {
        for key in PER_CALL {
            match job.env.iter().find(|(k, _)| k == key) {
                Some((_, v)) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    });
    crate::ballot_sig::export_hook_session(job.payload_session.as_deref());
    crate::bind_call_conversation(&job.raw, &crate::hook_call_as(&job.raw, Some(&job.event)));
}

/// Apply `batch` in a child that has the first job's environment and
/// directory, as the hook call had them. `false` when no child could
/// start, which the book log names.
fn run_batch(head: &Job, batch: &[PathBuf]) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        crate::bookkeep::log_line("book queue: the running binary has no path; drainer stops");
        return false;
    };
    let dir = if Path::new(&head.cwd).is_dir() {
        PathBuf::from(&head.cwd)
    } else {
        PathBuf::from("/")
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("hook")
        .arg("--apply-book")
        .args(batch)
        .env_clear()
        .envs(head.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            crate::bookkeep::log_line(&format!(
                "book queue: cannot start {} to apply a batch: {e}; drainer stops",
                exe.display()
            ));
            return false;
        }
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return true,
            Ok(None) if started.elapsed() > BATCH_DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                crate::bookkeep::log_line(
                    "book queue: a batch ran past its deadline and was stopped",
                );
                return true;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// `ljos hook --apply-book JOB...`: apply the jobs in order and commit the
/// tracker once at the end. A job the book lock keeps out stays queued,
/// and so do the ones after it.
#[must_use]
pub fn apply(files: &[PathBuf]) -> i32 {
    crate::batch_tracker_commits();
    for path in files {
        let Some(job) = read_job(path) else {
            let _ = std::fs::remove_file(path);
            continue;
        };
        wear_call(&job);
        crate::act_for_ancestry(Some(job.ancestry.clone()));
        crate::bookkeep::act_for(Some((job.owner, job.head.clone())));
        let call = crate::HookCall {
            event: job.event.clone(),
            cue: job.cue.clone(),
            session: Some(job.session.clone()),
            shape: crate::HookShape::default(),
        };
        let done = match job.ask.as_deref() {
            Some(reason) => crate::bookkeep::note_ask_within(&job.raw, &call, reason),
            None => crate::bookkeep::observe_queued(&job.raw, &call),
        };
        if !done {
            break;
        }
        let _ = std::fs::remove_file(path);
    }
    crate::act_for_ancestry(None);
    crate::bookkeep::act_for(None);
    let said = crate::flush_tracker_commits();
    for line in said.lines() {
        if line.contains("could not") || line.contains("refused") {
            crate::bookkeep::log_line(line.trim());
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            ok.status.success(),
            "{}",
            String::from_utf8_lossy(&ok.stderr)
        );
    }

    #[test]
    fn head_is_read_from_the_git_files_as_git_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-q", "-b", "main"]);
        assert_eq!(head_now(repo), None, "no commit yet");
        std::fs::create_dir(repo.join("sub")).unwrap();
        git(
            repo,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "one",
            ],
        );
        let said = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let sha = String::from_utf8_lossy(&said.stdout).trim().to_string();
        assert_eq!(head_now(&repo.join("sub")).as_deref(), Some(sha.as_str()));
        git(repo, &["pack-refs", "--all"]);
        assert_eq!(head_now(repo).as_deref(), Some(sha.as_str()), "packed ref");
        let tree = tempfile::tempdir().unwrap();
        let wt = tree.path().join("wt");
        git(
            repo,
            &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
        );
        assert_eq!(head_now(&wt).as_deref(), Some(sha.as_str()), "worktree");
        assert_eq!(head_now(Path::new("/")), None);
    }
}
