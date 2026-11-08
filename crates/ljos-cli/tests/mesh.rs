//! `ljos mesh up`, `ljos mesh down` and `ljos mesh acl` with a fake
//! `tailscale` and `tailscaled` on PATH. No network and no real daemon.

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const EXAMPLE: &str = include_str!("../../../examples/mesh.toml");
const KEY: &str = "tskey-auth-kQ7x2NeverShown0123456789abcdef";
const HOST: &str = "m6be-test";
const LOGIN: &str = "https://hs.grund.rgoswami.me";
/// Mesh addresses the fake `tailscale status --json` reports for this node.
const SELF_V4: &str = "100.64.0.8";
const SELF_V6: &str = "fd7a:115c:a1e0::8";
/// One other node on that status, name then its mesh addresses.
const PEER: &str = "other-lane";
const PEER_V4: &str = "100.64.0.9";
const PEER_V6: &str = "fd7a:115c:a1e0::9";

/// The bound on one mesh verb, `WAIT_SECS` in src/mesh.rs.
const WAIT_SECS: u64 = 30;
/// The step between looks at a process, `POLL` in src/mesh.rs: half the
/// 0.1 s a person reads as no delay (Nielsen, Usability Engineering,
/// 1993, §5.5).
const POLL: Duration = Duration::from_millis(100 / 2);
/// How long a fake tailscaled, or a fake tailscale that hangs, runs when
/// nothing stops it. `down` or the bound on `up` stops each one first, so
/// this only ends a fake that a failed test left behind. Twenty verbs at
/// `WAIT_SECS` each is more than any test here runs.
const FAKE_LIFE_SECS: u64 = 20 * WAIT_SECS;

/// Records each start, dumps its environment into its log as a crash
/// report would, opens its socket, and stays up. `FAKE_STUBBORN` ignores
/// SIGTERM.
const TAILSCALED: &str = r#"#!/bin/sh
[ -n "$FAKE_STUBBORN" ] && trap '' TERM
printf '%s\n' "$*" >> "$FAKE_MESH/daemon-args"
for a in "$@"; do case "$a" in --socket=*) sock="${a#--socket=}";; esac; done
echo "tailscaled $*"
env
echo daemon >> "$FAKE_MESH/daemons"
echo $$ > "$FAKE_MESH/daemon.pid"
: > "$sock"
exec sleep "$FAKE_LIFE"
"#;

/// The login server's side of a join, kept in `$FAKE_MESH`. `FAKE_ECHO`
/// repeats the key the file holds; `FAKE_REJECT` refuses it; `FAKE_HANG`
/// never answers.
const TAILSCALE: &str = r#"#!/bin/sh
verb=; host=; key=
for a in "$@"; do
  case "$a" in
    --auth-key=file:*) key=$(cat "${a#--auth-key=file:}" 2>/dev/null);;
    --auth-key=*) key=${a#--auth-key=};;
    --hostname=*) host=${a#--hostname=};;
    status|up|logout) [ -z "$verb" ] && verb="$a";;
  esac
done
printf '%s\n' "$*" >> "$FAKE_MESH/invocations"
[ -n "$FAKE_HANG" ] && exec sleep "$FAKE_LIFE"
case "$verb" in
status)
  if [ -f "$FAKE_MESH/status.json" ]; then
    cat "$FAKE_MESH/status.json"
  elif [ -f "$FAKE_MESH/node" ]; then
    name=$(cat "$FAKE_MESH/node")
    case " $* " in
      *" --json "*) printf '{"BackendState":"Running","Self":{"HostName":"%s","TailscaleIPs":["100.64.0.8","fd7a:115c:a1e0::8"]},"Peer":{"peer1":{"HostName":"other-lane","TailscaleIPs":["100.64.0.9","fd7a:115c:a1e0::9"]}}}\n' "$name";;
      *) printf '%s running\n' "$name";;
    esac
  else
    case " $* " in
      *" --json "*) printf '{"BackendState":"NeedsLogin","Self":{"HostName":""}}\n';;
      *) printf 'Logged out.\n';;
    esac
  fi ;;
up)
  if [ -n "$FAKE_ECHO" ]; then printf '%s\n' "$key" >&2; fi
  if [ -n "$FAKE_REJECT" ]; then printf 'backend error: invalid key: %s\n' "$key" >&2; exit 1; fi
  [ -n "$key" ] || exit 1
  echo up >> "$FAKE_MESH/joins"
  printf '%s' "$host" > "$FAKE_MESH/node" ;;
