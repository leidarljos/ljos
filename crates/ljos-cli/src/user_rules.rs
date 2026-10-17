//! Standing rules shared by every seat of this user, and by every machine
//! that shares the tracker repository.
//!
//! `ljos prefer --user` and `ljos rule --user` write them. The copy the
//! hooks read is the user config, not the pack, so the review clock never
//! sees them. The tracker carries one file per machine, and a sync merges
//! those files by id.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Rule;

/// How much of the preface a brief or a hook will carry.
pub const PREFACE_BUDGET: usize = 1200;

/// Handles and names a guard matches. The prose of the rule is not read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct People {
    /// GitHub logins, without a leading `@`, compared case-insensitively.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handles: Vec<String>,
    /// Display names, matched as whole words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
}

/// One standing rule. `due_at` is never written. A sweep drops a row only
/// when that field is present and already past, which a row this module
/// writes is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRule {
    pub id: String,
    pub text: String,
    pub author: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "people_empty")]
    pub deny_mention: People,
    #[serde(default, skip_serializing_if = "people_empty")]
    pub deny_reply_to: People,
    pub written_at: String,
    /// When this row last changed. A merge keeps the newer stamp, and with
    /// it that row's author and reason.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_at: String,
    /// A deletion. The row stays so an older copy of the same id does not
    /// come back on the next sync.
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
    /// The lesson id a promote copied, when this row came from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Absent on every row the commands write. Present only so a sweep can
    /// drop a row that was given a review date by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<String>,
}

fn people_empty(p: &People) -> bool {
    p.handles.is_empty() && p.names.is_empty()
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    rules: Vec<UserRule>,
}

/// `$XDG_CONFIG_HOME/ljos/user-rules.json`, else `~/.config/ljos/user-rules.json`.
#[must_use]
pub fn rules_path() -> Option<PathBuf> {
    Some(super::config_base()?.join("ljos").join("user-rules.json"))
}

/// The rows on disk that are still in force. An absent file is an empty
/// list. Tombstones are left out.
///
/// # Errors
///
/// The file is not readable, or it is not the JSON this module writes.
pub fn load() -> Result<Vec<UserRule>> {
    Ok(read_all()?
        .into_iter()
        .filter(|rule| !rule.deleted)
        .collect())
}

/// Every row on disk, tombstones included.
///
/// # Errors
///
/// The file is not readable, or it is not the JSON this module writes.
pub(crate) fn read_all() -> Result<Vec<UserRule>> {
    let Some(path) = rules_path() else {
        return Ok(Vec::new());
    };
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("user rules: read {} failed", path.display()))?;
    parse_rules(&text)
        .with_context(|| format!("user rules: {} is not a user-rules file", path.display()))
}

/// Parse one user-rules document. An empty body is an empty list.
///
/// # Errors
///
/// The text is not the JSON this module writes.
pub(crate) fn parse_rules(text: &str) -> Result<Vec<UserRule>> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let file: File = serde_json::from_str(text)?;
    Ok(file.rules)
}

/// The document this module writes, tombstones included.
///
/// # Errors
///
/// The rows cannot be encoded.
pub(crate) fn render_rules(rules: &[UserRule]) -> Result<String> {
    let rules: Vec<UserRule> = rules.iter().cloned().map(dated).collect();
    let body = serde_json::to_string_pretty(&File { rules })?;
    Ok(format!("{body}\n"))
}

fn store(rules: &[UserRule]) -> Result<()> {
    let path = rules_path().context("user rules: no home and no XDG_CONFIG_HOME")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("user rules: create {} failed", parent.display()))?;
    }
    let body = render_rules(rules)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &body)
        .with_context(|| format!("user rules: write {} failed", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("user rules: rename into {} failed", path.display()))?;
    Ok(())
}

/// Replace the local file. Used by a sync, which must not start another sync.
pub(crate) fn replace_stored(rules: &[UserRule]) -> Result<()> {
    store(rules)
}

fn append(rule: UserRule) -> Result<UserRule> {
    let mut rules = read_all()?;
    rules.push(rule.clone());
    store(&rules)?;
    sync_after_write();
    Ok(rule)
}

/// Whether a row would be due at `now`. A row with no `due_at` is never due.
#[must_use]
pub fn is_due(rule: &UserRule, now: &str) -> bool {
    rule.due_at
        .as_deref()
        .is_some_and(|due| !due.is_empty() && due <= now)
}

/// How many rows would be due at `now`. Ordinary rows contribute nothing.
#[must_use]
pub fn review_due(now: &str) -> usize {
    load()
        .unwrap_or_default()
        .iter()
        .filter(|rule| is_due(rule, now))
        .count()
}

/// Drop rows whose `due_at` is already past `now`, and leave every other
/// row where it is. Returns how many were dropped.
///
/// # Errors
///
/// The file cannot be read or rewritten.
pub fn sweep(now: &str) -> Result<usize> {
    let rules = read_all()?;
    let kept: Vec<UserRule> = rules
        .iter()
        .filter(|rule| !is_due(rule, now))
        .cloned()
        .collect();
    let dropped = rules.len() - kept.len();
    if dropped > 0 {
        store(&kept)?;
    }
    Ok(dropped)
}

fn new_id(text: &str) -> String {
    let mut raw = [0u8; 16];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = file.read_exact(&mut raw);
    }
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    for (i, byte) in raw.iter_mut().enumerate().take(8) {
        *byte ^= (tick >> (i * 8)) as u8;
    }
    for (i, byte) in text.bytes().enumerate() {
        raw[i % raw.len()] ^= byte.wrapping_add(i as u8);
    }
    let mut hex = String::with_capacity(32);
    for byte in raw {
        hex.push(b"0123456789abcdef"[(byte >> 4) as usize] as char);
        hex.push(b"0123456789abcdef"[(byte & 0x0f) as usize] as char);
    }
    format!("ur-{hex}")
}

fn stamp(rule: &UserRule) -> &str {
    let at = rule.updated_at.trim();
    if at.is_empty() {
        rule.written_at.trim()
    } else {
        at
    }
}

fn dated(mut rule: UserRule) -> UserRule {
    if rule.updated_at.trim().is_empty() {
        rule.updated_at = rule.written_at.clone();
    }
    rule
}

