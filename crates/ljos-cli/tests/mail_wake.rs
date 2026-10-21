//! Mail reaches a seat that is not at a prompt.
//!
//! Two seats share packsetd on a free port in a fresh home. The recipient
//! sets a wake command; a send starts it, and the command reads the
//! recipient's inbox. Interrupt mail holds a stop in the shape Claude Code
//! and Cursor send, once. A seat the pack has never heard from is refused
//! with the seat it most likely meant. A second test walks the whole
//! exchange: group, send, inbox, read, reply, interrupt, and the receipt
//! `ljos hook --prompt` writes.

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("free port");
    listener.local_addr().expect("addr").port()
}

struct Home {
    dir: tempfile::TempDir,
    port: u16,
    path: String,
}

impl Home {
    /// One `ljos` run as `seat`, with only this home's environment.
    fn ljos(&self, seat: &str, args: &[&str], stdin: Option<&str>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args);
        self.env(&mut cmd);
        cmd.env("LJOS_SEAT", seat);
        run(cmd, stdin)
    }

    fn env(&self, cmd: &mut Command) {
        let home = self.dir.path();
        cmd.env_clear()
            .current_dir(home)
            .env("HOME", home)
            .env("PATH", &self.path)
            .env("LANG", "C.UTF-8")
            .env("TMPDIR", home.join("tmp"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("PACKSET_PORT", self.port.to_string())
            .env("PACKSET_AUTH", "off")
            .env("LJOS_TRACKER_GIT", "off");
    }
}

fn run(mut cmd: Command, stdin: Option<&str>) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut pipe = child.stdin.take().expect("stdin");
    if let Some(text) = stdin {
        pipe.write_all(text.as_bytes()).expect("write stdin");
    }
    drop(pipe);
    child.wait_with_output().expect("wait")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn home() -> (Home, Daemon) {
    let Some(packsetd) = on_path("packsetd") else {
        panic!("packsetd is not on PATH; CI builds it from packset develop");
    };
    let dir = tempfile::tempdir().expect("home");
    for sub in [
        "tmp",
        "run",
        ".config",
        ".local/share",
        ".local/state",
        ".cache",
    ] {
        std::fs::create_dir_all(dir.path().join(sub)).expect("home dirs");
    }
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_ljos"))
        .parent()
        .expect("bin dir");
    let path = std::env::join_paths([
        bin_dir,
        packsetd.parent().expect("packsetd dir"),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .expect("PATH")
    .into_string()
    .expect("utf-8 PATH");
    let home = Home {
        port: free_port(),
        path,
        dir,
    };
    let pack_home = home.dir.path().join(".local/share/packset");
    std::fs::create_dir_all(&pack_home).expect("pack home");
    let mut daemon = Command::new(&packsetd);
    daemon.args([
        "--port",
        &home.port.to_string(),
        "--home",
        pack_home.to_str().expect("utf-8"),
    ]);
    home.env(&mut daemon);
    let daemon = Daemon(
        daemon
            .env("PACKSET_HOME", &pack_home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn packsetd"),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    while std::net::TcpStream::connect(("127.0.0.1", home.port)).is_err() {
        assert!(Instant::now() < deadline, "packsetd did not listen");
        std::thread::sleep(Duration::from_millis(50));
    }
    (home, daemon)
}

/// Wait for `path` to hold `needle`, and say how long it took.
fn wait_for(path: &Path, needle: &str, limit: Duration) -> Option<Duration> {
    let started = Instant::now();
    while started.elapsed() < limit {
        if std::fs::read_to_string(path).is_ok_and(|t| t.contains(needle)) {
            return Some(started.elapsed());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

const SENDER: &str = "grok-bot";
const RECIPIENT: &str = "claude-review";

#[test]
fn a_send_wakes_the_recipient_and_an_interrupt_holds_its_stop() {
    let (home, _daemon) = home();
    // The recipient is known once it has written to the pack.
    let joined = home.ljos(RECIPIENT, &["group", "crew", "--add", RECIPIENT], None);
    assert!(joined.status.success(), "{}", text(&joined));

    let missed = home.ljos(SENDER, &["send", "claude", "hello"], None);
    assert!(!missed.status.success(), "{}", text(&missed));
    assert!(
        text(&missed).contains("Did you mean claude-review?"),
        "{}",
        text(&missed)
    );

    // A wake the guard refuses is not set.
    let refused = home.ljos(
        RECIPIENT,
        &["wake", "--command", "kubectl delete namespace prod"],
        None,
    );
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("the guard says"),
        "{}",
        text(&refused)
    );

    let script = home.dir.path().join("wake.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nljos inbox > \"$HOME/woke.tmp\" && mv \"$HOME/woke.tmp\" \"$HOME/woke.$LJOS_WAKE_INTERRUPT\"\n",
    )
    .expect("script");
    let set = home.ljos(
        RECIPIENT,
        &["wake", "--command", &format!("sh {}", script.display())],
        None,
    );
    assert!(set.status.success(), "{}", text(&set));

    let started = Instant::now();
    let sent = home.ljos(
        SENDER,
        &[
            "send",
            RECIPIENT,
            "Software-d9n6 is yours",
            "--issue",
            "Software-d9n6",
        ],
        None,
    );
    let send_took = started.elapsed();
    assert!(sent.status.success(), "{}", text(&sent));
    assert!(
        text(&sent).contains(&format!("waking {RECIPIENT}")),
        "{}",
        text(&sent)
    );
    let woke = wait_for(
        &home.dir.path().join("woke.0"),
        "Software-d9n6 is yours",
        Duration::from_secs(20),
    );
    let log = std::fs::read_to_string(home.dir.path().join(".local/state/ljos/mail/wake.log"))
        .unwrap_or_default();
    let woke = woke.unwrap_or_else(|| panic!("the wake did not read the inbox: {log}"));
    eprintln!(
        "measure: send returned in {} ms; the recipient's wake read the message {} ms after send returned",
        send_took.as_millis(),
        woke.as_millis()
    );

    // An interrupt holds the stop of a running session once, in each
    // runner's words.
    let urgent = home.ljos(
        SENDER,
        &[
            "send",
            RECIPIENT,
            "stop the release, CI is red",
            "--interrupt",
        ],
        None,
    );
    assert!(urgent.status.success(), "{}", text(&urgent));
    let claude_stop = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "claude-1",
        "cwd": home.dir.path(),
        "stop_hook_active": false,
    })
    .to_string();
    let held = home.ljos(RECIPIENT, &["hook"], Some(&claude_stop));
    assert!(held.status.success(), "{}", text(&held));
    let answer: Value =
        serde_json::from_slice(&held.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&held)));
    assert_eq!(answer["decision"], "block", "{answer}");
    let reason = answer["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("stop the release, CI is red"), "{reason}");
    assert!(!reason.contains("Software-d9n6 is yours"), "{reason}");
    eprintln!("measure: Claude Code Stop answer: {answer}");

    let again = serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "claude-2",
        "cwd": home.dir.path(),
        "stop_hook_active": false,
    })
    .to_string();
    let quiet = home.ljos(RECIPIENT, &["hook"], Some(&again));
    assert!(
        !text(&quiet).contains("CI is red"),
        "a delivered letter holds the stop again: {}",
        text(&quiet)
    );

    let cursor = home.ljos(
        SENDER,
        &[
            "send",
            RECIPIENT,
            "rebase is off, merge develop",
            "--interrupt",
        ],
        None,
    );
    assert!(cursor.status.success(), "{}", text(&cursor));
    let cursor_stop = serde_json::json!({
        "hook_event_name": "stop",
        "cursor_version": "3.2.0",
        "conversation_id": "conv-9",
        "generation_id": "gen-9",
        "status": "completed",
        "loop_count": 0,
    })
    .to_string();
    let held = home.ljos(RECIPIENT, &["hook"], Some(&cursor_stop));
    let answer: Value =
        serde_json::from_slice(&held.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&held)));
    let follow = answer["followup_message"].as_str().unwrap_or_default();
    assert!(follow.contains("rebase is off, merge develop"), "{answer}");
    eprintln!("measure: Cursor stop answer: {answer}");
}

