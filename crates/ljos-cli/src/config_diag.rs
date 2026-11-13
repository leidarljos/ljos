//! What ljos reads from `ljos.toml` and from the environment, and the
//! mistakes in that config.
//!
//! A mistake is a warning. It names the file, the key or the variable,
//! and the nearest valid name or the type that was expected. A hook and
//! the MCP server print the warning and keep going.

use std::path::{Path, PathBuf};

use crate::Habitat;

/// A value a key may hold.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    Int,
    Str,
    StrList,
    Path,
    PathList,
    /// A number, a duration string, or true or false.
    Scalar,
    /// One string, or a list of strings.
    Command,
}

struct Key {
    section: &'static str,
    name: &'static str,
    kind: Kind,
    default: &'static str,
}

struct Var {
    name: &'static str,
    default: &'static str,
    group: &'static str,
}

const KEYS: &[Key] = &[
    Key {
        section: "",
        name: "quorum",
        kind: Kind::Int,
        default: "2",
    },
    Key {
        section: "gates",
        name: "quorum",
        kind: Kind::Int,
        default: "2",
    },
    Key {
        section: "",
        name: "consensus_branches",
        kind: Kind::StrList,
        default: "develop, main",
    },
    Key {
        section: "gates",
        name: "consensus_branches",
        kind: Kind::StrList,
        default: "develop, main",
    },
    Key {
        section: "",
        name: "protected_branches",
        kind: Kind::StrList,
        default: "main, master, develop",
    },
    Key {
        section: "gates",
        name: "protected_branches",
        kind: Kind::StrList,
        default: "main, master, develop",
    },
    Key {
        section: "",
        name: "rewrite",
        kind: Kind::Bool,
        default: "true",
    },
    Key {
        section: "gates",
        name: "rewrite",
        kind: Kind::Bool,
        default: "true",
    },
    Key {
        section: "",
        name: "policyd_config",
        kind: Kind::Path,
        default: "unset",
    },
    Key {
        section: "gates",
        name: "policyd_config",
        kind: Kind::Path,
        default: "unset",
    },
    Key {
        section: "",
        name: "graphify",
        kind: Kind::Path,
        default: "graphify-out/graph.json",
    },
    Key {
        section: "",
        name: "project",
        kind: Kind::Str,
        default: "the git checkout directory name",
    },
    Key {
        section: "",
        name: "owners",
        kind: Kind::StrList,
        default: "unset",
    },
    Key {
        section: "",
        name: "branches",
        kind: Kind::StrList,
        default: "develop",
    },
    Key {
        section: "cloud",
        name: "branches",
        kind: Kind::StrList,
        default: "develop",
    },
    Key {
        section: "git_hooks",
        name: "skip",
        kind: Kind::PathList,
        default: "/tmp, /var/tmp, /dev/shm",
    },
    Key {
        section: "mail",
        name: "notmuch_config",
        kind: Kind::Path,
        default: "unset",
    },
    Key {
        section: "mail",
        name: "notmuch",
        kind: Kind::Path,
        default: "unset",
    },
    Key {
        section: "mail",
        name: "identity",
        kind: Kind::Str,
        default: "unset",
    },
    Key {
        section: "mail",
        name: "deny_file",
        kind: Kind::Path,
        default: "unset",
    },
    Key {
        section: "mail",
        name: "attribution",
        kind: Kind::StrList,
        default: "unset",
    },
    Key {
        section: "identity",
        name: "name",
        kind: Kind::Str,
        default: "unset",
    },
    Key {
        section: "identity",
        name: "email",
        kind: Kind::Str,
        default: "unset",
    },
    Key {
        section: "panel",
        name: "command",
        kind: Kind::Command,
        default: "unset",
    },
    Key {
        section: "panel",
        name: "headless",
        kind: Kind::Command,
        default: "unset",
    },
    Key {
        section: "panel",
        name: "template",
        kind: Kind::Command,
        default: "unset",
    },
    Key {
        section: "panel",
        name: "runner",
        kind: Kind::Str,
        default: "unset",
    },
    Key {
        section: "panel",
        name: "harness",
        kind: Kind::Str,
        default: "unset",
    },
    Key {
        section: "panel",
        name: "max_children",
        kind: Kind::Int,
        default: "8",
    },
    Key {
        section: "panel",
        name: "max_descendants",
        kind: Kind::Int,
        default: "12",
    },
    Key {
        section: "panel",
        name: "max_wall_secs",
        kind: Kind::Int,
        default: "unset",
    },
    Key {
        section: "panel",
        name: "max_tokens",
        kind: Kind::Int,
        default: "unset",
    },
    Key {
        section: "workflow",
        name: "max_children",
        kind: Kind::Int,
        default: "24",
    },
    Key {
        section: "workflow",
        name: "max_descendants",
        kind: Kind::Int,
        default: "48",
    },
    Key {
        section: "workflow",
        name: "max_wall_secs",
        kind: Kind::Int,
        default: "unset",
    },
    Key {
        section: "workflow",
        name: "max_tokens",
        kind: Kind::Int,
        default: "unset",
    },
    Key {
        section: "mcp",
        name: "idle_drop",
        kind: Kind::Scalar,
        default: "120s",
    },
    Key {
        section: "mcp",
        name: "orphan_idle",
        kind: Kind::Scalar,
        default: "6h",
    },
];

