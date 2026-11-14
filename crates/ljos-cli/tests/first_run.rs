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

ljos_test_boot::harness!();

#[path = "common/pty.rs"]
mod pty;
#[path = "common/tools.rs"]
mod tools;

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
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

const NO_HITS: &str = "No hits for \"which fusion\", so run `ljos search fusion`.";

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
    fn open() -> Self {
        let root = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/home/ubuntu"))
            .join(format!(
                ".ljos-first-run-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
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
        std::fs::set_permissions(root.join("run"), std::fs::Permissions::from_mode(0o700)).unwrap();
        link_programs(&bin);
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
        std::fs::copy(src, bin.join(name)).unwrap_or_else(|err| panic!("copy {name}: {err}"));
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
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
            let _ = symlink(&path, bin.join(name));
        }
    }
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

fn tools_ready() -> bool {
    if tools::halt(tools::Tool::Vissue) || tools::halt(tools::Tool::Packsetd) {
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
    assert_ok(place, command, code, output, elapsed, limit);
    expect_documented(command, output);
}

fn expect_documented(command: &str, output: &str) {
    if command.contains("ljos doctor") && !command.contains("--all") && !command.contains("--fix") {
        for key in [
            "ljos is ready",
            "memory",
            "tracker",
            "command guard",
            "seat source",
            "sitting gate",
            "not active here",
        ] {
            assert!(output.contains(key), "{command}\nmissing {key:?}\n{output}");
        }
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
        if !ljos_cli::tool_installed("packset-embed") {
            assert!(
                output.contains("matched by words (encoder not installed)"),
                "{command}\n{output}"
            );
        }
    }
    if command.contains("ljos search")
        && command.contains("which fusion")
        && !ljos_cli::tool_installed("packset-embed")
    {
        assert!(output.contains(NO_HITS), "{command}\n{output}");
    }
    if command.contains("ljos file") && command.contains("Pick a codec") {
        for key in ["project:", "Pick a codec", "tracker git:"] {
            assert!(output.contains(key), "{command}\nmissing {key}\n{output}");
        }
    }
    if command.contains("sitting") && command.contains("--plan") {
        assert!(output.contains("next:"), "{command}\n{output}");
        if !ljos_cli::tool_installed("packset-embed") {
            assert!(
                output.contains("memory degraded: ranked by words"),
                "{command}\n{output}"
            );
        }
    }
    if command.contains("ljos finish") {
        assert!(output.contains("ticked"), "{command}\n{output}");
        assert!(output.contains("next:"), "{command}\n{output}");
    }
    if command.contains("ljos ready") && command.contains("--mine") {
        assert!(output.contains("next:"), "{command}\n{output}");
    }
    if command.contains("--assignee bob") {
        assert!(output.contains("claim-conflict"), "{command}\n{output}");
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
}
