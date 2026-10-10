//! Mail from one seat to another.
//!
//! A message is a pack atom. Seats on one box share the pack and see it at
//! once. `ljos sync` exports the machine's own atoms, so the same atom
//! crosses machines in the sealed log. A receipt is its own atom: the pack
//! and the log are append-only, and an import tells two atoms apart by the
//! hash of their text, so the text of a message carries its id.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

/// How many messages a prompt is shown. The rest stay unread.
const PROMPT_CAP: usize = 8;

/// The first packset that keeps kinds `message`, `receipt` and `group`.
///
/// 0.12.1 refuses them. A new kind has been a minor release: `outcome`
/// arrived in 0.12.0, and `prediction` and `rule` in 0.6.0.
pub(crate) const MAIL_PACKSET: &str = "0.13.0";

/// What a probe post of an empty `message` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probe {
    /// The writer refused the kind.
    Refused,
    /// The kind passed and a later check refused the empty probe.
    Kept,
    /// The writer did not answer.
    Store,
}

/// Classify a probe error. Kind is checked before the text, so an empty
/// message is refused as unknown when the pack cannot keep mail, and as
/// missing text when it can.
fn probe_of(text: &str) -> Probe {
    if text.contains("unknown atom kind") {
        Probe::Refused
    } else if text.contains(": 400:") {
        Probe::Kept
    } else {
        Probe::Store
    }
}

fn unsupported_mail(version: Option<&str>) -> String {
    match version {
        Some(version) => format!(
            "this pack does not keep mail. packset {version} refused kind message. Mail needs packset {MAIL_PACKSET}."
        ),
        None => format!(
            "this pack does not keep mail. The writer refused kind message. Mail needs packset {MAIL_PACKSET}."
        ),
    }
}

/// One message as it is written.
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub id: String,
    pub from: String,
    pub to: Vec<String>,
    pub body: String,
    pub interrupt: bool,
    pub issue: Option<String>,
    pub group: Option<String>,
    pub reply: Option<String>,
    pub scope: Option<String>,
}

/// What `ljos send` was asked to write.
pub struct Send<'a> {
    pub seat: Option<&'a str>,
    pub group: Option<&'a str>,
    pub text: &'a str,
    pub interrupt: bool,
    pub issue: Option<&'a str>,
}

/// One message as a reader sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Letter {
    pub id: String,
    pub from: String,
    pub to: Vec<String>,
    pub body: String,
    pub interrupt: bool,
    pub issue: Option<String>,
    pub group: Option<String>,
    pub reply: Option<String>,
    pub scope: Option<String>,
    pub read: bool,
    ts: String,
    seq: usize,
}

/// A read receipt addressed to the seat that sent the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub msg: String,
    pub by: String,
    pub at: String,
    pub key: String,
}

fn mint_id(kind: &str, parts: &[&str]) -> String {
    static N: AtomicU64 = AtomicU64::new(1);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let raw = format!(
        "{kind}\n{n}\n{nanos}\n{}\n{}",
        std::process::id(),
        parts.join("\n")
    );
    super::work_id(&raw)
}

fn check_name(kind: &str, name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || name
            .chars()
            .any(|c| c.is_whitespace() || c == ',' || c == ':')
    {
        bail!("{kind}: {name:?} is not a name");
    }
    Ok(name.to_string())
}

