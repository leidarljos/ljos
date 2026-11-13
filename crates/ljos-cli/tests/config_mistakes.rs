//! Config mistakes are warnings. Each one is visible from `ljos doctor`,
//! `ljos recall`, `ljos remember` and MCP startup. A hook and the MCP
//! server still run.

ljos_test_boot::harness!();

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

struct World {
    root: PathBuf,
    home: PathBuf,
    project: PathBuf,
}

fn world() -> World {
    let root = tempfile::tempdir().unwrap().keep();
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(home.join(".config/ljos")).unwrap();
    std::fs::create_dir_all(home.join(".local/share")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    std::fs::create_dir_all(home.join(".cache")).unwrap();
    std::fs::create_dir_all(root.join("run")).unwrap();
    std::fs::create_dir_all(root.join("tmp")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    World {
        root,
        home,
        project,
    }
}

impl World {
    fn user_toml(&self) -> PathBuf {
        self.home.join(".config/ljos/ljos.toml")
    }

    fn write_user(&self, body: &str) {
        std::fs::write(self.user_toml(), body).unwrap();
    }

    fn write_project(&self, body: &str) {
        std::fs::write(self.project.join("ljos.toml"), body).unwrap();
    }

    fn command(&self, args: &[&str], extra: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
        cmd.args(args)
            .current_dir(&self.project)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("PACKSET_URL", "off")
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_SEAT", "saddle")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LANG", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra {
            cmd.env(key, value);
        }
        cmd
    }

    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> (i32, String, String) {
        let out = self.command(args, extra).output().unwrap();
        (
            out.status.code().unwrap_or(1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn hook(&self, extra: &[(&str, &str)]) -> (i32, String) {
        let mut child = self
            .command(&["hook", "--event", "UserPromptSubmit"], extra)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(br#"{"hook_event_name":"UserPromptSubmit","prompt":"hi"}"#)
            .unwrap();
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(1),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn mcp(&self, extra: &[(&str, &str)]) -> (bool, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
        cmd.current_dir(&self.project)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("PACKSET_URL", "off")
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_SEAT", "saddle")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra {
            cmd.env(key, value);
        }
        let mut child = cmd.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"probe","version":"0"}}}}}}"#
        )
        .unwrap();
        stdin.flush().unwrap();
        drop(stdin);
        let stderr = child.stderr.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let started = std::time::Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead as _;
            let mut answer = String::new();
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.contains("\"id\":1") || line.contains("\"id\": 1") {
                    answer = line;
                    break;
                }
            }
            let err = std::io::read_to_string(stderr).unwrap_or_default();
            let _ = tx.send((answer, err));
        });
        let (answer, err) = rx.recv_timeout(Duration::from_secs(20)).unwrap_or_default();
        let _ = child.kill();
        let _ = child.wait();
        let answered = answer.contains("serverInfo") || answer.contains("\"result\"");
        let _ = started;
        (answered, err)
    }
}

fn shows(world: &World, extra: &[(&str, &str)], needle: &str) {
    let (doctor_code, doctor_out, doctor_err) = world.run(&["doctor"], extra);
    let doctor = format!("{doctor_out}{doctor_err}");
    assert!(doctor.contains(needle), "doctor missed {needle}:\n{doctor}");
    assert!(
        doctor.contains("config"),
        "doctor did not name config:\n{doctor}"
    );

    let (recall_code, _, recall_err) = world.run(&["recall", "demo"], extra);
    assert!(
        recall_err.contains(needle),
        "recall missed {needle}:\n{recall_err}"
    );

    let (remember_code, _, remember_err) = world.run(&["remember", "a note"], extra);
    assert!(
        remember_err.contains(needle),
        "remember missed {needle}:\n{remember_err}"
    );

    let (hook_code, hook_err) = world.hook(extra);
    assert_eq!(hook_code, 0, "hook failed on a warning:\n{hook_err}");
    assert!(
        hook_err.contains(needle),
        "hook missed {needle}:\n{hook_err}"
    );

    let (mcp_up, mcp_err) = world.mcp(extra);
    assert!(mcp_up, "MCP server did not start:\n{mcp_err}");
    assert!(
        mcp_err.contains(needle),
        "MCP startup missed {needle}:\n{mcp_err}"
    );

    let bare = World {
        root: world.root.clone(),
        home: world.home.clone(),
        project: world.project.clone(),
    };
    let _ = bare;
    let _ = (doctor_code, recall_code, remember_code);
}

fn quiet_exit(world: &World, extra: &[(&str, &str)]) -> (i32, i32, i32) {
    let (doctor, _, _) = world.run(&["doctor"], extra);
    let (recall, _, _) = world.run(&["recall", "demo"], extra);
    let (remember, _, _) = world.run(&["remember", "a note"], extra);
    (doctor, recall, remember)
}

#[test]
fn a_typed_key_is_quiet_and_doctor_names_the_file() {
    let world = world();
    world.write_user("quorum = 4\nrewrite = false\ngraphify = \"off\"\n");
    let (code, out, err) = world.run(&["doctor"], &[]);
    let text = format!("{out}{err}");
    assert!(!text.contains("warning:"), "{text}");
    assert!(
        text.contains(&format!("quorum is 4 from {}", world.user_toml().display())),
        "{text}"
    );
    assert!(
        text.contains("LJOS_SEAT is saddle from LJOS_SEAT"),
        "{text}"
    );
    let _ = code;
}

