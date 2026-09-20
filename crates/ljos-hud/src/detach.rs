//! Weaker detach: `process_group(0)` only. No `setsid`, no `pre_exec`.

use std::fs::OpenOptions;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::cli::HudCli;
use crate::summon;

/// Child argv for a detached owner: always `--foreground`, never a summon verb
/// that would bounce off a live socket.
pub fn child_args(cli: &HudCli) -> Vec<String> {
    let mut args = vec!["--foreground".to_string()];
    if let Some(socket) = &cli.socket {
        args.push("--socket".into());
        args.push(socket.display().to_string());
    }
    if cli.hide && !cli.show && !cli.toggle {
        args.push("--hide".into());
    }
    args
}

/// Spawn this executable with [`child_args`] and wait until the summon socket
/// accepts.
pub fn start_detached(cli: &HudCli) -> anyhow::Result<i32> {
    if summon::already_running() {
        return Ok(0);
    }
    let log_path = crate::log::path();
    if let Some(parent) = log_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let _ = writeln!(
        log_file,
        "\n--- ljos-hud --foreground spawn parent={} ---",
        std::process::id()
    );
    let exe = std::env::current_exe()?;
    let stdout = log_file.try_clone()?;
    let token = summon::peek_env_token();
    let mut cmd = Command::new(&exe);
    cmd.args(child_args(cli))
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(log_file));
    if let Some(tok) = &token {
        cmd.env("XDG_ACTIVATION_TOKEN", tok);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let _child = cmd.spawn()?;
    let _ = summon::take_env_token();
    wait_until_accepts(&summon::default_socket_path(), Duration::from_secs(8));
    Ok(0)
}

fn wait_until_accepts(path: &std::path::Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if summon::socket_accepts(path) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> HudCli {
        HudCli {
            socket: None,
            foreground: false,
            toggle: false,
            show: false,
            hide: false,
            install_desktop: false,
        }
    }

    #[test]
    fn child_always_foreground_and_drops_toggle() {
        let mut cli = base();
        cli.toggle = true;
        let args = child_args(&cli);
        assert!(args.contains(&"--foreground".into()));
        assert!(!args.iter().any(|a| a == "--toggle"));
        assert!(
            !args.iter().any(|a| a.contains("XDG") || a.contains("tok")),
            "token is forwarded in env, not argv"
        );
    }

    #[test]
    fn child_keeps_hide() {
        let mut cli = base();
        cli.hide = true;
        let args = child_args(&cli);
        assert!(args.contains(&"--hide".into()));
    }

    #[cfg(unix)]
    #[test]
    fn start_detached_is_noop_when_summon_accepts() {
        use std::os::unix::net::UnixListener;

        let _guard = crate::env_lock();
        let dir = std::env::temp_dir().join(format!("ljos-hud-detach-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("hud.sock");
        let _ = std::fs::remove_file(&path);
        let _listener = UnixListener::bind(&path).unwrap();
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(crate::summon::SOCKET_ENV, path.to_str().unwrap());
        }
        let code = start_detached(&base()).unwrap();
        assert_eq!(code, 0);
        #[allow(unused_unsafe)]
        unsafe {
            std::env::remove_var(crate::summon::SOCKET_ENV);
        }
        drop(_listener);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