const SECTIONS: &[&str] = &[
    "gates",
    "cloud",
    "git_hooks",
    "mail",
    "identity",
    "panel",
    "workflow",
    "mcp",
];

/// `LJOS_*` variables, then the packset and vissue settings ljos passes on.
const VARS: &[Var] = &[
    Var {
        name: "LJOS_ASK_PACK",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_ATOMS_DRIVER",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_ATOM_NOW_MS",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_BIN",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_BIN_DIR",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_CAPTURE",
        default: "on",
        group: "ljos",
    },
    Var {
        name: "LJOS_CARDS_DIR",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_CITE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_CLOUD_AGE_SECRET",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_CONFIG",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_EXTRACT",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_EXTRACT_DEBUG",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_GIT_HOOK",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_GIT_HOOKS",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_GRAPHIFY",
        default: "unset; then the graphify key, else graphify-out/graph.json",
        group: "ljos",
    },
    Var {
        name: "LJOS_GUARD",
        default: "on outside a temporary home",
        group: "ljos",
    },
    Var {
        name: "LJOS_GUARD_FAIL_OPEN",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_GUARD_TIMEOUT",
        default: "5 seconds",
        group: "ljos",
    },
    Var {
        name: "LJOS_HARNESS_SESSION",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_HOLD",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_HOOKD",
        default: "on",
        group: "ljos",
    },
    Var {
        name: "LJOS_HOOK_ADMIT_MS",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_HOOK_PACK_TIMEOUT_MS",
        default: "1500, at most 2000",
        group: "ljos",
    },
    Var {
        name: "LJOS_HOOK_QUEUE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_HOOK_RELAY",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_HOOK_SLOTS",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_HOOK_TIMING",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_HUD_BIN",
        default: "unset; sibling of ljos, then PATH",
        group: "ljos",
    },
    Var {
        name: "LJOS_HUD_LOG",
        default: "unset; beside the summon socket",
        group: "ljos",
    },
    Var {
        name: "LJOS_HUD_SOCKET",
        default: "$XDG_RUNTIME_DIR/ljos/hud.sock",
        group: "ljos",
    },
    Var {
        name: "LJOS_HUD_TRAY",
        default: "on",
        group: "ljos",
    },
    Var {
        name: "LJOS_INBOX_SYNC_WAIT",
        default: "30 seconds",
        group: "ljos",
    },
    Var {
        name: "LJOS_INSTALL_SH",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_INSTALL_URL",
        default: "the published install script",
        group: "ljos",
    },
    Var {
        name: "LJOS_IN_HOOK",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_JUDGE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_LEASE_SECS",
        default: "900",
        group: "ljos",
    },
    Var {
        name: "LJOS_MAX_PARALLEL",
        default: "the machine's parallelism, clamped to 4",
        group: "ljos",
    },
    Var {
        name: "LJOS_MCP_BIN",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_MCP_CONFIG",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_MCP_FORWARDED",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_MCP_GENERATION",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_MCP_IDLE",
        default: "30 minutes",
        group: "ljos",
    },
    Var {
        name: "LJOS_MCP_IDLE_DROP",
        default: "120s",
        group: "ljos",
    },
    Var {
        name: "LJOS_MCP_ORPHAN_IDLE",
        default: "6h",
        group: "ljos",
    },
    Var {
        name: "LJOS_MCP_TIMEOUT",
        default: "30 seconds",
        group: "ljos",
    },
    Var {
        name: "LJOS_MCP_TIMEOUT_MS",
        default: "unset; LJOS_MCP_TIMEOUT applies",
        group: "ljos",
    },
    Var {
        name: "LJOS_MCP_TOOLS",
        default: "the core set",
        group: "ljos",
    },
    Var {
        name: "LJOS_MEMBER_BIN",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_MEMLOCK_TEST_CHILD",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_MEMORY_WHY",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_MEM_PORT",
        default: "8761",
        group: "ljos",
    },
    Var {
        name: "LJOS_MESH_KEY_FILE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_MESH_LOGIN_SERVER",
        default: "https://hs.grund.rgoswami.me",
        group: "ljos",
    },
    Var {
        name: "LJOS_MESH_TUN",
        default: "/dev/net/tun",
        group: "ljos",
    },
    Var {
        name: "LJOS_MESH_WAIT",
        default: "30 seconds",
        group: "ljos",
    },
    Var {
        name: "LJOS_OFF",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_ORG_DRIVER",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PACKSETD_SIGINT_CHILD",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PACK_CONNECT_MS",
        default: "300, at most 1000",
        group: "ljos",
    },
    Var {
        name: "LJOS_PACK_TIMEOUT_MS",
        default: "4000, at most 5000",
        group: "ljos",
    },
    Var {
        name: "LJOS_PANEL_BIN",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PANEL_CHILD",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PANEL_COMMAND",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PANEL_CONCURRENCY",
        default: "the machine's parallelism, clamped to 4",
        group: "ljos",
    },
    Var {
        name: "LJOS_PANEL_HARNESS",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PANEL_PERSONAS",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PANEL_RUNNER",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PANEL_TEMPLATE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PANE_TOOL",
        default: "the first tool that answers",
        group: "ljos",
    },
    Var {
        name: "LJOS_POLICY_RECALL_WAIT",
        default: "3 seconds",
        group: "ljos",
    },
    Var {
        name: "LJOS_PROCESS_NONCE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PROC_TREE_PROBE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PROGRAMS",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_PROJECT",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_RELEASE_DIR",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_SEAT",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_SEAT_KEY",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_SESSION_ID",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_SHELL_SESSION",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_SITTING_CLAIM_WAIT",
        default: "10 seconds",
        group: "ljos",
    },
    Var {
        name: "LJOS_SYNC_HOST",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TEST_ARGV",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TEST_CANON",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TEST_CANON_TMP",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TEST_HARNESS",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TEST_HISTORY_ROOT",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TEST_HOSTNAME",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TEST_REQUIRE_TOOLS",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TEST_SESSION",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TRACKER_DIR",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TRACKER_GIT",
        default: "push",
        group: "ljos",
    },
    Var {
        name: "LJOS_TRACKER_LOCK_WAIT",
        default: "20 seconds",
        group: "ljos",
    },
    Var {
        name: "LJOS_TRACKER_PUSH_WAIT",
        default: "5 seconds",
        group: "ljos",
    },
    Var {
        name: "LJOS_TRACKER_REMOTE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TRACKER_SEARCH",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TRACKER_TOKEN",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_TRACK_WORK",
        default: "off",
        group: "ljos",
    },
    Var {
        name: "LJOS_UNATTENDED",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_UNSIGNED_BALLOTS",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_USER_RULES_SYNC",
        default: "on",
        group: "ljos",
    },
    Var {
        name: "LJOS_USER_RULES_SYNC_MS",
        default: "4000 after a write, 1500 at session start, 8000 for --sync",
        group: "ljos",
    },
    Var {
        name: "LJOS_USER_RULES_TRACKER",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WAKE_BIN",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WAKE_COMMIT",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WAKE_FROM",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WAKE_INTERRUPT",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WAKE_ISSUE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WAKE_LAUNCHER_KEY",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WAKE_MSG",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WAKE_SEAT",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WRITER_PIDFILE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WRITER_STANDIN",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "LJOS_WRITE_SCOPE",
        default: "unset",
        group: "ljos",
    },
    Var {
        name: "PACKSET_AUTH",
        default: "unset",
        group: "packset",
    },
    Var {
        name: "PACKSET_HOME",
        default: "$HOME/.local/share/packset",
        group: "packset",
    },
    Var {
        name: "PACKSET_PORT",
        default: "8761",
        group: "packset",
    },
    Var {
        name: "PACKSET_TIMEOUT_MS",
        default: "unset; a hook uses 1500 and a command uses 4000",
        group: "packset",
    },
    Var {
        name: "PACKSET_TOKEN",
        default: "unset",
        group: "packset",
    },
    Var {
        name: "PACKSET_TOKEN_FILE",
        default: "unset",
        group: "packset",
    },
    Var {
        name: "PACKSET_URL",
        default: "http://127.0.0.1:8761",
        group: "packset",
    },
    Var {
        name: "PACKSET_WORKSPACE",
        default: "seat",
        group: "packset",
    },
    Var {
        name: "ISSUE_ROOT",
        default: "unset",
        group: "vissue",
    },
    Var {
        name: "VISSUE_AGENT",
        default: "unset",
        group: "vissue",
    },
    Var {
        name: "VISSUE_CONFIG",
        default: "$XDG_CONFIG_HOME/vissue/config.toml",
        group: "vissue",
    },
    Var {
        name: "VISSUE_NO_ROUTE",
        default: "unset",
        group: "vissue",
    },
    Var {
        name: "VISSUE_PREFIX",
        default: "Software, when ljos creates the tracker",
        group: "vissue",
    },
    Var {
        name: "VISSUE_ROOT",
        default: "unset; then root in the vissue config, else $XDG_DATA_HOME/vissue/tracker",
        group: "vissue",
    },
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Seat,
    Project,
    Named,
    Gates,
    Mail,
}

