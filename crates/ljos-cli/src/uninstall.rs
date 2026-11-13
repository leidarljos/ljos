//! Take ljos off this machine.
//!
//! `ljos uninstall` offboards every runner, puts the global git hooks
//! back, removes the ljos programs and leaves a stub named `ljos` that
//! answers every hook call with an allow. A runner that loaded its hook
//! list while ljos was installed keeps calling the old path for the rest
//! of its session; the stub answers, so nothing is blocked. The seat's
//! state, data and configuration stay unless `purge` is set.
//!
//! `scripts/uninstall.sh` does the same without the program, for a
//! machine the binary was already deleted from.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::Step;

/// The programs the ljos repositories ship.
pub const LJOS_PROGRAMS: &[&str] = &[
    "ljos",
    "ljos-mcp",
    "ljos-policyd",
    "ljos-consensus",
    "ljos-hud",
];

/// The other seat programs the installer puts beside them. They hold the
/// tracker, the pack and the deeds, so they stay unless asked for.
pub const SEAT_PROGRAMS: &[&str] = &[
    "packset",
    "packsetd",
    "packset-mcp",
    "packset-embed",
    "vissue",
    "vissue-mcp",
    "deedar",
    "claimdag",
];

/// The stub left where `ljos` was. `scripts/uninstall.sh` writes the same
/// text; a test keeps the two equal.
pub const STUB: &str = r#"#!/bin/sh
# ljos was uninstalled. This stub stands where the program was, so a
# runner that loaded its hook list while ljos was installed still gets
# an answer: every hook call is allowed. Remove it with
#   rm "$0"
# or reinstall ljos over it.
case "$1" in
  hook)
    cat >/dev/null 2>&1
    case " $* " in *" --git-commit "*) exit 0 ;; esac
    printf '%s\n' '{"permission":"allow","continue":true}'
    exit 0
    ;;
  git-hook|hookd) exit 0 ;;
esac
printf '%s\n' "ljos is uninstalled; $0 is the stub that answers hooks. Reinstall: curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh" >&2
exit 1
"#;

/// What `uninstall` does beyond offboarding.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Report every step and write nothing.
    pub dry: bool,
    /// Leave the programs in place.
    pub keep_programs: bool,
    /// Remove [`SEAT_PROGRAMS`] too.
    pub all: bool,
    /// Remove the state, data, cache and configuration directories.
    pub purge: bool,
    /// Leave [`STUB`] where `ljos` was.
    pub stub: bool,
    /// The directory the programs are in; the running ljos's own when
    /// `None`.
    pub bin_dir: Option<PathBuf>,
}

/// Run every step. Steps that fail are reported, not fatal, so a runner
/// file that cannot be rewritten still leaves the programs removed.
///
/// # Errors
///
/// The onboard manifest cannot be read, or no program directory can be
/// found.
pub fn uninstall(opts: &Options) -> Result<Vec<Step>> {
    let mut steps = crate::offboard::offboard(None, opts.dry)?;
    if !opts.dry {
        steps.extend(still_pointing());
    }
    steps.push(git_hooks_step(opts.dry));
    steps.push(hookd_step(opts.dry));
    if !opts.keep_programs {
        let bin = bin_dir(opts.bin_dir.as_deref())?;
        let names = LJOS_PROGRAMS
            .iter()
            .chain(opts.all.then_some(SEAT_PROGRAMS).into_iter().flatten());
        for name in names {
            if let Some(step) = program_step(&bin, name, opts) {
                steps.push(step);
            }
        }
    }
    if opts.purge {
        for dir in purge_dirs() {
            if let Some(step) = purge_step(&dir, opts.dry) {
                steps.push(step);
            }
        }
    }
    Ok(steps)
}

/// Assistant files that still name the program after offboard. Offboard
/// removes the entries it wrote. A file it could not parse, or one the
/// person edited into a shape offboard does not own, is named here.
fn still_pointing() -> Vec<Step> {
    let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    let home = PathBuf::from(home);
    const RELS: &[&str] = &[
        ".cursor/mcp.json",
        ".cursor/hooks.json",
        ".claude.json",
        ".claude/settings.json",
        ".codex/config.toml",
        ".codex/hooks.json",
        ".gemini/settings.json",
        ".gemini/config/hooks.json",
        ".config/zed/settings.json",
        ".codeium/windsurf/mcp_config.json",
        ".hermes/config.yaml",
        ".grok/config.toml",
        ".factory/mcp.json",
        ".kiro/settings/mcp.json",
        ".copilot/mcp-config.json",
    ];
    RELS.iter()
        .map(|rel| home.join(rel))
        .filter(|path| points_at_ljos(path))
        .map(|path| Step {
            what: "assistants".into(),
            detail: format!(
                "{} still points at ljos. The programs are being removed, so the assistant will fail to start it. Edit {} and delete the ljos entry.",
                path.display(),
                path.display()
            ),
            ok: false,
        })
        .collect()
}

