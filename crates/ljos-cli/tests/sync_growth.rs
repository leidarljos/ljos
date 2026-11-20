//! A tracker grows by about what a sync changed. Ten syncs that each add
//! one atom grow the tracker's objects by less than one whole log, a sync
//! with nothing new commits nothing, and a log sealed as the single file
//! an ljos before chunks wrote still imports every atom. Runs the built
//! `ljos` binary against a pack that lists the test's atoms.
#![cfg(target_os = "linux")]

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::Value;

const HOST: &str = "me";

/// A pack that lists `atoms` as they stand at each request, keeps the
/// body of every atom posted to it, and answers anything else with `{}`.
struct Pack {
    url: String,
    atoms: Arc<Mutex<Vec<Value>>>,
    posted: Arc<Mutex<Vec<Value>>>,
}

fn pack() -> Pack {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let atoms = Arc::new(Mutex::new(Vec::new()));
    let posted = Arc::new(Mutex::new(Vec::new()));
    let (listed, kept) = (Arc::clone(&atoms), Arc::clone(&posted));
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head, body) = loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break (String::new(), Vec::new());
                }
                buf.extend_from_slice(&chunk[..n]);
                let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&buf[..at]).to_string();
                let need: usize = head
                    .to_ascii_lowercase()
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|n| n.trim().parse().ok())
                    .unwrap_or(0);
                if buf.len() >= at + 4 + need {
                    break (head, buf[at + 4..at + 4 + need].to_vec());
                }
            };
            let target = head.split_whitespace().take(2).collect::<Vec<_>>();
            let path = target
                .get(1)
                .and_then(|t| t.split('?').next())
                .unwrap_or("");
            let answer = match (target.first().copied(), path) {
                (Some("GET"), "/v1/atoms") => {
                    serde_json::json!({ "atoms": *listed.lock().unwrap() }).to_string()
                }
                (Some("POST"), "/v1/atoms") => {
                    if let Ok(atom) = serde_json::from_slice::<Value>(&body) {
                        kept.lock().unwrap().push(atom);
                    }
                    "{}".to_string()
                }
                _ => "{}".to_string(),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    Pack { url, atoms, posted }
}