/// Merge rows by id. The newer `updated_at` wins, and that row's author
/// and reason come with it. A tombstone beats an older edit of the same
/// id, and it also beats a live row that carries the same stamp.
#[must_use]
pub fn merge_rows(rows: impl IntoIterator<Item = UserRule>) -> Vec<UserRule> {
    let mut best: std::collections::BTreeMap<String, UserRule> = std::collections::BTreeMap::new();
    for rule in rows {
        let id = rule.id.trim().to_string();
        if id.is_empty() {
            continue;
        }
        let rule = dated(rule);
        if best.get(&id).is_some_and(|have| beats(have, &rule)) {
            continue;
        }
        best.insert(id, rule);
    }
    best.into_values().collect()
}

fn beats(have: &UserRule, incoming: &UserRule) -> bool {
    match stamp(have).cmp(stamp(incoming)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => have.deleted && !incoming.deleted,
    }
}

fn normalize_handle(raw: &str) -> Result<String> {
    let handle = raw.trim().trim_start_matches('@');
    if handle.is_empty()
        || !handle
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        bail!("prefer: {raw:?} is not a handle");
    }
    Ok(handle.to_ascii_lowercase())
}

fn clean_people(people: &People) -> Result<People> {
    let mut handles = Vec::new();
    for handle in &people.handles {
        let handle = normalize_handle(handle)?;
        if !handles.contains(&handle) {
            handles.push(handle);
        }
    }
    let mut names = Vec::new();
    for name in &people.names {
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            bail!("prefer: a name needs letters");
        }
        if !names
            .iter()
            .any(|kept: &String| kept.eq_ignore_ascii_case(&name))
        {
            names.push(name);
        }
    }
    Ok(People { handles, names })
}

fn fresh(text: &str, reason: &str, from: Option<String>) -> Result<UserRule> {
    let text = text.trim();
    let reason = reason.trim();
    if text.is_empty() {
        bail!("prefer: a user rule needs the words it states");
    }
    if reason.is_empty() {
        bail!("prefer: a user rule needs a reason, so say why with --why");
    }
    let now = super::now_utc();
    Ok(UserRule {
        id: new_id(text),
        text: text.to_string(),
        author: super::holder_name(),
        reason: reason.to_string(),
        pattern: None,
        verdict: None,
        deny_mention: People::default(),
        deny_reply_to: People::default(),
        written_at: now.clone(),
        updated_at: now,
        deleted: false,
        from,
        due_at: None,
    })
}

/// Write one prose rule. It is not also posted to the seat pack.
///
/// # Errors
///
/// The text or the reason is empty, a handle is not a login, or the file
/// cannot be written.
pub fn write_preference(
    text: &str,
    reason: &str,
    deny_mention: &People,
    deny_reply_to: &People,
) -> Result<UserRule> {
    let mut rule = fresh(text, reason, None)?;
    rule.deny_mention = clean_people(deny_mention)?;
    rule.deny_reply_to = clean_people(deny_reply_to)?;
    append(rule)
}

/// Write one argv law into the user file, under the same checks as a seat rule.
///
/// # Errors
///
/// [`super::validate_rule`] refuses the pattern, or the file cannot be written.
pub fn write_argv(rule: &Rule) -> Result<UserRule> {
    super::validate_rule(rule)?;
    let mut row = fresh(rule.reason.trim(), rule.reason.trim(), None)?;
    row.pattern = Some(rule.pattern.trim().to_string());
    row.verdict = Some(rule.verdict.clone());
    append(row)
}

/// Copy one lesson atom into a user rule. The atom is not modified.
///
/// # Errors
///
/// The atom is not a lesson, its text or the reason is empty, or the file
/// cannot be written.
pub fn promote_atom(
    atom: &Value,
    reason: &str,
    deny_mention: &People,
    deny_reply_to: &People,
) -> Result<UserRule> {
    let kind = atom.get("kind").and_then(Value::as_str).unwrap_or("");
    if kind != "lesson" {
        let id = atom.get("id").and_then(Value::as_str).unwrap_or("(no id)");
        bail!("prefer: {id} is a {kind}, and only a lesson is promoted");
    }
    let id = atom
        .get("id")
        .and_then(Value::as_str)
        .context("prefer: the lesson has no id")?
        .to_string();
    let text = atom
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let mut rule = fresh(text, reason, Some(id))?;
    rule.deny_mention = clean_people(deny_mention)?;
    rule.deny_reply_to = clean_people(deny_reply_to)?;
    append(rule)
}

/// Tombstone `id`. The author and the reason stay, so a later sync can say
/// who removed it and why the rule stood. The lesson, if this row was
/// promoted, is not touched.
///
/// # Errors
///
/// No such row, or the file cannot be written.
pub fn forget(id: &str) -> Result<UserRule> {
    let id = id.trim();
    if id.is_empty() {
        bail!("prefer: a user rule id is required");
    }
    let mut rules = read_all()?;
    let Some(rule) = rules.iter_mut().find(|rule| rule.id == id) else {
        bail!("prefer: no user rule {id}");
    };
    if !rule.deleted {
        rule.deleted = true;
        rule.updated_at = super::now_utc();
        let written = rule.clone();
        store(&rules)?;
        sync_after_write();
        return Ok(written);
    }
    Ok(rule.clone())
}

/// Read lesson `id` from this seat's pack and copy it into a user rule.
/// The lesson stays in the pack.
///
/// # Errors
///
/// The pack does not answer, no such lesson, or [`promote_atom`] refuses it.
pub fn promote_lesson(
    id: &str,
    reason: &str,
    deny_mention: &People,
    deny_reply_to: &People,
) -> Result<UserRule> {
    let id = id.trim();
    if id.is_empty() {
        bail!("prefer: a lesson id is required");
    }
    let client = super::pack()?;
    let atoms =
        super::atoms_lean(&client, &client.workspace()).context("prefer: GET /v1/atoms failed")?;
    let atom = atoms
        .into_iter()
        .find(|atom| atom.get("id").and_then(Value::as_str) == Some(id))
        .with_context(|| format!("prefer: no atom {id} in this seat's pack"))?;
    promote_atom(&atom, reason, deny_mention, deny_reply_to)
}

