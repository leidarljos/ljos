//! The seat as an agent surface.
//!
//! The command line's twelve verbs, none owned here: a pack write is an HTTP
//! call, everything else execs the habitat's own binary and hands back what
//! it said. Every tool says whether it writes; a habitat that refused is an
//! error; the cards are read-only resources under `ljos://cards/`; the
//! sequences that cross habitats are prompts. The pack is written only by
//! Remember and Prefer, and the text is the claim.

use std::path::{Path, PathBuf};

use ljos_cli::{
    ballots_from_json, cards, consensus_steps, doctor, due, graded, handover, learn, on_path,
    packset_search, packset_write, policy_line, receive, run_captured, trust_from_pack,
    write_trust, Trust, CARD_NAMES, LEARN_BETA, POLICY_TCB,
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
}

/// A question for the pack.
#[derive(Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// What to ask the seat's standing knowledge.
    pub query: String,
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

/// A ballot, or a request for the tally.
#[derive(Deserialize, JsonSchema)]
pub struct VoteArgs {
    /// The issue id.
    pub issue: String,
    /// `accept` or `reject`. Absent, the tool shows the tally.
    pub choice: Option<String>,
}

/// A session node to take.
#[derive(Deserialize, JsonSchema)]
pub struct TakeArgs {
    /// The node id, 32 hex.
    pub node: String,
    /// The assignee, 32 hex. One live claim per assignee.
    pub assignee: String,
}

/// A session node to finish.
#[derive(Deserialize, JsonSchema)]
pub struct FinishArgs {
    /// The node id, 32 hex.
    pub node: String,
    /// A terminal status. Defaults to the graph's own default.
    pub status: Option<String>,
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
}

