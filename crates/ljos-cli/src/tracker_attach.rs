//! Where `ljos cloud up` gets the shared tracker.
//!
//! `LJOS_TRACKER_REMOTE` names it. Without that variable, a clone an
//! earlier boot made names it, and then a tracker checkout in the
//! workspace does. A workspace checkout of the named remote lends its fetch
//! URL to the clone, so the clone has the credentials the environment gave
//! that checkout.
//!
//! `LJOS_TRACKER_TOKEN` is a token that can push to the tracker. Without
//! it, a password in the lending checkout's URL, as written or as a global
//! rewrite expands it, is the token. It is stored mode 0600 beside the env
//! file, and the clone's own credential helper reads it there; no URL git
//! stores or passes to a child carries it. Global URL rewrites and
//! credential helpers do not apply to the clone, so a token the machine
//! holds for another repository is not sent.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// The variable that holds a token for the tracker remote.
pub(crate) const TOKEN_VAR: &str = "LJOS_TRACKER_TOKEN";

/// Directories searched for a tracker checkout when nothing sets
/// `LJOS_TRACKER_SEARCH`: each one and the directories directly inside it.
const WORKSPACE_ROOTS: &[&str] = &["/workspace"];

/// The tracker remote this machine shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Remote {
    /// The URL with no user or password. This one is written down.
    pub shown: String,
    /// The URL the clone fetches from. It carries no password.
    pub fetch: String,
    /// Where the URL came from, for the line `cloud up` prints.
    pub from: String,
    /// A password that was in the URL, kept for the credential helper.
    pub secret: Option<Secret>,
}

/// A password taken out of a URL. Its `Debug` does not print it.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// The tracker remote for a clone at `dest`, or `None` when nothing names
/// one.
pub(crate) fn find(given: Option<&str>, dest: &Path) -> Option<Remote> {
    let checkouts = workspace_checkouts();
    if let Some(given) = given {
        let lent = checkouts.iter().find_map(|dir| {
            origin(dir)
                .filter(|(url, _)| same_remote(url, given))
                .map(|found| (dir, found))
        });
        return Some(match lent {
            Some((dir, (fetch, secret))) => Remote {
                shown: redact(given),
                fetch,
                from: format!(
                    "LJOS_TRACKER_REMOTE, fetched as {} fetches it",
                    dir.display()
                ),
                secret,
            },
            None => {
                let (fetch, secret) = split_userinfo(given);
                Remote {
                    shown: redact(given),
                    fetch,
                    from: "LJOS_TRACKER_REMOTE".into(),
                    secret: secret.map(Secret),
                }
            }
        });
    }
    if let Some((fetch, secret)) = origin(dest) {
        return Some(Remote {
            shown: redact(&fetch),
            fetch,
            from: format!("the clone at {}", dest.display()),
            secret,
        });
    }
    checkouts
        .iter()
        .find(|dir| is_tracker(dir))
        .and_then(|dir| {
            let (fetch, secret) = origin(dir)?;
            Some(Remote {
                shown: redact(&fetch),
                fetch,
                from: format!("the tracker checkout at {}", dir.display()),
                secret,
            })
        })
}

/// `url` without its password, and the password. An HTTP URL loses its
/// user too; another scheme keeps the user, which ssh needs. A path or
/// an scp-style address is returned as it is.
pub(crate) fn split_userinfo(url: &str) -> (String, Option<String>) {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once("://") else {
        return (url.to_string(), None);
    };
    let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
    let Some((userinfo, host)) = authority.rsplit_once('@') else {
        return (url.to_string(), None);
    };
    let (user, password) = match userinfo.split_once(':') {
        Some((user, password)) => (user, Some(password)),
        None => (userinfo, None),
    };
    let clean = if matches!(scheme, "http" | "https") || user.is_empty() {
        format!("{scheme}://{host}{path}")
    } else {
        format!("{scheme}://{user}@{host}{path}")
    };
    (
        clean,
        password.filter(|p| !p.is_empty()).map(percent_decode),
    )
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether two remote URLs name one repository. The scheme, a user or
/// token, a trailing `.git` and the host's case do not count.
pub(crate) fn same_remote(a: &str, b: &str) -> bool {
    key(a) == key(b)
}

/// `url` with any user and password removed.
pub(crate) fn redact(url: &str) -> String {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if path.is_empty() && !rest.contains('/') {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}/{path}")
    }
}

