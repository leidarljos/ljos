//! `scripts/install.sh` on a fresh home, from a directory of programs.

ljos_test_boot::harness!();
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PROGRAMS: [&str; 11] = [
    "ljos",
    "ljos-mcp",
    "packset",
    "packsetd",
    "packset-mcp",
    "vissue",
    "vissue-mcp",
    "deedar",
    "claimdag",
    "ljos-policyd",
    "ljos-consensus",
];

const PATH_LINE: &str = "export PATH=\"$HOME/.local/bin:$PATH\"";
const MARK: &str = "# Added by the ljos installer: its programs on PATH.";

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn install_sh() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh")
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Install the stand-ins in `progs` into `home`'s `~/.local/bin`, with
/// `path` as the caller's PATH.
fn install(home: &Path, progs: &Path, path: &str, extra: &[&str]) -> String {
    std::fs::create_dir_all(home).unwrap();
    let out = Command::new("sh")
        .arg(install_sh())
        .arg("--from-dir")
        .arg(progs)
        .arg("--bin-dir")
        .arg(home.join(".local/bin"))
        .args(["--no-onboard", "--no-embed"])
        .args(extra)
        .env_clear()
        .env("HOME", home)
        .env("PATH", path)
        .output()
        .unwrap();
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    text
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn install_puts_the_bin_dir_on_path_in_each_profile_once() {
    let dir = tempfile::tempdir().unwrap();
    let progs = dir.path().join("progs");
    std::fs::create_dir_all(&progs).unwrap();
    for name in PROGRAMS {
        script(&progs, name, "#!/bin/sh\nexit 0\n");
    }

    let home = dir.path().join("zsh-user");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join(".zshrc"), "# mine\n").unwrap();
    let first = install(&home, &progs, "/usr/bin:/bin", &[]);
    assert!(
        first.contains("to PATH in ~/.profile ~/.bashrc ~/.zshrc"),
        "{first}"
    );
    assert!(first.contains(PATH_LINE), "{first}");
    let again = install(&home, &progs, "/usr/bin:/bin", &[]);
    assert!(again.contains("already add"), "{again}");
    for rc in [".profile", ".bashrc", ".zshrc"] {
        let text = read(&home.join(rc));
        assert_eq!(text.matches(MARK).count(), 1, "{rc}:\n{text}");
        assert_eq!(text.matches(PATH_LINE).count(), 1, "{rc}:\n{text}");
    }
    assert!(read(&home.join(".zshrc")).starts_with("# mine\n\n"));
    let found = Command::new("sh")
        .args(["-c", ". \"$HOME/.profile\"; command -v ljos"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&found.stdout).trim(),
        home.join(".local/bin/ljos").display().to_string(),
        "{}",
        said(&found)
    );

    let bash_only = dir.path().join("bash-user");
    install(&bash_only, &progs, "/usr/bin:/bin", &[]);
    assert!(read(&bash_only.join(".bashrc")).contains(PATH_LINE));
    assert!(
        !bash_only.join(".zshrc").exists(),
        "a home without ~/.zshrc got one"
    );

    let opted_out = dir.path().join("opted-out");
    let note = install(&opted_out, &progs, "/usr/bin:/bin", &["--no-modify-path"]);
    assert!(note.contains("not on PATH"), "{note}");
    assert!(note.contains(PATH_LINE), "{note}");
    for rc in [".profile", ".bashrc"] {
        assert!(!opted_out.join(rc).exists(), "--no-modify-path wrote {rc}");
    }

    let on_path = dir.path().join("on-path");
    let path = format!("{}:/usr/bin:/bin", on_path.join(".local/bin").display());
    let quiet = install(&on_path, &progs, &path, &[]);
    assert!(!quiet.contains(PATH_LINE), "{quiet}");
    for rc in [".profile", ".bashrc"] {
        assert!(!on_path.join(rc).exists(), "a bin dir on PATH wrote {rc}");
    }
}

/// `vissue ARGS` over vissue-core, for the stand-in below. Not a test: it
/// returns at once unless the stand-in ran it.
#[test]
#[ignore = "run by the vissue stand-in"]
fn vissue_shim() {
    let (Ok(out), Ok(args)) = (
        std::env::var("LJOS_SHIM_OUT"),
        std::env::var("LJOS_SHIM_ARGS"),
    ) else {
        return;
    };
    let args: Vec<&str> = args
        .strip_suffix('\x1f')
        .unwrap_or(&args)
        .split('\x1f')
        .collect();
    let layout = || vissue_core::Layout::resolve(None, None).unwrap();
    let said = match args.as_slice() {
        ["--version"] => "vissue 0.20.0\n".to_string(),
        ["vote", "--help"] => {
            "Usage: vissue vote [OPTIONS] <ID>\n  --used <IDS>\n  --confidence <P>\n".to_string()
        }
        ["identity"] => {
            let layout = layout();
            format!(
                "vissue 0.20.0\nroot={}\nprefix={}\n",
                layout.root().display(),
                layout.prefix()
            )
        }
        ["show", id, "--json"] => vissue_core::agent::show_json(&layout(), id)
            .unwrap()
            .to_string(),
        ["note", id, text @ ..] => vissue_core::ops::note(&layout(), id, &text.join(" ")).unwrap(),
        ["recall", id] => vissue_core::report::recall(&layout(), id, 1, false).unwrap(),
        ["claim", id] => vissue_core::ops::claim(&layout(), id, false).unwrap(),
        _ => panic!("the stand-in does not run vissue {args:?}"),
    };
    std::fs::write(out, said).unwrap();
}

