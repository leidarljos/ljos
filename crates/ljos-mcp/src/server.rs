//! The seat as an agent surface.
//!
//! The command line's verbs, none owned here: a pack write is an HTTP
//! call, everything else execs the habitat's own binary and hands back what
//! it said. Every tool says whether it writes; a habitat that refused is an
//! error; the cards are read-only resources under `ljos://cards/`; the
//! sequences that cross habitats are prompts. The pack is written by
//! Remember, Prefer and Forget, and the text is the claim.

use std::path::{Path, PathBuf};

use ljos_cli::{
    age_of, announce_seat, ballots_from_json, brief, calibrate, cards, claim, complete, conflicts,
    consensus_steps_for, doctor, due, finish, format_change, format_consolidation, graded, habit,
    habits, handover, identity_or_seat, island_entities, issue_words, learn_and_write, now_utc,
    on_path, other_seat, packset_consolidate, packset_forget, packset_island_as,
    packset_search_as_of, packset_write_as, parse_every, personas_from_pack, personas_speaking_to,
    policy_line, receive, release, resolve_assignee, rows_about, run_captured, runner_pid,
    seat_name, sitting, timeline, topic_words, trust_from_pack, write_persona, write_prediction,
    write_rule, write_trust, Persona, Rule, Trust, CARD_NAMES, LEARN_BETA, POLICY_TCB, PROTOCOL,
};
use rmcp::{
    handler::server::wrapper::Json, handler::server::wrapper::Parameters,
    handler::server::ServerHandler, model::*, prompt, prompt_handler, prompt_router, tool,
    tool_handler, tool_router, ErrorData as McpError,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The scheme the cards are addressable under.
const SCHEME: &str = "ljos";
/// Where the sitting protocol is read from.
const PROTOCOL_URI: &str = "ljos://protocol";

#[derive(Clone)]
pub struct LjosServer {
    /// Where the cards are read from. The command line's `--dir`.
    cards_dir: PathBuf,
}

// ---- arguments -------------------------------------------------------------

/// One explicit claim.
#[derive(Deserialize, JsonSchema)]
pub struct ClaimArgs {
    /// The claim, as it will be stored. Two short sentences at most; the pack
    /// refuses more. Not a transcript, not a summary of one.
    pub text: String,
    /// Remember as this persona: the claim comes back to it first in its
    /// next brief. Absent, the seat's own.
    #[serde(rename = "as")]
    pub as_persona: Option<String>,
}

/// A question for the pack.
#[derive(Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// What to ask the seat's standing knowledge.
    pub query: String,
    /// Ask the pack as it stood at this time (`YYYY-MM-DD` or RFC 3339):
    /// what the seat knew then, memories withdrawn since included, memories
    /// learnt since left out. Omit for now.
    #[serde(default)]
    pub as_of: Option<String>,
}

/// A reading of a habit, or the question which readings stand.
#[derive(Deserialize, JsonSchema)]
pub struct HabitArgs {
    /// The habit's name (`mab cr all`, `hook p50 10k`). Absent, every habit.
    #[serde(default)]
    pub name: Option<String>,
    /// The reading. With it the tool writes; without it the tool lists.
    #[serde(default)]
    pub value: Option<f64>,
    /// The unit the reading is in, for the reader.
    #[serde(default)]
    pub unit: Option<String>,
    /// How often a reading is taken: `7d`, `24h`, `2w`. Absent, a week.
    #[serde(default)]
    pub every: Option<String>,
    /// Where the reading came from: a job id, a run, a file.
    #[serde(default)]
    pub source: Option<String>,
}

/// One habit as it stands.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ReadingRow {
    pub name: String,
    pub value: f64,
    pub unit: String,
    /// The change since the reading before, in words; `first reading` when none.
    pub change: String,
    /// When this reading was taken, in words.
    pub age: String,
    /// When the next reading is due, RFC 3339.
    pub due_at: Option<String>,
    pub source: String,
    pub id: Option<String>,
}

/// How many rows to print.
#[derive(Deserialize, JsonSchema)]
pub struct LimitArgs {
    /// Most rows; twelve when absent.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Whether a consolidation writes.
#[derive(Deserialize, JsonSchema)]
pub struct ConsolidateArgs {
    /// Write the closures. Absent or false: report the pairs, change nothing.
    #[serde(default)]
    pub apply: Option<bool>,
}

/// One atom to retire, and what withdrew it.
#[derive(Deserialize, JsonSchema)]
pub struct ForgetArgs {
    /// The atom's id, as `ljos_search` returned it. Not its text.
    pub id: String,
    /// The deed accession the retraction stands on, when the work minted one.
    /// Free text is refused: the point of writing it is that `ljos_evidence`
    /// can be asked about it later.
    #[serde(default)]
    pub why: Option<String>,
}

/// One deed accession.
#[derive(Deserialize, JsonSchema)]
pub struct AccessionArgs {
    /// `deed-<kind>-<slug>`, or `sha256:` of the deed or of a product path.
    pub accession: String,
}

/// A tracker node, and optionally a deed to cite on it.
#[derive(Deserialize, JsonSchema)]
pub struct DeedArgs {
    /// The issue id.
    pub issue: String,
    /// An accession to cite. Absent, the tool lists what the issue cites.
    pub add: Option<String>,
}

/// A tracker node.
#[derive(Deserialize, JsonSchema)]
pub struct IssueArgs {
    /// The issue id.
    pub issue: String,
}

/// A forecast of the others' ballots.
#[derive(Deserialize, JsonSchema)]
pub struct PredictArgs {
    /// The issue id.
    pub issue: String,
    /// The option you expect to win, or an object of option to share.
    pub expect: serde_json::Value,
    /// Forecast as this persona; absent, the seat's identity.
    #[serde(rename = "as")]
    pub as_persona: Option<String>,
}

/// A rule: argv law kept in the pack.
#[derive(Deserialize, JsonSchema)]
pub struct RuleArgs {
    /// A glob over the whole command line, such as `*--force*`.
    pub pattern: String,
    /// `deny` stops the action at the runner; `ask` hands it to the person.
    pub verdict: String,
    /// The reason a stopped reader sees.
    pub why: String,
}

/// A ballot, or a request for the tally.
#[derive(Deserialize, JsonSchema)]
pub struct VoteArgs {
    /// The issue id.
    pub issue: String,
    /// `accept` or `reject`. Absent, the tool shows the tally.
    pub choice: Option<String>,
    /// Cast as this persona (a name written with `ljos_persona`) instead of
    /// the seat's own identity.
    #[serde(rename = "as")]
    pub as_persona: Option<String>,
}

/// A persona and the issue it will read.
#[derive(Deserialize, JsonSchema)]
pub struct BriefArgs {
    /// The persona's name, as written with `ljos_persona`.
    pub name: String,
    /// The tracker id of the issue.
    pub issue: String,
}

/// A voter with a view.
#[derive(Deserialize, JsonSchema)]
pub struct PersonaArgs {
    /// The persona's name; ballots cast as it carry this name.
    pub name: String,
    /// How far it moves off its ballot in a settle, in [0, 1]: 0 never
    /// moves, 1 is a plain voter. Half when absent.
    pub anchor: Option<f64>,
    /// How this persona reads the work, in a sentence or two.
    pub view: String,
    /// Domains it speaks to; a trust row scoped to one applies when the
    /// issue is about it.
    #[serde(default)]
    pub about: Vec<String>,
}

/// Rows a tool answers with. The protocol wants a structured result to be
/// an object, so a list comes back under one key.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Rows<T: JsonSchema> {
    /// The rows, in the order the tool ranks them.
    pub rows: Vec<T>,
}

