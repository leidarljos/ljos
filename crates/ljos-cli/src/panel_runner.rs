//! Runner detection and command templates for panel members.
//!
//! A panel member runs on an installed runner CLI, or on any runner
//! with a configured command template in `harnesses.toml` or `ljos.toml`.
//! When no runner is installed, opening a panel fails with a clear message.

use std::path::Path;

use anyhow::{bail, Result};
use serde::Deserialize;

use crate::tools;

/// A known runner CLI and its default headless templates.
struct ShippedRunner {
    harness: &'static str,
    bin: &'static str,
    with_file: &'static [&'static str],
    with_prompt: &'static [&'static str],
}

const SHIPPED_RUNNERS: &[ShippedRunner] = &[
    ShippedRunner {
        harness: "cursor",
        bin: "cursor-agent",
        with_file: &["cursor-agent", "-p", "{prompt}"],
        with_prompt: &["cursor-agent", "-p", "{prompt}"],
    },
    ShippedRunner {
        harness: "claude",
        bin: "claude",
        with_file: &[
            "claude",
            "-p",
            "{prompt}",
            "--max-turns",
            "6",
            "--allowedTools",
            "Bash(ljos:*)",
            "Bash(vissue:*)",
        ],
        with_prompt: &[
            "claude",
            "-p",
            "{prompt}",
            "--max-turns",
            "6",
            "--allowedTools",
            "Bash(ljos:*)",
            "Bash(vissue:*)",
        ],
    },
    ShippedRunner {
        harness: "grok",
        bin: "grok",
        with_file: &[
            "grok",
            "--prompt-file",
            "{prompt_file}",
            "--yolo",
            "--max-turns",
            "6",
            "--effort",
            "low",
            "--disallowed-tools",
            "Agent",
            "--cwd",
            "{cwd}",
        ],
        with_prompt: &[
            "grok",
            "-p",
            "{prompt}",
            "--yolo",
            "--max-turns",
            "6",
            "--effort",
            "low",
            "--disallowed-tools",
            "Agent",
        ],
    },
    ShippedRunner {
        harness: "codex",
        bin: "codex",
        with_file: &["codex", "exec", "--cd", "{cwd}", "{prompt}"],
        with_prompt: &["codex", "exec", "{prompt}"],
    },
    ShippedRunner {
        harness: "gemini",
        bin: "gemini",
        with_file: &["gemini", "-p", "{prompt}"],
        with_prompt: &["gemini", "-p", "{prompt}"],
    },
    ShippedRunner {
        harness: "opencode",
        bin: "opencode",
        with_file: &["opencode", "run", "{prompt}"],
        with_prompt: &["opencode", "run", "{prompt}"],
    },
    ShippedRunner {
        harness: "antigravity",
        bin: "agy",
        with_file: &["agy", "-p", "{prompt}"],
        with_prompt: &["agy", "-p", "{prompt}"],
    },
    ShippedRunner {
        harness: "copilot",
        bin: "copilot",
        with_file: &["copilot", "-p", "{prompt}"],
        with_prompt: &["copilot", "-p", "{prompt}"],
    },
    ShippedRunner {
        harness: "qwen",
        bin: "qwen",
        with_file: &["qwen", "-p", "{prompt}"],
        with_prompt: &["qwen", "-p", "{prompt}"],
    },
    ShippedRunner {
        harness: "factory",
        bin: "droid",
        with_file: &["droid", "exec", "{prompt}"],
        with_prompt: &["droid", "exec", "{prompt}"],
    },
    ShippedRunner {
        harness: "crush",
        bin: "crush",
        with_file: &["crush", "run", "{prompt}"],
        with_prompt: &["crush", "run", "{prompt}"],
    },
    ShippedRunner {
        harness: "kiro",
        bin: "kiro-cli",
        with_file: &["kiro-cli", "chat", "--no-interactive", "{prompt}"],
        with_prompt: &["kiro-cli", "chat", "--no-interactive", "{prompt}"],
    },
    ShippedRunner {
        harness: "hermes",
        bin: "hermes",
        with_file: &["hermes", "-p", "{prompt}"],
        with_prompt: &["hermes", "-p", "{prompt}"],
    },
    ShippedRunner {
        harness: "omp",
        bin: "omp",
        with_file: &["omp", "-p", "{prompt}"],
        with_prompt: &["omp", "-p", "{prompt}"],
    },
];

