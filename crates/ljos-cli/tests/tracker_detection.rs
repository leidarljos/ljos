//! ljos-vxtx, ljos-76mj, ljos-13pu, ljos-hu8r, ljos-q753, ljos-xf4h: a
//! product checkout is never the tracker.
//!
//! vissue takes the working directory for the tracker when it holds a
//! `vissue.toml`, and as a guess when it holds a `Software/`. A product
//! checkout with an untracked `vissue.toml`, an empty `Software/`, or the
//! boards a clone of another project carries had tickets written and
//! committed into it. Each is left byte-for-byte as it was, and the write
//! says which tracker it looked for.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn vissue_bin() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let bin = dir.join("vissue");
        bin.is_file().then_some(bin)
    })
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

/// Every file and directory under `dir` but `.git`, with its bytes.
fn files(dir: &Path, base: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|name| name == ".git") {
            continue;
        }
        let rel = path.strip_prefix(base).unwrap().to_path_buf();
        if path.is_dir() {
            out.push((rel, Vec::new()));
            files(&path, base, out);
        } else {
            out.push((rel, std::fs::read(&path).unwrap()));
        }
    }
}

/// A seat with real vissue, nothing naming a tracker, and the deed store
/// and claim graph a sitting asks stubbed.
struct Seat {
    dir: tempfile::TempDir,
    vissue: PathBuf,
}