fn points_at_ljos(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.contains("ljos-mcp") || text.contains("ljos hook") || text.contains("[mcp_servers.ljos]")
}

fn git_hooks_step(dry: bool) -> Step {
    let what = "git-hooks".to_string();
    if dry {
        let status = crate::git_hooks::status();
        return Step {
            what,
            detail: format!(
                "would put the global git hooks back ({})",
                status.lines().next().unwrap_or("").trim()
            ),
            ok: true,
        };
    }
    match crate::git_hooks::uninstall() {
        Ok(()) => Step {
            what,
            detail: "the global git hooks are as they were before ljos".into(),
            ok: true,
        },
        Err(e) => Step {
            what,
            detail: format!("the global git hooks could not be put back: {e:#}"),
            ok: false,
        },
    }
}

/// The hook server exits once its socket is gone and no call is waiting.
fn hookd_step(dry: bool) -> Step {
    let what = "hookd".to_string();
    let sock = crate::runtime_dir().join("hookd.sock");
    if !sock.exists() {
        return Step {
            what,
            detail: "no hook server is running".into(),
            ok: true,
        };
    }
    if dry {
        return Step {
            what,
            detail: format!("would stop the hook server at {}", sock.display()),
            ok: true,
        };
    }
    match std::fs::remove_file(&sock) {
        Ok(()) => Step {
            what,
            detail: "the hook server stops once its last call is answered".into(),
            ok: true,
        },
        Err(e) => Step {
            what,
            detail: format!("{}: {e}", sock.display()),
            ok: false,
        },
    }
}

/// `dir`, else the directory of the running `ljos`.
fn bin_dir(dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = dir {
        return Ok(dir.to_path_buf());
    }
    let exe = std::env::current_exe().context("uninstall: no path for the running ljos")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    exe.parent()
        .map(Path::to_path_buf)
        .context("uninstall: the running ljos has no directory")
}

fn program_step(bin: &Path, name: &str, opts: &Options) -> Option<Step> {
    let path = bin.join(name);
    let what = "program".to_string();
    if !path.exists() {
        return None;
    }
    let stub = name == "ljos" && opts.stub;
    if opts.dry {
        return Some(Step {
            what,
            detail: if stub {
                format!(
                    "would replace {} with the stub that allows every hook",
                    path.display()
                )
            } else {
                format!("would remove {}", path.display())
            },
            ok: true,
        });
    }
    let done = if stub {
        write_stub(&path)
            .map(|()| format!("{} is now the stub that allows every hook", path.display()))
    } else {
        std::fs::remove_file(&path).map(|()| format!("removed {}", path.display()))
    };
    Some(match done {
        Ok(detail) => Step {
            what,
            detail,
            ok: true,
        },
        Err(e) => Step {
            what,
            detail: format!("{}: {e}", path.display()),
            ok: false,
        },
    })
}

/// Write [`STUB`] to `path` by rename, so a `ljos` that is running keeps
/// its old file until it exits.
///
/// # Errors
///
/// The directory cannot be written.
pub fn write_stub(path: &Path) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("stub.{}", std::process::id()));
    std::fs::write(&tmp, STUB)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, path)
}

fn xdg(var: &str, under_home: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(under_home))
        })
}

/// The directories the seat and its guard write under the XDG homes.
fn purge_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (var, under, names) in [
        ("XDG_CONFIG_HOME", ".config", &["ljos"][..]),
        ("XDG_DATA_HOME", ".local/share", &["ljos"]),
        ("XDG_STATE_HOME", ".local/state", &["ljos", "ljos-policyd"]),
        ("XDG_CACHE_HOME", ".cache", &["ljos"]),
    ] {
        if let Some(base) = xdg(var, under) {
            out.extend(names.iter().map(|n| base.join(n)));
        }
    }
    out
}

