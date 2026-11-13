//! The prompt hook on a pack of many claims. The tests check the lines
//! it shows for a fixed pack, and how many claims a stage reads as the
//! pack grows.
//!
//! A fresh home and its own packsetd. The hook shows the writer's owner
//! token, as a seat does, so it reads the listing as trusted.

ljos_test_boot::harness!();
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::Value;

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

struct Seat {
    dir: tempfile::TempDir,
    port: u16,
    path: String,
    token: String,
    _daemon: ljos_test_boot::Packsetd,
}

impl Seat {
    fn env(&self, cmd: &mut Command) {
        env_for(cmd, self.dir.path(), &self.path, Some(self.port));
    }

    /// A standing lesson numbered `i` that says `text`, written `age_days`
    /// ago and due for review at `due`, or on the writer's own clock.
    fn post(&self, i: usize, text: &str, age_days: u64, due: Option<&str>) {
        let mut atom = serde_json::json!({
            "schema": "inside.atom/v1",
            "id": format!("c{i:031}"),
            "kind": "lesson",
            "level": "explicit",
            "text": text,
            "workspace": ljos_cli::SEAT_WORKSPACE,
            "entities": ["seat:scale", "horizon:standing"],
            "ts": days_ago(age_days),
        });
        if let Some(due) = due {
            atom["due_at"] = due.into();
        }
        let posted = ureq::post(&format!("http://127.0.0.1:{}/v1/atoms", self.port))
            .set("authorization", &format!("Bearer {}", self.token))
            .timeout(Duration::from_secs(10))
            .send_json(atom);
        assert!(posted.is_ok(), "post {text:?}: {posted:?}");
    }

    fn live(&self) -> usize {
        let listed: Value = ureq::get(&format!("http://127.0.0.1:{}/v1/atoms", self.port))
            .query("workspace", ljos_cli::SEAT_WORKSPACE)
            .query("embedding", "omit")
            .set("authorization", &format!("Bearer {}", self.token))
            .timeout(Duration::from_secs(30))
            .call()
            .expect("GET /v1/atoms")
            .into_json()
            .expect("atoms");
        listed["atoms"].as_array().map_or(0, Vec::len)
    }

    /// The prompt hook's answer to `prompt` in `session`, with the stage
    /// clock on its stderr.
    fn prompt(&self, session: &str, prompt: &str) -> Output {
        let payload = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": session,
            "prompt": prompt,
            "cwd": self.dir.path(),
        })
        .to_string();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.arg("hook");
        self.env(&mut cmd);
        cmd.env("LJOS_HOOK_TIMING", "1");
        let said = run(cmd, Some(&payload));
        assert!(said.status.success(), "ljos hook: {}", text(&said));
        said
    }
}

fn env_for(cmd: &mut Command, home: &Path, path: &str, port: Option<u16>) {
    cmd.env_clear()
        .current_dir(home)
        .env("HOME", home)
        .env("PATH", path)
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", home.join("tmp"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off");
    if let Some(port) = port {
        cmd.env("PACKSET_PORT", port.to_string());
    }
}

fn run(mut cmd: Command, stdin: Option<&str>) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut pipe = child.stdin.take().expect("stdin");
    if let Some(text) = stdin {
        pipe.write_all(text.as_bytes()).expect("write stdin");
    }
    drop(pipe);
    child.wait_with_output().expect("wait")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A fresh home with its own packsetd, the owner token read from the pack