fn sync_budget_ms(default: u64) -> u64 {
    std::env::var("LJOS_USER_RULES_SYNC_MS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .filter(|ms: &u64| *ms > 0)
        .unwrap_or(default)
}

fn sync_after_write() {
    let budget = std::time::Duration::from_millis(sync_budget_ms(4_000));
    let host = super::sync::host();
    for root in super::sync::user_rule_trackers() {
        let _ = super::sync::exchange_user_rules(&root, &host, budget);
    }
}

/// Pull and push before a hook builds its line. A tracker that does not
/// answer is ignored, and the local file is what the hook shows.
pub fn sync_for_hook() {
    let budget = std::time::Duration::from_millis(sync_budget_ms(1_500));
    let host = super::sync::host();
    for root in super::sync::user_rule_trackers() {
        let _ = super::sync::exchange_user_rules(&root, &host, budget);
    }
}

/// One pull and push, as `ljos prefer --user --sync` runs it.
///
/// # Errors
///
/// Sync is switched off, no tracker is configured, or the tracker did not
/// answer inside the budget.
pub fn sync_now() -> Result<String> {
    if std::env::var("LJOS_USER_RULES_SYNC").ok().as_deref() == Some("off") {
        bail!("prefer: user-rule sync is off");
    }
    let roots = super::sync::user_rule_trackers();
    if roots.is_empty() {
        bail!(
            "prefer: no tracker repository to sync user rules through. Set VISSUE_ROOT, or repos in ~/.config/ljos/sync.toml"
        );
    }
    let budget = std::time::Duration::from_millis(sync_budget_ms(8_000));
    let host = super::sync::host();
    let mut out = String::new();
    for root in roots {
        out.push_str(&super::sync::exchange_user_rules(&root, &host, budget)?);
    }
    if out.is_empty() {
        out.push_str("sync: user rules: none\n");
    }
    Ok(out)
}

/// The rule as one line a person reads after writing it.
#[must_use]
pub fn ack(rule: &UserRule) -> String {
    if rule.deleted {
        format!("user rule {}\tremoved\t{}", rule.id, rule.text)
    } else {
        format!("user rule {}\t{}", rule.id, rule.text)
    }
}

/// The rule as the object an MCP tool returns.
#[must_use]
pub fn to_value(rule: &UserRule) -> Value {
    serde_json::to_value(rule).unwrap_or(Value::Null)
}

/// Text lines for `ljos rules`, empty when there are none.
#[must_use]
pub fn format_list() -> String {
    let Ok(rules) = load() else {
        return String::new();
    };
    if rules.is_empty() {
        return String::new();
    }
    let mut out = String::from("user rules (shared, never reviewed):\n");
    for rule in &rules {
        let law = match (&rule.verdict, &rule.pattern) {
            (Some(verdict), Some(pattern)) => format!("{verdict}\t{pattern}\t"),
            _ => String::new(),
        };
        out.push_str(&format!(
            "user\t{law}{}\t{}\t{}\n",
            rule.text, rule.author, rule.reason
        ));
    }
    out
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn omission(left: usize) -> String {
    if left == 0 {
        String::new()
    } else {
        format!("{left} user rules omitted.\n")
    }
}

/// The preface injected ahead of seat memory. Empty when there are no rules.
#[must_use]
pub fn preface() -> String {
    render(&load().unwrap_or_default(), PREFACE_BUDGET)
}

fn render(rules: &[UserRule], budget: usize) -> String {
    if rules.is_empty() || budget == 0 {
        return String::new();
    }
    let header = "User rules (standing, shared by every seat on this machine):\n";
    let mut body = String::new();
    let mut shown = 0usize;
    for rule in rules {
        let line = format!("- {}\n", one_line(&rule.text));
        let note = omission(rules.len() - (shown + 1));
        let used = header.chars().count()
            + body.chars().count()
            + line.chars().count()
            + note.chars().count();
        if used <= budget {
            body.push_str(&line);
            shown += 1;
            continue;
        }
        if shown == 0 {
            let note = omission(rules.len().saturating_sub(1));
            let room = budget.saturating_sub(header.chars().count() + note.chars().count());
            let take = room.saturating_sub(6);
            if take > 0 {
                let cut: String = one_line(&rule.text).chars().take(take).collect();
                body.push_str(&format!("- {cut}...\n"));
                shown = 1;
            }
        }
        break;
    }
    if shown == 0 {
        return String::new();
    }
    let mut out = format!("{header}{body}");
    let left = rules.len() - shown;
    if left > 0 {
        out.push_str(&omission(left));
    }
    if out.chars().count() > budget {
        out = out.chars().take(budget).collect();
    }
    out.trim_end().to_string()
}

/// `body`, with the user-rule preface in front. An empty preface leaves
/// `body` as it is.
#[must_use]
pub fn prepend(body: String) -> String {
    let head = preface();
    if head.is_empty() {
        body
    } else if body.is_empty() {
        head
    } else {
        format!("{head}\n{body}")
    }
}

/// `text` with one leading preface removed, so a session line and a prompt
/// note that both carry it are not pasted twice.
#[must_use]
pub fn without_preface(text: &str) -> String {
    let head = preface();
    if head.is_empty() {
        return text.to_string();
    }
    text.trim_start()
        .strip_prefix(&head)
        .map(|rest| rest.trim_start().to_string())
        .unwrap_or_else(|| text.to_string())
}

fn as_argv(rule: &UserRule) -> Option<Rule> {
    Some(Rule {
        pattern: rule.pattern.clone()?,
        verdict: rule.verdict.clone().unwrap_or_else(|| "ask".to_string()),
        reason: rule.reason.clone(),
    })
}

fn fired(rule: &UserRule) -> Rule {
    let why = rule.reason.trim();
    let text = rule.text.trim();
    let reason = if text.is_empty() || text == why {
        why.to_string()
    } else {
        format!("{text} ({why})")
    };
    Rule {
        pattern: format!("user-rule:{}", rule.id),
        verdict: "deny".into(),
        reason,
    }
}

/// Whether `rule` is an outbound-post denial, the only verdict
/// `beforeMCPExecution` enforces.
#[must_use]
pub fn mcp_event_denies(event: &str, rule: &Rule) -> bool {
    event == "beforeMCPExecution"
        && rule.verdict == "deny"
        && rule.pattern.starts_with("user-rule:")
}

struct Post {
    bodies: Vec<String>,
    reply_values: Vec<String>,
}

/// The user-rule verdict for a shell line, if one fires.
#[must_use]
pub fn guard_line(line: &str) -> Option<Rule> {
    let rules = load().unwrap_or_default();
    if rules.is_empty() || line.trim().is_empty() {
        return None;
    }
    let argv: Vec<Rule> = rules.iter().filter_map(as_argv).collect();
    if let Some(hit) = super::verdict_for(&argv, line).filter(|rule| rule.verdict == "deny") {
        return Some(hit.clone());
    }
    if let Some(hit) = outbound(line, &rules) {
        return Some(hit);
    }
    super::verdict_for(&argv, line).cloned()
}

/// The user-rule verdict for a hook payload that names a tool, if the tool
/// is a GitHub comment, review, issue or pull-request write.
#[must_use]
pub fn guard_payload(raw: &str) -> Option<Rule> {
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    let tool = value
        .get("tool_name")
        .or_else(|| value.get("toolName"))
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/toolCall/name").and_then(Value::as_str))?;
    let server = ["mcp_server_name", "mcpServerName", "server_name", "server"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str));
    let input = value
        .get("tool_input")
        .or_else(|| value.get("toolInput"))
        .or_else(|| value.pointer("/toolCall/args"));
    let parsed = match input {
        Some(Value::String(text)) => {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        Some(other) => other.clone(),
        None => Value::Null,
    };
    guard_mcp(tool, server, &parsed)
}

fn guard_mcp(tool: &str, server: Option<&str>, input: &Value) -> Option<Rule> {
    if !github_write(tool, server) {
        return None;
    }
    let rules = load().unwrap_or_default();
    let post = mcp_fields(input, tool);
    rules.iter().find(|rule| post_hits(rule, &post)).map(fired)
}

fn github_write(tool: &str, server: Option<&str>) -> bool {
    let tool_l = tool.to_ascii_lowercase();
    let short = tool_l.rsplit("__").next().unwrap_or(&tool_l);
    let short = short.rsplit('/').next().unwrap_or(short);
    let githubish = server.is_some_and(|name| name.to_ascii_lowercase().contains("github"))
        || tool_l.contains("github");
    if !githubish {
        return false;
    }
    if short.starts_with("get_")
        || short.starts_with("list_")
        || short.starts_with("search_")
        || short.starts_with("read_")
        || short.contains("_read")
    {
        return false;
    }
    ["comment", "review", "issue", "pull"]
        .iter()
        .any(|kind| short.contains(kind))
}

fn outbound(line: &str, rules: &[UserRule]) -> Option<Rule> {
    let posts = posts_of(line);
    if posts.is_empty() {
        return None;
    }
    rules.iter().find_map(|rule| {
        posts
            .iter()
            .any(|post| post_hits(rule, post))
            .then(|| fired(rule))
    })
}

fn post_hits(rule: &UserRule, post: &Post) -> bool {
    let mentioned = post
        .bodies
        .iter()
        .any(|body| mentions(&rule.deny_mention, body));
    let replies = post
        .reply_values
        .iter()
        .any(|value| identifies(&rule.deny_reply_to, value));
    mentioned || replies
}

fn posts_of(line: &str) -> Vec<Post> {
    let mut out = Vec::new();
    for sub in super::nested_lines(line, 0) {
        for raw in super::raw_segments(&sub) {
            let words = super::shell_words(&raw);
            if words.is_empty() {
                continue;
            }
            let (at, _) = super::prefix_end(&words);
            if at >= words.len() {
                continue;
            }
            if let Some(post) = gh_post(&words[at..]) {
                out.push(post);
            }
        }
    }
    out
}

fn gh_post(words: &[String]) -> Option<Post> {
    let bin = words.first()?.rsplit(['/', '\\']).next()?;
    if bin != "gh" {
        return None;
    }
    let args = skip_global(&words[1..]);
    let group = args.first()?.as_str();
    let verb = args.get(1).map(String::as_str);
    let write = match (group, verb) {
        ("pr" | "issue", Some("comment" | "review" | "create" | "edit")) => true,
        ("api", _) => api_write(&args[1..]),
        _ => false,
    };
    if !write {
        return None;
    }
    Some(Post {
        bodies: bodies_of(&args),
        reply_values: reply_values_of(&args),
    })
}

fn skip_global(words: &[String]) -> Vec<String> {
    let mut i = 0;
    while i < words.len() {
        let word = words[i].as_str();
        if word == "--" {
            return words[i + 1..].to_vec();
        }
        if !word.starts_with('-') {
            break;
        }
        let takes_value = matches!(word, "-R" | "--repo" | "--hostname");
        i += if word.contains('=') || !takes_value {
            1
        } else {
            2
        };
    }
    words[i..].to_vec()
}

fn api_write(args: &[String]) -> bool {
    matches!(method_of(args).as_deref(), Some("POST" | "PATCH" | "PUT"))
}

fn method_of(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let word = args[i].as_str();
        if let Some(value) = word
            .strip_prefix("--method=")
            .or_else(|| word.strip_prefix("-X="))
        {
            return Some(value.to_ascii_uppercase());
        }
        if word == "--method" || word == "-X" {
            return args.get(i + 1).map(|value| value.to_ascii_uppercase());
        }
        if let Some(value) = word.strip_prefix("-X") {
            if !value.is_empty() && !value.starts_with('-') {
                return Some(value.to_ascii_uppercase());
            }
        }
        i += 1;
    }
    None
}

const BODY_FLAGS: &[&str] = &["--body", "-b", "--title", "-t", "--subject"];
const FIELD_FLAGS: &[&str] = &["-f", "-F", "--field", "--raw-field"];
const BODY_KEYS: &[&str] = &["body", "title", "comment", "review"];
const REPLY_KEYS: &[&str] = &["in_reply_to", "in_reply_to_user", "reply_to", "reply-to"];

fn bodies_of(args: &[String]) -> Vec<String> {
    let mut out = flag_values(args, BODY_FLAGS);
    for (key, value) in assignments(args) {
        if BODY_KEYS.iter().any(|name| key.eq_ignore_ascii_case(name)) {
            out.push(value);
        }
    }
    out
}

fn reply_values_of(args: &[String]) -> Vec<String> {
    let mut out = flag_values(args, &["--reply-to", "--reply-to-user"]);
    for (key, value) in assignments(args) {
        if REPLY_KEYS.iter().any(|name| key.eq_ignore_ascii_case(name)) {
            out.push(value);
        }
    }
    for arg in args {
        if let Some((_, query)) = arg.split_once('?') {
            for pair in query.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    if REPLY_KEYS.iter().any(|name| key.eq_ignore_ascii_case(name)) {
                        out.push(value.to_string());
                    }
                }
            }
        }
    }
    out
}

