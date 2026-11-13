//! A test asks for `age-keygen`, `packsetd` or `vissue` before it runs.
//!
//! A missing tool prints one line and the test returns. The line names the
//! tool, the version and the one command that installs it.
//! `LJOS_TEST_REQUIRE_TOOLS=1` fails the test with that same line, so a run
//! that set the variable cannot pass a test it did not run.
//!
//! `vissue` is the row in `.github/sibling-pin`. `packsetd` is the version
//! and stamp in `scripts/sibling-tools.sh`. `age-keygen` 1.1.1 is the `age`
//! package the cargo test job installs on Ubuntu 24.04. A newer age-keygen
//! still runs the test.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Set to `1` when a missing tool must fail the test.
pub const REQUIRE_TOOLS: &str = "LJOS_TEST_REQUIRE_TOOLS";

/// `age` on Ubuntu 24.04. The package ships `age` and `age-keygen`.
const AGE_VERSION: &str = "1.1.1";

const AGE_COMMAND: &str = "apt-get install age";
const SIBLING_COMMAND: &str = "source scripts/sibling-tools.sh";

/// One tool a test runs, and does not stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// `age-keygen` from the `age` package.
    AgeKeygen,
    /// `packsetd` at the commit `scripts/sibling-tools.sh` builds.
    Packsetd,
    /// `vissue` at the commit `.github/sibling-pin` names.
    Vissue,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::AgeKeygen => "age-keygen",
            Self::Packsetd => "packsetd",
            Self::Vissue => "vissue",
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::AgeKeygen => AGE_COMMAND,
            Self::Packsetd | Self::Vissue => SIBLING_COMMAND,
        }
    }

    /// The version the line names. `packsetd` includes the eight-digit stamp
    /// its `--version` line prints after the number.
    fn version(self) -> String {
        match self {
            Self::AgeKeygen => AGE_VERSION.to_string(),
            Self::Packsetd => {
                let pin = pins();
                format!("{} ({})", pin.packset_version, pin.packset_stamp)
            }
            Self::Vissue => pins().vissue_version.clone(),
        }
    }

    /// `true` when `--version` has to be the whole first line.
    fn check_version(self) -> bool {
        !matches!(self, Self::AgeKeygen)
    }
}

struct Pins {
    vissue_version: String,
    packset_version: String,
    packset_stamp: String,
}

fn pins() -> &'static Pins {
    static PINS: OnceLock<Pins> = OnceLock::new();
    PINS.get_or_init(read_pins)
}

fn repo_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn read_pins() -> Pins {
    let pin_path = repo_file(".github/sibling-pin");
    let pin_text = std::fs::read_to_string(&pin_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", pin_path.display()));
    let script_path = repo_file("scripts/sibling-tools.sh");
    let script_text = std::fs::read_to_string(&script_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", script_path.display()));
    Pins {
        vissue_version: vissue_version(&pin_text),
        packset_version: assignment(&script_text, "packset_version"),
        packset_stamp: assignment(&script_text, "packset_stamp"),
    }
}

fn vissue_version(text: &str) -> String {
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        if fields.next() != Some("vissue") {
            continue;
        }
        let repo = fields.next();
        let commit = fields.next();
        let version = fields.next();
        if repo.is_none() || commit.is_none() || version.is_none() || fields.next().is_some() {
            panic!(
                ".github/sibling-pin:{}: want name repository commit version",
                idx + 1
            );
        }
        return version.unwrap().to_string();
    }
    panic!("no vissue row in .github/sibling-pin");
}

fn assignment(text: &str, key: &str) -> String {
    let prefix = format!("{key}=");
    for raw in text.lines() {
        let line = raw.trim();
        let Some(value) = line.strip_prefix(&prefix) else {
            continue;
        };
        if !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
            return value.to_string();
        }
    }
    panic!("scripts/sibling-tools.sh has no {key}");
}

/// What the test does once it has asked for the tool.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    /// The tool is on `path` at the version the test runs.
    Run,
    /// Print `line` and return. The test did not fail.
    Skip(String),
    /// Fail the test with `line`.
    Fail(String),
}

/// `require_tools` is the value of [`REQUIRE_TOOLS`]. `Some("1")` fails a
/// missing tool. Any other value skips it.
fn consider(path: &OsStr, tool: Tool, require_tools: Option<&str>) -> Action {
    match look(path, tool) {
        Ok(()) => Action::Run,
        Err(line) if require_tools == Some("1") => Action::Fail(line),
        Err(line) => Action::Skip(line),
    }
}

fn look(path: &OsStr, tool: Tool) -> Result<(), String> {
    let found = std::env::split_paths(path).find_map(|dir| {
        let bin = dir.join(tool.name());
        bin.is_file().then_some(bin)
    });
    let Some(bin) = found else {
        return Err(line(tool, None));
    };
    if !tool.check_version() {
        return Ok(());
    }
    let shown = version_of(&bin, path);
    let want = format!("{} {}", tool.name(), tool.version());
    if shown.as_deref() == Some(want.as_str()) {
        return Ok(());
    }
    let found = shown.unwrap_or_else(|| "no version".to_string());
    Err(line(tool, Some(&found)))
}