#[derive(Deserialize)]
#[serde(untagged)]
enum TemplateSpec {
    Words(Vec<String>),
    Line(String),
}

#[derive(Deserialize, Default)]
struct PanelRunnerSection {
    command: Option<TemplateSpec>,
    headless: Option<TemplateSpec>,
    template: Option<TemplateSpec>,
    runner: Option<String>,
    harness: Option<String>,
}

#[derive(Deserialize, Default)]
struct PanelRunnerConfigFile {
    #[serde(default)]
    panel: PanelRunnerSection,
}

/// Split a shell command line into words, respecting single and double quotes.
#[must_use]
pub fn split_template_line(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.starts_with('[') {
        if let Ok(words) = serde_json::from_str::<Vec<String>>(trimmed) {
            return words.into_iter().filter(|w| !w.is_empty()).collect();
        }
    }
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = trimmed.chars().peekable();
    while let Some(ch) = chars.next() {
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            Some('"') => match ch {
                '"' => quote = None,
                '\\' => {
                    if let Some(&next) = chars.peek() {
                        if matches!(next, '"' | '\\' | '$' | '`') {
                            current.push(chars.next().unwrap());
                            continue;
                        }
                    }
                    current.push(ch);
                }
                _ => current.push(ch),
            },
            Some(_) => {}
            None => match ch {
                '\'' | '"' => {
                    in_word = true;
                    quote = Some(ch);
                }
                '\\' => {
                    in_word = true;
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                c if c.is_whitespace() => {
                    if in_word {
                        words.push(std::mem::take(&mut current));
                        in_word = false;
                    }
                }
                _ => {
                    in_word = true;
                    current.push(ch);
                }
            },
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

fn spec_to_vec(spec: TemplateSpec) -> Vec<String> {
    match spec {
        TemplateSpec::Words(words) => words.into_iter().filter(|w| !w.is_empty()).collect(),
        TemplateSpec::Line(line) => split_template_line(&line),
    }
}

/// Whether `value` is a command template rather than a bare runner name.
fn looks_like_template(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.contains(char::is_whitespace)
        || trimmed.contains("{prompt}")
        || trimmed.contains("{prompt_file}")
        || trimmed.contains("{persona}")
        || trimmed.contains("{cwd}")
}

/// Read `[panel]` runner and command template settings across config layers.
fn configured_from_files() -> (Option<Vec<String>>, Option<String>) {
    let mut template = None;
    let mut runner = None;
    for path in crate::converge::config_paths(None) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = toml::from_str::<PanelRunnerConfigFile>(&text) else {
            continue;
        };
        let p = parsed.panel;
        if let Some(spec) = p.command.or(p.headless).or(p.template) {
            let words = spec_to_vec(spec);
            if !words.is_empty() {
                template = Some(words);
            }
        }
        if let Some(name) = p.runner.or(p.harness) {
            let trimmed = name.trim().to_string();
            if !trimmed.is_empty() {
                runner = Some(trimmed);
            }
        }
    }
    (template, runner)
}

/// Configured command template from `LJOS_PANEL_COMMAND`, `LJOS_PANEL_TEMPLATE`,
/// or `[panel] command` / `headless` / `template` in `ljos.toml`.
#[must_use]
pub fn configured_command_template() -> Option<Vec<String>> {
    for var in ["LJOS_PANEL_COMMAND", "LJOS_PANEL_TEMPLATE"] {
        if let Ok(raw) = std::env::var(var) {
            let words = split_template_line(&raw);
            if !words.is_empty() {
                return Some(words);
            }
        }
    }
    configured_from_files().0
}

/// Whether `bin` is an executable file path or a program found by `on_path`.
pub fn binary_available(bin: &str, on_path: &dyn Fn(&str) -> bool) -> bool {
    let trimmed = bin.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains('/') {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::metadata(trimmed)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        }
        #[cfg(not(unix))]
        {
            Path::new(trimmed).is_file()
        }
    } else {
        on_path(trimmed)
    }
}

fn shipped_for(name: &str) -> Option<&'static ShippedRunner> {
    let lower = name.trim().to_ascii_lowercase();
    SHIPPED_RUNNERS
        .iter()
        .find(|r| r.harness == lower || r.bin == lower)
}

