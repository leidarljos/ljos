//! A brand-new seat following the README quickstart and the getting-started
//! page, then a second order that page does not show.
//!
//! The home, the XDG directories and the pack port are empty. No config
//! file, no store and no writer exist until a documented command creates
//! them. `install.sh` and the release download need the network, so those
//! lines are skipped. The Install section says that script still sets up
//! the tracker, the memory writer and the deed store, which is `ljos
//! onboard` when no coding tool is installed. That command runs in the
//! skipped line's place. Every other command is the one the page prints.
//! The version check doctor makes asks a crates.io stand-in on the seat's
//! `PATH`, so no step reads the public network. The check that the registry
//! did not answer replaces that stand-in with a curl that fails for crates.io.

#![allow(
    clippy::disallowed_macros,
    reason = "a test writes its notes to the test harness, not to a person"
)]

ljos_test_boot::harness!();

#[path = "common/pty.rs"]
mod pty;
#[path = "common/registry.rs"]
mod registry;
#[path = "common/tools.rs"]
mod tools;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const README_START: &str = "<!-- ljos-quickstart:start -->";
const README_END: &str = "<!-- ljos-quickstart:end -->";

/// Session variables a parent runner would leak into a first shell.
const SESSION_VARS: &[&str] = &[
    "CLAUDE_CODE_SESSION_ID",
    "CODEX_THREAD_ID",
    "CODEX_SESSION_ID",
    "CURSOR_CONVERSATION_ID",
    "LJOS_SEAT",
];

const NOISE: &[&str] = &[
    "panicked at",
    "stack backtrace",
    "RUST_BACKTRACE",
    "Caused by:",
    "anyhow::",
    "ljos_cli::",
    "packset_client::",
    "called `Result",
    "called `Option",
    "unwrap()",
    "Os {",
    "Error {",
    "Custom {",
    "thread '",
    ".rs:",
    "Box<dyn",
    "internal error",
    "not yet implemented",
];

const LESSON: &str = "The lexical default is BM25+. It beat BM25 by two points on turns.";

const NO_HITS: &str =
    "Nothing matched \"which fusion\". Try a shorter query, such as: ljos search fusion";

struct DocStep {
    place: String,
    command: String,
    /// Ticket ids printed in the sample under this command, in order.
    sample_ids: Vec<String>,
}

struct Seat {
    root: PathBuf,
    /// The home directory. The documented steps run here, and it is not a git checkout.
    work: PathBuf,
    elsewhere: PathBuf,
    env: BTreeMap<String, String>,
}

impl Seat {
    /// The seat's `~/.local/bin`, first on its `PATH`.
    fn bin(&self) -> PathBuf {
        self.root.join("home/.local/bin")
    }

    fn open() -> Self {
        let base = seat_base().unwrap_or_else(|line| panic!("{line}"));
        let root = tempfile::Builder::new()
            .prefix(".ljos-first-run-")
            .tempdir_in(base)
            .unwrap_or_else(|err| panic!("seat root: {err}"))
            .keep();
        let home = root.join("home");
        let bin = home.join(".local/bin");
        let work = home.clone();
        let elsewhere = root.join("elsewhere");
        for dir in [
            &bin,
            &home.join(".config"),
            &home.join(".local/share"),
            &home.join(".local/state"),
            &home.join(".cache"),
            &root.join("tmp"),
            &root.join("run"),
            &elsewhere,
        ] {
            std::fs::create_dir_all(dir)
                .unwrap_or_else(|err| panic!("create {}: {err}", dir.display()));
        }
        ljos_cli::change_mode(root.join("run"), 0o700).unwrap();
        link_programs(&bin);
        registry::stand_in(&bin, Duration::ZERO);
        let port = free_port();
        let mut env = BTreeMap::new();
        env.insert("HOME".into(), home.display().to_string());
        env.insert("PATH".into(), format!("{}:/usr/bin:/bin", bin.display()));
        env.insert(
            "XDG_CONFIG_HOME".into(),
            home.join(".config").display().to_string(),
        );
        env.insert(
            "XDG_DATA_HOME".into(),
            home.join(".local/share").display().to_string(),
        );
        env.insert(
            "XDG_STATE_HOME".into(),
            home.join(".local/state").display().to_string(),
        );
        env.insert(
            "XDG_CACHE_HOME".into(),
            home.join(".cache").display().to_string(),
        );
        env.insert(
            "XDG_RUNTIME_DIR".into(),
            root.join("run").display().to_string(),
        );
        env.insert("TMPDIR".into(), root.join("tmp").display().to_string());
        env.insert("PACKSET_PORT".into(), port.to_string());
        env.insert("NO_COLOR".into(), "1".into());
        env.insert("TERM".into(), "xterm".into());
        env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
        env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
        // The writer this seat starts finds an encoder installed beside the
        // packsetd it links. The pages below are the no-encoder ones.
        env.insert("INSIDE_EMBED".into(), "off".into());
        env.insert("LANG".into(), "C.UTF-8".into());
        for name in SESSION_VARS {
            assert!(
                !env.contains_key(*name),
                "{name} leaked into a first-run seat"
            );
        }
        Self {
            root,
            work,
            elsewhere,
            env,
        }
    }

    fn run(&mut self, line: &str, cwd: &Path, limit: Duration) -> (i32, String, Duration) {
        if let Some(rest) = line.trim().strip_prefix("export ") {
            return self.export(line, rest);
        }
        let started = Instant::now();
        let term = pty::Pty::open();
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(line);
        cmd.current_dir(cwd);
        cmd.env_clear();
        cmd.envs(&self.env);
        cmd.stdin(Stdio::null());
        term.attach_output(&mut cmd);
        let mut child = term.spawn(&mut cmd);
        let pid = child.id();
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        std::thread::spawn(move || {
            let deadline = Instant::now() + limit;
            while Instant::now() < deadline {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            #[expect(
                clippy::disallowed_methods,
                reason = "starts a program outside git hook code"
            )]
            let _ = Command::new("kill")
                .args(["-s", "KILL", "--", &format!("-{pid}")])
                .status();
        });
        let status = child.wait().unwrap_or_else(|err| panic!("{line}: {err}"));
        done.store(true, Ordering::SeqCst);
        let elapsed = started.elapsed();
        (
            status.code().unwrap_or(127),
            plain(&settled(&term)),
            elapsed,
        )
    }

    fn export(&mut self, line: &str, body: &str) -> (i32, String, Duration) {
        let started = Instant::now();
        let name = body.split('=').next().unwrap_or("").trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return (2, format!("could not read `{line}`"), started.elapsed());
        }
        let script = format!("{line}\nprintf '%s' \"${{{name}}}\"");
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let out = Command::new("bash")
            .arg("-c")
            .arg(&script)
            .current_dir(&self.work)
            .env_clear()
            .envs(&self.env)
            .stdin(Stdio::null())
            .output()
            .unwrap_or_else(|err| panic!("{line}: {err}"));
        let code = out.status.code().unwrap_or(1);
        if code == 0 {
            self.env.insert(
                name.to_string(),
                String::from_utf8_lossy(&out.stdout).into_owned(),
            );
        }
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (code, text, started.elapsed())
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        if let Some(port) = self
            .env
            .get("PACKSET_PORT")
            .and_then(|raw| raw.parse().ok())
        {
            ljos_test_boot::reap_listener(port);
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let _ = Command::new("packset")
            .arg("stop")
            .env_clear()
            .envs(&self.env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn link_programs(bin: &Path) {
    use std::os::unix::fs::symlink;
    // A copy, not a link. `ljos uninstall` follows the running binary and
    // would replace the build this test was compiled against.
    for (name, src) in [
        ("ljos", env!("CARGO_BIN_EXE_ljos")),
        ("ljos-mcp", env!("CARGO_BIN_EXE_ljos-mcp")),
    ] {
        ljos_cli::copy_executable(Path::new(src), &bin.join(name))
            .unwrap_or_else(|err| panic!("copy {name}: {err}"));
    }
    for name in [
        "vissue",
        "claimdag",
        "deedar",
        "packset",
        "packsetd",
        "ljos-policyd",
        "ljos-consensus",
        "age",
        "age-keygen",
    ] {
        if let Some(path) = which(name) {
            #[expect(
                clippy::disallowed_methods,
                reason = "links a path that is not a git hook"
            )]
            let _ = symlink(&path, bin.join(name));
        }
    }
}

