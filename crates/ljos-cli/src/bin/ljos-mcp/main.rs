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

/// Clients keep one server per thread for days, so an idle one must be
/// cheap. A tool's work runs on the blocking pool or blocks a worker for
/// one call, so two workers keep the stdin reader moving; the pool is
/// capped and its threads leave after a short idle.
fn main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(8)
        .thread_keep_alive(std::time::Duration::from_secs(2))
        .enable_all()
        .build()?
        .block_on(serve())
}

async fn serve() -> anyhow::Result<()> {
    ljos_cli::normalize_tracker_env();
    ljos_cli::load_seat_env();
    if let Some(line) = ljos_cli::cloud::missing_mcp_line() {
        eprintln!("{line}");
    }
    // One process is one connection. A later seat argument cannot mint a
    // second session for a ballot.
    ljos_cli::ballot_sig::pin_connection_session(None);
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("ljos-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    // Starting the server is a run of this version: refresh skills and
    // runner env files ljos wrote.
    ljos_cli::runner_skills::refresh_on_new_version();
    ljos_cli::runner_env::refresh_on_new_version();
    ljos_cli::bookkeep::spawn_idle_finish();
    ljos_cli::idle_drop::spawn_idle_drop();
    // Dropped when the process returns, including after the client closes.
    // A name, not `let _`, so the finish runs at the end of main.
    let _book_finish = ljos_cli::bookkeep::ShutdownFinish;
    // The order is the order the lines are read. rmcp then runs each call on
    // its own task, which the runtime may poll newest first.
    let (read, write) = stdio();
    let running = server::LjosServer::from_env()
        .serve((server::InboundOrder::new(read), write))
        .await?;
    let ended = running.waiting().await;
    ljos_cli::retire_seat(ljos_cli::runner_pid());
    ended?;
    Ok(())
}
