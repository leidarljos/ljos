//! An isolated `git` for a scratch repository.
//!
//! The command clears its environment first. `GIT_CONFIG_NOSYSTEM`,
//! `GIT_CONFIG_GLOBAL`, `HOME`, `PATH` and the ceiling are set after that
//! clear. A `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM` or `GIT_CONFIG_NOSYSTEM`
//! on this process is not copied back. A test that needs a config sets it
//! on the command, or with [`Git::global`], [`Git::home_global`] or
//! [`Git::system`].
// Each integration test is its own crate. A method another test uses is
// dead in this one.
#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Locale and temp variables a scratch git may keep. Config variables are
/// not in this list: the clear drops them, and the command sets its own.
const KEPT: &[&str] = &["LANG", "LC_ALL", "LC_MESSAGES", "TZ"];

/// What one isolated git is built from.
pub struct Git {
    program: OsString,
    home: Option<PathBuf>,
    path: OsString,
    /// `GIT_CONFIG_GLOBAL`. `None` leaves the variable unset, so git reads
    /// `$HOME/.gitconfig`. The default is `/dev/null`.
    global: Option<PathBuf>,
    /// `GIT_CONFIG_SYSTEM` when the test opted in with [`Git::system`].
    /// Setting it leaves `GIT_CONFIG_NOSYSTEM` unset.
    system: Option<PathBuf>,
    ceiling: OsString,
    tmpdir: Option<OsString>,
    kept: Vec<(OsString, OsString)>,
}

impl Git {
    /// `git` from `PATH`, with the process home, path and temp directory.
    /// `GIT_CONFIG_GLOBAL` is `/dev/null` and `GIT_CONFIG_NOSYSTEM` is `1`.
    /// Nothing named `GIT_CONFIG_*` on this process is kept.
    #[must_use]
    pub fn new() -> Self {
        let path = std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"));
        let kept = KEPT
            .iter()
            .filter_map(|key| {
                let value = std::env::var_os(key)?;
                Some((OsString::from(*key), value))
            })
            .collect();
        Self {
            program: OsString::from("git"),
            home: std::env::var_os("HOME").map(PathBuf::from),
            path,
            global: Some(PathBuf::from("/dev/null")),
            system: None,
            ceiling: ljos_test_boot::git_ceiling(),
            tmpdir: std::env::var_os("TMPDIR"),
            kept,
        }
    }

    /// Run `program` instead of `git` on `PATH`.
    #[must_use]
    pub fn program(mut self, program: impl AsRef<OsStr>) -> Self {
        self.program = program.as_ref().to_os_string();
        self
    }

    /// `HOME` for this git. A later `.env("HOME", ...)` on the command
    /// still overrides it.
    #[must_use]
    pub fn home(mut self, home: impl AsRef<Path>) -> Self {
        self.home = Some(home.as_ref().to_path_buf());
        self
    }

    /// `PATH` for this git.
    #[must_use]
    pub fn path_env(mut self, path: impl AsRef<OsStr>) -> Self {
        self.path = path.as_ref().to_os_string();
        self
    }

    /// `GIT_CONFIG_GLOBAL` for this command. This replaces the default
    /// `/dev/null`.
    #[must_use]
    pub fn global(mut self, global: impl AsRef<Path>) -> Self {
        self.global = Some(global.as_ref().to_path_buf());
        self
    }

    /// Leave `GIT_CONFIG_GLOBAL` unset so git reads `$HOME/.gitconfig`.
    #[must_use]
    pub fn home_global(mut self) -> Self {
        self.global = None;
        self
    }

    /// Read `system` as the system gitconfig. `GIT_CONFIG_NOSYSTEM` is left
    /// unset so git reads that file.
    #[must_use]
    pub fn system(mut self, system: impl AsRef<Path>) -> Self {
        self.system = Some(system.as_ref().to_path_buf());
        self
    }

    /// The system config from [`Git::system`], when one was named.
    #[must_use]
    pub fn system_config(&self) -> Option<&Path> {
        self.system.as_deref()
    }

    /// The isolated command. The environment is cleared before any
    /// variable is set. `GIT_CONFIG_NOSYSTEM` is `1` and `GIT_CONFIG_GLOBAL`
    /// is `/dev/null`, unless [`Git::system`], [`Git::global`] or
    /// [`Git::home_global`] said otherwise. No `GIT_CONFIG_*` value is read
    /// from this process.
    #[must_use]
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.env_clear()
            .env(ljos_test_boot::GIT_CEILING, &self.ceiling)
            .env("PATH", &self.path)
            .env("GIT_TERMINAL_PROMPT", "0");
        if let Some(global) = &self.global {
            cmd.env("GIT_CONFIG_GLOBAL", global);
        }
        if let Some(system) = &self.system {
            cmd.env("GIT_CONFIG_SYSTEM", system);
        } else {
            cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        }
        if let Some(home) = &self.home {
            cmd.env("HOME", home);
        }
        if let Some(tmpdir) = &self.tmpdir {
            cmd.env("TMPDIR", tmpdir);
        }
        for (key, value) in &self.kept {
            cmd.env(key, value);
        }
        cmd
    }
}

/// `git` from `PATH`, isolated from the login and the system git config.
#[must_use]
pub fn command() -> Command {
    Git::new().command()
}

/// `program` as git, isolated the same way as [`command`].
#[must_use]
pub fn command_at(program: impl AsRef<OsStr>) -> Command {
    Git::new().program(program).command()
}
