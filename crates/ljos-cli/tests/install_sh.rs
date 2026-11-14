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
const SOURCE_CMD: &str = "cargo install --locked ljos packset vissue-cli vissue-mcp deedar-cli claimdag-cli ljos-policyd ljos-consensus";

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

/// vissue, claimdag and deedar come from `.github/sibling-pin` unless
/// `--latest` or `--pin` names them.
#[test]
fn print_assets_uses_the_sibling_pin_unless_latest() {
    let pin = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.github/sibling-pin"),
    )
    .unwrap();
    let mut pinned = Vec::new();
    for line in pin.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let name = parts.next().unwrap();
        let ver = parts.nth(2).unwrap();
        pinned.push((name.to_string(), ver.to_string()));
    }
    assert!(!pinned.is_empty());
    let out = Command::new("sh")
        .arg(install_sh())
        .args([
            "--print-assets",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--pin",
            "ljos=0.41.11",
            "--pin",
            "packset=0.17.3",
            "--pin",
            "ljos-policyd=0.7.0",
            "--pin",
            "consensus=0.10.1",
        ])
        .output()
        .unwrap();
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    for (name, ver) in &pinned {
        let asset = format!("{name}-v{ver}-x86_64-unknown-linux-gnu.tar.gz");
        assert!(
            text.lines().any(|line| line == asset),
            "{text}\nmissing {asset}"
        );
    }
    let latest = Command::new("sh")
        .arg(install_sh())
        .args([
            "--print-assets",
            "--latest",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--pin",
            "ljos=0.41.11",
            "--pin",
            "packset=0.17.3",
            "--pin",
            "ljos-policyd=0.7.0",
            "--pin",
            "consensus=0.10.1",
            "--pin",
            "vissue=9.9.9",
            "--pin",
            "claimdag=9.9.9",
            "--pin",
            "deedar=9.9.9",
        ])
        .output()
        .unwrap();
    let text = said(&latest);
    assert!(latest.status.success(), "{text}");
    assert!(
        text.lines()
            .any(|line| line == "vissue-v9.9.9-x86_64-unknown-linux-gnu.tar.gz"),
        "{text}"
    );
    assert!(
        text.lines()
            .all(|line| !line.starts_with("vissue-v0.23.1-")),
        "{text}"
    );
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
    let first = install(&home, &progs, "/usr/bin:/bin", &["--modify-path"]);
    assert!(
        first.contains("to PATH in ~/.profile ~/.bashrc ~/.zshrc"),
        "{first}"
    );
    assert!(first.contains(PATH_LINE), "{first}");
    let again = install(&home, &progs, "/usr/bin:/bin", &["--modify-path"]);
    assert!(again.contains("already add"), "{again}");
    for rc in [".profile", ".bashrc", ".zshrc"] {
        let text = read(&home.join(rc));
        assert_eq!(text.matches(MARK).count(), 1, "{rc}:\n{text}");
        assert_eq!(text.matches(PATH_LINE).count(), 1, "{rc}:\n{text}");
    }
    assert!(read(&home.join(".zshrc")).starts_with("# mine\n\n"));
    assert!(first.contains("source <(ljos completions bash)"), "{first}");
    assert!(
        first.contains("ljos completions powershell | Out-String | Invoke-Expression"),
        "{first}"
    );
    assert!(first.contains("eval (ljos completions elvish)"), "{first}");
    assert!(
        !read(&home.join(".bashrc")).contains("ljos completions"),
        "install wrote a completion line into ~/.bashrc:\n{}",
        read(&home.join(".bashrc"))
    );
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
    install(&bash_only, &progs, "/usr/bin:/bin", &["--modify-path"]);
    assert!(read(&bash_only.join(".bashrc")).contains(PATH_LINE));
    assert!(
        !bash_only.join(".zshrc").exists(),
        "a home without ~/.zshrc got one"
    );

    let opted_out = dir.path().join("opted-out");
    let note = install(&opted_out, &progs, "/usr/bin:/bin", &["--no-modify-path"]);
    assert!(note.contains("not on PATH"), "{note}");
    assert!(note.contains(PATH_LINE), "{note}");
    assert!(
        note.contains("Next: ") && note.contains("ljos doctor"),
        "{note}"
    );
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
    "timeout", "git", "ps", "id", "touch",
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