struct Layer {
    role: Role,
    path: PathBuf,
}

struct Found {
    key: String,
    value: String,
    path: PathBuf,
    role: Role,
    order: usize,
}

/// The line and column toml reports for `err`, both starting at 1.
pub(crate) fn toml_line_column(err: &toml::de::Error) -> (usize, usize) {
    let shown = err.to_string();
    let number = |label: &str| -> Option<usize> {
        let rest = shown.split_once(label)?.1;
        let digits: String = rest
            .chars()
            .skip_while(|ch| !ch.is_ascii_digit())
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        digits.parse().ok()
    };
    (number("line ").unwrap_or(1), number("column ").unwrap_or(1))
}

fn expected(kind: Kind) -> &'static str {
    match kind {
        Kind::Bool => "true or false",
        Kind::Int => "a whole number",
        Kind::Str => "a string",
        Kind::StrList => "a list of strings",
        Kind::Path => "a path",
        Kind::PathList => "a list of paths",
        Kind::Scalar => "a whole number, a duration or true or false",
        Kind::Command => "a string or a list of strings",
    }
}

fn got(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "a string",
        toml::Value::Integer(_) => "a whole number",
        toml::Value::Float(_) => "a number",
        toml::Value::Boolean(_) => "true or false",
        toml::Value::Array(_) => "a list",
        toml::Value::Table(_) => "a table",
        toml::Value::Datetime(_) => "a date",
    }
}

