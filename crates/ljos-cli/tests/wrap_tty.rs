//! TTY wrapping behavior for doctor, ready, list JSON, and finish.

ljos_test_boot::harness!();

#[path = "common/pty.rs"]
mod pty;
#[path = "common/seat_env.rs"]
mod seat_env;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ljos_cli::output_wrap::{prose_display_width, wrap_hanging_prose};

fn write_exec(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    let mut perm = std::fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(path, perm).unwrap();
}

struct Board {
    root: tempfile::TempDir,
    tracker: PathBuf,
    home: PathBuf,
    id: String,
}

impl Board {
    fn open(title: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let tracker = root.path().join("tracker");
        let home = root.path().join("home");
        std::fs::create_dir_all(&tracker).unwrap();
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(home.join(".state")).unwrap();
        std::fs::create_dir_all(root.path().join("run")).unwrap();
        let layout = vissue_core::Layout::new(&tracker, "Software");
        std::fs::create_dir_all(layout.projects_dir()).unwrap();
        let made = vissue_core::ops::create(
            &layout,
            "probe",
            title,
            vissue_core::CreateOpts {
                quiet: true,
                ..vissue_core::CreateOpts::default()
            },
        )
        .unwrap();
        let id = made.trim().to_string();
        vissue_core::ops::claim_as(&layout, &id, false, "saddle").unwrap();
        Self {
            root,
            tracker,
            home,
            id,
        }
    }

    fn command(&self, args: &[&str], columns: usize) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ljos"));
        command
            .args(args)
            .current_dir(self.root.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("COLUMNS", columns.to_string());
        seat_env::clear(&mut command);
        command.env("LJOS_SEAT", "saddle");
        command
    }

    fn command_without_columns(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ljos"));
        command
            .args(args)
            .current_dir(self.root.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("VISSUE_ROOT", &self.tracker)
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", "saddle")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .env("LJOS_ASK_PACK", "off")
            .env("LJOS_USER_RULES_SYNC", "off");
        seat_env::clear(&mut command);
        command.env("LJOS_SEAT", "saddle");
        command
    }
}

fn pty_output(mut cmd: Command, secs: u64) -> String {
    let term = pty::Pty::open();
    cmd.stdin(Stdio::null());
    term.attach_output(&mut cmd);
    let child = term.spawn(&mut cmd);
    let out = term.wait_output(child, secs);
    term.finish_reading();
    let shown = term.shown();
    assert!(out.status.success(), "{shown}");
    shown.replace("\r\n", "\n")
}

fn pty_output_with_cols(mut cmd: Command, cols: u16, secs: u64) -> String {
    let term = pty::Pty::open();
    term.set_winsize(cols, 40);
    cmd.stdin(Stdio::null());
    term.attach_output(&mut cmd);
    let child = term.spawn(&mut cmd);
    let out = term.wait_output(child, secs);
    term.finish_reading();
    let shown = term.shown();
    assert!(out.status.success(), "{shown}");
    shown.replace("\r\n", "\n")
}

fn pty_output_any_status(mut cmd: Command, secs: u64) -> (std::process::Output, String) {
    let term = pty::Pty::open();
    cmd.stdin(Stdio::null());
    term.attach_output(&mut cmd);
    let child = term.spawn(&mut cmd);
    let out = term.wait_output(child, secs);
    term.finish_reading();
    let shown = term.shown().replace("\r\n", "\n");
    (out, shown)
}

fn non_tty_output(mut cmd: Command) -> String {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

#[test]
fn ready_on_a_tty_wraps_and_keeps_alignment() {
    let board = Board::open(
        "This is a deliberately long title to verify that ready wraps at word boundaries in narrow terminals",
    );
    let shown = pty_output(board.command(&["ready"], 40), 10);
    assert!(shown.contains("ready  shown 1 of 1"), "{shown}");
    assert!(shown.contains("STARTED  [#"), "{shown}");
    assert!(!shown.contains("rea\ndy"), "{shown}");
    assert!(shown.contains("This is a\n"), "{shown}");
    assert!(shown.contains("ready wraps\n"), "{shown}");
    assert!(
        shown.lines().all(|line| line.chars().count() <= 40),
        "{shown}"
    );
}

#[test]
fn ready_on_a_tty_at_28_cols_wraps_once_without_overflow() {
    let board = Board::open(
        "This is a deliberately long title to verify single-pass wrapping at narrow widths",
    );
    let shown = pty_output(board.command(&["ready"], 28), 10);
    assert!(shown.contains("ready  shown 1 of 1"), "{shown}");
    for line in shown.lines() {
        assert!(line.chars().count() <= 28, "{shown}");
    }
}

#[test]
fn ready_json_on_a_tty_stays_valid_json() {
    let board = Board::open(
        "This is a deliberately long title to verify JSON stays valid on a narrow terminal",
    );
    let shown = pty_output(board.command(&["ready", "--json"], 40), 10);
    let parsed: serde_json::Value =
        serde_json::from_str(shown.trim()).unwrap_or_else(|err| panic!("{err}: {shown}"));
    assert!(parsed["rows"].is_array(), "{shown}");
}

#[test]
fn list_json_on_a_tty_stays_valid_json() {
    let board = Board::open(
        "This is a deliberately long title to verify list JSON stays valid on a narrow terminal",
    );
    let shown = pty_output(board.command(&["list", "--json"], 40), 10);
    let parsed: serde_json::Value =
        serde_json::from_str(shown.trim()).unwrap_or_else(|err| panic!("{err}: {shown}"));
    assert!(parsed["rows"].is_array(), "{shown}");
}

#[test]
fn doctor_on_a_tty_uses_spaces_not_tab_rows() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".state")).unwrap();
    std::fs::create_dir_all(root.path().join("run")).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("doctor")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("TMPDIR", root.path().join("tmp"))
        .env("PACKSET_URL", "off")
        .env("PACKSET_AUTH", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("COLUMNS", "40")
        .current_dir(root.path());
    let (_out, shown) = pty_output_any_status(cmd, 10);
    assert!(shown.contains("  sitting gate  "), "{shown}");
    assert!(!shown.lines().any(|line| line.contains('\t')), "{shown}");
}