/// Look up a `headless` template for `runner` in `harnesses.toml` or shipped shapes.
fn harness_file_template(runner: &str) -> Option<Vec<String>> {
    let name = runner.trim();
    if name.is_empty() {
        return None;
    }
    let from_user = crate::harnesses_with_shipped(&crate::harnesses_path())
        .ok()
        .and_then(|all| {
            all.harness.into_iter().find(|h| {
                h.name.eq_ignore_ascii_case(name)
                    || shipped_for(name).is_some_and(|s| h.name.eq_ignore_ascii_case(s.harness))
            })
        })
        .map(|h| h.headless)
        .filter(|t| !t.is_empty());
    if from_user.is_some() {
        return from_user;
    }
    toml::from_str::<crate::Harnesses>(crate::HARNESSES_EXAMPLE)
        .ok()
        .and_then(|all| {
            all.harness.into_iter().find(|h| {
                h.name.eq_ignore_ascii_case(name)
                    || shipped_for(name).is_some_and(|s| h.name.eq_ignore_ascii_case(s.harness))
            })
        })
        .map(|h| h.headless)
        .filter(|t| !t.is_empty())
}

/// Template for `runner`: user `harnesses.toml` first, then built-in runner table.
#[must_use]
pub fn template_for_runner(
    runner: &str,
    has_prompt_file: bool,
    has_cwd: bool,
) -> Option<Vec<String>> {
    if looks_like_template(runner) {
        let words = split_template_line(runner);
        if !words.is_empty() {
            return Some(words);
        }
    }
    if let Some(template) = harness_file_template(runner) {
        if has_prompt_file || !template.iter().any(|w| w.contains("{prompt_file}")) {
            return Some(template);
        }
    }
    let shipped = shipped_for(runner)?;
    let slice = if has_prompt_file && (has_cwd || shipped.bin == "grok") {
        shipped.with_file
    } else {
        shipped.with_prompt
    };
    Some(slice.iter().map(|s| (*s).to_string()).collect())
}

/// The runner CLIs installed on this machine, in selection order: custom
/// `harnesses.toml` entries with an installed `headless` binary first, then
/// shipped runner CLIs on `PATH`.
#[must_use]
pub fn installed_runners_with(on_path: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(user) = crate::harnesses_with_shipped(&crate::harnesses_path()) {
        for h in user.harness {
            if let Some(bin) = h.headless.first() {
                if binary_available(bin, on_path) && !found.contains(&h.name) {
                    found.push(h.name);
                }
            }
        }
    }
    for r in SHIPPED_RUNNERS {
        if binary_available(r.bin, on_path) && !found.iter().any(|f| f == r.bin || f == r.harness) {
            found.push(r.bin.to_string());
        }
    }
    found
}

/// The runner CLIs installed on `PATH`.
#[must_use]
pub fn installed_runners() -> Vec<String> {
    installed_runners_with(&crate::on_path)
}

/// Pick an installed runner: prefer `preferred` (e.g. the seat or active runner)
/// when its CLI is installed, else the first installed runner CLI.
pub fn pick_installed_runner(preferred: &[&str], on_path: &dyn Fn(&str) -> bool) -> Option<String> {
    for &candidate in preferred {
        let c = candidate.trim();
        if c.is_empty() {
            continue;
        }
        if let Some(template) = template_for_runner(c, true, true) {
            if let Some(bin) = template.first() {
                if binary_available(bin, on_path) {
                    return Some(c.to_string());
                }
            }
        }
    }
    installed_runners_with(on_path).into_iter().next()
}