fn keygen(path: &Path) -> String {
    let made = Command::new("age-keygen")
        .arg("-o")
        .arg(path)
        .output()
        .expect("age-keygen must be installed");
    assert!(made.status.success(), "age-keygen");
    let public = Command::new("age-keygen")
        .arg("-y")
        .arg(path)
        .output()
        .unwrap();
    assert!(public.status.success(), "age-keygen -y");
    String::from_utf8_lossy(&public.stdout).trim().to_string()
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A tracker checkout with a bare origin, its scope sealed to this
/// machine's key, and a home whose sync config names the checkout.
struct World {
    _dir: tempfile::TempDir,
    home: PathBuf,
    /// This machine's age recipient, the scope's only one.
    public: String,
    origin: PathBuf,
    repo: PathBuf,
    pack: Pack,
    /// A directory put ahead of `PATH` with an `age` in it that runs the
    /// real one, and refuses to decrypt while `age.fail` is beside it.
    /// While `age.kill` is beside it, the next decrypt removes that file
    /// and kills the `ljos` that ran it with SIGKILL.
    shim: PathBuf,
}

/// The `age` in [`World::shim`]. A refused `-d` says no identity
/// matched, as age does for a file sealed to a key this home lost.
fn age_shim(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let real = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join("age"))
        .find(|age| age.is_file())
        .unwrap_or_else(|| PathBuf::from("/usr/bin/age"));
    std::fs::create_dir_all(dir).unwrap();
    let age = dir.join("age");
    std::fs::write(
        &age,
        format!(
            "#!/bin/sh\ncase \" $* \" in *\" -d \"*) if [ -e \"$0.kill\" ]; then rm -f \"$0.kill\"; kill -9 \"$PPID\"; exit 1; fi; if [ -e \"$0.fail\" ]; then echo 'age: error: no identity matched any of the recipients' >&2; exit 1; fi;; esac\nexec '{}' \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&age, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir.to_path_buf()
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let config = home.join(".config");
    std::fs::create_dir_all(config.join("ljos")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    let public = keygen(&config.join("ljos/age.key"));
    let origin = dir.path().join("origin.git");
    let repo = dir.path().join("tracker");
    std::fs::create_dir_all(&origin).unwrap();
    std::fs::create_dir_all(repo.join(".ljos")).unwrap();
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    git(&repo, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("core.hooksPath", "/dev/null"),
        ("user.name", "probe"),
        ("user.email", "probe@example.invalid"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", key, value]);
    }
    std::fs::write(
        repo.join(".ljos/sync.toml"),
        format!("name = \"probe\"\nrecipients = [\"{public}\"]\n"),
    )
    .unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "scope"]);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(
        config.join("ljos/sync.toml"),
        format!("repos = [\"{}\"]\n", repo.display()),
    )
    .unwrap();
    let shim = age_shim(&dir.path().join("shim"));
    World {
        _dir: dir,
        home,
        public,
        origin,
        repo,
        pack: pack(),
        shim,
    }
}

impl World {
    fn ljos(&self, args: &[&str]) -> Output {
        self.ljos_as(HOST, args)
    }

    /// `ljos` as the machine named `host`, with this world's home and key.
    fn ljos_as(&self, host: &str, args: &[&str]) -> Output {
        let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", format!("{}:{path}", self.shim.display()))
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("LJOS_TEST_HOSTNAME", host)
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", &self.pack.url)
            .env("PACKSET_AUTH", "off")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .current_dir(&self.repo)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// `ljos sync`, which must succeed. Returns what it printed.
    fn sync(&self) -> String {
        let out = self.ljos(&["sync"]);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{text}");
        text
    }

    fn add(&self, atom: Value) {
        self.pack.atoms.lock().unwrap().push(atom);
    }

    /// Bytes of every object the origin holds from `since` to its head:
    /// blobs, trees and commits.
    fn grown_since(&self, since: &str) -> u64 {
        let listed = git(
            &self.origin,
            &["rev-list", "--objects", &format!("{since}..main")],
        );
        listed
            .lines()
            .filter_map(|line| line.split(' ').next())
            .map(|oid| {
                git(&self.origin, &["cat-file", "-s", oid])
                    .parse::<u64>()
                    .unwrap()
            })
            .sum()
    }

    fn head(&self) -> String {
        git(&self.origin, &["rev-parse", "main"])
    }

    fn log_blob(&self) -> String {
        let listed = git(
            &self.origin,
            &["ls-tree", "-r", "--name-only", "main", ".ljos/atoms/me"],
        );
        let snap = listed
            .lines()
            .filter(|line| line.contains("/snap-"))
            .max()
            .unwrap_or_else(|| panic!("no snapshot in {listed}"));
        git(&self.origin, &["rev-parse", &format!("main:{snap}")])
    }

    /// Each file of this machine's log at the origin's head, by path, with
    /// its blob id.
    fn pieces(&self) -> BTreeMap<String, String> {
        git(&self.origin, &["ls-tree", "-r", "main", ".ljos/atoms/me"])
            .lines()
            .filter_map(|line| {
                let (meta, path) = line.split_once('\t')?;
                Some((path.to_string(), meta.split(' ').nth(2)?.to_string()))
            })
            .collect()
    }

    fn blob_size(&self, oid: &str) -> u64 {
        git(&self.origin, &["cat-file", "-s", oid]).parse().unwrap()
    }

    /// Make the shim's `age -d` refuse, or run again.
    fn fail_opens(&self, on: bool) {
        let flag = self.shim.join("age.fail");
        if on {
            std::fs::write(flag, "").unwrap();
        } else {
            let _ = std::fs::remove_file(flag);
        }
    }

    /// Kill the next `ljos` that runs `age -d`, from inside that decrypt.
    fn kill_at_next_open(&self) {
        std::fs::write(self.shim.join("age.kill"), "").unwrap();
    }

    /// The plaintext of `rel` in the checkout, opened with this home's key.
    fn opened(&self, rel: &str) -> String {
        let out = Command::new("age")
            .arg("-d")
            .arg("-i")
            .arg(self.home.join(".config/ljos/age.key"))
            .arg(self.repo.join(rel))
            .output()
            .expect("age must be installed");
        assert!(out.status.success(), "age -d {rel}");
        String::from_utf8(out.stdout).unwrap()
    }

    /// Seal `plain` to this machine's key at `rel` in the checkout.
    fn seal_to_me(&self, rel: &str, plain: &str) {
        let mut seal = Command::new("age")
            .args(["-r", &self.public, "-o"])
            .arg(self.repo.join(rel))
            .stdin(Stdio::piped())
            .spawn()
            .expect("age must be installed");
        seal.stdin
            .take()
            .unwrap()
            .write_all(plain.as_bytes())
            .unwrap();
        assert!(seal.wait().unwrap().success(), "age seal {rel}");
    }

    /// The names in this machine's snapshot directory in the checkout.
    fn here(&self) -> BTreeSet<String> {
        std::fs::read_dir(self.repo.join(".ljos/atoms/me"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect()
    }

    /// Drop the plaintexts this home kept of sealed files, so the next read
    /// of a log runs `age -d`.
    fn forget_opened(&self) {
        let _ = std::fs::remove_dir_all(self.home.join(".local/state/ljos/sync/opened"));
    }

    /// The texts another machine with this key takes from this machine's
    /// log, read from every piece at the origin's head. The pack lists
    /// nothing during the import and its atoms again after.
    fn import_as_peer(&self) -> BTreeSet<String> {
        let listed = std::mem::take(&mut *self.pack.atoms.lock().unwrap());
        self.pack.posted.lock().unwrap().clear();
        let out = self.ljos_as("peer", &["sync", "--import"]);
        *self.pack.atoms.lock().unwrap() = listed;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{text}");
        self.pack
            .posted
            .lock()
            .unwrap()
            .iter()
            .filter_map(|atom| atom["text"].as_str().map(str::to_string))
            .collect()
    }
}

/// The pieces of `after` that `before` did not have, other than the
/// plaintext hash. Fails when a piece of `before` was removed or written
/// again.
fn sealed_since(
    w: &World,
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
    said: &str,
) -> Vec<(String, u64)> {
    for (path, oid) in before {
        if path.ends_with("/plain.sha256") {
            continue;
        }
        assert_eq!(
            after.get(path),
            Some(oid),
            "{path} was removed or sealed again: {said}"
        );
    }
    after
        .iter()
        .filter(|(path, oid)| !path.ends_with("/plain.sha256") && before.get(*path) != Some(oid))
        .map(|(path, oid)| (path.clone(), w.blob_size(oid)))
        .collect()
}

fn texts(atoms: &[Value]) -> BTreeSet<String> {
    atoms
        .iter()
        .map(|atom| atom["text"].as_str().unwrap().to_string())
        .collect()
}

/// A lesson whose JSON line is about `bytes` long, distinct by `n`.
fn lesson(n: usize, bytes: usize) -> Value {
    let words: String = (0..bytes / 8)
        .map(|w| format!("w{:06} ", n * 131 + w))
        .collect();
    serde_json::json!({
        "id": format!("{n:032x}"),
        "kind": "lesson",
        "text": format!("lesson {n}: {words}"),
        "entities": [],
        "ts": "2026-11-02T00:00:00.000Z",
    })
}

/// Readers open at most 64 chunks over a base. The ten measured syncs
/// follow this many that each wrote one, so they include the sync whose
/// chunk would be the 65th.
const WARM: usize = 55;
const MEASURED: usize = 10;
/// A sync seals at most the atoms added since the base: `WARM + MEASURED`
/// of them. Ten measured syncs then seal at most ten times that many atom
/// lines, and a base of more lines than that is outgrown only by a sync
/// that seals the whole log again.
const BASE: usize = MEASURED * (WARM + MEASURED) + MEASURED;
/// Long enough that the commit, the trees and the digest one sync writes
/// come to less than one atom line, as the bound on [`BASE`] counts lines.
const LINE: usize = 1200;

#[test]
fn ten_one_atom_syncs_grow_the_tracker_by_less_than_one_log() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let w = world();
    for n in 0..BASE {
        w.add(lesson(n, LINE));
    }
    let first = w.sync();
    assert!(
        first.contains(&format!("exported {BASE} probe atoms")),
        "{first}"
    );
    let base = w.log_blob();
    let whole: u64 = git(&w.origin, &["cat-file", "-s", &base]).parse().unwrap();
    for n in BASE..BASE + WARM {
        w.add(lesson(n, LINE));
        let said = w.sync();
        assert!(said.contains("1 changed"), "{said}");
    }
    let start = w.head();
    let mut grew = Vec::new();
    for n in BASE + WARM..BASE + WARM + MEASURED {
        let before = w.head();
        w.add(lesson(n, LINE));
        let said = w.sync();
        assert!(said.contains("1 changed"), "{said}");
        let now = w.log_blob();
        if now != base {
            assert!(
                said.contains("/snap-"),
                "a one-atom sync rewrote a sealed file in place: {said}"
            );
        }
        grew.push(w.grown_since(&before));
    }
    let grown = w.grown_since(&start);
    eprintln!(
        "measure: {MEASURED} one-atom syncs over a {whole} byte log grew the origin by {grown} bytes ({grew:?})"
    );
    // Each of these syncs appends a segment, the tenth past 64 included,
    // so the window has ten segments and no copy of the log.
    assert!(
        grown < whole,
        "{MEASURED} one-atom syncs grew the tracker by {grown} bytes, a whole log is {whole}: {grew:?}"
    );
}

#[test]
fn a_sync_with_no_new_atoms_leaves_git_log_unchanged() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let w = world();
    for n in 0..50 {
        w.add(lesson(n, LINE));
    }
    w.sync();
    w.add(lesson(50, LINE));
    let said = w.sync();
    assert!(said.contains("1 changed"), "{said}");
    let log = |dir: &Path| git(dir, &["log", "--format=%H %s", "main"]);
    let (repo, origin) = (log(&w.repo), log(&w.origin));

    let said = w.sync();
    assert!(
        said.contains("51 probe atoms unchanged since the last export"),
        "{said}"
    );
    assert_eq!(
        log(&w.repo),
        repo,
        "a sync with nothing new committed: {said}"
    );
    assert_eq!(
        log(&w.origin),
        origin,
        "a sync with nothing new pushed: {said}"
    );
    assert_eq!(git(&w.repo, &["status", "--porcelain"]), "");
}

#[test]
fn a_single_file_log_from_before_chunks_imports_every_atom() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let w = world();
    let atoms: Vec<Value> = (0..300).map(|n| lesson(n, LINE)).collect();
    let mut plain = String::from("{\"ljos_log\":1,\"host\":\"peer\",\"scope\":\"probe\"}\n");
    for atom in &atoms {
        plain.push_str(&format!("{atom}\n"));
    }
    std::fs::create_dir_all(w.repo.join(".ljos/atoms")).unwrap();
    let mut seal = Command::new("age")
        .args(["-r", &w.public, "-o"])
        .arg(w.repo.join(".ljos/atoms/peer.jsonl.age"))
        .stdin(Stdio::piped())
        .spawn()
        .expect("age must be installed");
    seal.stdin
        .take()
        .unwrap()
        .write_all(plain.as_bytes())
        .unwrap();
    assert!(seal.wait().unwrap().success(), "age seal");
    std::fs::write(
        w.repo.join(".ljos/atoms/peer.digest"),
        "0123456789abcdef0123456789abcdef 0123456789abcdef0123456789abcdef\n",
    )
    .unwrap();
    git(&w.repo, &["add", ".ljos/atoms"]);
    git(
        &w.repo,
        &["commit", "-q", "-m", "chore(sync): peer probe atoms"],
    );
    git(&w.repo, &["push", "-q"]);

    let out = w.ljos(&["sync", "--import"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("sync: peer: 300 probe atoms taken, 0 refused"),
        "{text}"
    );
    let posted: BTreeSet<String> = w
        .pack
        .posted
        .lock()
        .unwrap()
        .iter()
        .filter_map(|atom| atom["text"].as_str().map(str::to_string))
        .collect();
    let sent: BTreeSet<String> = atoms
        .iter()
        .map(|atom| atom["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(posted, sent, "{text}");
}

/// Atoms in the log the append tests start from.
const LOG_ATOMS: usize = 300;
/// Syncs in [`seventy_syncs_of_two_atoms_seal_seventy_small_segments`],
/// enough that the log passes 64 segments.
const SYNCS: usize = 70;
/// A segment of two `LINE` atoms seals to less than this, whatever the
/// size of the log it is sealed over.
const TWO_ATOM_SEGMENT: u64 = 4 * LINE as u64;

/// A world whose log is a snapshot of [`LOG_ATOMS`] lessons and one
/// segment of one more. Returns the atoms and the snapshot's size.
fn sealed_log() -> (World, Vec<Value>, u64) {
    let w = world();
    let mut atoms: Vec<Value> = (0..LOG_ATOMS).map(|n| lesson(n, LINE)).collect();
    for atom in &atoms {
        w.add(atom.clone());
    }
    w.sync();
    let whole = w.blob_size(&w.log_blob());
    atoms.push(lesson(LOG_ATOMS, LINE));
    w.add(atoms[LOG_ATOMS].clone());
    let said = w.sync();
    assert!(said.contains("1 changed"), "{said}");
    (w, atoms, whole)
}

#[test]
fn seventy_syncs_of_two_atoms_seal_seventy_small_segments() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, mut atoms, whole) = sealed_log();
    let mut sealed = 0u64;
    for k in 1..=SYNCS {
        let before = w.pieces();
        for _ in 0..2 {
            let atom = lesson(atoms.len(), LINE);
            w.add(atom.clone());
            atoms.push(atom);
        }
        let said = w.sync();
        let new = sealed_since(&w, &before, &w.pieces(), &format!("sync {k}: {said}"));
        assert_eq!(new.len(), 1, "sync {k} wrote {new:?}: {said}");
        let (path, bytes) = &new[0];
        assert!(path.contains("/seg-"), "sync {k} wrote {path}: {said}");
        assert!(
            *bytes < TWO_ATOM_SEGMENT,
            "sync {k} sealed {bytes} bytes for two atoms over a {whole} byte log: {said}"
        );
        sealed += bytes;
    }
    eprintln!("measure: {SYNCS} two-atom syncs over a {whole} byte snapshot sealed {sealed} bytes");
    assert!(
        sealed < SYNCS as u64 * TWO_ATOM_SEGMENT,
        "{SYNCS} two-atom syncs sealed {sealed} bytes"
    );
    assert_eq!(w.import_as_peer(), texts(&atoms));
}

#[test]
fn a_log_this_machine_cannot_open_is_sealed_whole_once() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, mut atoms, whole) = sealed_log();
    let before = w.pieces();
    w.forget_opened();
    w.fail_opens(true);
    for _ in 0..2 {
        let atom = lesson(atoms.len(), LINE);
        w.add(atom.clone());
        atoms.push(atom);
    }
    let said = w.sync();
    let new = sealed_since(&w, &before, &w.pieces(), &said);
    assert_eq!(new.len(), 1, "over a {whole} byte log, {new:?}: {said}");
    assert!(new[0].0.contains("/snap-"), "{new:?}: {said}");
    assert!(said.contains("warning"), "{said}");
    assert_eq!(git(&w.repo, &["status", "--porcelain"]), "", "{said}");

    let before = w.pieces();
    for _ in 0..2 {
        let atom = lesson(atoms.len(), LINE);
        w.add(atom.clone());
        atoms.push(atom);
    }
    let said = w.sync();
    let new = sealed_since(&w, &before, &w.pieces(), &said);
    assert_eq!(new.len(), 1, "{new:?}: {said}");
    assert!(new[0].0.contains("/seg-"), "{new:?}: {said}");
    assert!(new[0].1 < TWO_ATOM_SEGMENT, "{new:?}: {said}");
    w.fail_opens(false);
    assert_eq!(w.import_as_peer(), texts(&atoms));
}

#[test]
fn a_compaction_over_a_log_this_machine_cannot_open_removes_nothing() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, atoms, _) = sealed_log();
    let other = keygen(&w.home.join("other.key"));
    std::fs::write(
        w.repo.join(".ljos/sync.toml"),
        format!("name = \"probe\"\nrecipients = [\"{other}\"]\n"),
    )
    .unwrap();
    git(&w.repo, &["commit", "-q", "-am", "recipients"]);
    git(&w.repo, &["push", "-q"]);
    w.pack.atoms.lock().unwrap().pop();
    w.forget_opened();
    w.fail_opens(true);
    let before = w.pieces();
    let said = w.sync();
    assert_eq!(w.pieces(), before, "{said}");

    let out = w.ljos(&["sync", "--compact"]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("ljos sync --key"), "{said}");
    assert_eq!(w.pieces(), before, "{said}");
    for path in before.keys() {
        assert!(w.repo.join(path).is_file(), "{path}: {said}");
    }
    w.fail_opens(false);
    let taken = w.import_as_peer();
    let last = atoms[LOG_ATOMS]["text"].as_str().unwrap();
    assert!(taken.contains(last), "{said}");
}

