//! The shell lines between the quickstart markers in README.md, on a
//! fresh seat.
//!
//! The markers are HTML comments, so the rendered page is unchanged. A
//! command that needs the network, or `packsetd`, `vissue` or
//! `age-keygen` when that program is not installed, is skipped and the
//! skip line names it. A line the README quotes has to stay in the marked
//! block, and the command has to print it.

ljos_test_boot::harness!();

#[path = "common/pty.rs"]
mod pty;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const START: &str = "<!-- ljos-quickstart:start -->";
const END: &str = "<!-- ljos-quickstart:end -->";

const LESSON: &str = "The lexical default is BM25+. It beat BM25 by two points on turns.";

/// What `ljos search "which fusion"` prints when the stored lesson shares
/// no word with that query. The README quotes this line.
const NO_HITS: &str = "No hits for \"which fusion\", so run `ljos search fusion`.";

const DOCTOR: &[&str] = &[
    "ljos is ready: memory, tracker and command guard work",
    "ljos doctor --all",
    "seat source",
    "every seat",
    "sitting gate",
    "not active here",
];

const PACK_VERBS: &[&str] = &[
    "remember", "search", "prefer", "recall", "hook", "graded", "due", "forget", "memories",
    "doctor", "onboard",
];

const VIS_VERBS: &[&str] = &[
    "file",
    "claim",
    "note",
    "timeline",
    "vote",
    "consensus",
    "sitting",
    "finish",
    "ready",
    "release",
    "options",
    "doctor",
];

const AGE_VERBS: &[&str] = &["handover", "receive", "learn", "sync"];

fn readme_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../README.md")
}

fn marked_block(readme: &str) -> &str {
    let Some((_, rest)) = readme.split_once(START) else {
        panic!("README has no {START}");
    };
    let Some((block, _)) = rest.split_once(END) else {
        panic!("README has no {END}");
    };
    block
}

/// Shell lines in fenced blocks. A `console` prompt's `$ ` is not part of
/// the command. JSON fences are not shell.
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
        if !fence || !shell || trim.is_empty() || trim.starts_with('#') {
            continue;
        }
        cmds.push(trim.strip_prefix("$ ").unwrap_or(trim).to_string());
    }
    cmds
}

fn split_words(cmd: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quote = None;
    for ch in cmd.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => cur.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

fn ljos_verb(cmd: &str) -> Option<String> {
    let words = split_words(cmd);
    let mut words = words.as_slice();
    while words
        .first()
        .is_some_and(|word| word.contains('=') && !word.starts_with('-'))
    {
        words = &words[1..];
    }
    if words.first().map(String::as_str) != Some("ljos") {
        return None;
    }
    words.get(1).cloned()
}

fn needs_network(cmd: &str) -> bool {
    cmd.contains("curl")
        || cmd.contains("wget")
        || cmd.contains("https://")
        || cmd.contains("http://")
        || cmd.contains("cargo binstall")
        || cmd.contains("install.sh")
}

/// `None` when the command can run here. `Some` names the missing piece:
/// `network`, `packsetd`, `vissue` or `age-keygen`.
fn missing_tool(cmd: &str) -> Option<&'static str> {
    if needs_network(cmd) {
        return Some("network");
    }
    let verb = ljos_verb(cmd).unwrap_or_default();
    let age =
        AGE_VERBS.contains(&verb.as_str()) || split_words(cmd).iter().any(|w| w == "age-keygen");
    if age && !ljos_cli::tool_installed("age-keygen") {
        return Some("age-keygen");
    }
    if PACK_VERBS.contains(&verb.as_str()) && !ljos_test_boot::Packsetd::on_path() {
        return Some("packsetd");
    }
    if VIS_VERBS.contains(&verb.as_str()) && !ljos_cli::tool_installed("vissue") {
        return Some("vissue");
    }
    None
}

fn skip_line(cmd: &str, tool: &str) {
    if tool == "network" {
        eprintln!("skipped: {cmd}: needs network");
    } else {
        eprintln!("skipped: {cmd}: {tool} is not installed");
    }
}

