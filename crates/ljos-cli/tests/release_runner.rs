//! The release binary has no way to turn runner detection off.
//!
//! A test harness may leave a host runner cgroup. The binary it builds
//! must not. A copy whose path contains `/deps/` is still the product,
//! and a runner cgroup around that copy still refuses a person's keep.

ljos_test_boot::harness!();

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const BYPASS_NEEDLES: &[&[u8]] = &[
    b"ljos_test_enter_cgroup",
    b"ljos_test_session_cgroup",
    b"ljos test: could not leave host cgroup",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn release_dir() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir).join("release"),
        None => workspace_root().join("target").join("release"),
    }
}

fn release_bin(name: &str) -> PathBuf {
    let bin = release_dir().join(name);
    assert!(
        bin.is_file(),
        "release binary {} is missing; build it with cargo build --release --locked -p ljos --bins",
        bin.display()
    );
    bin
}

fn contains_needle(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|window| window == needle)
}

fn assert_no_bypass(bin: &Path) {
    let bytes = fs::read(bin).unwrap_or_else(|err| panic!("read {}: {err}", bin.display()));
    for needle in BYPASS_NEEDLES {
        let text = String::from_utf8_lossy(needle);
        assert!(
            !contains_needle(&bytes, needle),
            "{} contains {text}, a test-only runner bypass",
            bin.display()
        );
    }
}

/// `ljos hook` blocks in admission while stdin stays open, so the cgroup
/// can be read after load-time constructors would have run. Stderr goes to
/// a file: a pipe would fill, or a read would wait for the child to exit,
/// and the child waits on the stdin this function is holding.
fn cgroup_while_hook_blocks(bin: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let err_file = tempfile::NamedTempFile::new().unwrap();
    let mut child = Command::new(bin)
        .arg("hook")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(err_file.reopen().unwrap()))
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {}: {err}", bin.display()));
    let hold = child.stdin.take();
    let pid = child.id();
    let mut saw = None;
    for _ in 0..40 {
        if let Ok(text) = fs::read_to_string(format!("/proc/{pid}/cgroup")) {
            saw = Some(text);
            let cmd = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            if cmd.windows(4).any(|window| window == b"hook") {
                break;
            }
        }
        if child.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(200));
    let after = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok();
    drop(hold);
    let _ = child.kill();
    let _ = child.wait();
    let err = fs::read_to_string(err_file.path()).unwrap_or_default();
    after.or(saw).unwrap_or_else(|| {
        panic!(
            "{} exited before its cgroup could be read\n{err}",
            bin.display()
        )
    })
}

fn assert_deps_copy_stays_a_runner(bin: &Path) {
    let dir = tempfile::tempdir().unwrap();
    let deps = dir.path().join("deps");
    fs::create_dir_all(&deps).unwrap();
    let copy = deps.join("ljos");
    fs::copy(bin, &copy).unwrap();
    fs::set_permissions(&copy, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        copy.to_string_lossy().contains("/deps/"),
        "copy path {} has no /deps/ segment",
        copy.display()
    );
    let out = dir.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let cg = format!("/sys/fs/cgroup/claude-probe-{}", std::process::id());
    let script = r#"
set -euo pipefail
cg="$1"
copy="$2"
home="$3"
out="$4"
sudo -n mkdir -p "$cg"
echo $$ | sudo -n tee "$cg/cgroup.procs" >/dev/null
cat /proc/self/cgroup > "$out/shell.cgroup"
fifo="$out/fifo"
rm -f "$fifo"
mkfifo "$fifo"
sleep 30 > "$fifo" &
writer=$!
env -i PATH=/usr/bin:/bin HOME="$home" "$copy" hook >"$out/hook.out" 2>"$out/hook.err" < "$fifo" &
hp=$!
ok=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  if [ -r "/proc/$hp/cmdline" ]; then
    cmd=$(tr '\0' ' ' < "/proc/$hp/cmdline" || true)
    case "$cmd" in
      *hook*) ok=1; break ;;
    esac
  fi
  sleep 0.05
done
if [ "$ok" -ne 1 ]; then
  echo "hook copy did not start" >&2
  exit 1
fi
sleep 0.2
if [ -r "/proc/$hp/cgroup" ]; then
  cat "/proc/$hp/cgroup" > "$out/hook.cgroup"
else
  echo missing > "$out/hook.cgroup"
fi
if [ -r "/proc/$hp/cmdline" ]; then
  tr '\0' ' ' < "/proc/$hp/cmdline" > "$out/hook.cmd" || true
fi
kill "$hp" 2>/dev/null || true
kill "$writer" 2>/dev/null || true
wait "$hp" 2>/dev/null || true
wait "$writer" 2>/dev/null || true
set +e
env -i PATH=/usr/bin:/bin HOME="$home" "$copy" prefer --review --keep deadbeef >"$out/keep.out" 2>"$out/keep.err"
echo $? > "$out/keep.code"
exit 0
"#;
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let run = Command::new("bash")
        .args([
            "-c",
            script,
            "probe",
            &cg,
            &copy.to_string_lossy(),
            &home.to_string_lossy(),
            &out.to_string_lossy(),
        ])
        .output()
        .unwrap_or_else(|err| panic!("bash probe: {err}"));
    let dumped = format!(
        "status {}\nstdout:\n{}\nstderr:\n{}\nshell:\n{}\nhook cgroup:\n{}\nhook cmd:\n{}\nhook err:\n{}\nkeep err:\n{}\nkeep code:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
        fs::read_to_string(out.join("shell.cgroup")).unwrap_or_default(),
        fs::read_to_string(out.join("hook.cgroup")).unwrap_or_default(),
        fs::read_to_string(out.join("hook.cmd")).unwrap_or_default(),
        fs::read_to_string(out.join("hook.err")).unwrap_or_default(),
        fs::read_to_string(out.join("keep.err")).unwrap_or_default(),
        fs::read_to_string(out.join("keep.code")).unwrap_or_default(),
    );
    assert!(run.status.success(), "probe script failed\n{dumped}");
    let shell_cg = fs::read_to_string(out.join("shell.cgroup")).unwrap_or_default();
    let hook_cg = fs::read_to_string(out.join("hook.cgroup")).unwrap_or_default();
    assert!(
        shell_cg.contains("claude-probe-"),
        "probe shell never entered a runner cgroup\n{dumped}"
    );
    assert!(
        hook_cg.contains("claude-probe-"),
        "deps copy left the runner cgroup\n{dumped}"
    );
    let keep_err = fs::read_to_string(out.join("keep.err")).unwrap_or_default();
    assert!(
        keep_err.contains("a runner cannot keep"),
        "deps copy was not refused as a runner\n{dumped}"
    );
    let _ = Command::new("sudo").args(["-n", "rmdir", &cg]).output();
}

#[test]
fn release_ljos_does_not_leave_a_runner_cgroup() {
    let ljos = release_bin("ljos");
    let mcp = release_bin("ljos-mcp");
    assert_no_bypass(&ljos);
    assert_no_bypass(&mcp);
    let parent = fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    assert!(
        !parent
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| { word.len() >= 4 && ["cursor", "claude", "codex"].contains(&word) }),
        "the test process is still in a runner cgroup: {parent}"
    );
    let child = cgroup_while_hook_blocks(&ljos);
    assert_eq!(
        parent.trim(),
        child.trim(),
        "release ljos left its cgroup\nparent {parent}\nchild {child}"
    );
    assert_deps_copy_stays_a_runner(&ljos);
}
