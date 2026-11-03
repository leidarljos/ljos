//! A tracker grows by about what a sync changed. Ten syncs that each add
//! one atom grow the tracker's objects by less than one whole log, a sync
//! with nothing new commits nothing, and a log sealed as the single file
//! an ljos before chunks wrote still imports every atom. Runs the built
//! `ljos` binary against a pack that lists the test's atoms.
#![cfg(target_os = "linux")]

ljos_test_boot::harness!();
use std::collections::BTreeSet;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::Value;

const HOST: &str = "me";
const LOG: &str = ".ljos/atoms/me.jsonl.age";

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
    World {
        _dir: dir,
        home,
        public,
        origin,
        repo,
        pack: pack(),
    }
}

impl World {
    fn ljos(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env(
                "PATH",
                std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
            )
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("LJOS_TEST_HOSTNAME", HOST)
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
        git(&self.origin, &["rev-parse", &format!("main:{LOG}")])
    }
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
        assert_eq!(
            w.log_blob(),
            base,
            "a one-atom sync sealed the log whole: {said}"
        );
        grew.push(w.grown_since(&before));
    }
    let grown = w.grown_since(&start);
    eprintln!(
        "measure: {MEASURED} one-atom syncs over a {whole} byte log grew the origin by {grown} bytes ({grew:?})"
    );
    assert!(
        grown < whole,
        "{MEASURED} one-atom syncs grew the tracker by {grown} bytes, a whole log is {whole}: {grew:?}"
    );
}

#[test]
fn a_sync_with_no_new_atoms_leaves_git_log_unchanged() {
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