logout)
  rm -f "$FAKE_MESH/node"
  echo logout >> "$FAKE_MESH/logouts" ;;
esac
"#;

/// Write the script, sync it, and close the handle before anything execs it.
fn write_program(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(body.as_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

struct World {
    root: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["bin", "empty", "home/.config/ljos", "run", "fake"] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        write_program(&root.path().join("bin/tailscaled"), TAILSCALED);
        write_program(&root.path().join("bin/tailscale"), TAILSCALE);
        std::fs::write(root.path().join("home/.config/ljos/mesh.toml"), EXAMPLE).unwrap();
        std::fs::write(root.path().join("home/mesh.key"), KEY).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.path().join("home/mesh.key"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        Self { root }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    fn key(&self) -> PathBuf {
        self.path("home/mesh.key")
    }

    fn state(&self) -> PathBuf {
        self.path("home/.state/ljos/mesh")
    }

    fn ljos(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(self.path("home"))
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.path("bin").display()),
            )
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("home/.config"))
            .env("XDG_STATE_HOME", self.path("home/.state"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .env("FAKE_MESH", self.path("fake"))
            .env("FAKE_LIFE", FAKE_LIFE_SECS.to_string())
            .env("LJOS_USER_RULES_SYNC", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn up_cmd(&self) -> Command {
        let key = self.key();
        let mut cmd = self.ljos(&["mesh", "up", "--key-file"]);
        cmd.arg(&key).args(["--hostname", HOST]);
        cmd
    }

    fn up(&self) -> Output {
        self.up_cmd().output().unwrap()
    }

    fn down(&self) -> Output {
        self.ljos(&["mesh", "down"]).output().unwrap()
    }

    fn count(&self, name: &str) -> usize {
        std::fs::read_to_string(self.path("fake").join(name)).map_or(0, |text| text.lines().count())
    }

    fn joined_node(&self) -> String {
        std::fs::read_to_string(self.path("fake/node")).unwrap_or_default()
    }

    fn daemon_pid(&self) -> Option<String> {
        std::fs::read_to_string(self.path("fake/daemon.pid"))
            .ok()
            .map(|pid| pid.trim().to_string())
    }

    fn recorded(&self, name: &str) -> String {
        std::fs::read_to_string(self.path("fake").join(name)).unwrap_or_default()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = self.down();
    }
}

fn text_of(out: &Output) -> String {
    format!(
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The one line a mesh verb printed, after asserting it printed one and exited 0.
fn one_line(out: &Output, what: &str) -> String {
    assert!(out.status.success(), "{what}\n{}", text_of(out));
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<&str> = all.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "{what}: not one line\n{}", text_of(out));
    lines[0].to_string()
}

fn gone(pid: &str) -> bool {
    !Path::new("/proc").join(pid).exists()
}

/// Every file under `dir` whose bytes hold `needle`.
fn files_holding(dir: &Path, needle: &[u8]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            found.extend(files_holding(&path, needle));
        } else if kind.is_file() {
            let bytes = std::fs::read(&path).unwrap_or_default();
            if bytes.windows(needle.len()).any(|w| w == needle) {
                found.push(path);
            }
        }
    }
    found
}

/// A successful `mesh up` line: the node name, both mesh addresses, and the peer.
fn status_line_ok(line: &str) -> bool {
    line.starts_with("mesh up: ")
        && line.contains(HOST)
        && line.contains(SELF_V4)
        && line.contains(SELF_V6)
        && line.contains(PEER)
        && line.contains(PEER_V4)
        && line.contains(PEER_V6)
}

fn up_invocation(world: &World) -> String {
    world
        .recorded("invocations")
        .lines()
        .find(|line| line.split_whitespace().any(|arg| arg == "up"))
        .unwrap_or_else(|| panic!("no up invocation\n{}", world.recorded("invocations")))
        .to_string()
}

#[test]
fn a_successful_join_prints_the_node_its_addresses_and_its_peers() {
    let world = World::new();
    let line = one_line(&world.up(), "up");
    assert!(status_line_ok(&line), "{line}");
    let peers = line.split(" peer ").nth(1).unwrap_or("");
    assert!(
        peers.contains(PEER) && peers.contains(PEER_V4) && peers.contains(PEER_V6),
        "{line}"
    );
    assert!(
        !peers.contains(SELF_V4),
        "the node address was listed as a peer: {line}"
    );
}

#[test]
fn second_up_prints_status_and_never_advertises_tags() {
    let world = World::new();
    let first = one_line(&world.up(), "first up");
    assert_eq!(world.joined_node(), HOST);
    assert!(status_line_ok(&first), "{first}");
    let up = up_invocation(&world);
    for flag in [
        "--accept-routes=false",
        "--accept-dns=false",
        &format!("--login-server={LOGIN}"),
        &format!("--auth-key=file:{}", world.key().display()),
        &format!("--hostname={HOST}"),
        "--reset",
    ] {
        assert!(up.contains(flag), "{flag} missing from {up}");
    }
    let argv = format!(
        "{}{}",
        world.recorded("invocations"),
        world.recorded("daemon-args")
    );
    assert!(
        !argv.contains("advertise-tags"),
        "advertise-tags was passed\n{argv}"
    );
    assert!(
        !argv.contains(KEY),
        "the key was passed on a command line\n{argv}"
    );

    let second = one_line(&world.up(), "second up");
    assert!(status_line_ok(&second), "{second}");
    assert!(!second.contains("joined"), "{second}");
    assert_eq!(world.count("joins"), 1, "the second up joined again");
    assert_eq!(world.count("daemons"), 1, "the second up started a daemon");

    let third = world
        .ljos(&["mesh", "up", "--hostname", HOST])
        .output()
        .unwrap();
    let third = one_line(&third, "up with no key once joined");
    assert!(status_line_ok(&third), "{third}");
    assert_eq!(world.count("joins"), 1, "an up with no key joined again");

    let pid = world.daemon_pid().expect("the daemon recorded its pid");
    let left = one_line(&world.down(), "down");
    assert!(left.contains(HOST) && left.contains("left"), "{left}");
    assert_eq!(world.count("logouts"), 1);
    assert!(gone(&pid), "tailscaled {pid} is still running after down");
    assert!(
        !world.state().exists(),
        "down left the state directory at {}",
        world.state().display()
    );
    let again = one_line(&world.down(), "second down");
    assert!(again.contains("not on the mesh"), "{again}");
}

#[test]
fn userspace_when_tun_is_missing_or_unusable() {
    let world = World::new();
    let missing = world.path("no-such-tun");
    let unusable = world.path("home");
    for tun in [missing.as_path(), unusable.as_path()] {
        let out = world.up_cmd().env("LJOS_MESH_TUN", tun).output().unwrap();
        let line = one_line(&out, "userspace up");
        assert!(status_line_ok(&line), "{line}");
        let args = world.recorded("daemon-args");
        assert!(
            args.contains("--tun=userspace-networking"),
            "tun {} did not select userspace\n{args}",
            tun.display()
        );
        assert!(args.contains("--socks5-server=localhost:1055"), "{args}");
        one_line(&world.down(), "down after userspace");
        let _ = std::fs::remove_file(world.path("fake/daemon-args"));
        let _ = std::fs::remove_file(world.path("fake/node"));
    }

    let openable = world
        .up_cmd()
        .env("LJOS_MESH_TUN", "/dev/null")
        .output()
        .unwrap();
    let line = one_line(&openable, "openable tun");
    assert!(status_line_ok(&line), "{line}");
    let args = world.recorded("daemon-args");
    // CAP_NET_ADMIN is bit 12 of CapEff (linux/capability.h). Without it
    // an open device node still cannot become a tunnel.
    let admin = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|l| l.strip_prefix("CapEff:"))
                .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok())
        })
        .is_some_and(|caps| (caps >> 12) & 1 == 1);
    if admin {
        assert!(
            !args.contains("userspace-networking") && !args.contains("socks5-server"),
            "{args}"
        );
    } else {
        assert!(args.contains("--tun=userspace-networking"), "{args}");
        assert!(args.contains("--socks5-server=localhost:1055"), "{args}");
    }
}