#[test]
fn a_typo_names_the_nearest_key() {
    let world = world();
    world.write_user("quorun = 2\n");
    shows(&world, &[], "unknown key `quorun`");
    let (_, out, err) = world.run(&["doctor"], &[]);
    let text = format!("{out}{err}");
    assert!(text.contains("nearest is `quorum`"), "{text}");
}

#[test]
fn a_key_in_the_wrong_section_names_where_it_belongs() {
    let world = world();
    world.write_user("[identity]\nquorum = 3\n");
    shows(&world, &[], "key `quorum` under [identity]");
    let (_, out, err) = world.run(&["doctor"], &[]);
    let text = format!("{out}{err}");
    assert!(
        text.contains("it belongs [gates] or the top of the file")
            || text.contains("it belongs the top of the file or [gates]"),
        "{text}"
    );
}

#[test]
fn a_string_where_a_number_belongs_names_the_key_and_the_type() {
    let world = world();
    world.write_user("quorum = \"two\"\nrewrite = \"yes\"\n");
    shows(&world, &[], "key `quorum`");
    let (_, out, err) = world.run(&["doctor"], &[]);
    let text = format!("{out}{err}");
    assert!(
        text.contains("key `quorum`") && text.contains("expected a whole number"),
        "{text}"
    );
    assert!(
        text.contains("key `rewrite`") && text.contains("expected true or false"),
        "{text}"
    );
}

#[test]
fn a_missing_path_names_the_key_and_the_path() {
    let world = world();
    let missing = world.root.join("no-such-graph.json");
    world.write_user(&format!("graphify = \"{}\"\n", missing.display()));
    shows(
        &world,
        &[],
        &format!("names {}, which does not exist", missing.display()),
    );
}

#[test]
fn an_unreadable_file_names_the_file() {
    let world = world();
    world.write_user("quorum = 2\n");
    let path = world.user_toml();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    shows(&world, &[], &format!("{}: cannot be read", path.display()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
}

#[test]
fn invalid_toml_names_the_file_line_and_column() {
    let world = world();
    world.write_user("quorum = [\n");
    let path = world.user_toml();
    shows(
        &world,
        &[],
        &format!("{}:1:12: invalid TOML", path.display()),
    );
    shows(&world, &[], "so run `ljos doctor`.");
}

#[test]
fn an_unknown_ljos_variable_names_the_nearest_one() {
    let world = world();
    world.write_user("quorum = 2\n");
    shows(
        &world,
        &[("LJOS_SEET", "1")],
        "unknown LJOS_ variable `LJOS_SEET`",
    );
    let (_, _, err) = world.run(&["recall", "demo"], &[("LJOS_SEET", "1")]);
    assert!(err.contains("nearest is `LJOS_SEAT`"), "{err}");
}

#[test]
fn an_environment_value_wins_and_names_both() {
    let world = world();
    let graph = world.root.join("home-graph.json");
    std::fs::write(&graph, "{}\n").unwrap();
    world.write_user(&format!("graphify = \"{}\"\n", graph.display()));
    shows(&world, &[("LJOS_GRAPHIFY", "off")], "LJOS_GRAPHIFY is used");
    let (_, out, err) = world.run(&["doctor"], &[("LJOS_GRAPHIFY", "off")]);
    let text = format!("{out}{err}");
    assert!(
        text.contains("graphify is off from LJOS_GRAPHIFY"),
        "{text}"
    );
    assert!(text.contains(&graph.display().to_string()), "{text}");
}

#[test]
fn a_project_file_shadows_the_home_file() {
    let world = world();
    let home_graph = world.root.join("home-graph.json");
    let project_graph = world.root.join("project-graph.json");
    std::fs::write(&home_graph, "{}\n").unwrap();
    std::fs::write(&project_graph, "{}\n").unwrap();
    world.write_user(&format!(
        "graphify = \"{}\"\nquorum = 4\n",
        home_graph.display()
    ));
    world.write_project(&format!(
        "graphify = \"{}\"\nquorum = 9\n",
        project_graph.display()
    ));
    let project = world.project.join("ljos.toml");
    let home = world.user_toml();
    shows(
        &world,
        &[],
        &format!(
            "{} sets graphify and shadows {}",
            project.display(),
            home.display()
        ),
    );
    let (_, out, err) = world.run(&["doctor"], &[]);
    let text = format!("{out}{err}");
    assert!(
        text.contains(&format!(
            "graphify is {} from {}",
            project_graph.display(),
            project.display()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("quorum is 9 from {}", project.display())),
        "{text}"
    );
}

#[test]
fn a_warning_does_not_change_the_exit_of_recall_or_remember() {
    let world = world();
    world.write_user("quorum = 2\n");
    let clean = quiet_exit(&world, &[]);
    world.write_user("quorum = [\n");
    let broken = quiet_exit(&world, &[]);
    assert_eq!(clean, broken, "a warning changed the exit code");
    let (hook, _) = world.hook(&[]);
    assert_eq!(hook, 0, "hook failed");
}