/// Wrap a list as the object a structured result has to be.
fn rows<T: JsonSchema>(rows: Vec<T>) -> Json<Rows<T>> {
    Json(Rows { rows })
}

/// One persona of the roster.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PersonaRow {
    /// The name its ballots carry.
    pub name: String,
    /// How far it moves off its ballot in a settle, in [0, 1].
    pub anchor: f64,
    /// The domains it speaks to; empty speaks to every issue.
    pub about: Vec<String>,
    /// How it reads the work.
    pub view: String,
}

/// A session node to take or hand back.
#[derive(Deserialize, JsonSchema)]
pub struct TakeArgs {
    /// The tracker id of the issue (`proj-1a2b`), or a 32-hex claim-graph id.
    pub node: String,
    /// Your name; a named worker occupies one slot per issue. Absent, this
    /// conversation's holder: the session the runner stamped, else the seat
    /// named after the client that connected, tagged with the runner's process.
    #[serde(default)]
    pub assignee: Option<String>,
}

/// A session node to finish.
#[derive(Deserialize, JsonSchema)]
pub struct FinishArgs {
    /// The tracker id of the issue, or a 32-hex claim-graph id.
    pub node: String,
    /// `done` (the default), `failed`, or `cancelled`. To stop without
    /// finishing, use `ljos_release` instead.
    pub status: Option<String>,
    /// Generation from the sitting's claim. Required. A stale gen is refused.
    pub gen: u64,
    /// The name that holds it. Absent: the runner's seat name.
    #[serde(default)]
    pub assignee: Option<String>,
}

/// An argv to check.
#[derive(Deserialize, JsonSchema)]
pub struct ArgvArgs {
    /// The command line, one element per argument.
    pub argv: Vec<String>,
}

/// No arguments.
#[derive(Deserialize, JsonSchema)]
pub struct NoArgs {}

/// One trust row.
#[derive(Deserialize, JsonSchema)]
pub struct TrustArgs {
    /// The agent doing the weighing.
    pub from: String,
    /// The agent being weighed.
    pub to: String,
    /// In (0, 1].
    pub weight: f64,
    /// Deed accessions the row stands on.
    #[serde(default)]
    pub why: Vec<String>,
    /// Domains this row is scoped to; none means it applies everywhere.
    #[serde(default)]
    pub about: Vec<String>,
}

/// An issue and what turned out right on it.
#[derive(Deserialize, JsonSchema)]
pub struct LearnArgs {
    /// The tracker id.
    pub issue: String,
    /// The option that turned out right.
    pub outcome: String,
    /// The factor a refuted persona's anchor moves by; the seat's default
    /// when absent. The rows themselves follow each voter's record.
    pub beta: Option<f64>,
}

/// A slice of the seat to pack.
#[derive(Deserialize, JsonSchema)]
pub struct PackArgs {
    /// Where to write the satchel.
    pub out: String,
    /// Projects to take whole.
    #[serde(default)]
    pub projects: Vec<String>,
    /// Issues to take, with what they stand on.
    #[serde(default)]
    pub issues: Vec<String>,
}

/// A satchel that arrived.
#[derive(Deserialize, JsonSchema)]
pub struct ReceiveArgs {
    /// The satchel directory.
    pub dir: String,
    /// A bridge file from the last handover by the same sender.
    pub since: Option<String>,
    /// Put the enclosed atoms into this seat's pack.
    pub import: Option<bool>,
}

/// One review graded.
#[derive(Deserialize, JsonSchema)]
pub struct GradeArgs {
    /// The atom id.
    pub id: String,
    /// False when the claim had to be looked up again.
    pub recalled: Option<bool>,
}

/// A sitting to open.
#[derive(Deserialize, JsonSchema)]
pub struct SittingArgs {
    /// The tracker id of the issue (`proj-1a2b`).
    pub issue: String,
    /// Your name; a named worker occupies one slot per issue. Absent, this
    /// conversation's holder: the session the runner stamped, else the seat
    /// named after the client that connected, tagged with the runner's process.
    #[serde(default)]
    pub assignee: Option<String>,
}

/// A sitting to close.
#[derive(Deserialize, JsonSchema)]
pub struct FinishSittingArgs {
    /// The tracker id of the issue.
    pub issue: String,
    /// `done` (the default), `failed`, or `cancelled`.
    pub status: Option<String>,
    /// The lesson this sitting taught, two sentences at most. Omit only
    /// when there was none; the report says so.
    pub lesson: Option<String>,
    /// The option that turned out right, when the ballots are in and the
    /// world has said. Omit when nobody knows yet.
    pub outcome: Option<String>,
    /// Generation from the sitting's claim. Required. A stale gen is refused.
    pub gen: u64,
    /// The name that holds it. Absent: the runner's seat name.
    #[serde(default)]
    pub assignee: Option<String>,
}

/// A project whose history calibrates the voters.
#[derive(Deserialize, JsonSchema)]
pub struct CalibrateArgs {
    /// The tracker project.
    pub project: String,
    /// Expectation-maximisation rounds; twenty when absent.
    pub rounds: Option<usize>,
}

/// A cue: the task or question at hand.
#[derive(Deserialize, JsonSchema)]
pub struct CueArgs {
    /// What the seat is about to work on, in its own words.
    pub cue: String,
    /// Fire the strongest eight together so their links gain weight; say
    /// true when the island is the one you go on to use.
    pub fire: Option<bool>,
    /// Walk the island as this persona: its own weights lead the spread,
    /// and a fire moves its weights and not the seat's. A panel member
    /// passes its own name; the seat passes nothing.
    #[serde(default, rename = "as")]
    pub lens: Option<String>,
}

/// One memory an island holds.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IslandRow {
    pub id: Option<String>,
    pub kind: String,
    pub text: String,
    /// Relative to the strongest, so the top is 1.
    pub activation: f64,
    /// Whether search found it, or activation reached it.
    pub seed: bool,
    /// How long ago it was written, in words; empty when unstamped.
    pub age: String,
}

/// One habitat and whether it answers.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HabitatRow {
    pub name: String,
    pub state: String,
    pub ok: bool,
}

/// One atom due for review.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DueRow {
    pub id: Option<String>,
    pub kind: String,
    pub text: String,
    pub due_at: String,
}

/// One row of the influence graph.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TrustRow {
    pub from: String,
    pub to: String,
    pub weight: f64,
}

// ---- answers ---------------------------------------------------------------

/// What a habitat printed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Said {
    /// The habitat's answer, as it printed it.
    pub text: String,
    /// What it said aside, when anything.
    pub aside: Option<String>,
}

/// One remembered thing.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HitRow {
    /// The atom's id, when it has one.
    pub id: Option<String>,
    /// `lesson` or `preference`, or another kind the pack holds.
    pub kind: String,
    /// The claim as it was stored.
    pub text: String,
    /// The pack's score for it.
    pub score: f64,
    /// When it was written, RFC 3339; absent on a card paragraph.
    pub ts: Option<String>,
    /// How long ago that was, in words: `today`, `3 weeks ago`. Read the
    /// hits as a timeline: a later lesson revises an earlier one.
    pub age: String,
    /// The seat that wrote it, when it was not this one. Many seats share
    /// a pack.
    pub from: Option<String>,
}