fn purge_step(dir: &Path, dry: bool) -> Option<Step> {
    let what = "purge".to_string();
    if !dir.exists() {
        return None;
    }
    if dry {
        return Some(Step {
            what,
            detail: format!("would remove {}", dir.display()),
            ok: true,
        });
    }
    Some(match std::fs::remove_dir_all(dir) {
        Ok(()) => Step {
            what,
            detail: format!("removed {}", dir.display()),
            ok: true,
        },
        Err(e) => Step {
            what,
            detail: format!("{}: {e}", dir.display()),
            ok: false,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stub answers a hook with an allow, says nothing for a git hook,
    /// and refuses to stand in for the rest of the program.
    #[test]
    fn the_stub_allows_hooks_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ljos");
        write_stub(&path).unwrap();
        let run = |args: &[&str], stdin: &str| {
            use std::io::Write as _;
            let mut child = std::process::Command::new("sh")
                .arg(&path)
                .args(args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(stdin.as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            (
                out.status.code(),
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            )
        };
        let (code, out, _) = run(&["hook"], r#"{"hook_event_name":"PreToolUse"}"#);
        assert_eq!(code, Some(0));
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v["permission"], "allow");
        assert_eq!(v["continue"], true);
        let (code, out, _) = run(&["hook", "--git-commit"], "");
        assert_eq!((code, out.as_str()), (Some(0), ""));
        let (code, out, _) = run(&["git-hook", "post-commit"], "");
        assert_eq!((code, out.as_str()), (Some(0), ""));
        let (code, _, err) = run(&["sitting", "X-1"], "");
        assert_eq!(code, Some(1));
        assert!(err.contains("uninstalled"), "{err}");
    }

    /// The shipped script writes the same stub, so a machine without the
    /// program ends up as one with it.
    #[test]
    fn the_script_carries_the_same_stub() {
        let script = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/uninstall.sh"),
        )
        .unwrap();
        let body = STUB.trim_end_matches('\n');
        assert!(
            script.contains(body),
            "scripts/uninstall.sh does not carry the stub"
        );
    }

    /// The script names every hooks file the shipped runner table does, so
    /// a hand-deleted ljos leaves no runner blocked.
    #[test]
    fn the_script_names_every_shipped_hooks_file() {
        let script = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/uninstall.sh"),
        )
        .unwrap();
        let shipped: crate::Harnesses = toml::from_str(crate::HARNESSES_EXAMPLE).unwrap();
        for h in shipped
            .harness
            .iter()
            .filter(|h| !h.name.starts_with("runner-with-"))
        {
            let Some(hooks) = h.hooks.as_deref() else {
                continue;
            };
            let path = hooks.replace("~/", "$HOME/");
            assert!(
                script.contains(&path),
                "{}: {path} is not in scripts/uninstall.sh",
                h.name
            );
            if let Some(skills) = h.skills.as_deref() {
                let path = skills.replace("~/", "$HOME/");
                assert!(
                    script.contains(&path),
                    "{}: {path} is not in scripts/uninstall.sh",
                    h.name
                );
            }
        }
    }

    #[test]
    fn a_dry_run_removes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("ljos"), "x").unwrap();
        std::fs::write(bin.join("ljos-mcp"), "x").unwrap();
        std::fs::write(bin.join("packset"), "x").unwrap();
        let opts = Options {
            dry: true,
            stub: true,
            bin_dir: Some(bin.clone()),
            ..Options::default()
        };
        let steps: Vec<Step> = LJOS_PROGRAMS
            .iter()
            .filter_map(|n| program_step(&bin, n, &opts))
            .collect();
        assert_eq!(steps.len(), 2, "{steps:?}");
        assert!(steps[0].detail.contains("would replace"), "{steps:?}");
        assert!(steps[1].detail.contains("would remove"), "{steps:?}");
        assert_eq!(std::fs::read_to_string(bin.join("ljos")).unwrap(), "x");
        let live = Options { dry: false, ..opts };
        for n in LJOS_PROGRAMS {
            program_step(&bin, n, &live);
        }
        assert!(std::fs::read_to_string(bin.join("ljos"))
            .unwrap()
            .contains("stub"));
        assert!(!bin.join("ljos-mcp").exists());
        assert!(bin.join("packset").exists(), "other seat programs stay");
    }
}
