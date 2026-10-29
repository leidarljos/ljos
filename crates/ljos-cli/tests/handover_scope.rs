//! `ljos handover --scope cloud` carries only the scope's atoms, sealed
//! with age to the scope's recipients. No atom is in the bag in plaintext.
//! `ljos receive` opens the bag on a cloud VM that holds the cloud
//! identity. A machine without it leaves the atoms closed. A handover
//! without `--scope` still carries the whole workspace.
#![cfg(target_os = "linux")]

ljos_test_boot::harness!();
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const CLOUD_TEXT: &str = "Cloud seats clone the tracker before they vote.";
const BOX_TEXT: &str = "The passphrase hash lives only on the box.";

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn tools_dir() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin")))
        .find(|dir| dir.join("packsetd").is_file() && dir.join("vissue").is_file())
        .unwrap_or_else(|| {
            panic!("packsetd and vissue are not on PATH; CI builds them from packset and vissue")
        })
}

fn keygen(path: &Path) -> String {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let made = Command::new("age-keygen")
        .arg("-o")
        .arg(path)
        .output()
        .expect("age-keygen must be installed");
    assert!(made.status.success());
    let public = Command::new("age-keygen")
        .arg("-y")
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&public.stdout).trim().to_string()
}

fn http(port: u16, request: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.write_all(request.as_bytes()).ok()?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok()?;
    Some(answer)
}

