//! Grok Bot commits are booked by git hooks, without a prompt hook.
//! The directory is outside the temp roots, not /tmp: ljos skips a temp seat.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "common/seat_root.rs"]
mod seat_root;

fn work_root() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let path = seat_root::seat_root(ljos_cli::bookkeep::path_is_temp)
        .join("grokbot-books-tests")
        .join(format!(
            "{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&path).unwrap();
    path
}

struct Hab {
    root: PathBuf,
    tracker: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    home: PathBuf,
    run: PathBuf,
    config: PathBuf,
    share: PathBuf,
    gitconfig: PathBuf,
    project: Option<String>,
}

impl Hab {
    fn new(name: &str) -> Self {
        let root = work_root();
        let hab = Self {
            tracker: root.join("tracker"),
            repo: root.join(name),
            bin: root.join("bin"),
            home: root.join("home"),
            run: root.join("run"),
            config: root.join("config"),
            share: root.join("share"),
            gitconfig: root.join("gitconfig"),
            project: Some("probe".into()),
            root,
        };
        fs::create_dir_all(&hab.tracker).unwrap();
        fs::create_dir_all(&hab.repo).unwrap();
        fs::create_dir_all(&hab.home).unwrap();
        fs::create_dir_all(&hab.run).unwrap();
        fs::create_dir_all(&hab.config).unwrap();
        habitat_bin(&hab.bin);
        for dir in [&hab.tracker, &hab.repo] {
            git_bare(dir, &["init", "-q", "-b", "main"]);
            git_bare(dir, &["config", "user.name", "Rohit Goswami"]);
            git_bare(dir, &["config", "user.email", "rgoswami@ieee.org"]);
            git_bare(dir, &["config", "commit.gpgsign", "false"]);
        }
        fs::write(hab.repo.join("README.md"), "seed\n").unwrap();
        git_bare(&hab.repo, &["add", "README.md"]);
        git_bare(&hab.repo, &["commit", "-q", "-m", "seed"]);
        hab
    }

    fn ljos(&self, args: &[&str]) -> Output {
        self.ljos_in(&self.repo, args, None, &[])
    }

    fn ljos_in(
        &self,
        cwd: &Path,
        args: &[&str],
        stdin: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(cwd)
            .env_clear()
            .envs(self.base_env())
            .stdin(std::process::Stdio::piped());
        for (key, value) in extra {
            cmd.env(key, value);
        }
        let mut child = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(text) = stdin {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "ljos {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        // A hook's book is applied after its answer; what the test reads
        // next is the book after it.
        if args.first() == Some(&"hook") && !args.contains(&"--drain") {
            self.ljos_in(cwd, &["hook", "--drain"], None, extra);
        }
        out
    }

    fn git(&self, dir: &Path, args: &[&str]) -> Output {
        let out = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .current_dir(dir)
            .env_clear()
            .envs(self.base_env())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn base_env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            (
                "PATH".into(),
                format!("{}:/usr/bin:/bin", self.bin.display()),
            ),
            ("HOME".into(), self.home.display().to_string()),
            ("XDG_CONFIG_HOME".into(), self.config.display().to_string()),
            ("XDG_DATA_HOME".into(), self.share.display().to_string()),
            ("XDG_RUNTIME_DIR".into(), self.run.display().to_string()),
            (
                "XDG_STATE_HOME".into(),
                self.root.join("state").display().to_string(),
            ),
            (
                "GIT_CONFIG_GLOBAL".into(),
                self.gitconfig.display().to_string(),
            ),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("VISSUE_ROOT".into(), self.tracker.display().to_string()),
            ("VISSUE_NO_ROUTE".into(), "1".into()),
            ("LJOS_TRACKER_GIT".into(), "commit".into()),
            ("LJOS_USER_RULES_SYNC".into(), "off".into()),
            ("PACKSET_URL".into(), "off".into()),
            ("LJOS_SEAT".into(), "grokbot".into()),
            (
                "DEEDAR_URL".into(),
                format!("file://{}/deedar-store", self.root.display()),
            ),
            ("DEEDAR_HOST_SIGNING_KEY".into(), "off".into()),
        ];
        if let Some(project) = &self.project {
            env.push(("LJOS_PROJECT".into(), project.clone()));
        }
        env
    }
}

impl Drop for Hab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn git_bare(dir: &Path, args: &[&str]) {
    let out = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn habitat_bin(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("vissue"),
        r#"#!/usr/bin/env python3
import os, sys, pathlib, re
root = pathlib.Path(os.environ["VISSUE_ROOT"])
log = root / "vissue.log"
args = sys.argv[1:]
with log.open("a") as fh:
    fh.write(" ".join(args) + " VISSUE_AGENT=" + os.environ.get("VISSUE_AGENT", "") + "\n")
cmd, rest = args[0], args[1:]

def files():
    return list(root.rglob("issues.org"))

def rewrite(iid, state):
    pat = re.compile(r"(\* )[A-Z]+( \[#[A-Z]\] [^\n]*\n:PROPERTIES:\n:ID:\s+" + re.escape(iid) + r"\n)")
    for f in files():
        text = f.read_text()
        new, n = pat.subn(r"\1" + state + r"\2", text, count=1)
        if n:
            f.write_text(new)
            return
    sys.exit("no heading for " + iid)

if cmd == "create":
    proj, title, i = "agent", "agent work", 0
    while i < len(rest):
        if rest[i] == "-p" and i + 1 < len(rest):
            proj = rest[i + 1]
            i += 2
            continue
        if rest[i] == "-q":
            i += 1
            continue
        title = rest[i]
        i += 1
    seq = root / ".seq"
    n = int(seq.read_text()) + 1 if seq.exists() else 1
    seq.write_text(str(n))
    iid = f"{proj}-{n:04x}"
    dest = root / "Software" / proj
    dest.mkdir(parents=True, exist_ok=True)
    with (dest / "issues.org").open("a") as fh:
        fh.write(f"* TODO [#C] {title}\n:PROPERTIES:\n:ID:         {iid}\n:END:\n")
    print(iid)
elif cmd == "claim":
    rewrite(rest[0], "STARTED")
elif cmd == "update":
    state = rest[rest.index("-s") + 1]
    rewrite(rest[0], state)
elif cmd == "deed":
    iid = rest[0]
    if "--remove" in rest:
        acc = rest[rest.index("--remove") + 1]
        line = f"cited {acc}\n"
        for f in files():
            text = f.read_text()
            if f":ID:         {iid}\n" in text:
                f.write_text(text.replace(line, "", 1))
                sys.exit(0)
        sys.exit(0)
    acc = rest[rest.index("--add") + 1]
    needle = f":ID:         {iid}\n"
    for f in files():
        text = f.read_text()
        at = text.find(needle)
        if at >= 0:
            end = text.index(":END:\n", at) + len(":END:\n")
            f.write_text(text[:end] + f"cited {acc}\n" + text[end:])
            sys.exit(0)
    sys.exit("no issue " + iid)
elif cmd == "note":
    pass
elif cmd == "claims":
    print("[]")
elif cmd == "show":
    print("{}")
else:
    sys.exit("unknown " + cmd)
"#,
    )
    .unwrap();
    fs::write(
        dir.join("deedar"),
        r#"#!/usr/bin/env python3
import os, pathlib, sys
root = pathlib.Path(os.environ["VISSUE_ROOT"])
args = sys.argv[1:]
(root / "deedar.log").open("a").write(
    " ".join(args) + " VISSUE_AGENT=" + os.environ.get("VISSUE_AGENT", "") + "\n"
)
ident = None
i = 0
while i < len(args):
    if args[i] == "--id" and i + 1 < len(args):
        ident = args[i + 1]
    i += 1
if not ident:
    seq = root / ".deedseq"
    n = int(seq.read_text()) + 1 if seq.exists() else 1
    seq.write_text(str(n))
    ident = f"deed-{n:04x}"
print(f"id={ident}")
"#,
    )
    .unwrap();
    fs::write(dir.join("ljos"), "").unwrap();
    fs::remove_file(dir.join("ljos")).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos"), dir.join("ljos")).unwrap();
    for name in ["vissue", "deedar"] {
        fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn head(hab: &Hab, dir: &Path) -> String {
    let out = hab.git(dir, &["rev-parse", "HEAD"]);
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn commit_file(hab: &Hab, name: &str, message: &str) -> String {
    fs::write(hab.repo.join(name), "x\n").unwrap();
    hab.git(&hab.repo, &["add", name]);
    hab.git(&hab.repo, &["commit", "-q", "-m", message]);
    head(hab, &hab.repo)
}

fn deeds(tracker: &Path) -> usize {
    fs::read_to_string(tracker.join("deedar.log"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("create"))
        .count()
}

fn vissue_log(tracker: &Path) -> String {
    fs::read_to_string(tracker.join("vissue.log")).unwrap_or_default()
}

fn book_files(hab: &Hab) -> Vec<PathBuf> {
    let dir = hab.run.join("ljos");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("book-") && name.ends_with(".json"))
        })
        .collect()
}

fn install(hab: &Hab) {
    hab.ljos(&["git-hooks", "install"]);
}

/// A sitting on `id` for `holder` of `seat`: an open ticket the tracker
/// has claimed by `holder`, and a hold whose conversation is this test,
/// the ancestor of every hook its commits run. A runner's commit waits
/// for one.
fn sit(hab: &Hab, id: &str, holder: &str, seat: &str) {
    let project = hab.tracker.join("Software/probe");
    fs::create_dir_all(&project).unwrap();
    let issues = project.join("issues.org");
    let mut text = fs::read_to_string(&issues).unwrap_or_default();
    text.push_str(&format!(
        "* STARTED [#C] held work\n:PROPERTIES:\n:ID:         {id}\n:CLAIMED_BY: {holder}\n:END:\n"
    ));
    fs::write(&issues, text).unwrap();
    let comm = fs::read_to_string("/proc/self/comm").unwrap();
    fs::create_dir_all(hab.run.join("ljos")).unwrap();
    fs::write(
        hab.run.join("ljos").join(format!("hold-{id}")),
        format!(
            "{holder}\n{seat}\n{}\n{}\n2026-01-01T00:00:00Z\n{id}\n-\n",
            std::process::id(),
            comm.trim()
        ),
    )
    .unwrap();
}

/// A PostToolUse that also carries `prompt` still cites the commit.
#[test]
fn a_prompt_field_does_not_hide_the_commit() {
    let hab = Hab::new("repo");
    let sha = commit_file(&hab, "noted.txt", "readme");
    let payload = format!(
        r#"{{"hook_event_name":"PostToolUse","session_id":"seat:grokbot","cwd":"{}","prompt":"please commit the readme","tool_name":"Bash","tool_input":{{"command":"git commit -m readme"}},"tool_response":{{"exit_code":0}}}}"#,
        hab.repo.display()
    );
    hab.ljos_in(&hab.repo, &["hook"], Some(&payload), &[]);
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(
        org.contains(&format!("deed-file-commit-{sha}")),
        "HEAD was stored as the baseline and never cited:\n{org}"
    );
    assert!(
        vissue_log(&hab.tracker).contains("create"),
        "{}",
        vissue_log(&hab.tracker)
    );
    assert!(
        vissue_log(&hab.tracker).contains("claim"),
        "{}",
        vissue_log(&hab.tracker)
    );
}

/// A harness session books its commit through the hook payload, and the
/// git hook steps aside. A commit that names an open issue claims and cites
/// that issue there too, through the same function the git hook uses.
#[test]
fn a_harness_commit_naming_an_open_issue_books_that_issue() {
    for (message, command) in [
        (
            "probe-32fx: book the harness commit",
            "git commit -m 'probe-32fx: book the harness commit'".to_string(),
        ),
        (
            "tidy the harness commit\n\nFixes probe-32fx.",
            "git commit -m \"$(cat <<'EOF'\ntidy the harness commit\n\nFixes probe-32fx.\nEOF\n)\""
                .to_string(),
        ),
    ] {
        let hab = Hab::new("repo");
        seed_issue(&hab, "probe-32fx", "TODO");
        let payload = |event: &str| {
            serde_json::json!({
                "hook_event_name": event,
                "session_id": "claude-session-1",
                "cwd": hab.repo.display().to_string(),
                "tool_name": "Bash",
                "tool_input": {"command": command},
                "tool_response": {"exit_code": 0},
            })
            .to_string()
        };
        hab.ljos_in(&hab.repo, &["hook"], Some(&payload("PreToolUse")), &[]);
        let sha = commit_file(&hab, "harness.txt", message);
        hab.ljos_in(&hab.repo, &["hook"], Some(&payload("PostToolUse")), &[]);
        let log = vissue_log(&hab.tracker);
        assert!(
            !log.contains("create"),
            "{message:?} filed a new issue:\n{log}"
        );
        assert!(log.contains("claim probe-32fx"), "{message:?}:\n{log}");
        let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
        assert!(
            org.contains(&format!("deed-file-commit-{sha}")),
            "{message:?}:\n{org}"
        );
    }
}

/// A note written while the pack is off keeps its memory copy in the
/// queue, once however often the same note is written.
#[test]
fn a_note_with_the_pack_off_waits_in_the_queue_once() {
    let hab = Hab::new("repo");
    let said: Vec<String> = (0..2)
        .map(|_| {
            let out = hab.ljos_in(
                &hab.repo,
                &["note", "probe-32fx", "the pack was off for this note"],
                None,
                &[],
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        })
        .collect();
    assert!(said[0].contains("memory: queued the copy"), "{}", said[0]);
    assert!(said[1].contains("already queued"), "{}", said[1]);
    let queue = fs::read_to_string(hab.root.join("state/ljos/note-queue.jsonl")).unwrap();
    assert_eq!(queue.lines().count(), 1, "{queue}");
    assert!(queue.contains("\"issue\":\"probe-32fx\""), "{queue}");
}

#[test]
fn a_tool_hook_does_not_replace_owner_pid() {
    let hab = Hab::new("repo");
    let payload = format!(
        r#"{{"hook_event_name":"PreToolUse","session_id":"seat:grokbot","cwd":"{}","tool_name":"Write","tool_input":{{"file_path":"README.md"}}}}"#,
        hab.repo.display()
    );
    hab.ljos_in(&hab.repo, &["hook"], Some(&payload), &[]);
    let books = book_files(&hab);
    assert_eq!(books.len(), 1, "{books:?}");
    let mut book: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&books[0]).unwrap()).unwrap();
    book["owner_pid"] = serde_json::json!(4242);
    fs::write(&books[0], serde_json::to_string(&book).unwrap()).unwrap();
    let post = format!(
        r#"{{"hook_event_name":"PostToolUse","session_id":"seat:grokbot","cwd":"{}","tool_name":"Bash","tool_input":{{"command":"true"}}}}"#,
        hab.repo.display()
    );
    hab.ljos_in(&hab.repo, &["hook"], Some(&post), &[]);
    let book: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&books[0]).unwrap()).unwrap();
    assert_eq!(book["owner_pid"], 4242, "{book}");
    let start = format!(
        r#"{{"hook_event_name":"SessionStart","session_id":"seat:grokbot","cwd":"{}"}}"#,
        hab.repo.display()
    );
    hab.ljos_in(&hab.repo, &["hook"], Some(&start), &[]);
    let book: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&books[0]).unwrap()).unwrap();
    assert_ne!(book["owner_pid"], 4242, "{book}");
    assert_ne!(book["owner_pid"], 0, "{book}");
}