fn tag(atom: &Value, prefix: &str) -> Option<String> {
    atom["entities"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .find_map(|e| e.strip_prefix(prefix).map(str::to_string))
}

fn tags(atom: &Value, prefix: &str) -> Vec<String> {
    atom["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|e| e.strip_prefix(prefix).map(str::to_string))
        .collect()
}

fn has(atom: &Value, entity: &str) -> bool {
    atom["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|e| e.as_str() == Some(entity))
}

/// The body a person wrote. The lines above the blank line are the id and
/// the envelope, which keep two messages with the same words distinct.
#[must_use]
pub fn body_of(text: &str) -> String {
    text.split_once("\n\n")
        .map(|(_, body)| body.trim().to_string())
        .unwrap_or_else(|| text.trim().to_string())
}

/// The text that travels. The id is in it, so an import that keys on the
/// text keeps two copies of the same words.
#[must_use]
pub fn message_text(m: &Outgoing) -> String {
    let mut head = format!("msg {}\nfrom {}\nto {}", m.id, m.from, m.to.join(", "));
    if m.interrupt {
        head.push_str("\ninterrupt");
    }
    if let Some(issue) = &m.issue {
        head.push_str(&format!("\nissue {issue}"));
    }
    if let Some(group) = &m.group {
        head.push_str(&format!("\ngroup {group}"));
    }
    if let Some(reply) = &m.reply {
        head.push_str(&format!("\nreply {reply}"));
    }
    format!("{head}\n\n{}", m.body.trim())
}

/// The atom a message posts. The entities are what an inbox filters on.
#[must_use]
pub fn message_atom(m: &Outgoing, workspace: &str) -> Value {
    let mut entities = vec![
        format!("seat:{}", m.from),
        format!("from:{}", m.from),
        format!("msg:{}", m.id),
    ];
    for to in &m.to {
        entities.push(format!("to:{to}"));
    }
    if m.interrupt {
        entities.push("priority:interrupt".to_string());
    }
    if let Some(issue) = &m.issue {
        entities.push(format!("issue:{issue}"));
    }
    if let Some(group) = &m.group {
        entities.push(format!("group:{group}"));
    }
    if let Some(reply) = &m.reply {
        entities.push(format!("reply:{reply}"));
    }
    if let Some(scope) = &m.scope {
        entities.push(format!("scope:{scope}"));
    }
    json!({
        "schema": "inside.atom/v1",
        "kind": "message",
        "level": "explicit",
        "text": message_text(m),
        "workspace": workspace,
        "entities": entities,
    })
}

fn letter_of(atom: &Value, seq: usize) -> Option<Letter> {
    if atom["kind"].as_str() != Some("message") {
        return None;
    }
    let id = tag(atom, "msg:")?;
    let from = tag(atom, "from:").or_else(|| tag(atom, "seat:"))?;
    let to = tags(atom, "to:");
    if to.is_empty() {
        return None;
    }
    Some(Letter {
        id,
        from,
        to,
        body: body_of(atom["text"].as_str().unwrap_or("")),
        interrupt: has(atom, "priority:interrupt"),
        issue: tag(atom, "issue:"),
        group: tag(atom, "group:"),
        reply: tag(atom, "reply:"),
        scope: tag(atom, "scope:"),
        read: false,
        ts: atom["ts"].as_str().unwrap_or("").to_string(),
        seq,
    })
}

fn readers_of(atoms: &[Value]) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for atom in atoms {
        if atom["kind"].as_str() != Some("receipt") {
            continue;
        }
        let Some(msg) = tag(atom, "receipt:") else {
            continue;
        };
        let Some(by) = tag(atom, "from:").or_else(|| tag(atom, "seat:")) else {
            continue;
        };
        out.insert((msg, by));
    }
    out
}

/// Messages addressed to `seat`, interrupts first, then oldest first.
#[must_use]
pub fn inbox_letters(atoms: &[Value], seat: &str, all: bool) -> Vec<Letter> {
    let read = readers_of(atoms);
    let mut letters: Vec<Letter> = atoms
        .iter()
        .enumerate()
        .filter_map(|(seq, atom)| letter_of(atom, seq))
        .filter(|letter| letter.to.iter().any(|to| to == seat))
        .map(|mut letter| {
            letter.read = read.contains(&(letter.id.clone(), seat.to_string()));
            letter
        })
        .filter(|letter| all || !letter.read)
        .collect();
    letters.sort_by(|a, b| {
        b.interrupt
            .cmp(&a.interrupt)
            .then(a.ts.cmp(&b.ts))
            .then(a.seq.cmp(&b.seq))
    });
    letters
}

/// Receipts on messages this seat sent, newest stamp last.
#[must_use]
pub fn receipts_for(atoms: &[Value], seat: &str) -> Vec<Receipt> {
    let mut out = Vec::new();
    for atom in atoms {
        if atom["kind"].as_str() != Some("receipt") {
            continue;
        }
        if tag(atom, "to:").as_deref() != Some(seat) {
            continue;
        }
        let Some(msg) = tag(atom, "receipt:") else {
            continue;
        };
        let Some(by) = tag(atom, "from:") else {
            continue;
        };
        let text = atom["text"].as_str().unwrap_or("");
        let at = text
            .lines()
            .find_map(|l| l.strip_prefix("at "))
            .unwrap_or("")
            .to_string();
        out.push(Receipt {
            key: super::work_id(text),
            msg,
            by,
            at,
        });
    }
    out.sort_by(|a, b| a.at.cmp(&b.at).then(a.key.cmp(&b.key)));
    out
}

/// Seats in a group, in the order they were added. A later drop removes one.
#[must_use]
pub fn members(atoms: &[Value], group: &str) -> Vec<String> {
    let mut events: Vec<(String, usize, String, bool)> = Vec::new();
    for (seq, atom) in atoms.iter().enumerate() {
        if atom["kind"].as_str() != Some("group") {
            continue;
        }
        if tag(atom, "group:").as_deref() != Some(group) {
            continue;
        }
        let ts = atom["ts"].as_str().unwrap_or("").to_string();
        if let Some(seat) = tag(atom, "member:") {
            events.push((ts, seq, seat, false));
        } else if let Some(seat) = tag(atom, "drop:") {
            events.push((ts, seq, seat, true));
        }
    }
    events.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<String> = Vec::new();
    for (_, _, seat, drop) in events {
        if drop {
            out.retain(|s| s != &seat);
        } else if !out.iter().any(|s| s == &seat) {
            out.push(seat);
        }
    }
    out
}

fn one_line(body: &str, limit: Option<usize>) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    match limit {
        Some(n) if flat.chars().count() > n => {
            let t: String = flat.chars().take(n.saturating_sub(3)).collect();
            format!("{t}...")
        }
        _ => flat,
    }
}

fn flags(letter: &Letter) -> String {
    let mut parts = Vec::new();
    if letter.interrupt {
        parts.push("interrupt".to_string());
    }
    if let Some(issue) = &letter.issue {
        parts.push(format!("issue:{issue}"));
    }
    if let Some(group) = &letter.group {
        parts.push(format!("group:{group}"));
    }
    if let Some(reply) = &letter.reply {
        parts.push(format!("reply:{reply}"));
    }
    if letter.read {
        parts.push("read".to_string());
    }
    parts.join(" ")
}

/// The inbox a seat prints. Receipts already shown are left out.
#[must_use]
pub fn render_inbox(
    atoms: &[Value],
    seat: &str,
    all: bool,
    seen_receipts: &BTreeSet<String>,
) -> (String, Vec<String>) {
    let letters = inbox_letters(atoms, seat, all);
    let receipts: Vec<Receipt> = receipts_for(atoms, seat)
        .into_iter()
        .filter(|r| !seen_receipts.contains(&r.key))
        .collect();
    if letters.is_empty() && receipts.is_empty() {
        let line = if all {
            "inbox: nothing\n"
        } else {
            "inbox: nothing unread\n"
        };
        return (line.to_string(), Vec::new());
    }
    let mut out = String::new();
    for letter in &letters {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            letter.id,
            letter.from,
            flags(letter),
            one_line(&letter.body, None)
        ));
    }
    let mut keys = Vec::new();
    for receipt in &receipts {
        out.push_str(&format!(
            "receipt\t{}\t{}\t{}\n",
            receipt.msg, receipt.by, receipt.at
        ));
        keys.push(receipt.key.clone());
    }
    (out, keys)
}

