//! `ljos upgrade` replaces the seat binaries with published releases.
//!
//! The seat guard refuses an agent write to its own binaries, since an
//! agent that may rewrite them may rewrite the law.
//! The binaries still have to move forward, so this verb installs only what
//! a release workflow built and published.
//! The ljos archive comes from this repository.
//! Each other program comes from the repository that publishes it.
//! The archive sha256 is checked against the checksum published beside it.
//! Each binary must be an executable that names the release version, and
//! the old binary is kept beside the new one.
//! A component whose release is missing, or whose download or checksum
//! fails, prints a warning.
//! The others still install.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// The repository whose releases the seat installs ljos from.
pub const RELEASE_REPO: &str = "leidarljos/ljos";

/// Every component repository lives under this owner.
const RELEASE_OWNER: &str = "leidarljos";

/// The binaries a ljos release carries that the seat installs, when present
/// beside the running `ljos`: `ljos` and `ljos-mcp` always.
pub const BINARIES: &[&str] = &["ljos", "ljos-mcp", "ljos-hud"];

/// One program installed from another repository's release.
pub struct Component {
    /// The file name in the install directory, and the `--only` name.
    pub name: &'static str,
    /// The repository under [`RELEASE_OWNER`] that publishes the archive.
    pub repo: &'static str,
    /// The archive file on that release. `{version}` is the tag without a
    /// leading `v`. `{target}` is this machine's rustc triple.
    pub asset: &'static str,
}

/// The programs ljos runs that are published outside this repository.
///
/// Each asset name is the file that repository's release workflow uploads.
/// `install.sh` installs the same programs. A program that is not in the
/// install directory is left alone, so an upgrade does not add one the seat
/// never installed. `--only` names one of these, or one of [`BINARIES`].
pub const COMPONENTS: &[Component] = &[
    // packset-v{version}-{target}.tar.gz. The top directory has that
    // name and holds these four.
    Component {
        name: "packset",
        repo: "packset",
        asset: "packset-v{version}-{target}.tar.gz",
    },
    Component {
        name: "packsetd",
        repo: "packset",
        asset: "packset-v{version}-{target}.tar.gz",
    },
    Component {
        name: "packset-mcp",
        repo: "packset",
        asset: "packset-v{version}-{target}.tar.gz",
    },
    Component {
        name: "packset-embed",
        repo: "packset",
        asset: "packset-v{version}-{target}.tar.gz",
    },
    // vissue-v{version}-{target}.tar.gz.
    // The top directory has that name and holds vissue, vissue-mcp and
    // vissue-hud.
    // ljos runs vissue. vissue-hud is left out.
    Component {
        name: "vissue",
        repo: "vissue",
        asset: "vissue-v{version}-{target}.tar.gz",
    },
    Component {
        name: "vissue-mcp",
        repo: "vissue",
        asset: "vissue-v{version}-{target}.tar.gz",
    },
    Component {
        name: "deedar",
        repo: "deedar",
        asset: "deedar-v{version}-{target}.tar.gz",
    },
    Component {
        name: "claimdag",
        repo: "claimdag",
        asset: "claimdag-v{version}-{target}.tar.gz",
    },
    Component {
        name: "ljos-policyd",
        repo: "ljos-policyd",
        asset: "ljos-policyd-v{version}-{target}.tar.gz",
    },
    // consensus-v{version}-{target}.tar.gz. The top directory has
    // that name and holds ljos-consensus.
    Component {
        name: "ljos-consensus",
        repo: "consensus",
        asset: "consensus-v{version}-{target}.tar.gz",
    },
];

/// The archive triple for this `arch` and `os`.
///
/// The release workflows publish these four triples.
#[must_use]
pub fn target_for(arch: &str, os: &str) -> Option<&'static str> {
    match (arch, os) {
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => Some("aarch64-unknown-linux-gnu"),
        ("x86_64", "macos") => Some("x86_64-apple-darwin"),
        ("aarch64", "macos") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

/// The target triple of the published archive for this machine.
#[must_use]
pub fn target() -> Option<&'static str> {
    target_for(std::env::consts::ARCH, std::env::consts::OS)
}