#[test]
fn an_idle_sitting_is_finished_and_loses_its_prompt() {
    let hab = Hab::new("repo");
    let payload = format!(
        r#"{{"hook_event_name":"PreToolUse","session_id":"seat:grokbot","cwd":"{}","prompt":"old title about parsers","tool_name":"Write","tool_input":{{"file_path":"README.md"}}}}"#,
        hab.repo.display()
    );
    hab.ljos_in(&hab.repo, &["hook"], Some(&payload), &[]);
    let books = book_files(&hab);
    assert_eq!(books.len(), 1, "{books:?}");
    let mut book: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&books[0]).unwrap()).unwrap();
    book["last_unix"] = serde_json::json!(10);
    book["prompt"] = serde_json::json!("old title about parsers");
    book["prompt_unix"] = serde_json::json!(10);
    if let Some(repos) = book
        .get_mut("repos")
        .and_then(|value| value.as_object_mut())
    {
        for row in repos.values_mut() {
            row["last_unix"] = serde_json::json!(10);
        }
    }
    fs::write(&books[0], serde_json::to_string(&book).unwrap()).unwrap();
    let post = format!(
        r#"{{"hook_event_name":"PostToolUse","session_id":"seat:grokbot","cwd":"{}","tool_name":"Bash","tool_input":{{"command":"true"}}}}"#,
        hab.repo.display()
    );
    hab.ljos_in(&hab.repo, &["hook"], Some(&post), &[]);
    let log = vissue_log(&hab.tracker);
    assert!(log.contains("DONE"), "{log}");
    let book: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&books[0]).unwrap()).unwrap();
    let prompt = book["prompt"].as_str().unwrap_or("");
    assert!(
        !prompt.contains("old title about parsers"),
        "the next sitting inherited the title: {book}"
    );
}