/// A process group that is not tailscaled, as a reused pid would name.
/// `cat` on a pipe this test holds runs until the test drops it.
struct Bystander(std::process::Child);

impl Bystander {
    fn start() -> Self {
        use std::os::unix::process::CommandExt;
        let child = Command::new("cat")
            .process_group(0)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        Self(child)
    }

    fn pid(&self) -> String {
        self.0.id().to_string()
    }

    fn alive(&mut self) -> bool {
        self.0.try_wait().unwrap().is_none()
    }
}

impl Drop for Bystander {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_stale_pid_file_signals_nothing_on_up_or_down() {
    let world = World::new();
    let mut other = Bystander::start();
    let pid = other.pid();
    let state = world.state();
    let stale = |contents: String| {
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("tailscaled.pid"), contents).unwrap();
    };
    let named = format!("tailscaled.pid named pid {pid}");

    stale(format!("{pid}\n"));
    let up = one_line(&world.up(), "up over a stale pid file");
    assert!(other.alive(), "up signalled the group pid {pid} leads");
    let old = format!("tailscaled.pid names pid {pid} with no start time");
    assert!(status_line_ok(&up) && up.contains(&old), "{up}");
    let ours = world.daemon_pid().expect("the daemon recorded its pid");
    assert_ne!(ours, pid);
    one_line(&world.down(), "down");
    assert!(gone(&ours), "tailscaled {ours} is still running after down");