fn assignments(args: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let word = &args[i];
        if FIELD_FLAGS.contains(&word.as_str()) {
            if let Some(next) = args.get(i + 1) {
                if let Some((key, value)) = next.split_once('=') {
                    out.push((key.to_string(), value.to_string()));
                }
            }
            i += 2;
            continue;
        }
        if let Some(rest) = word
            .strip_prefix("--raw-field=")
            .or_else(|| word.strip_prefix("--field="))
        {
            if let Some((key, value)) = rest.split_once('=') {
                out.push((key.to_string(), value.to_string()));
            }
        }
        i += 1;
    }
    out
}

fn flag_values(args: &[String], names: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let word = &args[i];
        if let Some((key, value)) = word.split_once('=') {
            if names.contains(&key) && !value.is_empty() {
                out.push(value.to_string());
            }
        }
        if names.contains(&word.as_str()) {
            if let Some(next) = args.get(i + 1) {
                if !next.starts_with('-') {
                    out.push(next.clone());
                    i += 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

fn mcp_fields(input: &Value, tool: &str) -> Post {
    let mut bodies = Vec::new();
    let mut reply_values = Vec::new();
    collect_keyed(
        input,
        &["body", "comment", "title", "review", "message", "content"],
        &mut bodies,
    );
    collect_keyed(
        input,
        &[
            "in_reply_to",
            "in_reply_to_user",
            "inReplyTo",
            "reply_to",
            "replyTo",
            "user",
            "author",
            "reviewer",
        ],
        &mut reply_values,
    );
    if tool.to_ascii_lowercase().contains("reply") {
        collect_strings(input, &mut reply_values);
    }
    Post {
        bodies,
        reply_values,
    }
}

fn collect_keyed(value: &Value, keys: &[&str], out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if keys.iter().any(|name| key.eq_ignore_ascii_case(name)) {
                    collect_strings(child, out);
                } else {
                    collect_keyed(child, keys, out);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_keyed(child, keys, out);
            }
        }
        _ => {}
    }
}

fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => {
            for child in items {
                collect_strings(child, out);
            }
        }
        Value::Object(map) => {
            for child in map.values() {
                collect_strings(child, out);
            }
        }
        _ => {}
    }
}

