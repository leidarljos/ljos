//! `ljos`: one seat over the habitats. It does not own them.

use anyhow::Result;
use clap::{Parser, Subcommand};
use ljos_cli::{
    cards, consensus_steps, format_hits, join, on_path, packset_search, packset_write, policy_line,
    run, POLICY_TCB,
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
    },
    Complete {
        node: String,
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
    /// DeGroot/Seldon, then the tracker verb.
    Consensus {
        id: String,
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
        Cmd::Claim { node } => run("claimdag", &["claim", &node])?,
        Cmd::Complete { node } => run("claimdag", &["complete", &node])?,
        Cmd::Cards { dir } => print!("{}", cards(&dir)?),
        Cmd::Policy { argv } => {
            eprintln!("ljos: {POLICY_TCB}");
            println!("{}", policy_line(&argv)?);
        }
        Cmd::Consensus { id } => {
            for step in consensus_steps(&id, on_path("ljos-consensus"), on_path("vissue"))? {
                run(step.bin, &step.args)?;
            }
        }
    }
    Ok(())
}
