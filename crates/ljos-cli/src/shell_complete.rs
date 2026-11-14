//! Shell completion scripts, and the seat and ticket names they ask for.
//!
//! `ljos completions <bash|zsh|fish|powershell|elvish>` prints a script. The
//! script completes commands and flags on its own. Seat names and open ticket
//! ids are read here, and only when that read stays small: a roster file, a
//! short list of project directories, and issue files that are not large. A
//! bigger tracker completes commands and flags and nothing else. Nothing here
//! starts the pack, the tracker, or git.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use clap::{Command, ValueEnum};

/// How many project directories a completion will list.
const MAX_PROJECTS: usize = 40;
/// How many issue files a completion will open.
const MAX_ISSUE_FILES: usize = 24;
/// A roster or one issue file larger than this is left unread.
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ShellName {
    Bash,
    Zsh,
    Fish,
    /// The name is one word. Clap would otherwise print `power-shell`.
    #[value(name = "powershell")]
    PowerShell,
    Elvish,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum Candidates {
    Seats,
    Tickets,
}

/// Print a completion script, or the seat or ticket names the script asks for.
pub fn emit(shell: Option<ShellName>, which: Option<Candidates>, cmd: &mut Command) -> Result<()> {
    if let Some(which) = which {
        for line in candidates(which) {
            println!("{line}");
        }
        return Ok(());
    }
    let Some(shell) = shell else {
        bail!("completions: pass bash, zsh, fish, powershell or elvish");
    };
    let mut buf = Vec::new();
    let generated = match shell {
        ShellName::Bash => clap_complete::Shell::Bash,
        ShellName::Zsh => clap_complete::Shell::Zsh,
        ShellName::Fish => clap_complete::Shell::Fish,
        ShellName::PowerShell => clap_complete::Shell::PowerShell,
        ShellName::Elvish => clap_complete::Shell::Elvish,
    };
    clap_complete::generate(generated, cmd, "ljos", &mut buf);
    let mut text = String::from_utf8(buf)
        .map_err(|_| anyhow::anyhow!("completions: the script was not text"))?;
    text = attach_dynamic(shell, text);
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes())?;
    Ok(())
}

/// Commands and flags come from the generated script. Seat names and open
/// ticket ids are asked for in the same places in every shell, and only by
/// running `ljos completions --candidates`, which does not start a writer.
fn attach_dynamic(shell: ShellName, mut text: String) -> String {
    match shell {
        ShellName::Bash => {
            text = text.replace("complete -F _ljos ", "complete -F _ljos_full ");
            text.push_str(BASH_TAIL);
        }
        ShellName::Zsh => text.push_str(ZSH_TAIL),
        ShellName::Fish => text.push_str(FISH_TAIL),
        ShellName::PowerShell => {
            text = insert_before(&text, POWERSHELL_MARKER, POWERSHELL_TAIL);
        }
        ShellName::Elvish => {
            text = text.replacen(ELVISH_MARKER, ELVISH_TAIL, 1);
        }
    }
    text
}

fn insert_before(text: &str, marker: &str, insertion: &str) -> String {
    match text.find(marker) {
        Some(at) => {
            let mut out = String::with_capacity(text.len() + insertion.len());
            out.push_str(&text[..at]);
            out.push_str(insertion);
            out.push_str(&text[at..]);
            out
        }
        // The static script still completes commands and flags. The test
        // that looks for `completions --candidates` is what notices a
        // generator whose filter line moved.
        None => text.to_string(),
    }
}

fn candidates(which: Candidates) -> BTreeSet<String> {
    match which {
        Candidates::Seats => seat_names(),
        Candidates::Tickets => open_tickets(),
    }
}

/// The tracker this completion will read.
///
/// `VISSUE_ROOT` or `ISSUE_ROOT` wins. Otherwise the working directory, when
/// it holds `vissue.toml`, otherwise the seat's default tracker directory.
/// A named root that is not a directory is not replaced with another one.
fn tracker_root() -> Option<PathBuf> {
    for key in ["VISSUE_ROOT", "ISSUE_ROOT"] {
        if let Some(raw) = std::env::var_os(key) {
            if raw.is_empty() {
                continue;
            }
            let path = PathBuf::from(raw);
            return path.is_dir().then_some(path);
        }
    }
    if let Ok(here) = std::env::current_dir() {
        if here.join("vissue.toml").is_file() {
            return Some(here);
        }
    }
    let root = data_home()?.join("vissue").join("tracker");
    root.is_dir().then_some(root)
}