fn key(url: &str) -> String {
    let url = url.trim();
    let (host, path) = if let Some((_, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host.split_once(':').map_or(host, |(h, _)| h);
        (host.to_ascii_lowercase(), path.to_string())
    } else if let Some((left, path)) = url
        .split_once(':')
        .filter(|(left, _)| !left.contains('/') && left.len() > 1)
    {
        let host = left.rsplit_once('@').map_or(left, |(_, h)| h);
        (host.to_ascii_lowercase(), path.to_string())
    } else {
        let path =
            fs::canonicalize(url).map_or_else(|_| url.to_string(), |p| p.display().to_string());
        (String::new(), path)
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    format!("{host}/{}", path.trim_start_matches('/'))
}

/// Git checkouts in the workspace: each search root that is one, then the
/// directories directly inside it that are.
fn workspace_checkouts() -> Vec<PathBuf> {
    let roots: Vec<PathBuf> = match std::env::var("LJOS_TRACKER_SEARCH") {
        Ok(list) => std::env::split_paths(&list)
            .filter(|p| !p.as_os_str().is_empty())
            .collect(),
        Err(_) => WORKSPACE_ROOTS.iter().map(PathBuf::from).collect(),
    };
    let mut out = Vec::new();
    for root in roots {
        if root.join(".git").exists() {
            out.push(root.clone());
        }
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        let mut kids: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir() && p.join(".git").exists())
            .collect();
        kids.sort();
        out.extend(kids);
    }
    out
}

/// A checkout that holds a tracker: `vissue.toml` at its root, or an
/// `issues.org` under `Software/<project>/`.
fn is_tracker(dir: &Path) -> bool {
    if dir.join("vissue.toml").is_file() {
        return true;
    }
    fs::read_dir(dir.join("Software")).is_ok_and(|projects| {
        projects
            .filter_map(Result::ok)
            .any(|p| p.path().join("issues.org").is_file())
    })
}

/// The origin `dir` names, with no password, and the password it fetches
/// with: one written in the URL, or one a global rewrite adds to this
/// same repository's URL. A rewrite to another repository lends nothing.
fn origin(dir: &Path) -> Option<(String, Option<Secret>)> {
    if !dir.exists() {
        return None;
    }
    let written = git_line(dir, &["config", "--get", "remote.origin.url"])?;
    let (fetch, mut secret) = split_userinfo(&written);
    if secret.is_none() {
        secret = git_line(dir, &["remote", "get-url", "origin"])
            .filter(|expanded| *expanded != written && same_remote(expanded, &written))
            .and_then(|expanded| split_userinfo(&expanded).1);
    }
    Some((fetch, secret.map(Secret)))
}

fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let out = git(dir, args).ok()?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !line.is_empty()).then_some(line)
}

/// The token in `LJOS_TRACKER_TOKEN`, when it holds one.
pub(crate) fn token() -> Option<String> {
    std::env::var(TOKEN_VAR)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// The URL a token is sent to: HTTPS with the user `x-access-token` and no
/// password, for a hosted remote. A plain HTTP remote stays HTTP. `None`
/// for a path or a `file://` URL, which take no token.
pub(crate) fn token_url(url: &str) -> Option<String> {
    let url = url.trim();
    let scheme_out = if url.starts_with("http://") {
        "http"
    } else {
        "https"
    };
    let (host, path) = if let Some((scheme, rest)) = url.split_once("://") {
        if !matches!(scheme, "https" | "http" | "ssh" | "git+ssh") {
            return None;
        }
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = if scheme == "https" || scheme == "http" {
            host
        } else {
            host.split_once(':').map_or(host, |(h, _)| h)
        };
        (host.to_string(), path.to_string())
    } else {
        let (left, path) = url.split_once(':')?;
        if left.contains('/') || left.len() < 2 {
            return None;
        }
        let host = left.rsplit_once('@').map_or(left, |(_, h)| h);
        (host.to_string(), path.trim_start_matches('/').to_string())
    };
    (!host.is_empty() && !path.is_empty())
        .then(|| format!("{scheme_out}://x-access-token@{host}/{path}"))
}

/// Where the token is kept.
pub(crate) fn token_path(home: &Path) -> PathBuf {
    home.join(".config/ljos/tracker.token")
}

/// The credential helper a clone uses: it answers `get` with the token
/// file, read when git asks, so a new token needs only a new file.
pub(crate) fn helper(token_file: &Path) -> String {
    let file = token_file.display().to_string().replace('\'', "'\\''");
    format!(
        "!f() {{ test \"$1\" = get || exit 0; printf 'username=x-access-token\\npassword=%s\\n' \"$(cat '{file}')\"; }}; f"
    )
}

/// Store `token` mode 0600 and return the helper that reads it.
pub(crate) fn store_token(home: &Path, token: &str) -> Result<String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let path = token_path(home);
    let dir = path.parent().context("token path has no parent")?;
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!(
            "tracker: {} is a symlink, so the token was not written",
            path.display()
        );
    }
    let tmp = dir.join(format!(".tracker.token.tmp-{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .with_context(|| format!("create {}", tmp.display()))?;
    let wrote = (|| -> Result<()> {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(token.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&tmp, &path).with_context(|| format!("install {}", path.display()))?;
        Ok(())
    })();
    if wrote.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    wrote?;
    Ok(helper(&path))
}

/// The `-c` options a clone of `url` runs with. `url` rewritten to itself
/// is the longest `insteadOf` match, so no other rewrite applies. The
/// helper list is emptied and holds `helper` alone, and no askpass runs.
pub(crate) fn clone_config(url: &str, helper: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "-c".to_string(),
        format!("url.{url}.insteadOf={url}"),
        "-c".into(),
        "credential.helper=".into(),
        "-c".into(),
        "core.askPass=".into(),
    ];
    if let Some(helper) = helper {
        args.push("-c".into());
        args.push(format!("credential.helper={helper}"));
    }
    args
}