fn type_ok(kind: Kind, value: &toml::Value) -> bool {
    match kind {
        Kind::Bool => value.as_bool().is_some(),
        Kind::Int => value.as_integer().is_some(),
        Kind::Str | Kind::Path => value.as_str().is_some(),
        Kind::StrList | Kind::PathList => value
            .as_array()
            .is_some_and(|items| items.iter().all(|item| item.as_str().is_some())),
        Kind::Scalar => matches!(
            value,
            toml::Value::String(_) | toml::Value::Integer(_) | toml::Value::Boolean(_)
        ),
        Kind::Command => {
            value.as_str().is_some()
                || value
                    .as_array()
                    .is_some_and(|items| items.iter().all(|item| item.as_str().is_some()))
        }
    }
}

fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut prev: Vec<usize> = (0..=right.len()).collect();
    let mut next = vec![0; right.len() + 1];
    for (i, ca) in left.iter().enumerate() {
        next[0] = i + 1;
        for (j, cb) in right.iter().enumerate() {
            let cost = usize::from(ca != cb);
            next[j + 1] = (prev[j + 1] + 1).min(next[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut next);
    }
    prev[right.len()]
}

fn nearest<'a>(name: &str, pool: impl IntoIterator<Item = &'a str>) -> &'a str {
    let mut best = "";
    let mut best_distance = usize::MAX;
    for candidate in pool {
        if candidate == name {
            continue;
        }
        let distance = edit_distance(&name.to_ascii_lowercase(), &candidate.to_ascii_lowercase());
        if distance < best_distance
            || (distance == best_distance && (best.is_empty() || candidate < best))
        {
            best = candidate;
            best_distance = distance;
        }
    }
    best
}

fn key_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = KEYS.iter().map(|key| key.name).collect();
    names.sort_unstable();
    names.dedup();
    names
}

fn known_var(name: &str) -> bool {
    VARS.iter().any(|var| var.name == name) || name.starts_with("LJOS_WAKE_")
}

fn key_for(section: &str, name: &str) -> Option<&'static Key> {
    KEYS.iter()
        .find(|key| key.section == section && key.name == name)
}

fn homes_of(name: &str) -> String {
    let mut homes: Vec<&str> = KEYS
        .iter()
        .filter(|key| key.name == name)
        .map(|key| {
            if key.section.is_empty() {
                "the top of the file"
            } else {
                key.section
            }
        })
        .collect();
    homes.sort_unstable();
    homes.dedup();
    match homes.as_slice() {
        [] => "no section".to_string(),
        [one] if *one == "the top of the file" => (*one).to_string(),
        [one] => format!("[{one}]"),
        many => {
            let last = many[many.len() - 1];
            let last = if last == "the top of the file" {
                last.to_string()
            } else {
                format!("[{last}]")
            };
            let rest: Vec<String> = many[..many.len() - 1]
                .iter()
                .map(|place| {
                    if *place == "the top of the file" {
                        (*place).to_string()
                    } else {
                        format!("[{place}]")
                    }
                })
                .collect();
            format!("{} or {last}", rest.join(", "))
        }
    }
}