fn mentions(people: &People, text: &str) -> bool {
    people
        .handles
        .iter()
        .any(|handle| mentions_handle(text, handle))
        || people.names.iter().any(|name| mentions_name(text, name))
}

fn identifies(people: &People, value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let bare = value.trim_start_matches('@').to_ascii_lowercase();
    people.handles.iter().any(|handle| {
        let handle = handle.trim().trim_start_matches('@').to_ascii_lowercase();
        !handle.is_empty() && (bare == handle || mentions_handle(value, &handle))
    }) || people.names.iter().any(|name| mentions_name(value, name))
}

fn mentions_handle(text: &str, handle: &str) -> bool {
    let handle = handle.trim().trim_start_matches('@').to_ascii_lowercase();
    if handle.is_empty() {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    let needle = format!("@{handle}");
    let bytes = lower.as_bytes();
    let mark = needle.as_bytes();
    let mut i = 0;
    while i + mark.len() <= bytes.len() {
        if &bytes[i..i + mark.len()] == mark {
            let after = bytes.get(i + mark.len()).copied();
            if after.is_none_or(|c| !is_handle_char(c as char)) {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn is_handle_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

fn mentions_name(text: &str, name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let mut i = 0;
    while i + name.len() <= text.len() {
        if text[i..i + name.len()] == name[..] {
            let before = i == 0 || !text[i - 1].is_alphanumeric();
            let after = i + name.len();
            let after_ok = after == text.len() || !text[after].is_alphanumeric();
            if before && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated() -> (super::super::EnvLock, tempfile::TempDir) {
        let lock = super::super::test_env_lock();
        // A home under `/tmp` suspends the guard. The scratch directory
        // sits beside the build output so these rules are actually judged.
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/user-rule-homes");
        std::fs::create_dir_all(&root).unwrap();
        let dir = tempfile::TempDir::new_in(&root).unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
            std::env::set_var("HOME", dir.path());
            std::env::set_var("LJOS_HOLD", "rohit");
            std::env::set_var("LJOS_SEAT", "claude");
            std::env::set_var("PACKSET_URL", "off");
            std::env::set_var("PACKSET_WORKSPACE", "seat-a");
            std::env::set_var("LJOS_USER_RULES_SYNC", "off");
        }
        (lock, dir)
    }

    fn alan() -> UserRule {
        write_preference(
            "never post or reply to Alan (ocaisa)",
            "he asked not to be pinged",
            &People {
                handles: vec!["ocaisa".into()],
                names: vec!["Alan".into()],
            },
            &People {
                handles: vec!["@ocaisa".into()],
                names: vec!["Alan".into()],
            },
        )
        .unwrap()
    }

    #[test]
    fn a_user_rule_is_surfaced_in_a_second_seat() {
        let (_lock, dir) = isolated();
        let written = alan();
        assert_eq!(written.author, "rohit");
        assert!(written.due_at.is_none());
        unsafe {
            std::env::set_var("LJOS_SEAT", "codex");
            std::env::set_var("PACKSET_WORKSPACE", "seat-b");
        }
        let path = rules_path().unwrap();
        assert!(path.starts_with(dir.path().join("config")));
        assert!(!path.to_string_lossy().contains("codex"));
        assert!(!path.to_string_lossy().contains("claude"));
        let head = preface();
        assert!(head.starts_with("User rules"), "{head}");
        assert!(head.contains("never post or reply to Alan"), "{head}");
        let start = super::super::session_start_for(Some("seat-two"));
        assert!(start.starts_with("User rules"), "{start}");
        let at = start.find("never post or reply to Alan").unwrap();
        let on = start.find("ljos is on").unwrap();
        assert!(at < on, "{start}");
        let call = super::super::HookCall {
            event: "UserPromptSubmit".into(),
            cue: "open an easybuild pull request".into(),
            session: Some("seat-two".into()),
            shape: super::super::HookShape::Asks,
        };
        let (note, ids) = super::super::hook_note(&call, 5);
        assert!(note.starts_with("User rules"), "{note}");
        assert!(note.contains("never post or reply to Alan"), "{note}");
        assert!(ids.iter().all(|id| !id.starts_with("ur-")), "{ids:?}");
        let claude = super::super::hook_call(
            r#"{"hook_event_name":"UserPromptSubmit","session_id":"claude-1","prompt":"open an easybuild pull request"}"#,
        );
        let (claude_note, _) = super::super::hook_note(&claude, 5);
        let claude_out = super::super::hook_output(&claude, &claude_note);
        assert!(claude_out.contains("additionalContext"), "{claude_out}");
        assert!(
            claude_out.contains("never post or reply to Alan"),
            "{claude_out}"
        );
        let agy = super::super::hook_call_as(
            r#"{"conversationId":"agy-1","invocationNum":1,"prompt":"ship the easybuild change"}"#,
            Some("PreInvocation"),
        );
        assert_eq!(agy.event, "UserPromptSubmit");
        assert_eq!(agy.shape, super::super::HookShape::Steps);
        let (agy_note, _) = super::super::hook_note(&agy, 5);
        let agy_out = super::super::hook_output(&agy, &agy_note);
        assert!(agy_out.contains("injectSteps"), "{agy_out}");
        assert!(agy_out.contains("never post or reply to Alan"), "{agy_out}");
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path().join("other"));
        }
        assert!(preface().is_empty(), "a second config home has no rules");
    }

    #[test]
    fn a_user_rule_does_not_decay_after_simulated_time() {
        let (_lock, _dir) = isolated();
        let written = alan();
        let later = "2099-01-01T00:00:00Z";
        assert_eq!(sweep(later).unwrap(), 0);
        assert_eq!(review_due(later), 0);
        assert!(!is_due(&written, later));
        let kept = load().unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].text, written.text);
        assert!(kept[0].due_at.is_none());
        let raw = std::fs::read_to_string(rules_path().unwrap()).unwrap();
        assert!(!raw.contains("\"due_at\""), "{raw}");
        let mut file: File = serde_json::from_str(&raw).unwrap();
        file.rules.push(UserRule {
            id: "ur-expired".into(),
            text: "a row that was given a date".into(),
            author: "rohit".into(),
            reason: "fixture".into(),
            pattern: None,
            verdict: None,
            deny_mention: People::default(),
            deny_reply_to: People::default(),
            written_at: "2020-01-01T00:00:00Z".into(),
            updated_at: "2020-01-01T00:00:00Z".into(),
            deleted: false,
            from: None,
            due_at: Some("2020-01-02T00:00:00Z".into()),
        });
        std::fs::write(
            rules_path().unwrap(),
            serde_json::to_string_pretty(&file).unwrap(),
        )
        .unwrap();
        assert_eq!(sweep(later).unwrap(), 1);
        let kept = load().unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, written.id);
        assert_eq!(review_due(later), 0);
    }

    #[test]
    fn the_preface_stays_inside_its_budget_and_keeps_the_first_rule() {
        let (_lock, _dir) = isolated();
        let first = "a".repeat(80);
        write_preference(&first, "first", &People::default(), &People::default()).unwrap();
        for n in 0..30 {
            let text = format!("rule {n} {}", "b".repeat(200));
            write_preference(&text, "later", &People::default(), &People::default()).unwrap();
        }
        let head = preface();
        assert!(
            head.chars().count() <= PREFACE_BUDGET,
            "{}",
            head.chars().count()
        );
        assert!(head.contains(&first[..40]), "{head}");
        assert!(head.contains("omitted"), "{head}");
    }

    #[test]
    fn the_guard_denies_a_mention_and_a_reply_and_allows_another_comment() {
        let (_lock, _dir) = isolated();
        write_preference(
            "never post or reply to Alan (ocaisa)",
            "he asked not to be pinged",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        let mention = "gh pr comment 12 --body 'please ask @ocaisa'";
        assert!(
            super::super::static_guard(mention).is_none(),
            "prose alone is not a deny"
        );
        alan();
        let denied = super::super::static_guard(mention).expect("mention");
        assert_eq!(denied.verdict, "deny");
        assert!(denied.pattern.starts_with("user-rule:"), "{denied:?}");
        assert!(
            denied.reason.contains("ocaisa") || denied.reason.contains("Alan"),
            "{denied:?}"
        );
        let reply = "gh api --method POST repos/org/repo/issues/comments/7/replies -f body=seen -F in_reply_to_user=ocaisa";
        let denied = super::super::static_guard(reply).expect("reply");
        assert_eq!(denied.verdict, "deny");
        let allowed = "gh pr comment 12 --body 'tests passed'";
        assert!(
            super::super::static_guard(allowed).is_none(),
            "unrelated comment"
        );
        assert!(super::super::static_guard("gh pr comment 12 --body 'see ocaisa'").is_none());
        assert!(
            super::super::static_guard("gh pr comment 12 --body 'please ask @ocaisa2'").is_none()
        );
        assert!(
            super::super::static_guard("gh pr comment 12 --body 'alanine is a molecule'").is_none()
        );
        let named =
            super::super::static_guard("gh pr comment 12 --body 'ask Alan to look'").expect("name");
        assert_eq!(named.verdict, "deny");
        assert!(super::super::static_guard("gh api user/ocaisa").is_none());
        assert!(super::super::static_guard("echo please ask @ocaisa").is_none());
        let mcp = r#"{"hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"please ask @ocaisa","owner":"org","repo":"repo"}}"#;
        assert!(guard_payload(mcp).is_some());
        let mcp_reply = r#"{"tool_name":"add_reply_to_pull_request_comment","mcp_server_name":"github","tool_input":{"body":"seen","in_reply_to_user":"ocaisa"}}"#;
        assert!(guard_payload(mcp_reply).is_some());
        let mcp_ok = r#"{"tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"tests passed"}}"#;
        assert!(guard_payload(mcp_ok).is_none());
        let mcp_read = r#"{"tool_name":"list_issues","mcp_server_name":"github","tool_input":{"body":"@ocaisa"}}"#;
        assert!(guard_payload(mcp_read).is_none());
        let cursor = super::super::hook_call(
            r#"{"cursor_version":"2.4.0","conversation_id":"c-1","hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"please ask @ocaisa"}}"#,
        );
        let verdict = guard_payload(
            r#"{"cursor_version":"2.4.0","conversation_id":"c-1","hook_event_name":"beforeMCPExecution","tool_name":"add_issue_comment","mcp_server_name":"github","tool_input":{"body":"please ask @ocaisa"}}"#,
        )
        .unwrap();
        let out = super::super::hook_output_ruled(&cursor, "", Some(&verdict));
        assert!(out.contains("\"permission\":\"deny\""), "{out}");
    }

    #[test]
    fn promote_copies_a_lesson_and_refuses_anything_else() {
        let (_lock, _dir) = isolated();
        let atom = serde_json::json!({
            "id": "lesson-ocaisa",
            "kind": "lesson",
            "text": "Never post or reply to Alan (ocaisa).",
            "due_at": "2026-10-18T00:00:00Z"
        });
        let rule = promote_atom(
            &atom,
            "Alan asked that this hold for every seat",
            &People {
                handles: vec!["ocaisa".into()],
                names: vec!["Alan".into()],
            },
            &People {
                handles: vec!["ocaisa".into()],
                names: vec![],
            },
        )
        .unwrap();
        assert_eq!(rule.text, "Never post or reply to Alan (ocaisa).");
        assert_eq!(rule.author, "rohit");
        assert_eq!(rule.from.as_deref(), Some("lesson-ocaisa"));
        assert!(rule.due_at.is_none());
        assert_eq!(rule.deny_mention.handles, ["ocaisa"]);
        let raw = std::fs::read_to_string(rules_path().unwrap()).unwrap();
        assert!(!raw.contains("\"due_at\""), "{raw}");
        let preference = serde_json::json!({"id": "pref-1", "kind": "preference", "text": "no"});
        let err = promote_atom(
            &preference,
            "because",
            &People::default(),
            &People::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("only a lesson"), "{err}");
        let err = promote_atom(&atom, "  ", &People::default(), &People::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("reason"), "{err}");
    }

    #[test]
    fn an_argv_user_rule_denies_in_the_static_guard() {
        let (_lock, _dir) = isolated();
        write_argv(&Rule {
            pattern: "gh pr comment*".into(),
            verdict: "deny".into(),
            reason: "no pull request comments from this seat".into(),
        })
        .unwrap();
        let denied = super::super::static_guard("gh pr comment 12 --body 'tests passed'").unwrap();
        assert_eq!(denied.verdict, "deny");
        assert_eq!(denied.pattern, "gh pr comment*");
        assert!(super::super::static_guard("gh issue list").is_none());
    }

    fn git_ok(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} in {}: {}{}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
    }

    fn prepare_clone(dir: &std::path::Path) {
        git_ok(dir, &["config", "user.email", "seat@example.invalid"]);
        git_ok(dir, &["config", "user.name", "seat"]);
        git_ok(dir, &["config", "commit.gpgsign", "false"]);
        git_ok(dir, &["config", "core.hooksPath", "/dev/null"]);
    }

    struct Homes {
        _lock: super::super::EnvLock,
        _root: tempfile::TempDir,
        a: std::path::PathBuf,
        b: std::path::PathBuf,
        cfg_a: std::path::PathBuf,
        cfg_b: std::path::PathBuf,
    }

    impl Homes {
        fn new() -> Self {
            let lock = super::super::test_env_lock();
            let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/user-rule-homes");
            std::fs::create_dir_all(&base).unwrap();
            let root = tempfile::TempDir::new_in(&base).unwrap();
            let seed = root.path().join("seed");
            std::fs::create_dir_all(&seed).unwrap();
            git_ok(&seed, &["init", "-q", "-b", "main"]);
            prepare_clone(&seed);
            git_ok(&seed, &["commit", "--allow-empty", "-q", "-m", "init"]);
            let bare = root.path().join("bare.git");
            let laptop = root.path().join("laptop");
            let shared = root.path().join("shared");
            git_ok(
                root.path(),
                &[
                    "clone",
                    "--bare",
                    "-q",
                    seed.to_str().unwrap(),
                    bare.to_str().unwrap(),
                ],
            );
            git_ok(
                root.path(),
                &[
                    "clone",
                    "-q",
                    bare.to_str().unwrap(),
                    laptop.to_str().unwrap(),
                ],
            );
            git_ok(
                root.path(),
                &[
                    "clone",
                    "-q",
                    bare.to_str().unwrap(),
                    shared.to_str().unwrap(),
                ],
            );
            prepare_clone(&laptop);
            prepare_clone(&shared);
            let cfg_a = root.path().join("cfg-a");
            let cfg_b = root.path().join("cfg-b");
            unsafe {
                std::env::set_var("LJOS_USER_RULES_SYNC", "off");
                std::env::set_var("LJOS_HOLD", "rohit");
                std::env::set_var("PACKSET_URL", "off");
                std::env::set_var("HOME", root.path());
                std::env::remove_var("LJOS_USER_RULES_TRACKER");
            }
            Self {
                _lock: lock,
                _root: root,
                a: laptop,
                b: shared,
                cfg_a,
                cfg_b,
            }
        }

        fn home(&self, cfg: &std::path::Path) {
            unsafe { std::env::set_var("XDG_CONFIG_HOME", cfg) };
        }
    }

    fn exchange(tracker: &std::path::Path, host: &str) {
        let said = super::super::sync::exchange_user_rules(
            tracker,
            host,
            std::time::Duration::from_secs(8),
        )
        .unwrap_or_else(|err| panic!("sync {host}: {err:#}"));
        assert!(!said.contains("did not answer"), "{said}");
    }

    fn rewrite(id: &str, text: &str, author: &str, reason: &str, at: &str, deleted: bool) {
        let mut rows = read_all().unwrap();
        let rule = rows.iter_mut().find(|rule| rule.id == id).unwrap();
        rule.text = text.to_string();
        rule.author = author.to_string();
        rule.reason = reason.to_string();
        rule.updated_at = at.to_string();
        rule.deleted = deleted;
        store(&rows).unwrap();
    }

    #[test]
    fn two_homes_with_different_rules_converge() {
        let homes = Homes::new();
        homes.home(&homes.cfg_a);
        let alan = write_preference(
            "never post or reply to Alan (ocaisa)",
            "he asked not to be pinged",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        exchange(&homes.a, "laptop");
        homes.home(&homes.cfg_b);
        exchange(&homes.b, "shared");
        let got = load().unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].id, alan.id);
        assert_eq!(got[0].author, "rohit");
        assert_eq!(got[0].reason, "he asked not to be pinged");
        let credit = write_preference(
            "EasyBuild pull requests name eb-stack and gpt-oss-120b",
            "the credit line is part of the work",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        assert_eq!(credit.id, load().unwrap()[1].id);
        exchange(&homes.b, "shared");
        homes.home(&homes.cfg_a);
        exchange(&homes.a, "laptop");
        let got = load().unwrap();
        assert!(got.iter().any(|rule| rule.id == alan.id), "{got:?}");
        assert!(got.iter().any(|rule| rule.id == credit.id), "{got:?}");
        homes.home(&homes.cfg_b);
        let got = load().unwrap();
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got.iter().any(|rule| rule.id == alan.id));
        assert!(got.iter().any(|rule| rule.id == credit.id));
    }

    #[test]
    fn a_newer_edit_wins_and_a_tombstone_beats_an_older_edit() {
        let homes = Homes::new();
        homes.home(&homes.cfg_a);
        let rule = write_preference(
            "the standing line",
            "first reason",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        let id = rule.id.clone();
        exchange(&homes.a, "laptop");
        homes.home(&homes.cfg_b);
        exchange(&homes.b, "shared");

        rewrite(
            &id,
            "older edit",
            "ada",
            "because older",
            "2099-01-01T00:00:01.000Z",
            false,
        );
        exchange(&homes.b, "shared");
        homes.home(&homes.cfg_a);
        rewrite(
            &id,
            "newest edit",
            "rohit",
            "because newest",
            "2099-01-01T00:00:09.000Z",
            false,
        );
        exchange(&homes.a, "laptop");
        homes.home(&homes.cfg_b);
        exchange(&homes.b, "shared");
        let got = load().unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].id, id);
        assert_eq!(got[0].text, "newest edit");
        assert_eq!(got[0].author, "rohit");
        assert_eq!(got[0].reason, "because newest");
        homes.home(&homes.cfg_a);
        let got = load().unwrap();
        assert_eq!(got[0].text, "newest edit");
        assert_eq!(got[0].author, "rohit");
        assert_eq!(got[0].reason, "because newest");

        let removed = forget(&id).unwrap();
        assert!(removed.deleted);
        assert_eq!(removed.author, "rohit");
        assert_eq!(removed.reason, "because newest");
        rewrite(
            &id,
            "newest edit",
            "rohit",
            "because newest",
            "2099-01-02T00:00:00.000Z",
            true,
        );
        exchange(&homes.a, "laptop");
        homes.home(&homes.cfg_b);
        exchange(&homes.b, "shared");
        assert!(load().unwrap().is_empty(), "{:?}", load().unwrap());
        let tomb = read_all().unwrap();
        assert_eq!(tomb.len(), 1);
        assert!(tomb[0].deleted);
        assert_eq!(tomb[0].author, "rohit");
        assert_eq!(tomb[0].reason, "because newest");
        assert!(!preface().contains("newest edit"));
        rewrite(
            &id,
            "older edit",
            "ada",
            "because older",
            "2099-01-01T00:00:01.000Z",
            false,
        );
        exchange(&homes.b, "shared");
        assert!(
            load().unwrap().is_empty(),
            "a tombstone keeps the older edit out"
        );
        let tomb = read_all().unwrap();
        assert!(tomb[0].deleted);
        assert_eq!(tomb[0].updated_at, "2099-01-02T00:00:00.000Z");
    }

    #[test]
    fn the_hook_shows_local_rules_when_the_tracker_does_not_answer() {
        let (_lock, dir) = isolated();
        write_preference(
            "never post or reply to Alan (ocaisa)",
            "he asked not to be pinged",
            &People::default(),
            &People::default(),
        )
        .unwrap();
        let repo = dir.path().join("tracker");
        std::fs::create_dir_all(&repo).unwrap();
        git_ok(&repo, &["init", "-q", "-b", "main"]);
        prepare_clone(&repo);
        git_ok(&repo, &["commit", "--allow-empty", "-q", "-m", "init"]);
        git_ok(&repo, &["config", "http.lowSpeedLimit", "1"]);
        git_ok(&repo, &["config", "http.lowSpeedTime", "1"]);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 64];
            let _ = std::io::Read::read(&mut stream, &mut buf);
        });
        let url = format!("http://127.0.0.1:{port}/tracker.git");
        git_ok(&repo, &["remote", "add", "origin", &url]);
        unsafe {
            std::env::remove_var("LJOS_USER_RULES_SYNC");
            std::env::set_var("LJOS_USER_RULES_TRACKER", &repo);
            std::env::set_var("LJOS_USER_RULES_SYNC_MS", "400");
        }
        let started = std::time::Instant::now();
        let line = super::super::session_start_for(Some("seat-two"));
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "{elapsed:?}\n{line}"
        );
        assert!(line.starts_with("User rules"), "{line}");
        assert!(line.contains("never post or reply to Alan"), "{line}");
    }
}