/// Refuse a clone whose URL a rewrite still turns into one with a password.
/// Only a rule written for exactly `url` can, since it ties with the one
/// [`clone_config`] adds and git keeps the first.
///
/// # Errors
///
/// That rule exists. The error names `shown`, never the password.
pub(crate) fn refuse_secret_rewrite(url: &str, shown: &str) -> Result<()> {
    let mut args = clone_config(url, None);
    args.extend(["ls-remote".into(), "--get-url".into(), url.into()]);
    let out = Command::new("git")
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .context("git ls-remote --get-url")?;
    let expanded = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() && expanded != url && split_userinfo(&expanded).1.is_some() {
        bail!(
            "tracker: a url.*.insteadOf rule in git config rewrites {shown} to a URL with a password, so it was not cloned; remove that rule"
        );
    }
    Ok(())
}

/// Keep `helper` as the clone's only credential helper, fetch and push
/// through `url`, and let no global rewrite change `url`.
pub(crate) fn configure(dir: &Path, helper: Option<&str>, url: &str) -> Result<()> {
    let _ = git(
        dir,
        &["config", "--local", "--unset-all", "credential.helper"],
    );
    let rewrite = format!("url.{url}.insteadOf");
    let _ = git(dir, &["config", "--local", "--unset-all", &rewrite]);
    let mut steps = vec![vec!["config", "--local", "--add", "credential.helper", ""]];
    if let Some(helper) = helper {
        steps.push(vec![
            "config",
            "--local",
            "--add",
            "credential.helper",
            helper,
        ]);
    }
    steps.push(vec!["config", "--local", rewrite.as_str(), url]);
    steps.push(vec!["remote", "set-url", "origin", url]);
    for args in steps {
        let out = git(dir, &args)?;
        if !out.status.success() {
            bail!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    }
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("git {}", args.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_repository_is_one_remote_however_it_is_written() {
        let a = "https://x-access-token:secret@github.com/Org/tracker.git";
        for b in [
            "https://github.com/Org/tracker",
            "git@github.com:Org/tracker.git",
            "ssh://git@github.com/Org/tracker",
            "https://GitHub.com/Org/tracker/",
        ] {
            assert!(same_remote(a, b), "{b}");
        }
        assert!(!same_remote(a, "https://github.com/Org/other"));
        assert!(!same_remote(a, "https://gitlab.com/Org/tracker"));
    }

    #[test]
    fn a_written_url_carries_no_token() {
        assert_eq!(
            redact("https://x-access-token:secret@github.com/o/t"),
            "https://github.com/o/t"
        );
        assert_eq!(redact("git@github.com:o/t.git"), "git@github.com:o/t.git");
        assert_eq!(redact("/srv/tracker.git"), "/srv/tracker.git");
    }

    #[test]
    fn a_token_goes_only_to_a_hosted_remote() {
        let want = Some("https://x-access-token@github.com/o/t.git".to_string());
        assert_eq!(token_url("git@github.com:o/t.git"), want);
        assert_eq!(token_url("https://other:pw@github.com/o/t.git"), want);
        assert_eq!(token_url("ssh://git@github.com:22/o/t.git"), want);
        assert_eq!(token_url("/srv/tracker.git"), None);
        assert_eq!(token_url("file:///srv/tracker.git"), None);
        assert_eq!(
            token_url("http://127.0.0.1:8080/o/t.git").as_deref(),
            Some("http://x-access-token@127.0.0.1:8080/o/t.git")
        );
    }

    #[test]
    fn a_password_comes_out_of_the_url_and_ssh_keeps_its_user() {
        assert_eq!(
            split_userinfo("https://x-access-token:s%2Fcret@github.com/o/t.git"),
            (
                "https://github.com/o/t.git".to_string(),
                Some("s/cret".to_string())
            )
        );
        assert_eq!(
            split_userinfo("https://someone@github.com/o/t"),
            ("https://github.com/o/t".to_string(), None)
        );
        assert_eq!(
            split_userinfo("ssh://git:pw@github.com/o/t"),
            (
                "ssh://git@github.com/o/t".to_string(),
                Some("pw".to_string())
            )
        );
        for kept in [
            "git@github.com:o/t.git",
            "/srv/tracker.git",
            "https://github.com/o/t",
        ] {
            assert_eq!(split_userinfo(kept), (kept.to_string(), None), "{kept}");
        }
        assert_eq!(
            format!("{:?}", Secret("hunter2".into())),
            "Secret(..)",
            "a Debug print shows no password"
        );
    }
}