fn show_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(text) => text.clone(),
        toml::Value::Integer(n) => n.to_string(),
        toml::Value::Boolean(on) => on.to_string(),
        toml::Value::Float(n) => n.to_string(),
        toml::Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .or_else(|| Some(show_value(item)))
            })
            .collect::<Vec<_>>()
            .join(", "),
        toml::Value::Datetime(dt) => dt.to_string(),
        toml::Value::Table(_) => "a table".to_string(),
    }
}

fn redact(name: &str, value: &str) -> String {
    if name.contains("TOKEN") || name.contains("SECRET") || name.contains("PASSPHRASE") {
        "set".to_string()
    } else {
        value.to_string()
    }
}

fn off_word(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "off" | "0" | "false" | "no"
    )
}

fn seat_name() -> Option<String> {
    let name = std::env::var("LJOS_SEAT").ok()?;
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        None
    } else {
        Some(name.to_string())
    }
}

fn git_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn layers(cwd: Option<&Path>) -> Vec<Layer> {
    let mut out = Vec::new();
    if let Some(base) = crate::config_base() {
        out.push(Layer {
            role: Role::User,
            path: base.join("ljos").join("ljos.toml"),
        });
        if let Some(seat) = seat_name() {
            out.push(Layer {
                role: Role::Seat,
                path: base.join("ljos").join("seats").join(seat).join("ljos.toml"),
            });
        }
        out.push(Layer {
            role: Role::Mail,
            path: base.join("ljos").join("mail.toml"),
        });
    }
    if let Some(cwd) = cwd {
        out.push(Layer {
            role: Role::Project,
            path: cwd.join("ljos.toml"),
        });
        if let Some(root) = git_root(cwd) {
            let at_root = root.join("ljos.toml");
            if at_root != cwd.join("ljos.toml") {
                out.push(Layer {
                    role: Role::Project,
                    path: at_root,
                });
            }
            out.push(Layer {
                role: Role::Gates,
                path: root.join(".ljos").join("gates.toml"),
            });
        }
    }
    if let Some(path) = std::env::var_os("LJOS_CONFIG").filter(|path| !path.is_empty()) {
        out.push(Layer {
            role: Role::Named,
            path: PathBuf::from(path),
        });
    }
    out
}

struct Report {
    warnings: Vec<String>,
    rows: Vec<String>,
}

fn parse_file(layer: &Layer, warnings: &mut Vec<String>, found: &mut Vec<Found>) {
    if layer.role == Role::Named && !layer.path.exists() {
        warnings.push(format!(
            "LJOS_CONFIG names {}, which does not exist",
            layer.path.display()
        ));
        return;
    }
    let text = match std::fs::read_to_string(&layer.path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            warnings.push(format!("{}: cannot be read ({err})", layer.path.display()));
            return;
        }
    };
    if text.trim().is_empty() {
        return;
    }
    let value: toml::Value = match toml::from_str(&text) {
        Ok(value) => value,
        Err(err) => {
            let (line, column) = toml_line_column(&err);
            let message = err.message().lines().next().unwrap_or("invalid TOML");
            warnings.push(format!(
                "{}:{line}:{column}: invalid TOML: {message}, so run `ljos doctor`.",
                layer.path.display()
            ));
            return;
        }
    };
    let Some(root) = value.as_table() else {
        warnings.push(format!(
            "{}:1:1: invalid TOML: the file is not a table",
            layer.path.display()
        ));
        return;
    };
    walk(layer, "", root, warnings, found);
}