/// The seat's book still holds a prompt from another repository. The commit
/// in this one is claimed under the commit subject.
#[test]
fn a_stale_prompt_for_another_repo_does_not_title_the_commit() {
    let hab = Hab::new("repo");
    let other = hab.root.join("scratch-notes");
    fs::create_dir_all(&other).unwrap();
    git_bare(&other, &["init", "-q", "-b", "main"]);
    let payload = format!(
        r#"{{"hook_event_name":"UserPromptSubmit","session_id":"seat:grokbot","cwd":"{}","prompt":"notes: add a second line"}}"#,
        other.display()
    );
    hab.ljos_in(&other, &["hook"], Some(&payload), &[]);
    install(&hab);
    commit_file(&hab, "second.txt", "record the second reading");
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(
        org.contains("* STARTED [#C] record the second reading\n"),
        "the claim did not take the commit subject:\n{org}"
    );
    assert!(
        !org.contains("notes: add a second line"),
        "the other repository's prompt titled the claim:\n{org}"
    );
}

/// A prompt recorded for this checkout, in a sitting that is still open,
/// still names the claim. The commit subject does not replace it.
#[test]
fn a_live_prompt_in_this_repo_still_titles_the_commit() {
    let hab = Hab::new("repo");
    let payload = format!(
        r#"{{"hook_event_name":"UserPromptSubmit","session_id":"seat:grokbot","cwd":"{}","prompt":"retitle the readme"}}"#,
        hab.repo.display()
    );
    hab.ljos_in(&hab.repo, &["hook"], Some(&payload), &[]);
    install(&hab);
    commit_file(&hab, "titled.txt", "unrelated subject line");
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(
        org.contains("* STARTED [#C] retitle the readme\n"),
        "the current sitting's prompt lost to the commit subject:\n{org}"
    );
    assert!(
        !org.contains("unrelated subject line"),
        "the commit subject replaced the live prompt:\n{org}"
    );
}