fn has_short_id(text: &str) -> bool {
    text.lines().any(|line| {
        let Some(rest) = line.trim().strip_prefix("id ") else {
            return false;
        };
        let id: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        id.len() == 8
            && rest[id.len()..]
                .chars()
                .next()
                .is_none_or(|c| c == ',' || c.is_whitespace())
    })
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

struct Seat {
    /// Kept so the directory outlives every child.
    #[allow(dead_code)]
    root: tempfile::TempDir,
    work: PathBuf,
    env: BTreeMap<String, String>,
    pack: Option<ljos_test_boot::Packsetd>,
}

impl Seat {
    fn open() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let work = root.path().join("work");
        let bin = root.path().join("bin");
        for dir in [
            home.join(".config"),
            home.join(".local/bin"),
            home.join(".local/share"),
            home.join(".local/state"),
            home.join(".cache"),
            root.path().join("run"),
            work.clone(),
            bin.clone(),
        ] {
            std::fs::create_dir_all(&dir).unwrap();
        }
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos"), bin.join("ljos")).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos-mcp"), bin.join("ljos-mcp")).unwrap();
        let inherited = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
        let mut env = BTreeMap::new();
        env.insert("HOME".into(), home.display().to_string());
        env.insert("PATH".into(), format!("{}:{inherited}", bin.display()));
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
            root.path().join("run").display().to_string(),
        );
        env.insert(
            "TMPDIR".into(),
            root.path().join("run").display().to_string(),
        );
        env.insert("NO_COLOR".into(), "1".into());
        env.insert("TERM".into(), "xterm".into());
        env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
        env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
        Self {
            root,
            work,
            env,
            pack: None,
        }
    }

    fn start_pack(&mut self) {
        let pack = ljos_test_boot::Packsetd::open();
        self.env
            .insert("PACKSET_URL".into(), pack.url().to_string());
        self.env.insert("PACKSET_AUTH".into(), "off".into());
        self.env.insert(
            "PACKSET_HOME".into(),
            pack.pack_home().display().to_string(),
        );
        self.env.insert("PACKSET_WORKSPACE".into(), "seat".into());
        self.pack = Some(pack);
    }

    /// `install.sh` is skipped because it downloads. It leaves a tracker,
    /// so the later commands run on a seat that script would have made.
    fn prepare_tracker(&mut self) {
        let (code, output) = self.run_tty("ljos onboard");
        assert_eq!(
            code, 0,
            "ljos onboard, which install.sh runs before the quickstart, failed:\n{output}"
        );
    }

    fn apply_export(&mut self, line: &str) -> (i32, String) {
        let body = line.trim().strip_prefix("export ").expect("export");
        let name = body.split('=').next().unwrap_or("").trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return self.run_tty(line);
        }
        let script = format!("{line}\nprintf '%s' \"${{{name}}}\"");
        let out = Command::new("sh")
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
            let value = String::from_utf8_lossy(&out.stdout).into_owned();
            self.env.insert(name.to_string(), value);
        }
        (code, String::from_utf8_lossy(&out.stderr).into_owned())
    }

    fn run_tty(&self, line: &str) -> (i32, String) {
        let term = pty::Pty::open();
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(line);
        cmd.current_dir(&self.work);
        cmd.env_clear();
        cmd.envs(&self.env);
        cmd.stdin(Stdio::null());
        term.attach_output(&mut cmd);
        let mut child = term.spawn(&mut cmd);
        let pid = child.id();
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(90);
            while Instant::now() < deadline {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = Command::new("kill")
                .args(["-s", "KILL", "--", &format!("-{pid}")])
                .status();
        });
        let status = child.wait().unwrap_or_else(|err| panic!("{line}: {err}"));
        done.store(true, Ordering::SeqCst);
        (status.code().unwrap_or(1), plain(&settled(&term)))
    }

    fn run(&mut self, line: &str) -> (i32, String) {
        if line.trim_start().starts_with("export ") {
            self.apply_export(line)
        } else {
            self.run_tty(line)
        }
    }
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

fn promised_in_readme(cmd: &str) -> Vec<&'static str> {
    match ljos_verb(cmd).as_deref() {
        Some("doctor") => DOCTOR.to_vec(),
        Some("remember") => vec!["unknown-"],
        Some("search") if cmd.contains("lexical default") => {
            vec!["ranking is lexical: encoder unavailable"]
        }
        Some("file") => vec![
            "project:",
            "tracker git: committed",
            "no remote, kept local",
        ],
        _ => Vec::new(),
    }
}

