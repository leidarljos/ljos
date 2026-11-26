//! The cloud-agent brief rules a start pastes into a matching task.
//!
//! `ljos start` and `ljos_start` print the block when the task names a
//! cloud agent, a kit beside build, land, or handoff, a format-patch or
//! patch-only delivery, a `--brief` flag, or a cloud label. The words come
//! from the stored lesson whose title or text is `Cloud-agent brief rules`,
//! through the recall a start already runs. An ordinary task prints nothing.
//! With no stored lesson, the block is the built-in text. Its handoff
//! directory is `LJOS_HANDOFF_DIR`, or a placeholder when that is unset. A
//! stored lesson is kept as it was written, Org included, and cut past
//! 4096 bytes.

use serde_json::Value;

/// The lesson title a start looks up.
pub(crate) const PHRASE: &str = "Cloud-agent brief rules";

/// A stored lesson longer than this is cut. The built-in text is shorter.
const LESSON_CAP: usize = 4096;

/// How far, in tokens, `kit` may sit from build, land, or handoff.
const MARKER_REACH: usize = 3;

/// Shown when `LJOS_HANDOFF_DIR` is unset or blank.
const HANDOFF_PLACEHOLDER: &str = "<LJOS_HANDOFF_DIR>";

/// The issue title, for a start that was given an id rather than a sentence.
#[must_use]
pub fn issue_task(issue: &str) -> String {
    super::issue_title(issue).unwrap_or_else(|_| issue.to_string())
}

/// The brief-rules block for `task`, when `task` asks for one.
///
/// The body is the stored lesson whose title or text matches
/// [`PHRASE`]. The built-in text stands in when that lesson is absent.
/// An ordinary task returns nothing.
#[must_use]
pub fn for_task(task: &str) -> Option<String> {
    if !task_wants(task) {
        return None;
    }
    let body = match lesson_body(&super::recall_brief_atoms()) {
        Some(body) => bound_lesson(&body),
        None => default_brief(),
    };
    Some(surface(&body))
}

/// Whether `task` asks for the brief rules.
///
/// The clear phrases are `cloud agent`, `cloud-agent`, `format-patch`,
/// and `patch-only`. `kit` counts when it sits within [`MARKER_REACH`]
/// tokens of `build`, `land`, or `handoff`. A `--brief` flag or a cloud
/// label (`label:cloud`, `scope:cloud`, `[cloud]`, `+cloud`) counts on
/// its own. A lone brief, bundle, patch, kit, or handoff does not.
pub(crate) fn task_wants(task: &str) -> bool {
    if brief_flag(task) || cloud_label(task) {
        return true;
    }
    let tokens = task_tokens(task);
    token_pair(&tokens, "cloud", "agent")
        || token_pair(&tokens, "format", "patch")
        || token_pair(&tokens, "patch", "only")
        || kit_beside_marker(&tokens)
}

fn brief_flag(task: &str) -> bool {
    bounded(&task.to_ascii_lowercase(), "--brief")
}

fn cloud_label(task: &str) -> bool {
    let lower = task.to_ascii_lowercase();
    bounded(&lower, "[cloud]")
        || bounded(&lower, "+cloud")
        || bounded(&lower, "scope:cloud")
        || label_cloud(&lower)
}

fn bounded(text: &str, needle: &str) -> bool {
    let bytes = text.as_bytes();
    let pat = needle.as_bytes();
    let mut from = 0;
    while let Some(rel) = text[from..].find(needle) {
        let start = from + rel;
        let end = start + pat.len();
        let before = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if before && after {
            return true;
        }
        from = start + 1;
    }
    false
}

