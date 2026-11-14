//! `ljos proposals` on an empty queue does not talk to the pack, and a
//! silent pack does not hold the process.

ljos_test_boot::harness!();

use std::io::Read;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Slow {
    url: String,
    stop: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}

impl Slow {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let stop_flag = Arc::clone(&stop);
        let call_count = Arc::clone(&calls);
        std::thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        call_count.fetch_add(1, Ordering::Relaxed);
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                        let mut buf = [0u8; 1024];
                        let _ = stream.read(&mut buf);
                        let mut waited = Duration::ZERO;
                        while waited < Duration::from_secs(30) && !stop_flag.load(Ordering::Relaxed)
                        {
                            std::thread::sleep(Duration::from_millis(50));
                            waited += Duration::from_millis(50);
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });
        Self { url, stop, calls }
    }
}

impl Drop for Slow {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[test]
fn an_empty_queue_does_not_call_a_slow_pack() {
    let slow = Slow::start();
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["proposals"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("USER", "person")
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_DATA_HOME", home.join(".data"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("LJOS_SEAT", "ada")
        .env("PACKSET_URL", &slow.url)
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_AUTH", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .output()
        .unwrap();
    let took = started.elapsed();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert_eq!(out.stdout, b"no proposals\n", "{text}");
    assert_eq!(slow.calls.load(Ordering::Relaxed), 0, "{text}");
    assert!(
        took < Duration::from_secs(5),
        "empty proposals waited {took:?}\n{text}"
    );
}