/// The archive name the release workflow publishes for `version` on
/// `target`: `ljos-v0.22.3-x86_64-unknown-linux-gnu.tar.gz`.
#[must_use]
pub fn asset_name(version: &str, target: &str) -> String {
    format!("ljos-v{}-{target}.tar.gz", version.trim_start_matches('v'))
}

/// [`Component::asset`] with `{version}` and `{target}` filled in.
fn asset_of(pattern: &str, version: &str, target: &str) -> String {
    pattern
        .replace("{version}", version.trim_start_matches('v'))
        .replace("{target}", target)
}

/// The digest in a `sha256sum` line: its first field, when it is 64 hex
/// characters.
#[must_use]
pub fn digest_of(line: &str) -> Option<String> {
    let d = line.split_whitespace().next()?.to_ascii_lowercase();
    (d.len() == 64 && d.chars().all(|c| c.is_ascii_hexdigit())).then_some(d)
}

/// Whether `--version` output names this binary at `version`.
///
/// A line may carry a build note after the version, as `ljos-policyd`
/// prints `ljos-policyd 0.7.0 (phronesis)` and `packset` prints its commit.
/// The note is separated by whitespace, so `0.22.3` does not match `0.22.30`.
#[must_use]
pub fn names_version(output: &str, binary: &str, version: &str) -> bool {
    let want = format!("{binary} {}", version.trim_start_matches('v'));
    output.lines().next().is_some_and(|line| {
        let line = line.trim();
        match line.strip_prefix(&want) {
            Some(rest) => rest.is_empty() || rest.starts_with(char::is_whitespace),
            None => false,
        }
    })
}

fn is_ljos_name(name: &str) -> bool {
    BINARIES.contains(&name)
}

fn is_seat_component(name: &str) -> bool {
    COMPONENTS.iter().any(|c| c.name == name)
}

fn known_names() -> String {
    BINARIES
        .iter()
        .copied()
        .chain(COMPONENTS.iter().map(|c| c.name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every binary this install would replace already reports `version`.
fn already_installed(dir: &Path, version: &str) -> bool {
    let mut saw = false;
    for name in BINARIES {
        let installed = dir.join(name);
        if *name == "ljos-hud" && !installed.exists() {
            continue;
        }
        if !installed.is_file() {
            return false;
        }
        if !reports(&installed, name, version) {
            return false;
        }
        saw = true;
    }
    saw
}

/// Copy `installed` aside as `NAME.bak-pre-VERSION`, unless that file
/// already holds some other version. That copy is the rollback, and a
/// second install of the same release must not replace it with itself.
fn keep_rollback(name: &str, installed: &Path, dir: &Path, version: &str) -> Result<()> {
    if !installed.exists() {
        return Ok(());
    }
    let keep = dir.join(format!("{name}.bak-pre-{version}"));
    if keep.is_file() {
        if let Ok(said) = run_ok(&keep.display().to_string(), &["--version"]) {
            if !names_version(&said, name, version) {
                return Ok(());
            }
        }
    }
    std::fs::copy(installed, &keep)?;
    Ok(())
}

fn release_dir() -> Option<PathBuf> {
    std::env::var_os("LJOS_RELEASE_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
}

/// The published asset, from `LJOS_RELEASE_DIR` when a test or a mirror
/// has the files on disk.
fn obtain(name: &str, to: &Path, url: &str) -> Result<()> {
    if let Some(dir) = release_dir() {
        let src = dir.join(name);
        std::fs::copy(&src, to).with_context(|| {
            format!(
                "upgrade: could not read {} from the release directory, and the archive must be there or LJOS_RELEASE_DIR must be unset, so run `ljos upgrade`.",
                src.display()
            )
        })?;
        return Ok(());
    }
    fetch(url, to)
}

/// What a failed release download tells the person to run next.
fn download_failed(url: &str, err: &str) -> String {
    format!(
        "upgrade: could not download {url}: {err}, and the network must be up or LJOS_RELEASE_DIR must name the archives, so run `ljos upgrade`."
    )
}

fn fetch(url: &str, to: &Path) -> Result<()> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|err| anyhow::anyhow!("{}", download_failed(url, &err.to_string())))?;
    let mut body = Vec::new();
    resp.into_reader()
        .take(256 * 1024 * 1024)
        .read_to_end(&mut body)
        .with_context(|| {
            download_failed(url, "the download stopped before the file was complete")
        })?;
    std::fs::write(to, body)?;
    Ok(())
}

/// The version a fixture or the forge publishes for `repo`. `None` when
/// that repository has no release. `LJOS_RELEASE_DIR` supplies
/// `{repo}.version` and stands in for the forge, so a test makes no request.
fn latest_release(repo: &str) -> Result<Option<String>> {
    if let Some(dir) = release_dir() {
        let path = dir.join(format!("{repo}.version"));
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("upgrade: reading {}", path.display()))?;
        let v = text.trim().trim_start_matches('v');
        return Ok((!v.is_empty()).then(|| v.to_string()));
    }
    let url = format!("https://api.github.com/repos/{RELEASE_OWNER}/{repo}/releases/latest");
    let resp = ureq::get(&url)
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(30))
        .call();
    let resp = match resp {
        Ok(resp) => resp,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(err) => {
            return Err(anyhow::anyhow!(
                "{}",
                unreachable_release(repo, &err.to_string())
            ));
        }
    };
    let body: serde_json::Value = resp
        .into_json()
        .with_context(|| format!("upgrade: reading {repo}'s latest release"))?;
    Ok(body["tag_name"]
        .as_str()
        .map(|t| t.trim().trim_start_matches('v').to_string())
        .filter(|t| !t.is_empty()))
}