/// The note a prompt hook prints, and the ids to receipt when it is delivered.
///
/// Message ids are prefixed `mail:`. Receipt ids the sender is being told
/// about are prefixed `rcpt:` and are recorded locally, not written again.
#[must_use]
pub fn prompt_lines(letters: &[Letter], receipts: &[Receipt]) -> (String, Vec<String>) {
    if letters.is_empty() && receipts.is_empty() {
        return (String::new(), Vec::new());
    }
    let mut lines = vec!["mail:".to_string()];
    let mut ids = Vec::new();
    for letter in letters.iter().take(PROMPT_CAP) {
        let mark = if letter.interrupt { "! " } else { "" };
        let on = letter
            .issue
            .as_ref()
            .map(|issue| format!(" on {issue}"))
            .unwrap_or_default();
        lines.push(format!(
            "{mark}{} from {}{on}: {}",
            letter.id,
            letter.from,
            one_line(&letter.body, Some(160))
        ));
        ids.push(format!("mail:{}", letter.id));
    }
    let rest = letters.len().saturating_sub(PROMPT_CAP);
    if rest > 0 {
        lines.push(format!("{rest} more in ljos inbox"));
    }
    for receipt in receipts.iter().take(PROMPT_CAP) {
        lines.push(format!("read {} by {}", receipt.msg, receipt.by));
        ids.push(format!("rcpt:{}", receipt.key));
    }
    (lines.join("\n"), ids)
}

fn find_letter(atoms: &[Value], id: &str) -> Result<Letter> {
    atoms
        .iter()
        .enumerate()
        .filter_map(|(seq, atom)| letter_of(atom, seq))
        .find(|letter| letter.id == id)
        .ok_or_else(|| anyhow::anyhow!("no message {id}"))
}