fn checked_runner_bins() -> String {
    SHIPPED_RUNNERS
        .iter()
        .map(|r| r.bin)
        .collect::<Vec<_>>()
        .join(", ")
}

fn fill_vars(
    template: &[String],
    prompt: &str,
    prompt_file: Option<&Path>,
    cwd: Option<&str>,
    persona: &str,
) -> Vec<String> {
    let mut vars = tools::Vars::default()
        .with("prompt", prompt.to_string())
        .with("cwd", cwd.filter(|c| !c.is_empty()).unwrap_or("."))
        .with("persona", persona);
    if let Some(file) = prompt_file {
        vars = vars.with("prompt_file", file.display().to_string());
    }
    let mut filled = tools::fill(template, &vars);
    let mentions_prompt = template
        .iter()
        .any(|w| w.contains("{prompt}") || w.contains("{prompt_file}"));
    if !mentions_prompt && !filled.is_empty() {
        if let Some(file) = prompt_file {
            filled.push(file.display().to_string());
        } else {
            filled.push(prompt.to_string());
        }
    }
    filled
}

fn explicit_runner_choice(passed: Option<&str>) -> Option<String> {
    passed
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("LJOS_PANEL_RUNNER")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            std::env::var("LJOS_PANEL_HARNESS")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| configured_from_files().1)
}

fn missing_runner_error(requested: Option<(&str, &str)>, on_path: &dyn Fn(&str) -> bool) -> String {
    let installed = installed_runners_with(on_path);
    let available = if installed.is_empty() {
        format!("none of {} is on PATH", checked_runner_bins())
    } else {
        format!("installed on PATH: {}", installed.join(", "))
    };
    let how = format!(
        "install a runner CLI, or configure a command template with \
         `headless = [\"runner\", \"-p\", \"{{prompt}}\"]` in {} or \
         `[panel] command = [\"runner\", ...]` in ljos.toml (or set LJOS_PANEL_COMMAND)",
        crate::harnesses_path().display()
    );
    match requested {
        Some((name, bin)) if name == bin => {
            format!("panel runner {name:?} is not installed on PATH ({available}); {how}")
        }
        Some((name, bin)) => {
            format!("panel runner {name:?} ({bin:?}) is not installed on PATH ({available}); {how}")
        }
        None => {
            format!("no panel runner CLI is installed on PATH ({available}); {how}")
        }
    }
}

/// Resolve the argv for one `open-panel` member, failing loudly when no
/// runner CLI or configured command template is installed.
pub fn resolve_member_argv_with(
    prompt_file: &Path,
    cwd: Option<&str>,
    persona: &str,
    on_path: &dyn Fn(&str) -> bool,
) -> Result<Vec<String>> {
    if let Some(bin) = std::env::var("LJOS_MEMBER_BIN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        if !binary_available(&bin, on_path) {
            bail!("{}", missing_runner_error(Some((&bin, &bin)), on_path));
        }
        let mut args = vec![
            bin,
            "--prompt-file".into(),
            prompt_file.display().to_string(),
            "--yolo".into(),
            "--max-turns".into(),
            "6".into(),
            "--effort".into(),
            "low".into(),
            "--disallowed-tools".into(),
            "Agent".into(),
        ];
        if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
            args.push("--cwd".into());
            args.push(cwd.to_string());
        }
        return Ok(args);
    }

    let prompt = std::fs::read_to_string(prompt_file).unwrap_or_default();
    if let Some(template) = configured_command_template() {
        let argv = fill_vars(&template, &prompt, Some(prompt_file), cwd, persona);
        let Some(bin) = argv.first() else {
            bail!("configured panel command template is empty");
        };
        if !binary_available(bin, on_path) {
            bail!("{}", missing_runner_error(Some((bin, bin)), on_path));
        }
        return Ok(argv);
    }

    if let Some(named) = explicit_runner_choice(None) {
        let Some(template) = template_for_runner(&named, true, cwd.is_some()) else {
            if binary_available(&named, on_path) {
                let raw = vec![named, "-p".into(), "{prompt}".into()];
                return Ok(fill_vars(&raw, &prompt, Some(prompt_file), cwd, persona));
            }
            bail!("{}", missing_runner_error(Some((&named, &named)), on_path));
        };
        let argv = fill_vars(&template, &prompt, Some(prompt_file), cwd, persona);
        let bin = argv.first().map(String::as_str).unwrap_or(&named);
        if !binary_available(bin, on_path) {
            bail!("{}", missing_runner_error(Some((&named, bin)), on_path));
        }
        return Ok(argv);
    }

    let who = crate::whoami();
    let preferred = [who.seat.as_str()];
    let Some(chosen) = pick_installed_runner(&preferred, on_path) else {
        bail!("{}", missing_runner_error(None, on_path));
    };
    let template = template_for_runner(&chosen, true, cwd.is_some())
        .unwrap_or_else(|| vec![chosen.clone(), "-p".into(), "{prompt}".into()]);
    Ok(fill_vars(
        &template,
        &prompt,
        Some(prompt_file),
        cwd,
        persona,
    ))
}