#[test]
fn a_sync_seals_new_atoms_without_opening_its_own_log() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, mut atoms, whole) = sealed_log();
    let before = w.pieces();
    w.fail_opens(true);
    for _ in 0..2 {
        let atom = lesson(atoms.len(), LINE);
        w.add(atom.clone());
        atoms.push(atom);
    }
    let said = w.sync();
    let new = sealed_since(&w, &before, &w.pieces(), &said);
    assert_eq!(new.len(), 1, "over a {whole} byte log, {new:?}: {said}");
    assert!(new[0].0.contains("/seg-"), "{new:?}: {said}");
    assert!(new[0].1 < TWO_ATOM_SEGMENT, "{new:?}: {said}");
    w.fail_opens(false);
    assert_eq!(w.import_as_peer(), texts(&atoms));
}

#[test]
fn a_sync_with_no_new_atoms_commits_nothing_while_its_log_does_not_open() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, _, _) = sealed_log();
    let head = w.head();
    let before = w.pieces();
    w.forget_opened();
    w.fail_opens(true);
    let said = w.sync();
    assert!(said.contains("unchanged since the last export"), "{said}");
    assert_eq!(w.head(), head, "{said}");
    assert_eq!(w.pieces(), before, "{said}");
    assert_eq!(git(&w.repo, &["status", "--porcelain"]), "", "{said}");
}