/// home the hook finds through `XDG_DATA_HOME`.
fn seat() -> Seat {
    let Some(packsetd) = on_path("packsetd") else {
        panic!("packsetd is not on PATH; CI builds it from packset develop");
    };
    let dir = tempfile::tempdir().expect("home");
    for sub in [
        "tmp",
        "run",
        ".config",
        ".local/share",
        ".local/state",
        ".cache",
    ] {
        std::fs::create_dir_all(dir.path().join(sub)).expect("home dirs");
    }
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
    let pack_home = dir.path().join(".local/share/packset");
    std::fs::create_dir_all(&pack_home).expect("pack home");
    let mut started = ljos_test_boot::Packsetd::build();
    started.bin(&packsetd);
    started.pack_at(&pack_home);
    env_for(started.command(), dir.path(), &path, None);
    let daemon = started.spawn();
    let port = daemon.port();
    let token = std::fs::read_to_string(pack_home.join("token"))
        .expect("packsetd left no token in its home")
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    Seat {
        dir,
        port,
        path,
        token,
        _daemon: daemon,
    }
}

/// Word `n` of a vocabulary of three consonant-vowel pairs, 70³ words.
fn word(n: usize) -> String {
    const CONSONANTS: &[u8] = b"bdfgklmnprstvz";
    const VOWELS: &[u8] = b"aeiou";
    let mut n = n;
    let mut word = String::new();
    for _ in 0..3 {
        word.push(char::from(CONSONANTS[n % CONSONANTS.len()]));
        n /= CONSONANTS.len();
        word.push(char::from(VOWELS[n % VOWELS.len()]));
        n /= VOWELS.len();
    }
    word
}

/// The six words lesson `i` alone says.
fn own_words(i: usize) -> Vec<String> {
    (0..6).map(|k| word(i * 6 + k)).collect()
}

/// Distinct lessons as `scripts/terra/hook_scale.py` writes them: the case
/// number opens the sentence and six words of its own follow, so every
/// write stays live, and every lesson shares `case` with every prompt.
fn lesson(i: usize) -> String {
    format!("Case {i}: {}.", own_words(i).join(" "))
}

/// A lesson that shares no word with any prompt here.
fn note(i: usize) -> String {
    format!("Note {i}: {}.", own_words(i).join(" "))
}

/// The prompt that names lesson `i`: its sentence without the stop.
fn prompt_for(i: usize) -> String {
    lesson(i).trim_end_matches('.').to_string()
}

/// A stamp `days` before now, so the pack's recency and the hook's ages
/// read the same at any date.
fn days_ago(days: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs();
    ljos_cli::utc_at(now - days * 86_400)
}

/// The fixed pack: sixty lessons, more than the hook's window of twenty
/// that all share `case`, so the writer's cut comes back full and the
/// hook's own pass over every stored claim runs. Every fifth came due
/// two days ago, inside the due line's week, and every seventh a month
/// ago, so the line counts both. Their ages cycle through nine days, so
/// the lessons one prompt names show different ages, oldest first.
fn fixed_pack(seat: &Seat) {
    const LESSONS: usize = 60;
    for i in 0..LESSONS {
        let due = if i % 5 == 0 {
            Some(days_ago(2))
        } else if i % 7 == 0 {
            Some(days_ago(30))
        } else {
            None
        };
        seat.post(i, &lesson(i), (i % 9) as u64, due.as_deref());
    }
    assert_eq!(seat.live(), LESSONS, "a lesson replaced another");
}

/// A prompt that names lessons 22 and 41 by two of their own words each.
fn two_lessons_prompt() -> String {
    let (a, b) = (own_words(22), own_words(41));
    format!("Case 22 and case 41: {} {} {} {}", a[0], a[1], b[2], b[3])
}

/// What the prompt hook printed for the fixed pack before the listing was
/// shared between its readers, recorded from that build on these prompts.
const SAID: [&str; 3] = [CASE_14, TWO_LESSONS, CASE_14];

const CASE_14: &str = concat!(
    r#"{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"Remembered, shown 1 of 1, oldest first. `ljos search` lists the pack.\n  lesson, agent-derived, 5 days ago, from scale\n    Case 14: bedaba dedaba fedaba gedaba kedaba ledaba.\n12 claims came due for review this week (19 due in all). Review is not the task:\nwhen the work reaches a pause, `ljos due` shows the soonest 8; grade one only\nafter checking it against what you know (`ljos graded ID`, `--lapsed` when it no\nlonger holds) and leave the rest due."}}"#,
    "\n"
);

