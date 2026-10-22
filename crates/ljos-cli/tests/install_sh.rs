//! `scripts/install.sh` on a fresh home, from a directory of programs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PROGRAMS: [&str; 11] = [
    "ljos",
    "ljos-mcp",
    "packset",
    "packsetd",
    "packset-mcp",
    "vissue",
    "vissue-mcp",
    "deedar",
    "claimdag",
    "ljos-policyd",
    "ljos-consensus",
];

const PATH_LINE: &str = "export PATH=\"$HOME/.local/bin:$PATH\"";
const MARK: &str = "# Added by the ljos installer: its programs on PATH.";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn install_sh() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh")
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Install the stand-ins in `progs` into `home`'s `~/.local/bin`, with
/// `path` as the caller's PATH.
fn install(home: &Path, progs: &Path, path: &str, extra: &[&str]) -> String {
    std::fs::create_dir_all(home).unwrap();
    let out = Command::new("sh")
        .arg(install_sh())
        .arg("--from-dir")
        .arg(progs)
        .arg("--bin-dir")
        .arg(home.join(".local/bin"))
        .args(["--no-onboard", "--no-embed"])
        .args(extra)
        .env_clear()
        .env("HOME", home)
        .env("PATH", path)
        .output()
        .unwrap();
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    text
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn install_puts_the_bin_dir_on_path_in_each_profile_once() {
    let dir = tempfile::tempdir().unwrap();
    let progs = dir.path().join("progs");
    std::fs::create_dir_all(&progs).unwrap();
    for name in PROGRAMS {
        script(&progs, name, "#!/bin/sh\nexit 0\n");
    }

    let home = dir.path().join("zsh-user");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join(".zshrc"), "# mine\n").unwrap();
    let first = install(&home, &progs, "/usr/bin:/bin", &[]);
    assert!(
        first.contains("to PATH in ~/.profile ~/.bashrc ~/.zshrc"),
        "{first}"
    );
    assert!(first.contains(PATH_LINE), "{first}");
    let again = install(&home, &progs, "/usr/bin:/bin", &[]);
    assert!(again.contains("already add"), "{again}");
    for rc in [".profile", ".bashrc", ".zshrc"] {
        let text = read(&home.join(rc));
        assert_eq!(text.matches(MARK).count(), 1, "{rc}:\n{text}");
        assert_eq!(text.matches(PATH_LINE).count(), 1, "{rc}:\n{text}");
    }
    assert!(read(&home.join(".zshrc")).starts_with("# mine\n\n"));
    let found = Command::new("sh")
        .args(["-c", ". \"$HOME/.profile\"; command -v ljos"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&found.stdout).trim(),
        home.join(".local/bin/ljos").display().to_string(),
        "{}",
        said(&found)
    );

    let bash_only = dir.path().join("bash-user");
    install(&bash_only, &progs, "/usr/bin:/bin", &[]);
    assert!(read(&bash_only.join(".bashrc")).contains(PATH_LINE));
    assert!(
        !bash_only.join(".zshrc").exists(),
        "a home without ~/.zshrc got one"
    );

    let opted_out = dir.path().join("opted-out");
    let note = install(&opted_out, &progs, "/usr/bin:/bin", &["--no-modify-path"]);
    assert!(note.contains("not on PATH"), "{note}");
    assert!(note.contains(PATH_LINE), "{note}");
    for rc in [".profile", ".bashrc"] {
        assert!(!opted_out.join(rc).exists(), "--no-modify-path wrote {rc}");
    }

    let on_path = dir.path().join("on-path");
    let path = format!("{}:/usr/bin:/bin", on_path.join(".local/bin").display());
    let quiet = install(&on_path, &progs, &path, &[]);
    assert!(!quiet.contains(PATH_LINE), "{quiet}");
    for rc in [".profile", ".bashrc"] {
        assert!(!on_path.join(rc).exists(), "a bin dir on PATH wrote {rc}");
    }
}
