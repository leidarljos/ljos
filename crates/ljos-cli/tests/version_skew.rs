//! Two ljos binaries on one seat. Doctor names the skew. A hook does not.
//! A match is silent. Doctor does not rewrite a script it did not write.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn script(path: &Path, version: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(
        path,
        format!("#!/bin/sh\nprintf '%s\\n' 'ljos {version}'\n"),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn real_version() -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("--version")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .find(|word| word.split('.').count() == 3)
        .unwrap_or_else(|| panic!("no version in {text}"))
        .to_string()
}

struct Seat {
    root: PathBuf,
    path_ljos: PathBuf,
    bin_ljos: PathBuf,
    hooks: PathBuf,
    placeholder: PathBuf,
    path_body: String,
    hooks_body: String,
}

impl Seat {
    fn new(path_version: &str, bin_version: &str, hook_version: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ljos-skew-{}-{}",
            std::process::id(),
            path_version.replace('.', "")
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path_ljos = root.join("path-bin/ljos");
        let bin_ljos = root.join("seat-bin/ljos");
        let hook_ljos = root.join("hook-bin/ljos");
        script(&path_ljos, path_version);
        script(&bin_ljos, bin_version);
        script(&hook_ljos, hook_version);
        let hooks = root.join("hooks.json");
        std::fs::write(
            &hooks,
            format!(
                "{{\"hooks\":{{\"UserPromptSubmit\":[{{\"hooks\":[{{\"type\":\"command\",\"command\":\"{} hook\"}}]}}]}}}}\n",
                hook_ljos.display()
            ),
        )
        .unwrap();
        let placeholder = root.join("placeholder");
        std::fs::write(&placeholder, "leave this placeholder\n").unwrap();
        let config = root.join("config/ljos");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("harnesses.toml"),
            format!(
                "[[harness]]\nname = \"box\"\nhooks = \"{}\"\n",
                hooks.display()
            ),
        )
        .unwrap();
        let path_body = std::fs::read_to_string(&path_ljos).unwrap();
        let hooks_body = std::fs::read_to_string(&hooks).unwrap();
        Self {
            root,
            path_ljos,
            bin_ljos,
            hooks,
            placeholder,
            path_body,
            hooks_body,
        }
    }

    fn doctor(&self, all: bool) -> (i32, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.arg("doctor");
        if all {
            cmd.arg("--all");
        }
        let out = cmd
            .env_clear()
            .env("HOME", &self.root)
            .env("USER", "person")
            .env(
                "PATH",
                format!(
                    "{}:/usr/bin:/bin",
                    self.path_ljos.parent().unwrap().display()
                ),
            )
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("VISSUE_ROOT", self.root.join("tracker"))
            .env("LJOS_BIN", &self.bin_ljos)
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }

    fn untouched(&self) {
        assert_eq!(
            std::fs::read_to_string(&self.path_ljos).unwrap(),
            self.path_body
        );
        assert_eq!(
            std::fs::read_to_string(&self.hooks).unwrap(),
            self.hooks_body
        );
        assert_eq!(
            std::fs::read_to_string(&self.placeholder).unwrap(),
            "leave this placeholder\n"
        );
        assert!(std::fs::read_to_string(&self.bin_ljos)
            .unwrap()
            .starts_with("#!/bin/sh\n"));
    }
}

#[test]
fn a_mismatch_is_a_label_and_a_match_is_silent() {
    let version = real_version();
    let mismatch = Seat::new("0.35.8", &version, "0.36.1");
    let (code, text) = mismatch.doctor(false);
    let (all_code, all) = mismatch.doctor(true);
    assert_eq!(
        code, all_code,
        "skew changed the doctor exit\n{text}\n{all}"
    );
    for body in [&text, &all] {
        assert!(
            body.contains("label version-skew:"),
            "missing the label:\n{body}"
        );
        assert!(body.contains("0.35.8"), "{body}");
        assert!(body.contains("0.36.1"), "{body}");
        assert!(
            body.contains(&format!("hooks run {}", mismatch.bin_ljos.display())),
            "{body}"
        );
        assert!(
            body.contains("put the new binary first on PATH, or point LJOS_BIN at it"),
            "{body}"
        );
        assert!(body.contains("warn\tversion skew\t"), "{body}");
        assert!(!body.contains("no\tversion skew\t"), "{body}");
    }
    let lines = text
        .lines()
        .filter(|line| line.contains("label version-skew:"))
        .count();
    assert_eq!(lines, 2, "one line per mismatch:\n{text}");
    mismatch.untouched();

    let prompt = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["hook", "--prompt", "--text"])
        .env_clear()
        .env("HOME", &mismatch.root)
        .env(
            "PATH",
            format!(
                "{}:/usr/bin:/bin",
                mismatch.path_ljos.parent().unwrap().display()
            ),
        )
        .env("XDG_CONFIG_HOME", mismatch.root.join("config"))
        .env("LJOS_BIN", &mismatch.bin_ljos)
        .env("PACKSET_URL", "off")
        .output()
        .unwrap();
    let heard = format!(
        "{}{}",
        String::from_utf8_lossy(&prompt.stdout),
        String::from_utf8_lossy(&prompt.stderr)
    );
    assert!(
        !heard.contains("version-skew"),
        "the hook named the skew:\n{heard}"
    );

    let same = Seat::new(&version, &version, &version);
    let (_, matched) = same.doctor(false);
    let (_, matched_all) = same.doctor(true);
    assert!(!matched.contains("version-skew"), "{matched}");
    assert!(!matched_all.contains("version-skew"), "{matched_all}");
    same.untouched();
}