/// A `curl` that fails for crates.io and runs the real one for anything
/// else. The unreachable-crates.io check installs it over the stand-in, so
/// that row does not depend on whether this host can reach the registry.
///
/// The real `curl` is the one on this test's `PATH`, found before the stub
/// is installed. With none, the stub fails other calls as a missing `curl`.
fn stub_unreachable_crates_io(bin: &Path) {
    let real = which("curl");
    ljos_cli::install_executable(&bin.join("curl"), curl_stub(real.as_deref()))
        .unwrap_or_else(|err| panic!("curl stub: {err}"));
}

fn curl_stub(real: Option<&Path>) -> String {
    let rest = match real {
        Some(real) => format!(
            "exec '{}' \"$@\"\n",
            real.display().to_string().replace('\'', r"'\''")
        ),
        None => "echo 'curl: not found on PATH' >&2\nexit 127\n".to_string(),
    };
    format!(
        "#!/bin/sh\n\
         for arg in \"$@\"; do\n\
         \x20 case \"$arg\" in\n\
         \x20   https://crates.io/*) echo 'curl: (6) Could not resolve host: crates.io' >&2; exit 6;;\n\
         \x20 esac\n\
         done\n\
         {rest}"
    )
}

#[test]
fn the_curl_stub_runs_the_curl_on_path_for_other_urls() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("it's curl");
    ljos_cli::install_executable(&real, "#!/bin/sh\necho \"real curl: $*\"\n").unwrap();
    let stub = dir.path().join("stub");
    ljos_cli::install_executable(&stub, curl_stub(Some(&real))).unwrap();
    let run = |url: &str| {
        #[expect(
            clippy::disallowed_methods,
            reason = "runs the curl stub a first-run seat gets"
        )]
        let out = std::process::Command::new(&stub)
            .args(["-fsS", url])
            .output()
            .unwrap();
        out
    };
    let other = run("https://example.invalid/x");
    assert!(other.status.success(), "{other:?}");
    assert_eq!(
        String::from_utf8_lossy(&other.stdout),
        "real curl: -fsS https://example.invalid/x\n"
    );
    let registry = run("https://crates.io/api/v1/crates/ljos");
    assert_eq!(registry.status.code(), Some(6), "{registry:?}");
    assert!(registry.stdout.is_empty(), "{registry:?}");

    ljos_cli::install_executable(&stub, curl_stub(None)).unwrap();
    let missing = run("https://example.invalid/x");
    assert_eq!(missing.status.code(), Some(127), "{missing:?}");
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Bytes a seat root may take. The deepest socket a seat opens sits about
/// 30 bytes below it, and `sun_path` holds 103 on macOS.
const ROOT_ROOM: usize = 70;

/// Where the seat root goes: a directory with no git checkout above it,
/// because the confused order runs from a directory that is not one, and
/// outside every temporary root, because ljos turns memory off for a home
/// that sits in one. The checkout's parent is the first choice. A checkout
/// under `/tmp` or `/var/tmp` has a parent in a temporary root, and then a
/// cache under `HOME` holds the seat.
fn seat_base() -> Result<PathBuf, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let beside = manifest
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let cache =
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/ljos-test-seats"));
    for candidate in [beside, cache].into_iter().flatten() {
        if usable_seat_base(&candidate) {
            return Ok(candidate);
        }
    }
    Err("no directory outside the temporary roots and outside a git checkout holds a first-run seat, so run the tests from a checkout outside /tmp or set HOME to a directory outside it.".to_string())
}

fn usable_seat_base(base: &Path) -> bool {
    if ljos_cli::bookkeep::path_is_temp(Some(base)) || base.as_os_str().len() > ROOT_ROOM {
        return false;
    }
    if std::fs::create_dir_all(base).is_err() {
        return false;
    }
    !base.ancestors().any(|dir| dir.join(".git").exists())
}

fn tools_ready() -> bool {
    if tools::halt(tools::Tool::Vissue) || tools::halt(tools::Tool::Packsetd) {
        return false;
    }
    if let Err(line) = seat_base() {
        if std::env::var(tools::REQUIRE_TOOLS).ok().as_deref() == Some("1") {
            panic!("{line}");
        }
        eprintln!("{line}");
        return false;
    }
    for name in ["claimdag", "deedar", "ljos-policyd", "packset"] {
        if which(name).is_none() {
            let line = format!("{name} is not on PATH, so run `source scripts/sibling-tools.sh`.");
            if std::env::var(tools::REQUIRE_TOOLS).ok().as_deref() == Some("1") {
                panic!("{line}");
            }
            eprintln!("{line}");
            return false;
        }
    }
    true
}

#[test]
fn a_seat_base_sits_outside_the_temporary_roots_and_any_checkout() {
    if let Ok(base) = seat_base() {
        assert!(
            !ljos_cli::bookkeep::path_is_temp(Some(&base)),
            "{} is in a temporary root",
            base.display()
        );
        assert!(
            !base.ancestors().any(|dir| dir.join(".git").exists()),
            "{} is inside a checkout",
            base.display()
        );
    }
    assert!(!usable_seat_base(&std::env::temp_dir()));
    assert!(!usable_seat_base(Path::new("/tmp/ljos-first-run-seats")));
    assert!(!usable_seat_base(Path::new("/var/tmp")));
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    if manifest.ancestors().any(|dir| dir.join(".git").exists()) {
        assert!(!usable_seat_base(manifest), "a checkout holds no seat");
    }
    let long = format!("/{}", "a".repeat(ROOT_ROOM));
    assert!(!usable_seat_base(Path::new(&long)));
}

#[test]
fn the_remember_sample_uses_a_neutral_host() {
    let runners = [
        "cursor",
        "claude",
        "codex",
        "grok",
        "gemini",
        "opencode",
        "antigravity",
        "agy",
    ];
    for rel in [
        "../../docs/source/getting-started.rst",
        "../../docs/orgmode/getting-started.org",
    ] {
        let text = repo_file(rel);
        assert!(
            text.contains("this write uses ada"),
            "{rel} does not use the sample login"
        );
        for name in runners {
            assert!(
                !text.contains(&format!("unknown-{name}"))
                    && !text.contains(&format!("uses {name}")),
                "{rel} uses the runner name {name} as a seat"
            );
        }
    }
}

/// The crates.io stand-in passes doctor's own lookup, and fails a call
/// that names another URL beside it, a path under the crate, or a URL
/// given with `--url`. Each call starts with `-q`, which is what doctor
/// passes so curl reads no config file.
#[test]
fn the_curl_stand_in_reads_every_url_a_call_names() {
    let lookup = "https://crates.io/api/v1/crates/ljos";
    let doctor: Vec<&str> = ljos_cli::CRATE_CURL_FLAGS
        .iter()
        .copied()
        .chain([lookup])
        .collect();
    assert!(
        matches!(doctor.first().copied(), Some("-q" | "--disable")),
        "doctor's curl must skip config files"
    );
    let cases: Vec<(Vec<&str>, bool)> = vec![
        (doctor, true),
        (vec!["-q", "-sSm", "3", "-Aljos-doctor", lookup], true),
        (
            vec!["-q", "-sSm3", "--user-agent", "ljos-doctor", lookup],
            true,
        ),
        (
            vec![
                "-q",
                "-sS",
                "https://github.com/leidarljos/ljos/releases/latest",
                lookup,
            ],
            false,
        ),
        (
            vec!["-q", "-sS", "https://crates.io/api/v1/crates/ljos/owners"],
            false,
        ),
        (
            vec!["-q", "-sS", "https://crates.io/api/v1/crates/ljos?page=2"],
            false,
        ),
        (
            vec!["-q", "--url", "https://github.com/leidarljos/ljos", lookup],
            false,
        ),
        (vec!["-q", "-sS", "-A", "ljos-doctor"], false),
    ];
    for (args, passes) in cases {
        let (answered, held) = stand_in_call(&args, false, CurlSeat::Clean);
        assert_eq!(answered, passes, "{args:?}");
        assert_eq!(held, passes, "{args:?}");
    }
}

/// A URL a config file names that the arguments do not.
const HIDDEN: &str = "https://github.com/leidarljos/ljos/releases/latest";

/// Where a call to the stand-in finds a default `.curlrc`.
#[derive(Debug, Clone, Copy)]
enum CurlSeat {
    /// An empty home, with no `.curlrc` planted in it.
    Clean,
    Home,
    CurlHome,
    Xdg,
    XdgNoDot,
    NoHome,
}