fn label_cloud(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("label") {
        let start = from + rel;
        let before = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let mut index = start + "label".len();
        if index < bytes.len() && bytes[index] == b's' {
            index += 1;
        }
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if !before || index >= bytes.len() || bytes[index] != b':' {
            from = start + 1;
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let end = index + "cloud".len();
        let word = end <= bytes.len() && lower[index..].starts_with("cloud");
        if word && (end == bytes.len() || !bytes[end].is_ascii_alphanumeric()) {
            return true;
        }
        from = start + 1;
    }
    false
}

fn token_pair(tokens: &[String], left: &str, right: &str) -> bool {
    tokens
        .windows(2)
        .any(|pair| pair[0] == left && pair[1] == right)
}

fn kit_beside_marker(tokens: &[String]) -> bool {
    let kits: Vec<usize> = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.as_str() == "kit")
        .map(|(index, _)| index)
        .collect();
    if kits.is_empty() {
        return false;
    }
    tokens.iter().enumerate().any(|(index, _)| {
        marker_at(tokens, index)
            && kits
                .iter()
                .any(|kit| index != *kit && index.abs_diff(*kit) <= MARKER_REACH)
    })
}

fn marker_at(tokens: &[String], index: usize) -> bool {
    match tokens[index].as_str() {
        "build" | "land" | "handoff" => true,
        "hand" => tokens.get(index + 1).is_some_and(|token| token == "off"),
        _ => false,
    }
}

fn task_tokens(task: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in task.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && prev_lower && !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            current.push(ch.to_ascii_lowercase());
            prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
            prev_lower = false;
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn phrase_line(line: &str) -> bool {
    let line = line.trim().trim_start_matches('#').trim();
    let line = line.trim_start_matches('*').trim();
    line.eq_ignore_ascii_case(PHRASE)
}

fn kept_kind(atom: &Value) -> bool {
    if atom.get("tombstone") == Some(&Value::Bool(true)) {
        return false;
    }
    let Some(kind) = atom.get("kind").and_then(Value::as_str) else {
        return true;
    };
    kind.is_empty() || crate::memories::REVIEW_KINDS.contains(&kind)
}

fn atom_title(atom: &Value) -> &str {
    atom.get("source")
        .and_then(|source| source.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn atom_text(atom: &Value) -> &str {
    atom.get("text").and_then(Value::as_str).unwrap_or("")
}

/// A lesson body is more than the title line. `---` fences and a bare
/// Org heading do not count.
fn has_body(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !phrase_line(line) && line != "---" && line != "*"
    })
}

/// The stored lesson whose title or text matches [`PHRASE`].
///
/// A title match wins over a text that only mentions the phrase. Among
/// matches of the same kind, the longer body is the rules.
fn lesson_body(atoms: &[Value]) -> Option<String> {
    let mut best: Option<(bool, String)> = None;
    for atom in atoms {
        if !kept_kind(atom) {
            continue;
        }
        let title = atom_title(atom);
        let text = atom_text(atom);
        let title_hit = phrase_line(title);
        let text_hit = text
            .to_ascii_lowercase()
            .contains(&PHRASE.to_ascii_lowercase())
            || text.lines().any(phrase_line);
        if !title_hit && !text_hit {
            continue;
        }
        let body = text.trim();
        if !has_body(body) {
            continue;
        }
        let replace = match &best {
            None => true,
            Some((was_title, was_body)) => {
                if title_hit != *was_title {
                    title_hit
                } else {
                    body.len() > was_body.len()
                }
            }
        };
        if replace {
            best = Some((title_hit, body.to_string()));
        }
    }
    best.map(|(_, body)| body)
}

fn bound_lesson(body: &str) -> String {
    if body.len() <= LESSON_CAP {
        return body.to_string();
    }
    let mut end = LESSON_CAP;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    if let Some(newline) = body[..end].rfind('\n') {
        if end - newline <= 160 {
            end = newline;
        }
    }
    let mut out = body[..end].trim_end().to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("(truncated)\n");
    out
}

fn handoff_dir() -> String {
    let Ok(value) = std::env::var("LJOS_HANDOFF_DIR") else {
        return HANDOFF_PLACEHOLDER.to_string();
    };
    let trimmed = value.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        HANDOFF_PLACEHOLDER.to_string()
    } else {
        trimmed.to_string()
    }
}