/// `--no-embed` is the one-command install: the seat programs are copied,
/// the encoder is not, and the script says search matches words.
/// A flagless run still copies the encoder.
#[test]
fn no_embed_leaves_the_encoder_out_and_the_default_still_installs_it() {
    let dir = tempfile::tempdir().unwrap();
    let progs = dir.path().join("progs");
    std::fs::create_dir_all(&progs).unwrap();
    for name in PROGRAMS {
        script(&progs, name, "#!/bin/sh\nexit 0\n");
    }
    script(&progs, "packset-embed", "#!/bin/sh\nexit 0\n");

    let lexical = dir.path().join("lexical");
    let text = install(&lexical, &progs, "/usr/bin:/bin", &[]);
    assert!(
        text.contains("Search matches words. The encoder is not installed."),
        "{text}"
    );
    assert!(lexical.join(".local/bin/ljos").is_file(), "{text}");
    assert!(
        !lexical.join(".local/bin/packset-embed").exists(),
        "the one-command install copied the encoder:\n{text}"
    );

    let full = dir.path().join("full");
    std::fs::create_dir_all(&full).unwrap();
    let out = Command::new("sh")
        .arg(install_sh())
        .arg("--from-dir")
        .arg(&progs)
        .arg("--bin-dir")
        .arg(full.join(".local/bin"))
        .args(["--no-onboard", "--no-modify-path"])
        .env_clear()
        .env("HOME", &full)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let said = said(&out);
    assert!(out.status.success(), "{said}");
    assert!(
        full.join(".local/bin/packset-embed").is_file(),
        "a flagless install left the encoder out:\n{said}"
    );
    assert!(!said.contains("The encoder is not installed."), "{said}");
    assert!(said.contains("Encoder model ready."), "{said}");
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
        installed.contains(
            "none found. Setting up the ticket tracker, the memory writer, and the deed store."
        ),
        "{installed}"
    );
    assert!(
        installed.contains("The deed store keeps signed records of the work."),
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
    let claimed = sitting
        .find("tracker: demo-0001 STARTED under demo")
        .expect(&sitting);
    let ready = sitting.find("ljos is ready:").expect(&sitting);
    assert!(
        claimed < ready,
        "the claim is printed before the doctor summary:\n{sitting}"
    );
    assert!(!sitting.contains("uncommitted edit"), "{sitting}");
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

fn run_install(home: &Path, path: &str, extra: &[&str]) -> Output {
    std::fs::create_dir_all(home).unwrap();
    Command::new("sh")
        .arg(install_sh())
        .args(extra)
        .env_clear()
        .env("HOME", home)
        .env("PATH", path)
        .output()
        .unwrap()
}

/// curl exits before any byte arrives. The script names that and the retry.
#[test]
fn a_failed_release_download_names_the_network_and_the_retry() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(
        &bin,
        "curl",
        "#!/bin/sh\necho 'curl: (6) Could not resolve host: github.com' >&2\nexit 6\n",
    );
    let home = dir.path().join("home");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let out = run_install(&home, &path, &["--no-embed", "--no-onboard"]);
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("could not read the latest ljos release"),
        "{text}"
    );
    assert!(text.contains("Could not resolve host"), "{text}");
    assert!(text.contains("sh install.sh"), "{text}");
    assert!(text.contains("--version VER"), "{text}");
    assert!(text.contains("--asset-dir DIR"), "{text}");
}

/// The release answers, and the archive request fails. That is not a source build.
#[test]
fn a_release_that_answers_an_error_names_the_download() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(
        &bin,
        "curl",
        r#"#!/bin/sh
url=
for a in "$@"; do
  case $a in
    http*) url=$a ;;
  esac
done
case $url in
  *releases/latest*)
    printf '%s' 'https://github.com/leidarljos/ljos/releases/tag/v9.9.9'
    exit 0
    ;;
