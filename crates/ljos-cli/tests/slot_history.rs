//! A vote on a long tracker still names the same seat, and does not
//! pickaxe the whole history to decide it.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `mina` is pinned before the slot note and stays in that commit's parent.
/// `eessi` is pinned only after the note, though both lines are in `HEAD`.
fn tracker(repo: &Path) {
    std::fs::create_dir_all(repo).unwrap();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.email", "seat@example.invalid"]);
    git(repo, &["config", "user.name", "seat"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    let hooks = repo.join("no-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    git(repo, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    std::fs::write(repo.join("issues.org"), "board\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "board"]);
    for n in 0..30 {
        std::fs::write(repo.join("filler"), format!("before {n}\n")).unwrap();
        git(repo, &["add", "filler"]);
        git(repo, &["commit", "-q", "-m", &format!("before {n}")]);
    }
    std::fs::write(repo.join(".ljos-seat-keys"), "mina bb\n").unwrap();
    git(repo, &["add", ".ljos-seat-keys"]);
    git(repo, &["commit", "-q", "-m", "pin mina"]);
    for n in 0..20 {
        std::fs::write(repo.join("filler"), format!("mid {n}\n")).unwrap();
        git(repo, &["add", "filler"]);
        git(repo, &["commit", "-q", "-m", &format!("mid {n}")]);
    }
    std::fs::write(
        repo.join("issues.org"),
        "board\n- Note: \"slot: mina model=m run=r key=k ada aa bb\"\n",
    )
    .unwrap();
    git(repo, &["add", "issues.org"]);
    git(repo, &["commit", "-q", "-m", "slot"]);
    for n in 0..20 {
        std::fs::write(repo.join("filler"), format!("after {n}\n")).unwrap();
        git(repo, &["add", "filler"]);
        git(repo, &["commit", "-q", "-m", &format!("after {n}")]);
    }
    std::fs::write(repo.join(".ljos-seat-keys"), "mina bb\neessi aa\n").unwrap();
    git(repo, &["add", ".ljos-seat-keys"]);
    git(repo, &["commit", "-q", "-m", "pin eessi"]);
}

fn invocations(log: &str) -> Vec<Vec<&str>> {
    log.split("\n\n")
        .filter(|block| !block.is_empty())
        .map(|block| block.lines().collect())
        .collect()
}

/// `git log -S` or `git log -G` with no `--since` or `--max-count`.
fn unbounded_pickaxe(args: &[&str]) -> bool {
    let is_log = args.contains(&"log");
    let pick = args.iter().any(|arg| {
        *arg == "-S"
            || *arg == "-G"
            || (arg.starts_with("-S") && arg.len() > 2)
            || (arg.starts_with("-G") && arg.len() > 2)
    });
    let bounded = args.iter().any(|arg| {
        *arg == "--since"
            || arg.starts_with("--since=")
            || *arg == "--max-count"
            || arg.starts_with("--max-count=")
    });
    is_log && pick && !bounded
}

#[test]
fn a_vote_on_a_long_tracker_matches_and_does_not_pickaxe() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("tracker");
    tracker(&repo);
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let drawer = root.path().join("drawer.json");
    let drawer_s = drawer.display().to_string();
    script(
        &bin,
        "vissue",
        &format!(
            "#!/bin/sh\n\
             if [ \"$1\" = vote ] && [ \"$3\" = --json ]; then\n\
               cat '{drawer_s}' 2>/dev/null || echo '[]'\n\
               exit 0\n\
             fi\n\
             if [ \"$1\" = vote ] && [ \"$3\" = --for ]; then\n\
               printf '[{{\"agent\":\"%s\",\"choice\":\"%s\"}}]\\n' \"$VISSUE_AGENT\" \"$4\" > '{drawer_s}'\n\
               exit 0\n\
             fi\n\
             if [ \"$1\" = show ]; then\n\
               echo '{{ \"id\": \"probe-humz\", \"logbook\": [] }}'\n\
               exit 0\n\
             fi\n\
             exit 0\n"
        ),
    );
    let git_log = root.path().join("git.log");
    let git_log_s = git_log.display().to_string();
    script(
        &bin,
        "git",
        &format!(
            "#!/bin/sh\n\
             for arg in \"$@\"; do printf '%s\\n' \"$arg\" >> '{git_log_s}'; done\n\
             printf '\\n' >> '{git_log_s}'\n\
             exec /usr/bin/git \"$@\"\n"
        ),
    );
    let home = root.path().join("home");
    let runtime = root.path().join("run");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ljos = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("VISSUE_ROOT", &repo)
            .env("LJOS_SEAT", "ada")
            .env("VISSUE_AGENT", "ada")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
            .env("GIT_CONFIG_SYSTEM", home.join("gitconfig"))
            .output()
            .unwrap()
    };
    std::fs::write(home.join("gitconfig"), b"").unwrap();

    let mina = ljos(&[
        "vote",
        "probe-humz",
        "--for",
        "ship",
        "--used",
        "none",
        "--as",
        "mina",
    ]);
    let mina_out = String::from_utf8_lossy(&mina.stdout);
    let mina_err = String::from_utf8_lossy(&mina.stderr);
    assert!(
        mina.status.success(),
        "mina vote failed\n{mina_out}\n{mina_err}"
    );
    assert!(
        mina_out.contains("recorded on the tracker: persona:ada:mina for ship on probe-humz"),
        "a seat pinned before slots was not recorded as that persona\n{mina_out}\n{mina_err}"
    );
    assert!(
        !mina_err.contains("slot history: partial"),
        "the lookup stopped early\n{mina_err}"
    );

    let eessi = ljos(&[
        "vote",
        "probe-humz",
        "--for",
        "hold",
        "--used",
        "none",
        "--as",
        "eessi",
    ]);
    let eessi_out = String::from_utf8_lossy(&eessi.stdout);
    let eessi_err = String::from_utf8_lossy(&eessi.stderr);
    assert!(
        eessi.status.success(),
        "eessi vote failed\n{eessi_out}\n{eessi_err}"
    );
    assert!(
        eessi_out.contains("recorded on the tracker: eessi for hold on probe-humz"),
        "a pin added after slots was recorded as a standing seat\n{eessi_out}\n{eessi_err}"
    );
    assert!(
        !eessi_out.contains("persona:ada:eessi"),
        "eessi stood\n{eessi_out}"
    );

    let search = ljos(&["search", "slot"]);
    let search_err = String::from_utf8_lossy(&search.stderr);
    assert!(
        !search_err.contains("slot history: partial"),
        "{search_err}"
    );

    let text = std::fs::read_to_string(&git_log).unwrap_or_default();
    let calls = invocations(&text);
    // Guards the pickaxe that held about 1.1 GiB for over two minutes.
    for args in &calls {
        assert!(
            !unbounded_pickaxe(args),
            "unbounded pickaxe: {}\n{text}",
            args.join(" ")
        );
    }
}