/// One line when the forge cannot be asked for `repo`.
#[must_use]
pub fn unreachable_release(repo: &str, why: &str) -> String {
    let why = why.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "upgrade: could not reach the {repo} release ({why}), and the network must be up, so run `ljos upgrade`."
    )
}

/// The newest published ljos release's version, from the forge.
fn latest() -> Result<String> {
    latest_release("ljos")?.ok_or_else(|| {
        anyhow::anyhow!("upgrade: the latest release has no tag, so run `ljos upgrade VERSION`.")
    })
}

fn run_ok(cmd: &str, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("upgrade: running {cmd}"))?;
    if !out.status.success() {
        bail!(
            "upgrade: {cmd} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn reports(path: &Path, name: &str, version: &str) -> bool {
    run_ok(&path.display().to_string(), &["--version"])
        .ok()
        .is_some_and(|said| names_version(&said, name, version))
}

/// The semver `--version` printed, or a stand-in when the file is missing
/// or the line has no version.
fn old_version(path: &Path) -> String {
    if !path.is_file() {
        return "absent".to_string();
    }
    run_ok(&path.display().to_string(), &["--version"])
        .ok()
        .and_then(|said| crate::parse_semver(&said).map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn executable_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x7fELF") || bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe])
}

/// No release archive for this `os` and `arch`, and the command that fixes it.
fn unsupported_archive(os: &str, arch: &str) -> String {
    format!(
        "no published archive for {os} {arch}, and release archives are Linux and macOS on x86_64 and aarch64 (x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, x86_64-apple-darwin, aarch64-apple-darwin), and on Windows this runs inside WSL, so run `ljos upgrade`."
    )
}

/// Processes, other than this one, whose executable is the file at `path`.
fn pids_holding(path: &Path) -> Vec<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let Ok(meta) = std::fs::metadata(path) else {
            return Vec::new();
        };
        let dev = meta.dev();
        let ino = meta.ino();
        let me = std::process::id();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        let mut pids = Vec::new();
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if pid == me {
                continue;
            }
            let Ok(held) = std::fs::metadata(entry.path().join("exe")) else {
                continue;
            };
            if held.dev() == dev && held.ino() == ino {
                pids.push(pid);
            }
        }
        pids.sort_unstable();
        pids.dedup();
        pids
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Vec::new()
    }
}

fn running_note(name: &str, pids: &[u32]) -> String {
    if pids.is_empty() {
        return String::new();
    }
    let list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{name} is still running (pid {list}) and is the previous binary, and the assistant must be restarted (kill {list}), so run `ljos doctor`.\n"
    )
}