fn finish_world() -> (tempfile::TempDir, Command) {
    let root = tempfile::tempdir().unwrap();
    let tracker = root.path().join("tracker");
    let project = tracker.join("Software/demo");
    let home = root.path().join("home");
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".state")).unwrap();
    std::fs::create_dir_all(home.join("run")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(tracker.join("vissue.toml"), "prefix = \"Software\"\n").unwrap();
    std::fs::write(
        project.join("issues.org"),
        "\
#+TITLE: demo
#+VISSUE: 1
#+CATEGORY: demo
#+TODO: TODO STARTED | DONE CANCELLED

* STARTED [#C] Long finish output title for wrapping demonstration
:PROPERTIES:
:ID:         demo-uqy6
:CLAIMED_BY: saddle
:END:
",
    )
    .unwrap();
    write_exec(
        &bin.join("claimdag"),
        "#!/bin/sh\ncase \"$1\" in\n  get) echo 'gen=2 assignee=saddle state=claimed' ;;\n  complete|release) echo 'ok' ;;\n  *) echo 'ok' ;;\nesac\nexit 0\n",
    );
    write_exec(
        &bin.join("vissue"),
        "#!/bin/sh\ncase \"$1\" in\n  show) echo '{\"id\":\"demo-uqy6\",\"state\":\"STARTED\",\"claimed_by\":\"saddle\",\"title\":\"Long finish output title for wrapping demonstration\"}' ;;\n  *) echo '{\"id\":\"demo-uqy6\",\"state\":\"DONE\",\"claimed_by\":\"\"}' ;;\nesac\nexit 0\n",
    );
    write_exec(&bin.join("deedar"), "#!/bin/sh\necho '{}'\nexit 0\n");
    write_exec(&bin.join("packset"), "#!/bin/sh\nexit 1\n");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.args(["finish", "demo-uqy6"])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("TMPDIR", root.path().join("tmp"))
        .env("VISSUE_ROOT", &tracker)
        .env("LJOS_SEAT", "saddle")
        .env("LJOS_TRACKER_GIT", "off")
        .env("LJOS_HOOKD", "off")
        .env("PACKSET_URL", "off")
        .current_dir(root.path());
    (root, cmd)
}

#[test]
fn finish_on_a_tty_does_not_indent_continuations() {
    let (_root, mut cmd40) = finish_world();
    cmd40.env("COLUMNS", "40");
    let shown40 = pty_output(cmd40, 10);
    assert!(!shown40.contains("\n "), "{shown40}");
    assert!(!shown40.contains("`ljos remember\n"), "{shown40}");
    assert!(shown40.contains("`ljos remember \"...\"`"), "{shown40}");

    let (_root, mut cmd60) = finish_world();
    cmd60.env("COLUMNS", "60");
    let shown60 = pty_output(cmd60, 10);
    assert!(!shown60.contains("\n "), "{shown60}");
    assert!(shown60.contains("`ljos remember \"...\"`"), "{shown60}");
}

#[test]
fn ready_on_wide_tty_without_columns_uses_tiocgwinsz_width() {
    let board = Board::open(
        "Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau",
    );
    let shown = pty_output_with_cols(board.command_without_columns(&["ready"]), 160, 10);
    assert!(shown.contains("ready  shown 1 of 1"), "{shown}");
    assert!(
        shown.contains("STARTED  [#C]  Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau"),
        "{shown}"
    );
    assert!(!shown.contains("..."), "{shown}");
}

#[test]
fn show_on_a_tty_wraps_to_columns() {
    let board = Board::open(
        "This is a deliberately long title to verify that show respects narrow terminal width",
    );
    let shown = pty_output(board.command(&["show", &board.id], 20), 10);
    assert!(shown.contains("STARTED"), "{shown}");
    for line in shown.lines() {
        assert!(line.chars().count() <= 20, "{shown}");
    }
}

/// A terminal narrower than `MIN_COLS` still wraps, at `MIN_COLS`.
#[test]
fn show_on_a_ten_column_tty_wraps_at_twenty() {
    let board = Board::open(
        "This is a deliberately long title to verify that a very narrow terminal still wraps",
    );
    let shown = pty_output_with_cols(board.command_without_columns(&["show", &board.id]), 10, 10);
    assert!(shown.contains("STARTED"), "{shown}");
    assert!(
        shown.contains("\n  This is a\n  deliberately long\n"),
        "{shown}"
    );
    for line in shown.lines() {
        assert!(prose_display_width(line) <= 20, "{shown}");
    }
    let at_twenty = pty_output(board.command(&["show", &board.id], 20), 10);
    assert_eq!(shown, at_twenty);
}

#[test]
fn ready_and_list_on_a_tty_handle_multibyte_backtick_spans() {
    let board = Board::open("Title with `café 你好` inside backticks and extra words");
    let ready = pty_output(board.command(&["ready"], 80), 10);
    assert!(ready.contains("`café 你好`"), "{ready}");
    let listed = pty_output(board.command(&["list"], 80), 10);
    assert!(listed.contains("`café 你好`"), "{listed}");
}

#[test]
fn doctor_only_leaves_overwide_command_lines_on_narrow_tty() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".state")).unwrap();
    std::fs::create_dir_all(root.path().join("run")).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
    cmd.arg("doctor")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", root.path().join("run"))
        .env("TMPDIR", root.path().join("tmp"))
        .env("PACKSET_URL", "off")
        .env("PACKSET_AUTH", "off")
        .env("LJOS_HOOKD", "off")
        .env("LJOS_TRACKER_GIT", "off")
        .env("COLUMNS", "20")
        .current_dir(root.path());
    let (_out, shown) = pty_output_any_status(cmd, 10);
    for line in shown.lines() {
        if line.chars().count() > 20 {
            if line.trim_start().starts_with('`') {
                assert!(!line.starts_with(' '), "line={line:?}\n{shown}");
            }
            let token_count = line.split_whitespace().count();
            let allowed = token_count <= 1
                || line.contains("`git ")
                || line.contains("`curl ")
                || line.contains("`cargo ")
                || line.trim_start().starts_with('`')
                || line.trim_start().starts_with('/')
                || line.trim_start().starts_with("http");
            assert!(allowed, "line={line:?}\n{shown}");
        }
    }
}