/// Runs the crates.io stand-in once with `args` in an empty home, and says
/// whether it answered and whether `assert_only_registry` held after it.
/// `curl.conf` in the working directory names [`HIDDEN`], and so does
/// stdin when `stdin_conf` is set and any `.curlrc` `seat` puts down.
fn stand_in_call(args: &[&str], stdin_conf: bool, seat: CurlSeat) -> (bool, bool) {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let home = dir.path().join("home");
    let curl_home = dir.path().join("curl-home");
    let xdg = home.join(".config");
    for dir in [&home, &curl_home, &xdg] {
        std::fs::create_dir_all(dir).unwrap();
    }
    registry::stand_in(&bin, Duration::ZERO);
    let conf = format!("url = \"{HIDDEN}\"\n");
    let write = |path: PathBuf| {
        ljos_cli::write_file(&path, &conf)
            .unwrap_or_else(|err| panic!("write {}: {err}", path.display()))
    };
    write(dir.path().join("curl.conf"));
    match seat {
        CurlSeat::Clean | CurlSeat::NoHome => {}
        CurlSeat::Home => write(home.join(".curlrc")),
        CurlSeat::CurlHome => write(curl_home.join(".curlrc")),
        CurlSeat::Xdg => write(xdg.join(".curlrc")),
        CurlSeat::XdgNoDot => write(xdg.join("curlrc")),
    }
    let mut call =
        ljos_cli::program(bin.join("curl")).unwrap_or_else(|err| panic!("the stand-in: {err}"));
    call.args(args)
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("XDG_CONFIG_HOME", &xdg)
        .env("CURL_HOME", &curl_home);
    if !matches!(seat, CurlSeat::NoHome) {
        call.env("HOME", &home);
    }
    if stdin_conf {
        call.stdin(std::fs::File::open(dir.path().join("curl.conf")).unwrap());
    } else {
        call.stdin(Stdio::null());
    }
    let said = call
        .output()
        .unwrap_or_else(|err| panic!("run the stand-in: {err}"));
    let held = std::panic::catch_unwind(|| registry::assert_only_registry(&bin, "a stand-in call"))
        .is_ok();
    (said.status.success(), held)
}

/// A config file can name a URL the arguments do not. curl reads one
/// unless `-q` or `--disable` is the first argument, so the stand-in
/// refuses a call that does not start that way. It also refuses every
/// way to pass `-K` or `--config`, and every option it does not know.
#[test]
fn the_curl_stand_in_refuses_what_could_hide_a_url() {
    use CurlSeat::{Clean, CurlHome, Home, NoHome, Xdg, XdgNoDot};
    let lookup = "https://crates.io/api/v1/crates/ljos";
    let doctor = ["-sS", "-A", "ljos-doctor", "--max-time", "3", lookup];
    let quiet: Vec<&str> = ljos_cli::CRATE_CURL_FLAGS
        .iter()
        .copied()
        .chain([lookup])
        .collect();
    let cases: &[(&[&str], bool, CurlSeat, bool)] = &[
        (&doctor, false, Clean, false),
        (quiet.as_slice(), false, Home, true),
        (quiet.as_slice(), false, CurlHome, true),
        (quiet.as_slice(), false, NoHome, true),
        (
            &[
                "-q",
                "--silent",
                "--show-error",
                "--fail",
                "--connect-timeout",
                "2",
                lookup,
            ],
            false,
            Clean,
            true,
        ),
        (&["-q", "-K", "curl.conf", lookup], false, Clean, false),
        (&["-q", "-sK", "curl.conf", lookup], false, Clean, false),
        (&["-q", "-sSKcurl.conf", lookup], false, Clean, false),
        (
            &["-q", "--config", "curl.conf", lookup],
            false,
            Clean,
            false,
        ),
        (&["-q", "--config=curl.conf", lookup], false, Clean, false),
        (&["-q", "--conf", "curl.conf", lookup], false, Clean, false),
        (&["-q", "-K", "-", lookup], true, Clean, false),
        (&["-q", "-sK-", lookup], true, Clean, false),
        (&["-q", "--config", "-", lookup], true, Clean, false),
        (&doctor, false, Home, false),
        (&doctor, false, CurlHome, false),
        (&doctor, false, Xdg, false),
        (&doctor, false, XdgNoDot, false),
        (&doctor, false, NoHome, false),
        (&["-sS", "-q", lookup], false, Home, false),
        (&["-qs", lookup], false, Home, false),
        (&["-q", lookup, "--next", HIDDEN], false, Clean, false),
        (&["-q", lookup, "-:", HIDDEN], false, Clean, false),
        (&["-q", "--next", lookup], false, Clean, false),
        (&["-q", "@curl.conf"], false, Clean, false),
        (&["-q", lookup, "@curl.conf"], false, Clean, false),
        (
            &["-q", "https://crates.io/api/v1/crates/{ljos,serde}"],
            false,
            Clean,
            false,
        ),
        (
            &["-q", "https://crates.io/api/v1/crates/ljo[r-s]"],
            false,
            Clean,
            false,
        ),
        (
            &["-q", "--user-agent=ljos-doctor", lookup],
            false,
            Clean,
            false,
        ),
        (
            &["-q", "--url=https://example.org/", lookup],
            false,
            Clean,
            false,
        ),
        (
            &["-q", "-x", "http://127.0.0.1:9", lookup],
            false,
            Clean,
            false,
        ),
        (
            &["-q", "--resolve", "crates.io:443:127.0.0.1", lookup],
            false,
            Clean,
            false,
        ),
        (&["-q", "-sSv", lookup], false, Clean, false),
        (&["-q", "", lookup], false, Clean, false),
        (&["-q", lookup, "--max-time"], false, Clean, false),
    ];
    for (args, stdin_conf, seat, passes) in cases {
        let (answered, held) = stand_in_call(args, *stdin_conf, *seat);
        assert_eq!(answered, *passes, "{args:?} with {seat:?}");
        assert_eq!(held, *passes, "{args:?} with {seat:?}");
    }
}

/// Body a curlrc names with `url=`, and the body the arguments name.
const CURLRC_BODY: &str = "HIDDEN-CURLRC-BODY\n";
const ASKED_BODY: &str = "ASKED-BODY\n";

/// One file curl reads when `-q` is not the first argument.
struct CurlrcFile {
    label: &'static str,
    file: PathBuf,
    env: Vec<(&'static str, PathBuf)>,
    /// Run curl with this directory as uid 0's passwd home. The host
    /// account's `~/.curlrc` is left alone.
    passwd_home: Option<PathBuf>,
}

fn make_dir(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).unwrap_or_else(|err| panic!("create {}: {err}", path.display()));
    path
}

fn curlrc_files(root: &Path) -> Vec<CurlrcFile> {
    let blank = make_dir(root, "blank");
    let mut files = Vec::new();
    let curl_home = make_dir(root, "curl-home");
    files.push(CurlrcFile {
        label: "CURL_HOME/.curlrc",
        file: curl_home.join(".curlrc"),
        env: vec![("CURL_HOME", curl_home), ("HOME", blank.clone())],
        passwd_home: None,
    });
    let xdg = make_dir(root, "xdg");
    files.push(CurlrcFile {
        label: "XDG_CONFIG_HOME/.curlrc",
        file: xdg.join(".curlrc"),
        env: vec![("XDG_CONFIG_HOME", xdg), ("HOME", blank.clone())],
        passwd_home: None,
    });
    let xdg_no_dot = make_dir(root, "xdg-no-dot");
    files.push(CurlrcFile {
        label: "XDG_CONFIG_HOME/curlrc",
        file: xdg_no_dot.join("curlrc"),
        env: vec![("XDG_CONFIG_HOME", xdg_no_dot), ("HOME", blank.clone())],
        passwd_home: None,
    });
    let home = make_dir(root, "home");
    files.push(CurlrcFile {
        label: "HOME/.curlrc",
        file: home.join(".curlrc"),
        env: vec![("HOME", home)],
        passwd_home: None,
    });
    let curl_home_config = make_dir(root, "curl-home-config");
    files.push(CurlrcFile {
        label: "CURL_HOME/.config/curlrc",
        file: curl_home_config.join(".config/curlrc"),
        env: vec![("CURL_HOME", curl_home_config), ("HOME", blank.clone())],
        passwd_home: None,
    });
    let home_config = make_dir(root, "home-config");
    files.push(CurlrcFile {
        label: "HOME/.config/curlrc",
        file: home_config.join(".config/curlrc"),
        env: vec![("HOME", home_config)],
        passwd_home: None,
    });
    let home_config_empty = make_dir(root, "home-config-empty-xdg");
    files.push(CurlrcFile {
        label: "HOME/.config/curlrc with XDG_CONFIG_HOME empty",
        file: home_config_empty.join(".config/curlrc"),
        env: vec![
            ("HOME", home_config_empty),
            ("XDG_CONFIG_HOME", PathBuf::new()),
        ],
        passwd_home: None,
    });
    let curl_home_config_empty = make_dir(root, "curl-home-config-empty-xdg");
    files.push(CurlrcFile {
        label: "CURL_HOME/.config/curlrc with XDG_CONFIG_HOME empty",
        file: curl_home_config_empty.join(".config/curlrc"),
        env: vec![
            ("CURL_HOME", curl_home_config_empty),
            ("HOME", blank.clone()),
            ("XDG_CONFIG_HOME", PathBuf::new()),
        ],
        passwd_home: None,
    });
    let passwd_home = make_dir(root, "passwd-home");
    files.push(CurlrcFile {
        label: "passwd home .curlrc",
        file: passwd_home.join(".curlrc"),
        env: vec![("HOME", blank)],
        passwd_home: Some(passwd_home),
    });
    let passwd_home_no_home = make_dir(root, "passwd-home-no-home");
    files.push(CurlrcFile {
        label: "passwd home .curlrc with HOME unset",
        file: passwd_home_no_home.join(".curlrc"),
        env: Vec::new(),
        passwd_home: Some(passwd_home_no_home),
    });
    files
}

