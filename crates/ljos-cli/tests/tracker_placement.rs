//! ljos-unm9, ljos-e5iq, ljos-3lea, ljos-ge74: where the tracker lives.
//!
//! A seat whose home sat inside a product checkout had its tracker made
//! there, among that project's files. A seat run from a product checkout
//! filed into the directory it stood in, because vissue takes the working
//! directory as the tracker when nothing else names one and the directory
//! holds a `Software`. Onboard now refuses the first, and every habitat
//! ljos starts is told which tracker it means.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git_repo(at: &Path) {
    std::fs::create_dir_all(at).unwrap();
    let out = Command::new("git")
        .args(["init", "-q"])
        .current_dir(at)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
}

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A seat whose home is inside `product`, onboarded from there.
fn onboard(product: &Path, args: &[&str]) -> Output {
    let home = product.join("home");
    std::fs::create_dir_all(&home).unwrap();
    Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("onboard")
        .args(args)
        .current_dir(product)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("PACKSET_URL", "off")
        .env("LJOS_SEAT", "saddle")
        .output()
        .unwrap()
}

fn tracker_row(text: &str) -> &str {
    text.lines()
        .find(|line| line.contains("\ttracker\t"))
        .unwrap_or_else(|| panic!("no tracker row:\n{text}"))
}

/// The complaint: the tracker was made among a product's files, because
/// the seat's home sat inside that checkout.
#[test]
fn a_tracker_is_not_made_inside_a_product_checkout() {
    let dir = tempfile::TempDir::new().unwrap();
    let product = dir.path().join("myapp");
    git_repo(&product);
    let would_be = product.join("home/.local/share/vissue/tracker");

    let dry = said(&onboard(&product, &["--dry-run", "--harness", "claude"]));
    let row = tracker_row(&dry);
    assert!(row.starts_with("no\t"), "{row}");
    assert!(row.contains(&product.display().to_string()), "{row}");
    assert!(row.contains("VISSUE_ROOT"), "{row}");

    let made = said(&onboard(&product, &["--harness", "claude"]));
    assert!(tracker_row(&made).starts_with("no\t"), "{made}");
    assert!(
        !would_be.exists(),
        "a tracker was made in the checkout anyway"
    );
}

/// A path the checkout keeps out of itself is nobody's files, so the
/// tracker may live there.
#[test]
fn a_path_the_checkout_ignores_is_still_a_place_for_a_tracker() {
    let dir = tempfile::TempDir::new().unwrap();
    let product = dir.path().join("myapp");
    git_repo(&product);
    std::fs::write(product.join(".gitignore"), "home/\n").unwrap();

    let made = said(&onboard(&product, &["--harness", "claude"]));
    let row = tracker_row(&made);
    assert!(row.starts_with("ok\t"), "{row}");
    assert!(
        product
            .join("home/.local/share/vissue/tracker/Software")
            .is_dir(),
        "{made}"
    );
}

/// A seat standing in a product checkout, with its tracker named in the
/// seat's own configuration. The habitat is told that tracker, whatever
/// the directory it was started in looks like.
struct Seat {
    dir: tempfile::TempDir,
}

impl Seat {
    /// A product checkout holding a `Software` directory, which is what
    /// made vissue take it for a tracker, and a configured tracker
    /// outside it.
    fn new() -> Self {
        let seat = Self {
            dir: tempfile::TempDir::new().unwrap(),
        };
        git_repo(&seat.product());
        std::fs::create_dir_all(seat.product().join("Software")).unwrap();
        std::fs::create_dir_all(seat.tracker().join("Software")).unwrap();
        script(
            &seat.dir.path().join("bin"),
            "vissue",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"root=${{VISSUE_ROOT-unset}}\" >> '{}'\nexit 0\n",
                seat.calls().display()
            ),
        );
        let cfg = seat.dir.path().join("home/.config/vissue/config.toml");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(
            &cfg,
            format!(
                "root = {}\n",
                toml::Value::String(seat.tracker().display().to_string())
            ),
        )
        .unwrap();
        seat
    }

    fn product(&self) -> PathBuf {
        self.dir.path().join("myapp")
    }

    fn tracker(&self) -> PathBuf {
        self.dir.path().join("vault/tracker")
    }

    fn calls(&self) -> PathBuf {
        self.dir.path().join("calls.log")
    }

    /// `ljos note`, run from `from`: one tracker write, through the same
    /// habitat call every other write takes.
    fn note(&self, from: &Path) -> Output {
        let home = self.dir.path().join("home");
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(["note", "proj-aaaa", "The parser kept the hyphen."])
            .current_dir(from)
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.dir.path().join("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("PACKSET_URL", "off")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .output()
            .unwrap()
    }

    fn roots(&self) -> Vec<String> {
        std::fs::read_to_string(self.calls())
            .unwrap_or_default()
            .lines()
            .map(|line| line.trim_start_matches("root=").to_string())
            .collect()
    }
}

#[test]
fn a_write_from_a_product_checkout_goes_to_the_configured_tracker() {
    let seat = Seat::new();
    let out = seat.note(&seat.product());
    let roots = seat.roots();
    assert!(!roots.is_empty(), "no habitat ran: {}", said(&out));
    for root in &roots {
        assert_eq!(
            root,
            &seat.tracker().display().to_string(),
            "a write went to the checkout: {roots:?}"
        );
    }
}

/// vissue's own rule: standing in a tracker means that tracker. A seat
/// run there is not told another one.
#[test]
fn standing_in_a_tracker_still_means_that_tracker() {
    let seat = Seat::new();
    let other = seat.dir.path().join("vault/second");
    std::fs::create_dir_all(other.join("Software")).unwrap();
    std::fs::write(other.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    seat.note(&other);
    assert_eq!(seat.roots(), ["unset"], "the directory it stands in lost");
}
