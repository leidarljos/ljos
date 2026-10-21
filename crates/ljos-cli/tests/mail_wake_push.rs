//! Push mode puts the sealed log on the remote before the wake fires.
//!
//! Two clones of one bare remote stand in for two machines. `ljos send`
//! and `ljos reply` return while a pre-push hook is still asleep, and the
//! wake command finds the log already on the remote. A remote that refuses
//! the push still gets the wake, and the refusal is a line in the wake log.

use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const SENDER: &str = "grok-bot";
const RECIPIENT: &str = "claude-review";
const SENDER_HOST: &str = "sender";
const RECIPIENT_HOST: &str = "recipient";
const LETTER: &str = "the build is red";
const REPLY: &str = "looking";
/// Longer than a pack write, shorter than the test's wait. A send that
/// pushes before it returns cannot beat this while the hook is asleep.
const SEND_BUDGET: Duration = Duration::from_secs(4);
const HOOK_SLEEP_SECS: u64 = 5;

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct World {
    root: tempfile::TempDir,
    origin: PathBuf,
    sender_repo: PathBuf,
    recipient_repo: PathBuf,
    sender_home: PathBuf,
    recipient_home: PathBuf,
    key: PathBuf,
    port: u16,
    path: String,
    gitconfig: PathBuf,
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn packsetd_bin() -> PathBuf {
    on_path("packsetd")
        .or_else(|| {
            let fallback = PathBuf::from("/tmp/packset-bin/packsetd");
            fallback.is_file().then_some(fallback)
        })
        .unwrap_or_else(|| panic!("packsetd is not on PATH; CI builds it from packset develop"))
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("free port");
    listener.local_addr().expect("addr").port()
}

fn git(cfg: &Path, dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", cfg)
        .env("GIT_CONFIG_SYSTEM", cfg)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}\n{}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn git_at(cfg: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .env("GIT_CONFIG_GLOBAL", cfg)
        .env("GIT_CONFIG_SYSTEM", cfg)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}\n{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write_exe(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("dir");
    }
    std::fs::write(path, body).expect("write");
    let mut perms = std::fs::metadata(path).expect("meta").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("perms");
}

fn age_key(path: &Path) -> String {
    let made = Command::new("age-keygen")
        .arg("-o")
        .arg(path)
        .output()
        .expect("age-keygen");
    assert!(
        made.status.success(),
        "age-keygen: {}",
        String::from_utf8_lossy(&made.stderr)
    );
    let public = Command::new("age-keygen")
        .args(["-y"])
        .arg(path)
        .output()
        .expect("age-keygen -y");
    assert!(
        public.status.success(),
        "{}",
        String::from_utf8_lossy(&public.stderr)
    );
    String::from_utf8_lossy(&public.stdout).trim().to_string()
}

fn identity(cfg: &Path, repo: &Path, hooks: &Path) {
    git(cfg, repo, &["config", "user.email", "seat@example.invalid"]);
    git(cfg, repo, &["config", "user.name", "seat"]);
    git(cfg, repo, &["config", "commit.gpgsign", "false"]);
    git(
        cfg,
        repo,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
}

fn home_dirs(home: &Path) {
    for sub in [
        "tmp",
        "run",
        ".config/ljos",
        ".local/share",
        ".local/state",
        ".cache",
    ] {
        std::fs::create_dir_all(home.join(sub)).expect("home dirs");
    }
}

fn world(slow_push: bool, reject: bool) -> (World, Daemon) {
    let root = tempfile::tempdir().expect("root");
    let gitconfig = root.path().join("gitconfig");
    std::fs::write(
        &gitconfig,
        "[user]\n\tname = seat\n\temail = seat@example.invalid\n[commit]\n\tgpgsign = false\n[safe]\n\tdirectory = *\n[init]\n\tdefaultBranch = main\n",
    )
    .expect("gitconfig");
    let origin = root.path().join("origin.git");
    git_at(
        &gitconfig,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    git(
        &gitconfig,
        &origin,
        &["config", "receive.denyNonFastForwards", "true"],
    );

    let key = root.path().join("age.key");
    let public = age_key(&key);
    let seed = root.path().join("seed");
    std::fs::create_dir_all(&seed).expect("seed");
    git(&gitconfig, &seed, &["init", "-q", "-b", "main"]);
    let no_hooks = root.path().join("no-hooks");
    std::fs::create_dir_all(&no_hooks).expect("hooks");
    identity(&gitconfig, &seed, &no_hooks);
    git(
        &gitconfig,
        &seed,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    std::fs::create_dir_all(seed.join(".ljos")).expect("ljos");
    std::fs::write(
        seed.join(".ljos/sync.toml"),
        format!("name = \"cloud\"\nrecipients = [\"{public}\"]\n"),
    )
    .expect("sync.toml");
    std::fs::write(
        seed.join(".gitattributes"),
        "*.org merge=ljos-org\n.ljos/atoms/* merge=ljos-atoms\n",
    )
    .expect("attributes");
    std::fs::write(seed.join("README"), "tracker\n").expect("readme");
    git(&gitconfig, &seed, &["add", "."]);
    git(&gitconfig, &seed, &["commit", "-q", "-m", "seed"]);
    git(&gitconfig, &seed, &["push", "-q", "-u", "origin", "HEAD"]);
    if reject {
        write_exe(
            &origin.join("hooks/pre-receive"),
            "#!/bin/sh\necho \"remote rejected the push\" >&2\nexit 1\n",
        );
    }

    let sender_repo = root.path().join("sender-repo");
    let recipient_repo = root.path().join("recipient-repo");
    for repo in [&sender_repo, &recipient_repo] {
        git_at(
            &gitconfig,
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                repo.to_str().unwrap(),
            ],
        );
        if slow_push {
            let hooks = repo.join(".git/hooks");
            write_exe(
                &hooks.join("pre-push"),
                &format!("#!/bin/sh\nsleep {HOOK_SLEEP_SECS}\nexit 0\n"),
            );
            identity(&gitconfig, repo, &hooks);
        } else {
            identity(&gitconfig, repo, &no_hooks);
        }
    }

    let sender_home = root.path().join("sender-home");
    let recipient_home = root.path().join("recipient-home");
    for (home, repo) in [
        (&sender_home, &sender_repo),
        (&recipient_home, &recipient_repo),
    ] {
        home_dirs(home);
        std::fs::copy(&key, home.join(".config/ljos/age.key")).expect("age key");
        std::fs::write(
            home.join(".config/ljos/sync.toml"),
            format!("repos = [\"{}\"]\n", repo.display()),
        )
        .expect("local sync");
    }

    let packsetd = packsetd_bin();
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_ljos"))
        .parent()
        .expect("bin dir");
    let path = std::env::join_paths([
        bin_dir,
        packsetd.parent().expect("packsetd dir"),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .expect("PATH")
    .into_string()
    .expect("utf-8 PATH");
    let port = free_port();
    let world = World {
        origin,
        sender_repo,
        recipient_repo,
        sender_home,
        recipient_home,
        key,
        port,
        path,
        gitconfig,
        root,
    };
    let pack_home = world.root.path().join("pack");
    std::fs::create_dir_all(&pack_home).expect("pack home");
    let mut daemon = Command::new(&packsetd);
    daemon.args([
        "--port",
        &world.port.to_string(),
        "--home",
        pack_home.to_str().expect("utf-8"),
    ]);
    world.env(&mut daemon, &world.sender_home);
    let daemon = Daemon(
        daemon
            .env("PACKSET_HOME", &pack_home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn packsetd"),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    while std::net::TcpStream::connect(("127.0.0.1", world.port)).is_err() {
        assert!(Instant::now() < deadline, "packsetd did not listen");
        std::thread::sleep(Duration::from_millis(50));
    }
    (world, daemon)
}

impl World {
    fn env(&self, cmd: &mut Command, home: &Path) {
        cmd.env_clear()
            .current_dir(home)
            .env("HOME", home)
            .env("PATH", &self.path)
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", home.join("tmp"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("PACKSET_PORT", self.port.to_string())
            .env("PACKSET_AUTH", "off")
            .env("LJOS_TRACKER_GIT", "push")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_SYSTEM", &self.gitconfig)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "seat")
            .env("GIT_AUTHOR_EMAIL", "seat@example.invalid")
            .env("GIT_COMMITTER_NAME", "seat")
            .env("GIT_COMMITTER_EMAIL", "seat@example.invalid")
            .env("ORIGIN", &self.origin);
    }

    /// `ljos` as `seat` with this machine's hostname. The detached wake
    /// inherits the namespace, so the log is named after `host`.
    fn ljos(&self, home: &Path, host: &str, seat: &str, args: &[&str]) -> Output {
        let mut cmd = Command::new("/usr/bin/unshare");
        cmd.args([
            "--uts",
            "--map-root-user",
            "--",
            "/bin/sh",
            "-c",
            "/usr/bin/hostname \"$1\" && shift && exec \"$@\"",
        ]);
        cmd.arg("ljos").arg(host).arg(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args);
        self.env(&mut cmd, home);
        cmd.env("LJOS_SEAT", seat);
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("unshare");
        child.wait_with_output().expect("wait")
    }

    fn sender(&self, seat: &str, args: &[&str]) -> Output {
        self.ljos(&self.sender_home, SENDER_HOST, seat, args)
    }

    fn recipient(&self, seat: &str, args: &[&str]) -> Output {
        self.ljos(&self.recipient_home, RECIPIENT_HOST, seat, args)
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn wait_for(path: &Path, limit: Duration) -> Option<Duration> {
    let started = Instant::now();
    while started.elapsed() < limit {
        if path.is_file() {
            return Some(started.elapsed());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

fn wake_log(home: &Path) -> String {
    std::fs::read_to_string(home.join(".local/state/ljos/mail/wake.log")).unwrap_or_default()
}

fn open_log(world: &World, host: &str) -> String {
    let spec = format!("HEAD:.ljos/atoms/{host}.jsonl.age");
    let show = Command::new("git")
        .arg("--git-dir")
        .arg(&world.origin)
        .args(["show", &spec])
        .env("GIT_CONFIG_GLOBAL", &world.gitconfig)
        .env("GIT_CONFIG_SYSTEM", &world.gitconfig)
        .output()
        .expect("git show");
    assert!(
        show.status.success(),
        "git show {spec}: {}",
        String::from_utf8_lossy(&show.stderr)
    );
    let sealed = world.root.path().join(format!("{host}.jsonl.age"));
    std::fs::write(&sealed, &show.stdout).expect("sealed");
    let plain = Command::new("age")
        .args(["-d", "-i"])
        .arg(&world.key)
        .arg(&sealed)
        .output()
        .expect("age -d");
    assert!(
        plain.status.success(),
        "age -d: {}",
        String::from_utf8_lossy(&plain.stderr)
    );
    String::from_utf8_lossy(&plain.stdout).into_owned()
}

fn fetch_ff(world: &World, repo: &Path) {
    git(&world.gitconfig, repo, &["fetch", "-q", "origin"]);
    git(
        &world.gitconfig,
        repo,
        &["merge", "-q", "--ff-only", "origin/main"],
    );
}

fn script(home: &Path) -> PathBuf {
    let path = home.join("wake.sh");
    write_exe(
        &path,
        "#!/bin/sh\nset -e\nhost=$(/usr/bin/hostname)\nprintf '%s\\n' \"$host\" > \"$HOME/wake-host\"\ngit --git-dir \"$ORIGIN\" cat-file -e \"HEAD:.ljos/atoms/${host}.jsonl.age\"\nprintf '%s\\n' \"$host\" > \"$HOME/woke\"\n",
    );
    path
}

fn touch_script(home: &Path) -> PathBuf {
    let path = home.join("wake.sh");
    write_exe(
        &path,
        "#!/bin/sh\n/usr/bin/hostname > \"$HOME/wake-host\"\n: > \"$HOME/woke\"\n",
    );
    path
}

#[test]
fn a_send_and_a_reply_push_the_log_before_the_wake() {
    let (world, _daemon) = world(true, false);
    let joined = world.sender(RECIPIENT, &["group", "crew", "--add", RECIPIENT]);
    assert!(joined.status.success(), "{}", text(&joined));

    let sender_script = script(&world.sender_home);
    let set = world.sender(
        RECIPIENT,
        &[
            "wake",
            RECIPIENT,
            "--command",
            &format!("sh {}", sender_script.display()),
        ],
    );
    assert!(set.status.success(), "{}", text(&set));

    let started = Instant::now();
    let sent = world.sender(SENDER, &["send", RECIPIENT, LETTER]);
    let send_took = started.elapsed();
    assert!(sent.status.success(), "{}", text(&sent));
    assert!(
        text(&sent).contains(&format!("waking {RECIPIENT}")),
        "{}",
        text(&sent)
    );
    assert!(
        send_took < SEND_BUDGET,
        "send waited for the push ({send_took:?}); the hook sleeps {HOOK_SLEEP_SECS}s. {}",
        text(&sent)
    );
    let woke =
        wait_for(&world.sender_home.join("woke"), Duration::from_secs(30)).unwrap_or_else(|| {
            panic!(
                "the wake did not run after the push: {}",
                wake_log(&world.sender_home)
            )
        });
    assert_eq!(
        std::fs::read_to_string(world.sender_home.join("wake-host"))
            .unwrap()
            .trim(),
        SENDER_HOST
    );
    let plain = open_log(&world, SENDER_HOST);
    assert!(
        plain.contains(LETTER),
        "the pushed log does not hold the letter: {plain}"
    );
    fetch_ff(&world, &world.recipient_repo);
    eprintln!(
        "measure: send returned in {} ms; the wake ran {} ms later",
        send_took.as_millis(),
        woke.as_millis()
    );

    let recipient_script = script(&world.recipient_home);
    let set = world.recipient(
        SENDER,
        &[
            "wake",
            SENDER,
            "--command",
            &format!("sh {}", recipient_script.display()),
        ],
    );
    assert!(set.status.success(), "{}", text(&set));
    let id = text(&sent)
        .lines()
        .find_map(|line| line.strip_prefix("sent "))
        .and_then(|rest| rest.split_whitespace().next())
        .expect("sent id")
        .to_string();
    let started = Instant::now();
    let reply = world.recipient(RECIPIENT, &["reply", &id, REPLY]);
    let reply_took = started.elapsed();
    assert!(reply.status.success(), "{}", text(&reply));
    assert!(
        text(&reply).contains(&format!("waking {SENDER}")),
        "{}",
        text(&reply)
    );
    assert!(
        reply_took < SEND_BUDGET,
        "reply waited for the push ({reply_took:?}); the hook sleeps {HOOK_SLEEP_SECS}s. {}",
        text(&reply)
    );
    wait_for(&world.recipient_home.join("woke"), Duration::from_secs(30)).unwrap_or_else(|| {
        panic!(
            "the reply's wake did not run after the push: {}",
            wake_log(&world.recipient_home)
        )
    });
    assert_eq!(
        std::fs::read_to_string(world.recipient_home.join("wake-host"))
            .unwrap()
            .trim(),
        RECIPIENT_HOST
    );
    let plain = open_log(&world, RECIPIENT_HOST);
    assert!(
        plain.contains(REPLY),
        "the pushed log does not hold the reply: {plain}"
    );
    fetch_ff(&world, &world.sender_repo);
}

#[test]
fn a_refused_push_still_wakes_and_is_logged() {
    let (world, _daemon) = world(false, true);
    let joined = world.sender(RECIPIENT, &["group", "crew", "--add", RECIPIENT]);
    assert!(joined.status.success(), "{}", text(&joined));
    let script = touch_script(&world.sender_home);
    let set = world.sender(
        RECIPIENT,
        &[
            "wake",
            RECIPIENT,
            "--command",
            &format!("sh {}", script.display()),
        ],
    );
    assert!(set.status.success(), "{}", text(&set));

    let sent = world.sender(SENDER, &["send", RECIPIENT, LETTER]);
    assert!(
        sent.status.success(),
        "a refused push failed the send: {}",
        text(&sent)
    );
    assert!(
        text(&sent).contains(&format!("waking {RECIPIENT}")),
        "{}",
        text(&sent)
    );
    wait_for(&world.sender_home.join("woke"), Duration::from_secs(30)).unwrap_or_else(|| {
        panic!(
            "the wake did not run after the refused push: {}",
            wake_log(&world.sender_home)
        )
    });
    let log = wake_log(&world.sender_home);
    assert!(
        log.contains("push refused"),
        "the wake log does not record the refusal: {log}"
    );
    let remote = Command::new("git")
        .arg("--git-dir")
        .arg(&world.origin)
        .args(["cat-file", "-e", "HEAD:.ljos/atoms/sender.jsonl.age"])
        .env("GIT_CONFIG_GLOBAL", &world.gitconfig)
        .env("GIT_CONFIG_SYSTEM", &world.gitconfig)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("git");
    assert!(!remote.success(), "a refused push still updated the remote");
}