/// Who a send addresses. A group send leaves the sender off the list.
pub fn recipients(
    from: &str,
    seat: Option<&str>,
    group: Option<&str>,
    members: &[String],
) -> Result<Vec<String>> {
    if let Some(group) = group {
        if seat.is_some() {
            bail!("send: name a seat or a group");
        }
        let to: Vec<String> = members
            .iter()
            .filter(|m| m.as_str() != from)
            .cloned()
            .collect();
        if to.is_empty() {
            bail!("send: {group} has no other member");
        }
        return Ok(to);
    }
    let Some(seat) = seat.map(str::trim).filter(|s| !s.is_empty()) else {
        bail!("send: name a seat or a group");
    };
    let seat = check_name("send", seat)?;
    if seat == from {
        bail!("send: {seat} is this seat");
    }
    Ok(vec![seat])
}

fn is_mail_atom(atom: &Value) -> bool {
    matches!(atom["kind"].as_str(), Some("message" | "receipt" | "group"))
}

fn writer_version(client: &packset_client::PacksetClient) -> Option<String> {
    client
        .status(Some(&client.workspace()))
        .ok()
        .and_then(|status| {
            status
                .get("version")
                .and_then(Value::as_str)
                .filter(|version| !version.is_empty())
                .map(str::to_string)
        })
}

/// Fail when this writer does not keep mail. An empty message is not
/// stored: the kind is refused, or the text is.
fn ensure_mail(client: &packset_client::PacksetClient, verb: &str) -> Result<()> {
    let probe = json!({
        "schema": "inside.atom/v1",
        "kind": "message",
        "level": "explicit",
        "text": "",
        "workspace": workspace_of(Some(&client.workspace())),
    });
    let posted = super::with_writer(|| client.post_atom(&probe).map_err(anyhow::Error::from));
    match posted {
        Ok(_) => Ok(()),
        Err(err) => match probe_of(&format!("{err:#}")) {
            Probe::Kept => Ok(()),
            Probe::Refused => {
                let version = writer_version(client);
                bail!("{verb}: {}", unsupported_mail(version.as_deref()));
            }
            Probe::Store => Err(err).context("mail: the pack did not answer"),
        },
    }
}

fn listed() -> Result<Vec<Value>> {
    let client = super::pack()?;
    let workspace = client.workspace();
    super::atoms_lean(&client, &workspace).context("mail: the pack did not answer")
}

fn load(verb: &str) -> Result<Vec<Value>> {
    let client = super::pack()?;
    let atoms = listed()?;
    if !atoms.iter().any(is_mail_atom) {
        ensure_mail(&client, verb)?;
    }
    Ok(atoms)
}

fn load_quiet() -> Option<Vec<Value>> {
    let client = super::pack().ok()?;
    let workspace = client.workspace();
    super::atoms_lean(&client, &workspace).ok()
}

fn post(atom: &Value, verb: &str) -> Result<Value> {
    let client = super::pack()?;
    super::with_writer(|| {
        client
            .post_atom(atom)
            .with_context(|| format!("{verb}: POST /v1/atoms failed"))
    })
}

fn workspace_of(client_workspace: Option<&str>) -> String {
    client_workspace.unwrap_or("seat").to_string()
}

fn stamp_source(atom: &mut Value) {
    atom["source"] = super::atom_source();
}

/// Write one message and print its id.
pub fn send(args: &Send<'_>) -> Result<String> {
    let text = args.text.trim();
    if text.is_empty() {
        bail!("send: empty text");
    }
    let from = super::seat_name();
    let group = args
        .group
        .map(|name| check_name("send", name))
        .transpose()?;
    let client = super::pack()?;
    // A named seat is checked before the pack, so "this seat" stays that
    // error. A group has no roster until the pack answers.
    let atoms = if group.is_some() {
        ensure_mail(&client, "send")?;
        listed()?
    } else {
        Vec::new()
    };
    let roster = group
        .as_deref()
        .map(|name| members(&atoms, name))
        .unwrap_or_default();
    let to = recipients(from.as_str(), args.seat, group.as_deref(), &roster)?;
    if group.is_none() {
        ensure_mail(&client, "send")?;
    }
    let issue = args
        .issue
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| check_name("send", s))
        .transpose()?;
    let scope = issue.as_deref().and_then(super::sync::scope_for_issue);
    let outgoing = Outgoing {
        id: mint_id("msg", &[&from, text]),
        from,
        to: to.clone(),
        body: text.to_string(),
        interrupt: args.interrupt,
        issue,
        group,
        reply: None,
        scope,
    };
    let mut atom = message_atom(&outgoing, &workspace_of(Some(&client.workspace())));
    stamp_source(&mut atom);
    post(&atom, "send")?;
    Ok(format!("sent {} to {}\n", outgoing.id, to.join(", ")))
}

