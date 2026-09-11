//! `ljos`: one seat over the habitats. It does not own them.
//!
//! Cards are read-only. Consensus is a different crate (`ljos consensus`
//! execs `ljos-consensus` or `vissue consensus`). Policyd is argv law.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

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
    /// Standing knowledge. Remember: / Prefer: only.
    Remember { text: Vec<String> },
    Prefer { text: Vec<String> },
    Search { query: Vec<String> },
    /// Frozen product.
    Evidence { accession: String },
    Current { accession: String },
    /// Graph and agreement.
    Deed {
        issue: String,
        #[arg(long)]
        add: Option<String>,
    },
    Recall { issue: String },
    Vote {
        issue: String,
        #[arg(long = "for")]
        choice: Option<String>,
    },
    /// This-session work.
    Claim { node: String },
    Complete { node: String },
    /// Frozen working core. Read-only. Never extract-on-write.
    Cards {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Argv law. Not a store.
    Policy { argv: Vec<String> },
    /// DeGroot/Seldon. Other crate.
    Consensus { id: String },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Remember { text } => packset_write("Remember", &join(&text))?,
        Cmd::Prefer { text } => packset_write("Prefer", &join(&text))?,
        Cmd::Search { query } => run("packset", &["search", &join(&query)])?,
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
        Cmd::Cards { dir } => cards(&dir)?,
        Cmd::Policy { argv } => policy(&argv)?,
        Cmd::Consensus { id } => consensus(&id)?,
    }
    Ok(())
}

fn join(parts: &[String]) -> String {
    parts.join(" ")
}

fn packset_write(kind: &str, text: &str) -> Result<()> {
    if text.trim().is_empty() {
        bail!("{kind}: empty text is not a claim");
    }
    // packset CLI is inspect-only; the writer is HTTP POST /v1/atoms.
    // Until packset grows a remember verb, we print the law and the curl.
    eprintln!("ljos: write only on explicit {kind}:");
    println!("{kind}: {text}");
    if which::which("packset").is_ok() {
        let _ = run("packset", &["ensure"]);
    }
    Ok(())
}

fn cards(dir: &std::path::Path) -> Result<()> {
    for name in ["USER.md", "MEMORY.md"] {
        let p = dir.join(name);
        if p.is_file() {
            println!("--- {} ---", p.display());
            print!("{}", std::fs::read_to_string(&p)?);
        }
    }
    Ok(())
}

fn policy(argv: &[String]) -> Result<()> {
    if argv.is_empty() {
        bail!("policy: pass the argv to check");
    }
    // grok-policyd is the TCB. This process does not reload a pack as a check.
    eprintln!("ljos: argv law. grok-policyd is the TCB, not this process.");
    println!("{}", argv.join(" "));
    Ok(())
}

fn consensus(id: &str) -> Result<()> {
    if which::which("ljos-consensus").is_ok() {
        return run("ljos-consensus", &["settle", "--issue", id]);
    }
    if which::which("vissue").is_ok() {
        return run("vissue", &["consensus", id]);
    }
    bail!("neither ljos-consensus nor vissue is on PATH");
}

fn run(bin: &str, args: &[impl AsRef<str>]) -> Result<()> {
    let path = which::which(bin).with_context(|| format!("{bin} not on PATH"))?;
    let mut cmd = Command::new(path);
    for a in args {
        cmd.arg(a.as_ref());
    }
    let st = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !st.success() {
        bail!("{bin} exited {st}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::join;

    #[test]
    fn join_keeps_spaces() {
        assert_eq!(
            join(&["the default fuse".into(), "is CombMNZ".into()]),
            "the default fuse is CombMNZ"
        );
    }
}