/// Every file below `.ljos/atoms` in the checkout, with its bytes and the
/// time it was last written.
fn atom_files(repo: &Path) -> BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime)> {
    fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime)>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                let at = entry.metadata().unwrap().modified().unwrap();
                out.insert(path.clone(), (std::fs::read(&path).unwrap(), at));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(&repo.join(".ljos/atoms"), &mut out);
    out
}

/// Bytes of the blobs the origin's newest commit added or changed.
fn blob_bytes_of_last_commit(w: &World) -> u64 {
    git(
        &w.origin,
        &[
            "diff-tree",
            "-r",
            "--no-commit-id",
            "--diff-filter=AM",
            "main",
        ],
    )
    .lines()
    .filter_map(|line| line.split_whitespace().nth(3))
    .map(|oid| w.blob_size(oid))
    .sum()
}

#[test]
fn two_syncs_with_no_change_write_no_byte_and_make_no_commit() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, _, _) = sealed_log();
    let objects = || git(&w.repo, &["count-objects", "-v"]);
    let (head, origin, files, counted) = (
        git(&w.repo, &["rev-parse", "HEAD"]),
        w.head(),
        atom_files(&w.repo),
        objects(),
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    for n in 1..=2 {
        let said = w.sync();
        assert!(
            said.contains("unchanged since the last export"),
            "sync {n}: {said}"
        );
        assert_eq!(
            git(&w.repo, &["rev-parse", "HEAD"]),
            head,
            "sync {n}: {said}"
        );
        assert_eq!(w.head(), origin, "sync {n}: {said}");
        let now = atom_files(&w.repo);
        let written: Vec<&PathBuf> = now
            .iter()
            .filter(|(path, file)| files.get(*path) != Some(*file))
            .map(|(path, _)| path)
            .collect();
        assert!(written.is_empty(), "sync {n} wrote {written:?}: {said}");
        assert_eq!(now.len(), files.len(), "sync {n}: {said}");
        assert_eq!(objects(), counted, "sync {n} wrote git objects: {said}");
    }
}