/// A curl release as `major.minor.patch`.
type CurlVersion = (u32, u32, u32);

/// The curl releases that read each place [`curlrc_files`] plants, as
/// `(label, first release, first release that no longer reads it)`. From
/// `findfile` in curl's `src/tool_findfile.c`, and `homedir` in
/// `src/tool_homedir.c` before 7.81.0:
///
/// - 7.73.0 reads `XDG_CONFIG_HOME/.curlrc` (curl commit 4be1f8dc0, #5837).
/// - 7.81.0 tries the directory of each variable in turn, then the passwd
///   home. Before it, the first of `CURL_HOME`, `XDG_CONFIG_HOME` holding a
///   `.curlrc`, `HOME` and the passwd home was the only place read, so a
///   set `HOME` hid the passwd home (2be1aa619, #8035).
/// - 7.82.0 reads `.config/curlrc` under `CURL_HOME` and `HOME` when
///   `XDG_CONFIG_HOME` is unset or empty (764e4f066, #8213).
/// - 8.10.0 reads `XDG_CONFIG_HOME/curlrc`, without the dot, in place of
///   `XDG_CONFIG_HOME/.curlrc` (26e9d3a89, #14600).
///
/// `CURL_HOME/.curlrc`, `HOME/.curlrc` and the passwd home with `HOME`
/// unset are read by every release.
const CURLRC_READ_BY: &[(&str, CurlVersion, Option<CurlVersion>)] = &[
    ("CURL_HOME/.curlrc", (0, 0, 0), None),
    ("XDG_CONFIG_HOME/.curlrc", (7, 73, 0), Some((8, 10, 0))),
    ("XDG_CONFIG_HOME/curlrc", (8, 10, 0), None),
    ("HOME/.curlrc", (0, 0, 0), None),
    ("CURL_HOME/.config/curlrc", (7, 82, 0), None),
    ("HOME/.config/curlrc", (7, 82, 0), None),
    (
        "HOME/.config/curlrc with XDG_CONFIG_HOME empty",
        (7, 82, 0),
        None,
    ),
    (
        "CURL_HOME/.config/curlrc with XDG_CONFIG_HOME empty",
        (7, 82, 0),
        None,
    ),
    ("passwd home .curlrc", (7, 81, 0), None),
    ("passwd home .curlrc with HOME unset", (0, 0, 0), None),
];

fn curl_reads(label: &str, version: CurlVersion) -> bool {
    let Some((_, since, until)) = CURLRC_READ_BY.iter().find(|(name, ..)| *name == label) else {
        panic!("{label} is not in CURLRC_READ_BY");
    };
    version >= *since && until.is_none_or(|until| version < until)
}

/// The release `curl --version` names on its first line, such as
/// `curl 8.5.0 (x86_64-pc-linux-gnu) libcurl/8.5.0 …`.
fn curl_version(curl: &str) -> CurlVersion {
    let said = ljos_cli::program(curl)
        .unwrap_or_else(|err| panic!("{curl}: {err}"))
        .arg("--version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|err| panic!("{curl} --version: {err}"));
    let text = String::from_utf8_lossy(&said.stdout);
    let first = text.lines().next().unwrap_or("");
    let number = |part: Option<&str>| -> Option<u32> {
        let digits: String = part?.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    let mut parts = first.split_whitespace().nth(1).unwrap_or("").split('.');
    match (
        number(parts.next()),
        number(parts.next()),
        number(parts.next()),
    ) {
        (Some(major), Some(minor), Some(patch)) => (major, minor, patch),
        _ => panic!("{curl} --version names no release: {first:?}"),
    }
}

/// Why the passwd home places cannot run here, or `None` when they can.
/// They bind a copy of `/etc/passwd` inside
/// `unshare --user --mount --map-root-user`, which a kernel or AppArmor can
/// refuse to an unprivileged user. That refusal is a skip whatever
/// [`tools::REQUIRE_TOOLS`] says; a missing `unshare` fails under it.
fn no_user_namespace() -> Option<String> {
    let said = ljos_cli::program("unshare")
        .unwrap_or_else(|err| panic!("unshare: {err}"))
        .args([
            "--user",
            "--mount",
            "--map-root-user",
            "--",
            "mount",
            "--bind",
            "/etc/passwd",
            "/etc/passwd",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output();
    let why = match said {
        Ok(said) if said.status.success() => return None,
        Ok(said) => String::from_utf8_lossy(&said.stderr)
            .lines()
            .next()
            .unwrap_or("no message")
            .trim()
            .to_string(),
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                && std::env::var(tools::REQUIRE_TOOLS).as_deref() == Ok("1") =>
        {
            panic!("unshare is not in /usr/bin or /bin ({err}); apt-get install util-linux")
        }
        Err(err) => err.to_string(),
    };
    Some(format!(
        "unshare --user --mount --map-root-user is unavailable ({why}), so the passwd home curlrc places were skipped."
    ))
}

fn passwd_naming_root(home: &Path) -> String {
    let text =
        std::fs::read_to_string("/etc/passwd").unwrap_or_else(|err| panic!("read passwd: {err}"));
    let home = home.display().to_string();
    let mut out = String::new();
    let mut found = false;
    for line in text.lines() {
        let mut parts: Vec<String> = line.split(':').map(str::to_string).collect();
        if parts.len() >= 6 && parts[2] == "0" {
            parts[5] = home.clone();
            found = true;
        }
        out.push_str(&parts.join(":"));
        out.push('\n');
    }
    assert!(found, "passwd has no uid 0");
    out
}

fn curl_env(cmd: &mut Command, place: &CurlrcFile) {
    cmd.env_clear();
    cmd.env("PATH", "/usr/bin:/bin");
    for (key, value) in &place.env {
        cmd.env(key, value);
    }
}

fn flags_for(url: &str, quiet: bool) -> Vec<String> {
    let mut flags: Vec<String> = ljos_cli::CRATE_CURL_FLAGS
        .iter()
        .map(|flag| (*flag).to_string())
        .collect();
    if !quiet && matches!(flags.first().map(String::as_str), Some("-q" | "--disable")) {
        flags.remove(0);
    }
    flags.push(url.to_string());
    flags
}

fn run_curl(args: &[String], place: &CurlrcFile) -> std::process::Output {
    let curl = "/usr/bin/curl";
    if let Some(home) = &place.passwd_home {
        let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("passwd dir: {err}"));
        let passwd = dir.path().join("passwd");
        ljos_cli::write_file(&passwd, passwd_naming_root(home))
            .unwrap_or_else(|err| panic!("write {}: {err}", passwd.display()));
        let mut cmd = ljos_cli::program("unshare").unwrap_or_else(|err| panic!("unshare: {err}"));
        cmd.args([
            "--user",
            "--mount",
            "--map-root-user",
            "--",
            "bash",
            "-c",
            "mount --bind \"$1\" /etc/passwd && shift && exec \"$@\"",
            "bash",
        ])
        .arg(&passwd)
        .arg(curl)
        .args(args);
        curl_env(&mut cmd, place);
        // The passwd file has to outlive the child.
        let said = cmd
            .output()
            .unwrap_or_else(|err| panic!("{}: {err}", place.label));
        drop(dir);
        said
    } else {
        let mut cmd = ljos_cli::program(curl).unwrap_or_else(|err| panic!("curl: {err}"));
        cmd.args(args);
        curl_env(&mut cmd, place);
        cmd.output()
            .unwrap_or_else(|err| panic!("{}: {err}", place.label))
    }
}

fn curl_body(said: &std::process::Output) -> Result<String, String> {
    if !said.status.success() {
        return Err(format!(
            "curl exited {}\n{}",
            said.status,
            String::from_utf8_lossy(&said.stderr)
        ));
    }
    String::from_utf8(said.stdout.clone()).map_err(|err| err.to_string())
}

/// Doctor's crates.io curl starts with the flags in [`ljos_cli::CRATE_CURL_FLAGS`].
/// A curlrc `url=` in every file curl reads is absent from that run, and
/// the same arguments without a leading `-q` or `--disable` do fetch it
/// from each place [`CURLRC_READ_BY`] says this curl reads.
#[test]
fn a_curlrc_url_in_every_file_curl_reads_is_not_fetched() {
    let version = curl_version("/usr/bin/curl");
    let no_userns = no_user_namespace();
    if let Some(line) = &no_userns {
        eprintln!("{line}");
    }
    let root = tempfile::tempdir().unwrap_or_else(|err| panic!("root: {err}"));
    let asked = root.path().join("asked");
    let hidden = root.path().join("hidden");
    ljos_cli::write_file(&asked, ASKED_BODY)
        .unwrap_or_else(|err| panic!("write {}: {err}", asked.display()));
    ljos_cli::write_file(&hidden, CURLRC_BODY)
        .unwrap_or_else(|err| panic!("write {}: {err}", hidden.display()));
    let asked_url = format!("file://{}", asked.display());
    let conf = format!("url = \"file://{}\"\n", hidden.display());
    let lookup = "https://crates.io/api/v1/crates/ljos";
    let mut failures = Vec::new();
    for place in curlrc_files(root.path()) {
        if place.passwd_home.is_some() && no_userns.is_some() {
            continue;
        }
        if let Some(parent) = place.file.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|err| panic!("create {}: {err}", parent.display()));
        }
        ljos_cli::write_file(&place.file, &conf)
            .unwrap_or_else(|err| panic!("write {}: {err}", place.file.display()));
        let loud = if curl_reads(place.label, version) {
            match curl_body(&run_curl(&flags_for(&asked_url, false), &place)) {
                Ok(text) if text.contains(CURLRC_BODY) && text.contains(ASKED_BODY) => None,
                Ok(text) => Some(format!(
                    "{}: without -q, curl {}.{}.{} did not read {}\n{text}",
                    place.label,
                    version.0,
                    version.1,
                    version.2,
                    place.file.display()
                )),
                Err(err) => Some(format!("{}: control curl failed: {err}", place.label)),
            }
        } else {
            None
        };
        if let Some(err) = loud {
            failures.push(err);
            continue;
        }
        match curl_body(&run_curl(&flags_for(&asked_url, true), &place)) {
            Ok(text) if text.contains(ASKED_BODY) && !text.contains("HIDDEN-CURLRC-BODY") => {}
            Ok(text) => failures.push(format!(
                "{}: doctor's curl fetched the curlrc url\n{text}",
                place.label
            )),
            Err(err) => failures.push(format!("{}: doctor's curl failed: {err}", place.label)),
        }
        let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("stand-in dir: {err}"));
        let bin = dir.path().join("bin");
        registry::stand_in(&bin, Duration::ZERO);
        let mut call =
            ljos_cli::program(bin.join("curl")).unwrap_or_else(|err| panic!("stand-in: {err}"));
        call.args(flags_for(lookup, true));
        curl_env(&mut call, &place);
        let said = call
            .output()
            .unwrap_or_else(|err| panic!("{} stand-in: {err}", place.label));
        let held =
            std::panic::catch_unwind(|| registry::assert_only_registry(&bin, place.label)).is_ok();
        if !said.status.success() || !held {
            failures.push(format!(
                "{}: stand-in did not answer with only the crates URL\n{}",
                place.label,
                String::from_utf8_lossy(&said.stderr)
            ));
        } else if String::from_utf8_lossy(&said.stdout).contains("HIDDEN-CURLRC-BODY") {
            failures.push(format!("{}: stand-in printed the curlrc body", place.label));
        }
    }
    assert!(
        failures.is_empty(),
        "a curlrc url was fetched:\n{}",
        failures.join("\n")
    );
}