/// An issue and what turned out right on it.
#[derive(Deserialize, JsonSchema)]
pub struct LearnArgs {
    /// The tracker id.
    pub issue: String,
    /// The option that turned out right.
    pub outcome: String,
    /// The factor a refuted voter shrinks by; the seat's default when absent.
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
        description = "Remember one lesson. The text is the claim and is stored as given: two short sentences at most, never a transcript. One of the pack's writes.",
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
        packset_write("Remember", &args.text)
            .map(Json)
            .map_err(refused)
    }

    #[tool(
        description = "Prefer one thing over another, as a standing preference. Stored as given. Another of the pack's writes.",
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
        packset_write("Prefer", &args.text)
            .map(Json)
            .map_err(refused)
    }

    #[tool(
        description = "What the seat knows, standing. Asks the pack; does not open a deed. An empty list means the pack holds nothing matching, and a failure means the writer is not running, which is a different thing: `packset ensure` starts one.",
        annotations(
            title = "Search the pack",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn ljos_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<Json<Vec<HitRow>>, McpError> {
        let hits = packset_search(&args.query).map_err(refused)?;
        Ok(Json(
            hits.into_iter()
                .map(|h| HitRow {
                    id: h.id,
                    kind: h.kind,
                    text: h.text,
                    score: h.score,
                })
                .collect(),
        ))
    }

    // ---- the deed store ----------------------------------------------------

    #[tool(
        description = "Whether a deed's bytes are intact and its sources are too. The deed store answers; an accession it does not hold is a failure, not an empty answer.",
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
        description = "Whether a deed is still the tip or a later take superseded it. A citation that resolves and is stale is worse than one that fails, because nothing complains.",
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
        description = "Cite a deed on a tracker node, or list what the node cites. A citation names the accession; it does not paste the product into the ticket. Citation is not a merge.",
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
        description = "The working set for a tracker node: its plan, what its inputs produced, and its own deeds. Read this before starting the work.",
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
        description = "Cast this identity's ballot on a node, or read the tally. One ballot per identity; a recast replaces. A tally is a count and is not the consensus model.",
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
            Some(c) => habitat("vissue", &["vote", &args.issue, "--for", c]),
            None => habitat("vissue", &["vote", &args.issue]),
        }
    }

    // ---- the session graph --------------------------------------------------

    #[tool(
        description = "Take a session node. One live claim per assignee; a second is refused with the node already held. Completing a node later does not close a ticket.",
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
        habitat(
            "claimdag",
            &["claim", &args.node, "--assignee", &args.assignee],
        )
    }

    #[tool(
        description = "Finish a session node. Completing is not closing: the ticket the node cites stays open until the tracker says otherwise.",
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
        match &args.status {
            Some(s) => habitat("claimdag", &["complete", &args.node, "--status", s]),
            None => habitat("claimdag", &["complete", &args.node]),
        }
    }

    // ---- cards, policy, consensus -------------------------------------------

    #[tool(
        description = "The cards: what the human froze. USER.md and MEMORY.md from the cards directory, read-only. A missing card prints nothing. Nothing here writes one.",
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
        description = "Argv law: print the line as it would run. This is not a check, and reloading a policy pack is not one either; grok-policyd is the trusted base when it exists.",
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
        description = "Settle agreement on a node: the consensus model first, DeGroot or Friedkin-Johnsen over the trust rows the pack holds, then the tracker's own verb. Not a vote count. With no rows every voter weighs the same.",
        annotations(title = "Consensus", read_only_hint = true, open_world_hint = false)
    )]
    async fn ljos_consensus(
        &self,
        Parameters(args): Parameters<IssueArgs>,
    ) -> Result<Json<Vec<Said>>, McpError> {
        let trust = trust_from_pack().unwrap_or_default();
        let steps = consensus_steps(
            &args.issue,
            on_path("ljos-consensus"),
            on_path("vissue"),
            &trust,
        )
        .map_err(refused)?;
        let mut out = Vec::new();
        for step in steps {
            let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
            out.push(habitat(step.bin, &args)?.0);
        }
        Ok(Json(out))
    }

    #[tool(
        description = "Write one trust row to the pack: from weighs to at weight in (0, 1], citing the deeds it stands on. Trust is memory: the row has a validity window and a later row for the same pair supersedes it.",
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
        };
        write_trust(&row, &args.why).map(Json).map_err(refused)
    }

    #[tool(
        description = "Reweigh the voters on an issue by what turned out right: every voter whose ballot the outcome refuted shrinks in every other voter's row (Hedge), a vindicated one keeps its weight. Writes the complete set of rows to the pack and returns them.",
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
    ) -> Result<Json<Vec<TrustRow>>, McpError> {
        let said = run_captured("vissue", &["vote", &args.issue, "--json"]).map_err(refused)?;
        let ballots = ballots_from_json(&said.stdout).map_err(refused)?;
        let held = trust_from_pack().map_err(refused)?;
        let rows = learn(
            &ballots,
            &args.outcome,
            &held,
            args.beta.unwrap_or(LEARN_BETA),
        )
        .map_err(refused)?;
        for row in &rows {
            write_trust(row, &[]).map_err(refused)?;
        }
        Ok(Json(
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
        description = "Which habitats answer: the binaries on PATH, the pack over PACKSET_URL, the deed store, the tracker, the claim graph. Ask this first when another verb fails.",
        annotations(title = "Doctor", read_only_hint = true, open_world_hint = false)
    )]
    async fn ljos_doctor(&self) -> Result<Json<Vec<HabitatRow>>, McpError> {
        Ok(Json(
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
        description = "Pack a slice of the seat for somebody else: the tracker's satchel for the projects and issues named, the pack's atoms (trust rows included), the deeds both cite with their receipts, sealed, and signed when the host has a key.",
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
    ) -> Result<Json<Vec<String>>, McpError> {
        handover(Path::new(&args.out), &args.projects, &args.issues)
            .map(Json)
            .map_err(refused)
    }

    #[tool(
        description = "Check a satchel that arrived: manifest, deed receipts against the head in the bag (and the bridge from a kept head when given), the signature, and what the atoms hold. With import, the atoms go into this seat's pack.",
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
    ) -> Result<Json<Vec<String>>, McpError> {
        receive(
            Path::new(&args.dir),
            args.since.as_deref().map(Path::new),
            args.import.unwrap_or(false),
        )
        .map(Json)
        .map_err(refused)
    }

    #[tool(
        description = "The claims whose review is due, soonest first. Read each; then grade it recalled or lapsed so the review clock moves.",
        annotations(title = "Due", read_only_hint = true, open_world_hint = false)
    )]
    async fn ljos_due(&self) -> Result<Json<Vec<DueRow>>, McpError> {
        Ok(Json(
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
        description = "Grade one review: recalled (default) pushes the next review out, lapsed brings it back sooner. Returns the atom with its new due_at.",
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
             Four habitats answer four different questions, and the order matters:\n\
             \n\
             1. `ljos_cards`. What the human froze. Read them first and leave them\n\
                as they are; if they are empty, they are empty.\n\
             2. `ljos_search` for what the seat already knows about this work. An\n\
                empty list means the pack holds nothing; a failure means the writer\n\
                is down, which is a different thing.\n\
             3. `ljos_recall` on the node. What it stands on, what its inputs\n\
                produced, and what it has cited so far.\n\
             4. `ljos_due` for the claims whose review is due; read each and\n\
                `ljos_graded` it, recalled or lapsed, so the clock moves.\n\
             5. `ljos_claim` a session node for it. One live claim per assignee.\n\
             \n\
             When something is learned that will still be true next sitting, say it\n\
             with `ljos_remember` in two short sentences. When the work produces\n\
             something, mint the deed in the deed store and `ljos_deed` it on the\n\
             node. Completing the session node does not close the ticket. A tool\n\
             that fails is a habitat refusing or down: `ljos_doctor` says which."
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
            "One seat over five habitats. The pack is written by remember, prefer, \
             trust, learn, graded and an imported handover; the text is the claim. \
             Cards are read at ljos://cards/ and \
             never written. Citing a deed on a node names an accession and does not \
             paste the product. Completing a session node does not close a ticket. \
             A tool that fails means a habitat refused or is not running; it is not \
             an empty answer.",
        )
    }

    /// The two cards, and nothing else.
    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(
            CARD_NAMES
                .iter()
                .map(|name| {
                    let mut resource =
                        Resource::new(format!("{SCHEME}://cards/{name}"), (*name).to_string());
                    resource.title = Some(format!("{name}, frozen by the human"));
                    resource.description = Some(
                        "A card. Read-only; overflow is an error, not a prompt to write more."
                            .into(),
                    );
                    resource.mime_type = Some("text/markdown".to_string());
                    resource
                })
                .collect(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri.clone();
        let name = card_named(&uri).ok_or_else(|| {
            McpError::resource_not_found(
                format!("{uri}: the cards are {SCHEME}://cards/USER.md and MEMORY.md"),
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

    /// Every tool is annotated, and the writers are the contract's eleven.
    #[test]
    fn the_writers_are_the_eleven_the_contract_names() {
        let tools = LjosServer::tool_router().list_all();
        assert_eq!(
            tools.len(),
            20,
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
                    assert_eq!(
                        hints.destructive_hint,
                        Some(false),
                        "{} destroys",
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
                "ljos_claim",
                "ljos_complete",
                "ljos_deed",
                "ljos_graded",
                "ljos_handover",
                "ljos_learn",
                "ljos_prefer",
                "ljos_receive",
                "ljos_remember",
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

    /// Every prompt renders from what it declares.
    #[tokio::test]
    async fn every_prompt_renders() {
        let declared = LjosServer::prompt_router().list_all();
        let mut names: Vec<&str> = declared.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["check_a_handover", "start_a_sitting"]);
        let server = LjosServer::at(std::env::temp_dir());
        let begun = server
            .start_a_sitting_prompt(Parameters(PickUpArgs {
                issue: Some("proj-1a2b".into()),
            }))
            .await
            .expect("renders");
        let text = format!("{:?}", begun[0].content);
        assert!(text.contains("proj-1a2b"), "{text}");
        assert!(text.contains("does not close the ticket"), "{text}");
        let checked = server
            .check_a_handover_prompt(Parameters(HandoverArgs {
                dir: "/tmp/bag".into(),
            }))
            .await
            .expect("renders");
        assert!(format!("{:?}", checked[0].content).contains("/tmp/bag"));
    }
}