/// Unread mail for this seat, then receipts on what it sent that it has not
/// been shown yet.
///
/// Listing does not write a receipt. `ljos hook --prompt` does, for each
/// message it shows. A pack that does not answer, or that refuses mail, is
/// an error. `ljos_inbox` calls this.
pub fn inbox(all: bool) -> Result<String> {
    let atoms = load("inbox")?;
    let seat = super::seat_name();
    let seen = receipt_seen();
    let (text, keys) = render_inbox(&atoms, &seat, all, &seen);
    remember_receipts(&keys);
    Ok(text)
}

fn already_read(atoms: &[Value], msg: &str, seat: &str) -> bool {
    readers_of(atoms).contains(&(msg.to_string(), seat.to_string()))
}

fn receipt_atom(msg: &Letter, reader: &str, at: &str, workspace: &str) -> Value {
    let id = mint_id("receipt", &[msg.id.as_str(), reader, at]);
    let text = format!("read {}\nby {reader}\nat {at}\nmsg {id}", msg.id);
    let mut entities = vec![
        format!("seat:{reader}"),
        format!("from:{reader}"),
        format!("to:{}", msg.from),
        format!("msg:{id}"),
        format!("receipt:{}", msg.id),
    ];
    if let Some(scope) = &msg.scope {
        entities.push(format!("scope:{scope}"));
    }
    json!({
        "schema": "inside.atom/v1",
        "kind": "receipt",
        "level": "explicit",
        "text": text,
        "workspace": workspace,
        "entities": entities,
    })
}

/// Write a receipt for one message. A second read of the same message
/// writes nothing.
pub fn read(id: &str) -> Result<String> {
    let id = id.trim();
    if id.is_empty() {
        bail!("read: name a message");
    }
    let atoms = load("read")?;
    let seat = super::seat_name();
    let letter = find_letter(&atoms, id).context("read")?;
    if !letter.to.iter().any(|to| to == &seat) {
        bail!("read: {id} is not for {seat}");
    }
    if already_read(&atoms, id, &seat) {
        return Ok(format!("read {id} already\n"));
    }
    let client = super::pack()?;
    let mut atom = receipt_atom(
        &letter,
        &seat,
        &super::now_utc(),
        &workspace_of(Some(&client.workspace())),
    );
    stamp_source(&mut atom);
    post(&atom, "read")?;
    Ok(format!("read {id}\n"))
}

fn read_ids(ids: &[String]) {
    let Some(atoms) = load_quiet() else {
        return;
    };
    let seat = super::seat_name();
    let Ok(client) = super::pack() else {
        return;
    };
    let workspace = client.workspace();
    let at = super::now_utc();
    for id in ids {
        let Ok(letter) = find_letter(&atoms, id) else {
            continue;
        };
        if !letter.to.iter().any(|to| to == &seat) || already_read(&atoms, id, &seat) {
            continue;
        }
        let mut atom = receipt_atom(&letter, &seat, &at, &workspace_of(Some(&workspace)));
        stamp_source(&mut atom);
        let _ = post(&atom, "read");
    }
}

/// Answer a message. The reply goes to its sender and keeps its issue.
pub fn reply(id: &str, text: &str, interrupt: bool) -> Result<String> {
    let id = id.trim();
    let text = text.trim();
    if id.is_empty() {
        bail!("reply: name a message");
    }
    if text.is_empty() {
        bail!("reply: empty text");
    }
    let atoms = load("reply")?;
    let parent = find_letter(&atoms, id).context("reply")?;
    let from = super::seat_name();
    if parent.from == from {
        bail!("reply: {id} is from this seat");
    }
    let issue = parent.issue.clone();
    let scope = issue
        .as_deref()
        .and_then(super::sync::scope_for_issue)
        .or(parent.scope);
    let outgoing = Outgoing {
        id: mint_id("msg", &[&from, id, text]),
        from,
        to: vec![parent.from.clone()],
        body: text.to_string(),
        interrupt,
        issue,
        group: None,
        reply: Some(parent.id.clone()),
        scope,
    };
    let client = super::pack()?;
    let mut atom = message_atom(&outgoing, &workspace_of(Some(&client.workspace())));
    stamp_source(&mut atom);
    post(&atom, "reply")?;
    let mut line = format!(
        "sent {} to {}\nreply {}\n",
        outgoing.id, parent.from, parent.id
    );
    if let Some(issue) = &outgoing.issue {
        line.push_str(&format!("issue {issue}\n"));
    }
    Ok(line)
}