/// Atoms in the large store of
/// [`one_atom_writes_one_atom_of_bytes_over_a_small_or_large_store`]. The
/// small one has [`LOG_ATOMS`].
const LARGE_ATOMS: usize = 1500;

#[test]
fn one_atom_writes_one_atom_of_bytes_over_a_small_or_large_store() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let mut written = Vec::new();
    for store in [LOG_ATOMS, LARGE_ATOMS] {
        let w = world();
        for n in 0..store {
            w.add(lesson(n, LINE));
        }
        w.sync();
        let whole = w.blob_size(&w.log_blob());
        let mut bytes = Vec::new();
        // Past 64 segments, where a sync used to seal the store whole.
        for k in 0..SYNCS {
            w.add(lesson(store + k, LINE));
            let said = w.sync();
            assert!(said.contains("1 changed"), "{said}");
            let b = blob_bytes_of_last_commit(&w);
            assert!(
                b < 2 * LINE as u64,
                "one atom of {LINE} bytes wrote {b} blob bytes over a {whole} byte store of {store} atoms: {said}"
            );
            bytes.push(b);
        }
        eprintln!("measure: {store} atoms ({whole} sealed bytes): one-atom syncs wrote {bytes:?}");
        written.push(bytes);
    }
    let most = |bytes: &[u64]| *bytes.iter().max().unwrap();
    let least = |bytes: &[u64]| *bytes.iter().min().unwrap();
    assert!(
        most(&written[1]) < least(&written[0]) + LINE as u64 / 4,
        "a one-atom sync wrote more over the large store: {written:?}"
    );
}