esac
echo 'curl: (22) The requested URL returned error: 500' >&2
printf '%s' '500'
exit 0
"#,
    );
    let home = dir.path().join("home");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let out = run_install(&home, &path, &["--no-embed", "--no-onboard"]);
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("could not download"), "{text}");
    assert!(text.contains("500"), "{text}");
    assert!(text.contains("sh install.sh"), "{text}");
    assert!(text.contains("--asset-dir DIR"), "{text}");
    assert!(
        !text.contains("building "),
        "a failed download started a source build:\n{text}"
    );
}

/// FreeBSD is not one of the four release archives.
#[test]
fn an_unsupported_os_names_the_machine_and_wsl() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(
        &bin,
        "uname",
        "#!/bin/sh\ncase \"$1\" in\n  -s) echo FreeBSD ;;\n  -m) echo amd64 ;;\n  *) echo FreeBSD ;;\nesac\n",
    );
    let home = dir.path().join("home");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new("dash")
        .arg(install_sh())
        .args(["--no-embed", "--no-onboard"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &path)
        .output()
        .unwrap();
    let text = said(&out);
    assert!(
        !out.status.success(),
        "status {:?}\n{text}",
        out.status.code()
    );
    let line = text
        .lines()
        .find(|line| line.contains("is not supported"))
        .unwrap_or("");
    assert!(line.contains("FreeBSD amd64 is not supported"), "{text}");
    assert!(
        line.contains("Supported: Linux and macOS on x86_64 and aarch64"),
        "{text}"
    );
    assert!(line.contains(SOURCE_CMD), "{text}");
    assert!(line.contains("WSL"), "{text}");
    assert_eq!(
        text.lines().filter(|line| !line.is_empty()).count(),
        1,
        "unsupported OS printed more than one line:\n{text}"
    );
}

/// tar is how a release archive is unpacked. A missing tar stops before that.
#[test]
fn a_missing_tar_names_the_install_command() {
    let dir = tempfile::tempdir().unwrap();
    let sys = dir.path().join("sys");
    std::fs::create_dir_all(&sys).unwrap();
    for name in [
        "sh",
        "mktemp",
        "date",
        "cp",
        "chmod",
        "mkdir",
        "rm",
        "mv",
        "cat",
        "grep",
        "sed",
        "awk",
        "find",
        "uname",
        "head",
        "sort",
        "dirname",
        "sha256sum",
    ] {
        for root in ["/usr/bin", "/bin"] {
            let found = Path::new(root).join(name);
            if found.is_file() {
                std::os::unix::fs::symlink(&found, sys.join(name)).unwrap();
                break;
            }
        }
    }
    let home = dir.path().join("home");
    let out = run_install(
        &home,
        &sys.display().to_string(),
        &[
            "--no-embed",
            "--no-onboard",
            "--asset-dir",
            "/no/such/assets",
        ],
    );
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("missing tar"), "{text}");
    assert!(text.contains("sh install.sh"), "{text}");
    assert!(text.contains("--from-dir DIR"), "{text}");
}

/// curl and wget are both absent, so nothing can fetch the release.
#[test]
fn a_missing_curl_names_how_to_install_one() {
    let dir = tempfile::tempdir().unwrap();
    let sys = dir.path().join("sys");
    std::fs::create_dir_all(&sys).unwrap();
    for name in ["sh", "uname", "mkdir", "date"] {
        for root in ["/usr/bin", "/bin"] {
            let found = Path::new(root).join(name);
            if found.is_file() {
                std::os::unix::fs::symlink(&found, sys.join(name)).unwrap();
                break;
            }
        }
    }
    let home = dir.path().join("home");
    let out = run_install(&home, &sys.display().to_string(), &["--no-embed"]);
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("curl"), "{text}");
    assert!(text.contains("wget"), "{text}");
    assert!(text.contains("missing curl or wget"), "{text}");
    assert!(text.contains("sh install.sh"), "{text}");
    assert!(text.contains("--from-dir DIR"), "{text}");
}