fn repo_file(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn readme_steps() -> Vec<DocStep> {
    let text = repo_file("../../README.md");
    let Some((_, rest)) = text.split_once(README_START) else {
        panic!("README has no {README_START}");
    };
    let Some((block, _)) = rest.split_once(README_END) else {
        panic!("README has no {README_END}");
    };
    shell_commands(block)
        .into_iter()
        .map(|command| DocStep {
            place: "README.md (Tutorial)".to_string(),
            command,
            sample_ids: Vec::new(),
        })
        .collect()
}

fn getting_started_steps() -> Vec<DocStep> {
    let text = repo_file("../../docs/source/getting-started.rst");
    let lines: Vec<&str> = text.lines().collect();
    let mut steps = Vec::new();
    let mut section = "Getting started".to_string();
    let mut index = 0;
    while index < lines.len() {
        if index + 1 < lines.len()
            && is_underline(lines[index + 1])
            && !lines[index].trim().is_empty()
        {
            section = lines[index].trim().to_string();
            index += 2;
            continue;
        }
        if lines[index].trim() == ".. code:: console" {
            index += 1;
            let (block, next) = console_block(&lines, index);
            steps.extend(commands_in_block(&block, &section));
            index = next;
            continue;
        }
        index += 1;
    }
    steps
}

fn is_underline(line: &str) -> bool {
    let trim = line.trim();
    trim.len() >= 3
        && (trim.chars().all(|ch| ch == '=')
            || trim.chars().all(|ch| ch == '-')
            || trim.chars().all(|ch| ch == '~'))
}

fn console_block<'a>(lines: &'a [&str], start: usize) -> (Vec<&'a str>, usize) {
    let mut index = start;
    while index < lines.len() && lines[index].trim().is_empty() {
        index += 1;
    }
    if index >= lines.len() {
        return (Vec::new(), index);
    }
    let indent = lines[index]
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .count();
    let mut block = Vec::new();
    while index < lines.len() {
        let line = lines[index];
        if line.trim().is_empty() {
            block.push(line);
            index += 1;
            continue;
        }
        let width = line.chars().take_while(|ch| ch.is_whitespace()).count();
        if width < indent {
            break;
        }
        block.push(line);
        index += 1;
    }
    (block, index)
}

fn commands_in_block(block: &[&str], section: &str) -> Vec<DocStep> {
    let mut steps = Vec::new();
    let mut command: Option<String> = None;
    let mut sample = String::new();
    let flush = |command: &mut Option<String>, sample: &mut String, steps: &mut Vec<DocStep>| {
        let Some(command) = command.take() else {
            return;
        };
        steps.push(DocStep {
            place: format!("docs/source/getting-started.rst ({section})"),
            command,
            sample_ids: sample_ids(sample),
        });
        sample.clear();
    };
    for line in block {
        let trim = line.trim();
        if let Some(cmd) = trim.strip_prefix("$ ") {
            flush(&mut command, &mut sample, &mut steps);
            command = Some(cmd.to_string());
        } else if command.is_some() {
            sample.push_str(trim);
            sample.push('\n');
        }
    }
    flush(&mut command, &mut sample, &mut steps);
    steps
}

fn shell_commands(block: &str) -> Vec<String> {
    let mut cmds = Vec::new();
    let mut fence = false;
    let mut shell = false;
    for line in block.lines() {
        let trim = line.trim();
        if trim.starts_with("```") {
            if fence {
                fence = false;
                shell = false;
            } else {
                fence = true;
                let lang = trim.trim_start_matches('`').trim();
                shell = lang.is_empty() || matches!(lang, "sh" | "bash" | "shell" | "console");
            }
            continue;
        }
        if fence && shell && !trim.is_empty() && !trim.starts_with('#') {
            cmds.push(trim.strip_prefix("$ ").unwrap_or(trim).to_string());
        }
    }
    cmds
}

fn sample_ids(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in tokens(text) {
        if is_ticket(&token) && !out.iter().any(|seen| seen == &token) {
            out.push(token);
        }
    }
    out
}

fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            cur.push(ch);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_ticket(token: &str) -> bool {
    let Some((project, suffix)) = token.split_once('-') else {
        return false;
    };
    !project.is_empty()
        && project
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase())
        && project
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        && suffix.len() >= 4
        && suffix.chars().all(|ch| ch.is_ascii_alphanumeric())
        && project != "unknown"
}