fn walk(
    layer: &Layer,
    section: &str,
    table: &toml::map::Map<String, toml::Value>,
    warnings: &mut Vec<String>,
    found: &mut Vec<Found>,
) {
    if section == "mail" && !matches!(layer.role, Role::User | Role::Mail) {
        warnings.push(format!(
            "[mail] in {} is read only from the user's ljos.toml or mail.toml",
            layer.path.display()
        ));
    }
    if section == "cloud" && layer.role != Role::Seat {
        warnings.push(format!(
            "[cloud] in {} is read only from the seat ljos.toml",
            layer.path.display()
        ));
    }
    for (name, value) in table {
        if section.is_empty() && SECTIONS.contains(&name.as_str()) {
            if let Some(inner) = value.as_table() {
                walk(layer, name, inner, warnings, found);
            } else {
                warnings.push(format!(
                    "key `{name}` in {} is {}, expected a table",
                    layer.path.display(),
                    got(value)
                ));
            }
            continue;
        }
        let Some(spec) = key_for(section, name) else {
            if section.is_empty() && value.as_table().is_some() {
                warnings.push(format!(
                    "unknown key `{name}` in {}; nearest is `{}`",
                    layer.path.display(),
                    nearest(name, key_names())
                ));
                continue;
            }
            if KEYS.iter().any(|key| key.name == name) {
                let place = if section.is_empty() {
                    "the top of the file".to_string()
                } else {
                    format!("[{section}]")
                };
                warnings.push(format!(
                    "key `{name}` under {place} in {} is not read there; it belongs {}",
                    layer.path.display(),
                    homes_of(name)
                ));
            } else {
                let where_ = if section.is_empty() {
                    format!("in {}", layer.path.display())
                } else {
                    format!("under [{section}] in {}", layer.path.display())
                };
                warnings.push(format!(
                    "unknown key `{name}` {where_}; nearest is `{}`",
                    nearest(name, key_names())
                ));
            }
            continue;
        };
        if !type_ok(spec.kind, value) {
            warnings.push(format!(
                "key `{name}` in {} is {}, expected {}",
                layer.path.display(),
                got(value),
                expected(spec.kind)
            ));
            continue;
        }
        if spec.kind == Kind::Int {
            if let Some(n) = value.as_integer() {
                if name == "quorum" && n < 1 {
                    warnings.push(format!(
                        "key `quorum` in {} is {n}, expected a whole number of seats, 1 or more",
                        layer.path.display()
                    ));
                    continue;
                }
            }
        }
        if spec.kind == Kind::Path {
            if let Some(raw) = value.as_str() {
                if !raw.is_empty() && !off_word(raw) {
                    let target = resolve_path(raw, &layer.path);
                    if !target.exists() {
                        warnings.push(format!(
                            "key `{name}` in {} names {}, which does not exist",
                            layer.path.display(),
                            target.display()
                        ));
                    }
                }
            }
        }
        let key = if section.is_empty() || section == "gates" {
            (*name).clone()
        } else {
            format!("{section}.{name}")
        };
        let order = found.len();
        found.push(Found {
            key,
            value: show_value(value),
            path: layer.path.clone(),
            role: layer.role,
            order,
        });
    }
}

fn resolve_path(raw: &str, file: &Path) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        file.parent().unwrap_or(Path::new(".")).join(path)
    }
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn report(cwd: Option<&Path>) -> Report {
    let mut warnings = Vec::new();
    let mut found = Vec::new();
    let files = layers(cwd);
    for layer in &files {
        parse_file(layer, &mut warnings, &mut found);
    }
    for (key, value) in std::env::vars() {
        if !key.starts_with("LJOS_") || known_var(&key) {
            continue;
        }
        let near = nearest(
            &key,
            VARS.iter()
                .filter(|var| var.name.starts_with("LJOS_"))
                .map(|var| var.name),
        );
        warnings.push(format!(
            "unknown LJOS_ variable `{key}`; nearest is `{near}`"
        ));
        let _ = value;
    }
    let mut rows = Vec::new();
    graphify_rows(&found, &mut warnings, &mut rows);
    quorum_row(&found, &mut warnings, &mut rows);
    shadow_rows(&found, &mut warnings);
    for item in winners(&found) {
        if item.key == "graphify" || item.key == "quorum" {
            continue;
        }
        rows.push(format!(
            "{} is {} from {}",
            item.key,
            item.value,
            item.path.display()
        ));
    }
    for var in VARS {
        if let Some(value) = env_nonempty(var.name) {
            if var.name == "LJOS_GRAPHIFY" {
                continue;
            }
            if var.name == "LJOS_CONFIG" && !Path::new(&value).exists() {
                warnings.push(format!("LJOS_CONFIG names {value}, which does not exist"));
            }
            rows.push(format!(
                "{} is {} from {}",
                var.name,
                redact(var.name, &value),
                var.name
            ));
        }
    }
    warnings.sort();
    warnings.dedup();
    rows.sort();
    rows.dedup();
    Report { warnings, rows }
}

fn winners(found: &[Found]) -> Vec<&Found> {
    let mut best: Vec<&Found> = Vec::new();
    for item in found {
        if let Some(slot) = best.iter_mut().find(|have| have.key == item.key) {
            if item.order >= slot.order {
                *slot = item;
            }
        } else {
            best.push(item);
        }
    }
    best
}

fn quorum_row(found: &[Found], warnings: &mut Vec<String>, rows: &mut Vec<String>) {
    let mut items: Vec<&Found> = found.iter().filter(|item| item.key == "quorum").collect();
    items.sort_by_key(|item| item.order);
    let mut value = 2_i64;
    let mut source = String::new();
    for item in items {
        let Ok(n) = item.value.parse::<i64>() else {
            continue;
        };
        let person = matches!(item.role, Role::User | Role::Seat);
        if !person && n <= value {
            continue;
        }
        if !source.is_empty() && source != item.path.display().to_string() && n != value {
            warnings.push(format!(
                "{} sets quorum and shadows {}",
                item.path.display(),
                source
            ));
        }
        value = n;
        source = item.path.display().to_string();
    }
    if !source.is_empty() {
        rows.push(format!("quorum is {value} from {source}"));
    }
}