/// The argv law's answer.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PolicyRow {
    /// The line as it would run.
    pub argv: String,
    /// What this process is not: a check. Reloading a pack is not one either.
    pub note: String,
}

fn said(out: ljos_cli::Said) -> Json<Said> {
    let aside = out.stderr.trim();
    Json(Said {
        text: out.stdout,
        aside: (!aside.is_empty()).then(|| aside.to_string()),
    })
}

/// A refusal, as the protocol carries one.
fn refused(e: anyhow::Error) -> McpError {
    McpError::internal_error(format!("{e:#}"), None)
}

/// Run a habitat's verb and hand back what it said.
fn habitat(bin: &str, args: &[&str]) -> Result<Json<Said>, McpError> {
    run_captured(bin, args).map(said).map_err(refused)
}

/// [`habitat`] with `VISSUE_AGENT` set, so a ballot is recorded under a
/// persona's name.
fn habitat_as(bin: &str, args: &[&str], identity: Option<&str>) -> Result<Json<Said>, McpError> {
    let Some(who) = identity_or_seat(identity) else {
        return habitat(bin, args);
    };
    use std::process::{Command, Stdio};
    let path = which::which(bin).map_err(|_| refused(anyhow::anyhow!("{bin} not on PATH")))?;
    let out = Command::new(path)
        .env("VISSUE_AGENT", who)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| refused(anyhow::anyhow!("{bin}: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        return Err(refused(anyhow::anyhow!(
            "{bin} exited {}: {stderr}",
            out.status
        )));
    }
    Ok(said(ljos_cli::Said { stdout, stderr }))
}