#[test]
fn show_with_escape_before_multibyte_keeps_utf8_on_tty_and_pipe() {
    let board = Board::open("\u{1b}你好 wrap me");

    let piped = non_tty_output(board.command(&["show", &board.id], 80));
    assert!(piped.contains("你好 wrap me"), "{piped}");

    for cols in [1u16, 10, 20] {
        let shown = pty_output_with_cols(
            board.command_without_columns(&["show", &board.id]),
            cols,
            10,
        );
        assert!(shown.contains("你好"), "cols={cols} shown={shown}");
    }
}

#[test]
fn show_with_esc_intermediate_keeps_base_non_tty_bytes() {
    let board = Board::open("Lead \u{1b}(Bhello 你好 wrap me");

    let mut outputs = Vec::new();
    outputs.push(non_tty_output(
        board.command_without_columns(&["show", &board.id]),
    ));
    outputs.push(non_tty_output(board.command(&["show", &board.id], 40)));
    outputs.push(non_tty_output(board.command(&["show", &board.id], 80)));
    outputs.push(non_tty_output(board.command(&["show", &board.id], 200)));

    for shown in outputs {
        assert!(shown.contains("Lead (Bhello 你好 wrap me"), "{shown}");
        assert!(!shown.contains("Lead hello 你好 wrap me"), "{shown}");
    }
}

