//! Grok Bot commits are booked by git hooks, without a prompt hook.
//! The directory is outside the temp roots, not /tmp: ljos skips a temp seat.

ljos_test_boot::harness!();
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
        if rest[i] in ("-p", "--parent", "-t", "--tags", "--priority", "--body") and i + 1 < len(rest):
            if rest[i] == "-p":
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
    fs::write(
        dir.join("claimdag"),
        r#"#!/usr/bin/env python3
import json, os, pathlib, sys
root = pathlib.Path(os.environ["VISSUE_ROOT"])
st = root / "claimdag.json"
s = json.loads(st.read_text()) if st.exists() else {"known": [], "claims": {}}
a = sys.argv[1:]
def opt(n):
    return a[a.index(n) + 1] if n in a else None
def save():
    st.write_text(json.dumps(s))
cmd = a[0] if a else ""
if cmd == "--version":
    print("claimdag 99.0.0")
elif cmd == "upsert":
    s["known"].append(opt("--id"))
    save()
elif cmd == "get":
    i = a[1]
    if i in s["claims"]:
        print(f"id={i} status=claimed assignee={s['claims'][i]} gen=2")
    elif i in s["known"]:
        print(f"id={i} status=ready gen=1")
    else:
        sys.exit(1)
elif cmd == "claim":
    i, who = a[1], opt("--assignee")
    cur = s["claims"].get(i)
    if cur and cur != who:
        sys.stderr.write(f"claimdag: node {i} status claimed\n")
        sys.exit(1)
    s["claims"][i] = who
    if i not in s["known"]:
        s["known"].append(i)
    save()
    print("gen=2")
elif cmd == "renew":
    print("gen=2")
elif cmd == "release":
    s["claims"].pop(a[1], None)
    save()
else:
    print("[]")
"#,
    )
    .unwrap();
    fs::write(dir.join("ljos"), "").unwrap();
    fs::remove_file(dir.join("ljos")).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_ljos"), dir.join("ljos")).unwrap();
    for name in ["vissue", "deedar", "claimdag"] {
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
    let node_id = ljos_cli::work_id(id);
    let actor = ljos_cli::work_id(&format!("{holder}:{id}"));
    let state_path = hab.tracker.join("claimdag.json");
    let mut state: serde_json::Value = fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"known": [], "claims": {}}));
    state["known"]
        .as_array_mut()
        .unwrap()
        .push(node_id.clone().into());
    state["claims"][&node_id] = actor.clone().into();
    fs::write(&state_path, state.to_string()).unwrap();
    fs::create_dir_all(hab.run.join("ljos")).unwrap();
    for key in [id, actor.as_str()] {
        fs::write(
            hab.run.join("ljos").join(format!("hold-{key}")),
            format!(
                "{holder}\n{seat}\n{}\n{}\n2026-01-01T00:00:00Z\n{id}\n-\n",
                std::process::id(),
                comm.trim()
            ),
        )
        .unwrap();
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
fn ljos_seat_ignores_a_seats_file_and_a_raw_conversation_id() {
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
    let said = String::from_utf8(known.stdout).unwrap();
    assert!(!said.contains("rgoswami"), "{said}");
    assert!(!said.contains("seats.toml"), "{said}");
    let unknown_id = "zzzzzzzz-hello";
    let unknown = hab.ljos_in(
        &hab.repo,
        &["seat"],
        None,
        &[("LJOS_SEAT", ""), ("CURSOR_CONVERSATION_ID", unknown_id)],
    );
    let name = String::from_utf8(unknown.stdout).unwrap();
    assert!(!name.contains("grokbot-zzzzzzzz"), "{name}");
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

/// The parent is `exec-daemon`. A seats file and a conversation id do not
/// book the commit, and neither does the process tree.
#[test]
fn a_git_hook_ignores_a_seats_file_and_the_process_tree() {
    let hab = Hab::new("repo");
    let id = "bc-1234abcd-rest";
    let dir = hab.config.join("ljos");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("seats.toml"), format!("{id:?} = \"rgoswami\"\n")).unwrap();
    let out = under_exec_daemon(&hab, &[("CURSOR_CONVERSATION_ID", id)]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let vlog = vissue_log(&hab.tracker);
    let dlog = fs::read_to_string(hab.tracker.join("deedar.log")).unwrap_or_default();
    assert!(
        !vlog.contains("rgoswami") && !vlog.contains("exec-daemon"),
        "the hook booked from the file or the tree:\n{vlog}\n{err}"
    );
    assert!(
        !dlog.contains("rgoswami") && !dlog.contains("exec-daemon"),
        "{dlog}"
    );

    let hab = Hab::new("repo");
    let unknown = "zzzzzzzz-hello";
    let out = under_exec_daemon(&hab, &[("CURSOR_CONVERSATION_ID", unknown)]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let vlog = vissue_log(&hab.tracker);
    let dlog = fs::read_to_string(hab.tracker.join("deedar.log")).unwrap_or_default();
    assert!(
        !vlog.contains("grokbot-zzzzzzzz") && !vlog.contains("exec-daemon"),
        "{vlog}"
    );
    assert!(
        !dlog.contains("grokbot-zzzzzzzz") && !dlog.contains("exec-daemon"),
        "{dlog}"
    );
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

/// Re-running `ljos claim <parent>` from the same sand-sub conversation
/// should be idempotent: once one child exists for that parent+subagent,
/// further retries must reuse it, not create unbounded siblings.
#[test]
fn repeated_subagent_claim_on_parent_is_idempotent() {
    let hab = Hab::new("repo");
    let id = "probe-parent";
    seed_issue(&hab, id, "STARTED");
    // Parent conversation holds parent issue.
    let parent = "0199a1b2-c3d4-4e5f-8a6b-00000000000x";
    let holder = format!("grokbot@{}", ljos_cli::session_tag(parent));
    sit(&hab, id, &holder, "grokbot");

    // Same subagent retries parent claim three times.
    for _ in 0..3 {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["claim", id])
            .current_dir(&hab.repo)
            .env_clear()
            .envs(hab.base_env())
            .env("CURSOR_CONVERSATION_ID", "sand-subagent-0001cccc")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let log = vissue_log(&hab.tracker);
    let creates = log.lines().filter(|l| l.starts_with("create ")).count();
    assert_eq!(
        creates, 1,
        "retries created duplicate children under the same parent:\n{log}"
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

fn book_log(hab: &Hab) -> String {
    fs::read_to_string(hab.run.join("ljos/book.log")).unwrap_or_default()
}

/// A commit made by hand in the tracker's own checkout is a tracker write.
/// It is never booked, and a seat vissue.toml does not list, or no seat
/// at all, is told so and kept in the book log.
#[test]
fn a_tracker_commit_is_checked_as_a_tracker_write() {
    let hab = Hab::new("repo");
    install(&hab);
    fs::write(hab.tracker.join("vissue.toml"), "seats = [\"saddle\"]\n").unwrap();
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

/// A `core.hooksPath` the user already set stays put. Install chains the
/// user's `post-commit`, and uninstall puts that file back.
#[test]
fn git_hooks_keep_a_user_hooks_path_and_chain_post_commit() {
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
        "{configured}"
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
    hab.git(&hab.repo, &["commit", "-q", "-m", "book the readme"]);
    let marked = fs::read_to_string(&marker).unwrap();
    assert!(
        marked.starts_with("ran"),
        "the user's hook did not run: {marked}"
    );
    let log = book_log(&hab);
    assert!(log.contains("book the readme"), "{log}");

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
    let before = book_log(&hab);
    fs::write(hab.repo.join("two.txt"), "y\n").unwrap();
    hab.git(&hab.repo, &["add", "two.txt"]);
    hab.git(&hab.repo, &["commit", "-q", "-m", "after uninstall"]);
    assert_eq!(
        fs::read_to_string(&marker).unwrap(),
        format!("{marked}ran"),
        "the user's hook stopped running"
    );
    assert_eq!(
        book_log(&hab),
        before,
        "uninstall left the booking hook in place"
    );
}

/// An owned `post-rewrite` keeps stdin and hands that copy to the
/// repository hook.
#[test]
fn owned_hooks_pass_post_rewrite_stdin_to_the_repo_hook() {
    let hab = Hab::new("repo");
    install(&hab);
    let sha = commit_file(&hab, "one.txt", "book the readme");
    let rewrite_in = hab.root.join("rewrite-stdin");
    let rewrite_hook = hab.repo.join(".git/hooks/post-rewrite");
    fs::create_dir_all(hab.repo.join(".git/hooks")).unwrap();
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
}

/// A `pre-push` stub that ends in `|| true` is named by doctor and
/// `ljos git-hooks status`. The push goes out. Reinstall makes the
/// refusal stop the push.
#[test]
fn a_stale_pre_push_stub_is_named_and_reinstall_stops_the_push() {
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
    let push = |dest: &str| {
        Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&hab.repo)
            .args(["push", "-q", "origin", dest])
            .current_dir(&hab.repo)
            .env_clear()
            .envs(commit_env(&hab, &runner, false))
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    };
    let sent = |branch: &str| {
        Command::new("/usr/bin/git")
            .arg("--git-dir")
            .arg(&bare)
            .args([
                "rev-parse",
                "-q",
                "--verify",
                &format!("refs/heads/{branch}"),
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
            .status
            .success()
    };
    let hooks_row = || {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["doctor", "--all"])
            .current_dir(&hab.repo)
            .env_clear()
            .envs(hab.base_env())
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout).to_string();
        said.lines()
            .find(|line| line.contains("\tgit hooks\t"))
            .unwrap_or_else(|| panic!("no git hooks row:\n{said}"))
            .to_string()
    };
    let blocked = push("HEAD:develop");
    let err = String::from_utf8_lossy(&blocked.stderr);
    assert!(
        !blocked.status.success(),
        "the hook let develop through: {err}"
    );
    assert!(err.contains("sitting-gate"), "{err}");
    assert!(!err.contains("predates this gate"), "{err}");
    assert!(!sent("develop"), "develop reached the remote: {err}");
    assert!(hooks_row().starts_with("ok\t"), "{}", hooks_row());

    let path = hab.share.join("ljos/git-hooks/pre-push");
    let stub = fs::read_to_string(&path).unwrap();
    let begin = stub.find("# ljos:begin git-hook").unwrap();
    let end_mark = "# ljos:end git-hook\n";
    let end = stub.find(end_mark).unwrap() + end_mark.len();
    let bin = shellish(Path::new(env!("CARGO_BIN_EXE_ljos")));
    let old = format!(
        "# ljos:begin git-hook\n\
         if command -v ljos >/dev/null 2>&1; then\n\
         \tljos git-hook pre-push \"$@\" >&2 || true\n\
         else\n\
         \t{bin} git-hook pre-push \"$@\" >&2 || true\n\
         fi\n\
         {end_mark}"
    );
    fs::write(&path, format!("{}{old}{}", &stub[..begin], &stub[end..])).unwrap();

    let out = push("HEAD:develop");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("sitting-gate"), "{err}");
    assert!(
        err.contains("predates this gate"),
        "the refusal hid that git pushed: {err}"
    );
    assert!(out.status.success() && sent("develop"), "{err}");
    let row = hooks_row();
    assert!(!row.starts_with("ok\t"), "{row}");
    assert!(row.contains("pre-push stub predates"), "{row}");
    assert!(row.contains("ljos git-hooks install"), "{row}");
    let status = String::from_utf8(hab.ljos(&["git-hooks", "status"]).stdout).unwrap();
    assert!(status.contains("pre-push stub predates"), "{status}");

    install(&hab);
    fs::write(hab.repo.join("again.txt"), "z\n").unwrap();
    let add = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&hab.repo)
        .args(["add", "again.txt"])
        .env_clear()
        .envs(hab.base_env())
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let commit = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&hab.repo)
        .args(["commit", "-q", "-m", "after reinstall"])
        .env_clear()
        .envs(hab.base_env())
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "{}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let remote = Command::new("/usr/bin/git")
        .arg("--git-dir")
        .arg(&bare)
        .args(["rev-parse", "refs/heads/develop"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let before = String::from_utf8_lossy(&remote.stdout).trim().to_string();
    let again = push("HEAD:develop");
    let err = String::from_utf8_lossy(&again.stderr);
    assert!(
        !again.status.success(),
        "reinstall left the stale stub: {err}"
    );
    assert!(err.contains("sitting-gate"), "{err}");
    assert!(!err.contains("predates this gate"), "{err}");
    let remote = Command::new("/usr/bin/git")
        .arg("--git-dir")
        .arg(&bare)
        .args(["rev-parse", "refs/heads/develop"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&remote.stdout).trim(),
        before,
        "develop moved after reinstall: {err}"
    );
    assert!(hooks_row().starts_with("ok\t"), "{}", hooks_row());
}

/// `write_seat("commit")` checks `vissue.toml` before the post-commit note.
/// An unlisted seat and an agy conversation write no note. A listed seat does.
#[test]
fn a_git_hook_notes_only_a_seat_vissue_toml_lists() {
    let hab = Hab::new("repo");
    install(&hab);
    fs::write(hab.tracker.join("vissue.toml"), "seats = [\"saddle\"]\n").unwrap();
    let said = |name: &str, message: &str, extra: &[(&str, &str)], drop_seat: bool| {
        fs::write(hab.repo.join(name), "x\n").unwrap();
        let env = commit_env(&hab, extra, drop_seat);
        let run = |args: &[&str]| {
            Command::new("/usr/bin/git")
                .arg("-C")
                .arg(&hab.repo)
                .args(args)
                .current_dir(&hab.repo)
                .env_clear()
                .envs(env.clone())
                .output()
                .unwrap()
        };
        let add = run(&["add", name]);
        assert!(
            add.status.success(),
            "{}",
            String::from_utf8_lossy(&add.stderr)
        );
        let out = run(&["commit", "-q", "-m", message]);
        assert!(
            out.status.success(),
            "git commit: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    let unlisted = said(
        "a.txt",
        "unlisted seat",
        &[("LJOS_SEAT", "stranger")],
        false,
    );
    assert!(unlisted.contains("not booked"), "{unlisted}");
    assert!(unlisted.contains("no seat named stranger"), "{unlisted}");
    assert!(
        !book_log(&hab).contains("unlisted seat"),
        "{}",
        book_log(&hab)
    );

    let agy = said(
        "c.txt",
        "agy conversation",
        &[("ANTIGRAVITY_CONVERSATION_ID", "agy-conv-0003-deed")],
        true,
    );
    assert!(agy.contains("not booked"), "{agy}");
    assert!(
        !book_log(&hab).contains("agy conversation"),
        "{}",
        book_log(&hab)
    );

    let listed = said("d.txt", "listed seat", &[("LJOS_SEAT", "saddle")], false);
    assert!(
        book_log(&hab).contains("listed seat") || listed.contains("note:"),
        "listed seat was not noted\nstderr: {listed}\nlog: {}",
        book_log(&hab)
    );
}
