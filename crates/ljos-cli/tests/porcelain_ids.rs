//! `ljos file` prints the status line, the file and the tracker lines on
//! stdout, so `id=$(ljos file ...)` is not an id and `ljos sitting $id`
//! is given the rest of that line. `-q` and `--porcelain` print only the
//! new id. `ljos deed` does the same for an accession it records.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

struct World {
    _dir: tempfile::TempDir,
    home: std::path::PathBuf,
    path: String,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        let vissue = format!(
            r#"#!/bin/sh
echo "vissue $*" >> '{log}'
quiet=0
for a in "$@"; do
  [ "$a" = -q ] && quiet=1
done
case "$1" in
  create)
    if [ "$quiet" = 1 ]; then
      printf '%s\n' papers-2jco
    else
      printf '%s\n' 'papers-2jco  TODO  [#A]  title here' 'file: /tmp/papers/issues.org'
    fi
    ;;
  deed)
    if [ "$3" = --add ]; then
      printf '%s\n' "papers-2jco: deeds += $4" 'papers-2jco: 1 deed' "  $4"
    else
      printf '%s\n' 'papers-2jco: 1 deed' '  deed-file-survey'
    fi
    ;;
esac
exit 0
"#,
            log = log.display()
        );
        let deedar = "#!/bin/sh\nprintf 'id=%s kind=file\\n' \"$2\"\nexit 0\n";
        for (name, body) in [("vissue", vissue.as_str()), ("deedar", deedar)] {
            let path = dir.path().join(name);
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("run")).unwrap();
        let path = format!(
            "{}:{}",
            dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Self {
            _dir: dir,
            home,
            path,
        }
    }

    fn ljos(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .env("LJOS_SEAT", "papers")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap()
    }
}

fn text(out: &Output) -> String {
    format!(
        "status {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn assert_only_id(out: &Output, id: &str) {
    let said = text(out);
    assert!(out.status.success(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{id}\n"),
        "{said}"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("tracker"), "{said}");
}

#[test]
fn a_plain_file_capture_is_not_an_id_sitting_can_take() {
    let world = World::new();
    let filed = world.ljos(&["file", "title here", "--top", "-p", "papers"]);
    let said = text(&filed);
    assert!(filed.status.success(), "{said}");
    let stdout = String::from_utf8_lossy(&filed.stdout);
    assert!(
        stdout.contains("papers-2jco  TODO  [#A]  title here"),
        "{said}"
    );
    assert!(stdout.contains("file:"), "{said}");
    assert!(stdout.contains("tracker"), "{said}");
    let words: Vec<&str> = stdout.split_whitespace().collect();
    let mut sit_args = Vec::with_capacity(words.len() + 1);
    sit_args.push("sitting");
    sit_args.extend(words);
    let sitting = world.ljos(&sit_args);
    let sit = text(&sitting);
    assert!(!sitting.status.success(), "{sit}");
    assert!(
        String::from_utf8_lossy(&sitting.stderr).contains("unexpected argument"),
        "{sit}"
    );
}

#[test]
fn porcelain_file_prints_only_the_new_id() {
    let world = World::new();
    for flag in ["-q", "--porcelain"] {
        let filed = world.ljos(&["file", flag, "title here", "--top", "-p", "papers"]);
        let said = text(&filed);
        assert_only_id(&filed, "papers-2jco");
        let err = String::from_utf8_lossy(&filed.stderr);
        assert!(
            err.contains("papers-2jco  TODO  [#A]  title here"),
            "{flag}: {said}"
        );
        assert!(err.contains("file:"), "{flag}: {said}");
        let id = String::from_utf8_lossy(&filed.stdout);
        let id = id.trim();
        let sitting = Command::new("timeout")
            .args(["30", env!("CARGO_BIN_EXE_ljos"), "sitting", id])
            .env_clear()
            .env("PATH", &world.path)
            .env("HOME", &world.home)
            .env("XDG_CONFIG_HOME", world.home.join(".config"))
            .env("XDG_STATE_HOME", world.home.join(".state"))
            .env("XDG_RUNTIME_DIR", world.home.join("run"))
            .env("LJOS_SEAT", "papers")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap();
        let sit = text(&sitting);
        let err = String::from_utf8_lossy(&sitting.stderr);
        assert!(
            !err.contains("unexpected argument"),
            "{flag}: sitting was not given the id alone: {sit}"
        );
    }
}

#[test]
fn porcelain_deed_prints_only_the_accession() {
    let world = World::new();
    let created = "id=deed-file-survey kind=file name=survey\naccession=deed-file-survey\n";
    for flag in ["-q", "--porcelain"] {
        let cited = world.ljos(&["deed", "papers-2jco", flag, "--add", created]);
        assert_only_id(&cited, "deed-file-survey");
        let err = String::from_utf8_lossy(&cited.stderr);
        assert!(
            err.contains("deeds += deed-file-survey"),
            "{flag}: {}",
            text(&cited)
        );
    }
    let listed = world.ljos(&["deed", "papers-2jco", "--porcelain"]);
    let said = text(&listed);
    assert!(listed.status.success(), "{said}");
    assert_eq!(
        String::from_utf8_lossy(&listed.stdout),
        "deed-file-survey\n",
        "{said}"
    );
    assert!(
        String::from_utf8_lossy(&listed.stderr).contains("papers-2jco: 1 deed"),
        "{said}"
    );
}