/// Repo A's row is hours old. The book's clock is fresh because this commit
/// is in repo B. A's claim is finished.
#[test]
fn a_commit_in_one_repo_finishes_an_idle_row_in_another() {
    let hab = Hab::new("repo-b");
    let repo_a = hab.root.join("repo-a");
    fs::create_dir_all(&repo_a).unwrap();
    git_bare(&repo_a, &["init", "-q", "-b", "main"]);
    let top_a = String::from_utf8(hab.git(&repo_a, &["rev-parse", "--show-toplevel"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let book_path = hab.run.join("ljos").join(format!(
        "book-{}.json",
        ljos_cli::session_tag("seat:grokbot")
    ));
    fs::create_dir_all(book_path.parent().unwrap()).unwrap();
    let mut repos = serde_json::Map::new();
    repos.insert(
        top_a.clone(),
        serde_json::json!({
            "issue": "probe-a11e",
            "title": "notes: add a second line",
            "finished": false,
            "last_unix": 10
        }),
    );
    let book = serde_json::json!({
        "prompt": "notes: add a second line",
        "prompt_repo": top_a.clone(),
        "prompt_unix": 10,
        "last_unix": now,
        "repos": repos
    });
    fs::write(&book_path, serde_json::to_string(&book).unwrap()).unwrap();
    fs::create_dir_all(hab.tracker.join("Software/probe")).unwrap();
    fs::write(
        hab.tracker.join("Software/probe/issues.org"),
        "* TODO [#C] notes: add a second line\n:PROPERTIES:\n:ID:         probe-a11e\n:END:\n",
    )
    .unwrap();
    install(&hab);
    commit_file(&hab, "fresh.txt", "record the second reading");
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(
        org.contains("* DONE [#C] notes: add a second line\n"),
        "repo A's row was still open:\n{org}"
    );
    let log = vissue_log(&hab.tracker);
    assert!(
        log.contains("update probe-a11e -s DONE"),
        "repo A was not finished:\n{log}"
    );
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&book_path).unwrap()).unwrap();
    let still_open = saved["repos"]
        .as_object()
        .into_iter()
        .flat_map(|repos| repos.iter())
        .any(|(key, row)| {
            key == &top_a
                && row["issue"].as_str() == Some("probe-a11e")
                && row["finished"].as_bool() != Some(true)
        });
    assert!(!still_open, "repo A's row is still unfinished: {saved}");
}

#[test]
fn git_hooks_book_a_commit_once_and_uninstall_restores_the_config() {
    let hab = Hab::new("repo");
    let saved = hab.root.join("saved-hooks");
    fs::create_dir_all(&saved).unwrap();
    let marker = hab.root.join("repo-hook");
    let user = format!("#!/bin/sh\nprintf ran >> {}\n", shellish(&marker));
    fs::write(saved.join("post-commit"), &user).unwrap();
    fs::set_permissions(saved.join("post-commit"), fs::Permissions::from_mode(0o755)).unwrap();
    hab.git(
        &hab.repo,
        &[
            "config",
            "--global",
            "core.hooksPath",
            saved.to_str().unwrap(),
        ],
    );
    install(&hab);
    install(&hab);
    let configured = String::from_utf8(
        hab.git(
            &hab.repo,
            &["config", "--global", "--get", "core.hooksPath"],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(
        configured.trim(),
        saved.display().to_string(),
        "a hooksPath the user set was replaced: {configured}"
    );
    let hooked = fs::read_to_string(saved.join("post-commit")).unwrap();
    assert!(hooked.starts_with(&user), "{hooked}");
    assert_eq!(
        hooked.matches("# ljos:begin git-hook").count(),
        1,
        "{hooked}"
    );
    assert!(hooked.contains("ljos git-hook post-commit "), "{hooked}");
    assert!(saved.join("pre-push").is_file());
    let status = String::from_utf8(hab.ljos(&["git-hooks", "status"]).stdout).unwrap();
    assert!(status.contains("ljos hooks run"), "{status}");

    fs::write(hab.repo.join("one.txt"), "x\n").unwrap();
    hab.git(&hab.repo, &["add", "one.txt"]);
    let committed = hab.git(&hab.repo, &["commit", "-q", "-m", "book the readme"]);
    let sha = head(&hab, &hab.repo);
    let err = String::from_utf8_lossy(&committed.stderr);
    assert!(
        err.contains("recall: book the readme"),
        "the commit hook printed no recall: {err}"
    );
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(org.contains("STARTED"), "{org}");
    assert!(org.contains(&format!("deed-file-commit-{sha}")), "{org}");
    assert!(
        vissue_log(&hab.tracker).contains("claim"),
        "{}",
        vissue_log(&hab.tracker)
    );
    let marked = fs::read_to_string(&marker).unwrap();
    assert!(
        marked.starts_with("ran"),
        "the user's hook did not run: {marked}"
    );
    assert_eq!(deeds(&hab.tracker), 1);

    let before = vissue_log(&hab.tracker);
    let out = Command::new("timeout")
        .args(["15", "git", "-C"])
        .arg(&hab.tracker)
        .args(["commit", "--allow-empty", "-q", "-m", "tracker"])
        .env_clear()
        .envs(hab.base_env())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        vissue_log(&hab.tracker),
        before,
        "the tracker hook booked itself:\n{}",
        vissue_log(&hab.tracker)
    );

    hab.git(
        &hab.repo,
        &["commit", "--amend", "-q", "-m", "book the readme again"],
    );
    let amended = head(&hab, &hab.repo);
    assert_ne!(amended, sha, "amend did not move HEAD");
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(
        org.contains(&format!("deed-file-commit-{amended}")),
        "{org}"
    );
    assert!(
        !org.contains(&format!("deed-file-commit-{sha}")),
        "the amended sha is still cited:\n{org}"
    );
    assert_eq!(deeds(&hab.tracker), 2, "an amend was booked more than once");

    hab.git(&hab.repo, &["checkout", "-q", "-b", "side"]);
    let side_a = commit_file(&hab, "a.txt", "side a");
    let side_b = commit_file(&hab, "b.txt", "side b");
    hab.git(&hab.repo, &["checkout", "-q", "main"]);
    commit_file(&hab, "c.txt", "main c");
    let before = deeds(&hab.tracker);
    hab.git(&hab.repo, &["checkout", "-q", "side"]);
    hab.git(&hab.repo, &["rebase", "main"]);
    let tip = head(&hab, &hab.repo);
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(
        org.contains(&format!("deed-file-commit-{tip}")),
        "the rebased tip is not cited:\n{org}"
    );
    assert!(
        org.contains(&format!("deed-file-commit-{amended}")),
        "the commit on main lost its citation:\n{org}"
    );
    assert!(
        !org.contains(&format!("deed-file-commit-{side_a}")),
        "a rewritten sha is still cited:\n{org}"
    );
    assert!(
        !org.contains(&format!("deed-file-commit-{side_b}")),
        "a rewritten sha is still cited:\n{org}"
    );
    assert_eq!(
        deeds(&hab.tracker),
        before + 2,
        "each rewritten commit is cited once"
    );

    hab.ljos(&["git-hooks", "uninstall"]);
    let restored = String::from_utf8(
        hab.git(
            &hab.repo,
            &["config", "--global", "--get", "core.hooksPath"],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(restored.trim(), saved.display().to_string());
    assert_eq!(fs::read_to_string(saved.join("post-commit")).unwrap(), user);
    assert!(!saved.join("pre-push").exists());
    let before_marker = fs::read_to_string(&marker).unwrap();
    let deeds_before = deeds(&hab.tracker);
    fs::write(hab.repo.join("two.txt"), "y\n").unwrap();
    hab.git(&hab.repo, &["add", "two.txt"]);
    hab.git(&hab.repo, &["commit", "-q", "-m", "after uninstall"]);
    assert_eq!(
        fs::read_to_string(&marker).unwrap(),
        format!("{before_marker}ran"),
        "the user's hook stopped running"
    );
    assert_eq!(
        deeds(&hab.tracker),
        deeds_before,
        "uninstall left the booking hook in place"
    );
}

#[test]
fn owned_hooks_pass_post_rewrite_stdin_to_the_repo_hook() {
    let hab = Hab::new("repo");
    install(&hab);
    let sha = commit_file(&hab, "one.txt", "book the readme");
    let rewrite_in = hab.root.join("rewrite-stdin");
    let rewrite_hook = hab.repo.join(".git/hooks/post-rewrite");
    fs::write(
        &rewrite_hook,
        format!("#!/bin/sh\ncat > {}\n", shellish(&rewrite_in)),
    )
    .unwrap();
    fs::set_permissions(&rewrite_hook, fs::Permissions::from_mode(0o755)).unwrap();
    hab.git(
        &hab.repo,
        &["commit", "--amend", "-q", "-m", "book the readme again"],
    );
    let amended = head(&hab, &hab.repo);
    let seen = fs::read_to_string(&rewrite_in).unwrap_or_default();
    assert!(
        seen.contains(&sha) && seen.contains(&amended),
        "the chained post-rewrite lost stdin: {seen}"
    );
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(
        org.contains(&format!("deed-file-commit-{amended}")),
        "{org}"
    );
    assert!(
        !org.contains(&format!("deed-file-commit-{sha}")),
        "the amended sha is still cited:\n{org}"
    );
}

#[test]
fn uninstall_restores_a_hooks_path_ljos_already_owns() {
    let hab = Hab::new("repo");
    let saved = hab.root.join("saved-hooks");
    fs::create_dir_all(&saved).unwrap();
    install(&hab);
    let ours = hab.share.join("ljos/git-hooks");
    let configured = String::from_utf8(
        hab.git(
            &hab.repo,
            &["config", "--global", "--get", "core.hooksPath"],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(
        configured.trim(),
        ours.display().to_string(),
        "{configured}"
    );
    fs::write(
        ours.join("previous-hooks-path"),
        format!("{}\n", saved.display()),
    )
    .unwrap();
    hab.ljos(&["git-hooks", "uninstall"]);
    let restored = String::from_utf8(
        hab.git(
            &hab.repo,
            &["config", "--global", "--get", "core.hooksPath"],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(restored.trim(), saved.display().to_string());
    assert!(!ours.join("post-commit").exists());
}

#[test]
fn onboard_installs_the_git_hooks() {
    let hab = Hab::new("repo");
    let skills = hab.root.join("skills");
    hab.ljos(&[
        "onboard",
        "--harness",
        "grokbot",
        "--skills",
        skills.to_str().unwrap(),
    ]);
    let configured = String::from_utf8(
        hab.git(
            &hab.repo,
            &["config", "--global", "--get", "core.hooksPath"],
        )
        .stdout,
    )
    .unwrap();
    assert!(configured.contains("git-hooks"), "{configured}");
    let status = hab.ljos(&["git-hooks", "status"]);
    let text = String::from_utf8(status.stdout).unwrap();
    assert!(text.contains("core.hooksPath:"), "{text}");
}

#[test]
fn a_commit_lands_on_the_project_named_by_its_remote() {
    let mut hab = Hab::new("workspace");
    hab.project = None;
    fs::create_dir_all(hab.tracker.join("Software/ljos")).unwrap();
    fs::create_dir_all(hab.tracker.join("Software/other")).unwrap();
    fs::write(
        hab.tracker.join("Software/ljos/issues.org"),
        "#+REMOTE: https://github.com/acme/ljos.git\n",
    )
    .unwrap();
    fs::write(
        hab.tracker.join("Software/other/issues.org"),
        "* TODO other\n",
    )
    .unwrap();
    git_bare(
        &hab.repo,
        &["remote", "add", "origin", "git@github.com:acme/ljos.git"],
    );
    install(&hab);
    commit_file(&hab, "remote.txt", "match the remote");
    assert!(hab.tracker.join("Software/ljos/issues.org").is_file());
    let org = fs::read_to_string(hab.tracker.join("Software/ljos/issues.org")).unwrap();
    assert!(org.contains("STARTED"), "{org}");
    assert!(
        !hab.tracker.join("Software/workspace").exists(),
        "the claim was filed under the cwd basename"
    );
}

#[test]
fn the_guard_prints_the_session_brief_once() {
    let hab = Hab::new("repo");
    let skills = hab.root.join("skills");
    hab.ljos(&[
        "onboard",
        "--harness",
        "grokbot",
        "--skills",
        skills.to_str().unwrap(),
    ]);
    let guard = hab.share.join("ljos/guard-bin");
    assert!(
        guard.join("git").is_file(),
        "onboard did not write the git shim"
    );
    let path = format!("{}:{}:/usr/bin:/bin", guard.display(), hab.bin.display());
    let run = |id: &str| {
        Command::new(guard.join("git"))
            .arg("status")
            .current_dir(&hab.repo)
            .env_clear()
            .envs(hab.base_env())
            .env("PATH", &path)
            .env("CURSOR_CONVERSATION_ID", id)
            .output()
            .unwrap()
    };
    let first = run("conv-once-0001");
    let second = run("conv-once-0001");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&first.stderr),
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(first.status.success(), "{err}");
    assert!(second.status.success(), "{err}");
    assert_eq!(err.matches("claims:").count(), 1, "{err}");
    let other = run("conv-other-0002");
    let again = String::from_utf8_lossy(&other.stderr);
    assert!(again.contains("claims:"), "{again}");
}

#[test]
fn ljos_seat_reads_seats_toml_and_never_prints_a_raw_id() {
    let hab = Hab::new("repo");
    let id = "bc-1234abcd-rest";
    let dir = hab.config.join("ljos");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("seats.toml"), format!("{id:?} = \"rgoswami\"\n")).unwrap();
    let known = hab.ljos_in(
        &hab.repo,
        &["seat"],
        None,
        &[("LJOS_SEAT", ""), ("CURSOR_CONVERSATION_ID", id)],
    );
    assert_eq!(String::from_utf8(known.stdout).unwrap().trim(), "rgoswami");
    let unknown_id = "zzzzzzzz-hello";
    let unknown = hab.ljos_in(
        &hab.repo,
        &["seat"],
        None,
        &[("LJOS_SEAT", ""), ("CURSOR_CONVERSATION_ID", unknown_id)],
    );
    let name = String::from_utf8(unknown.stdout).unwrap();
    assert_eq!(name.trim(), "grokbot-zzzzzzzz");
    assert_ne!(name.trim(), unknown_id);
}

/// `LJOS_SEAT` names the seat for `ljos seat` and `ljos doctor` alike.
/// A conversation id does not replace it with `grokbot-<id8>`.
#[test]
fn ljos_seat_and_doctor_keep_an_exported_seat() {
    let hab = Hab::new("repo");
    let extra = [
        ("LJOS_SEAT", "ljosbot"),
        ("CURSOR_CONVERSATION_ID", "sand-subagent-001"),
    ];
    let seat = hab.ljos_in(&hab.repo, &["seat"], None, &extra);
    let said = String::from_utf8(seat.stdout).unwrap();
    assert!(said.contains("ljosbot"), "{said}");
    assert!(said.contains("LJOS_SEAT"), "{said}");
    assert!(!said.contains("grokbot-sand-sub"), "{said}");
    let doctor = Command::new("timeout")
        .args(["90", env!("CARGO_BIN_EXE_ljos"), "doctor", "--all"])
        .current_dir(&hab.repo)
        .env_clear()
        .envs(hab.base_env())
        .envs(extra)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&doctor.stdout);
    let row = text
        .lines()
        .find(|line| line.split('\t').nth(1) == Some("seat"))
        .unwrap_or("");
    assert!(row.contains("ljosbot"), "{text}");
    assert!(row.contains("LJOS_SEAT"), "{row}");
    assert!(!row.contains("grokbot-sand-sub"), "{row}");
    assert_eq!(
        said.lines()
            .find_map(|line| line.strip_prefix("source\t"))
            .unwrap_or(""),
        row.split(" (from ")
            .nth(1)
            .unwrap_or("")
            .trim_end_matches(')'),
        "seat and doctor name different sources:\n{said}\n{row}"
    );
}

/// A project whose name contains a hyphen or digits is created, claimed
/// and cited. The id is the project plus the four-character suffix vissue mints.
#[test]
fn a_hyphenated_project_is_created_claimed_and_cited() {
    for project in ["ljos-policyd", "lgio-1792329616"] {
        let mut hab = Hab::new("repo");
        hab.project = Some(project.to_string());
        install(&hab);
        let sha = commit_file(&hab, "noted.txt", "book the hyphenated project");
        let org = fs::read_to_string(hab.tracker.join(format!("Software/{project}/issues.org")))
            .unwrap_or_default();
        assert!(org.contains("STARTED"), "{project}:\n{org}");
        assert!(
            org.contains(&format!("deed-file-commit-{sha}")),
            "{project}:\n{org}"
        );
        let log = vissue_log(&hab.tracker);
        assert!(log.contains("create"), "{project}:\n{log}");
        assert!(log.contains("claim"), "{project}:\n{log}");
        let books = book_files(&hab);
        assert_eq!(books.len(), 1, "{project}: {books:?}");
        let book: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&books[0]).unwrap()).unwrap();
        let row = book["repos"]
            .as_object()
            .and_then(|repos| repos.values().next())
            .unwrap_or(&serde_json::Value::Null);
        assert_eq!(row["claim_failed"], false, "{project}: {book}");
        let issue = row["issue"].as_str().unwrap_or("");
        let suffix = issue
            .strip_prefix(&format!("{project}-"))
            .unwrap_or_else(|| panic!("{project}: issue {issue} in {book}"));
        assert_eq!(suffix.len(), 4, "{issue}");
        assert!(
            suffix
                .bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()),
            "{issue}"
        );
        assert!(
            row["deeds"]
                .as_array()
                .is_some_and(|deeds| !deeds.is_empty()),
            "{project}: {book}"
        );
    }
}

/// An issue already in the tracker, in `state`, under project `probe`.
fn seed_issue(hab: &Hab, id: &str, state: &str) {
    let dir = hab.tracker.join("Software/probe");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("issues.org");
    let mut org = fs::read_to_string(&file).unwrap_or_else(|_| {
        "#+TITLE: probe issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n".to_string()
    });
    org.push_str(&format!(
        "* {state} [#C] seeded {id}\n:PROPERTIES:\n:ID:         {id}\n:END:\n"
    ));
    fs::write(&file, org).unwrap();
}

/// A commit whose subject or body names an open issue is claimed and cited
/// on that issue. Nothing new is filed.
#[test]
fn a_commit_naming_an_open_issue_books_that_issue() {
    for (message, id) in [
        ("probe-32fx: book the named issue", "probe-32fx"),
        ("tidy the hook\n\nFixes probe-xsor.", "probe-xsor"),
        (
            "ljos-policyd-c0jc: a hyphenated project",
            "ljos-policyd-c0jc",
        ),
    ] {
        let hab = Hab::new("repo");
        if id.starts_with("ljos-policyd-") {
            let dir = hab.tracker.join("Software/ljos-policyd");
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("issues.org"),
                format!(
                    "#+TITLE: ljos-policyd issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
                     * TODO [#C] seeded {id}\n:PROPERTIES:\n:ID:         {id}\n:END:\n"
                ),
            )
            .unwrap();
        } else {
            seed_issue(&hab, id, "TODO");
        }
        install(&hab);
        let sha = commit_file(&hab, "named.txt", message);
        let log = vissue_log(&hab.tracker);
        assert!(
            !log.contains("create"),
            "{message:?} filed a new issue:\n{log}"
        );
        assert!(log.contains(&format!("claim {id}")), "{message:?}:\n{log}");
        let org: String = ["probe", "ljos-policyd"]
            .iter()
            .map(|p| {
                fs::read_to_string(hab.tracker.join(format!("Software/{p}/issues.org")))
                    .unwrap_or_default()
            })
            .collect();
        assert!(
            org.contains(&format!("deed-file-commit-{sha}")),
            "{message:?}:\n{org}"
        );
        assert!(org.contains("* STARTED"), "{message:?}:\n{org}");
        let books = book_files(&hab);
        assert_eq!(books.len(), 1, "{books:?}");
        let book: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&books[0]).unwrap()).unwrap();
        let row = book["repos"]
            .as_object()
            .and_then(|repos| repos.values().next())
            .unwrap_or(&serde_json::Value::Null);
        assert_eq!(row["issue"], id, "{book}");
        assert_eq!(row["named"], true, "{book}");
    }
}