#[test]
fn a_temp_file_left_by_a_cut_write_is_not_committed() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, atoms, _) = sealed_log();
    let stray = w.repo.join(".ljos/atoms/me/.seg-9-0000.jsonl.age.tmp-1");
    std::fs::write(&stray, b"half a seal").unwrap();
    let head = w.head();
    let said = w.sync();
    assert_eq!(w.head(), head, "an unchanged sync committed: {said}");
    w.add(lesson(atoms.len(), LINE));
    let said = w.sync();
    let listed = git(&w.origin, &["ls-tree", "-r", "--name-only", "main"]);
    assert!(!listed.contains(".tmp-"), "{listed}\n{said}");
}

impl World {
    /// Where this home keeps the plaintext of sealed files.
    fn opened_dir(&self) -> PathBuf {
        self.home.join(".local/state/ljos/sync/opened")
    }
}

#[test]
fn the_kept_plaintexts_are_this_users_alone_and_not_committed() {
    use std::os::unix::fs::PermissionsExt as _;
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, atoms, _) = sealed_log();
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    std::fs::set_permissions(w.opened_dir(), std::fs::Permissions::from_mode(0o755)).unwrap();
    w.add(lesson(atoms.len(), LINE));
    let said = w.sync();
    assert!(said.contains("1 changed"), "{said}");
    let dir = w.opened_dir();
    assert_eq!(mode(&dir), 0o700, "{}", dir.display());
    let files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(files.len() >= 3, "{files:?}");
    for file in &files {
        assert_eq!(mode(file), 0o600, "{}", file.display());
    }
    let tracked = git(&w.repo, &["ls-files"]);
    assert!(!tracked.contains("opened"), "{tracked}");
    assert_eq!(git(&w.repo, &["status", "--porcelain"]), "", "{said}");
}

impl World {
    /// Rewrite each kept plaintext whose first line starts with `first`.
    /// The line before it, the tag the home signed, stays as it was.
    fn tamper_kept(&self, first: &str, edit: impl Fn(&str) -> String) -> usize {
        let mut changed = 0;
        for entry in std::fs::read_dir(self.opened_dir()).unwrap() {
            let path = entry.unwrap().path();
            let kept = std::fs::read_to_string(&path).unwrap_or_default();
            let Some((tag, text)) = kept.split_once('\n') else {
                continue;
            };
            if text.starts_with(first) {
                std::fs::write(&path, format!("{tag}\n{}", edit(text))).unwrap();
                changed += 1;
            }
        }
        changed
    }
}

#[test]
fn a_kept_plaintext_that_names_an_atom_the_log_lacks_does_not_hide_it() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, mut atoms, _) = sealed_log();
    let ghost = lesson(atoms.len(), LINE);
    let line = format!("{ghost}\n");
    assert_eq!(
        w.tamper_kept("{\"ljos_seg\"", |text| format!("{text}{line}")),
        1
    );
    w.add(ghost.clone());
    atoms.push(ghost);
    let said = w.sync();
    w.forget_opened();
    let again = w.sync();
    let taken = w.import_as_peer();
    assert_eq!(taken.len(), atoms.len(), "{said}{again}");
    assert_eq!(taken, texts(&atoms), "{said}{again}");
}

#[test]
fn a_stale_kept_plaintext_does_not_hide_a_dropped_atom() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, mut atoms, _) = sealed_log();
    let gone = atoms.remove(0);
    let id = format!("\"id\":\"{}\"", gone["id"].as_str().unwrap());
    assert_eq!(
        w.tamper_kept("{\"ljos_log\"", |text| text
            .lines()
            .filter(|line| !line.contains(&id))
            .map(|line| format!("{line}\n"))
            .collect()),
        1
    );
    w.pack
        .atoms
        .lock()
        .unwrap()
        .retain(|atom| atom["id"] != gone["id"]);
    let said = w.sync();
    w.forget_opened();
    let again = w.sync();
    let taken = w.import_as_peer();
    let still_there = taken.contains(gone["text"].as_str().unwrap());
    assert!(!still_there, "{said}{again}");
    assert_eq!(taken, texts(&atoms), "{said}{again}");
}