    stale(format!("{pid}\n1\n"));
    let down = one_line(&world.down(), "down over a stale pid file");
    assert!(other.alive(), "down signalled the group pid {pid} leads");
    assert!(
        down.contains("left the mesh") && down.contains(&named),
        "{down}"
    );
}

#[test]
fn running_up_twice_joins_once_and_down_leaves() {
    let world = World::new();
    let first = one_line(&world.up(), "first up");
    assert_eq!(world.joined_node(), HOST);
    assert!(status_line_ok(&first), "{first}");

    let second = one_line(&world.up(), "second up");
    assert!(status_line_ok(&second), "{second}");
    assert_eq!(world.count("joins"), 1, "the second up joined again");
    assert_eq!(world.count("daemons"), 1, "the second up started a daemon");

    let pid = world.daemon_pid().expect("the daemon recorded its pid");
    let left = one_line(&world.down(), "down");
    assert!(left.contains(HOST) && left.contains("left"), "{left}");
    assert_eq!(world.count("logouts"), 1);
    assert!(gone(&pid), "tailscaled {pid} is still running after down");
    assert!(
        !world.state().exists(),
        "down left the state directory at {}",
        world.state().display()
    );
    let again = one_line(&world.down(), "second down");
    assert!(again.contains("not on the mesh"), "{again}");
}

#[test]
fn up_notes_a_pid_file_in_the_old_format_when_already_up() {
    let world = World::new();
    one_line(&world.up(), "up");
    let file = world.state().join("tailscaled.pid");
    let current = std::fs::read_to_string(&file).unwrap();
    let pid = world.daemon_pid().expect("the daemon recorded its pid");
    std::fs::write(&file, format!("{pid}\n")).unwrap();
    let again = one_line(&world.up(), "up over an old pid file");
    assert!(
        status_line_ok(&again)
            && again.contains(&format!(
                "tailscaled.pid names pid {pid} with no start time"
            ))
            && again.contains("no source.mesh label"),
        "{again}"
    );
    std::fs::write(&file, current).unwrap();
}

#[test]
fn down_kills_a_tailscaled_that_ignores_sigterm_after_the_grace() {
    let world = World::new();
    let up = world.up_cmd().env("FAKE_STUBBORN", "1").output().unwrap();
    one_line(&up, "up");
    let pid = world.daemon_pid().expect("the daemon recorded its pid");
    let left = one_line(&world.down(), "down");
    assert!(left.contains("left the mesh"), "{left}");
    // `down` sends SIGKILL and returns. Up to the `-k 5` grace for the
    // kernel to end the daemon and its new parent to reap it.
    let until = Instant::now() + Duration::from_secs(5);
    while !gone(&pid) && Instant::now() < until {
        std::thread::sleep(POLL);
    }
    assert!(
        gone(&pid),
        "tailscaled {pid} outlived SIGTERM, the grace and down"
    );
}

