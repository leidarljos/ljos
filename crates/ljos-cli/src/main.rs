//! `ljos`: one seat over the habitats. It does not own them.

use anyhow::Result;
use clap::{Parser, Subcommand};
use ljos_cli::{
    ballots_from_json, calibrate, cards, claim, consensus_steps_anchored, doctor, due_report,
    finish, island_entities, learn_about, personas_from_pack, rows_about, run_as, topic_words,
    write_persona, Persona,
    format_doctor, format_hits, format_island, format_steps, graded, handover, healthy, join,
    learn, node_for, on_path, onboard, packset_forget, packset_island, packset_search,
    packset_write, policy_line, receive, release, run, run_captured, sitting, trust_from_pack,
    write_trust, Trust, HARNESSES_EXAMPLE, LEARN_BETA, POLICY_TCB, PROTOCOL,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ljos",
    about = "One seat over cards, packset, deedar, vissue, claimdag, and policyd"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Remember one lesson that will still be true next sitting. Two sentences at most.
    Remember { text: Vec<String> },
    /// Prefer one way over another, as a standing preference. Stored as written.
    Prefer { text: Vec<String> },
    /// Retire one atom by id. Tombstones it; the pack keeps the record.
    Forget {
        id: String,
        /// The deed accession that withdrew the claim. Refused if it is not one.
        #[arg(long)]
        why: Option<String>,
    },
    /// What the seat knows about a topic, ranked. Empty means the pack holds nothing on it.
    Search { query: Vec<String> },
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
        #[arg(long)]
        about: Vec<String>,
    },
    /// Take a session node for an issue. One live claim per assignee.
    Claim {
        /// A tracker id, or a 32-hex claim-graph id.
        node: String,
        /// Your name; mapped to one actor id.
        #[arg(long)]
        assignee: String,
    },
    /// Hand a session node back unfinished: ready again, generation moved.
    Release {
        /// A tracker id, or a 32-hex claim-graph id.
        node: String,
        /// The name that holds it.
        #[arg(long)]
        assignee: String,
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
    /// Argv law. Not a store. Does not reload a pack.
    Policy { argv: Vec<String> },
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
        #[arg(long)]
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
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        project: Vec<String>,
        #[arg(long)]
        issue: Vec<String>,
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
        /// Your name; one live claim per name.
        #[arg(long)]
        assignee: String,
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
    },
}

fn main() -> Result<()> {
    // A closed pipe ends the run quietly: `ljos learn | head` is not a panic.
    // SAFETY: resetting a signal disposition before any thread is spawned.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    match Cli::parse().cmd {
        Cmd::Remember { text } => {
            let body = packset_write("Remember", &join(&text))?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Cmd::Prefer { text } => {
            let body = packset_write("Prefer", &join(&text))?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Cmd::Forget { id, why } => {
            let body = packset_forget(&id, why.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Cmd::Island { cue, fire } => {
            print!("{}", format_island(&packset_island(&join(&cue), fire)?));
        }
        Cmd::Search { query } => {
            print!("{}", format_hits(&packset_search(&join(&query))?));
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
        Cmd::Claim { node, assignee } => print!("{}", claim(&node, &assignee)?),
        Cmd::Release { node, assignee } => print!("{}", release(&node, &assignee)?),
        Cmd::Complete { node, status } => match status {
            Some(s) => run("claimdag", &["complete", &node_for(&node)?, "--status", &s])?,
            None => run("claimdag", &["complete", &node_for(&node)?])?,
        },
        Cmd::Cards { dir } => print!("{}", cards(&dir)?),
        Cmd::Policy { argv } => {
            eprintln!("ljos: {POLICY_TCB}");
            println!("{}", policy_line(&argv)?);
        }
        Cmd::Consensus { id } => {
            // Rows scoped to a domain apply when the issue is about it; the
            // personas' anchors go to both settles.
            let topic = issue_topic(&id);
            let trust = rows_about(&pack_trust_or_none(), &topic);
            let personas = personas_from_pack().unwrap_or_default();
            for step in consensus_steps_anchored(
                &id,
                on_path("ljos-consensus"),
                on_path("vissue"),
                &trust,
                &personas,
            )? {
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
        } => {
            for line in handover(&out, &project, &issue)? {
                println!("{line}");
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
        } => print!("{}", sitting(&issue, &assignee, &cards_dir)?),
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
        Cmd::Learn { id, outcome, beta } => {
            let said = run_captured("vissue", &["vote", &id, "--json"])?;
            let ballots = ballots_from_json(&said.stdout)?;
            // The rows written are scoped to what the issue's island is
            // about, so a voter wrong here keeps its standing elsewhere.
            let about = island_entities(&id).unwrap_or_default();
            let rows = learn_about(&ballots, &outcome, &trust_from_pack()?, beta, &about)?;
            // Every row lands before any is printed, so a closed pipe cannot
            // leave the graph half written.
            for row in &rows {
                write_trust(row, &[])?;
            }
            for row in &rows {
                println!("{} weighs {} at {:.3}", row.from, row.to, row.weight);
            }
        }
    }
    Ok(())
}

/// The words an issue is about, from its title; none when the tracker does
/// not answer, which scopes nothing out.
fn issue_topic(id: &str) -> Vec<String> {
    run_captured("vissue", &["show", id, "--json"])
        .ok()
        .and_then(|said| serde_json::from_str::<serde_json::Value>(&said.stdout).ok())
        .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(topic_words))
        .unwrap_or_default()
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