/// The force-push sample keeps its bypass log out of the seat's data
/// directory, and still reads the pack home onboard made. A sample that
/// loses that home prints a token warning the seat does not have.
#[test]
fn the_policy_sample_keeps_the_pack_home() {
    let dir = tempfile::tempdir().unwrap();
    let progs = dir.path().join("progs");
    std::fs::create_dir_all(&progs).unwrap();
    for name in PROGRAMS {
        script(&progs, name, "#!/bin/sh\nexit 0\n");
    }
    script(
        &progs,
        "ljos",
        "#!/bin/sh\n\
         case \"$1\" in\n\
           --version) echo ljos-test 0; exit 0 ;;\n\
           policy)\n\
             printf '%s\\n' \"PACKSET_HOME=${PACKSET_HOME-unset}\"\n\
             printf '%s\\n' \"XDG_DATA_HOME=${XDG_DATA_HOME-unset}\"\n\
             exit 0 ;;\n\
         esac\n\
         exit 0\n",
    );
    let home = dir.path().join("home");
    let text = install(&home, &progs, "/usr/bin:/bin", &[]);
    let pack = home.join(".local/share/packset");
    assert!(
        text.contains(&format!("PACKSET_HOME={}", pack.display())),
        "{text}"
    );
    assert!(
        home.join(".local/state/ljos").is_dir(),
        "the install left the state directory missing"
    );
    assert!(text.contains("/policy-demo"), "{text}");
    assert!(
        !text.contains("no token"),
        "the sample said the search had no token:\n{text}"
    );
}

/// An older ljos earlier on PATH is named, and this install is put first.
#[test]
fn an_older_ljos_earlier_on_path_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let progs = dir.path().join("progs");
    std::fs::create_dir_all(&progs).unwrap();
    for name in PROGRAMS {
        script(
            &progs,
            name,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo \"$0 0.2.0\"; fi\nexit 0\n",
        );
    }
    let old = dir.path().join("old");
    std::fs::create_dir_all(&old).unwrap();
    script(
        &old,
        "ljos",
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'ljos 0.1.0'; fi\necho \"old-ljos $*\"\nexit 0\n",
    );
    let home = dir.path().join("home");
    let bin = home.join(".local/bin");
    let path = format!("{}:{}:/usr/bin:/bin", old.display(), bin.display());
    let out = Command::new("sh")
        .arg(install_sh())
        .arg("--from-dir")
        .arg(&progs)
        .args(["--no-embed", "--no-onboard", "--no-modify-path"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &path)
        .output()
        .unwrap();
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("ljos 0.1.0"), "{text}");
    assert!(
        text.contains(&old.join("ljos").display().to_string()),
        "{text}"
    );
    assert!(text.contains("Put the new install first"), "{text}");
    assert!(text.contains(PATH_LINE), "{text}");
    assert!(text.contains("ljos doctor"), "{text}");
    assert!(
        !text.contains("old-ljos"),
        "the policy sample ran the older ljos:\n{text}"
    );
}

/// A second run says the programs were already there and replaces them.
#[test]
fn a_second_install_says_it_replaced_the_programs() {
    let dir = tempfile::tempdir().unwrap();
    let progs = dir.path().join("progs");
    std::fs::create_dir_all(&progs).unwrap();
    for name in PROGRAMS {
        script(
            &progs,
            name,
            &format!(
                "#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{name} 0.2.0'; fi\nexit 0\n"
            ),
        );
    }
    let home = dir.path().join("home");
    let bin = home.join(".local/bin");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let args = [
        "--from-dir",
        progs.to_str().unwrap(),
        "--no-embed",
        "--no-onboard",
    ];
    let first = run_install(&home, &path, &args);
    assert!(first.status.success(), "{}", said(&first));
    let second = run_install(&home, &path, &args);
    let text = said(&second);
    assert!(second.status.success(), "{text}");
    assert!(
        text.contains("ljos 0.2.0 is already installed at"),
        "{text}"
    );
    assert!(text.contains("Reinstalled the same version."), "{text}");
    assert!(text.contains("Next: `ljos doctor`."), "{text}");
}