#[test]
fn every_failure_is_one_line_and_exit_0() {
    let world = World::new();
    let no_key = world.ljos(&["mesh", "up"]).output().unwrap();
    let no_key = one_line(&no_key, "no key");
    assert!(no_key.contains("key file"), "{no_key}");

    let wait = 2;
    let started = Instant::now();
    let hung = world
        .up_cmd()
        .env("LJOS_MESH_WAIT", wait.to_string())
        .env("FAKE_HANG", "1")
        .output()
        .unwrap();
    let took = started.elapsed();
    let line = one_line(&hung, "a tailscale that hangs");
    assert!(line.contains("did not finish within 2 s"), "{line}");
    // The bound and the kill grace `timeout -k 5` would give it.
    assert!(took < Duration::from_secs(wait + 5), "up took {took:?}");
    let pid = world.daemon_pid().expect("the daemon recorded its pid");
    assert!(gone(&pid), "a failed join left tailscaled {pid} running");
    assert_eq!(world.count("joins"), 0);
}

#[test]
fn the_key_never_appears_in_output_or_any_file() {
    let world = World::new();
    let rejected = world
        .up_cmd()
        .env("FAKE_ECHO", "1")
        .env("FAKE_REJECT", "1")
        .output()
        .unwrap();
    let line = one_line(&rejected, "a rejected key");
    assert!(line.contains("invalid key"), "{line}");
    assert!(
        !text_of(&rejected).contains(KEY),
        "the key was printed\n{}",
        text_of(&rejected)
    );
    let accepted = world.up_cmd().env("FAKE_ECHO", "1").output().unwrap();
    one_line(&accepted, "an accepted key");
    assert_eq!(world.count("joins"), 1);
    let down = world.down();
    let up_again = world.up();
    one_line(&up_again, "up after down");
    for out in [&rejected, &accepted, &down, &up_again] {
        assert!(
            !text_of(out).contains(KEY),
            "the key was printed\n{}",
            text_of(out)
        );
    }
    let held = files_holding(world.root.path(), KEY.as_bytes());
    let held: Vec<_> = held
        .into_iter()
        .filter(|path| path != &world.key())
        .collect();
    assert!(
        held.is_empty(),
        "files other than the key file hold the key: {held:?}"
    );
}

#[test]
fn a_failing_join_exits_0_and_names_the_step() {
    let world = World::new();
    let no_key = world.ljos(&["mesh", "up"]).output().unwrap();
    let no_key = one_line(&no_key, "no key");
    assert!(no_key.contains("key file"), "{no_key}");
    assert_eq!(
        String::from_utf8_lossy(&world.ljos(&["mesh", "up"]).output().unwrap().stdout).trim(),
        "",
        "a failure printed on stdout"
    );

    let rejected = world.up_cmd().env("FAKE_REJECT", "1").output().unwrap();
    let line = one_line(&rejected, "a rejected key");
    assert!(line.contains("tailscale up"), "{line}");
    assert!(line.contains("invalid key"), "{line}");
    assert!(
        !text_of(&rejected).contains(KEY),
        "the key was printed\n{}",
        text_of(&rejected)
    );
    let pid = world.daemon_pid().expect("the daemon recorded its pid");
    assert!(gone(&pid), "a failed join left tailscaled {pid} running");
    assert_eq!(world.count("joins"), 0);

    let wait = 2;
    let started = Instant::now();
    let hung = world
        .up_cmd()
        .env("LJOS_MESH_WAIT", wait.to_string())
        .env("FAKE_HANG", "1")
        .output()
        .unwrap();
    let took = started.elapsed();
    let line = one_line(&hung, "a tailscale that hangs");
    assert!(line.contains("tailscale up"), "{line}");
    assert!(line.contains("did not finish within 2 s"), "{line}");
    assert!(took < Duration::from_secs(wait + 5), "up took {took:?}");
    let pid = world.daemon_pid().expect("the daemon recorded its pid");
    assert!(gone(&pid), "a timed out join left tailscaled {pid} running");
}

#[test]
fn the_key_never_appears_in_output_or_any_other_file() {
    let world = World::new();
    let echoed = world
        .up_cmd()
        .env("FAKE_ECHO", "1")
        .env("FAKE_REJECT", "1")
        .output()
        .unwrap();
    let line = one_line(&echoed, "a rejected key that is echoed");
    assert!(line.contains("[key]"), "{line}");
    assert!(
        !text_of(&echoed).contains(KEY),
        "the key was printed\n{}",
        text_of(&echoed)
    );

    let accepted = world.up_cmd().env("FAKE_ECHO", "1").output().unwrap();
    one_line(&accepted, "an accepted key");
    assert!(
        !text_of(&accepted).contains(KEY),
        "the key was printed\n{}",
        text_of(&accepted)
    );
    assert_eq!(world.count("joins"), 1);
    let held = files_holding(world.root.path(), KEY.as_bytes());
    let held: Vec<_> = held
        .into_iter()
        .filter(|path| path != &world.key())
        .collect();
    assert!(
        held.is_empty(),
        "files other than the key file hold the key: {held:?}"
    );
}