fn group_atom(name: &str, seat: &str, drop: bool, by: &str, workspace: &str) -> Value {
    let at = super::now_utc();
    let id = mint_id("group", &[name, seat, by, &at]);
    let verb = if drop { "drop" } else { "member" };
    let text = format!("group {id}\nname {name}\n{verb} {seat}\nby {by}\nat {at}");
    let mut entities = vec![
        format!("seat:{by}"),
        format!("group:{name}"),
        format!("msg:{id}"),
    ];
    if drop {
        entities.push(format!("drop:{seat}"));
    } else {
        entities.push(format!("member:{seat}"));
    }
    json!({
        "schema": "inside.atom/v1",
        "kind": "group",
        "level": "explicit",
        "text": text,
        "workspace": workspace,
        "entities": entities,
    })
}

/// List a group, or add and remove members. A drop is its own atom.
pub fn group(name: &str, add: &[String], remove: &[String]) -> Result<String> {
    let name = check_name("group", name)?;
    if add.is_empty() && remove.is_empty() {
        let atoms = load("group")?;
        let seats = members(&atoms, &name);
        if seats.is_empty() {
            return Ok(format!("{name}\t(no members)\n"));
        }
        let mut out = String::new();
        for seat in seats {
            out.push_str(&format!("{name}\t{seat}\n"));
        }
        return Ok(out);
    }
    let atoms = load("group")?;
    let by = super::seat_name();
    let client = super::pack()?;
    let workspace = client.workspace();
    let mut roster = members(&atoms, &name);
    let mut out = String::new();
    for seat in add {
        let seat = check_name("group", seat)?;
        if roster.iter().any(|s| s == &seat) {
            out.push_str(&format!("{seat} is already in {name}\n"));
            continue;
        }
        let mut atom = group_atom(&name, &seat, false, &by, &workspace_of(Some(&workspace)));
        stamp_source(&mut atom);
        post(&atom, "group")?;
        roster.push(seat.clone());
        out.push_str(&format!("added {seat} to {name}\n"));
    }
    for seat in remove {
        let seat = check_name("group", seat)?;
        if !roster.iter().any(|s| s == &seat) {
            out.push_str(&format!("{seat} is not in {name}\n"));
            continue;
        }
        let mut atom = group_atom(&name, &seat, true, &by, &workspace_of(Some(&workspace)));
        stamp_source(&mut atom);
        post(&atom, "group")?;
        roster.retain(|s| s != &seat);
        out.push_str(&format!("removed {seat} from {name}\n"));
    }
    Ok(out)
}

fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
        .join("ljos")
        .join("mail")
}

fn receipt_seen_path() -> PathBuf {
    state_dir().join("receipts-seen")
}