fn default_brief() -> String {
    let dir = handoff_dir();
    format!(
        "\
---
Cost rules (tokens are the bill; every call re-reads everything you have seen):
- One job. Do only what this brief asks; when the deliverable is written and checked, stop.
- Read narrowly: rg -n, then sed -n 'A,Bp'. Never open a whole file over ~200 lines.
- Quiet commands: output to a file, show tail -n 30 and EXIT code. Never paste full logs or diffs; name path and sha256.
- Batch mechanical steps (format-patch, bundle, verify, sha256sum, copy) in one shell call.
- Full suite on the tip only; on the base run only new/changed tests to show fail-before.
- Patch-only, compressed, through the store. Never push or open a PR. Deliver git format-patch --stdout BASE..HEAD | zstd -19 -q -o TAG.mbox.zst plus a git bundle (BASE prerequisite) and LOG.zst in {dir}/TAG/ with SHA256SUMS; check with sha256sum -c. No base64 unless that check fails.
- Final message under 40 lines: base, tip, tree, sha256s, test counts, EXIT codes, anything odd.
---
"
    )
}

fn surface(body: &str) -> String {
    let mut out = String::new();
    let first = body.lines().next().unwrap_or("");
    if !phrase_line(first) {
        out.push_str(PHRASE);
        out.push('\n');
    }
    out.push_str(body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Isolated {
        _env: crate::EnvLock,
        _dir: tempfile::TempDir,
    }

    fn isolated() -> Isolated {
        let env = crate::test_env_lock();
        let dir = tempfile::tempdir().expect("temp dir");
        let data = dir.path().join("data");
        std::fs::create_dir_all(&data).expect("data dir");
        crate::with_env_mutation(|| unsafe {
            std::env::set_var("PACKSET_URL", "off");
            std::env::set_var("XDG_DATA_HOME", &data);
            std::env::remove_var("LJOS_HANDOFF_DIR");
        });
        crate::plant_brief_atoms(None);
        Isolated {
            _env: env,
            _dir: dir,
        }
    }

    struct Planted;

    impl Drop for Planted {
        fn drop(&mut self) {
            crate::plant_brief_atoms(None);
        }
    }

    fn plant(atom: Value) -> Planted {
        crate::plant_brief_atoms(Some(vec![atom]));
        Planted
    }

    fn vendor_word() -> String {
        ["cur", "sor"].concat()
    }

    fn assert_no_vendor(block: &str) {
        let banned = vendor_word();
        if let Some(line) = block
            .lines()
            .find(|line| line.to_ascii_lowercase().contains(&banned))
        {
            panic!("vendor word on this line: {line}");
        }
    }

    #[test]
    fn a_kit_task_shows_the_default_brief_rules() {
        let _env_read = crate::test_env_read_lock();
        let _iso = isolated();
        let block = for_task("Ship the kit as a format-patch bundle").expect("kit task");
        assert!(block.starts_with("Cloud-agent brief rules\n"), "{block}");
        assert_eq!(
            block.matches("Cloud-agent brief rules").count(),
            1,
            "{block}"
        );
        assert!(block.contains("tokens are the bill"), "{block}");
        assert!(block.contains("<LJOS_HANDOFF_DIR>/TAG/"), "{block}");
        assert_no_vendor(&block);
    }

    #[test]
    fn a_plain_task_leaves_the_brief_rules_out() {
        let _env_read = crate::test_env_read_lock();
        let _iso = isolated();
        assert!(for_task("Fix the flaky retry test").is_none());
    }

    #[test]
    fn a_stored_lesson_overrides_the_default_brief_rules() {
        let _env_read = crate::test_env_read_lock();
        let _iso = isolated();
        let _plant = plant(serde_json::json!({
            "kind": "lesson",
            "text": "Stored kiln rule: stop after the deliverable.",
            "source": {"title": PHRASE},
        }));
        let block = for_task("build the kit").expect("kit task");
        assert!(
            block.contains("Stored kiln rule: stop after the deliverable."),
            "{block}"
        );
        assert!(!block.contains("tokens are the bill"), "{block}");
        assert_eq!(
            block.matches("Cloud-agent brief rules").count(),
            1,
            "{block}"
        );
        drop(_plant);

        let _plant = plant(serde_json::json!({
            "kind": "lesson",
            "text": "* Cloud-agent brief rules\nUse the org heading body.\n",
        }));
        let block = for_task("prepare the kit handoff").expect("handoff task");
        assert!(block.contains("Use the org heading body."), "{block}");
        assert!(!block.contains("tokens are the bill"), "{block}");
        assert!(block.contains("* Cloud-agent brief rules\n"), "{block}");
        assert_eq!(
            block.matches("Cloud-agent brief rules").count(),
            1,
            "{block}"
        );
    }

    #[test]
    fn ordinary_tasks_do_not_show_brief_rules() {
        let _env_read = crate::test_env_read_lock();
        let _iso = isolated();
        let ordinary = [
            "write the brief for the paper",
            "patch the typo",
            "bundle the docs",
            "Brief the team on retry behavior",
            "Bundle the web assets",
            "Apply the security patch",
            "Ship the kit",
            "prepare the handoff bundle",
            "cloud storage notes",
        ];
        let shown: Vec<_> = ordinary
            .into_iter()
            .filter(|task| for_task(task).is_some())
            .collect();
        assert!(shown.is_empty(), "rules shown for: {shown:?}");
    }

    #[test]
    fn clear_phrases_show_brief_rules() {
        let _env_read = crate::test_env_read_lock();
        let _iso = isolated();
        let phrases = [
            "file it label:cloud",
            "file it labels: cloud",
            "file it [cloud]",
            "file it +cloud",
            "file it scope:cloud",
            "write the paper --brief",
            "launch a cloud agent",
            "cloud-agent handoff",
            "build the kit",
            "land the kit",
            "kit handoff",
            "hand-off the kit",
            "send the format-patch",
            "deliver patch-only",
        ];
        let missed: Vec<_> = phrases
            .into_iter()
            .filter(|task| for_task(task).is_none())
            .collect();
        assert!(missed.is_empty(), "missing rules: {missed:?}");
    }

    #[test]
    fn the_builtin_block_uses_the_handoff_setting() {
        let _env_read = crate::test_env_read_lock();
        let _iso = isolated();
        let unset = for_task("build the kit").expect("rules");
        assert!(unset.contains("<LJOS_HANDOFF_DIR>/TAG/"), "{unset}");
        assert_eq!(
            unset.matches("Cloud-agent brief rules").count(),
            1,
            "{unset}"
        );
        assert_no_vendor(&unset);
        crate::with_env_mutation(|| unsafe {
            std::env::set_var("LJOS_HANDOFF_DIR", "/var/handoff/out/");
        });
        let set = for_task("build the kit").expect("rules");
        assert!(set.contains("/var/handoff/out/TAG/"), "{set}");
        assert!(!set.contains("<LJOS_HANDOFF_DIR>"), "{set}");
        assert_no_vendor(&set);
        crate::with_env_mutation(|| unsafe {
            std::env::set_var("LJOS_HANDOFF_DIR", "  ");
        });
        let blank = for_task("build the kit").expect("rules");
        assert!(blank.contains("<LJOS_HANDOFF_DIR>/TAG/"), "{blank}");
        assert_no_vendor(&blank);
    }

    #[test]
    fn a_long_lesson_is_cut() {
        let _env_read = crate::test_env_read_lock();
        let _iso = isolated();
        let mut text = String::from("Stored rule start.\n");
        text.push_str(&"x".repeat(8_000));
        text.push_str("\nUNIQUE_TAIL_MARKER\n");
        let _plant = plant(serde_json::json!({
            "kind": "lesson",
            "text": text,
            "source": {"title": PHRASE},
        }));
        let block = for_task("build the kit").expect("kit task");
        assert!(!block.contains("UNIQUE_TAIL_MARKER"), "len {}", block.len());
        assert!(block.contains("Stored rule start."), "len {}", block.len());
        assert!(block.contains("(truncated)"), "len {}", block.len());
        assert!(block.len() <= 4_200, "len {}", block.len());
    }
}
