//! `ljos-mcp`: the one seat over the Model Context Protocol.
//!
//! Agents load this instead of the four habitat servers. Each habitat
//! keeps its own crate: a write goes to the pack over HTTP the way the command line's
//! does, and everything else execs the habitat's own binary and hands back
//! what it said. What this adds over execing `ljos` per call is the protocol
//! doing its job: a tool says whether it writes, an answer is typed, a habitat
//! that refused is an error rather than a success carrying the word "error",
//! the cards are resources, and the sequences that cross the habitats are
//! prompts a person can pick.

mod server;

use rmcp::{transport::stdio, ServiceExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ljos_cli::normalize_tracker_env();
    // One process is one connection. A later seat argument cannot mint a
    // second session for a ballot.
    ljos_cli::ballot_sig::pin_connection_session(None);
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("ljos-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    ljos_cli::bookkeep::spawn_idle_finish();
    // Dropped when the process returns, including after the client closes.
    // A name, not `let _`, so the finish runs at the end of main.
    let _book_finish = ljos_cli::bookkeep::ShutdownFinish;
    let running = server::LjosServer::from_env().serve(stdio()).await?;
    let ended = running.waiting().await;
    ljos_cli::retire_seat(ljos_cli::runner_pid());
    ended?;
    Ok(())
}