/// Replacing ljos-mcp while a process still has the old file mapped.
#[test]
fn upgrading_over_a_running_mcp_server_names_the_pid() {
    let dir = tempfile::tempdir().unwrap();
    let progs = dir.path().join("progs");
    std::fs::create_dir_all(&progs).unwrap();
    for name in PROGRAMS {
        script(
            &progs,
            name,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo \"$0 0.2.0\"; fi\nexit 0\n",
        );
    }
    let home = dir.path().join("home");
    let bin = home.join(".local/bin");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let first = Command::new("sh")
        .arg(install_sh())
        .arg("--from-dir")
        .arg(&progs)
        .args(["--no-embed", "--no-onboard", "--no-modify-path"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(first.status.success(), "{}", said(&first));
    let sleep = ["/bin/sleep", "/usr/bin/sleep"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .unwrap();
    std::fs::copy(&sleep, bin.join("ljos-mcp")).unwrap();
    let mut child = Command::new(bin.join("ljos-mcp"))
        .arg("60")
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let second = Command::new("sh")
        .arg(install_sh())
        .arg("--from-dir")
        .arg(&progs)
        .args(["--no-embed", "--no-onboard", "--no-modify-path"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &path)
        .output()
        .unwrap();
    let _ = child.kill();
    let _ = child.wait();
    let text = said(&second);
    assert!(second.status.success(), "{text}");
    assert!(
        text.contains(&format!("ljos-mcp is still running (pid {pid})")),
        "{text}"
    );
    assert!(text.contains(&format!("kill {pid}")), "{text}");
    assert!(text.contains("previous binary"), "{text}");
}

fn link_tools(dir: &Path, names: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    for name in names {
        for root in ["/usr/bin", "/bin"] {
            let found = Path::new(root).join(name);
            if found.is_file() {
                std::os::unix::fs::symlink(&found, dir.join(name)).unwrap();
                break;
            }
        }
    }
}

fn sha256_file(bytes: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("body");
    std::fs::write(&file, bytes).unwrap();
    let out = Command::new("sha256sum").arg(&file).output().unwrap();
    assert!(out.status.success(), "{}", said(&out));
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// `sha256sum` is how a download is checked. `shasum` is the macOS name.
#[test]
fn a_missing_checksum_tool_names_sha256sum_and_shasum() {
    let dir = tempfile::tempdir().unwrap();
    let sys = dir.path().join("sys");
    link_tools(
        &sys,
        &[
            "sh", "mktemp", "date", "cp", "chmod", "mkdir", "rm", "mv", "cat", "grep", "sed",
            "awk", "find", "uname", "head", "sort", "dirname", "tar",
        ],
    );
    let home = dir.path().join("home");
    let out = run_install(
        &home,
        &sys.display().to_string(),
        &[
            "--no-embed",
            "--no-onboard",
            "--asset-dir",
            "/no/such/assets",
        ],
    );
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("sha256sum"), "{text}");
    assert!(text.contains("shasum"), "{text}");
    assert!(text.contains("sh install.sh"), "{text}");
}

/// A bin directory the person cannot write names `--bin-dir`.
#[test]
fn a_bin_dir_that_is_not_writable_names_bin_dir() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let locked = dir.path().join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    let mut perms = std::fs::metadata(&locked).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o555);
    std::fs::set_permissions(&locked, perms).unwrap();
    let out = run_install(
        &home,
        "/usr/bin:/bin",
        &[
            "--from-dir",
            "/no/such/progs",
            "--no-embed",
            "--no-onboard",
            "--bin-dir",
            locked.to_str().unwrap(),
        ],
    );
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("is not writable"), "{text}");
    assert!(text.contains("--bin-dir"), "{text}");
    assert!(text.contains(locked.to_str().unwrap()), "{text}");
}

/// The default install names the bash, zsh and fish lines and writes none of them.
#[test]
fn bin_dir_off_path_names_each_shell_and_does_not_edit_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let progs = dir.path().join("progs");
    std::fs::create_dir_all(&progs).unwrap();
    for name in PROGRAMS {
        script(&progs, name, "#!/bin/sh\nexit 0\n");
    }
    let home = dir.path().join("home");
    let text = install(&home, &progs, "/usr/bin:/bin", &[]);
    assert!(text.contains("bash (~/.bashrc):"), "{text}");
    assert!(text.contains("zsh (~/.zshrc):"), "{text}");
    assert!(
        text.contains("fish (~/.config/fish/config.fish):"),
        "{text}"
    );
    assert!(text.contains(PATH_LINE), "{text}");
    assert!(text.contains("fish_add_path $HOME/.local/bin"), "{text}");
    assert!(text.contains("does not edit that file"), "{text}");
    for rc in [".profile", ".bashrc", ".zshrc", ".config/fish/config.fish"] {
        assert!(!home.join(rc).exists(), "{rc} was written");
    }
}

