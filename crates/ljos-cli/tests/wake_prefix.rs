use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());

struct Guard {
    saved: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    _lock: MutexGuard<'static, ()>,
}

impl Guard {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let saved = std::env::vars_os().collect();
        Self { saved, _lock: lock }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let keep: Vec<_> = self.saved.iter().map(|(k, _)| k.clone()).collect();
        for (k, _) in std::env::vars_os() {
            if !keep.iter().any(|have| have == &k) {
                std::env::remove_var(&k);
            }
        }
        for (k, v) in &self.saved {
            std::env::set_var(k, v);
        }
    }
}

#[test]
fn a_prefix_wake_matches_longer_run_seats() {
    let _g = Guard::new();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
    std::env::set_var("XDG_STATE_HOME", dir.path().join("state"));

    let set = ljos_cli::wake::set_webhook("ljosbot-review-", "http://127.0.0.1:9/hook", None, None)
        .expect("set prefix");
    assert!(set.contains("starts with"), "{set}");

    let shown = ljos_cli::wake::show("ljosbot-review-abc");
    assert!(
        shown.contains("matches ljosbot-review-"),
        "prefix wake did not match run seat: {shown}"
    );
    let none = ljos_cli::wake::show("ljosbot-review");
    assert!(none.contains("none set"), "{none}");
}