fn graphify_rows(found: &[Found], warnings: &mut Vec<String>, rows: &mut Vec<String>) {
    let env = env_nonempty("LJOS_GRAPHIFY");
    let mut hits: Vec<&Found> = found
        .iter()
        .filter(|item| item.key == "graphify" || item.key.ends_with(".graphify"))
        .collect();
    // Search order for the key: LJOS_CONFIG, the project file, the seat file, the user file.
    let rank = |role: Role| match role {
        Role::Named => 0,
        Role::Project => 1,
        Role::Gates => 2,
        Role::Seat => 3,
        Role::User => 4,
        Role::Mail => 5,
    };
    hits.sort_by_key(|item| rank(item.role));
    if let Some(value) = env.as_deref() {
        if !off_word(value) && !Path::new(value).exists() && value != "off" {
            warnings.push(format!("LJOS_GRAPHIFY names {value}, which does not exist"));
        }
        rows.push(format!("graphify is {value} from LJOS_GRAPHIFY"));
        if let Some(file) = hits.first() {
            if file.value != value {
                warnings.push(format!(
                    "graphify is {value} from LJOS_GRAPHIFY and {} from {}; LJOS_GRAPHIFY is used",
                    file.value,
                    file.path.display()
                ));
            }
        }
        return;
    }
    let Some(winner) = hits.first() else {
        return;
    };
    rows.push(format!(
        "graphify is {} from {}",
        winner.value,
        winner.path.display()
    ));
    for other in hits.iter().skip(1) {
        warnings.push(format!(
            "{} sets graphify and shadows {}",
            winner.path.display(),
            other.path.display()
        ));
    }
}

fn shadow_rows(found: &[Found], warnings: &mut Vec<String>) {
    let mut by_key: Vec<&Found> = found
        .iter()
        .filter(|item| item.key != "graphify" && item.key != "quorum")
        .collect();
    by_key.sort_by(|a, b| a.key.cmp(&b.key).then(a.order.cmp(&b.order)));
    let mut index = 0;
    while index < by_key.len() {
        let mut end = index + 1;
        while end < by_key.len() && by_key[end].key == by_key[index].key {
            end += 1;
        }
        if end - index > 1 {
            let winner = by_key[end - 1];
            for loser in &by_key[index..end - 1] {
                if loser.path != winner.path {
                    warnings.push(format!(
                        "{} sets {} and shadows {}",
                        winner.path.display(),
                        winner.key,
                        loser.path.display()
                    ));
                }
            }
        }
        index = end;
    }
}

/// Warnings for the config this process reads. Each line has no `warning:` prefix.
#[must_use]
pub fn warnings(cwd: Option<&Path>) -> Vec<String> {
    report(cwd).warnings
}

/// Print each warning on stderr. A caller that exits on its own work is unchanged.
pub fn emit() {
    let cwd = std::env::current_dir().ok();
    for line in warnings(cwd.as_deref()) {
        eprintln!("warning: {line}");
    }
}

/// Doctor rows: one warning row per mistake, and one row per effective value
/// that a file or an environment variable set.
#[must_use]
pub fn doctor_rows(cwd: Option<&Path>) -> Vec<Habitat> {
    let report = report(cwd);
    let mut out = Vec::new();
    for line in &report.warnings {
        out.push(Habitat {
            name: "config",
            state: format!("warning: {line}"),
            ok: true,
        });
    }
    for line in &report.rows {
        out.push(Habitat {
            name: "config",
            state: line.clone(),
            ok: true,
        });
    }
    if out.is_empty() {
        out.push(Habitat {
            name: "config",
            state: "no ljos.toml key and no LJOS_ variable is set; defaults stand".into(),
            ok: true,
        });
    }
    out
}

fn section_places(sections: &[&str]) -> String {
    let mut homes: Vec<&str> = sections
        .iter()
        .map(|section| {
            if section.is_empty() {
                "the top of the file"
            } else {
                section
            }
        })
        .collect();
    homes.sort_unstable();
    homes.dedup();
    match homes.as_slice() {
        [] => "no section".to_string(),
        [one] if *one == "the top of the file" => (*one).to_string(),
        [one] => format!("[{one}]"),
        many => {
            let last = many[many.len() - 1];
            let last = if last == "the top of the file" {
                last.to_string()
            } else {
                format!("[{last}]")
            };
            let rest: Vec<String> = many[..many.len() - 1]
                .iter()
                .map(|place| {
                    if *place == "the top of the file" {
                        (*place).to_string()
                    } else {
                        format!("[{place}]")
                    }
                })
                .collect();
            format!("{} or {last}", rest.join(", "))
        }
    }
}