impl World {
    /// Seal this tracker's scope to `recipients` from the next sync on.
    fn recipients(&self, recipients: &[&str]) {
        let list: Vec<String> = recipients.iter().map(|r| format!("\"{r}\"")).collect();
        std::fs::write(
            self.repo.join(".ljos/sync.toml"),
            format!("name = \"probe\"\nrecipients = [{}]\n", list.join(", ")),
        )
        .unwrap();
        git(&self.repo, &["commit", "-q", "-am", "recipients"]);
        git(&self.repo, &["push", "-q"]);
    }
}

#[test]
fn a_waiting_atom_goes_out_once_this_machine_is_a_recipient_again() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, mut atoms, _) = sealed_log();
    let other = keygen(&w.home.join("other.key"));
    w.recipients(&[&other]);
    for _ in 0..2 {
        let atom = lesson(atoms.len(), LINE);
        w.add(atom.clone());
        atoms.push(atom);
        w.sync();
    }
    let waiting = w.sync();
    assert!(waiting.contains("is not a recipient"), "{waiting}");

    let public = w.public.clone();
    w.recipients(&[&public, &other]);
    let before = w.pieces();
    let said = w.sync();
    let new = sealed_since(&w, &before, &w.pieces(), &said);
    assert_eq!(new.len(), 1, "{new:?}: {waiting}{said}");
    assert!(new[0].0.contains("/snap-"), "{new:?}: {said}");
    assert_eq!(w.import_as_peer(), texts(&atoms), "{waiting}{said}");

    let before = w.pieces();
    let atom = lesson(atoms.len(), LINE);
    w.add(atom.clone());
    atoms.push(atom);
    let said = w.sync();
    let new = sealed_since(&w, &before, &w.pieces(), &said);
    assert_eq!(new.len(), 1, "{new:?}: {said}");
    assert!(new[0].0.contains("/seg-"), "{new:?}: {said}");
    assert!(new[0].1 < TWO_ATOM_SEGMENT, "{new:?}: {said}");
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");
}

impl World {
    /// What the plaintext hash beside this machine's log says.
    fn plain_hash(&self) -> String {
        std::fs::read_to_string(self.repo.join(".ljos/atoms/me/plain.sha256")).unwrap()
    }

    fn forge_plain_hash(&self, line: &str) {
        std::fs::write(self.repo.join(".ljos/atoms/me/plain.sha256"), line).unwrap();
    }

    fn keep_only(&self, atoms: &[Value]) {
        *self.pack.atoms.lock().unwrap() = atoms.to_vec();
    }
}

#[test]
fn a_forged_plain_hash_does_not_hide_an_added_or_dropped_atom() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, atoms, _) = sealed_log();
    let all = w.plain_hash();
    let gone = atoms[0].clone();
    let id = format!("\"id\":\"{}\"", gone["id"].as_str().unwrap());
    let rest = atoms[1..].to_vec();
    w.keep_only(&rest);
    w.sync();
    let without = w.plain_hash();

    w.keep_only(&atoms);
    w.forge_plain_hash(&all);
    let said = w.sync();
    assert!(said.contains("1 changed"), "{said}");
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");

    let tampered = w.tamper_kept("{\"ljos_seg\"", |text| {
        text.lines()
            .filter(|line| !line.contains(&id))
            .map(|line| format!("{line}\n"))
            .collect()
    });
    assert!(tampered > 0);
    w.keep_only(&rest);
    w.forge_plain_hash(&without);
    let said = w.sync();
    assert!(said.contains("1 changed"), "{said}");
    assert_eq!(w.import_as_peer(), texts(&rest), "{said}");

    w.forget_opened();
    let again = w.sync();
    assert!(again.contains("unchanged since the last export"), "{again}");
    assert_eq!(w.import_as_peer(), texts(&rest), "{said}{again}");
}

#[test]
fn a_sync_does_not_commit_a_changed_or_removed_sealed_piece() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, mut atoms, _) = sealed_log();
    let before = w.pieces();
    let seg = before
        .keys()
        .find(|path| path.contains("/seg-"))
        .unwrap()
        .clone();
    let path = w.repo.join(&seg);
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x5a;
    std::fs::write(&path, &bytes).unwrap();
    let said = w.sync();
    sealed_since(&w, &before, &w.pieces(), &said);
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");

    std::fs::remove_file(&path).unwrap();
    let before = w.pieces();
    let said = w.sync();
    let new = sealed_since(&w, &before, &w.pieces(), &said);
    assert!(new.is_empty(), "{new:?}: {said}");

    let atom = lesson(atoms.len(), LINE);
    w.add(atom.clone());
    atoms.push(atom);
    let said = w.sync();
    let new = sealed_since(&w, &before, &w.pieces(), &said);
    assert_eq!(new.len(), 1, "{new:?}: {said}");
    assert!(new[0].0.contains("/seg-"), "{new:?}: {said}");
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");

    let out = w.ljos(&["sync", "--compact"]);
    let said = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{said}");
    let left: Vec<String> = w
        .pieces()
        .into_keys()
        .filter(|path| !path.ends_with("/plain.sha256"))
        .collect();
    assert_eq!(left.len(), 1, "{left:?}: {said}");
    assert!(left[0].contains("/snap-"), "{left:?}: {said}");
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");
}

