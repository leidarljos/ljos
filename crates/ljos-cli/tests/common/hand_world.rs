//! A real pack, a real tracker and a checkout for a test to drive `ljos`
//! against. The tracker prefix is `day`, the project is `kiln`, and the seat
//! is `saddle`. A test that leaves a tool out returns before it opens one.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::tools;

pub struct World {
    _daemon: ljos_test_boot::Packsetd,
    pub home: tempfile::TempDir,
    pub root: PathBuf,
    pub port: u16,
    path: String,
}

/// Whether the tools a world needs are on the machine.
pub fn ready() -> Option<()> {
    if tools::halt(tools::Tool::Packsetd) || tools::halt(tools::Tool::Vissue) {
        return None;
    }
    Some(())
}

impl World {
    pub fn open() -> Self {
        if tools::halt(tools::Tool::Packsetd) || tools::halt(tools::Tool::Vissue) {
            panic!("halt returns before open when the tools are present");
        }
        let packsetd = on_path("packsetd").expect("packsetd");
        let home = tempfile::tempdir().expect("home");
        for sub in [
            "tmp",
            "run",
            ".config/ljos",
            ".local/share/packset",
            ".local/state/ljos",
            ".cache",
        ] {
            std::fs::create_dir_all(home.path().join(sub)).expect("home dir");
        }
        let root = home.path().join("tracker");
        std::fs::create_dir_all(&root).expect("tracker");
        std::fs::create_dir_all(root.join(".ljos/workflows")).expect("workflows");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(root.join(".ljos/project"), "kiln\n").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(root.join("vissue.toml"), "prefix = \"day\"\n").unwrap();
        git_init(&root);
        let bin_dir = Path::new(env!("CARGO_BIN_EXE_ljos"))
            .parent()
            .expect("bin dir");
        let sibling = std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".cache/ljos/sibling-bin"))
            .filter(|path| path.is_dir());
        let mut dirs = vec![
            bin_dir.to_path_buf(),
            packsetd.parent().unwrap().to_path_buf(),
        ];
        if let Some(sibling) = sibling {
            dirs.push(sibling);
        }
        dirs.push(PathBuf::from("/usr/bin"));
        dirs.push(PathBuf::from("/bin"));
        if let Some(path) = std::env::var_os("PATH") {
            dirs.extend(std::env::split_paths(&path));
        }
        let path = std::env::join_paths(dirs).unwrap().into_string().unwrap();
        let mut started = ljos_test_boot::Packsetd::build();
        started.bin(&packsetd);
        started.home_at(home.path());
        started.pack_at(home.path().join(".local/share/packset"));
        started.command().env("PACKSET_AUTH", "off");
        let daemon = started.spawn();
        let port = daemon.port();
        assert_ne!(port, 8761, "the tests leave port 8761 free");
        let world = Self {
            _daemon: daemon,
            home,
            root,
            port,
            path,
        };
        world.ticket("kiln", "kiln-seed", "Seed the tracker", None, "");
        world
    }

    pub fn ljos(&self, args: &[&str]) -> Output {
        self.ljos_with(args, &[])
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    pub fn ljos_with(&self, args: &[&str], extra: &[(&str, &str)]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ljos"))
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", self.home.path())
            .env("TMPDIR", self.home.path().join("tmp"))
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("XDG_DATA_HOME", self.home.path().join(".local/share"))
            .env("XDG_STATE_HOME", self.home.path().join(".local/state"))
            .env("XDG_CACHE_HOME", self.home.path().join(".cache"))
            .env("XDG_RUNTIME_DIR", self.home.path().join("run"))
            .env("PACKSET_PORT", self.port.to_string())
            .env("PACKSET_URL", format!("http://127.0.0.1:{}", self.port))
            .env(
                "PACKSET_HOME",
                self.home.path().join(".local/share/packset"),
            )
            .env("PACKSET_WORKSPACE", "hand")
            .env("PACKSET_AUTH", "off")
            .env("VISSUE_ROOT", &self.root)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .envs(extra.iter().copied())
            .output()
            .expect("spawn ljos")
    }

    pub fn ok(&self, args: &[&str]) -> String {
        let out = self.ljos(args);
        assert!(out.status.success(), "{args:?}\n{}", dump(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Every atom the pack keeps in this world's workspace.
    pub fn atoms(&self) -> Vec<serde_json::Value> {
        let url = format!("http://127.0.0.1:{}/v1/atoms?workspace=hand", self.port);
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let out = Command::new("curl")
            .args(["-sS", "-m", "10", &url])
            .output()
            .expect("curl");
        assert!(out.status.success(), "list atoms: {}", dump(&out));
        let doc: serde_json::Value =
            serde_json::from_slice(&out.stdout).expect("atoms are one JSON document");
        doc["atoms"].as_array().cloned().unwrap_or_default()
    }

    /// The live atom of `kind` whose text is `text`.
    pub fn atom(&self, kind: &str, text: &str) -> Option<serde_json::Value> {
        self.atoms()
            .into_iter()
            .find(|atom| atom["kind"] == kind && atom["text"] == text && atom["tombstone"] != true)
    }

    /// Write a lesson straight to the pack, with the entities given.
    pub fn post_lesson(&self, text: &str, entities: &[&str]) {
        let body = serde_json::json!({
            "schema": "inside.atom/v1",
            "kind": "lesson",
            "level": "explicit",
            "text": text,
            "workspace": "hand",
            "entities": entities,
            "source": {"seat": "saddle", "basis": "person"}
        });
        let file = self.home.path().join("atom.json");
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(&file, body.to_string()).unwrap();
        let url = format!("http://127.0.0.1:{}/v1/atoms", self.port);
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        let out = Command::new("curl")
            .args([
                "-sS",
                "-m",
                "10",
                "-X",
                "POST",
                &url,
                "-H",
                "content-type: application/json",
                "--data-binary",
                &format!("@{}", file.display()),
            ])
            .output()
            .expect("curl");
        assert!(out.status.success(), "post lesson: {}", dump(&out));
    }

    /// Create a ticket in the tracker. `tags` are heading tags.
    pub fn ticket(&self, project: &str, id: &str, title: &str, body: Option<&str>, tags: &str) {
        let layout = vissue_core::Layout::new(&self.root, "day");
        std::fs::create_dir_all(layout.projects_dir()).unwrap();
        let opts = vissue_core::CreateOpts {
            id: Some(id),
            body,
            tags: (!tags.is_empty()).then_some(tags),
            ..Default::default()
        };
        vissue_core::ops::create(&layout, project, title, opts)
            .unwrap_or_else(|err| panic!("create {id}: {err}"));
    }
}

pub fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .or_else(|| {
            let sibling = std::env::var_os("HOME").map(|home| {
                PathBuf::from(home)
                    .join(".cache/ljos/sibling-bin")
                    .join(name)
            })?;
            sibling.is_file().then_some(sibling)
        })
}

fn git_init(root: &Path) {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "Rohit Goswami")
        .env("GIT_AUTHOR_EMAIL", "rgoswami@ieee.org")
        .env("GIT_COMMITTER_NAME", "Rohit Goswami")
        .env("GIT_COMMITTER_EMAIL", "rgoswami@ieee.org")
        .output()
        .expect("git init");
    assert!(out.status.success(), "{}", dump(&out));
}

pub fn dump(out: &Output) -> String {
    format!(
        "status: {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}