/// A second install of an older ljos names both versions.
#[test]
fn an_older_install_names_both_versions() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let bin = home.join(".local/bin");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let old = dir.path().join("old");
    std::fs::create_dir_all(&old).unwrap();
    for name in PROGRAMS {
        script(
            &old,
            name,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo ljos 0.1.0; fi\nexit 0\n",
        );
    }
    let newer = dir.path().join("new");
    std::fs::create_dir_all(&newer).unwrap();
    for name in PROGRAMS {
        script(
            &newer,
            name,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo ljos 0.2.0; fi\nexit 0\n",
        );
    }
    let first = Command::new("sh")
        .arg(install_sh())
        .arg("--from-dir")
        .arg(&old)
        .args(["--no-embed", "--no-onboard", "--no-modify-path"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(first.status.success(), "{}", said(&first));
    let second = Command::new("sh")
        .arg(install_sh())
        .arg("--from-dir")
        .arg(&newer)
        .args(["--no-embed", "--no-onboard", "--no-modify-path"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &path)
        .output()
        .unwrap();
    let text = said(&second);
    assert!(second.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "Updated ljos at {} from 0.1.0 to 0.2.0.",
            bin.display()
        )),
        "{text}"
    );
}

const FAKE_CURL: &str = r#"#!/bin/sh
out=
url=
prev=
for a in "$@"; do
  case $prev in
    -o) out=$a ;;
  esac
  case $a in
    http://*|https://*) url=$a ;;
  esac
  prev=$a