/// Resolve the argv for one `open-panel` member using `PATH`.
pub fn resolve_member_argv(
    prompt_file: &Path,
    cwd: Option<&str>,
    persona: &str,
) -> Result<Vec<String>> {
    resolve_member_argv_with(prompt_file, cwd, persona, &crate::on_path)
}

/// Non-failing argv computation for `panel_member_argv`, using configured
/// templates or an installed runner CLI when available, and falling back to
/// the legacy `grok` argv when nothing is installed.
#[must_use]
pub fn member_argv_or_fallback(
    prompt_file: &Path,
    cwd: Option<&str>,
    persona: &str,
    on_path: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    if let Some(bin) = std::env::var("LJOS_MEMBER_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        let mut args = vec![
            bin,
            "--prompt-file".into(),
            prompt_file.display().to_string(),
            "--yolo".into(),
            "--max-turns".into(),
            "6".into(),
            "--effort".into(),
            "low".into(),
            "--disallowed-tools".into(),
            "Agent".into(),
        ];
        if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
            args.push("--cwd".into());
            args.push(cwd.to_string());
        }
        return args;
    }
    let prompt = std::fs::read_to_string(prompt_file).unwrap_or_default();
    if let Some(template) = configured_command_template() {
        return fill_vars(&template, &prompt, Some(prompt_file), cwd, persona);
    }
    if let Some(named) = explicit_runner_choice(None) {
        if let Some(template) = template_for_runner(&named, true, cwd.is_some()) {
            return fill_vars(&template, &prompt, Some(prompt_file), cwd, persona);
        }
    } else {
        let who = crate::whoami();
        let preferred = [who.seat.as_str()];
        if let Some(chosen) = pick_installed_runner(&preferred, on_path) {
            if let Some(template) = template_for_runner(&chosen, true, cwd.is_some()) {
                return fill_vars(&template, &prompt, Some(prompt_file), cwd, persona);
            }
        }
    }
    let mut args = vec![
        "grok".into(),
        "--prompt-file".into(),
        prompt_file.display().to_string(),
        "--yolo".into(),
        "--max-turns".into(),
        "6".into(),
        "--effort".into(),
        "low".into(),
        "--disallowed-tools".into(),
        "Agent".into(),
    ];
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        args.push("--cwd".into());
        args.push(cwd.to_string());
    }
    args
}