fn sent_id(out: &Output) -> String {
    text(out)
        .lines()
        .find_map(|line| line.strip_prefix("sent "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or("")
        .to_string()
}

fn show(seat: &str, args: &[&str], out: &Output) {
    eprintln!(
        "--- {seat} ljos {} (exit {:?}) ---\n{}",
        args.join(" "),
        out.status.code(),
        text(out)
    );
}

fn atoms(port: u16) -> Value {
    use std::io::{Read, Write};
    let mut sock = std::net::TcpStream::connect(("127.0.0.1", port)).expect("atoms");
    sock.set_read_timeout(Some(Duration::from_secs(5))).ok();
    write!(
        sock,
        "GET /v1/atoms?workspace=seat&embedding=omit HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .expect("write");
    let mut buf = Vec::new();
    sock.read_to_end(&mut buf).expect("read");
    let text = String::from_utf8_lossy(&buf);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");
    serde_json::from_str(body).unwrap_or_else(|err| panic!("{err}: {body}"))
}

fn receipt_for<'a>(pack: &'a Value, msg: &str) -> Vec<&'a Value> {
    pack["atoms"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|atom| {
            atom["kind"].as_str() == Some("receipt")
                && atom["entities"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|e| e.as_str() == Some(&format!("receipt:{msg}")))
        })
        .collect()
}

/// Two seats on one pack: send, inbox, read, reply, an interrupt, a group,
/// and the receipt a prompt hook writes. A shell prompt writes it before
/// the command exits. Cursor holds the note and writes it when the next
/// tool result delivers the note, or on Stop when the turn ran no tool.
#[test]
fn two_seats_exchange_mail_and_the_prompt_writes_a_receipt() {
    let (home, _daemon) = home();
    const ALICE: &str = "alice";
    const BOB: &str = "bob";

    let run = |seat: &str, args: &[&str], stdin: Option<&str>| {
        let out = home.ljos(seat, args, stdin);
        show(seat, args, &out);
        out
    };

    let added = run(ALICE, &["group", "ops", "--add", ALICE, "--add", BOB], None);
    assert!(added.status.success(), "{}", text(&added));
    assert!(
        text(&added).contains("added alice to ops"),
        "{}",
        text(&added)
    );
    assert!(
        text(&added).contains("added bob to ops"),
        "{}",
        text(&added)
    );
    let listed = run(BOB, &["group", "ops"], None);
    assert!(listed.status.success(), "{}", text(&listed));
    assert!(text(&listed).contains("ops\talice"), "{}", text(&listed));
    assert!(text(&listed).contains("ops\tbob"), "{}", text(&listed));

    let sent = run(ALICE, &["send", BOB, "the build is red"], None);
    assert!(sent.status.success(), "{}", text(&sent));
    let id = sent_id(&sent);
    assert!(!id.is_empty(), "{}", text(&sent));

    let bob = run(BOB, &["inbox"], None);
    assert!(bob.status.success(), "{}", text(&bob));
    assert!(text(&bob).contains(&id), "{}", text(&bob));
    assert!(text(&bob).contains("the build is red"), "{}", text(&bob));
    assert!(!text(&bob).contains("receipt\t"), "{}", text(&bob));
    let quiet = run(ALICE, &["inbox"], None);
    assert_eq!(text(&quiet).trim(), "inbox: nothing unread");

    let read = run(BOB, &["read", &id], None);
    assert!(read.status.success(), "{}", text(&read));
    assert!(
        text(&read).contains(&format!("read {id}")),
        "{}",
        text(&read)
    );
    assert!(!text(&read).contains("already"), "{}", text(&read));
    let again = run(BOB, &["read", &id], None);
    assert!(again.status.success(), "{}", text(&again));
    assert!(text(&again).contains("already"), "{}", text(&again));

    let saw = run(ALICE, &["inbox"], None);
    assert!(saw.status.success(), "{}", text(&saw));
    assert!(
        text(&saw).contains(&format!("receipt\t{id}\t{BOB}\t")),
        "{}",
        text(&saw)
    );

    let reply = run(BOB, &["reply", &id, "looking"], None);
    assert!(reply.status.success(), "{}", text(&reply));
    let reply_id = sent_id(&reply);
    assert!(
        text(&reply).contains(&format!("reply {id}")),
        "{}",
        text(&reply)
    );
    let back = run(ALICE, &["inbox"], None);
    assert!(text(&back).contains(&reply_id), "{}", text(&back));
    assert!(text(&back).contains("looking"), "{}", text(&back));
    assert!(
        text(&back).contains(&format!("reply:{id}")) || text(&back).contains("reply"),
        "{}",
        text(&back)
    );
    let took = run(ALICE, &["read", &reply_id], None);
    assert!(took.status.success(), "{}", text(&took));

    let urgent = run(
        ALICE,
        &["send", BOB, "stop the release", "--interrupt"],
        None,
    );
    assert!(urgent.status.success(), "{}", text(&urgent));
    let urgent_id = sent_id(&urgent);
    let hot = run(BOB, &["inbox"], None);
    assert!(text(&hot).starts_with(&urgent_id), "{}", text(&hot));
    assert!(text(&hot).contains("interrupt"), "{}", text(&hot));
    assert!(text(&hot).contains("stop the release"), "{}", text(&hot));

    let prompt = run(BOB, &["hook", "--prompt"], Some("what should I do next\n"));
    assert!(prompt.status.success(), "{}", text(&prompt));
    let shown = String::from_utf8_lossy(&prompt.stdout);
    assert!(shown.contains("stop the release"), "{shown}");
    assert!(
        shown.contains(&format!("! {urgent_id} from {ALICE}")),
        "{shown}"
    );
    let unread = run(BOB, &["inbox"], None);
    assert!(
        text(&unread).contains("nothing unread"),
        "{}",
        text(&unread)
    );
    let all = run(BOB, &["inbox", "--all"], None);
    assert!(text(&all).contains(&urgent_id), "{}", text(&all));
    assert!(text(&all).contains("read"), "{}", text(&all));

    let pack = atoms(home.port);
    let receipts = receipt_for(&pack, &urgent_id);
    assert_eq!(receipts.len(), 1, "one receipt atom: {pack}");
    let body = receipts[0]["text"].as_str().unwrap_or("");
    assert!(
        body.starts_with(&format!("read {urgent_id}\nby {BOB}\n")),
        "{body}"
    );
    assert!(body.contains("\nat "), "{body}");
    assert!(body.contains("\nmsg "), "{body}");
    let entities: Vec<&str> = receipts[0]["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        entities.contains(&format!("to:{ALICE}").as_str()),
        "{entities:?}"
    );
    assert!(
        entities.contains(&format!("from:{BOB}").as_str()),
        "{entities:?}"
    );
    assert!(
        entities.contains(&format!("receipt:{urgent_id}").as_str()),
        "{entities:?}"
    );

    let told = run(ALICE, &["inbox"], None);
    assert!(
        text(&told).contains(&format!("receipt\t{urgent_id}\t{BOB}\t")),
        "{}",
        text(&told)
    );
    let second = run(BOB, &["hook", "--prompt"], Some("anything else to read\n"));
    assert!(second.status.success(), "{}", text(&second));
    assert!(
        !String::from_utf8_lossy(&second.stdout).contains("stop the release"),
        "{}",
        text(&second)
    );
    let pack = atoms(home.port);
    assert_eq!(
        receipt_for(&pack, &urgent_id).len(),
        1,
        "a second prompt writes no second receipt"
    );

    let plain = run(ALICE, &["send", BOB, "the log is in the usual place"], None);
    assert!(plain.status.success(), "{}", text(&plain));
    let plain_id = sent_id(&plain);
    let held = serde_json::json!({
        "cursor_version": "3.2.0",
        "conversation_id": "conv-mail",
        "hook_event_name": "beforeSubmitPrompt",
        "prompt": "check the mail",
    })
    .to_string();
    let cursor = run(BOB, &["hook"], Some(&held));
    assert!(cursor.status.success(), "{}", text(&cursor));
    assert!(
        !String::from_utf8_lossy(&cursor.stdout).contains("the log is in the usual place"),
        "cursor shows the prompt note later: {}",
        text(&cursor)
    );
    let pack = atoms(home.port);
    assert!(
        receipt_for(&pack, &plain_id).is_empty(),
        "the prompt hook held the note and wrote no receipt: {pack}"
    );

    let delivered = serde_json::json!({
        "cursor_version": "3.2.0",
        "conversation_id": "conv-mail",
        "hook_event_name": "postToolUse",
        "tool_name": "Shell",
        "tool_input": {"command": "ls"},
    })
    .to_string();
    let tool = run(BOB, &["hook"], Some(&delivered));
    assert!(tool.status.success(), "{}", text(&tool));
    assert!(
        String::from_utf8_lossy(&tool.stdout).contains("the log is in the usual place"),
        "{}",
        text(&tool)
    );
    let pack = atoms(home.port);
    assert_eq!(
        receipt_for(&pack, &plain_id).len(),
        1,
        "the tool result writes the receipt: {pack}"
    );

    let later = run(ALICE, &["send", BOB, "merge after the tag"], None);
    assert!(later.status.success(), "{}", text(&later));
    let later_id = sent_id(&later);
    let ask = serde_json::json!({
        "cursor_version": "3.2.0",
        "conversation_id": "conv-stop",
        "hook_event_name": "beforeSubmitPrompt",
        "prompt": "one more look",
    })
    .to_string();
    let asked = run(BOB, &["hook"], Some(&ask));
    assert!(asked.status.success(), "{}", text(&asked));
    let pack = atoms(home.port);
    assert!(receipt_for(&pack, &later_id).is_empty(), "{pack}");
    let stop = serde_json::json!({
        "hook_event_name": "stop",
        "cursor_version": "3.2.0",
        "conversation_id": "conv-stop",
        "generation_id": "gen-stop",
        "status": "completed",
        "loop_count": 0,
    })
    .to_string();
    let stopped = run(BOB, &["hook"], Some(&stop));
    assert!(stopped.status.success(), "{}", text(&stopped));
    assert!(
        String::from_utf8_lossy(&stopped.stdout).contains("merge after the tag"),
        "{}",
        text(&stopped)
    );
    let pack = atoms(home.port);
    assert_eq!(
        receipt_for(&pack, &later_id).len(),
        1,
        "stop delivers the held note and writes the receipt: {pack}"
    );
}