#[tool_router]
impl LjosServer {
    /// Read the cards directory from `LJOS_CARDS_DIR`, or the working directory,
    /// the same default the command line has.
    #[must_use]
    pub fn from_env() -> Self {
        let cards_dir = std::env::var_os("LJOS_CARDS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self { cards_dir }
    }

    /// Open on a named cards directory, for a test.
    #[cfg(test)]
    #[must_use]
    pub fn at(cards_dir: PathBuf) -> Self {
        Self { cards_dir }
    }

    // ---- the pack --------------------------------------------------------

    #[tool(
        description = "Call this when the work taught something that will still be true next sitting: one lesson, two short sentences at most, stored as given. Never a transcript or a summary of the session. Not for progress notes; those go on the issue.",
        annotations(
            title = "Remember",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_remember(
        &self,
        Parameters(args): Parameters<ClaimArgs>,
    ) -> Result<Json<serde_json::Value>, McpError> {
        packset_write_as("Remember", &args.text, args.as_persona.as_deref())
            .map(Json)
            .map_err(refused)
    }

    #[tool(
        description = "Call this when a choice between two ways was settled and should hold from now on: one standing preference, stored as given. A lesson that is not a choice is ljos_remember.",
        annotations(
            title = "Prefer",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_prefer(
        &self,
        Parameters(args): Parameters<ClaimArgs>,
    ) -> Result<Json<serde_json::Value>, McpError> {
        packset_write_as("Prefer", &args.text, args.as_persona.as_deref())
            .map(Json)
            .map_err(refused)
    }

    #[tool(
        description = "Call this when reviewing the seat's memory for contradictions the words do not show: the lowest passes between single memories in the pack's embedding landscape, read by the optional `landscape` habitat. On a record of planted contradictions the lowest passes were the contradictions nine times in ten. Nothing is written; judge each pair, then ljos_forget the one that turned out wrong or ljos_remember the rewrite. Absent the habitat, the tool says so.",
        annotations(
            title = "Contradiction candidates",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_conflicts(
        &self,
        Parameters(args): Parameters<LimitArgs>,
    ) -> Result<Json<Said>, McpError> {
        let text = conflicts(args.limit.unwrap_or(12)).map_err(refused)?;
        Ok(Json(Said { text, aside: None }))
    }

    #[tool(
        description = "Call this after a run of lessons on one matter, or after importing a handover: consolidate the seat's memory. Every claim that rewrites an earlier one of the same kind (same opening words, a new object; a correction; an explicit supersedes) closes the earlier one's validity window and names it, the rule a write applies on arrival run over what is already held. With apply false (the default) it reports the pairs and writes nothing; read them, then call again with apply true.",
        annotations(
            title = "Consolidate",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_consolidate(
        &self,
        Parameters(args): Parameters<ConsolidateArgs>,
    ) -> Result<Json<Said>, McpError> {
        let body = packset_consolidate(args.apply.unwrap_or(false)).map_err(refused)?;
        Ok(Json(Said {
            text: format_consolidation(&body),
            aside: None,
        }))
    }

    #[tool(
        description = "Call this when the work showed a standing claim wrong, with the deed that showed it: retire the atom by id, so the seat stops recalling it. The pack tombstones rather than erases and keeps the record, and `why` names the deed the retraction stands on. Forget a claim that turned out wrong; do not forget one merely because this sitting disagrees with it.",
        annotations(
            title = "Forget",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_forget(
        &self,
        Parameters(args): Parameters<ForgetArgs>,
    ) -> Result<Json<serde_json::Value>, McpError> {
        packset_forget(&args.id, args.why.as_deref())
            .map(Json)
            .map_err(refused)
    }

    #[tool(
        description = "Call this at the start of any task, before reading code or files, with the topic in a few words: what the seat already knows, ranked, each hit with its age so the list reads as a timeline. Pass as_of to ask what the seat knew at an earlier time. An empty list means the pack holds nothing on it; a failure means the writer is down, which is a different thing: `packset ensure` starts one. Follow with ljos_island for the cluster the task touches.",
        annotations(
            title = "Search the pack",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<Json<Rows<HitRow>>, McpError> {
        let hits =
            packset_search_as_of(&args.query, 10, args.as_of.as_deref(), false).map_err(refused)?;
        let now = args.as_of.clone().unwrap_or_else(now_utc);
        let mine = seat_name();
        Ok(rows(
            hits.into_iter()
                .map(|h| HitRow {
                    id: h.id,
                    kind: h.kind,
                    text: h.text,
                    score: h.score,
                    age: age_of(h.ts.as_deref(), &now),
                    from: other_seat(&h.entities, &mine),
                    ts: h.ts,
                })
                .collect(),
        ))
    }

    #[tool(
        description = "Call this when a number the seat tracks has been measured again: a benchmark score, a latency, a count. With a value it takes the reading, closes the habit's earlier reading (kept as `was`), and sets the next reading due one cadence on, so ljos_due and the hook say when it is late. Without a value it lists the habits as they stand, each with the change since the last reading and when the next is due. ljos_search with as_of answers what a habit stood at then.",
        annotations(
            title = "Take or read a habit's reading",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_habit(
        &self,
        Parameters(args): Parameters<HabitArgs>,
    ) -> Result<Json<serde_json::Value>, McpError> {
        let now = now_utc();
        if let (Some(name), Some(value)) = (&args.name, args.value) {
            let every_s = parse_every(args.every.as_deref().unwrap_or("7d")).map_err(refused)?;
            let (body, prev) = habit(
                name,
                value,
                args.unit.as_deref().unwrap_or(""),
                every_s,
                args.source.as_deref().unwrap_or(""),
            )
            .map_err(refused)?;
            return Ok(Json(serde_json::json!({
                "reading": body,
                "was": prev.map(|p| serde_json::json!({"value": p.value, "ts": p.ts, "id": p.id})),
            })));
        }
        let rows: Vec<ReadingRow> = habits()
            .map_err(refused)?
            .into_iter()
            .filter(|r| args.name.as_deref().is_none_or(|n| r.name == n.trim()))
            .map(|r| ReadingRow {
                change: format_change(&r, &now),
                age: age_of(r.ts.as_deref(), &now),
                name: r.name,
                value: r.value,
                unit: r.unit,
                due_at: r.due_at,
                source: r.source,
                id: r.id,
            })
            .collect();
        Ok(Json(serde_json::json!(rows)))
    }

    // ---- the deed store ----------------------------------------------------

    #[tool(
        description = "Call this before standing on a deed somebody cited: whether its bytes are intact and its sources are too. The deed store answers; an accession it does not hold is a failure, not an empty answer.",
        annotations(
            title = "Evidence a deed",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_evidence(
        &self,
        Parameters(args): Parameters<AccessionArgs>,
    ) -> Result<Json<Said>, McpError> {
        habitat("deedar", &["evidence", &args.accession])
    }

    #[tool(
        description = "Call this on every deed a handover or an issue names: whether it is still the tip or a later take superseded it. A citation that resolves and is stale is worse than one that fails, because nothing complains.",
        annotations(
            title = "Is a deed current",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_current(
        &self,
        Parameters(args): Parameters<AccessionArgs>,
    ) -> Result<Json<Said>, McpError> {
        habitat("deedar", &["current", &args.accession])
    }

    // ---- the tracker ---------------------------------------------------------

    #[tool(
        description = "Call this after the work produced something and the deed store minted it (deedar create): cite the accession on the issue. Omit the accession to list what the issue cites. A citation names the accession; the bytes stay in deedar.",
        annotations(
            title = "Cite a deed",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_deed(
        &self,
        Parameters(args): Parameters<DeedArgs>,
    ) -> Result<Json<Said>, McpError> {
        match &args.add {
            Some(a) => habitat("vissue", &["deed", &args.issue, "--add", a]),
            None => habitat("vissue", &["deed", &args.issue]),
        }
    }

    #[tool(
        description = "Call this before claiming an issue: its plan, what its inputs produced, and the deeds it has cited so far. The issue id is the tracker id (proj-1a2b).",
        annotations(
            title = "Recall a node",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_recall(
        &self,
        Parameters(args): Parameters<IssueArgs>,
    ) -> Result<Json<Said>, McpError> {
        habitat("vissue", &["recall", &args.issue])
    }

    #[tool(
        description = "Call this after ljos_recall when the order of events matters, or when a question is about when: the issue's timeline, one dated list oldest first across the three stores, the tracker's logbook (creation, state changes, claims, notes), the deeds it cites with the time each was produced, and the memories its title activates with the time each was written. Each line carries its age and the gap since the line before; a later line supersedes an earlier one on the same matter. Read-only.",
        annotations(
            title = "Timeline of an issue",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_timeline(
        &self,
        Parameters(args): Parameters<IssueArgs>,
    ) -> Result<Json<Said>, McpError> {
        let text = timeline(&args.issue, 60).map_err(refused)?;
        Ok(Json(Said { text, aside: None }))
    }

    #[tool(
        description = "Call this when a decision on an issue has more than one defensible answer: cast this identity's ballot for an option, or omit the option to read the tally. One ballot per identity (VISSUE_AGENT); a recast replaces. Then ljos_consensus settles it; the tally is only a count.",
        annotations(
            title = "Vote",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_vote(
        &self,
        Parameters(args): Parameters<VoteArgs>,
    ) -> Result<Json<Said>, McpError> {
        match &args.choice {
            Some(c) => habitat_as(
                "vissue",
                &["vote", &args.issue, "--for", c],
                args.as_persona.as_deref(),
            ),
            None => habitat("vissue", &["vote", &args.issue]),
        }
    }

    // ---- the session graph --------------------------------------------------

    #[tool(
        description = "Call this before starting work on an issue, after ljos_recall: it takes the session node for that tracker id under your name. A named worker occupies one slot (busy names the held node and ljos_complete / ljos_release). A harness seat occupies per issue. Completing a node later does not close the ticket.",
        annotations(
            title = "Claim a node",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_claim(
        &self,
        Parameters(args): Parameters<TakeArgs>,
    ) -> Result<Json<Said>, McpError> {
        let text =
            claim(&args.node, &resolve_assignee(args.assignee.as_deref())).map_err(refused)?;
        Ok(Json(Said { text, aside: None }))
    }

    #[tool(
        description = "Call this to begin work on an issue; it is the whole opening of a sitting in the protocol's order and stops at the first store that does not answer: doctor, cards, the review clock, the island the issue's title activates, the working set, and the claim. Occupancy is the runner session (`*_SESSION_ID`) then `{name}:{issue}`: two conversations hold two tickets. Omit assignee.",
        annotations(
            title = "Open a sitting",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_sitting(
        &self,
        Parameters(args): Parameters<SittingArgs>,
    ) -> Result<Json<Said>, McpError> {
        let text = sitting(
            &args.issue,
            &resolve_assignee(args.assignee.as_deref()),
            &self.cards_dir,
        )
        .map_err(refused)?;
        Ok(Json(Said { text, aside: None }))
    }

    #[tool(
        description = "Call this when the work on an issue ends; it is the whole closing of a sitting: remember the lesson, fire the island so its links gain weight, complete the session node, and learn from the outcome when one is named. Pass the lesson: a sitting that taught nothing worth two sentences is rare, and the report says so when none is given.",
        annotations(
            title = "Close a sitting",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_finish(
        &self,
        Parameters(args): Parameters<FinishSittingArgs>,
    ) -> Result<Json<Said>, McpError> {
        let text = finish(
            &args.issue,
            args.status.as_deref().unwrap_or("done"),
            args.lesson.as_deref(),
            args.outcome.as_deref(),
            LEARN_BETA,
            &resolve_assignee(args.assignee.as_deref()),
            args.gen,
        )
        .map_err(refused)?;
        Ok(Json(Said { text, aside: None }))
    }

    #[tool(
        description = "Call this once a project has a few voted issues, and again when it has many more: estimate each voter's accuracy from the project's voting history with no truth labels (Dawid and Skene) and write the accuracies back as trust rows, so a consensus stops being a count even when nobody named an outcome.",
        annotations(
            title = "Calibrate the voters",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_calibrate(
        &self,
        Parameters(args): Parameters<CalibrateArgs>,
    ) -> Result<Json<Rows<TrustRow>>, McpError> {
        let rows = calibrate(&args.project, args.rounds.unwrap_or(20)).map_err(refused)?;
        Ok(rows(
            rows.into_iter()
                .map(|r| TrustRow {
                    from: r.from,
                    to: r.to,
                    weight: r.weight,
                })
                .collect(),
        ))
    }

    #[tool(
        description = "Call this when you stop working on an issue without finishing it: the session node goes back to ready, your name is free to claim again, and the generation moves. Not for finished work; that is ljos_complete.",
        annotations(
            title = "Hand a node back",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_release(
        &self,
        Parameters(args): Parameters<TakeArgs>,
    ) -> Result<Json<Said>, McpError> {
        let text =
            release(&args.node, &resolve_assignee(args.assignee.as_deref())).map_err(refused)?;
        Ok(Json(Said { text, aside: None }))
    }

    #[tool(
        description = "Call this when the work on an issue is finished, failed, or cancelled: the session node goes terminal. Completing is not closing; the ticket stays open until the tracker changes its state. To stop without finishing, ljos_release.",
        annotations(
            title = "Complete a node",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_complete(
        &self,
        Parameters(args): Parameters<FinishArgs>,
    ) -> Result<Json<Said>, McpError> {
        let text = complete(
            &args.node,
            args.status.as_deref(),
            &resolve_assignee(args.assignee.as_deref()),
            args.gen,
        )
        .map_err(refused)?;
        Ok(Json(Said { text, aside: None }))
    }

    // ---- cards, policy, consensus -------------------------------------------

    #[tool(
        description = "Call this second in a sitting, after ljos_doctor: the cards, what the human froze. USER.md and MEMORY.md from the cards directory, read-only. A missing card prints nothing. Nothing here writes one.",
        annotations(
            title = "Read the cards",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_cards(&self, Parameters(_): Parameters<NoArgs>) -> Result<Json<Said>, McpError> {
        cards(&self.cards_dir)
            .map(|text| Json(Said { text, aside: None }))
            .map_err(refused)
    }

    #[tool(
        description = "Argv law: print the line as it would run. This is not a check, and reloading a pack is not one either; ljos-policyd is the trusted base when it is on PATH or POLICYD_BIN.",
        annotations(
            title = "Print an argv",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_policy(
        &self,
        Parameters(args): Parameters<ArgvArgs>,
    ) -> Result<Json<PolicyRow>, McpError> {
        let argv = policy_line(&args.argv).map_err(refused)?;
        Ok(Json(PolicyRow {
            argv,
            note: POLICY_TCB.to_string(),
        }))
    }

    #[tool(
        description = "Call this after the ballots are in on an issue: the consensus model first, DeGroot or Friedkin-Johnsen over the trust rows the pack holds, then the tracker's own verb. Not a vote count. With no rows every voter weighs the same.",
        annotations(title = "Consensus", read_only_hint = true, open_world_hint = false)
    )]
    async fn ljos_consensus(
        &self,
        Parameters(args): Parameters<IssueArgs>,
    ) -> Result<Json<Rows<Said>>, McpError> {
        // Rows scoped to a domain apply when the issue is about it; the
        // personas' anchors go to both settles.
        let shown = run_captured("vissue", &["show", &args.issue, "--json"])
            .ok()
            .and_then(|said| serde_json::from_str::<serde_json::Value>(&said.stdout).ok());
        let topic = shown
            .as_ref()
            .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(topic_words))
            .unwrap_or_default();
        let tags: Vec<String> = shown
            .as_ref()
            .and_then(|v| v.get("org_tags").and_then(|t| t.as_array()).cloned())
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_lowercase))
            .collect();
        let trust = rows_about(&trust_from_pack().unwrap_or_default(), &topic);
        let personas = personas_from_pack().unwrap_or_default();
        let steps = consensus_steps_for(
            &args.issue,
            on_path("ljos-consensus"),
            on_path("vissue"),
            &trust,
            &personas,
            &tags,
        )
        .map_err(refused)?;
        let mut out = Vec::new();
        for step in steps {
            let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
            out.push(habitat(step.bin, &args)?.0);
        }
        Ok(rows(out))
    }

    #[tool(
        description = "Call this when a person or a checked outcome says how much one voter should weigh another: write one trust row to the pack, from weighs to at weight in (0, 1], citing the deeds it stands on. Trust is memory: the row has a validity window and a later row for the same pair supersedes it.",
        annotations(
            title = "Trust",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_trust(
        &self,
        Parameters(args): Parameters<TrustArgs>,
    ) -> Result<Json<serde_json::Value>, McpError> {
        let row = Trust {
            from: args.from,
            to: args.to,
            weight: args.weight,
            about: args.about,
        };
        write_trust(&row, &args.why).map(Json).map_err(refused)
    }

    #[tool(
        description = "Call this beside a ballot when the question is hard and the majority may be wrong: forecast what share the others give each option. With two or more forecasts, ljos_consensus also names the surprisingly popular answer, the option whose actual share most exceeds its forecast share.",
        annotations(
            title = "Forecast the others",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_predict(
        &self,
        Parameters(args): Parameters<PredictArgs>,
    ) -> Result<Json<serde_json::Value>, McpError> {
        let who =
            identity_or_seat(args.as_persona.as_deref()).unwrap_or_else(|| "seat".to_string());
        let expect = match &args.expect {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        write_prediction(&args.issue, &who, &expect)
            .map(Json)
            .map_err(refused)
    }

    #[tool(
        description = "Call this when the work has shown that a kind of command must never run, or must be asked about first: writes a rule to the pack, a glob over the command line with a verdict. The memory hook and `ljos policy` enforce it at the point of action, and it travels in handovers like any memory.",
        annotations(
            title = "Write a rule",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_rule(
        &self,
        Parameters(args): Parameters<RuleArgs>,
    ) -> Result<Json<serde_json::Value>, McpError> {
        write_rule(&Rule {
            pattern: args.pattern,
            verdict: args.verdict,
            reason: args.why,
        })
        .map(Json)
        .map_err(refused)
    }

    #[tool(
        description = "Call this to start a subagent that plays a persona on an issue: the text it should begin from. The persona's view and anchor, what the seat knows on its domains (preferences first), the issue's working set, and the one ballot it must end with. Read-only.",
        annotations(
            title = "Brief a persona",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_brief(
        &self,
        Parameters(args): Parameters<BriefArgs>,
    ) -> Result<Json<Said>, McpError> {
        let text = brief(&args.name, &args.issue).map_err(refused)?;
        Ok(Json(Said { text, aside: None }))
    }

    #[tool(
        description = "Call this when the work wants a voter with a view of its own, such as a reviewer for a broad audience or a domain expert: write a persona with a name, an anchor in [0, 1] for how far it moves off its ballot in a settle (0 never moves), a sentence or two on how it reads the work, and the domains it speaks to. Then vote with `as` set to its name; ljos_consensus reads its anchor.",
        annotations(
            title = "Write a persona",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_persona(
        &self,
        Parameters(args): Parameters<PersonaArgs>,
    ) -> Result<Json<serde_json::Value>, McpError> {
        let persona = Persona {
            name: args.name,
            anchor: args.anchor.unwrap_or(0.5),
            view: args.view,
            entities: args.about,
        };
        write_persona(&persona).map(Json).map_err(refused)
    }

    #[tool(
        description = "Call this when the world has said which option on an issue was right: reweigh the voters by it; every voter whose ballot the outcome refuted shrinks in every other voter's row (Hedge), a vindicated one keeps its weight. Writes the complete set of rows to the pack and returns them.",
        annotations(
            title = "Learn",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_learn(
        &self,
        Parameters(args): Parameters<LearnArgs>,
    ) -> Result<Json<Rows<TrustRow>>, McpError> {
        let said = run_captured("vissue", &["vote", &args.issue, "--json"]).map_err(refused)?;
        let ballots = ballots_from_json(&said.stdout).map_err(refused)?;
        // Scoped to what the issue's island is about, so a voter wrong here
        // keeps its standing elsewhere; a refuted persona listens more.
        let about = island_entities(&args.issue).unwrap_or_default();
        let (rows, _moved) = learn_and_write(
            &ballots,
            &args.outcome,
            args.beta.unwrap_or(LEARN_BETA),
            &about,
        )
        .map_err(refused)?;
        Ok(rows(
            rows.into_iter()
                .map(|r| TrustRow {
                    from: r.from,
                    to: r.to,
                    weight: r.weight,
                })
                .collect(),
        ))
    }

    #[tool(
        description = "Call this first in a sitting, and again whenever another tool fails: which habitats answer (binaries, pack, host key, deed store, tracker, claim graph) and whether this harness is onboarded. A tool failing is a habitat down or refusing, never an empty answer.",
        annotations(title = "Doctor", read_only_hint = true, open_world_hint = false)
    )]
    async fn ljos_doctor(&self) -> Result<Json<Rows<HabitatRow>>, McpError> {
        Ok(rows(
            doctor()
                .into_iter()
                .map(|h| HabitatRow {
                    name: h.name.to_string(),
                    state: h.state,
                    ok: h.ok,
                })
                .collect(),
        ))
    }

    #[tool(
        description = "Call this when another seat takes the work over: pack a slice of this seat for it, the tracker's satchel for the projects and issues named, the pack's atoms (trust rows included), the deeds both cite with their receipts, sealed, and signed when the host has a key.",
        annotations(
            title = "Handover",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_handover(
        &self,
        Parameters(args): Parameters<PackArgs>,
    ) -> Result<Json<Rows<String>>, McpError> {
        handover(Path::new(&args.out), &args.projects, &args.issues)
            .map(rows)
            .map_err(refused)
    }

    #[tool(
        description = "Call this when a handover directory arrived from another seat, before anything in it is trusted: check the satchel, manifest, deed receipts against the head in the bag (and the bridge from a kept head when given), the signature, and what the atoms hold. With import, the atoms go into this seat's pack.",
        annotations(
            title = "Receive",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_receive(
        &self,
        Parameters(args): Parameters<ReceiveArgs>,
    ) -> Result<Json<Rows<String>>, McpError> {
        receive(
            Path::new(&args.dir),
            args.since.as_deref().map(Path::new),
            args.import.unwrap_or(false),
        )
        .map(rows)
        .map_err(refused)
    }

    #[tool(
        description = "Call this after ljos_search with the task in your own words: the memories the task activates, the top search hits as seeds, spread two hops along the pack's links, strongest first. Not a persona or a view; the cluster this task touches. Read it before starting the work; pass fire when you go on to use it, so those links gain weight.",
        annotations(
            title = "Island",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_island(
        &self,
        Parameters(args): Parameters<CueArgs>,
    ) -> Result<Json<Rows<IslandRow>>, McpError> {
        let now = now_utc();
        let body = packset_island_as(&args.cue, args.fire.unwrap_or(false), args.lens.as_deref())
            .map_err(refused)?;
        Ok(rows(
            body["island"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|a| IslandRow {
                    id: a["id"].as_str().map(str::to_string),
                    kind: a["kind"].as_str().unwrap_or("").to_string(),
                    text: a["text"].as_str().unwrap_or("").to_string(),
                    activation: a["activation"].as_f64().unwrap_or(0.0),
                    seed: a["seed"].as_bool().unwrap_or(false),
                    age: age_of(a["ts"].as_str(), &now),
                })
                .collect(),
        ))
    }

    #[tool(
        description = "Call this before a panel or a persona vote: the personas the pack holds, each with its name, anchor, the domains it speaks to and its view. Write a missing one with ljos_persona.",
        annotations(title = "Personas", read_only_hint = true, open_world_hint = false)
    )]
    async fn ljos_personas(&self) -> Result<Json<Rows<PersonaRow>>, McpError> {
        Ok(rows(
            personas_from_pack()
                .map_err(refused)?
                .into_iter()
                .map(|p| PersonaRow {
                    name: p.name,
                    anchor: p.anchor,
                    about: p.entities,
                    view: p.view,
                })
                .collect(),
        ))
    }

    #[tool(
        description = "Call this at the start of a sitting, after the cards: the claims whose review is due, soonest first. Read each, then ljos_graded it recalled or lapsed; the review clock moves only when you grade.",
        annotations(title = "Due", read_only_hint = true, open_world_hint = false)
    )]
    async fn ljos_due(&self) -> Result<Json<Rows<DueRow>>, McpError> {
        Ok(rows(
            due()
                .map_err(refused)?
                .into_iter()
                .map(|a| DueRow {
                    id: a["id"].as_str().map(str::to_string),
                    kind: a["kind"].as_str().unwrap_or("").to_string(),
                    text: a["text"].as_str().unwrap_or("").to_string(),
                    due_at: a["due_at"].as_str().unwrap_or("").to_string(),
                })
                .collect(),
        ))
    }

    #[tool(
        description = "Call this for each claim ljos_due listed once you have read it: recalled (default) pushes the next review out, lapsed brings it back sooner. Returns the atom with its new due_at.",
        annotations(
            title = "Graded",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ljos_graded(
        &self,
        Parameters(args): Parameters<GradeArgs>,
    ) -> Result<Json<serde_json::Value>, McpError> {
        graded(&args.id, args.recalled.unwrap_or(true))
            .map(Json)
            .map_err(refused)
    }
}

// ---- prompts ---------------------------------------------------------------

/// A node to pick up.
#[derive(Deserialize, JsonSchema)]
pub struct PickUpArgs {
    /// The tracker id of the work, when known.
    pub issue: Option<String>,
}

/// A handover to check.
#[derive(Deserialize, JsonSchema)]
pub struct HandoverArgs {
    /// The satchel directory that arrived.
    pub dir: String,
}

fn asked(text: String) -> Vec<PromptMessage> {
    vec![PromptMessage::new_text(Role::User, text)]
}

#[prompt_router(vis = "pub(crate)")]
impl LjosServer {
    /// Start a sitting: read the cards, ask the pack, then take the work.
    #[prompt(name = "start_a_sitting")]
    pub async fn start_a_sitting_prompt(
        &self,
        Parameters(args): Parameters<PickUpArgs>,
    ) -> Result<Vec<PromptMessage>, McpError> {
        let work = args.issue.filter(|i| !i.trim().is_empty()).map_or_else(
            || "what the tracker says is ready".to_string(),
            |i| format!("tracker node {i}"),
        );
        Ok(asked(format!(
            "Begin a sitting on {work}.\n\
             \n\
             Five habitats answer five different questions, and the order matters:\n\
             \n\
             1. `ljos_cards`. What the human froze. Read them first and leave them\n\
                as they are; if they are empty, they are empty.\n\
             2. `ljos_due` for the claims whose review is due; a sitting prints a\n\
                short prefix. Read each shown row and `ljos_graded` it.\n\
             3. `ljos_search` then `ljos_island` with the task in your own words.\n\
             4. `ljos_recall` on the node. What it stands on and what it cited.\n\
             5. `ljos_timeline` the last twelve dated events across the stores.\n\
             6. `ljos_claim` a session node for it. A harness seat occupies per\n\
                issue; a named worker occupies one slot.\n\
             \n\
             When something is learned that will still be true next sitting, say it\n\
             with `ljos_remember` in two short sentences. When the work shows a\n\
             standing claim was wrong, `ljos_forget` it and name the deed that\n\
             showed it; disagreeing with a claim is not showing it wrong. When the\n\
             work produces something, mint the deed in the deed store and\n\
             `ljos_deed` it on the node. Completing the session node does not\n\
             close the ticket. A tool that fails is a habitat refusing or down:\n\
             `ljos_doctor` says which."
        )))
    }

    /// Run a panel: one subagent per persona in the pack, each voting as
    /// itself, then the settle under the trust rows.
    #[prompt(name = "run_a_panel")]
    pub async fn run_a_panel_prompt(
        &self,
        Parameters(args): Parameters<IssueArgs>,
    ) -> Result<Vec<PromptMessage>, McpError> {
        let issue = args.issue;
        // Only the personas whose domains the issue speaks to sit; a seat
        // that runs every persona on every issue is a count, not a panel.
        let all = personas_from_pack().unwrap_or_default();
        let personas = personas_speaking_to(&all, &issue_words(&issue));
        let roster = if personas.is_empty() {
            "The pack holds no personas yet. Write two or three with `ljos_persona` first: a \
             name, an anchor in [0, 1] (0 never moves off its ballot), a sentence on how it \
             reads the work, and the domains it speaks to."
                .to_string()
        } else {
            personas
                .iter()
                .map(|p| {
                    format!(
                        "- {} (anchor {:.2}{}): {}",
                        p.name,
                        p.anchor,
                        if p.entities.is_empty() {
                            String::new()
                        } else {
                            format!(", about {}", p.entities.join(", "))
                        },
                        p.view
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let seated = if personas.len() < all.len() {
            format!(
                "The personas whose domains this issue speaks to ({} of {} in the pack):",
                personas.len(),
                all.len()
            )
        } else {
            "The personas in this seat's pack:".to_string()
        };
        Ok(asked(format!(
            "Run a panel on {issue}.\n\n\
             {seated}\n{roster}\n\n\
             1. `ljos_recall` on {issue}, and `ljos_search` for what the seat knows about it.\n\
             2. For each persona, `ljos_brief` with its name and {issue}, and start one \
                subagent with that text as its whole brief: the persona's view, what the seat \
                knows on its domains, the working set. Each subagent reads the work in its own \
                way: `ljos_island` on {issue} with `as` set to its name walks the pack through \
                its own weights, and with `fire` true once it has read the work tightens the \
                paths it walked and nobody else's; `ljos_remember` with `as` writes what it \
                concluded into its own set. It ends by casting exactly one ballot: `ljos_vote` \
                on {issue} with `as` set to the persona's name, for the option it would defend. \
                Subagents run in parallel and do not see each other's ballots.\n\
             3. `ljos_consensus` on {issue}. The settle weighs the ballots by the trust rows \
                the pack holds and holds each persona to its ballot by its anchor; it \
                reports polarization and disagreement, not only shares. A tally is not this.\n\
             4. Act on the settle, not on the count. When the world later says which option \
                was right, `ljos_learn` on {issue} with that outcome, and the personas that \
                were wrong lose weight on this topic.\n\n\
             A panel with equal rows is a count. Run `ljos_calibrate` on the project once it \
             holds a few voted issues, so the rows carry what the personas' history says."
        )))
    }

    /// Check a handover somebody sent, in the order the questions come.
    #[prompt(name = "check_a_handover")]
    pub async fn check_a_handover_prompt(
        &self,
        Parameters(args): Parameters<HandoverArgs>,
    ) -> Result<Vec<PromptMessage>, McpError> {
        let dir = args.dir;
        Ok(asked(format!(
            "Check the handover at {dir}.\n\
             \n\
             `ljos_receive` on {dir} answers three questions in order, each\n\
             unanswered by the one before it: did it arrive as written (the\n\
             manifest), do the deeds predate the asking (every receipt to the head\n\
             in the bag, and the bridge from a kept head when `since` is given),\n\
             and who wrote it (the signature, when there is one). Pass `since` when\n\
             this sender has handed over before.\n\
             \n\
             Then, for each deed the bag names, `ljos_current`: a deed that arrived\n\
             intact and is no longer the tip is a different finding from one that\n\
             failed. Report which questions passed; a bag that fails one has not\n\
             mostly checked out. Only when all pass, `ljos_receive` again with\n\
             `import` so the atoms, trust rows included, join this seat's pack."
        )))
    }
}

#[tool_handler]
#[prompt_handler(router = Self::prompt_router())]
impl ServerHandler for LjosServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("ljos", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "One seat over five habitats: tracker, pack, deed store, claim graph, \
             consensus. Read ljos://protocol first; it says which store answers \
             which question and the order of tools in a sitting. ljos_sitting \
             runs the whole opening (doctor, cards, due, island, recall, timeline, claim) \
             and ljos_finish the whole closing (remember, fire, complete, learn); \
             prefer them. By hand: ljos_doctor, ljos_cards, ljos_due then \
             ljos_graded, ljos_search then ljos_island, ljos_recall, ljos_claim; \
             during the work ljos_deed, ljos_remember, ljos_vote; after it \
             ljos_island with fire, ljos_complete, ljos_learn. ljos_calibrate \
             moves the trust rows from a project's history when nobody names an \
             outcome. \
             The pack is written by remember, prefer, trust, learn, graded and an \
             imported handover; the text is the claim. Cards are read at \
             ljos://cards/ and never written. Citing a deed on a node names an \
             accession and does not paste the product. Completing a session node \
             does not close a ticket; ljos_finish with status done does, so a \
             board never shows TODO over finished work; ljos_release hands one \
             back unfinished. A \
             tool that fails means a habitat refused or is not running; it is not \
             an empty answer. The seat is named after the client that connected. \
             The holder is any `*_SESSION_ID` the runner stamped, the whole value, \
             else that name tagged with the runner's process; a shell the runner \
             opens finds the same names, so no field here needs an assignee.",
        )
    }

    /// The client has said who it is: name the seat after it and leave the
    /// record a shell below the same runner reads.
    async fn on_initialized(&self, context: rmcp::service::NotificationContext<rmcp::RoleServer>) {
        let client = context
            .peer
            .peer_info()
            .map(|info| info.client_info.name.clone())
            .unwrap_or_else(|| "runner".to_string());
        announce_seat(&client, runner_pid());
    }

    /// The two cards and the protocol, and nothing else.
    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let mut protocol = Resource::new(PROTOCOL_URI, "protocol".to_string());
        protocol.title = Some("The seat protocol".into());
        protocol.description = Some(
            "Which store answers which question, the order of tools before, during and \
             after the work, and the refusals worth knowing. Read this first."
                .into(),
        );
        protocol.mime_type = Some("text/markdown".to_string());
        Ok(ListResourcesResult::with_all_items(
            std::iter::once(protocol)
                .chain(CARD_NAMES.iter().map(|name| {
                    let mut resource =
                        Resource::new(format!("{SCHEME}://cards/{name}"), (*name).to_string());
                    resource.title = Some(format!("{name}, frozen by the human"));
                    resource.description = Some(
                        "A card. Read-only; overflow is an error, not a prompt to write more."
                            .into(),
                    );
                    resource.mime_type = Some("text/markdown".to_string());
                    resource
                }))
                .collect(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri.clone();
        if uri == PROTOCOL_URI {
            return Ok(ReadResourceResult::new(vec![ResourceContents::text(PROTOCOL, uri)]).into());
        }
        let name = card_named(&uri).ok_or_else(|| {
            McpError::resource_not_found(
                format!(
                    "{uri}: the resources are {PROTOCOL_URI}, {SCHEME}://cards/USER.md and \
                     {SCHEME}://cards/MEMORY.md"
                ),
                None,
            )
        })?;
        let path = self.cards_dir.join(name);
        // A missing card is an empty card; nothing is created.
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Ok(ReadResourceResult::new(vec![ResourceContents::text(text, uri)]).into())
    }
}

/// The card a resource uri names; anything else, including a third file in
/// the same directory, is nothing.
fn card_named(uri: &str) -> Option<&'static str> {
    let name = uri.strip_prefix(&format!("{SCHEME}://cards/"))?;
    CARD_NAMES.iter().copied().find(|card| *card == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tool is annotated, and the writers are the contract's twelve.
    #[test]
    fn the_writers_are_the_thirteen_the_contract_names() {
        let tools = LjosServer::tool_router().list_all();
        assert_eq!(
            tools.len(),
            35,
            "{:?}",
            tools.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
        let mut writers: Vec<String> = Vec::new();
        for tool in &tools {
            let hints = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{} carries no annotations", tool.name));
            assert_eq!(hints.open_world_hint, Some(false), "{}", tool.name);
            match hints.read_only_hint {
                Some(true) => {}
                Some(false) => {
                    // Forget is the one verb that takes something away. Every
                    // other writer adds, so saying so is the whole annotation.
                    let expected = Some(tool.name == "ljos_forget");
                    assert_eq!(
                        hints.destructive_hint, expected,
                        "{} misreports whether it destroys",
                        tool.name
                    );
                    writers.push(tool.name.to_string());
                }
                None => panic!("{} does not say whether it writes", tool.name),
            }
        }
        writers.sort();
        assert_eq!(
            writers,
            [
                "ljos_calibrate",
                "ljos_claim",
                "ljos_complete",
                "ljos_consolidate",
                "ljos_deed",
                "ljos_finish",
                "ljos_forget",
                "ljos_graded",
                "ljos_habit",
                "ljos_handover",
                "ljos_island",
                "ljos_learn",
                "ljos_persona",
                "ljos_predict",
                "ljos_prefer",
                "ljos_receive",
                "ljos_release",
                "ljos_remember",
                "ljos_rule",
                "ljos_sitting",
                "ljos_trust",
                "ljos_vote"
            ]
        );
    }

    /// A refusal is an error, not a success carrying an error message.
    #[tokio::test]
    async fn a_refusal_is_an_error() {
        let server = LjosServer::at(std::env::temp_dir());
        let Err(err) = server
            .ljos_evidence(Parameters(AccessionArgs {
                accession: "deed-file-nobody-minted-this".into(),
            }))
            .await
        else {
            panic!("a missing deed came back as an answer");
        };
        let said = format!("{err:?}");
        assert!(
            said.contains("not on PATH") || said.contains("exited") || said.contains("not found"),
            "{said}"
        );
    }

    /// Cards are the two named files; nothing is created; nothing else reads.
    #[tokio::test]
    async fn cards_are_read_and_never_made() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("USER.md"), "the user\n").expect("write");
        let server = LjosServer::at(dir.path().to_path_buf());
        let out = server
            .ljos_cards(Parameters(NoArgs {}))
            .await
            .expect("reads");
        assert!(out.0.text.contains("the user"));
        assert!(
            !dir.path().join("MEMORY.md").exists(),
            "a missing card was created"
        );

        assert_eq!(card_named("ljos://cards/USER.md"), Some("USER.md"));
        assert_eq!(card_named("ljos://cards/MEMORY.md"), Some("MEMORY.md"));
        assert_eq!(card_named("ljos://cards/NOTES.md"), None);
        assert_eq!(card_named("ljos://cards/../USER.md"), None);
        assert_eq!(card_named("file:///etc/passwd"), None);
    }

    /// The protocol is a resource every harness can read, and it names the
    /// tools in the order a sitting calls them.
    #[test]
    fn the_protocol_is_served_and_orders_the_sitting() {
        assert_eq!(card_named(PROTOCOL_URI), None, "the protocol is not a card");
        for verb in [
            "ljos doctor",
            "ljos cards",
            "ljos due",
            "ljos search",
            "ljos island",
            "ljos recall",
            "ljos claim",
        ] {
            assert!(PROTOCOL.contains(verb), "{verb} missing from the protocol");
        }
        let order: Vec<usize> = [
            "## Before the work",
            "## During the work",
            "## After the work",
        ]
        .iter()
        .map(|h| PROTOCOL.find(h).unwrap_or_else(|| panic!("{h} missing")))
        .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]));
        // Every tool the server declares is named in the protocol, so a reader
        // of the protocol has heard of everything the server can do.
        for tool in LjosServer::tool_router().list_all() {
            let verb = tool.name.trim_start_matches("ljos_").to_string();
            assert!(
                PROTOCOL.contains(&format!("`{verb}`"))
                    || PROTOCOL.contains(&format!("ljos {verb}")),
                "tool {verb} is not in the protocol"
            );
        }
    }

    /// Every prompt renders from what it declares.
    #[tokio::test]
    async fn every_prompt_renders() {
        fn text(message: &PromptMessage) -> &str {
            &message.content.as_text().expect("a text prompt").text
        }
        fn ordered(said: &str, verbs: &[&str]) {
            let at: Vec<usize> = verbs
                .iter()
                .map(|v| {
                    said.find(v)
                        .unwrap_or_else(|| panic!("{v} missing: {said}"))
                })
                .collect();
            assert!(
                at.windows(2).all(|w| w[0] < w[1]),
                "{verbs:?} out of order: {said}"
            );
        }
        let declared = LjosServer::prompt_router().list_all();
        let mut names: Vec<&str> = declared.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["check_a_handover", "run_a_panel", "start_a_sitting"]
        );
        let server = LjosServer::at(std::env::temp_dir());
        let panel = server
            .run_a_panel_prompt(Parameters(IssueArgs {
                issue: "proj-1a2b".into(),
            }))
            .await
            .expect("renders");
        let said = text(&panel[0]);
        assert!(said.contains("proj-1a2b"), "{said}");
        ordered(
            said,
            &[
                "`ljos_recall`",
                "`ljos_brief`",
                "`ljos_vote`",
                "`ljos_consensus`",
                "`ljos_learn`",
                "`ljos_calibrate`",
            ],
        );
        let server = LjosServer::at(std::env::temp_dir());
        let begun = server
            .start_a_sitting_prompt(Parameters(PickUpArgs {
                issue: Some("proj-1a2b".into()),
            }))
            .await
            .expect("renders");
        let said = text(&begun[0]);
        assert!(said.contains("proj-1a2b"), "{said}");
        ordered(
            said,
            &[
                "`ljos_cards`",
                "`ljos_due`",
                "`ljos_graded`",
                "`ljos_search`",
                "`ljos_island`",
                "`ljos_recall`",
                "`ljos_claim`",
                "`ljos_remember`",
                "`ljos_forget`",
                "`ljos_deed`",
                "`ljos_doctor`",
            ],
        );
        let checked = server
            .check_a_handover_prompt(Parameters(HandoverArgs {
                dir: "/tmp/bag".into(),
            }))
            .await
            .expect("renders");
        let said = text(&checked[0]);
        assert!(said.contains("/tmp/bag"), "{said}");
        ordered(said, &["`ljos_receive`", "`ljos_current`", "`import`"]);
    }
}