/// An id-shaped word the tracker does not hold, or an issue already closed,
/// is not a named issue: the commit files its own.
#[test]
fn a_commit_naming_no_open_issue_files_one() {
    let hab = Hab::new("repo");
    seed_issue(&hab, "probe-done", "DONE");
    install(&hab);
    let sha = commit_file(&hab, "unnamed.txt", "fix-hook: probe-done and probe-gone");
    let log = vissue_log(&hab.tracker);
    assert!(log.contains("create"), "{log}");
    assert!(!log.contains("claim probe-done"), "{log}");
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap();
    assert!(org.contains("* DONE [#C] seeded probe-done"), "{org}");
    assert!(org.contains(&format!("deed-file-commit-{sha}")), "{org}");
}

fn shellish(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn exec_daemon(dir: &Path) -> PathBuf {
    let src = dir.join("exec-daemon.c");
    let bin = dir.join("exec-daemon");
    fs::write(
        &src,
        "#include <unistd.h>\n\
         int main(int argc, char **argv) {\n\
         \tif (argc < 2) return 127;\n\
         \texecvp(argv[1], argv + 1);\n\
         \treturn 127;\n\
         }\n",
    )
    .unwrap();
    let out = Command::new("/usr/bin/cc")
        .args(["-O2", "-o"])
        .arg(&bin)
        .arg(&src)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "cc: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    bin
}

fn env_without_seat(hab: &Hab) -> Vec<(String, String)> {
    hab.base_env()
        .into_iter()
        .filter(|(key, _)| key != "LJOS_SEAT")
        .collect()
}

fn under_exec_daemon(hab: &Hab, extra: &[(&str, &str)]) -> Output {
    let daemon = exec_daemon(&hab.bin);
    let mut cmd = Command::new(&daemon);
    cmd.arg(env!("CARGO_BIN_EXE_ljos"))
        .args(["hook", "--git-commit"])
        .current_dir(&hab.repo)
        .env_clear()
        .envs(env_without_seat(hab));
    for (key, value) in extra {
        cmd.env(key, value);
    }
    cmd.output().unwrap()
}

/// The parent is `exec-daemon`. The claim, the deed and the tracker record
/// use the seat from `seats.toml`, then the `grokbot-<id8>` fallback, never
/// the process tree.
#[test]
fn a_git_hook_under_exec_daemon_books_the_seat() {
    let hab = Hab::new("repo");
    let id = "bc-1234abcd-rest";
    let dir = hab.config.join("ljos");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("seats.toml"), format!("{id:?} = \"rgoswami\"\n")).unwrap();
    let out = under_exec_daemon(&hab, &[("CURSOR_CONVERSATION_ID", id)]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let holder = format!("rgoswami@{}", ljos_cli::session_tag(id));
    let vlog = vissue_log(&hab.tracker);
    let dlog = fs::read_to_string(hab.tracker.join("deedar.log")).unwrap_or_default();
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap_or_default();
    assert!(
        vlog.contains(&format!("VISSUE_AGENT={holder}")),
        "the claim was not {holder}:\n{vlog}\n{err}"
    );
    assert!(
        dlog.contains(&format!("--agent {holder}")),
        "the deed was not {holder}:\n{dlog}"
    );
    assert!(
        org.contains("by rgoswami"),
        "the record was not the seat:\n{org}"
    );
    for (what, text) in [("claim", &vlog), ("deed", &dlog), ("record", &org)] {
        assert!(
            !text.contains("exec-daemon"),
            "the {what} used the process tree:\n{text}"
        );
    }

    let hab = Hab::new("repo");
    let unknown = "zzzzzzzz-hello";
    let out = under_exec_daemon(&hab, &[("CURSOR_CONVERSATION_ID", unknown)]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let holder = format!("grokbot-zzzzzzzz@{}", ljos_cli::session_tag(unknown));
    let vlog = vissue_log(&hab.tracker);
    let dlog = fs::read_to_string(hab.tracker.join("deedar.log")).unwrap_or_default();
    assert!(
        vlog.contains(&format!("VISSUE_AGENT={holder}")),
        "an unmapped conversation did not fall back to grokbot-<id8>@tag:\n{vlog}"
    );
    assert!(
        dlog.contains(&format!("--agent {holder}")),
        "the deed did not use the fallback:\n{dlog}"
    );
    assert!(!vlog.contains("exec-daemon"), "{vlog}");
    assert!(!dlog.contains("exec-daemon"), "{dlog}");
}

/// A subagent's conversation id is not a seat. Its first eight characters
/// named every subagent `grokbot-sand-sub`; the hook now books nothing and
/// says what to export.
#[test]
fn a_subagent_commit_is_not_booked_and_says_why() {
    let hab = Hab::new("repo");
    let out = under_exec_daemon(&hab, &[("CURSOR_CONVERSATION_ID", "sand-subagent-002")]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let said = String::from_utf8_lossy(&out.stdout);
    let vlog = vissue_log(&hab.tracker);
    assert!(!vlog.contains("grokbot-sand-sub"), "{vlog}");
    assert!(
        !vlog.contains("claim"),
        "a subagent commit claimed:\n{vlog}"
    );
    // The installed hook prints stdout and drops stderr.
    assert!(said.contains("not booked"), "{said}\n{err}");
    assert!(said.contains("LJOS_SEAT"), "{said}");
    // `ljos_in` asserts success, so the refusal runs the binary directly.
    let seat = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("seat")
        .current_dir(&hab.repo)
        .env_clear()
        .envs(hab.base_env())
        .env("LJOS_SEAT", "")
        .env("CURSOR_CONVERSATION_ID", "sand-subagent-002")
        .output()
        .unwrap();
    let seat_err = String::from_utf8_lossy(&seat.stderr);
    assert!(!seat.status.success(), "ljos seat named a subagent seat");
    assert!(seat_err.contains("no seat of its own"), "{seat_err}");
}

/// With `LJOS_SEAT` exported, a git hook holds under the holder `ljos seat`
/// prints (`SEAT@TAG`), the name `ljos claim` takes in the same shell. The
/// hook used to hold under the bare seat, so one conversation held its
/// issues under two names.
#[test]
fn a_git_hook_holds_under_the_seat_holder() {
    let hab = Hab::new("repo");
    let extra = [
        ("LJOS_SEAT", "ljosbot"),
        ("CURSOR_CONVERSATION_ID", "conv-holder-0001"),
    ];
    let seat = hab.ljos_in(&hab.repo, &["seat"], None, &extra);
    let said = String::from_utf8(seat.stdout).unwrap();
    let holder = said
        .lines()
        .find_map(|line| line.strip_prefix("holder\t"))
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    assert!(holder.starts_with("ljosbot@"), "{said}");
    let out = under_exec_daemon(&hab, &extra);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let vlog = vissue_log(&hab.tracker);
    assert!(
        vlog.lines()
            .any(|line| line.starts_with("claim ")
                && line.ends_with(&format!("VISSUE_AGENT={holder}"))),
        "the hook did not claim as {holder}:\n{vlog}\n{err}"
    );
    assert!(
        !vlog
            .lines()
            .any(|line| line.starts_with("claim ") && line.ends_with("VISSUE_AGENT=ljosbot")),
        "the hook claimed under the bare seat:\n{vlog}"
    );
}

/// No `LJOS_SEAT` and no conversation id: the process tree is not a seat,
/// so the hook books nothing.
#[test]
fn a_git_hook_without_a_resolved_seat_does_not_book() {
    let hab = Hab::new("repo");
    let out = under_exec_daemon(&hab, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(vissue_log(&hab.tracker), "");
    assert_eq!(
        fs::read_to_string(hab.tracker.join("deedar.log")).unwrap_or_default(),
        ""
    );
}

/// promo's notes name the ljos URL. The commit's remote is that repository,
/// and the claim is filed on `ljos`, not on promo.
#[test]
fn a_commit_ignores_a_decoy_project_note() {
    let mut hab = Hab::new("workspace");
    hab.project = None;
    fs::create_dir_all(hab.tracker.join("Software/ljos")).unwrap();
    fs::create_dir_all(hab.tracker.join("Software/promo")).unwrap();
    fs::write(
        hab.tracker.join("Software/ljos/issues.org"),
        "* TODO ljos\n",
    )
    .unwrap();
    fs::write(
        hab.tracker.join("Software/promo/issues.org"),
        "The notes mention https://github.com/leidarljos/ljos as the tool this promo is about.\n* TODO promo\n",
    )
    .unwrap();
    git_bare(
        &hab.repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:leidarljos/ljos.git",
        ],
    );
    install(&hab);
    commit_file(&hab, "landed.txt", "land in ljos");
    let ljos = fs::read_to_string(hab.tracker.join("Software/ljos/issues.org")).unwrap_or_default();
    let promo = fs::read_to_string(hab.tracker.join("Software/promo/issues.org")).unwrap();
    assert!(
        ljos.contains("STARTED"),
        "the claim missed ljos:\n{ljos}\npromo:\n{promo}"
    );
    assert!(
        !promo.contains("STARTED"),
        "promo took the claim because its notes name the url:\n{promo}"
    );
}

fn write_hook(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn git_status(hab: &Hab, args: &[&str]) -> Output {
    Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&hab.repo)
        .args(args)
        .current_dir(&hab.repo)
        .env_clear()
        .envs(hab.base_env())
        .output()
        .unwrap()
}

/// A repository `pre-commit` that exits 1 still blocks the commit. `commit-msg`
/// and `pre-push` still run, with the push's stdin.
#[test]
fn a_repo_pre_commit_still_blocks_and_later_hooks_still_run() {
    let hab = Hab::new("repo");
    install(&hab);
    let stub = fs::read_to_string(hab.share.join("ljos/git-hooks/pre-commit")).unwrap();
    assert!(stub.contains("exec "), "{stub}");
    assert!(hab.share.join("ljos/git-hooks/pre-push").is_file());
    assert!(hab.share.join("ljos/git-hooks/commit-msg").is_file());

    let marker = hab.root.join("hook-marker");
    let hooks = hab.repo.join(".git/hooks");
    write_hook(&hooks.join("pre-commit"), "exit 1\n");
    write_hook(
        &hooks.join("commit-msg"),
        &format!("printf 'commit-msg\\n' >> {}\n", shellish(&marker)),
    );
    write_hook(
        &hooks.join("pre-push"),
        &format!(
            "printf 'pre-push %s %s\\n' \"$1\" \"$2\" >> {m}\ncat >> {m}\n",
            m = shellish(&marker)
        ),
    );

    fs::write(hab.repo.join("blocked.txt"), "x\n").unwrap();
    hab.git(&hab.repo, &["add", "blocked.txt"]);
    let blocked = git_status(&hab, &["commit", "-q", "-m", "blocked"]);
    assert!(
        !blocked.status.success(),
        "pre-commit did not block: {}",
        String::from_utf8_lossy(&blocked.stderr)
    );
    assert_eq!(vissue_log(&hab.tracker), "");

    write_hook(&hooks.join("pre-commit"), "exit 0\n");
    let committed = git_status(&hab, &["commit", "-q", "-m", "allowed"]);
    assert!(
        committed.status.success(),
        "{}",
        String::from_utf8_lossy(&committed.stderr)
    );
    let text = fs::read_to_string(&marker).unwrap_or_default();
    assert!(text.contains("commit-msg"), "{text}");

    let bare = hab.root.join("bare.git");
    fs::create_dir_all(&bare).unwrap();
    git_bare(&bare, &["init", "--bare", "-q"]);
    hab.git(
        &hab.repo,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    let pushed = git_status(&hab, &["push", "-q", "origin", "HEAD"]);
    assert!(
        pushed.status.success(),
        "{}",
        String::from_utf8_lossy(&pushed.stderr)
    );
    let text = fs::read_to_string(&marker).unwrap();
    assert!(text.contains("pre-push origin "), "{text}");
    assert!(text.contains("refs/heads/"), "{text}");
}

/// Under a runner, the installed `pre-commit` and `pre-push` hold what no
/// shell line showed the guard: a commit waits for a live sitting, and a
/// push onto develop or main for the sitting's ticket to clear it. A topic
/// branch needs only the sitting. The person is not gated.
#[test]
fn the_installed_hooks_hold_a_runner_to_its_sitting() {
    let hab = Hab::new("repo");
    install(&hab);
    let bare = hab.root.join("bare.git");
    fs::create_dir_all(&bare).unwrap();
    git_bare(&bare, &["init", "--bare", "-q"]);
    hab.git(
        &hab.repo,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    let runner = [("GROK_SESSION_ID", "5b0c2a71-9d1e-4f3a-8c6b-2e7f1a0d9c43")];
    let as_runner = |args: &[&str]| {
        Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&hab.repo)
            .args(args)
            .current_dir(&hab.repo)
            .env_clear()
            .envs(commit_env(&hab, &runner, false))
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    };
    let refused = |out: &Output, why: &str| {
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "the hook let it through: {err}");
        assert!(err.contains("sitting-gate"), "{err}");
        assert!(err.contains(why), "wanted {why:?}: {err}");
    };

    fs::write(hab.repo.join("work.txt"), "x\n").unwrap();
    hab.git(&hab.repo, &["add", "work.txt"]);
    refused(
        &as_runner(&["commit", "-q", "-m", "work"]),
        "ljos sitting ID",
    );
    refused(
        &as_runner(&["push", "-q", "origin", "HEAD:topic"]),
        "`git push` writes history",
    );

    sit(&hab, "probe-51a1", "grokbot@sit", "grokbot");
    for args in [
        &["commit", "-q", "-m", "work"][..],
        &["push", "-q", "origin", "HEAD:topic"],
    ] {
        let out = as_runner(args);
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    for (dest, sent) in [
        ("HEAD:develop", "sends `develop`"),
        ("HEAD:main", "sends `main`"),
    ] {
        let out = as_runner(&["push", "-q", "origin", dest]);
        refused(&out, "ljos consensus probe-51a1 --no-file");
        refused(&out, sent);
    }
    let person = git_status(&hab, &["push", "-q", "origin", "HEAD:main"]);
    assert!(
        person.status.success(),
        "{}",
        String::from_utf8_lossy(&person.stderr)
    );
    // Up to date, git runs pre-push with no refs: nothing lands on main.
    let again = as_runner(&["push", "-q", "origin", "HEAD:main"]);
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
}

/// A path listed in `[git_hooks] skip` is not booked, and reinstall leaves
/// that file alone.
#[test]
fn a_skip_list_books_nothing_and_reinstall_keeps_the_config() {
    let hab = Hab::new("repo");
    let cfg = hab.config.join("ljos");
    fs::create_dir_all(&cfg).unwrap();
    let path = cfg.join("ljos.toml");
    let repo = hab.repo.canonicalize().unwrap();
    let text = format!("[git_hooks]\nskip = [{:?}]\n", repo.display().to_string());
    fs::write(&path, &text).unwrap();
    install(&hab);
    commit_file(&hab, "skipped.txt", "do not book");
    assert_eq!(vissue_log(&hab.tracker), "");
    assert_eq!(
        fs::read_to_string(hab.tracker.join("deedar.log")).unwrap_or_default(),
        ""
    );
    let before = fs::read(&path).unwrap();
    install(&hab);
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "reinstall rewrote ljos.toml"
    );
    commit_file(&hab, "skipped-again.txt", "still do not book");
    assert_eq!(vissue_log(&hab.tracker), "");
}

fn commit_env(hab: &Hab, extra: &[(&str, &str)], drop_seat: bool) -> Vec<(String, String)> {
    hab.base_env()
        .into_iter()
        .filter(|(key, _)| !drop_seat || key != "LJOS_SEAT")
        .chain(
            extra
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string())),
        )
        .collect()
}

fn commit_with(
    hab: &Hab,
    name: &str,
    message: &str,
    extra: &[(&str, &str)],
    drop_seat: bool,
) -> String {
    fs::write(hab.repo.join(name), "x\n").unwrap();
    let env = commit_env(hab, extra, drop_seat);
    let run = |args: &[&str]| {
        let out = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&hab.repo)
            .args(args)
            .current_dir(&hab.repo)
            .env_clear()
            .envs(env.clone())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["add", name]);
    run(&["commit", "-q", "-m", message]);
    head(hab, &hab.repo)
}