done
case $url in
  */releases/tag/*|*/releases/tags/*)
    case "$LJOS_FAKE_TAG" in
      missing)
        printf '%s' 404
        exit 0
        ;;
    esac
    printf '%s' 200
    exit 0
    ;;
esac
case "$LJOS_FAKE_MODE" in
  truncate)
    printf '%s' partial > "$out"
    echo 'curl: (18) transfer closed with outstanding read data remaining' >&2
    exit 18
    ;;
  missing)
    printf '%s' 404
    exit 0
    ;;
esac
case $url in
  *.sha256)
    printf '%s\n' "$LJOS_FAKE_SHA" > "$out"
    printf '%s' 200
    exit 0
    ;;
esac
printf '%s' "$LJOS_FAKE_BODY" > "$out"
printf '%s' 200
exit 0
"#;

fn logging_rm(dir: &Path) {
    script(
        dir,
        "rm",
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$LJOS_RM_LOG\"\nexec /bin/rm \"$@\"\n",
    );
}

/// A download whose checksum does not match is deleted, and the line says to rerun.
#[test]
fn a_checksum_mismatch_removes_the_partial_file_and_says_to_rerun() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(&bin, "curl", FAKE_CURL);
    logging_rm(&bin);
    let log = dir.path().join("rm.log");
    let home = dir.path().join("home");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let asset = "ljos-v0.0.1-x86_64-unknown-linux-gnu.tar.gz";
    let out = Command::new("sh")
        .arg(install_sh())
        .args([
            "--no-embed",
            "--no-onboard",
            "--no-modify-path",
            "--version",
            "0.0.1",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--bin-dir",
        ])
        .arg(home.join(".local/bin"))
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &path)
        .env("LJOS_FAKE_MODE", "mismatch")
        .env("LJOS_FAKE_BODY", "not-the-archive")
        .env("LJOS_FAKE_SHA", sha256_file(b"the-archive"))
        .env("LJOS_RM_LOG", &log)
        .output()
        .unwrap();
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("checksum mismatch"), "{text}");
    assert!(text.contains("Removed the partial file"), "{text}");
    assert!(text.contains("sh install.sh"), "{text}");
    assert!(!home.join(".local/bin/ljos").exists(), "{text}");
    let removed = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        removed.contains(asset),
        "the partial archive was not deleted:\n{removed}\n{text}"
    );
}

/// A download that stops early is deleted, and the line says to rerun.
#[test]
fn a_truncated_download_removes_the_partial_file_and_says_to_rerun() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(&bin, "curl", FAKE_CURL);
    logging_rm(&bin);
    let log = dir.path().join("rm.log");
    let home = dir.path().join("home");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let asset = "ljos-v0.0.1-x86_64-unknown-linux-gnu.tar.gz";
    let out = Command::new("sh")
        .arg(install_sh())
        .args([
            "--no-embed",
            "--no-onboard",
            "--no-modify-path",
            "--version",
            "0.0.1",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--bin-dir",
        ])
        .arg(home.join(".local/bin"))
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &path)
        .env("LJOS_FAKE_MODE", "truncate")
        .env("LJOS_RM_LOG", &log)
        .output()
        .unwrap();
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("truncated"), "{text}");
    assert!(text.contains("Removed the partial file"), "{text}");
    assert!(text.contains("sh install.sh"), "{text}");
    assert!(!home.join(".local/bin/ljos").exists(), "{text}");
    let removed = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        removed.contains(asset),
        "the partial archive was not deleted:\n{removed}\n{text}"
    );
}

/// `--version` for a release GitHub does not have names the releases page.
#[test]
fn a_pinned_version_with_no_release_says_how_to_list_versions() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(&bin, "curl", FAKE_CURL);
    script(
        &bin,
        "cargo",
        "#!/bin/sh\necho ran >> \"$LJOS_CARGO_LOG\"\nexit 1\n",
    );
    let cargo_log = dir.path().join("cargo.log");
    let home = dir.path().join("home");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let out = Command::new("sh")
        .arg(install_sh())
        .args([
            "--no-embed",
            "--no-onboard",
            "--no-modify-path",
            "--version",
            "9.9.9",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--bin-dir",
        ])
        .arg(home.join(".local/bin"))
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &path)
        .env("LJOS_FAKE_MODE", "missing")
        .env("LJOS_FAKE_TAG", "missing")
        .env("LJOS_CARGO_LOG", &cargo_log)
        .output()
        .unwrap();
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("ljos 9.9.9 has no release"), "{text}");
    assert!(
        text.contains("https://github.com/leidarljos/ljos/releases"),
        "{text}"
    );
    assert!(text.contains("sh install.sh --version VER"), "{text}");
    assert!(
        !cargo_log.exists(),
        "a missing release started cargo:\n{text}"
    );
}

/// The sibling pin names vissue 0.23.1, and that version has no GitHub
/// release. A machine without Rust still finishes: the script says so
/// and installs the latest vissue release. Cargo is not started.
#[test]
fn a_default_pin_with_no_release_installs_the_latest() {
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    let target = "x86_64-unknown-linux-gnu";
    let packs = [
        ("ljos-v0.42.1", &["ljos", "ljos-mcp"][..]),
        (
            "packset-v0.17.10",
            &["packset", "packsetd", "packset-mcp"][..],
        ),
        ("vissue-v0.24.0", &["vissue", "vissue-mcp"][..]),
        ("deedar-v0.4.0", &["deedar"][..]),
        ("claimdag-v0.8.0", &["claimdag"][..]),
        ("ljos-policyd-v0.7.0", &["ljos-policyd"][..]),
        ("consensus-v0.10.1", &["ljos-consensus"][..]),
    ];
    for (stem, bins) in packs {
        let top = assets.join(stem);
        std::fs::create_dir_all(&top).unwrap();
        for bin in bins {
            script(&top, bin, "#!/bin/sh\nprintf '%s\\n' from-latest\nexit 0\n");
        }
        let archive = assets.join(format!("{stem}-{target}.tar.gz"));
        let packed = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&assets)
            .arg(stem)
            .status()
            .unwrap();
        assert!(packed.success(), "tar {stem}");
        let sum = format!("{}.sha256", archive.display());
        std::fs::write(&sum, sha256_of(&archive)).unwrap();
    }
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(
        &bin,
        "curl",
        r#"#!/bin/sh
out=
url=
prev=
for a in "$@"; do
  case $prev in
    -o) out=$a ;;
  esac
  case $a in
    http://*|https://*) url=$a ;;
  esac
  prev=$a
done
repo=
case $url in
  *github.com/leidarljos/*/releases/*)
    repo=${url#*github.com/leidarljos/}
    repo=${repo%%/*}
    ;;
esac
case $url in
  */releases/latest)
    ver=0.42.1
    case $repo in
      packset) ver=0.17.10 ;;
      vissue) ver=0.24.0 ;;
      deedar) ver=0.4.0 ;;
      claimdag) ver=0.8.0 ;;
      ljos-policyd) ver=0.7.0 ;;
      consensus) ver=0.10.1 ;;
    esac
    printf '%s' "https://github.com/leidarljos/$repo/releases/tag/v$ver"
    exit 0
    ;;
  */releases/tag/v0.23.1|*/releases/tag/v0.23.1/*)
    printf '%s' 404
    exit 0
    ;;
  */releases/tag/*)
    printf '%s' 200
    exit 0
    ;;
esac
case $url in
  *vissue-v0.23.1-*)
    printf '%s' 404
    exit 0
    ;;
  *.sha256)
    base=$(basename "$url" .sha256)
    cp "$LJOS_FAKE_ASSETS/$base.sha256" "$out"
    printf '%s' 200
    exit 0
    ;;
esac
base=$(basename "$url")
cp "$LJOS_FAKE_ASSETS/$base" "$out"
printf '%s' 200
exit 0
"#,
    );
    script(&bin, "gh", "#!/bin/sh\nexit 1\n");
    script(&bin, "cosign", "#!/bin/sh\nexit 1\n");
    script(
        &bin,
        "cargo",
        "#!/bin/sh\necho ran >> \"$LJOS_CARGO_LOG\"\nexit 1\n",
    );
    let cargo_log = dir.path().join("cargo.log");
    let home = dir.path().join("home");
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let out = Command::new("sh")
        .arg(install_sh())
        .args([
            "--no-embed",
            "--no-onboard",
            "--no-modify-path",
            "--target",
            target,
            "--bin-dir",
        ])
        .arg(home.join(".local/bin"))
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &path)
        .env("LJOS_FAKE_ASSETS", &assets)
        .env("LJOS_CARGO_LOG", &cargo_log)
        .output()
        .unwrap();
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(
            "vissue 0.23.1 has no release archive. Installing the latest vissue release."
        ),
        "{text}"
    );
    assert!(!cargo_log.exists(), "cargo was started:\n{text}");
    let installed = home.join(".local/bin/vissue");
    assert!(installed.is_file(), "{text}");
    let ran = Command::new(&installed).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "from-latest");
}

fn sha256_of(path: &Path) -> String {
    let hex = |out: std::process::Output| -> Option<String> {
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .map(|word| format!("{word}\n"))
    };
    if let Some(line) = Command::new("sha256sum")
        .arg(path)
        .output()
        .ok()
        .and_then(hex)
    {
        return line;
    }
    Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .ok()
        .and_then(hex)
        .expect("sha256sum or shasum")
}

#[test]
fn install_sh_passes_shellcheck() {
    let out = Command::new("shellcheck")
        .args(["-s", "sh"])
        .arg(install_sh())
        .output()
        .expect("shellcheck");
    let text = said(&out);
    assert!(out.status.success(), "{text}");
}