#[test]
fn login_server_and_key_file_come_from_the_environment() {
    let world = World::new();
    let server = "https://headscale.example.org";
    let out = world
        .ljos(&["mesh", "up", "--hostname", HOST])
        .env("LJOS_MESH_KEY_FILE", world.key())
        .env("LJOS_MESH_LOGIN_SERVER", server)
        .output()
        .unwrap();
    one_line(&out, "up from the environment");
    let up = up_invocation(&world);
    assert!(up.contains(&format!("--login-server={server}")), "{up}");
    assert!(
        up.contains(&format!("--auth-key=file:{}", world.key().display())),
        "{up}"
    );
    assert!(!up.contains("advertise-tags"), "{up}");
}

#[test]
fn hostname_defaults_to_the_system_hostname() {
    let world = World::new();
    let expected = Command::new("hostname").output().unwrap();
    let expected = String::from_utf8_lossy(&expected.stdout).trim().to_string();
    assert!(!expected.is_empty(), "this machine has no hostname");
    let out = world
        .ljos(&["mesh", "up", "--key-file"])
        .arg(world.key())
        .output()
        .unwrap();
    let line = one_line(&out, "up with the system hostname");
    assert!(line.contains(&expected), "{line}");
    let up = up_invocation(&world);
    assert!(up.contains(&format!("--hostname={expected}")), "{up}");
}

#[test]
fn the_acl_opens_the_configs_ports_and_no_cross_swarm_rule() {
    let world = World::new();
    let out = world.ljos(&["mesh", "acl", "other"]).output().unwrap();
    assert!(out.status.success(), "{}", text_of(&out));
    let policy: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let config: toml::Value = toml::from_str(EXAMPLE).unwrap();
    let port = |name: &str| config["ports"][name].as_integer().unwrap().to_string();
    let mut on_box: Vec<String> = ["api", "store-ro", "docs-ro", "drop"]
        .iter()
        .map(|name| port(name))
        .collect();
    on_box.sort();

    let rules = policy["acls"].as_array().expect("acls");
    assert_eq!(rules.len(), 4, "{policy:#}");
    for rule in rules {
        assert_eq!(rule["action"], "accept", "{rule}");
        let src = rule["src"].as_array().unwrap();
        let dst = rule["dst"].as_array().unwrap();
        assert_eq!((src.len(), dst.len()), (1, 1), "{rule}");
        let src = src[0].as_str().unwrap();
        let dst = dst[0].as_str().unwrap();
        assert!(
            src == "tag:swarm-example" || src == "tag:swarm-other",
            "{rule}"
        );
        if let Some(ports) = dst.strip_prefix("tag:box:") {
            let mut ports: Vec<String> = ports.split(',').map(str::to_string).collect();
            ports.sort();
            assert_eq!(ports, on_box, "{rule}");
            assert!(!ports.contains(&port("socks")), "{rule}");
        } else {
            assert_eq!(dst, format!("{src}:*"), "a cross-swarm rule: {rule}");
        }
    }
    let mut owners: Vec<&String> = policy["tagOwners"].as_object().unwrap().keys().collect();
    owners.sort();
    assert_eq!(
        owners,
        ["tag:box", "tag:swarm-example", "tag:swarm-other"],
        "{policy:#}"
    );
}

#[test]
fn git_config_is_byte_identical_after_up_and_down() {
    let world = World::new();
    let repo = world.path("home/checkout");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(
        world.path("home/.gitconfig"),
        "[user]\n\tname = Someone\n[core]\n\thooksPath = /nowhere/hooks\n",
    )
    .unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", world.path("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}\n{}", text_of(&out));
        out.stdout
    };
    git(&["init", "-q"]);
    git(&["config", "--local", "core.hooksPath", "/elsewhere/hooks"]);
    let read = || {
        (
            git(&["config", "--list", "--show-origin"]),
            std::fs::read(world.path("home/.gitconfig")).unwrap(),
            std::fs::read(repo.join(".git/config")).unwrap(),
        )
    };
    let before = read();
    let up = world.up_cmd().current_dir(&repo).output().unwrap();
    assert!(status_line_ok(&one_line(&up, "up")), "{}", text_of(&up));
    let down = world
        .ljos(&["mesh", "down"])
        .current_dir(&repo)
        .output()
        .unwrap();
    one_line(&down, "down");
    assert!(read() == before, "git config changed across up and down");
}