/// ljos-6x14: agy stamps a conversation id and no seat. The git hook books
/// the commit under antigravity@tag.
#[test]
fn an_agy_commit_is_booked_from_the_git_hook() {
    let hab = Hab::new("repo");
    install(&hab);
    let id = "agy-conv-0001-deed";
    sit(&hab, "probe-51a1", "antigravity@sit", "antigravity");
    let sha = commit_with(
        &hab,
        "agy.txt",
        "land from agy",
        &[("ANTIGRAVITY_CONVERSATION_ID", id)],
        true,
    );
    let holder = format!("antigravity@{}", ljos_cli::session_tag(id));
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap_or_default();
    let vlog = vissue_log(&hab.tracker);
    assert!(org.contains(&format!("deed-file-commit-{sha}")), "{org}");
    assert!(vlog.contains(&format!("VISSUE_AGENT={holder}")), "{vlog}");
    assert!(!vlog.contains("exec-daemon"), "{vlog}");
}

/// ljos-qpiw: OpenCode's shell sets OPENCODE_SESSION_ID and does not call
/// the plugin after the command. The git hook books that commit.
#[test]
fn an_opencode_shell_commit_is_booked() {
    let hab = Hab::new("repo");
    install(&hab);
    let id = "ses_cloudflare96de8ec";
    sit(&hab, "probe-51a1", "opencode@sit", "opencode");
    let sha = commit_with(
        &hab,
        "cf.txt",
        "hello from cloudflare",
        &[("OPENCODE_SESSION_ID", id)],
        true,
    );
    let holder = format!("opencode@{}", ljos_cli::session_tag(id));
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap_or_default();
    let vlog = vissue_log(&hab.tracker);
    assert!(org.contains(&format!("deed-file-commit-{sha}")), "{org}");
    assert!(vlog.contains(&format!("VISSUE_AGENT={holder}")), "{vlog}");
}

