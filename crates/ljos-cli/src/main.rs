//! `ljos`: one seat over the habitats. It does not own them.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ljos_cli::{
    ballots_from_json, brief, calibrate, cards, claim, conflicts, consensus_steps_for, doctor,
    due_report, finish, format_consolidation, format_doctor, format_hits, format_hubs,
    format_island, format_steps, graded, handover, healthy, hook_call, hook_context,
    hook_output_ruled, island_entities, join, learn_anchors, learn_and_write, learn_shared,
    node_for, on_path, onboard, pack, packset_consolidate, packset_forget, packset_hubs,
    packset_island, packset_search_as_of, packset_write_as, panel, panel_steps, personas_from_pack,
    policy_with_memory, predictions_of, receive, release, rows_about, rules_from_pack, run, run_as,
    tcb_check,
    run_captured, seat_name, session_end, sitting, timeline, topic_words, trust_from_pack,
    verdict_for, write_persona, write_prediction, write_rule, write_trust, Persona, Rule, Trust,
    HARNESSES_EXAMPLE, LEARN_BETA, POLICY_TCB, PROTOCOL,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ljos",
    version,
    about = "One seat over cards, packset, deedar, vissue, claimdag, and policyd"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Remember one lesson that will still be true next sitting. Two sentences at most.
    Remember {
        text: Vec<String>,
        /// Remember as this persona: the lesson comes back to it first in its next brief.
        #[arg(long = "as")]
        as_persona: Option<String>,
    },
    /// Prefer one way over another, as a standing preference. Stored as written.
    Prefer {
        text: Vec<String>,
        /// Prefer as this persona.
        #[arg(long = "as")]
        as_persona: Option<String>,
    },
    /// Retire one atom by id. Tombstones it; the pack keeps the record.
    Forget {
        id: String,
        /// The deed accession that withdrew the claim. Refused if it is not one.
        #[arg(long)]
        why: Option<String>,
    },
    /// What the seat knows about a topic, ranked. Empty means the pack holds nothing on it.
    Search {
        query: Vec<String>,
        /// Most hits to print.
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: u32,
        /// Rerank the top hits with the writer's cross-encoder; slower, sharper.
        #[arg(long)]
        rerank: bool,
        /// Ask the pack as it stood then (YYYY-MM-DD or RFC 3339): what the seat knew at that time, withdrawn memories included, later ones left out.
        #[arg(long, value_name = "TIME")]
        as_of: Option<String>,
    },
    /// Candidate contradictions from the geometry of the seat's memory: the lowest passes between single memories, read by the optional `landscape` habitat.
    Conflicts {
        /// Most pairs to print.
        #[arg(short = 'n', long, default_value_t = 12)]
        limit: usize,
    },
    /// Consolidate the seat's memory: a later claim that rewrites an earlier one closes it. Reports the pairs; --apply writes.
    Consolidate {
        /// Write the closures. Without it the pairs are reported and nothing changes.
        #[arg(long)]
        apply: bool,
    },
    /// What this seat's memory turns on: the claims most linked to, by a weighted PageRank over the pack's links.
    Hubs {
        /// Most hubs to print.
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: usize,
    },
    /// The memories a task activates: search hits as seeds, spread along the pack's links.
    Island {
        cue: Vec<String>,
        /// The strongest eight fired together: their links gain weight.
        #[arg(long)]
        fire: bool,
    },
    /// Whether a deed's bytes are intact and its sources are too.
    Evidence { accession: String },
    /// Whether a deed is still the tip, or a later take superseded it.
    Current { accession: String },
    /// Cite a deed on an issue, or list what it cites. Citation is not a merge.
    Deed {
        issue: String,
        /// The accession to cite, from `deedar create`.
        #[arg(long)]
        add: Option<String>,
    },
    /// The working set for an issue: plan, its inputs' deeds, its own citations.
    Recall { issue: String },
    /// Cast this identity's ballot (VISSUE_AGENT), or read the tally with no --for.
    Vote {
        issue: String,
        /// The option to vote for.
        #[arg(long = "for")]
        choice: Option<String>,
        /// Cast as this persona instead of the seat's identity.
        #[arg(long = "as")]
        as_persona: Option<String>,
    },
    /// The brief a subagent playing a persona starts from: view, domains, what the seat knows there, the work.
    Brief {
        /// The persona's name.
        name: String,
        /// The tracker id of the issue.
        issue: String,
    },
    /// An issue's timeline: the tracker's logbook, the deeds it cites, and the memories it activates, one dated list oldest first with age and gap.
    Timeline {
        /// The tracker id of the issue.
        issue: String,
        /// Most events to print, the latest kept.
        #[arg(short = 'n', long, default_value_t = 60)]
        limit: usize,
    },
    /// A panel without MCP: one brief per persona written to a directory, then the settle line.
    Panel {
        /// The tracker id of the issue.
        issue: String,
        /// Where the briefs go, one `<persona>.md` each.
        #[arg(long, default_value = "panel")]
        out: PathBuf,
    },
    /// A voter with a view: NAME holds its ballot by ANCHOR in [0, 1] (0 never moves).
    Persona {
        name: String,
        /// How far the persona moves off its ballot in a settle; 1 is a plain voter.
        #[arg(long, default_value_t = 0.5)]
        anchor: f64,
        /// How this persona reads the work, in a sentence or two.
        #[arg(long)]
        view: String,
        /// Domains it speaks to; a trust row scoped to one of them applies when the issue is about it.
        #[arg(long, value_delimiter = ',')]
        about: Vec<String>,
    },
    /// Take a session node for an issue. One live claim per assignee.
    Claim {
        /// A tracker id, or a 32-hex claim-graph id.
        node: String,
        /// Your name; mapped to one actor id. Absent: LJOS_SEAT, else VISSUE_AGENT, else `seat`.
        #[arg(long)]
        assignee: Option<String>,
    },
    /// Hand a session node back unfinished: ready again, generation moved.
    Release {
        /// A tracker id, or a 32-hex claim-graph id.
        node: String,
        /// The name that holds it. Absent: LJOS_SEAT, else VISSUE_AGENT, else `seat`.
        #[arg(long)]
        assignee: Option<String>,
    },
    /// Finish a session node. Does not close the ticket.
    Complete {
        /// A tracker id, or a 32-hex claim-graph id.
        node: String,
        /// done (default), failed, or cancelled.
        #[arg(long)]
        status: Option<String>,
    },
    /// Frozen working core. Read-only. Never extract-on-write.
    Cards {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Forecast how the others will vote on an issue; two or more forecasts let the settle name the surprisingly popular answer.
    Predict {
        issue: String,
        /// The option you expect to win, or a JSON object of option to share.
        #[arg(long)]
        expect: String,
        /// Forecast as this persona instead of the seat's identity.
        #[arg(long = "as")]
        as_persona: Option<String>,
    },
    /// Argv law kept in the pack: a glob over the command line with a verdict the hook and `policy` enforce.
    Rule {
        /// A glob over the whole command line: `rm -rf *`, `*--force*`, `git push*`.
        pattern: String,
        /// deny stops the action; ask hands it to the person.
        #[arg(long, default_value = "ask")]
        verdict: String,
        /// The reason a stopped reader sees.
        #[arg(long)]
        why: String,
    },
    /// Argv law: the line as it would run, then what the pack knows that bears on it.
    Policy { argv: Vec<String> },
    /// The memory hook a runner or a policy layer calls before an action: reads the
    /// hook JSON (or plain text) on stdin, answers with the memories the action activates.
    Hook {
        /// Most memories to inject per call; each is injected once per session.
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// DeGroot/Seldon over the pack's trust rows, then the tracker verb.
    Consensus { id: String },
    /// One trust row: FROM weighs TO at WEIGHT in (0, 1]. Written to the pack.
    Trust {
        from: String,
        to: String,
        weight: f64,
        /// Deed accessions this row stands on.
        #[arg(long)]
        why: Vec<String>,
        /// Domains this row is scoped to; none means it applies everywhere.
        #[arg(long, value_delimiter = ',')]
        about: Vec<String>,
    },
    /// Which habitats answer. Exit 1 when a required one does not.
    Doctor,
    /// Print the sitting protocol: which store answers what, and the order of verbs.
    Protocol,
    /// Register ljos-mcp with an agent runner and install the protocol as its skill.
    Onboard {
        /// A runner named in ~/.config/ljos/harnesses.toml, or json to print the server entry.
        #[arg(long, default_value = "json")]
        harness: String,
        /// Report what would be written and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Print an example harnesses.toml and stop.
        #[arg(long)]
        example: bool,
    },
    /// Pack a slice of the seat: satchel, atoms, the deeds both cite; sealed and signed.
    Handover {
        /// Where to write the satchel.
        #[arg(long)]
        out: PathBuf,
        /// Projects to take whole.
        #[arg(long)]
        project: Vec<String>,
        /// Issues to take, with what they stand on.
        #[arg(long)]
        issue: Vec<String>,
        /// Copy the sealed satchel to another seat over ssh, as `user@host:path`; the receiver runs `ljos receive`.
        #[arg(long)]
        to: Option<String>,
    },
    /// Check a satchel that arrived; --import puts its atoms in this seat's pack.
    Receive {
        dir: PathBuf,
        /// A bridge file from the last handover by the same sender.
        #[arg(long)]
        since: Option<PathBuf>,
        #[arg(long)]
        import: bool,
    },
    /// Atoms whose review is due, then one line on the state of the clock.
    Due,
    /// Open a sitting on an issue in the protocol's order: doctor, cards, due, island, recall, claim.
    Sitting {
        /// The tracker id of the issue.
        issue: String,
        /// Your name; one live claim per name. Absent: LJOS_SEAT, else VISSUE_AGENT, else `seat`.
        #[arg(long)]
        assignee: Option<String>,
        /// Where the cards are read from.
        #[arg(long, default_value = ".")]
        cards: PathBuf,
    },
    /// Close a sitting: remember the lesson, fire the island, complete the node, learn from the outcome.
    Finish {
        /// The tracker id of the issue.
        issue: String,
        /// done, failed, or cancelled.
        #[arg(long, default_value = "done")]
        status: String,
        /// The lesson this sitting taught, two sentences at most.
        #[arg(long)]
        lesson: Option<String>,
        /// The option that turned out right, when the ballots are in and the world has said.
        #[arg(long)]
        outcome: Option<String>,
        /// The factor a refuted voter shrinks by when an outcome is named.
        #[arg(long, default_value_t = LEARN_BETA)]
        beta: f64,
    },
    /// Write trust rows from a project's voting history: Dawid-Skene accuracy per voter, no truth labels.
    Calibrate {
        /// The tracker project whose settled issues to read.
        #[arg(short, long)]
        project: String,
        /// Expectation-maximisation rounds.
        #[arg(long, default_value_t = 20)]
        rounds: usize,
    },
    /// Grade one review; recalled unless --lapsed.
    Graded {
        id: String,
        #[arg(long)]
        lapsed: bool,
    },
    /// Reweigh the voters on an issue by what turned out right.
    Learn {
        id: String,
        /// The option that turned out right.
        #[arg(long)]
        outcome: String,
        /// The factor a refuted voter shrinks by.
        #[arg(long, default_value_t = LEARN_BETA)]
        beta: f64,
        /// A fixed share of recovery toward one after the step, so a voter refuted long ago can come back; 0 is plain Hedge.
        #[arg(long, default_value_t = 0.0)]
        share: f64,
        /// `record` (the default): each voter's hits and misses so far as log-odds weights. `hedge`: refuted voters shrink by --beta, with --share recovery.
        #[arg(long, default_value = "record")]
        rule: String,
    },
}

fn main() -> Result<()> {
    // A closed pipe ends the run quietly: `ljos learn | head` is not a panic.
    // SAFETY: resetting a signal disposition before any thread is spawned.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    match Cli::parse().cmd {
        Cmd::Remember { text, as_persona } => {
            let body = packset_write_as("Remember", &join(&text), as_persona.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&body)?);
            if let Some(ids) = body["supersedes"].as_array().filter(|ids| !ids.is_empty()) {
                eprintln!(
                    "revises {} earlier memor{}, now closed: {}",
                    ids.len(),
                    if ids.len() == 1 { "y" } else { "ies" },
                    ids.iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        Cmd::Prefer { text, as_persona } => {
            let body = packset_write_as("Prefer", &join(&text), as_persona.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Cmd::Forget { id, why } => {
            let body = packset_forget(&id, why.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Cmd::Hubs { limit } => print!("{}", format_hubs(&packset_hubs(limit)?)),
        Cmd::Conflicts { limit } => print!("{}", conflicts(limit)?),
        Cmd::Consolidate { apply } => {
            print!("{}", format_consolidation(&packset_consolidate(apply)?))
        }
        Cmd::Island { cue, fire } => {
            print!("{}", format_island(&packset_island(&join(&cue), fire)?));
        }
        Cmd::Search {
            query,
            limit,
            rerank,
            as_of,
        } => {
            print!(
                "{}",
                format_hits(&packset_search_as_of(
                    &join(&query),
                    limit,
                    as_of.as_deref(),
                    rerank
                )?)
            );
        }
        Cmd::Evidence { accession } => run("deedar", &["evidence", &accession])?,
        Cmd::Current { accession } => run("deedar", &["current", &accession])?,
        Cmd::Deed { issue, add } => match add {
            Some(a) => run("vissue", &["deed", &issue, "--add", &a])?,
            None => run("vissue", &["deed", &issue])?,
        },
        Cmd::Recall { issue } => run("vissue", &["recall", &issue])?,
        Cmd::Vote {
            issue,
            choice,
            as_persona,
        } => match choice {
            Some(c) => run_as(
                "vissue",
                &["vote", &issue, "--for", &c],
                as_persona.as_deref(),
            )?,
            None => run("vissue", &["vote", &issue])?,
        },
        Cmd::Brief { name, issue } => print!("{}", brief(&name, &issue)?),
        Cmd::Timeline { issue, limit } => print!("{}", timeline(&issue, limit)?),
        Cmd::Panel { issue, out } => print!("{}", panel(&issue, &out)?),
        Cmd::Persona {
            name,
            anchor,
            view,
            about,
        } => {
            let body = write_persona(&Persona {
                name,
                anchor,
                view,
                entities: about,
            })?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Cmd::Claim { node, assignee } => {
            print!("{}", claim(&node, &assignee.unwrap_or_else(seat_name))?)
        }
        Cmd::Release { node, assignee } => {
            print!("{}", release(&node, &assignee.unwrap_or_else(seat_name))?)
        }
        Cmd::Complete { node, status } => match status {
            Some(s) => run("claimdag", &["complete", &node_for(&node)?, "--status", &s])?,
            None => run("claimdag", &["complete", &node_for(&node)?])?,
        },
        Cmd::Cards { dir } => print!("{}", cards(&dir)?),
        Cmd::Predict {
            issue,
            expect,
            as_persona,
        } => {
            let who = as_persona
                .or_else(|| std::env::var("VISSUE_AGENT").ok())
                .unwrap_or_else(whoami_tracker);
            let body = write_prediction(&issue, &who, &expect)?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Cmd::Rule {
            pattern,
            verdict,
            why,
        } => {
            let body = write_rule(&Rule {
                pattern,
                verdict,
                reason: why,
            })?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Cmd::Policy { argv } => {
            eprintln!("ljos: {POLICY_TCB}");
            print!("{}", policy_with_memory(&argv)?);
        }
        Cmd::Hook { limit } => {
            use std::io::Read;
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let call = hook_call(&input);
            // At the end of a session the memories it used fire together,
            // and there is nothing to say.
            if call.event == "SessionEnd" {
                session_end(call.session.as_deref());
                return Ok(());
            }
            // On a tool call the pack's rules give a verdict; on a prompt
            // there is nothing to stop, only something to know.
            let rules = if call.event == "PreToolUse" || call.event == "argv" {
                rules_from_pack().unwrap_or_default()
            } else {
                Vec::new()
            };
            let argv: Vec<String> = call.cue.split_whitespace().map(String::from).collect();
            let tcb_rule = tcb_check(&argv).and_then(|t| {
                t.starts_with("deny").then(|| Rule {
                    pattern: "ljos-policyd".into(),
                    verdict: "deny".into(),
                    reason: t.split('\t').nth(1).unwrap_or("tcb").to_string(),
                })
            });
            let verdict = tcb_rule.as_ref().or_else(|| verdict_for(&rules, &call.cue));
            print!(
                "{}",
                hook_output_ruled(&call, &hook_context(&call, limit), verdict)
            );
        }
        Cmd::Consensus { id } => {
            // Rows scoped to a domain apply when the issue is about it; the
            // personas' anchors go to both settles; the issue's tags pick
            // the model.
            let (topic, tags) = issue_topic_and_tags(&id);
            let trust = rows_about(&pack_trust_or_none(), &topic);
            let personas = personas_from_pack().unwrap_or_default();
            for step in consensus_steps_for(
                &id,
                on_path("ljos-consensus"),
                on_path("vissue"),
                &trust,
                &personas,
                &tags,
            )? {
                run(step.bin, &step.args)?;
            }
            // Beside the settle: the surprisingly popular answer when two
            // or more voters forecast, and the voters' standing when rows
            // exist.
            let predictions = pack()
                .and_then(|c| {
                    c.atoms_as_of(&c.workspace(), None)
                        .context("consensus: GET /v1/atoms failed")
                })
                .map(|atoms| predictions_of(&atoms, &id))
                .unwrap_or_default();
            for step in panel_steps(&id, on_path("ljos-consensus"), &trust, &predictions) {
                run(step.bin, &step.args)?;
            }
        }
        Cmd::Trust {
            from,
            to,
            weight,
            why,
            about,
        } => {
            let row = Trust {
                from,
                to,
                weight,
                about,
            };
            let v = write_trust(&row, &why)?;
            println!("{v}");
        }
        Cmd::Doctor => {
            let rows = doctor();
            print!("{}", format_doctor(&rows));
            if !healthy(&rows) {
                std::process::exit(1);
            }
        }
        Cmd::Protocol => print!("{PROTOCOL}"),
        Cmd::Onboard {
            harness,
            dry_run,
            example,
        } => {
            if example {
                print!("{HARNESSES_EXAMPLE}");
                return Ok(());
            }
            let steps = onboard(&harness, dry_run)?;
            print!("{}", format_steps(&steps));
            if steps.iter().any(|s| !s.ok) {
                std::process::exit(1);
            }
        }
        Cmd::Handover {
            out,
            project,
            issue,
            to,
        } => {
            for line in handover(&out, &project, &issue)? {
                println!("{line}");
            }
            if let Some(dest) = to {
                // The bag is sealed and signed before it moves; the copy is
                // the runner's scp, so the receiving seat's keys and hosts
                // apply as they would by hand.
                run_captured("scp", &["-rq", &out.display().to_string(), &dest])?;
                println!(
                    "copied to {dest}; there, `ljos receive {}`",
                    out.file_name()
                        .map_or_else(|| "DIR".into(), |n| n.to_string_lossy().into_owned())
                );
            }
        }
        Cmd::Receive { dir, since, import } => {
            for line in receive(&dir, since.as_deref(), import)? {
                println!("{line}");
            }
        }
        Cmd::Due => print!("{}", due_report()?),
        Cmd::Sitting {
            issue,
            assignee,
            cards: cards_dir,
        } => print!(
            "{}",
            sitting(&issue, &assignee.unwrap_or_else(seat_name), &cards_dir)?
        ),
        Cmd::Finish {
            issue,
            status,
            lesson,
            outcome,
            beta,
        } => print!(
            "{}",
            finish(&issue, &status, lesson.as_deref(), outcome.as_deref(), beta)?
        ),
        Cmd::Calibrate { project, rounds } => {
            let rows = calibrate(&project, rounds)?;
            for row in &rows {
                println!("{} weighs {} at {:.3}", row.from, row.to, row.weight);
            }
        }

        Cmd::Graded { id, lapsed } => {
            let v = graded(&id, !lapsed)?;
            println!("{}", v["due_at"].as_str().unwrap_or("graded"));
        }
        Cmd::Learn {
            id,
            outcome,
            beta,
            share,
            rule,
        } => {
            let said = run_captured("vissue", &["vote", &id, "--json"])?;
            let ballots = ballots_from_json(&said.stdout)?;
            // The rows written are scoped to what the issue's island is
            // about, so a voter wrong here keeps its standing elsewhere.
            let about = island_entities(&id).unwrap_or_default();
            let (rows, moved) = if rule == "hedge" {
                let rows =
                    learn_shared(&ballots, &outcome, &trust_from_pack()?, beta, &about, share)?;
                let moved = learn_anchors(&personas_from_pack()?, &ballots, &outcome, beta);
                for row in &rows {
                    write_trust(row, &[])?;
                }
                for p in &moved {
                    write_persona(p)?;
                }
                (rows, moved)
            } else {
                learn_and_write(&ballots, &outcome, beta, &about)?
            };
            for row in &rows {
                println!("{} weighs {} at {:.3}", row.from, row.to, row.weight);
            }
            for p in &moved {
                println!("{} now holds its ballot at anchor {:.3}", p.name, p.anchor);
            }
        }
    }
    Ok(())
}

/// The words an issue is about, from its title, and its tags; none of
/// either when the tracker does not answer, which scopes nothing out.
fn issue_topic_and_tags(id: &str) -> (Vec<String>, Vec<String>) {
    let Some(v) = run_captured("vissue", &["show", id, "--json"])
        .ok()
        .and_then(|said| serde_json::from_str::<serde_json::Value>(&said.stdout).ok())
    else {
        return (Vec::new(), Vec::new());
    };
    let topic = v
        .get("title")
        .and_then(|t| t.as_str())
        .map(topic_words)
        .unwrap_or_default();
    let tags = v
        .get("org_tags")
        .and_then(|t| t.as_array())
        .into_iter()
        .flatten()
        .filter_map(|t| t.as_str())
        .map(str::to_lowercase)
        .collect();
    (topic, tags)
}

/// The identity the tracker records ballots under, so a forecast and a
/// ballot from the same seat carry the same name.
fn whoami_tracker() -> String {
    run_captured("vissue", &["whoami"])
        .ok()
        .and_then(|s| s.stdout.lines().next().map(|l| l.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "seat".to_string())
}

/// The pack's rows, or none with a note: a seat without a pack still settles.
fn pack_trust_or_none() -> Vec<Trust> {
    match trust_from_pack() {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("ljos: no trust rows ({e:#}); settling with equal weights");
            Vec::new()
        }
    }
}