/// A pack that keeps each posted atom and answers reads with nothing.
fn pack(atoms: Arc<Mutex<Vec<serde_json::Value>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = match stream.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&tmp[..n]);
                let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= end + 4 + len {
                    break;
                }
            }
            let req = String::from_utf8_lossy(&buf).into_owned();
            let line = req.lines().next().unwrap_or("");
            let body = req.split_once("\r\n\r\n").map_or("", |(_, b)| b);
            let payload = if line.starts_with("POST /v1/atoms") {
                let mut atom: serde_json::Value =
                    serde_json::from_str(body).unwrap_or(serde_json::json!({}));
                let mut kept = atoms.lock().unwrap();
                atom["id"] = serde_json::json!(format!("{:b>32}", kept.len()));
                kept.push(atom.clone());
                atom.to_string()
            } else {
                r#"{"hits":[],"atoms":[]}"#.to_string()
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
        }
    });
    format!("http://{addr}")
}

#[test]
fn a_write_on_the_mesh_carries_the_node_and_the_seat_it_claims() {
    let world = World::new();
    let atoms = Arc::new(Mutex::new(Vec::new()));
    let url = pack(Arc::clone(&atoms));
    let remember = |text: &str| {
        let out = world
            .ljos(&["remember", text])
            .env("PACKSET_URL", &url)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_TIMEOUT_MS", "2000")
            .env("LJOS_SEAT", "probe")
            .output()
            .unwrap();
        assert!(out.status.success(), "remember\n{}", text_of(&out));
        let kept = atoms.lock().unwrap();
        kept.iter()
            .find(|a| a["text"] == text)
            .cloned()
            .unwrap_or_else(|| panic!("{text:?} was not stored: {kept:?}\n{}", text_of(&out)))
    };
    one_line(&world.up(), "up");
    let node = world.joined_node();
    let on = remember("The nightly build uploads its logs before it prunes.");
    let label = &on["source"]["mesh"];
    assert_eq!(label["node"], node.as_str(), "{on}");
    assert_eq!(label["seat"], on["source"]["seat"], "{on}");
    assert!(
        label["seat"].as_str().is_some_and(|s| !s.is_empty()),
        "{on}"
    );

    one_line(&world.down(), "down");
    let off = remember("The staging host keeps three days of logs.");
    assert!(off["source"].get("mesh").is_none(), "{off}");

    one_line(&world.up(), "up again");
    let pid = world.daemon_pid().expect("the daemon recorded its pid");
    let killed = Command::new("kill").args(["-9", &pid]).status().unwrap();
    assert!(killed.success(), "kill -9 {pid}");
    // Up to the `-k 5` grace a sitting gets. ljos reads a zombie that
    // nobody has reaped yet as gone, so the wait only keeps /proc tidy.
    let until = Instant::now() + Duration::from_secs(5);
    while !gone(&pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    let crashed = remember("The release job tags the commit it built.");
    assert!(crashed["source"].get("mesh").is_none(), "{crashed}");
}

/// Tagged broker nodes come from the daemon's status document. The sample
/// names one broker. The parser does not.
#[test]
fn mesh_status_shows_a_tagged_broker_from_the_status_document() {
    let world = World::new();
    one_line(&world.up(), "up");
    let fixture = include_str!("../../../examples/mesh-status.json");
    std::fs::write(world.path("fake/status.json"), fixture).unwrap();
    let out = world.ljos(&["mesh", "status"]).output().unwrap();
    let said = text_of(&out);
    assert!(out.status.success(), "{said}");
    for needle in [
        "ag-example-abcdef",
        "id 99",
        "tag:example",
        "100.64.0.99",
        "fd7a:115c:a1e0::99",
        "non-ephemeral",
        "tag:swarm-example",
    ] {
        assert!(said.contains(needle), "{needle} missing\n{said}");
    }
    let logic =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/mesh.rs")).unwrap();
    assert!(
        !logic.contains("ag-example-abcdef") && !logic.contains("tag:example"),
        "a broker belongs in the fixture, not in the parser"
    );
}