/// Claude and Codex book from their own hooks. The git hook stays out.
#[test]
fn a_claude_session_is_not_booked_by_the_git_hook() {
    let hab = Hab::new("repo");
    install(&hab);
    sit(&hab, "probe-51a1", "grokbot@sit", "grokbot");
    commit_with(
        &hab,
        "claude.txt",
        "from claude",
        &[("CLAUDECODE", "1")],
        false,
    );
    assert_eq!(vissue_log(&hab.tracker), "");
    assert_eq!(
        fs::read_to_string(hab.tracker.join("deedar.log")).unwrap_or_default(),
        ""
    );
}

/// With a seats file, the hook books a commit only for a seat it lists:
/// an unlisted `LJOS_SEAT`, the `grokbot-<id8>` of an unmapped
/// conversation and an agy conversation book nothing, a commit that names
/// an open issue included.
#[test]
fn a_git_hook_books_only_a_seat_the_seats_file_lists() {
    let hab = Hab::new("repo");
    seed_issue(&hab, "probe-32fx", "TODO");
    install(&hab);
    fs::create_dir_all(hab.config.join("ljos")).unwrap();
    fs::write(hab.config.join("ljos/seats.toml"), "seats = [\"saddle\"]\n").unwrap();
    let cursor = "0b1c2d3e-0000-4000-8000-000000000000";
    let agy = "agy-conv-0003-deed";
    sit(
        &hab,
        "probe-51a1",
        "grokbot-0b1c2d3e@sit",
        "grokbot-0b1c2d3e",
    );
    sit(&hab, "probe-51a2", "antigravity@sit", "antigravity");
    commit_with(&hab, "a.txt", "probe-32fx: unlisted seat", &[], false);
    commit_with(
        &hab,
        "b.txt",
        "unmapped conversation",
        &[("CURSOR_CONVERSATION_ID", cursor)],
        true,
    );
    commit_with(
        &hab,
        "c.txt",
        "agy conversation",
        &[("ANTIGRAVITY_CONVERSATION_ID", agy)],
        true,
    );
    assert_eq!(vissue_log(&hab.tracker), "");
    assert_eq!(deeds(&hab.tracker), 0);

    let sha = commit_with(
        &hab,
        "d.txt",
        "listed seat",
        &[("LJOS_SEAT", "saddle")],
        false,
    );
    let org = fs::read_to_string(hab.tracker.join("Software/probe/issues.org")).unwrap_or_default();
    assert!(org.contains(&format!("deed-file-commit-{sha}")), "{org}");
}