fn data_home() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            return Some(path);
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
}

fn seat_names() -> BTreeSet<String> {
    let Some(root) = tracker_root() else {
        return BTreeSet::new();
    };
    let Some(text) = small_text(&root.join("vissue.toml")) else {
        return BTreeSet::new();
    };
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return BTreeSet::new();
    };
    let mut names = BTreeSet::new();
    let Some(seats) = value.get("seats") else {
        return names;
    };
    match seats {
        toml::Value::Table(table) => {
            for key in table.keys() {
                insert_seat(&mut names, key);
            }
        }
        toml::Value::Array(list) => {
            for item in list {
                if let Some(name) = item.as_str() {
                    insert_seat(&mut names, name);
                }
            }
        }
        _ => {}
    }
    names
}

fn insert_seat(names: &mut BTreeSet<String>, raw: &str) {
    let name = raw.trim();
    if name.is_empty() || name.starts_with("sand-sub") || name.len() > 64 {
        return;
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    {
        return;
    }
    names.insert(name.to_string());
}

fn open_tickets() -> BTreeSet<String> {
    let Some(root) = tracker_root() else {
        return BTreeSet::new();
    };
    let Some(files) = issue_files(&root) else {
        return BTreeSet::new();
    };
    let mut ids = BTreeSet::new();
    for path in files {
        let Some(text) = small_text(&path) else {
            continue;
        };
        ids.extend(open_ids(&text));
    }
    ids
}

/// Issue files under the tracker, or `None` when listing them is already
/// more than a completion should do.
fn issue_files(root: &Path) -> Option<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut projects = 0usize;
    for prefix in prefixes(root) {
        let Ok(projects_dir) = std::fs::read_dir(root.join(&prefix)) else {
            continue;
        };
        for entry in projects_dir.flatten() {
            let path = entry.path();
            if !path
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_dir())
            {
                continue;
            }
            projects += 1;
            if projects > MAX_PROJECTS {
                return None;
            }
            let board = path.join("issues.org");
            if plain_file(&board) {
                files.push(board);
            }
            let issues = path.join("issues");
            if issues
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_dir())
            {
                let Ok(inner) = std::fs::read_dir(&issues) else {
                    continue;
                };
                for file in inner.flatten() {
                    let path = file.path();
                    if path.extension().is_some_and(|ext| ext == "org") && plain_file(&path) {
                        files.push(path);
                    }
                }
            }
            if files.len() > MAX_ISSUE_FILES {
                return None;
            }
        }
    }
    Some(files)
}

fn prefixes(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(prefix) = std::env::var("VISSUE_PREFIX") {
        push_prefix(&mut out, prefix.trim());
    }
    if let Some(text) = small_text(&root.join("vissue.toml")) {
        if let Ok(value) = toml::from_str::<toml::Value>(&text) {
            if let Some(prefix) = value.get("prefix").and_then(|v| v.as_str()) {
                push_prefix(&mut out, prefix.trim());
            }
        }
    }
    push_prefix(&mut out, "Software");
    out
}

fn push_prefix(out: &mut Vec<String>, prefix: &str) {
    if prefix.is_empty()
        || prefix.contains('/')
        || prefix.contains('\\')
        || prefix.contains("..")
        || out.iter().any(|have| have == prefix)
    {
        return;
    }
    out.push(prefix.to_string());
}

fn plain_file(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_file())
}

fn small_text(path: &Path) -> Option<String> {
    let meta = path.symlink_metadata().ok()?;
    if !meta.file_type().is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Open states vissue writes. A custom `#+TODO` line is not read: completion
/// stays on the three states a tracker uses when nobody has changed them.
fn is_open_state(word: &str) -> bool {
    matches!(word, "TODO" | "STARTED" | "BLOCKED")
}

fn open_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut want = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = heading_rest(trimmed) {
            let word = rest.split_whitespace().next().unwrap_or("");
            want = is_open_state(word);
            continue;
        }
        if want {
            if let Some(id) = trimmed.strip_prefix(":ID:") {
                let id = id.trim();
                if ticket_id(id) {
                    ids.push(id.to_string());
                }
                want = false;
            } else if trimmed == ":END:" {
                want = false;
            }
        }
    }
    ids
}

fn heading_rest(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('*');
    if rest.len() == line.len() || !rest.starts_with(' ') {
        return None;
    }
    Some(rest.trim())
}