fn version_of(bin: &Path, path: &OsStr) -> Option<String> {
    let output = Command::new(bin)
        .arg("--version")
        .env("PATH", path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let shown = text.lines().next().unwrap_or("").trim();
    if shown.is_empty() {
        None
    } else {
        Some(shown.to_string())
    }
}

fn line(tool: Tool, found: Option<&str>) -> String {
    let mut text = format!("{} {} is not on PATH", tool.name(), tool.version());
    if let Some(found) = found {
        text.push_str(&format!(" (found {found})"));
    }
    format!("{text}, so run `{}`.", tool.command())
}

/// True when the caller should return.
///
/// A missing tool prints one line on stderr. [`REQUIRE_TOOLS`] `=1` panics
/// with that line instead.
#[must_use]
pub fn halt(tool: Tool) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let require = std::env::var(REQUIRE_TOOLS).ok();
    act(tool, &path, require.as_deref())
}

fn act(tool: Tool, path: &OsStr, require_tools: Option<&str>) -> bool {
    match consider(path, tool, require_tools) {
        Action::Run => false,
        Action::Skip(line) => {
            eprintln!("{line}");
            true
        }
        Action::Fail(line) => panic!("{line}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stand_in(dir: &std::path::Path, name: &str, body: &str) {
        use std::io::Write;
        let path = dir.join(name);
        let tmp = dir.join(format!(".{name}.new"));
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg("/bin/cat > \"$1\" && /bin/chmod 755 \"$1\"")
            .arg("stand-in")
            .arg(&tmp)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        let mut stdin = child.stdin.take().expect("stdin");
        stdin
            .write_all(body.as_bytes())
            .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        drop(stdin);
        let out = child
            .wait_with_output()
            .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        assert!(
            out.status.success(),
            "write {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::rename(&tmp, &path).unwrap_or_else(|e| panic!("install {}: {e}", path.display()));
    }

    fn skip_line(tool: Tool) -> String {
        match consider(OsStr::new("/no/such/tools"), tool, None) {
            Action::Skip(line) => line,
            other => panic!("{tool:?} did not skip: {other:?}"),
        }
    }

    #[test]
    fn a_missing_tool_skips() {
        for tool in [Tool::AgeKeygen, Tool::Packsetd, Tool::Vissue] {
            let line = skip_line(tool);
            assert_eq!(line.lines().count(), 1, "{line}");
            assert!(line.contains(tool.name()), "{line}");
            assert!(line.contains(&tool.version()), "{line}");
            assert!(line.contains(tool.command()), "{line}");
            assert!(line.contains(", so run `"), "{line}");
            assert!(line.ends_with("`."), "{line}");
            assert!(
                act(tool, OsStr::new("/no/such/tools"), None),
                "{tool:?} ran"
            );
        }
        let dir = tempfile::tempdir().unwrap();
        stand_in(dir.path(), "vissue", "#!/bin/sh\necho 'vissue 0.0.0'\n");
        let action = consider(dir.path().as_os_str(), Tool::Vissue, None);
        let Action::Skip(line) = action else {
            panic!("a different vissue ran: {action:?}");
        };
        assert!(line.contains("(found vissue 0.0.0)"), "{line}");
        assert!(line.contains(&Tool::Vissue.version()), "{line}");
        assert_eq!(line.lines().count(), 1, "{line}");
    }

    #[test]
    fn a_missing_tool_fails_when_ljos_test_require_tools_is_1() {
        let path = OsStr::new("/no/such/tools");
        let line = skip_line(Tool::AgeKeygen);
        assert_eq!(
            consider(path, Tool::AgeKeygen, Some("1")),
            Action::Fail(line.clone())
        );
        assert!(matches!(
            consider(path, Tool::AgeKeygen, Some("0")),
            Action::Skip(_)
        ));
        assert!(matches!(
            consider(path, Tool::AgeKeygen, None),
            Action::Skip(_)
        ));
        let caught = std::panic::catch_unwind(|| act(Tool::AgeKeygen, path, Some("1")));
        let payload = caught.expect_err("a missing tool ran");
        let text = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert_eq!(text, line);
    }

    #[test]
    fn a_present_tool_runs() {
        let dir = tempfile::tempdir().unwrap();
        let vissue = format!("#!/bin/sh\necho 'vissue {}'\n", Tool::Vissue.version());
        let packsetd = format!("#!/bin/sh\necho 'packsetd {}'\n", Tool::Packsetd.version());
        stand_in(dir.path(), "vissue", &vissue);
        stand_in(dir.path(), "packsetd", &packsetd);
        stand_in(dir.path(), "age-keygen", "#!/bin/sh\nexit 0\n");
        let path = dir.path().as_os_str();
        for tool in [Tool::AgeKeygen, Tool::Packsetd, Tool::Vissue] {
            assert_eq!(consider(path, tool, None), Action::Run, "{tool:?}");
            assert_eq!(consider(path, tool, Some("1")), Action::Run, "{tool:?}");
            assert!(!act(tool, path, Some("1")), "{tool:?} did not run");
        }
    }
}
