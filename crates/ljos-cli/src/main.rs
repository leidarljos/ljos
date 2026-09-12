//! `ljos`: one seat over the habitats. It does not own them.

use anyhow::Result;
use clap::{Parser, Subcommand};
use ljos_cli::{
    ballots_from_json, cards, consensus_steps, format_hits, join, learn, on_path, packset_search,
    packset_write, policy_line, run, run_captured, trust_from_pack, write_trust, Trust, LEARN_BETA,
    POLICY_TCB,
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
    /// Standing knowledge. Remember: / Prefer: only. POST /v1/atoms.
    Remember {
        text: Vec<String>,
    },
    Prefer {
        text: Vec<String>,
    },
    Search {
        query: Vec<String>,
    },
    /// Frozen product.
    Evidence {
        accession: String,
    },
    Current {
        accession: String,
    },
    /// Graph and agreement.
    Deed {
        issue: String,
        #[arg(long)]
        add: Option<String>,
    },
    Recall {
        issue: String,
    },
    Vote {
        issue: String,
        #[arg(long = "for")]
        choice: Option<String>,
    },
    /// This-session work.
    Claim {
        node: String,
        #[arg(long)]
        assignee: String,
    },
    Complete {
        node: String,
        #[arg(long)]
        status: Option<String>,
    },
    /// Frozen working core. Read-only. Never extract-on-write.
    Cards {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Argv law. Not a store. Does not reload a pack.
    Policy {
        argv: Vec<String>,
    },
    /// DeGroot/Seldon over the pack's trust rows, then the tracker verb.
    Consensus {
        id: String,
    },
    /// One trust row: FROM weighs TO at WEIGHT in (0, 1]. Written to the pack.
    Trust {
        from: String,
        to: String,
        weight: f64,
        /// Deed accessions this row stands on.
        #[arg(long)]
        why: Vec<String>,
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
    match Cli::parse().cmd {
        Cmd::Remember { text } => {
            let body = packset_write("Remember", &join(&text))?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Cmd::Prefer { text } => {
            let body = packset_write("Prefer", &join(&text))?;
            println!("{}", serde_json::to_string_pretty(&body)?);
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
        Cmd::Vote { issue, choice } => match choice {
            Some(c) => run("vissue", &["vote", &issue, "--for", &c])?,
            None => run("vissue", &["vote", &issue])?,
        },
        Cmd::Claim { node, assignee } => {
            run("claimdag", &["claim", &node, "--assignee", &assignee])?
        }
        Cmd::Complete { node, status } => match status {
            Some(s) => run("claimdag", &["complete", &node, "--status", &s])?,
            None => run("claimdag", &["complete", &node])?,
        },
        Cmd::Cards { dir } => print!("{}", cards(&dir)?),
        Cmd::Policy { argv } => {
            eprintln!("ljos: {POLICY_TCB}");
            println!("{}", policy_line(&argv)?);
        }
        Cmd::Consensus { id } => {
            let trust = pack_trust_or_none();
            for step in consensus_steps(&id, on_path("ljos-consensus"), on_path("vissue"), &trust)?
            {
                run(step.bin, &step.args)?;
            }
        }
        Cmd::Trust {
            from,
            to,
            weight,
            why,
        } => {
            let row = Trust { from, to, weight };
            let v = write_trust(&row, &why)?;
            println!("{v}");
        }
        Cmd::Learn { id, outcome, beta } => {
            let said = run_captured("vissue", &["vote", &id, "--json"])?;
            let ballots = ballots_from_json(&said.stdout)?;
            let rows = learn(&ballots, &outcome, &trust_from_pack()?, beta)?;
            for row in &rows {
                write_trust(row, &[])?;
                println!("{} weighs {} at {:.3}", row.from, row.to, row.weight);
            }
        }
    }
    Ok(())
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