fn ticket_id(word: &str) -> bool {
    let Some((project, suffix)) = word.rsplit_once('-') else {
        return false;
    };
    !project.is_empty()
        && !suffix.is_empty()
        && suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
        && project
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'-')
        && project.bytes().any(|b| b.is_ascii_lowercase())
}

// The open-state reader above ignores a custom `#+TODO` line and always
// uses the three states vissue writes. That is the cheap path: one set,
// no allocation kept past the call. A heading that is not one of those
// three is not offered.

const BASH_TAIL: &str = r#"
# Seat names and open ticket ids, when the tracker is small enough to read.
_ljos_from() {
    local bin="$1" kind="$2" cur="$3" line
    line="$("$bin" completions --candidates "$kind" 2>/dev/null)" || return 0
    [[ -n "$line" ]] || return 0
    COMPREPLY+=($(compgen -W "$line" -- "$cur"))
}

_ljos_full() {
    _ljos "$@"
    local cur prev
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    [[ "$cur" == -* ]] && return 0
    prev="$3"
    case "$prev" in
        --seat|--assignee|--source-seat|--via|--add|--remove)
            _ljos_from "$1" seats "$cur"
            return 0
            ;;
    esac
    local sub="" word i
    for ((i = 1; i < COMP_CWORD; i++)); do
        word="${COMP_WORDS[i]}"
        [[ "$word" == -* ]] && continue
        if [[ -z "$sub" ]]; then
            sub="$word"
        else
            case "$sub" in
                seat|memory|memories|mesh|cloud|record|proposals)
                    sub="$sub $word"
                    ;;
            esac
            break
        fi
    done
    case "$sub" in
        recall|show|finish|claim|release|complete|vote|reopen|withdraw|timeline|workflow|playbook|consensus|predict|options|block|deed|sitting|panel)
            _ljos_from "$1" tickets "$cur"
            ;;
        send|wake|group|log|seat\ add|seat\ grant)
            _ljos_from "$1" seats "$cur"
            ;;
    esac
}
"#;

const ZSH_TAIL: &str = r#"
# Seat names and open ticket ids, when the tracker is small enough to read.
_ljos_candidates() {
    local kind="$1" bin
    bin="${words[1]:-ljos}"
    local -a found
    found=("${(@f)$($bin completions --candidates "$kind" 2>/dev/null)}")
    (( ${#found} )) || return 1
    compadd -- $found
}

_ljos_dynamic() {
    local sub="${words[2]-}"
    case "$sub" in
        recall|show|finish|claim|release|complete|vote|reopen|withdraw|timeline|workflow|playbook|consensus|predict|options|block|deed|sitting|panel)
            _ljos_candidates tickets
            ;;
        send|wake|group|log)
            _ljos_candidates seats
            ;;
        seat)
            case "${words[3]-}" in
                add|grant) _ljos_candidates seats ;;
            esac
            ;;
    esac
    case "${words[CURRENT-1]-}" in
        --seat|--assignee|--source-seat|--via|--add|--remove)
            _ljos_candidates seats
            ;;
    esac
}

if (( $+functions[_ljos] )); then
    functions[_ljos_static]="$functions[_ljos]"
    _ljos() {
        _ljos_static "$@"
        _ljos_dynamic
    }
fi
"#;

const FISH_TAIL: &str = r#"
# Seat names and open ticket ids, when the tracker is small enough to read.
function __ljos_candidates
    set -l bin (commandline -o)[1]
    if test -z "$bin"
        set bin ljos
    end
    "$bin" completions --candidates $argv[1] 2>/dev/null
end

complete -c ljos -n "__fish_seen_subcommand_from recall show finish claim release complete vote reopen withdraw timeline workflow playbook consensus predict options block deed sitting panel" -a "(__ljos_candidates tickets)"
complete -c ljos -n "__fish_seen_subcommand_from send wake group log" -a "(__ljos_candidates seats)"
complete -c ljos -n "__fish_seen_subcommand_from seat; and __fish_seen_subcommand_from add grant" -a "(__ljos_candidates seats)"
complete -c ljos -l seat -a "(__ljos_candidates seats)"
complete -c ljos -l assignee -a "(__ljos_candidates seats)"
complete -c ljos -l source-seat -a "(__ljos_candidates seats)"
complete -c ljos -l via -a "(__ljos_candidates seats)"
complete -c ljos -l add -a "(__ljos_candidates seats)"
complete -c ljos -l remove -a "(__ljos_candidates seats)"
"#;

/// The line clap's PowerShell script filters on. The seat and ticket names
/// are added to `$completions` just before that filter.
const POWERSHELL_MARKER: &str =
    "    $completions.Where{ $_.CompletionText -like \"$wordToComplete*\" } |";