/// The org section that lists every key and variable with its default.
#[must_use]
pub fn reference_org() -> String {
    let mut out = String::new();
    out.push_str("** Config reference\n\n");
    out.push_str("ljos reads =ljos.toml= from the user config (~/.config/ljos/ljos.toml), then the seat file (~/.config/ljos/seats/$LJOS_SEAT/ljos.toml), then the project file (ljos.toml), then =LJOS_CONFIG=, then .ljos/gates.toml. A later file overrides the keys it sets. =graphify= is read the other way: =LJOS_GRAPHIFY=, then =LJOS_CONFIG=, then the project file, then the seat file, then the user file, and the first value wins. =[mail]= is only the user's ljos.toml or mail.toml. =[cloud] branches= is only the seat file. A mistake is a warning. A hook and the MCP server print it and keep going. =ljos doctor= names the file or the environment variable that set each value.\n\n");
    out.push_str("*** ljos.toml keys\n\n");
    let mut groups: std::collections::BTreeMap<(&str, &str), Vec<&str>> =
        std::collections::BTreeMap::new();
    for key in KEYS {
        groups
            .entry((key.name, key.default))
            .or_default()
            .push(key.section);
    }
    let shared: std::collections::BTreeSet<&str> = {
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for (name, _) in groups.keys() {
            *counts.entry(*name).or_default() += 1;
        }
        counts
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(name, _)| name)
            .collect()
    };
    for ((name, default), sections) in &groups {
        let label = if shared.contains(name) {
            sections
                .iter()
                .map(|section| format!("{section}.{name}"))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            (*name).to_string()
        };
        out.push_str(&format!(
            "- ={label}= ({}) default {default}\n",
            section_places(sections)
        ));
    }
    for group in ["ljos", "packset", "vissue"] {
        let title = match group {
            "ljos" => "LJOS_ variables",
            "packset" => "Packset settings ljos passes through",
            "vissue" => "Vissue settings ljos passes through",
            _ => group,
        };
        out.push_str(&format!("\n*** {title}\n\n"));
        for var in VARS.iter().filter(|var| var.group == group) {
            out.push_str(&format!("- ={}= default {}\n", var.name, var.default));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_misspelled_variable_names_the_nearest_one() {
        assert_eq!(
            nearest("LJOS_SEET", VARS.iter().map(|var| var.name)),
            "LJOS_SEAT"
        );
        assert_eq!(nearest("quorun", key_names()), "quorum");
    }

    #[test]
    fn invalid_toml_names_a_line_and_a_column() {
        let text = "quorum = [\n";
        let err = toml::from_str::<toml::Value>(text).unwrap_err();
        let (line, column) = toml_line_column(&err);
        assert_eq!((line, column), (1, 12));
    }

    #[test]
    fn the_reference_lists_the_keys_and_the_variables() {
        let text = reference_org();
        assert!(text.contains("=quorum="), "{text}");
        assert!(text.contains("default 2"), "{text}");
        assert!(text.contains("=LJOS_SEAT="), "{text}");
        assert!(text.contains("=LJOS_GRAPHIFY="), "{text}");
        assert!(text.contains("=PACKSET_URL="), "{text}");
        assert!(text.contains("=VISSUE_ROOT="), "{text}");
        let org = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/orgmode/reference-environment.org"
        ))
        .unwrap();
        let begin = "# BEGIN ljos-config-ref\n";
        let end = "\n# END ljos-config-ref\n";
        let start = org.find(begin).expect("config ref begin");
        let stop = org.find(end).expect("config ref end");
        let body = &org[start + begin.len()..stop];
        assert_eq!(body, text.trim_end());
    }

    #[test]
    fn every_ljos_name_in_the_sources_is_known() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = Vec::new();
        for entry in walk(&root) {
            let text = std::fs::read_to_string(&entry).unwrap();
            let text = text.split("\n#[cfg(test)]").next().unwrap_or(&text);
            for name in names_in(text) {
                if !known_var(name) {
                    missing.push(format!("{}: {name}", entry.display()));
                }
            }
        }
        assert!(missing.is_empty(), "{}", missing.join("\n"));
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let entries = std::fs::read_dir(dir).unwrap();
        for entry in entries {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                out.push(path);
            }
        }
        out
    }

    fn names_in(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let bytes = text.as_bytes();
        let mut index = 0;
        while index + 5 < bytes.len() {
            if bytes[index..].starts_with(b"LJOS_") {
                let start = index;
                index += 5;
                while index < bytes.len()
                    && (bytes[index].is_ascii_uppercase()
                        || bytes[index].is_ascii_digit()
                        || bytes[index] == b'_')
                {
                    index += 1;
                }
                let name = &text[start..index];
                if name.ends_with('_') || name.contains('<') {
                    continue;
                }
                out.push(name);
            } else {
                index += 1;
            }
        }
        out
    }
}