fn receipt_seen() -> BTreeSet<String> {
    std::fs::read_to_string(receipt_seen_path())
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn remember_receipts(keys: &[String]) {
    if keys.is_empty() {
        return;
    }
    let path = receipt_seen_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    for key in keys {
        if text.lines().any(|l| l == key) {
            continue;
        }
        text.push_str(key);
        text.push('\n');
    }
    let _ = std::fs::write(path, text);
}

/// What the prompt hook prints when the pack does not answer.
///
/// One line. The hook still exits 0, so the prompt is not blocked.
pub const MAIL_UNCHECKED: &str = "mail could not be checked";

/// Unread mail and unseen receipts for the prompt hook. A pack that does
/// not answer leaves one line, so unread mail is not silent.
#[must_use]
pub fn prompt_note() -> (String, Vec<String>) {
    let Some(atoms) = load_quiet() else {
        return (MAIL_UNCHECKED.to_string(), Vec::new());
    };
    let seat = super::seat_name();
    let letters = inbox_letters(&atoms, &seat, false);
    let seen = receipt_seen();
    let receipts: Vec<Receipt> = receipts_for(&atoms, &seat)
        .into_iter()
        .filter(|r| !seen.contains(&r.key))
        .collect();
    prompt_lines(&letters, &receipts)
}

/// Receipts for messages a runner was just shown, and a local mark for
/// receipts this seat was told about. A pack that does not answer leaves
/// the message unread, so the next prompt shows it again.
pub fn ack_delivered(ids: &[String]) {
    let mut reads = Vec::new();
    let mut seen = Vec::new();
    for id in ids {
        if let Some(msg) = id.strip_prefix("mail:") {
            reads.push(msg.to_string());
        } else if let Some(key) = id.strip_prefix("rcpt:") {
            seen.push(key.to_string());
        }
    }
    if !reads.is_empty() {
        read_ids(&reads);
    }
    remember_receipts(&seen);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outgoing(id: &str, from: &str, to: &[&str], body: &str) -> Outgoing {
        Outgoing {
            id: id.to_string(),
            from: from.to_string(),
            to: to.iter().map(|s| (*s).to_string()).collect(),
            body: body.to_string(),
            interrupt: false,
            issue: None,
            group: None,
            reply: None,
            scope: None,
        }
    }

    fn atom(m: &Outgoing, ts: &str) -> Value {
        let mut v = message_atom(m, "seat");
        v["ts"] = json!(ts);
        v
    }

    #[test]
    fn the_same_words_stay_two_messages() {
        let a = outgoing("a", "alice", &["bob"], "ok");
        let b = outgoing("b", "carol", &["bob"], "ok");
        let left = message_text(&a);
        let right = message_text(&b);
        assert_ne!(left, right);
        assert_ne!(super::super::work_id(&left), super::super::work_id(&right));
        assert_eq!(body_of(&left), "ok");
        let atoms = [
            atom(&a, "2026-10-09T00:00:00Z"),
            atom(&b, "2026-10-09T00:00:01Z"),
        ];
        let plain = super::super::sync::log_plaintext(&atoms, "seat", "seat");
        assert_eq!(plain.lines().count(), 2, "{plain}");
        let got = super::super::sync::incoming(&plain, "other", &BTreeSet::new(), &BTreeSet::new());
        assert_eq!(got.post.len(), 2);
    }

    #[test]
    fn inbox_is_the_seat_named_and_an_interrupt_leads() {
        let mut late = outgoing("late", "alice", &["bob"], "when you can");
        late.issue = Some("acme-4".into());
        let mut hot = outgoing("hot", "carol", &["bob"], "the build is red");
        hot.interrupt = true;
        hot.issue = Some("acme-4".into());
        let other = outgoing("other", "alice", &["dave"], "not for bob");
        let atoms = [
            atom(&late, "2026-10-09T00:00:00Z"),
            atom(&hot, "2026-10-09T00:00:02Z"),
            atom(&other, "2026-10-09T00:00:01Z"),
        ];
        let letters = inbox_letters(&atoms, "bob", false);
        assert_eq!(
            letters.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            ["hot", "late"]
        );
        assert!(letters[0].interrupt);
        assert_eq!(letters[1].issue.as_deref(), Some("acme-4"));
        let (text, ids) = prompt_lines(&letters, &[]);
        assert!(
            text.starts_with("mail:\n! hot from carol on acme-4:"),
            "{text}"
        );
        assert!(ids.contains(&"mail:hot".to_string()), "{ids:?}");
        assert!(text.contains("late from alice on acme-4:"));
    }

    #[test]
    fn a_receipt_takes_the_message_out_of_the_unread_list() {
        let msg = outgoing("m1", "alice", &["bob"], "look at the log");
        let message = atom(&msg, "2026-10-09T00:00:00Z");
        let receipt = json!({
            "kind": "receipt",
            "text": "read m1\nby bob\nat 2026-10-09T00:01:00Z\nmsg r1",
            "ts": "2026-10-09T00:01:00Z",
            "entities": ["seat:bob", "from:bob", "to:alice", "receipt:m1", "msg:r1"],
        });
        let held = [message, receipt];
        let unread = inbox_letters(&held, "bob", false);
        assert!(unread.is_empty());
        let all = inbox_letters(&held, "bob", true);
        assert!(all[0].read);
        let (text, keys) = render_inbox(&held, "alice", false, &BTreeSet::new());
        assert!(
            text.contains("receipt\tm1\tbob\t2026-10-09T00:01:00Z"),
            "{text}"
        );
        assert_eq!(keys.len(), 1);
        let (again, _) = render_inbox(&held, "alice", false, &keys.into_iter().collect());
        assert_eq!(again, "inbox: nothing unread\n");
    }

    #[test]
    fn a_reply_keeps_the_issue_and_names_the_sender() {
        let mut parent = outgoing("p", "alice", &["bob"], "the build is red");
        parent.issue = Some("acme-4".into());
        parent.scope = Some("tools".into());
        let letter = letter_of(&atom(&parent, "2026-10-09T00:00:00Z"), 0).unwrap();
        let reply = Outgoing {
            id: "r".into(),
            from: "bob".into(),
            to: vec![letter.from.clone()],
            body: "looking".into(),
            interrupt: false,
            issue: letter.issue.clone(),
            group: None,
            reply: Some(letter.id.clone()),
            scope: letter.scope.clone(),
        };
        let atom = message_atom(&reply, "seat");
        let back = letter_of(&atom, 1).unwrap();
        assert_eq!(back.to, ["alice"]);
        assert_eq!(back.reply.as_deref(), Some("p"));
        assert_eq!(back.issue.as_deref(), Some("acme-4"));
        assert_eq!(back.scope.as_deref(), Some("tools"));
        assert!(super::super::sync::is_own(&atom));
    }

    #[test]
    fn a_group_send_reaches_the_other_members() {
        let atoms = [
            json!({
                "kind": "group",
                "ts": "2026-10-09T00:00:00Z",
                "entities": ["group:ops", "member:alice"],
            }),
            json!({
                "kind": "group",
                "ts": "2026-10-09T00:00:01Z",
                "entities": ["group:ops", "member:bob"],
            }),
            json!({
                "kind": "group",
                "ts": "2026-10-09T00:00:02Z",
                "entities": ["group:ops", "member:carol"],
            }),
            json!({
                "kind": "group",
                "ts": "2026-10-09T00:00:03Z",
                "entities": ["group:ops", "drop:carol"],
            }),
        ];
        assert_eq!(members(&atoms, "ops"), ["alice", "bob"]);
        let to = recipients("alice", None, Some("ops"), &members(&atoms, "ops")).unwrap();
        assert_eq!(to, ["bob"]);
        let err = recipients("alice", None, Some("ops"), &["alice".into()]).unwrap_err();
        assert!(err.to_string().contains("no other member"), "{err}");
        let err = recipients("alice", Some("alice"), None, &[]).unwrap_err();
        assert!(err.to_string().contains("this seat"), "{err}");
    }

    #[test]
    fn a_prompt_caps_the_note_and_keeps_the_rest_unread() {
        let letters: Vec<Letter> = (0..10)
            .map(|n| Letter {
                id: format!("m{n}"),
                from: "alice".into(),
                to: vec!["bob".into()],
                body: format!("line {n}"),
                interrupt: n == 9,
                issue: None,
                group: None,
                reply: None,
                scope: None,
                read: false,
                ts: format!("2026-10-09T00:00:{n:02}Z"),
                seq: n,
            })
            .collect();
        let mut ordered = letters.clone();
        ordered.sort_by(|a, b| b.interrupt.cmp(&a.interrupt).then(a.ts.cmp(&b.ts)));
        let (text, ids) = prompt_lines(&ordered, &[]);
        assert!(text.lines().next().unwrap() == "mail:");
        assert!(text.contains("! m9 from alice:"));
        assert!(text.contains("2 more in ljos inbox"), "{text}");
        assert_eq!(ids.iter().filter(|id| id.starts_with("mail:")).count(), 8);
        assert!(!ids.iter().any(|id| id == "mail:m7" || id == "mail:m8"));
    }

    #[test]
    fn an_unknown_kind_is_a_pack_without_mail() {
        let refused =
            "bad response: http://127.0.0.1:8761/v1/atoms: 400: unknown atom kind: message";
        assert_eq!(probe_of(refused), Probe::Refused);
        let kept = "bad response: http://127.0.0.1:8761/v1/atoms: 400: atom text is required";
        assert_eq!(probe_of(kept), Probe::Kept);
        let down = "http://127.0.0.1:8761/v1/atoms: http: connection refused";
        assert_eq!(probe_of(down), Probe::Store);
        let named = unsupported_mail(Some("0.12.1"));
        assert!(named.contains("packset 0.12.1"), "{named}");
        assert!(named.contains(MAIL_PACKSET), "{named}");
        assert!(named.contains("kind message"), "{named}");
        let quiet = unsupported_mail(None);
        assert!(quiet.contains(MAIL_PACKSET), "{quiet}");
        assert!(quiet.contains("refused kind message"), "{quiet}");
    }
}