const TWO_LESSONS: &str = concat!(
    r#"{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"Remembered, shown 2 of 2, oldest first. `ljos search` lists the pack.\n  lesson, agent-derived, 5 days ago, from scale\n    Case 41: pigaba rigaba sigaba tigaba vigaba zigaba.\n\n  lesson, agent-derived, 4 days ago, from scale\n    Case 22: mudaba nudaba pudaba rudaba sudaba tudaba.\n12 claims came due for review this week (19 due in all). Review is not the task:\nwhen the work reaches a pause, `ljos due` shows the soonest 8; grade one only\nafter checking it against what you know (`ljos graded ID`, `--lapsed` when it no\nlonger holds) and leave the rest due."}}"#,
    "\n"
);

/// The hook shows the same lines for a fixed pack as the build before it
/// shared the listing. The first prompt lists the pack from the writer;
/// the later ones read the listing kept on disk and its token sidecar.
#[test]
fn the_prompt_hook_shows_the_same_lines_for_a_fixed_pack() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let seat = seat();
    fixed_pack(&seat);
    let prompts = [prompt_for(14), two_lessons_prompt(), prompt_for(14)];
    for (k, prompt) in prompts.iter().enumerate() {
        let said = seat.prompt(&format!("same-{k}"), prompt);
        assert_eq!(
            String::from_utf8_lossy(&said.stdout),
            SAID[k],
            "prompt {k} {prompt:?} printed other lines: {}",
            text(&said)
        );
    }
}

/// Lessons that share `case` with every prompt here: more than the hook's
/// window of twenty, so the writer's cut comes back full and the hook
/// adds its own pass over the stored claims.
const SHARING: usize = 30;

/// The larger pack is this many times the smaller, the sharing lessons
/// alone, in notes. A stage that reads every claim reads this many times
/// as many. Ten is the step between the packs of `prompt-hook.scale-*`.
const GROWTH: usize = 10;

/// The stored claims the rerank stage walked, from the hook's stage clock.
fn rerank_walked(said: &Output) -> usize {
    let stderr = String::from_utf8_lossy(&said.stderr);
    stderr
        .lines()
        .find_map(|line| {
            line.strip_prefix("hook-timing rerank ")?
                .strip_suffix(" claims walked")?
                .parse()
                .ok()
        })
        .unwrap_or_else(|| panic!("no rerank count on the hook's stderr: {stderr}"))
}

/// The rerank stage's counts for a cold prompt and a warm one, on a pack
/// of `n` claims with notes added after the sharing lessons.
fn rerank_walked_at(seat: &Seat, n: usize) -> (usize, usize) {
    for i in seat.live()..n {
        seat.post(i, &note(i), 1, None);
    }
    assert_eq!(seat.live(), n, "a note replaced another");
    let prompt = prompt_for(7);
    let cold = rerank_walked(&seat.prompt(&format!("cold-{n}"), &prompt));
    let warm = rerank_walked(&seat.prompt(&format!("warm-{n}"), &prompt));
    eprintln!("rerank walked {cold} claims cold and {warm} warm at {n} claims");
    (cold, warm)
}

/// The reranked search reads the hits the writer sent and the pass its
/// prompt's search already made, not every stored claim again. The
/// stage's count stays where it was when the pack grows tenfold in
/// claims that share no word with the prompt.
#[test]
fn the_rerank_walks_no_more_claims_when_the_pack_grows_in_claims_the_prompt_does_not_share() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let seat = seat();
    for i in 0..SHARING {
        seat.post(i, &lesson(i), 1, None);
    }
    let small = rerank_walked_at(&seat, SHARING);
    let large = rerank_walked_at(&seat, SHARING * GROWTH);
    assert_eq!(
        small,
        large,
        "the rerank stage walked (cold, warm) {small:?} claims at {SHARING} and {large:?} at {}",
        SHARING * GROWTH
    );
}
