//! A scratch git does not read a git config this process inherited.
//!
//! The helper clears the environment, then sets `GIT_CONFIG_NOSYSTEM` and
//! `GIT_CONFIG_GLOBAL` itself. A `GIT_CONFIG_*` on the inherited environment
//! is not put back. [`isolated_git::Git::system`] is the opt-in that does
//! pass a system file through.

ljos_test_boot::harness!();

#[path = "common/git.rs"]
mod isolated_git;

use std::path::{Path, PathBuf};
use std::process::Output;

fn said(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn listed(git: isolated_git::Git, repo: &Path) -> String {
    let out = git
        .command()
        .args(["config", "--show-origin", "--list"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", said(&out));
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn write_config(path: &Path, leak: &str, hooks: &Path) {
    std::fs::write(
        path,
        format!(
            "[ljos]\n\tleak = {leak}\n[core]\n\thooksPath = {}\n",
            hooks.display()
        ),
    )
    .unwrap();
}

/// The child inherits planted `GIT_CONFIG_SYSTEM`, `GIT_CONFIG_GLOBAL` and
/// `GIT_CONFIG_NOSYSTEM=0`. Each planted file sets `ljos.leak` and
/// `core.hooksPath`. The scratch git shows none of that. The same child
/// checks that [`isolated_git::Git::system`] and [`isolated_git::Git::home_global`]
/// do pass the file the test names.
#[test]
fn a_system_gitconfig_does_not_reach_scratch_git() {
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("system.gitconfig");
    let global = dir.path().join("global.gitconfig");
    write_config(&system, "from-system", &dir.path().join("sys-hooks"));
    write_config(&global, "from-global", &dir.path().join("global-hooks"));
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args([
            "--exact",
            "inherited_git_config_stays_out_of_scratch_git",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env(ljos_test_boot::GIT_CEILING, ljos_test_boot::git_ceiling())
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", dir.path())
        .env("GIT_CONFIG_SYSTEM", &system)
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_CONFIG_NOSYSTEM", "0")
        .output()
        .unwrap();
    let text = said(&out);
    assert!(
        text.contains("test inherited_git_config_stays_out_of_scratch_git ... ok"),
        "{text}"
    );
    assert!(out.status.success(), "{text}");
}

/// Planted by [`a_system_gitconfig_does_not_reach_scratch_git`]. Not part of
/// the suite: without those variables the assertions have nothing to catch.
#[test]
#[ignore = "run by a_system_gitconfig_does_not_reach_scratch_git"]
fn inherited_git_config_stays_out_of_scratch_git() {
    let system = PathBuf::from(std::env::var_os("GIT_CONFIG_SYSTEM").expect("planted system"));
    let global = PathBuf::from(std::env::var_os("GIT_CONFIG_GLOBAL").expect("planted global"));
    assert_eq!(
        std::env::var("GIT_CONFIG_NOSYSTEM").expect("planted nosystem"),
        "0"
    );
    let system_text = std::fs::read_to_string(&system).unwrap();
    let global_text = std::fs::read_to_string(&global).unwrap();
    assert!(system_text.contains("from-system"), "{system_text}");
    assert!(system_text.contains("hooksPath"), "{system_text}");
    assert!(global_text.contains("from-global"), "{global_text}");
    assert!(global_text.contains("hooksPath"), "{global_text}");

    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join(".gitconfig"),
        "[ljos]\n\tleak = from-home\n",
    )
    .unwrap();
    let repo = home.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let init = isolated_git::Git::new()
        .home(home.path())
        .command()
        .args(["init", "-q"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(init.status.success(), "{}", said(&init));

    let bare = isolated_git::Git::new().home(home.path());
    let bare_cmd = bare.command();
    let envs: Vec<_> = bare_cmd.get_envs().collect();
    let val = |key: &str| {
        envs.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.map(std::ffi::OsStr::to_owned))
    };
    assert_eq!(val("GIT_CONFIG_GLOBAL").unwrap().unwrap(), "/dev/null");
    assert_eq!(val("GIT_CONFIG_NOSYSTEM").unwrap().unwrap(), "1");
    assert!(val("GIT_CONFIG_SYSTEM").is_none());
    let text = listed(isolated_git::Git::new().home(home.path()), &repo);
    for needle in ["from-system", "from-global", "from-home", "hooksPath"] {
        assert!(!text.contains(needle), "{needle} leaked:\n{text}");
    }

    let opted = isolated_git::Git::new().home(home.path()).system(&system);
    assert_eq!(opted.system_config(), Some(system.as_path()));
    let opted_cmd = opted.command();
    let opted_envs: Vec<_> = opted_cmd.get_envs().collect();
    let opted_val = |key: &str| {
        opted_envs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.map(std::ffi::OsStr::to_owned))
    };
    assert_eq!(
        opted_val("GIT_CONFIG_SYSTEM").unwrap().unwrap(),
        system.as_os_str()
    );
    assert!(opted_val("GIT_CONFIG_NOSYSTEM").is_none());
    let opted_text = listed(
        isolated_git::Git::new().home(home.path()).system(&system),
        &repo,
    );
    assert!(
        opted_text.contains("from-system"),
        "the named system config was not read:\n{opted_text}"
    );
    assert!(
        opted_text.contains(&system.display().to_string()),
        "the named system path was not the origin:\n{opted_text}"
    );
    assert!(
        !opted_text.contains("from-global"),
        "the planted global config leaked:\n{opted_text}"
    );

    let at_home = listed(
        isolated_git::Git::new().home(home.path()).home_global(),
        &repo,
    );
    assert!(
        at_home.contains("from-home"),
        "the home gitconfig was not read:\n{at_home}"
    );
    assert!(
        !at_home.contains("from-global"),
        "the planted global config leaked:\n{at_home}"
    );
    assert!(
        !at_home.contains("from-system"),
        "the planted system config leaked:\n{at_home}"
    );
}

/// [`isolated_git::Git::system`] passes the path to git. The assertion
/// fails when the path is only stored and never set on the command.
#[test]
fn a_named_system_config_is_read_when_the_test_opts_in() {
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("system.gitconfig");
    write_config(&system, "opt-in-system", &dir.path().join("opt-hooks"));
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let init = isolated_git::Git::new()
        .home(dir.path())
        .system(&system)
        .command()
        .args(["init", "-q"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(init.status.success(), "{}", said(&init));
    let text = listed(
        isolated_git::Git::new().home(dir.path()).system(&system),
        &repo,
    );
    assert!(
        text.contains("opt-in-system"),
        "the named system config was not read:\n{text}"
    );
    assert!(
        text.contains(&system.display().to_string()),
        "the named system path was not the origin:\n{text}"
    );
}