fn expect_output(cmd: &str, output: &str) {
    match ljos_verb(cmd).as_deref() {
        Some("doctor") => {
            for key in DOCTOR {
                assert!(output.contains(key), "{cmd}\nmissing {key:?}\n{output}");
            }
        }
        Some("remember") => {
            assert!(output.contains(LESSON), "{cmd}\n{output}");
            assert!(
                has_short_id(output),
                "remember did not print the first eight characters of the id:\n{output}"
            );
            assert!(output.contains("unknown-"), "{cmd}\n{output}");
        }
        Some("search") if cmd.contains("lexical default") => {
            assert!(output.contains(LESSON), "{cmd}\n{output}");
            if !ljos_cli::tool_installed("packset-embed") {
                assert!(
                    output.contains("ranking is lexical: encoder unavailable"),
                    "{cmd}\n{output}"
                );
            }
        }
        Some("file") => {
            for key in [
                "project:",
                "Pick a codec",
                "tracker git: committed",
                "no remote, kept local",
            ] {
                assert!(output.contains(key), "{cmd}\nmissing {key}\n{output}");
            }
            assert!(output.contains("demo-"), "file printed no id:\n{output}");
            assert!(
                output.lines().any(|line| line.contains('/')),
                "file printed no path:\n{output}"
            );
        }
        Some("onboard") => assert!(!output.trim().is_empty(), "{cmd} printed nothing"),
        _ => {}
    }
}

fn git_value(seat: &Seat, key: &str) -> String {
    let out = Command::new("git")
        .args(["config", "--global", "--get", key])
        .env_clear()
        .envs(&seat.env)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|err| panic!("git config --get {key}: {err}"));
    assert!(out.status.success(), "git config --get {key}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn the_readme_quickstart_runs() {
    let readme = std::fs::read_to_string(readme_path()).unwrap_or_else(|err| {
        panic!("read {}: {err}", readme_path().display());
    });
    let block = marked_block(&readme);
    let cmds = shell_commands(block);
    assert!(
        !cmds.is_empty(),
        "no shell commands between the quickstart markers"
    );
    for needle in [
        "install.sh",
        "export PATH=",
        "git config --global user.name",
        "git config --global user.email",
        "ljos doctor",
        "ljos remember",
        "ljos search",
        "ljos file",
        "ljos onboard",
    ] {
        assert!(
            cmds.iter().any(|cmd| cmd.contains(needle)),
            "quickstart has no {needle}"
        );
    }
    assert!(
        block.contains("first eight characters"),
        "README no longer says remember prints the short id"
    );
    assert!(
        block.contains(NO_HITS),
        "README no longer quotes the empty-search line"
    );
    assert!(
        block.contains(LESSON),
        "README dropped the quickstart lesson"
    );
    for cmd in &cmds {
        for key in promised_in_readme(cmd) {
            assert!(
                block.contains(key),
                "README quickstart dropped {key:?} promised by {cmd}"
            );
        }
    }

    let mut seat = Seat::open();
    if ljos_test_boot::Packsetd::on_path() {
        seat.start_pack();
    }
    // The install script is the command that makes the tracker. It is
    // skipped below. Prepare that seat only when the tools it would have
    // installed are already on PATH.
    let ready_seat = seat.pack.is_some()
        && ljos_cli::tool_installed("vissue")
        && ljos_cli::tool_installed("packset")
        && ljos_cli::tool_installed("deedar");
    if ready_seat {
        seat.prepare_tracker();
    }

    let mut ran = 0u32;
    for cmd in &cmds {
        if let Some(tool) = missing_tool(cmd) {
            skip_line(cmd, tool);
            continue;
        }
        let (code, output) = seat.run(cmd);
        assert_eq!(code, 0, "{cmd}\n{output}");
        expect_output(cmd, &output);
        if cmd.contains("user.name") {
            assert_eq!(git_value(&seat, "user.name"), "Your Name");
        }
        if cmd.contains("user.email") {
            assert_eq!(git_value(&seat, "user.email"), "you@example.com");
        }
        if cmd.contains("export PATH=") {
            let path = seat.env.get("PATH").map(String::as_str).unwrap_or("");
            assert!(
                path.contains(".local/bin"),
                "export PATH did not keep the install directory: {path}"
            );
        }
        if cmd.contains("ljos search") && cmd.contains("lexical default") && block.contains(NO_HITS)
        {
            let extra = "ljos search \"which fusion\"";
            let (code, output) = seat.run_tty(extra);
            assert_eq!(code, 0, "{extra}\n{output}");
            if !ljos_cli::tool_installed("packset-embed") {
                assert!(output.contains(NO_HITS), "{extra}\n{output}");
            }
        }
        ran += 1;
    }
    assert!(ran > 0, "every quickstart command was skipped");
}