impl Seat {
    fn new() -> Option<Self> {
        let Some(vissue) = vissue_bin() else {
            eprintln!("skip: vissue is not on PATH");
            return None;
        };
        let dir = tempfile::TempDir::new().unwrap();
        let bin = dir.path().join("bin");
        script(
            &bin,
            "claimdag",
            "#!/bin/sh\ncase \"$1\" in\n  get) exit 1 ;;\n  list) echo '[]' ;;\n  *) echo 'gen=1' ;;\nesac\n",
        );
        script(&bin, "deedar", "#!/bin/sh\necho head\n");
        std::fs::write(
            dir.path().join("gitconfig"),
            "[user]\n\tname = seat\n\temail = seat@example.test\n\
             [core]\n\thooksPath = /dev/null\n[commit]\n\tgpgsign = false\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        Some(Self { dir, vissue })
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn command(&self, program: &Path, from: &Path) -> Command {
        let home = self.home();
        let mut cmd = Command::new(program);
        cmd.current_dir(from)
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:{}:/usr/bin:/bin",
                    self.dir.path().join("bin").display(),
                    self.vissue.parent().unwrap().display()
                ),
            )
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("run"))
            .env("GIT_CONFIG_GLOBAL", self.dir.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LJOS_SEAT", "saddle")
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "commit")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_LEASE_SECS", "0")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", "off")
            .stdin(Stdio::null());
        cmd
    }

    fn ljos_with(&self, from: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = self.command(Path::new(env!("CARGO_BIN_EXE_ljos")), from);
        cmd.args(args).envs(env.iter().copied());
        cmd.output().unwrap()
    }

    fn ljos(&self, from: &Path, args: &[&str]) -> Output {
        self.ljos_with(from, args, &[])
    }

    fn git(&self, at: &Path, args: &[&str]) -> String {
        std::fs::create_dir_all(at).unwrap();
        let out = self
            .command(Path::new("git"), at)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", said(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A product checkout with one committed file.
    fn product(&self, name: &str) -> PathBuf {
        let at = self.dir.path().join(name);
        self.git(&at, &["init", "-q"]);
        std::fs::write(at.join("README"), "a product\n").unwrap();
        self.git(&at, &["add", "README"]);
        self.git(&at, &["commit", "-q", "-m", "product"]);
        at
    }

    /// The three checkouts that passed for the tracker, and the id of the
    /// one issue the cloned project's boards hold.
    fn checkouts(&self) -> ([PathBuf; 3], String) {
        let untracked = self.product("untracked");
        std::fs::write(untracked.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
        let bare = self.product("bare");
        std::fs::create_dir_all(bare.join("Software")).unwrap();
        let upstream = self.product("upstream");
        let out = self
            .command(&self.vissue, &upstream)
            .args(["q", "-p", "app", "a board the project keeps"])
            .env("VISSUE_ROOT", &upstream)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", said(&out));
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        self.git(&upstream, &["add", "-A"]);
        self.git(&upstream, &["commit", "-q", "-m", "boards"]);
        let clone = self.dir.path().join("clone");
        self.git(
            self.dir.path(),
            &["clone", "-q", upstream.to_str().unwrap(), "clone"],
        );
        assert!(clone.join("Software/app/issues.org").is_file());
        ([untracked, bare, clone], id)
    }

    /// A tracker the seat's vissue configuration names, as `ljos onboard`
    /// makes one.
    fn configured_tracker(&self) -> PathBuf {
        let root = self.dir.path().join("vault/tracker");
        self.git(&root, &["init", "-q"]);
        std::fs::create_dir_all(root.join("Software")).unwrap();
        let cfg = self.home().join(".config/vissue/config.toml");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(
            &cfg,
            format!(
                "root = {}\n",
                toml::Value::String(root.display().to_string())
            ),
        )
        .unwrap();
        root
    }

    /// What a write into `dir` would change: its files, its commits, and
    /// what git sees in its work tree.
    fn snapshot(&self, dir: &Path) -> (Vec<(PathBuf, Vec<u8>)>, String) {
        let mut out = Vec::new();
        files(dir, dir, &mut out);
        out.sort();
        let git = self.git(dir, &["log", "--all", "--format=%H %s"])
            + &self.git(dir, &["status", "--porcelain", "--untracked-files=all"]);
        (out, git)
    }
}

/// Nothing names a tracker. Each checkout is left as it was, and the write
/// names the tracker `ljos onboard` would make. A sitting on an issue the
/// checkout does not hold writes no `vissue.toml` there either.
#[test]
fn a_product_checkout_is_not_written_as_the_tracker() {
    let Some(seat) = Seat::new() else {
        return;
    };
    let (checkouts, id) = seat.checkouts();
    let looked_for = seat.home().join(".local/share/vissue/tracker");
    for product in &checkouts {
        let before = seat.snapshot(product);
        let filed = seat.ljos(product, &["file", "-p", "app", "crash on start"]);
        let noted = seat.ljos(product, &["note", &id, "seen again"]);
        let sat = seat.ljos(product, &["sitting", &id]);
        let text = format!("{}{}{}", said(&filed), said(&noted), said(&sat));
        assert_eq!(
            seat.snapshot(product),
            before,
            "{} was written:\n{text}",
            product.display()
        );
        assert!(!filed.status.success(), "{text}");
        assert!(
            said(&filed).contains(&looked_for.display().to_string()),
            "{text}"
        );
        assert!(!noted.status.success(), "{text}");
        assert!(!sat.status.success(), "{text}");
    }
}

/// The seat configured a tracker. Every write from a product checkout
/// lands there, an untracked `vissue.toml` in the checkout included.
#[test]
fn a_write_from_a_product_checkout_lands_in_the_configured_tracker() {
    let Some(seat) = Seat::new() else {
        return;
    };
    let (checkouts, _) = seat.checkouts();
    let tracker = seat.configured_tracker();
    for product in &checkouts {
        let before = seat.snapshot(product);
        let filed = seat.ljos(product, &["file", "-p", "app", "crash on start"]);
        assert!(filed.status.success(), "{}", said(&filed));
        let stdout = String::from_utf8_lossy(&filed.stdout);
        let id = stdout.split_whitespace().next().unwrap().to_string();
        let noted = seat.ljos(product, &["note", &id, "seen again"]);
        assert!(noted.status.success(), "{}", said(&noted));
        let sat = seat.ljos(product, &["sitting", &id]);
        assert!(sat.status.success(), "{}", said(&sat));
        assert_eq!(seat.snapshot(product), before, "{}", product.display());
        let log = seat.git(&tracker, &["log", "--format=%s"]);
        for verb in ["filed", "noted", "claimed"] {
            assert!(log.contains(&format!("{id} {verb} by saddle")), "{log}");
        }
    }
}

/// A tracker whose projects sit under `VISSUE_PREFIX` is the tracker when
/// the seat stands in it.
#[test]
fn a_tracker_under_vissue_prefix_is_the_tracker() {
    let Some(seat) = Seat::new() else {
        return;
    };
    let root = seat.dir.path().join("work");
    seat.git(&root, &["init", "-q"]);
    std::fs::create_dir_all(root.join("Work")).unwrap();
    std::fs::write(root.join("vissue.toml"), "").unwrap();
    seat.git(&root, &["add", "vissue.toml"]);
    seat.git(&root, &["commit", "-q", "-m", "tracker"]);
    let filed = seat.ljos_with(
        &root,
        &["file", "-p", "app", "crash on start"],
        &[("VISSUE_PREFIX", "Work")],
    );
    assert!(filed.status.success(), "{}", said(&filed));
    let board = root.join("Work/app/issues.org");
    assert!(
        said(&filed).contains(&board.display().to_string()),
        "{}",
        said(&filed)
    );
    let log = seat.git(&root, &["log", "--format=%s"]);
    assert!(log.contains("filed by saddle"), "{log}");
}