const POWERSHELL_TAIL: &str = r#"
    # Seat names and open ticket ids, when the tracker is small enough to read.
    $kind = ''
    $prev = ''
    if ($commandElements.Count -ge 2) {
        $prev = $commandElements[$commandElements.Count - 2].Extent.Text
    }
    if ($prev -in @('--seat','--assignee','--source-seat','--via','--add','--remove')) {
        $kind = 'seats'
    } else {
        $sub = ''
        foreach ($element in $commandElements | Select-Object -Skip 1) {
            $value = $element.Extent.Text
            if ($value.StartsWith('-')) { continue }
            if ($value -eq $wordToComplete) { break }
            if (-not $sub) {
                $sub = $value
            } elseif ($sub -in @('seat','memory','memories','mesh','cloud','record','proposals')) {
                $sub = "$sub $value"
                break
            } else {
                break
            }
        }
        if ($sub -in @('recall','show','finish','claim','release','complete','vote','reopen','withdraw','timeline','workflow','playbook','consensus','predict','options','block','deed','sitting','panel')) {
            $kind = 'tickets'
        } elseif ($sub -in @('send','wake','group','log','seat add','seat grant')) {
            $kind = 'seats'
        }
    }
    if ($kind -and $wordToComplete -notlike '-*') {
        $bin = $commandElements[0].Extent.Text
        if (-not $bin) { $bin = 'ljos' }
        foreach ($name in (& $bin completions --candidates $kind 2>$null)) {
            if ($name -and $name -like "$wordToComplete*") {
                $completions += [CompletionResult]::new($name, $name, [CompletionResultType]::ParameterValue, $name)
            }
        }
    }

"#;

/// The line clap's Elvish script runs for the static candidates.
const ELVISH_MARKER: &str = "    $completions[$command]\n";

const ELVISH_TAIL: &str = r#"    # Seat names and open ticket ids, when the tracker is small enough to read.
    var kind = ''
    var n = (count $words)
    var prev = ''
    if (> $n 1) {
        set prev = $words[-2]
    }
    if (has-value ['--seat' '--assignee' '--source-seat' '--via' '--add' '--remove'] $prev) {
        set kind = seats
    } else {
        var sub = ''
        for word $words[1..-1] {
            if (str:has-prefix $word '-') {
                continue
            }
            if (eq $sub '') {
                set sub = $word
            } else {
                if (has-value [seat memory memories mesh cloud record proposals] $sub) {
                    set sub = $sub' '$word
                }
                break
            }
        }
        if (has-value [recall show finish claim release complete vote reopen withdraw timeline workflow playbook consensus predict options block deed sitting panel] $sub) {
            set kind = tickets
        } elif (has-value [send wake group log 'seat add' 'seat grant'] $sub) {
            set kind = seats
        }
    }
    var cur = ''
    if (> $n 0) {
        set cur = $words[-1]
    }
    if (and (not-eq $kind '') (not (str:has-prefix $cur '-'))) {
        var bin = ljos
        if (and (> $n 0) (not-eq $words[0] '')) {
            set bin = $words[0]
        }
        var lines = []
        try {
            set lines = ($bin completions --candidates $kind 2>/dev/null)
        } catch e {
            set lines = []
        }
        for line $lines {
            if (not-eq $line '') {
                cand $line $line
            }
        }
    }
    try {
        $completions[$command]
    } catch e {
    }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_heading_is_an_id_and_a_closed_one_is_not() {
        let text = "\
#+TODO: TODO STARTED | DONE CANCELLED
* TODO [#C] Pick a codec
:PROPERTIES:
:ID:         demo-2f5j
:END:
* DONE [#B] Old
:PROPERTIES:
:ID:         demo-old1
:END:
* STARTED Live
:PROPERTIES:
:ID:         demo-live
:END:
";
        let ids = open_ids(text);
        assert!(ids.contains(&"demo-2f5j".to_string()), "{ids:?}");
        assert!(ids.contains(&"demo-live".to_string()), "{ids:?}");
        assert!(!ids.iter().any(|id| id == "demo-old1"), "{ids:?}");
    }

    #[test]
    fn a_word_that_is_not_an_id_is_left_out() {
        assert!(!ticket_id(""));
        assert!(!ticket_id("demo"));
        assert!(!ticket_id("-ab"));
        assert!(ticket_id("demo-2f5j"));
        assert!(ticket_id("my-app-ab12"));
    }
}