fn start_pack(home: &Path, tools: &Path) -> (Daemon, u16) {
    std::fs::create_dir_all(home).unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = Command::new(tools.join("packsetd"))
        .args(["--port", &port.to_string(), "--home"])
        .arg(home)
        .env("PACKSET_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn packsetd");
    let daemon = Daemon(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    let health = "GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";
    while !(home.join("token").is_file()
        && http(port, health).is_some_and(|a| a.starts_with("HTTP/1.1 200")))
    {
        assert!(Instant::now() < deadline, "packsetd did not answer");
        std::thread::sleep(Duration::from_millis(100));
    }
    (daemon, port)
}

fn post_atom(port: u16, token: &str, text: &str, entities: &[&str]) {
    let body = serde_json::json!({
        "schema": "inside.atom/v1",
        "kind": "lesson",
        "level": "explicit",
        "text": text,
        "workspace": "seat",
        "entities": entities,
        "source": {"kind": "person"},
    })
    .to_string();
    let answer = http(
        port,
        &format!(
            "POST /v1/atoms HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
    .unwrap();
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
}

struct World {
    root: tempfile::TempDir,
    tools: PathBuf,
    tracker: PathBuf,
    port: u16,
    _pack: Daemon,
}

impl World {
    fn new() -> Self {
        let tools = tools_dir();
        let root = tempfile::tempdir().unwrap();
        let box_public = keygen(&root.path().join("box/.config/ljos/age.key"));
        let cloud_public = keygen(&root.path().join("vm/.config/ljos/cloud.age"));
        keygen(&root.path().join("other/.config/ljos/age.key"));
        let tracker = root.path().join("tracker");
        std::fs::create_dir_all(tracker.join("Software/probe")).unwrap();
        std::fs::create_dir_all(tracker.join(".ljos")).unwrap();
        std::fs::write(
            tracker.join("Software/probe/issues.org"),
            "#+VISSUE: 1\n#+TODO: TODO | DONE\n\n* TODO [#A] Probe\n:PROPERTIES:\n:ID:       probe-0001\n:END:\n",
        )
        .unwrap();
        std::fs::write(
            tracker.join(".ljos/sync.toml"),
            format!(
                "name = \"grokbot\"\nrecipients = [\"{box_public}\"]\n[scopes]\ncloud = [\"{cloud_public}\"]\n"
            ),
        )
        .unwrap();
        let (pack, port) = start_pack(&root.path().join("box/pack"), &tools);
        let token = std::fs::read_to_string(root.path().join("box/pack/token")).unwrap();
        post_atom(
            port,
            token.trim(),
            CLOUD_TEXT,
            &["seat:boxseat", "scope:cloud"],
        );
        post_atom(port, token.trim(), BOX_TEXT, &["seat:boxseat"]);
        Self {
            root,
            tools,
            tracker,
            port,
            _pack: pack,
        }
    }

    fn ljos(&self, machine: &str, args: &[&str]) -> Output {
        self.ljos_with(machine, args, &self.tools)
    }

    /// [`World::ljos`] with `first` searched before the tools.
    fn ljos_with(&self, machine: &str, args: &[&str], first: &Path) -> Output {
        let home = self.root.path().join(machine);
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env(
                "PATH",
                format!("{}:{}:/usr/bin:/bin", first.display(), self.tools.display()),
            )
            .env("PACKSET_URL", format!("http://127.0.0.1:{}", self.port))
            .env("PACKSET_HOME", self.root.path().join("box/pack"))
            .env("ISSUE_ROOT", &self.tracker)
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("DEEDAR_HOST_SIGNING_KEY", "off")
            .current_dir(self.root.path())
            .output()
            .unwrap()
    }

    fn bag(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn a_cloud_handover_seals_only_cloud_memory_and_a_cloud_vm_opens_it() {
    let w = World::new();
    let bag = w.bag("bag");
    let bag_arg = bag.display().to_string();
    let out = w.ljos(
        "box",
        &[
            "handover",
            "--out",
            &bag_arg,
            "--project",
            "probe",
            "--scope",
            "cloud",
        ],
    );
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("1 scope cloud atoms sealed to 1 recipients"),
        "{text}"
    );
    assert!(text.contains("1 atoms of other scopes left out"), "{text}");

    let atoms: Vec<String> = std::fs::read_dir(bag.join("data/atoms"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(atoms, ["cloud.jsonl.age"], "{text}");
    for file in files_under(&bag) {
        let bytes = std::fs::read(&file).unwrap();
        let plain = String::from_utf8_lossy(&bytes);
        assert!(
            !plain.contains(CLOUD_TEXT),
            "{} is plaintext",
            file.display()
        );
        assert!(
            !plain.contains(BOX_TEXT),
            "{} holds box memory",
            file.display()
        );
        assert!(
            !file.to_string_lossy().contains(".scope-export-"),
            "the plaintext export was left at {}",
            file.display()
        );
    }

    let vm = w.ljos("vm", &["receive", &bag_arg]);
    let opened = said(&vm);
    assert!(vm.status.success(), "{opened}");
    assert!(
        opened.contains("scope cloud: 1 sealed atoms opened"),
        "{opened}"
    );
    assert!(opened.contains("1 atoms enclosed"), "{opened}");

    let other = w.ljos("other", &["receive", &bag_arg]);
    let closed = said(&other);
    assert!(other.status.success(), "{closed}");
    assert!(
        closed.contains("scope cloud atoms are not sealed to this machine; left closed"),
        "{closed}"
    );
    assert!(closed.contains("0 atoms enclosed"), "{closed}");
}

#[test]
fn a_scope_with_no_recipients_writes_no_atoms() {
    let w = World::new();
    let bag = w.bag("nowhere");
    let bag_arg = bag.display().to_string();
    let out = w.ljos(
        "box",
        &[
            "handover",
            "--out",
            &bag_arg,
            "--project",
            "probe",
            "--scope",
            "nowhere",
        ],
    );
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("no tracker lists recipients for scope nowhere"),
        "{text}"
    );
    assert!(!bag.join("data/atoms").exists(), "{text}");
}

/// The plaintext export is made outside the bag, so a copy of the bag
/// cannot carry it. An export that cannot be removed stops the handover.
#[test]
fn a_plaintext_export_that_cannot_be_removed_stops_the_handover() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new();
    let wrap = w.root.path().join("wrap");
    std::fs::create_dir_all(&wrap).unwrap();
    let seen = w.root.path().join("export-dir");
    let script = format!(
        "#!/bin/sh\n'{}' \"$@\" || exit $?\n\
         if [ \"$1\" = export ] && [ \"$2\" = --into ]; then\n\
         printf '%s' \"$3\" > '{}'\n\
         mkdir -p \"$3/locked\" && touch \"$3/locked/kept\" && chmod 0500 \"$3/locked\"\n\
         fi\n",
        w.tools.join("packset").display(),
        seen.display()
    );
    std::fs::write(wrap.join("packset"), script).unwrap();
    std::fs::set_permissions(wrap.join("packset"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let bag = w.bag("stuck");
    let bag_arg = bag.display().to_string();
    let out = w.ljos_with(
        "box",
        &[
            "handover",
            "--out",
            &bag_arg,
            "--project",
            "probe",
            "--scope",
            "cloud",
        ],
        &wrap,
    );
    let text = said(&out);
    let export = PathBuf::from(std::fs::read_to_string(&seen).expect("packset export ran"));
    let _ = std::fs::set_permissions(
        export.join("locked"),
        std::fs::Permissions::from_mode(0o700),
    );
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("could not be removed"), "{text}");
    assert!(text.contains("nothing was handed over"), "{text}");
    assert!(
        !export.starts_with(&bag),
        "the export {} is inside the bag",
        export.display()
    );
    if bag.exists() {
        for file in files_under(&bag) {
            let plain = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
            assert!(
                !plain.contains(CLOUD_TEXT),
                "{} is plaintext",
                file.display()
            );
            assert!(
                !plain.contains(BOX_TEXT),
                "{} holds box memory",
                file.display()
            );
        }
    }
}

#[test]
fn a_handover_without_a_scope_still_carries_the_workspace() {
    let w = World::new();
    let bag = w.bag("whole");
    let bag_arg = bag.display().to_string();
    let out = w.ljos(
        "box",
        &["handover", "--out", &bag_arg, "--project", "probe"],
    );
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    let seat = std::fs::read_to_string(bag.join("data/atoms/seat.jsonl")).unwrap();
    assert!(
        seat.contains(CLOUD_TEXT) && seat.contains(BOX_TEXT),
        "{seat}"
    );
}