#[test]
fn non_tty_wide_cjk_title_keeps_base_char_count_formatting() {
    let title = "你好你好你好你好你好你好你好你好你好你好你好你好你好你好你好";
    let board = Board::open(title);
    let show = non_tty_output(board.command(&["show", &board.id], 20));
    assert!(show.contains(title), "{show}");
    assert!(!show.contains("..."), "{show}");

    let ready = non_tty_output(board.command(&["ready"], 20));
    assert!(ready.contains(title), "{ready}");
    assert!(!ready.contains("..."), "{ready}");

    let listed = non_tty_output(board.command(&["list"], 20));
    assert!(listed.contains(title), "{listed}");
    assert!(!listed.contains("..."), "{listed}");
}

/// The title lines of `shown` that start at `first` and join, with single
/// spaces, into `title`.
fn title_lines<'a>(shown: &'a str, first: &str, title: &str) -> Vec<&'a str> {
    let lines: Vec<&str> = shown.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.starts_with(first))
        .unwrap_or_else(|| panic!("no line starts with {first:?}\n{shown}"));
    let mut taken = Vec::new();
    let mut joined = String::new();
    for line in &lines[start..] {
        taken.push(*line);
        let words: Vec<&str> = line.split_whitespace().collect();
        joined = joined
            .split_whitespace()
            .chain(words)
            .collect::<Vec<_>>()
            .join(" ");
        if joined.ends_with(title) {
            return taken;
        }
    }
    panic!("title not found under {first:?}\n{shown}");
}

/// Each line is as full as a single greedy wrap at `cols` leaves it: the
/// first word of the next line would not have fit.
fn assert_wrapped_once(lines: &[&str], cols: usize, shown: &str) {
    for pair in lines.windows(2) {
        let next = pair[1].split_whitespace().next().unwrap_or("");
        assert!(
            prose_display_width(pair[0]) + 1 + prose_display_width(next) > cols,
            "cols={cols}: {:?} had room for {next:?}\n{shown}",
            pair[0]
        );
    }
    assert!(
        lines.iter().all(|line| prose_display_width(line) <= cols),
        "cols={cols}\n{shown}"
    );
}

#[test]
fn a_built_page_is_wrapped_once_at_the_terminal_width() {
    let title = "Kiln damper readings drift after the morning firing so the thermocouple log needs a second pass before the glaze batch is opened and the hearth is swept for the next load of tiles";
    let board = Board::open(title);
    for cols in [40usize, 160] {
        let shown = pty_output(board.command(&["show", &board.id], cols), 10);
        let block = wrap_hanging_prose(title, cols, "  ", "  ");
        assert!(
            shown.contains(&format!("{block}\n")),
            "cols={cols}\n{shown}"
        );
        let lines = title_lines(&shown, "  Kiln", title);
        assert_wrapped_once(&lines, cols, &shown);
        if cols == 160 {
            assert!(
                lines.iter().any(|line| prose_display_width(line) > 80),
                "{shown}"
            );
        }

        let shown = pty_output(board.command(&["timeline", &board.id], cols), 10);
        let lead = format!("timeline of {}: ", board.id);
        let cont = " ".repeat(lead.len());
        let block = wrap_hanging_prose(title, cols, &lead, &cont);
        assert!(
            shown.contains(&format!("{block}\n")),
            "cols={cols}\n{shown}"
        );
        let lines = title_lines(&shown, &lead, title);
        assert_wrapped_once(&lines, cols, &shown);
        if cols == 160 {
            assert!(
                lines.iter().any(|line| prose_display_width(line) > 80),
                "{shown}"
            );
        }
    }
}

/// The prose of the bare screen wraps. A command row stays whole, as a
/// command line does on every page, and so does a URL.
#[test]
fn the_bare_opening_screen_wraps_on_a_tty() {
    let board = Board::open("probe");
    let cols = 40;
    let prose = |line: &&str| !line.starts_with("  ") && line.split_whitespace().count() > 1;
    let piped = non_tty_output(board.command(&[], cols));
    assert!(
        piped
            .lines()
            .filter(prose)
            .any(|line| prose_display_width(line) > cols),
        "the piped screen has no prose line wider than {cols}:\n{piped}"
    );
    let shown = pty_output(board.command(&[], cols), 10);
    assert!(shown.contains("The docs are at"), "{shown}");
    for line in shown.lines().filter(prose) {
        assert!(prose_display_width(line) <= cols, "{line:?}\n{shown}");
    }
}