/// Resolve the harness label and check availability for `ljos panel run`.
pub fn resolve_panel_run_harness_with(
    passed: Option<&str>,
    dry_run: bool,
    on_path: &dyn Fn(&str) -> bool,
) -> Result<String> {
    if let Some(bin) = std::env::var("LJOS_MEMBER_BIN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        if !dry_run && !binary_available(&bin, on_path) {
            bail!("{}", missing_runner_error(Some((&bin, &bin)), on_path));
        }
        return Ok(explicit_runner_choice(passed).unwrap_or_else(|| "cursor-agent".into()));
    }
    if let Some(template) = configured_command_template() {
        let Some(bin) = template.first() else {
            bail!("configured panel command template is empty");
        };
        if !dry_run && !binary_available(bin, on_path) {
            bail!("{}", missing_runner_error(Some((bin, bin)), on_path));
        }
        return Ok(bin.clone());
    }
    if let Some(named) = explicit_runner_choice(passed) {
        if !dry_run {
            let argv = run_argv_for(&named, "probe", "probe");
            let bin = argv.first().map(String::as_str).unwrap_or(&named);
            if !binary_available(bin, on_path) {
                bail!("{}", missing_runner_error(Some((&named, bin)), on_path));
            }
        }
        return Ok(named);
    }
    if dry_run && std::env::var_os("VISSUE_NO_ROUTE").is_some() {
        return Ok("cursor-agent".into());
    }
    let who = crate::whoami();
    let preferred = [who.seat.as_str()];
    if let Some(chosen) = pick_installed_runner(&preferred, on_path) {
        return Ok(chosen);
    }
    if dry_run {
        return Ok("cursor-agent".into());
    }
    bail!("{}", missing_runner_error(None, on_path))
}

/// Build the argv for one `ljos panel run` session on `harness`.
#[must_use]
pub fn run_argv_for(harness: &str, prompt: &str, role: &str) -> Vec<String> {
    if let Some(bin) = std::env::var("LJOS_MEMBER_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return vec![bin, "-p".into(), prompt.to_string()];
    }
    if let Some(template) = configured_command_template() {
        return fill_vars(&template, prompt, None, None, role);
    }
    let trimmed = harness.trim();
    if trimmed.is_empty() || trimmed == "cursor" || trimmed == "cursor-agent" {
        if let Some(template) = harness_file_template("cursor") {
            return fill_vars(&template, prompt, None, None, role);
        }
        return vec!["cursor-agent".into(), "-p".into(), prompt.to_string()];
    }
    if let Some(template) = template_for_runner(trimmed, false, false) {
        return fill_vars(&template, prompt, None, None, role);
    }
    vec![trimmed.into(), "-p".into(), prompt.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_installed_runners_and_picks_in_order() {
        let only_codex = |bin: &str| bin == "codex";
        assert_eq!(installed_runners_with(&only_codex), vec!["codex"]);
        assert_eq!(
            pick_installed_runner(&["ljosbot"], &only_codex).as_deref(),
            Some("codex")
        );

        let claude_and_gemini = |bin: &str| matches!(bin, "claude" | "gemini");
        assert_eq!(
            installed_runners_with(&claude_and_gemini),
            vec!["claude", "gemini"]
        );
        assert_eq!(
            pick_installed_runner(&["gemini"], &claude_and_gemini).as_deref(),
            Some("gemini"),
            "prefers the seat's own runner when installed"
        );
    }

    #[test]
    fn fails_loudly_when_no_runner_is_installed() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let prompt_file = dir.path().join("member.prompt");
        std::fs::write(&prompt_file, "vote on the issue").unwrap();
        let none = |_: &str| false;

        let err = resolve_member_argv_with(&prompt_file, None, "skeptic", &none)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("no panel runner CLI is installed on PATH"),
            "{err}"
        );
        assert!(err.contains("harnesses.toml"), "{err}");
        assert!(err.contains("ljos.toml"), "{err}");

        let run_err = resolve_panel_run_harness_with(None, false, &none)
            .unwrap_err()
            .to_string();
        assert!(
            run_err.contains("no panel runner CLI is installed on PATH"),
            "{run_err}"
        );
    }

    #[test]
    fn custom_command_template_supports_any_runner() {
        let _g = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let prompt_file = dir.path().join("member.prompt");
        std::fs::write(&prompt_file, "review the diff").unwrap();

        let words = split_template_line(
            "custom-runner --prompt {prompt} --file {prompt_file} --as {persona} --dir {cwd}",
        );
        let filled = fill_vars(
            &words,
            "review the diff",
            Some(&prompt_file),
            Some("/repo"),
            "critic",
        );
        assert_eq!(
            filled,
            vec![
                "custom-runner",
                "--prompt",
                "review the diff",
                "--file",
                &prompt_file.display().to_string(),
                "--as",
                "critic",
                "--dir",
                "/repo",
            ]
        );
    }
}