/// What `out` printed on stdout and stderr.
fn printed(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_compaction_seals_past_a_snapshot_path_head_has() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let w = world();
    let atoms: Vec<Value> = (0..4).map(|n| lesson(n, LINE)).collect();
    for atom in &atoms[..3] {
        w.add(atom.clone());
    }
    w.sync();
    w.add(atoms[3].clone());
    let said = w.sync();
    assert!(said.contains("1 changed"), "{said}");
    let stale = ".ljos/atoms/me/snap-2.jsonl.age";
    let first: String = w
        .opened(".ljos/atoms/me/snap-0.jsonl.age")
        .lines()
        .take(2)
        .map(|line| format!("{line}\n"))
        .collect();
    w.seal_to_me(stale, &first);
    git(&w.repo, &["add", stale]);
    git(&w.repo, &["commit", "-q", "-m", "a one-atom snapshot"]);
    git(&w.repo, &["push", "-q"]);
    std::fs::remove_file(w.repo.join(stale)).unwrap();

    let out = w.ljos(&["sync", "--compact"]);
    let said = printed(&out);
    assert!(out.status.success(), "{said}");
    let left: Vec<String> = w
        .pieces()
        .into_keys()
        .filter(|path| !path.ends_with("/plain.sha256"))
        .collect();
    assert_eq!(left, [".ljos/atoms/me/snap-3.jsonl.age"], "{said}");
    assert_eq!(git(&w.repo, &["status", "--porcelain"]), "", "{said}");
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");
    let again = w.sync();
    assert!(again.contains("unchanged"), "{again}");
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}{again}");
}

#[test]
fn a_compaction_commits_nothing_beside_a_piece_that_differs_from_head() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let w = world();
    let atoms: Vec<Value> = (0..3).map(|n| lesson(n, LINE)).collect();
    for atom in &atoms {
        w.add(atom.clone());
    }
    w.sync();
    let snap = ".ljos/atoms/me/snap-0.jsonl.age";
    w.seal_to_me(snap, &w.opened(snap));
    let before = w.pieces();

    let out = w.ljos(&["sync", "--compact"]);
    let said = printed(&out);
    assert!(!out.status.success(), "{said}");
    assert!(
        said.contains(&format!("{snap} differs from HEAD")),
        "{said}"
    );
    assert_eq!(w.pieces(), before, "{said}");

    git(&w.repo, &["checkout", "--", snap]);
    let out = w.ljos(&["sync", "--compact"]);
    assert!(out.status.success(), "{}", printed(&out));
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");
}

#[test]
fn a_compaction_killed_before_its_removals_loses_no_atom() {
    use std::os::unix::process::ExitStatusExt as _;
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, atoms, _) = sealed_log();
    let before = w.pieces();
    let had = w.here();
    w.kill_at_next_open();
    let out = w.ljos(&["sync", "--compact"]);
    let said = printed(&out);
    assert_eq!(out.status.signal(), Some(9), "{said}");
    assert!(!w.shim.join("age.kill").exists(), "{said}");
    let here = w.here();
    assert!(had.is_subset(&here), "{had:?} {here:?}");
    assert!(here.contains("snap-2.jsonl.age"), "{here:?}");
    assert_eq!(w.pieces(), before, "{said}");

    let said = w.sync();
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");
    let out = w.ljos(&["sync", "--compact"]);
    let said = printed(&out);
    assert!(out.status.success(), "{said}");
    let left: Vec<String> = w
        .pieces()
        .into_keys()
        .filter(|path| !path.ends_with("/plain.sha256"))
        .collect();
    assert_eq!(left.len(), 1, "{left:?}: {said}");
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");
}

#[test]
fn a_committed_snapshot_age_cannot_authenticate_is_sealed_over_once() {
    if tools::halt(tools::Tool::AgeKeygen) {
        return;
    }
    let (w, mut atoms, _) = sealed_log();
    let snap = w
        .pieces()
        .into_keys()
        .find(|path| path.contains("/snap-"))
        .unwrap();
    let path = w.repo.join(&snap);
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x5a;
    std::fs::write(&path, &bytes).unwrap();
    git(&w.repo, &["commit", "-q", "-am", "a damaged snapshot"]);
    git(&w.repo, &["push", "-q"]);
    w.forget_opened();

    let before = w.pieces();
    let atom = lesson(atoms.len(), LINE);
    w.add(atom.clone());
    atoms.push(atom);
    let said = w.sync();
    let new = sealed_since(&w, &before, &w.pieces(), &said);
    assert_eq!(new.len(), 1, "{new:?}: {said}");
    assert!(new[0].0.contains("/snap-"), "{new:?}: {said}");
    assert!(said.contains("did not open"), "{said}");
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");

    let before = w.pieces();
    let atom = lesson(atoms.len(), LINE);
    w.add(atom.clone());
    atoms.push(atom);
    let said = w.sync();
    let new = sealed_since(&w, &before, &w.pieces(), &said);
    assert_eq!(new.len(), 1, "{new:?}: {said}");
    assert!(new[0].0.contains("/seg-"), "{new:?}: {said}");
    assert!(new[0].1 < TWO_ATOM_SEGMENT, "{new:?}: {said}");
    assert_eq!(w.import_as_peer(), texts(&atoms), "{said}");
}