fn replace_installed(staged: &Path, installed: &Path, held: &[u32]) -> Result<()> {
    if let Err(err) = std::fs::rename(staged, installed) {
        let list = held
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        let next = if list.is_empty() {
            "and the process using it must stop, so run `ljos upgrade`.".to_string()
        } else {
            format!("and the process using it must stop (kill {list}), so run `ljos upgrade`.")
        };
        if err.raw_os_error() == Some(26) {
            bail!(
                "upgrade: could not replace {}: Text file busy, {next}",
                installed.display()
            );
        }
        bail!(
            "upgrade: could not replace {}: {err}, {next}",
            installed.display()
        );
    }
    Ok(())
}

fn one_line(err: &anyhow::Error) -> String {
    format!("{err:#}")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn resolve_dir(dir: Option<&Path>) -> Result<PathBuf> {
    match dir {
        Some(d) => Ok(d.to_path_buf()),
        None => Ok(std::env::current_exe()?
            .canonicalize()?
            .parent()
            .context("upgrade: the running ljos has no directory")?
            .to_path_buf()),
    }
}

fn work_dir(label: &str) -> Result<PathBuf> {
    // The cache directory, on the home filesystem: a temporary directory
    // can be a quota'd tmpfs that is already full.
    let work = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("ljos")
        .join(format!("upgrade-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    Ok(work)
}

/// Install release `version` (the newest when `None`) into `dir`.
/// `only` limits the ljos archive to that one binary.
fn upgrade_ljos(version: Option<&str>, dir: &Path, only: Option<&str>) -> Result<String> {
    let version = match version {
        Some(v) => v.trim_start_matches('v').to_string(),
        None => latest()?,
    };
    let target = match target() {
        Some(target) => target,
        None => bail!(
            "upgrade: {}",
            unsupported_archive(std::env::consts::OS, std::env::consts::ARCH)
        ),
    };
    if let Some(name) = only {
        if dir.join(name).is_file() && reports(&dir.join(name), name, &version) {
            return Ok(format!("{name} {version} current\n"));
        }
    } else if already_installed(dir, &version) {
        return Ok(format!(
            "ljos {version} is already installed at {}\n",
            dir.display()
        ));
    }
    let asset = asset_name(&version, target);
    let base = format!("https://github.com/{RELEASE_REPO}/releases/download/v{version}");
    let work = work_dir("ljos")?;
    let archive = work.join(&asset);
    let sum = work.join(format!("{asset}.sha256"));
    obtain(&asset, &archive, &format!("{base}/{asset}"))?;
    obtain(
        &format!("{asset}.sha256"),
        &sum,
        &format!("{base}/{asset}.sha256"),
    )?;
    let want = digest_of(&std::fs::read_to_string(&sum)?)
        .context("upgrade: the published checksum is not a sha256 line")?;
    let have = digest_of(&run_ok("sha256sum", &[&archive.display().to_string()])?)
        .context("upgrade: sha256sum gave no digest")?;
    if want != have {
        bail!(
            "upgrade: {asset} has sha256 {have}, the release publishes {want}; nothing installed"
        );
    }
    run_ok(
        "tar",
        &[
            "-xzf",
            &archive.display().to_string(),
            "-C",
            &work.display().to_string(),
        ],
    )?;
    let unpacked = work.join(asset.trim_end_matches(".tar.gz"));
    // Check every binary before replacing any, so a bad archive changes nothing.
    let mut ready = Vec::new();
    for name in BINARIES {
        if only.is_some_and(|want| want != *name) {
            continue;
        }
        let new = unpacked.join(name);
        let installed = dir.join(name);
        if !new.is_file() {
            if *name == "ljos-hud" && only != Some("ljos-hud") {
                continue;
            }
            bail!("upgrade: {asset} carries no {name}; nothing installed");
        }
        if *name == "ljos-hud" && !installed.exists() && only.is_none() {
            continue;
        }
        let bytes = std::fs::read(&new)?;
        if !executable_image(&bytes) {
            bail!("upgrade: {name} in {asset} is not an executable; nothing installed");
        }
        let said = run_ok(&new.display().to_string(), &["--version"])?;
        if !names_version(&said, name, &version) {
            bail!(
                "upgrade: {name} in {asset} says {:?}, not {name} {version}; nothing installed",
                said.lines().next().unwrap_or("")
            );
        }
        ready.push((*name, new, installed));
    }
    let mut out = String::new();
    for (name, new, installed) in ready {
        let old = old_version(&installed);
        keep_rollback(name, &installed, dir, &version)?;
        let staged = dir.join(format!("{name}.new"));
        std::fs::copy(&new, &staged)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
        }
        let held = pids_holding(&installed);
        replace_installed(&staged, &installed, &held)?;
        out.push_str(&running_note(name, &held));
        if only.is_some() {
            out.push_str(&format!("{name} {old} {version}\n"));
        } else {
            out.push_str(&format!(
                "installed {name} {version} at {}\n",
                installed.display()
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    if only.is_none() {
        out.push_str(&format!(
            "from {base}/{asset}, sha256 {have}; the old binaries are kept as NAME.bak-pre-{version}\n"
        ));
    }
    Ok(out)
}

/// A file named `name` anywhere under `root`, not reached through a symlink.
fn find_named(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(path);
            } else if entry.file_name() == name {
                return Some(path);
            }
        }
    }
    None
}

fn tar_flag(asset: &str) -> Result<&'static str, String> {
    if asset.ends_with(".tar.gz") {
        Ok("-xzf")
    } else if asset.ends_with(".tar.xz") {
        Ok("-xJf")
    } else {
        Err(format!("{asset} is not a tar.gz or tar.xz archive"))
    }
}

/// Download and unpack one archive. The error is one line a warning can carry.
fn fetch_tree(repo: &str, pattern: &str, version: &str, work: &Path) -> Result<PathBuf, String> {
    let target = target()
        .ok_or_else(|| unsupported_archive(std::env::consts::OS, std::env::consts::ARCH))?;
    let asset = asset_of(pattern, version, target);
    let archive = work.join(&asset);
    let sum = work.join(format!("{asset}.sha256"));
    let base = format!("https://github.com/{RELEASE_OWNER}/{repo}/releases/download/v{version}");
    obtain(&asset, &archive, &format!("{base}/{asset}")).map_err(|e| one_line(&e))?;
    obtain(
        &format!("{asset}.sha256"),
        &sum,
        &format!("{base}/{asset}.sha256"),
    )
    .map_err(|e| one_line(&e))?;
    let text =
        std::fs::read_to_string(&sum).map_err(|e| format!("reading {}: {e}", sum.display()))?;
    let want = digest_of(&text).ok_or_else(|| format!("{asset}.sha256 is not a sha256 line"))?;
    let summed =
        run_ok("sha256sum", &[&archive.display().to_string()]).map_err(|e| one_line(&e))?;
    let have = digest_of(&summed).ok_or_else(|| "sha256sum gave no digest".to_string())?;
    if want != have {
        return Err(format!(
            "checksum mismatch (archive {have}, release {want})"
        ));
    }
    let flag = tar_flag(&asset)?;
    let unpack = work.join(format!("x-{asset}"));
    let _ = std::fs::remove_dir_all(&unpack);
    std::fs::create_dir_all(&unpack).map_err(|e| e.to_string())?;
    run_ok(
        "tar",
        &[
            flag,
            &archive.display().to_string(),
            "-C",
            &unpack.display().to_string(),
        ],
    )
    .map_err(|e| one_line(&e))?;
    Ok(unpack)
}

fn place_component(dir: &Path, name: &str, version: &str, tree: &Path) -> Result<String, String> {
    let new = find_named(tree, name).ok_or_else(|| format!("the archive holds no {name}"))?;
    let bytes = std::fs::read(&new).map_err(|e| e.to_string())?;
    if !executable_image(&bytes) {
        return Err(format!("{name} in the archive is not an executable"));
    }
    let said = run_ok(&new.display().to_string(), &["--version"]).map_err(|e| one_line(&e))?;
    if !names_version(&said, name, version) {
        let got = said.lines().next().unwrap_or("").trim();
        return Err(format!(
            "{name} in the archive says {got:?}, not {name} {version}"
        ));
    }
    let installed = dir.join(name);
    let old = old_version(&installed);
    keep_rollback(name, &installed, dir, version).map_err(|e| one_line(&e))?;
    let staged = dir.join(format!("{name}.new"));
    std::fs::copy(&new, &staged).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    let held = pids_holding(&installed);
    replace_installed(&staged, &installed, &held).map_err(|e| one_line(&e))?;
    Ok(format!(
        "{name} {old} {version}\n{}",
        running_note(name, &held)
    ))
}

/// Each installed component, one line. A failure is a warning line.
fn upgrade_components(dir: &Path, only: Option<&str>) -> Result<String> {
    let mut out = String::new();
    let mut versions: HashMap<String, Result<Option<String>, String>> = HashMap::new();
    let mut trees: HashMap<(String, String), Result<PathBuf, String>> = HashMap::new();
    let mut work: Option<PathBuf> = None;
    for component in COMPONENTS {
        if only.is_some_and(|name| name != component.name) {
            continue;
        }
        if only.is_none() && !dir.join(component.name).is_file() {
            continue;
        }
        let published = if let Some(hit) = versions.get(component.repo) {
            hit.clone()
        } else {
            let got = match latest_release(component.repo) {
                Ok(v) => Ok(v),
                Err(err) => Err(one_line(&err)),
            };
            versions.insert(component.repo.to_string(), got.clone());
            got
        };
        let version = match published {
            Ok(Some(v)) => v,
            Ok(None) => {
                out.push_str(&format!("warning: {}: no release\n", component.name));
                continue;
            }
            Err(reason) => {
                out.push_str(&format!("warning: {}: {reason}\n", component.name));
                continue;
            }
        };
        let installed = dir.join(component.name);
        if installed.is_file() && reports(&installed, component.name, &version) {
            out.push_str(&format!("{} {version} current\n", component.name));
            continue;
        }
        let key = (component.repo.to_string(), component.asset.to_string());
        if !trees.contains_key(&key) {
            let fetched = (|| {
                let slot = match work {
                    Some(ref dir) => dir.clone(),
                    None => {
                        let made = work_dir("components").map_err(|e| one_line(&e))?;
                        work = Some(made.clone());
                        made
                    }
                };
                fetch_tree(component.repo, component.asset, &version, &slot)
            })();
            trees.insert(key.clone(), fetched);
        }
        match trees.get(&key) {
            Some(Ok(tree)) => match place_component(dir, component.name, &version, tree) {
                Ok(line) => out.push_str(&line),
                Err(reason) => {
                    let old = old_version(&installed);
                    out.push_str(&format!(
                        "warning: {}: {reason}; kept {old}\n",
                        component.name
                    ));
                }
            },
            Some(Err(reason)) => {
                let old = old_version(&installed);
                out.push_str(&format!(
                    "warning: {}: {reason}; kept {old}\n",
                    component.name
                ));
            }
            None => {}
        }
    }
    if let Some(dir) = &work {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok(out)
}

/// Install the ljos release `version` (the newest when `None`) and each
/// other seat program from its own latest release, into the directory of
/// the running `ljos`, or `dir`.
/// `only` moves that one program.
///
/// One line a program: `name old new` when it moved, `name version current`
/// when it already matches.
/// A component that cannot be installed prints `warning:` and the rest
/// still run.
///
/// # Errors
///
/// `--only` names nothing this installs.
/// The ljos archive has no build for this machine, or its download,
/// checksum or version does not hold.
/// A component failure is a warning, not an error.
pub fn upgrade(version: Option<&str>, dir: Option<&Path>, only: Option<&str>) -> Result<String> {
    if let Some(name) = only {
        if !is_ljos_name(name) && !is_seat_component(name) {
            bail!(
                "upgrade: --only {name} is not a component: {}",
                known_names()
            );
        }
    }
    let dir = resolve_dir(dir)?;
    let mut out = String::new();
    if only.is_none_or(is_ljos_name) {
        out.push_str(&upgrade_ljos(version, &dir, only)?);
    }
    if only.is_none_or(is_seat_component) {
        out.push_str(&upgrade_components(&dir, only)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_published_target_has_its_archive_triple() {
        assert_eq!(
            target_for("x86_64", "linux"),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(
            target_for("aarch64", "linux"),
            Some("aarch64-unknown-linux-gnu")
        );
        assert_eq!(target_for("x86_64", "macos"), Some("x86_64-apple-darwin"));
        assert_eq!(target_for("aarch64", "macos"), Some("aarch64-apple-darwin"));
    }

    #[test]
    fn the_archive_name_and_its_checks_are_the_release_workflow_s() {
        assert_eq!(
            asset_name("0.22.3", "x86_64-unknown-linux-gnu"),
            "ljos-v0.22.3-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(
            asset_name("v0.22.3", "aarch64-apple-darwin"),
            "ljos-v0.22.3-aarch64-apple-darwin.tar.gz"
        );
        let d = "d6e305288c898a34ec65e8f7c067bb680e77f49eeb041692be2e6be2e68d0f85";
        assert_eq!(
            digest_of(&format!("{d}  ljos-v0.22.3.tar.gz\n")).as_deref(),
            Some(d)
        );
        assert_eq!(digest_of("not a digest"), None);
        assert_eq!(digest_of(&d[..63]), None, "a short digest is no digest");
        assert!(names_version("ljos 0.22.3\n", "ljos", "0.22.3"));
        assert!(names_version("ljos-mcp 0.22.3", "ljos-mcp", "v0.22.3"));
        assert!(!names_version("ljos 0.22.2", "ljos", "0.22.3"));
        assert!(
            !names_version("#!/usr/bin/env python3", "ljos", "0.22.3"),
            "a script is not the release"
        );
        assert!(
            names_version("ljos-policyd 0.7.0 (phronesis)\n", "ljos-policyd", "0.7.0"),
            "a build note after the version is still that version"
        );
        assert!(names_version(
            "packset 0.17.3 (abc123)\n",
            "packset",
            "v0.17.3"
        ));
        assert!(
            !names_version("ljos 0.22.30\n", "ljos", "0.22.3"),
            "a longer version is a different version"
        );
        let deedar = COMPONENTS.iter().find(|c| c.name == "deedar").unwrap();
        assert_eq!(
            asset_of(deedar.asset, "0.6.1", "x86_64-unknown-linux-gnu"),
            "deedar-v0.6.1-x86_64-unknown-linux-gnu.tar.gz"
        );
        let vissue = COMPONENTS.iter().find(|c| c.name == "vissue").unwrap();
        assert_eq!(
            asset_of(vissue.asset, "0.23.1", "x86_64-unknown-linux-gnu"),
            "vissue-v0.23.1-x86_64-unknown-linux-gnu.tar.gz"
        );
        let consensus = COMPONENTS
            .iter()
            .find(|c| c.name == "ljos-consensus")
            .unwrap();
        assert_eq!(
            asset_of(consensus.asset, "v0.9.0", "aarch64-apple-darwin"),
            "consensus-v0.9.0-aarch64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn an_unsupported_machine_names_the_archive_and_the_next_command() {
        let text = unsupported_archive("linux", "riscv64");
        assert!(
            text.contains("no published archive for linux riscv64"),
            "{text}"
        );
        assert!(text.contains("x86_64-unknown-linux-gnu"), "{text}");
        assert!(text.contains("aarch64-apple-darwin"), "{text}");
        assert!(
            text.contains("inside WSL, so run `ljos upgrade`."),
            "{text}"
        );
    }

    #[test]
    fn a_failed_download_names_the_retry() {
        let text = download_failed("https://example.invalid/ljos.tar.gz", "dns error");
        assert!(
            text.contains("could not download https://example.invalid/ljos.tar.gz: dns error"),
            "{text}"
        );
        assert!(text.contains("so run `ljos upgrade`."), "{text}");
        assert!(text.contains("LJOS_RELEASE_DIR"), "{text}");
    }
}