fn ids_for_project(text: &str, project: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in tokens(text) {
        if is_ticket(&token)
            && token
                .split_once('-')
                .is_some_and(|(name, _)| name == project)
            && !out.iter().any(|seen| seen == &token)
        {
            out.push(token);
        }
    }
    out
}

fn learn_ids(sample: &[String], output: &str, map: &mut BTreeMap<String, String>) {
    let mut used = Vec::new();
    for sample_id in sample {
        let Some(project) = sample_id.split_once('-').map(|(name, _)| name) else {
            continue;
        };
        let Some(actual) = ids_for_project(output, project)
            .into_iter()
            .find(|id| !used.contains(id))
        else {
            continue;
        };
        used.push(actual.clone());
        map.insert(sample_id.clone(), actual);
    }
}

fn apply_ids(command: &str, map: &BTreeMap<String, String>) -> String {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by_key(|key| std::cmp::Reverse(key.len()));
    let mut out = command.to_string();
    for key in keys {
        if let Some(value) = map.get(key) {
            out = out.replace(key, value);
        }
    }
    out
}

fn needs_network(command: &str) -> bool {
    command.contains("curl")
        || command.contains("https://")
        || command.contains("http://")
        || command.contains("cargo binstall")
        || command.contains("install.sh")
}

fn budget(command: &str) -> Duration {
    const SLOW: &[&str] = &[
        "onboard",
        "doctor",
        "sitting",
        "consensus",
        "uninstall",
        "offboard",
        "file",
        "claim",
        "vote",
        "finish",
        "ready",
    ];
    if SLOW.iter().any(|word| command.contains(word)) {
        Duration::from_secs(90)
    } else {
        Duration::from_secs(45)
    }
}

fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            continue;
        }
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn settled(term: &pty::Pty) -> String {
    let mut last = String::new();
    let mut same = 0u8;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && same < 3 {
        let now = term.shown();
        if now == last {
            same += 1;
        } else {
            same = 0;
            last = now;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    term.shown()
}

fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        let cont = line.starts_with(' ') || line.starts_with('\t');
        if cont && !cur.is_empty() {
            cur.push(' ');
            cur.push_str(line.trim());
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur = line.to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn backtick_commands(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('`') else {
            break;
        };
        let body = &rest[..end];
        if body.starts_with("ljos ")
            || body.starts_with("export ")
            || body.starts_with("packset ")
            || body.starts_with("git ")
            || body.starts_with("sh ")
            || body.starts_with("cargo ")
        {
            out.push(body.to_string());
        }
        rest = &rest[end + 1..];
    }
    out
}

fn is_error_line(line: &str) -> bool {
    let trim = line.trim();
    if trim.is_empty() {
        return false;
    }
    let lower = trim.to_ascii_lowercase();
    trim.starts_with("no\t")
        || trim.starts_with("warn\t")
        || lower.starts_with("error:")
        || lower.starts_with("error ")
        || trim.starts_with("ljos is not ready")
        || trim.starts_with("ljos:")
        || trim.starts_with("label ")
        || lower.starts_with("warning:")
        || lower.contains("panicked")
        || lower.contains("commit refused")
        || trim.contains("so run `")
}

fn names_next(line: &str) -> bool {
    let ticks = backtick_commands(line);
    if ticks.len() == 1 {
        return true;
    }
    if ticks.len() > 1 {
        return false;
    }
    let so_run = line.matches("so run ").count();
    if so_run == 1 {
        let rest = line.split("so run ").nth(1).unwrap_or("");
        return !rest.trim().is_empty();
    }
    so_run == 0 && line.contains("run the command again") && line.contains("user.name")
}

fn assert_plain(place: &str, command: &str, output: &str) {
    for noise in NOISE {
        assert!(
            !output.contains(noise),
            "{place}\n{command}\noutput contains {noise:?}:\n{output}"
        );
    }
    assert!(
        !debug_struct(output),
        "{place}\n{command}\noutput has a Debug struct:\n{output}"
    );
    for line in logical_lines(output) {
        if is_error_line(&line) {
            assert!(
                names_next(&line),
                "{place}\n{command}\nan error line does not name the one next command:\n{line}\n---\n{output}"
            );
        }
    }
}

fn debug_struct(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 3 < bytes.len() {
        if bytes[index].is_ascii_alphabetic() {
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            if index - start >= 2
                && bytes.get(index) == Some(&b' ')
                && bytes.get(index + 1) == Some(&b'{')
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn assert_ok(
    place: &str,
    command: &str,
    code: i32,
    output: &str,
    elapsed: Duration,
    limit: Duration,
) {
    assert!(
        elapsed <= limit,
        "{place}\n{command}\ntook {}s, past {}s:\n{output}",
        elapsed.as_secs(),
        limit.as_secs()
    );
    assert_plain(place, command, output);
    assert_eq!(code, 0, "{place}\n{command}\n{output}");
}

fn assert_step(
    place: &str,
    command: &str,
    code: i32,
    output: &str,
    elapsed: Duration,
    limit: Duration,
) {
    if command.contains("LJOS_SEAT=bob ljos claim") {
        assert!(
            elapsed <= limit,
            "{place}\n{command}\ntook {}s, past {}s:\n{output}",
            elapsed.as_secs(),
            limit.as_secs()
        );
        assert_plain(place, command, output);
        assert_eq!(code, 1, "{place}\n{command}\n{output}");
        assert_eq!(output.lines().count(), 1, "{place}\n{command}\n{output}");
        expect_documented(command, output);
        return;
    }
    assert_ok(place, command, code, output, elapsed, limit);
    expect_documented(command, output);
}

fn expect_documented(command: &str, output: &str) {
    if command.contains("ljos doctor") && !command.contains("--all") && !command.contains("--fix") {
        // An info row, including an encoder that is not installed, is printed
        // and is not one of the warn or no rows the summary counts.
        let needs_action = output
            .lines()
            .any(|line| line.starts_with("warn\t") || line.starts_with("no\t"));
        if needs_action {
            assert!(output.contains("ljos needs action"), "{command}\n{output}");
            assert!(!output.contains("ljos is ready"), "{command}\n{output}");
        } else {
            assert!(output.contains("ljos is ready"), "{command}\n{output}");
        }
        let mut keys = vec!["seat source", "sitting gate", "not active here"];
        if !needs_action {
            keys.extend(["memory", "tracker", "command guard"]);
        }
        for key in keys {
            assert!(output.contains(key), "{command}\nmissing {key:?}\n{output}");
        }
        let seat = output.find("seat source").expect("seat source");
        let encoder = output
            .find("encoder")
            .unwrap_or_else(|| panic!("{command}\nno encoder row:\n{output}"));
        let gate = output.find("sitting gate").expect("sitting gate");
        assert!(
            seat < encoder && encoder < gate,
            "{command}\nencoder row is not between the seat and the gate:\n{output}"
        );
        assert!(
            output.contains(&format!(
                "not installed. Search matches words, so run `{}`.",
                ljos_cli::RELEASE_INSTALL_COMMAND
            )),
            "{command}\n{output}"
        );
        assert!(
            !output.contains("cargo binstall"),
            "{command}\ncargo-binstall is not on this PATH:\n{output}"
        );
    }
    if command.contains("ljos remember") && command.contains("lexical default") {
        assert!(
            output.contains(LESSON) || output.contains("already stored"),
            "{command}\n{output}"
        );
        assert!(
            output.contains("id ") || output.contains("already stored"),
            "{command}\n{output}"
        );
    }
    if command.contains("ljos search") && command.contains("lexical default") {
        assert!(output.contains(LESSON), "{command}\n{output}");
        assert!(
            output.contains("matched by words (encoder not installed)"),
            "{command}\n{output}"
        );
    }
    if command.contains("ljos search") && command.contains("which fusion") {
        assert!(output.contains(NO_HITS), "{command}\n{output}");
    }
    if command.contains("ljos file") && command.contains("Pick a codec") {
        for key in ["project:", "Pick a codec"] {
            assert!(output.contains(key), "{command}\nmissing {key}\n{output}");
        }
        assert!(
            !output.contains("tracker git: committed"),
            "{command}\na commit that landed was printed:\n{output}"
        );
    }
    if command.contains("sitting") && command.contains("--plan") {
        assert!(output.contains("next:"), "{command}\n{output}");
        assert_eq!(
            output.matches("next:").count(),
            1,
            "{command}\nprinted next more than once:\n{output}"
        );
        assert!(
            output.contains("matched by words (encoder not installed)"),
            "{command}\n{output}"
        );
    }
    if command.contains("ljos finish") {
        assert!(output.contains("ticked"), "{command}\n{output}");
        assert!(output.contains("next:"), "{command}\n{output}");
    }
    if command.contains("ljos ready") && command.contains("--mine") {
        assert!(output.contains("next:"), "{command}\n{output}");
    }
    if command.contains("--assignee bob") {
        assert!(output.contains("held by another"), "{command}\n{output}");
        assert!(
            !output.contains("label claim-conflict"),
            "{command}\n{output}"
        );
        assert!(output.contains("ljos send"), "{command}\n{output}");
    }
    if command.contains("ljos offboard") {
        assert!(output.contains("offboard"), "{command}\n{output}");
    }
    if command.contains("ljos consensus") {
        assert!(
            output.contains("verdict:") || output.contains("folded"),
            "{command}\n{output}"
        );
    }
}

fn documented_commands() -> Vec<DocStep> {
    let mut steps = readme_steps();
    steps.extend(getting_started_steps());
    for needle in [
        "install.sh",
        "ljos doctor",
        "ljos remember",
        "ljos search",
        "ljos file",
        "ljos onboard",
        "ljos prefer",
        "ljos sitting",
        "ljos finish",
        "ljos ready",
        "ljos claim",
        "ljos note",
        "ljos timeline",
        "ljos vote",
        "ljos consensus",
        "ljos offboard",
        "ljos uninstall",
        "git config --global user.name",
        "git config --global user.email",
    ] {
        assert!(
            steps.iter().any(|step| step.command.contains(needle)),
            "the README or the getting-started page dropped {needle}"
        );
    }
    steps
}

#[test]
fn the_getting_started_doctor_sample_names_the_missing_encoder() {
    for rel in [
        "../../docs/source/getting-started.rst",
        "../../docs/orgmode/getting-started.org",
        "../../docs/source/index.rst",
        "../../docs/orgmode/index.org",
    ] {
        let text = repo_file(rel);
        assert!(
            text.contains(
                "info\tencoder\tnot installed. Search matches words, so run `cargo install --locked packset-embed`."
            ),
            "{rel} dropped the encoder row a machine with cargo prints"
        );
    }
    let readme = repo_file("../../README.md");
    assert!(
        readme.contains(
            "an `encoder` row follows and names the install command this machine can run"
        ),
        "README dropped the encoder row"
    );
}

fn sample_region(text: &str, key: &str) -> String {
    let begin = format!("ljos-sample tutorial-{key}");
    let start = text
        .find(&begin)
        .unwrap_or_else(|| panic!("{key} sample is missing"))
        + begin.len();
    let rest = &text[start..];
    let end_at = format!("END ljos-sample tutorial-{key}");
    let end = rest
        .find(&end_at)
        .unwrap_or_else(|| panic!("{key} sample has no end"));
    rest[..end].to_string()
}

#[test]
fn the_getting_started_search_sample_is_a_terminal() {
    for rel in [
        "../../docs/source/getting-started.rst",
        "../../docs/orgmode/getting-started.org",
        "../../docs/source/index.rst",
        "../../docs/orgmode/index.org",
    ] {
        let text = repo_file(rel);
        let search = if rel.contains("index") {
            sample_region(&text, "index")
        } else {
            sample_region(&text, "search")
        };
        assert!(
            search.contains("lesson, today"),
            "{rel} search sample is not the terminal row:\n{search}"
        );
        assert!(
            !search.contains("from an agent"),
            "{rel} search sample is a runner transcript:\n{search}"
        );
    }
    for rel in [
        "../../docs/source/getting-started.rst",
        "../../docs/orgmode/getting-started.org",
    ] {
        let text = repo_file(rel);
        let prefer = sample_region(&text, "prefer");
        assert!(
            prefer.contains("preference, today"),
            "{rel} preference sample is not the terminal row:\n{prefer}"
        );
        assert!(
            !prefer.contains("from an agent"),
            "{rel} preference sample is a runner transcript:\n{prefer}"
        );
        let script = sample_region(&text, "script");
        assert!(
            script.contains("lesson, from an agent"),
            "{rel} script sample is not the no-terminal row:\n{script}"
        );
        assert!(
            text.contains("script or cron"),
            "{rel} does not name a script or cron"
        );
        let timeline = sample_region(&text, "timeline");
        assert!(
            timeline.contains("[end 0123456789abcdef]"),
            "{rel} timeline sample split the fence:\n{timeline}"
        );
        assert!(
            timeline.contains("the codec is one file"),
            "{rel} timeline sample has no note:\n{timeline}"
        );
    }
}

#[test]
fn the_getting_started_page_names_both_preference_rows() {
    for rel in [
        "../../docs/source/getting-started.rst",
        "../../docs/orgmode/getting-started.org",
    ] {
        let text = repo_file(rel);
        assert!(
            text.contains("preference, today"),
            "{rel} does not name the terminal preference row"
        );
        assert!(
            text.contains("preference, from an agent, not checked"),
            "{rel} does not name the runner preference row"
        );
    }
    for rel in [
        "../../docs/source/index.rst",
        "../../docs/orgmode/index.org",
    ] {
        let text = repo_file(rel);
        assert!(
            text.contains("lesson, today"),
            "{rel} does not name the terminal lesson row"
        );
        assert!(
            text.contains("script or cron") && text.contains("lesson, from an agent"),
            "{rel} does not name the no-terminal lesson row"
        );
    }
}

#[test]
fn the_getting_started_plan_sample_prints_next_once() {
    for rel in [
        "../../docs/source/getting-started.rst",
        "../../docs/orgmode/getting-started.org",
    ] {
        let text = repo_file(rel);
        let (_, after) = text
            .split_once("ljos sitting --plan")
            .unwrap_or_else(|| panic!("{rel} missing sitting --plan"));
        let (block, _) = after
            .split_once("ljos finish day-cd34")
            .unwrap_or_else(|| panic!("{rel} missing finish day-cd34"));
        assert_eq!(
            block.matches("next: day-cd34  measure the files").count(),
            1,
            "{rel} printed the plan's next line more than once:\n{block}"
        );
    }
}

#[test]
fn the_getting_started_page_names_both_empty_memory_outcomes() {
    for rel in [
        "../../docs/source/getting-started.rst",
        "../../docs/orgmode/getting-started.org",
    ] {
        let text = repo_file(rel);
        assert!(
            text.contains(ljos_cli::NO_MATCHING_MEMORIES_LINE.trim_end()),
            "{rel} does not name an answering search with no matches"
        );
        assert!(
            text.contains("matched by words (encoder not installed)"),
            "{rel} does not name the no-encoder path"
        );
    }
}

#[test]
fn a_new_seat_follows_the_readme_and_getting_started() {
    if !tools_ready() {
        return;
    }
    let mut seat = Seat::open();
    let mut ids = BTreeMap::new();
    let mut stood_in_for_install = false;
    let mut quoted_miss = false;
    for step in documented_commands() {
        if needs_network(&step.command) {
            eprintln!("skipped: {} ({})", step.command, step.place);
            if !stood_in_for_install && step.command.contains("install.sh") {
                stood_in_for_install = true;
                let place = "README.md (Tutorial) and docs/source/getting-started.rst (Install): install.sh sets up the tracker, the memory writer and the deed store";
                let limit = budget("ljos onboard");
                let cwd = seat.work.clone();
                let (code, output, elapsed) = seat.run("ljos onboard", &cwd, limit);
                assert_step(place, "ljos onboard", code, &output, elapsed, limit);
            }
            continue;
        }
        let command = apply_ids(&step.command, &ids);
        let cwd = seat.work.clone();
        let limit = budget(&command);
        let (code, output, elapsed) = seat.run(&command, &cwd, limit);
        learn_ids(&step.sample_ids, &output, &mut ids);
        assert_step(&step.place, &command, code, &output, elapsed, limit);
        if !quoted_miss && command.contains("ljos search") && command.contains("lexical default") {
            quoted_miss = true;
            let place =
                "README.md (Tutorial) and docs/source/getting-started.rst (A lesson and a search)";
            let extra = "ljos search \"which fusion\"";
            let limit = budget(extra);
            let (code, output, elapsed) = seat.run(extra, &cwd, limit);
            assert_step(place, extra, code, &output, elapsed, limit);
        }
    }
    assert!(
        stood_in_for_install,
        "the docs no longer tell a new seat to run install.sh"
    );
    registry::assert_only_registry(&seat.bin(), "the README and getting started");
}

/// Two votes typed in one shell are one voice. The settle this prints is
/// the getting-started sample: the push stays closed, and the advice
/// names social power and holds.
#[test]
fn one_shell_folds_two_votes_and_names_the_settle() {
    if !tools_ready() {
        return;
    }
    let mut seat = Seat::open();
    let cwd = seat.work.clone();
    let limit = budget("ljos onboard");
    let (code, output, elapsed) = seat.run("ljos onboard", &cwd, limit);
    assert_step(
        "one shell: onboard",
        "ljos onboard",
        code,
        &output,
        elapsed,
        limit,
    );
    for line in [
        "git config --global user.name \"Your Name\"",
        "git config --global user.email \"you@example.com\"",
    ] {
        let (code, output, _) = seat.run(line, &cwd, Duration::from_secs(20));
        assert_eq!(code, 0, "{line}\n{output}");
    }
    let script = r#"set -e
id=$(LJOS_SEAT=alice ljos file "Pick a codec" -p fold -t decision --options keep,drop)
LJOS_SEAT=alice ljos claim "$id" --assignee alice >/dev/null
LJOS_SEAT=alice ljos vote "$id" --for keep --used none
LJOS_SEAT=bob ljos vote "$id" --for keep --used none
LJOS_SEAT=alice ljos consensus "$id" --no-file
"#;
    let limit = Duration::from_secs(120);
    let (code, output, elapsed) = seat.run(script, &cwd, limit);
    assert!(
        elapsed <= limit,
        "one shell took {}s:\n{output}",
        elapsed.as_secs()
    );
    assert_eq!(code, 0, "{output}");
    assert_plain("one shell", script, &output);
    for key in [
        "claimed session bob, folded into alice (same shell)",
        "folded into one voice (same shell alice)",
        "does not clear a push",
        "social power",
        "holds: keep",
        "trust default (equal weight)",
    ] {
        assert!(output.contains(key), "missing {key:?}\n{output}");
    }
    assert!(
        !output.contains("DeGroot") && !output.contains("influence:"),
        "the settle sample drifted back:\n{output}"
    );
}

/// The seat's `curl` cannot reach crates.io, so doctor prints the skipped
/// version check, and that warn row names the one next command.
#[test]
fn an_unreachable_crates_io_row_names_the_next_command() {
    if !tools_ready() {
        return;
    }
    let mut seat = Seat::open();
    // The open seat answers crates.io. This row is the lookup that does
    // not, so the failing curl replaces that stand-in.
    stub_unreachable_crates_io(&seat.bin());
    let cwd = seat.work.clone();
    let limit = budget("ljos onboard");
    let (code, output, elapsed) = seat.run("ljos onboard", &cwd, limit);
    assert_step(
        "crates.io unreachable: onboard",
        "ljos onboard",
        code,
        &output,
        elapsed,
        limit,
    );
    for line in [
        "git config --global user.name \"Your Name\"",
        "git config --global user.email \"you@example.com\"",
    ] {
        let (code, output, _) = seat.run(line, &cwd, Duration::from_secs(20));
        assert_eq!(code, 0, "{line}\n{output}");
    }
    let limit = budget("ljos doctor");
    let (_, output, elapsed) = seat.run("ljos doctor", &cwd, limit);
    assert!(
        elapsed <= limit,
        "doctor took {}s:\n{output}",
        elapsed.as_secs()
    );
    let row = logical_lines(&output)
        .into_iter()
        .find(|line| line.trim().starts_with("warn\tcrates.io\t"))
        .unwrap_or_else(|| panic!("no crates.io row:\n{output}"));
    assert!(row.contains("version check was skipped"), "{row}");
    assert!(names_next(&row), "{row}");
    assert_plain("crates.io unreachable: doctor", "ljos doctor", &output);
}

#[test]
fn a_confused_order_stays_plain() {
    if !tools_ready() {
        return;
    }
    let mut seat = Seat::open();
    let elsewhere = seat.elsewhere.clone();
    // No writer yet. Search is the command a person tries before remember.
    let limit = budget("ljos search");
    let (code, output, elapsed) = seat.run("ljos search \"lexical default\"", &elsewhere, limit);
    assert!(
        elapsed <= limit,
        "search before a writer took {}s:\n{output}",
        elapsed.as_secs()
    );
    assert_plain(
        "confused: search before remember, no writer, not a git directory",
        "ljos search \"lexical default\"",
        &output,
    );
    assert!(
        matches!(code, 0 | 1 | 75),
        "search before a writer exited {code}:\n{output}"
    );
    if code != 0 {
        let lines: Vec<String> = logical_lines(&output)
            .into_iter()
            .filter(|line| !line.trim().is_empty())
            .collect();
        assert!(
            !lines.is_empty(),
            "a failed search printed nothing:\n{output}"
        );
        for line in &lines {
            assert!(
                names_next(line),
                "a failed search did not name the one next command:\n{line}\n---\n{output}"
            );
        }
    }

    let limit = budget("ljos onboard");
    let (code, output, elapsed) = seat.run("ljos onboard", &elsewhere, limit);
    assert_step(
        "confused: onboard from a directory that is not a git checkout",
        "ljos onboard",
        code,
        &output,
        elapsed,
        limit,
    );

    // The store is still empty. A shorter search is not the next step.
    let limit = budget("ljos search");
    let (code, output, elapsed) = seat.run("ljos search \"lexical default\"", &elsewhere, limit);
    assert_ok(
        "confused: search before any remember",
        "ljos search \"lexical default\"",
        code,
        &output,
        elapsed,
        limit,
    );
    assert!(
        output.contains("ljos remember"),
        "an empty search did not name remember:\n{output}"
    );

    let (code, output, elapsed) = seat.run("ljos search \"lexical default\"", &elsewhere, limit);
    assert_ok(
        "confused: the same search a second time",
        "ljos search \"lexical default\"",
        code,
        &output,
        elapsed,
        limit,
    );
    assert!(
        output.contains("ljos remember"),
        "a second empty search did not name remember:\n{output}"
    );

    let remember = format!("ljos remember \"{LESSON}\"");
    let limit = budget("ljos remember");
    let (code, output, elapsed) = seat.run(&remember, &elsewhere, limit);
    assert_step(
        "confused: remember from a directory that is not a git checkout",
        &remember,
        code,
        &output,
        elapsed,
        limit,
    );

    let (code, output, elapsed) = seat.run(&remember, &elsewhere, limit);
    assert_step(
        "confused: the same remember a second time",
        &remember,
        code,
        &output,
        elapsed,
        limit,
    );
    assert!(
        (output.contains("already stored") || output.contains("instead of adding a copy"))
            && output.contains("ljos search"),
        "a second remember was not a plain next step:\n{output}"
    );
    assert!(
        !output.contains("unreviewed"),
        "a second remember called the stored lesson unreviewed:\n{output}"
    );

    let limit = budget("ljos file");
    let (code, output, elapsed) = seat.run("ljos file \"Pick a codec\" -p demo", &elsewhere, limit);
    assert!(
        elapsed <= limit,
        "file took {}s:\n{output}",
        elapsed.as_secs()
    );
    assert_plain(
        "confused: file before git user.name, not a git directory",
        "ljos file \"Pick a codec\" -p demo",
        &output,
    );
    assert!(
        output.contains("user.name") && output.contains("run the command again"),
        "a file with no git name did not say what to set:\n{output}"
    );
    let _ = code;

    let (code, _, _) = seat.run(
        "git config --global user.name \"Your Name\"",
        &elsewhere,
        Duration::from_secs(20),
    );
    assert_eq!(code, 0, "git config user.name");
    let (code, _, _) = seat.run(
        "git config --global user.email \"you@example.com\"",
        &elsewhere,
        Duration::from_secs(20),
    );
    assert_eq!(code, 0, "git config user.email");

    let (code, output, elapsed) = seat.run("ljos file \"Pick a codec\" -p demo", &elsewhere, limit);
    assert_step(
        "confused: file again from a directory that is not a git checkout",
        "ljos file \"Pick a codec\" -p demo",
        code,
        &output,
        elapsed,
        limit,
    );

    let limit = budget("ljos doctor");
    for pass in ["first", "second"] {
        let (code, output, elapsed) = seat.run("ljos doctor", &elsewhere, limit);
        assert_step(
            &format!("confused: ljos doctor a {pass} time, not a git directory"),
            "ljos doctor",
            code,
            &output,
            elapsed,
            limit,
        );
    }
    registry::assert_only_registry(&seat.bin(), "a confused order");
}