fn book_log(hab: &Hab) -> String {
    fs::read_to_string(hab.run.join("ljos/book.log")).unwrap_or_default()
}

/// A commit made by hand in the tracker's own checkout is a tracker write.
/// It is never booked, and a seat the seats file does not list, or no seat
/// at all, is told so and kept in the book log.
#[test]
fn a_tracker_commit_is_checked_as_a_tracker_write() {
    let hab = Hab::new("repo");
    install(&hab);
    fs::create_dir_all(hab.config.join("ljos")).unwrap();
    fs::write(hab.config.join("ljos/seats.toml"), "seats = [\"saddle\"]\n").unwrap();
    let commit = |name: &str, seat: Option<&str>| -> String {
        fs::write(hab.tracker.join(name), "x\n").unwrap();
        let env: Vec<(String, String)> = env_without_seat(&hab)
            .into_iter()
            .chain(seat.map(|s| ("LJOS_SEAT".to_string(), s.to_string())))
            .collect();
        let run = |args: &[&str]| {
            let out = Command::new("/usr/bin/git")
                .arg("-C")
                .arg(&hab.tracker)
                .args(args)
                .current_dir(&hab.tracker)
                .env_clear()
                .envs(env.clone())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stderr).to_string()
        };
        run(&["add", name]);
        run(&["commit", "-q", "-m", name])
    };

    let unlisted = commit("stranger.org", Some("stranger"));
    assert!(unlisted.contains("tracker commit"), "{unlisted}");
    assert!(unlisted.contains("no seat named stranger"), "{unlisted}");
    let unnamed = commit("unnamed.org", None);
    assert!(unnamed.contains("tracker commit"), "{unnamed}");
    assert!(unnamed.contains("no seat is named"), "{unnamed}");
    let log = book_log(&hab);
    assert!(log.contains("no seat named stranger"), "{log}");
    assert!(log.contains("no seat is named"), "{log}");

    let listed = commit("saddle.org", Some("saddle"));
    assert!(!listed.contains("tracker commit"), "{listed}");
    assert_eq!(vissue_log(&hab.tracker), "", "a tracker commit was booked");
    assert_eq!(deeds(&hab.tracker), 0);
}

/// ljos's own tracker commit runs as the seat its write check named, so
/// the hook passes it when `VISSUE_AGENT` alone named that seat.
#[test]
fn an_ljos_tracker_commit_is_checked_as_the_seat_that_wrote() {
    let hab = Hab::new("repo");
    seed_issue(&hab, "probe-32fx", "TODO");
    install(&hab);
    fs::create_dir_all(hab.config.join("ljos")).unwrap();
    fs::write(hab.config.join("ljos/seats.toml"), "seats = [\"saddle\"]\n").unwrap();
    let filed = hab.ljos_in(
        &hab.repo,
        &["file", "a tracker write", "--top", "-p", "probe"],
        None,
        &[("LJOS_SEAT", ""), ("VISSUE_AGENT", "saddle")],
    );
    let said = String::from_utf8_lossy(&filed.stdout);
    let made = hab.git(&hab.tracker, &["log", "--format=%s"]);
    assert!(!made.stdout.is_empty(), "no tracker commit: {said}");
    let log = book_log(&hab);
    assert!(!log.contains("tracker commit"), "{log}");
}
