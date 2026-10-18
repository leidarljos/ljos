//! Grok Bot commits are booked by git hooks, without a prompt hook.
//! The directory is under the workspace target, not /tmp: ljos skips a temp seat.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

fn work_root() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/grokbot-books-tests")
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
        out
    }

    fn git(&self, dir: &Path, args: &[&str]) -> Output {
        let out = Command::new("git")
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
    let out = Command::new("git")
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
    fh.write(" ".join(args) + "\n")
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
    iid, acc = rest[0], rest[rest.index("--add") + 1]
    needle = f":ID:         {iid}\n:END:\n"
    for f in files():
        text = f.read_text()
        if needle in text:
            f.write_text(text.replace(needle, needle + f"cited {acc}\n", 1))
            sys.exit(0)
    sys.exit("no issue " + iid)
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
(root / "deedar.log").open("a").write(" ".join(args) + "\n")
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

#[test]
fn git_hooks_book_a_commit_once_and_uninstall_restores_the_config() {
    let hab = Hab::new("repo");
    let saved = hab.root.join("saved-hooks");
    fs::create_dir_all(&saved).unwrap();
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
    let configured = String::from_utf8(
        hab.git(
            &hab.repo,
            &["config", "--global", "--get", "core.hooksPath"],
        )
        .stdout,
    )
    .unwrap();
    assert!(
        configured.contains("git-hooks"),
        "hooksPath was not installed: {configured}"
    );
    let marker = hab.root.join("repo-hook");
    let hook = hab.repo.join(".git/hooks/post-commit");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(
        &hook,
        format!("#!/bin/sh\nprintf ran >> {}\n", shellish(&marker)),
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

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
    assert_eq!(
        fs::read_to_string(&marker).unwrap(),
        "ran",
        "the repo hook did not run"
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
    assert_ne!(head(&hab, &hab.repo), sha, "amend did not move HEAD");
    assert_eq!(deeds(&hab.tracker), 2, "an amend was booked more than once");

    hab.git(&hab.repo, &["checkout", "-q", "-b", "side"]);
    commit_file(&hab, "a.txt", "side a");
    commit_file(&hab, "b.txt", "side b");
    hab.git(&hab.repo, &["checkout", "-q", "main"]);
    commit_file(&hab, "c.txt", "main c");
    let before = deeds(&hab.tracker);
    hab.git(&hab.repo, &["checkout", "-q", "side"]);
    hab.git(&hab.repo, &["rebase", "main"]);
    assert_eq!(
        deeds(&hab.tracker),
        before + 1,
        "a rebase was booked more than once"
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
        &[("CURSOR_CONVERSATION_ID", id)],
    );
    assert_eq!(String::from_utf8(known.stdout).unwrap().trim(), "rgoswami");
    let unknown_id = "zzzzzzzz-hello";
    let unknown = hab.ljos_in(
        &hab.repo,
        &["seat"],
        None,
        &[("CURSOR_CONVERSATION_ID", unknown_id)],
    );
    let name = String::from_utf8(unknown.stdout).unwrap();
    assert_eq!(name.trim(), "grokbot-zzzzzzzz");
    assert_ne!(name.trim(), unknown_id);
}

fn shellish(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}