/// The system programs the installer and a sitting run. Nothing else from
/// `/usr/bin` is on PATH, so a coding tool installed on this machine is not
/// found and the install takes the branch a machine with none takes. curl
/// and wget stay out too: nothing is downloaded, and the doctor does not
/// ask crates.io.
const SYSTEM: &[&str] = &[
    "sh", "env", "mktemp", "date", "cp", "chmod", "mkdir", "rm", "mv", "cat", "grep", "sed", "awk",
    "find", "uname", "wc", "tr", "head", "tail", "cut", "sort", "dirname", "basename", "sleep",
    "timeout", "git", "ps", "id",
];

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    })
}

/// A fresh machine: a home with no coding tool in it, a PATH of system
/// programs, and a free port for the writer the install starts. It lives
/// under the build directory, because ljos treats a HOME under `/tmp` as a
/// test sandbox and keeps the guard and the memory writer off there.
struct Machine {
    root: PathBuf,
    port: u16,
}

impl Machine {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .join(format!("ljos-install-{}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["home", "sys", "run"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        // git does not stop at a ceiling spelled with `..`, and above this
        // one is the ljos checkout.
        let root = std::fs::canonicalize(&root).unwrap();
        std::fs::write(
            root.join("home/.gitconfig"),
            "[user]\n\tname = demo\n\temail = demo@example.invalid\n[commit]\n\tgpgsign = false\n",
        )
        .unwrap();
        for name in SYSTEM {
            let found = ["/usr/bin", "/bin"]
                .iter()
                .map(|dir| Path::new(dir).join(name))
                .find(|p| p.is_file());
            if let Some(found) = found {
                std::os::unix::fs::symlink(found, root.join("sys").join(name)).unwrap();
            }
        }
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .unwrap()
            .port();
        Self { root, port }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn bin(&self) -> PathBuf {
        self.home().join(".local/bin")
    }

    /// `program` run as the person at a terminal in their home, with
    /// `bin` ahead of the system programs when it is given.
    fn run(&self, program: &Path, bin: Option<&Path>) -> Command {
        let sys = self.root.join("sys");
        let path = match bin {
            Some(bin) => format!("{}:{}", bin.display(), sys.display()),
            None => sys.display().to_string(),
        };
        let mut cmd = Command::new(program);
        cmd.current_dir(self.home())
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", path)
            .env("PACKSET_PORT", self.port.to_string())
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("GIT_CEILING_DIRECTORIES", &self.root);
        cmd
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        let packset = self.bin().join("packset");
        if packset.is_file() {
            let _ = self.run(&packset, Some(&self.bin())).arg("stop").output();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// With no coding tool on the machine, install.sh still runs `ljos onboard`
/// for the seat: it makes the tracker and names it in vissue's config, the
/// deed store and the host key, and starts the memory writer. A sitting on
/// a demo issue then opens and claims it with no variable naming a store.
#[test]
fn install_with_no_coding_tool_leaves_a_seat_a_sitting_opens_on() {
    if !ljos_cli::tool_installed("packsetd") {
        eprintln!("skipped: packsetd is not installed");
        return;
    }
    let packsetd = on_path("packsetd")
        .unwrap_or_else(|| panic!("packsetd is not on PATH; CI builds it from packset develop"));
    let packset = on_path("packset")
        .unwrap_or_else(|| panic!("packset is not on PATH; CI builds it from packset develop"));
    let machine = Machine::new();
    let progs = machine.root.join("progs");
    std::fs::create_dir_all(&progs).unwrap();
    for (name, real) in [
        ("ljos", PathBuf::from(env!("CARGO_BIN_EXE_ljos"))),
        ("ljos-mcp", PathBuf::from(env!("CARGO_BIN_EXE_ljos-mcp"))),
        ("packset", packset),
        ("packsetd", packsetd),
    ] {
        std::os::unix::fs::symlink(real, progs.join(name)).unwrap();
    }
    let calls = machine.root.join("calls.log");
    let log = calls.display();
    script(
        &progs,
        "vissue",
        &format!(
            "#!/bin/sh\necho \"vissue $*\" >> '{log}'\nout=\"$(mktemp)\"\nLJOS_SHIM_OUT=\"$out\" LJOS_SHIM_ARGS=\"$(printf '%s\\037' \"$@\")\" '{}' --exact vissue_shim --ignored --test-threads=1 --nocapture >\"$out.log\" 2>&1\ncode=$?\ncat \"$out\"\n[ $code -eq 0 ] || cat \"$out.log\" >&2\nrm -f \"$out\" \"$out.log\"\nexit $code\n",
            std::env::current_exe().unwrap().display()
        ),
    );
    script(
        &progs,
        "claimdag",
        &format!(
            "#!/bin/sh\necho \"claimdag $*\" >> '{log}'\ncase \"$1 $2\" in\n  '--version '*) echo 'claimdag 0.8.0' ;;\n  'list --json') echo '[]' ;;\n  'get '*) echo 'not found' >&2; exit 1 ;;\n  'upsert --id') echo \"$3\" ;;\n  'claim '*) echo 'gen=1' ;;\nesac\nexit 0\n"
        ),
    );
    script(
        &progs,
        "deedar",
        &format!(
            "#!/bin/sh\necho \"deedar $*\" >> '{log}'\ncase \"$1 $2\" in\n  '--version '*) echo 'deedar 0.4.0' ;;\n  'host ') echo 'host key: a signer the store lists' ;;\n  'log head') echo 'size=0' ;;\nesac\nexit 0\n"
        ),
    );
    for (name, version) in [
        ("ljos-policyd", "ljos-policyd 0.5.0 (phronesis)"),
        ("ljos-consensus", "ljos-consensus 0.7.0"),
        ("vissue-mcp", "vissue-mcp 0.21.0"),
        ("packset-mcp", "packset-mcp 0.16.0"),
    ] {
        script(
            &progs,
            name,
            &format!("#!/bin/sh\n[ \"$1\" = --version ] && echo '{version}'\nexit 0\n"),
        );
    }

    let out = machine
        .run(Path::new("/bin/sh"), None)
        .arg(install_sh())
        .arg("--from-dir")
        .arg(&progs)
        .arg("--bin-dir")
        .arg(machine.bin())
        .args(["--no-embed", "--no-modify-path"])
        .output()
        .unwrap();
    let installed = said(&out);
    assert!(out.status.success(), "{installed}");
    assert!(
        installed.contains("none found. Setting up the tracker, memory and deed store."),
        "{installed}"
    );
    assert!(!installed.contains("reported a problem"), "{installed}");

    let home = machine.home();
    let tracker = home.join(".local/share/vissue/tracker");
    let config = read(&home.join(".config/vissue/config.toml"));
    assert!(
        config.contains(&format!("root = \"{}\"", tracker.display())),
        "{config}"
    );
    assert!(tracker.join("Software").is_dir());
    assert!(home.join(".local/share/deedar/store").is_dir());
    let key = std::fs::metadata(home.join(".config/deedar/host.key")).unwrap();
    assert_eq!(key.len(), 32);
    let writer = machine
        .run(&machine.bin().join("packset"), Some(&machine.bin()))
        .arg("url")
        .output()
        .unwrap();
    assert!(writer.status.success(), "{}", said(&writer));

    let board = tracker.join("Software/demo/issues.org");
    std::fs::create_dir_all(board.parent().unwrap()).unwrap();
    std::fs::write(
        &board,
        "#+TITLE: demo issues\n#+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n\
         * TODO [#C] Try the first sitting\n:PROPERTIES:\n:ID:         demo-0001\n:END:\n",
    )
    .unwrap();
    let sat = machine
        .run(&machine.bin().join("ljos"), Some(&machine.bin()))
        .args(["sitting", "demo-0001", "--assignee", "demo"])
        .env("LJOS_SEAT", "demo")
        .output()
        .unwrap();
    let sitting = said(&sat);
    let logged = read(&calls);
    assert!(sat.status.success(), "{sitting}\ncalls:\n{logged}");
    assert!(
        sitting.starts_with("tracker: 1 uncommitted edit; fix: "),
        "{sitting}"
    );
    let ready = sitting
        .split_once('\n')
        .map(|(_, rest)| rest)
        .unwrap_or(sitting.as_str());
    assert!(ready.starts_with("ljos is ready:"), "{sitting}");
    assert!(
        sitting.contains("tracker: demo-0001 STARTED under demo"),
        "{sitting}"
    );
    assert!(sitting.contains("demo-0001 is yours."), "{sitting}");
    assert!(sitting.contains("no remote, kept local"), "{sitting}");
    let top = machine
        .run(&machine.root.join("sys/git"), None)
        .arg("-C")
        .arg(&tracker)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&top.stdout).trim(),
        tracker.display().to_string(),
        "the claim was committed outside the tracker: {}",
        said(&top)
    );
    let org = read(&board);
    assert!(
        org.contains("* STARTED [#C] Try the first sitting"),
        "{org}"
    );
    assert!(org.contains(":CLAIMED_BY: demo"), "{org}");
    assert!(logged.contains("claimdag claim "), "{logged}");
}
