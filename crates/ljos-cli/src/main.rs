//! `ljos`: one seat over the habitats. Each habitat keeps its own crate.

use anyhow::{bail, Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand};

mod help_groups;
mod shell_complete;
use ljos_cli::{
    age_of, begin_sitting_preclaim, brief, bump_plan, calibrate, cards, claim, claim_next,
    cli_assignee, compact_sitting_block, complete, conflicts, consensus_steps_for, copy_playbook,
    due_report, finish, forecasts_from_json, format_bump_rows, format_consolidation, format_doctor,
    format_findings, format_hubs, format_island, format_playbooks, format_readings,
    format_remembered, format_seat_command, format_steps, graded, habit, habits, handover_scoped,
    healthy, hook_note, island_entities, join, learn_anchors, learn_and_write, learn_reading,
    learn_shared, mark_seen, now_utc, on_path, onboard_to, packset_consolidate, packset_forget,
    packset_hubs, packset_island_as, packset_search_as_of, packset_write_as, panel, panel_steps,
    parse_every, personas_from_pack, playbooks_from_pack, policy_with_memory, post_hook_stdout,
    predictions_of, prompt_hook_stdout, protocol_brief, protocol_text, read_campaign, receive,
    release, remember_findings, reopen, rows_about, run, run_as, run_captured, settle_discount,
    sitting_close, sitting_stream_started, stop_hook_stdout, timeline, topic_words,
    tracker_show_json, trim_num, trust_from_pack, uncite_deed, with_discount, write_outcome,
    write_persona_as, write_prediction, write_rule, write_trust, Persona, PersonaWrite, Reading,
    Rule, Trust, HARNESSES_EXAMPLE, LEARN_BETA,
};
use std::io::IsTerminal;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ljos",
    version,
    about = "Memory, a work tracker and a command guard for coding runners",
    after_help = "A seat is the name in `LJOS_SEAT`: the person or agent this process acts as. \
                  Start with `ljos doctor`, `ljos prefer \"...\"`, `ljos policy -- COMMAND` and \
                  `ljos onboard`. `ljos protocol` explains the rest. \
                  `ljos help --all` lists every command.",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum MemoryCmd {
    /// Write this seat's memories to a local file. The file is not sent.
    ///
    /// A directory exits 1. `ljos memory export --out FILE` writes a file.
    /// When the lessons already in the file are the same, the file is left
    /// in place. The seed is the file this command writes.
    /// The line is `seed unchanged: N memories`.
    ///
    /// Example: `ljos memory export --out memories.json`
    Export {
        /// Where the file is written. `-` writes the file to stdout.
        #[arg(long)]
        out: PathBuf,
        /// Machine name in the file. The default is `LJOS_SYNC_HOST` when that is set, then the hostname.
        #[arg(long)]
        machine: Option<String>,
        /// Print `memories`, `unchanged` and `path` as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Read a portable memory file into this seat's import slot.
    ///
    /// A missing file or a directory exits 1.
    ///
    /// Example: `ljos memory import memories.json`
    Import {
        /// The file to read.
        path: PathBuf,
        /// Machine name when the file does not carry one.
        #[arg(long)]
        source_machine: Option<String>,
        /// Seat name when the file does not carry one.
        #[arg(long)]
        source_seat: Option<String>,
        /// Machine stored for this import, in place of the one the file names. The file bytes are not changed.
        #[arg(long)]
        as_machine: Option<String>,
        /// Print the counts as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Map rows in one import onto memories already stored.
    ///
    /// `new` means this import has one copy of that text. A text already
    /// stored is `held`. It does not post a text that is already stored.
    /// `--dry-run` writes nothing. Pass an import id or `--all`.
    /// `ljos memory imports` lists the ids. A bad id exits 1.
    ///
    /// Example: `ljos memory integrate --all --dry-run`
    Integrate {
        /// One import. The id is the input digest, or a unique prefix of it.
        import_id: Option<String>,
        /// Every import in this seat's slot.
        #[arg(long)]
        all: bool,
        /// Print the mappings and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Print one JSON document. The mappings stay, and the counts are added.
        #[arg(long)]
        json: bool,
    },
    /// List imports in this seat's slot.
    ///
    /// Example: `ljos memory imports`
    Imports {
        /// Print each import as JSON, the same rows as the text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum MemCmd {
    /// List memories newest first with their provenance. Unreviewed agents' memories by default.
    ///
    /// Example: `ljos memories list`
    List {
        #[command(flatten)]
        f: MemFilter,
        /// Print the list as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Review unreviewed memories: interactive on a terminal; with --keep/--drop/--edit/--merge, for scripts.
    ///
    /// Example: `ljos memories review`
    Review {
        #[command(flatten)]
        f: MemFilter,
        /// Print the memories as JSON and do not review them.
        #[arg(long)]
        json: bool,
        /// Keep this memory as it stands. Repeatable.
        #[arg(long, value_name = "ID")]
        keep: Vec<String>,
        /// Drop this memory. The text stays, marked removed, and later searches leave it out. Repeatable.
        #[arg(long = "drop", value_name = "ID")]
        drop_ids: Vec<String>,
        /// Replace this memory with the text in `--text`.
        #[arg(long, value_name = "ID")]
        edit: Option<String>,
        /// Merge these memories into one, with `--text` or their texts joined. Repeatable, two or more.
        #[arg(long, value_name = "ID")]
        merge: Vec<String>,
        /// Replacement text for `--edit`, or the text of the merged memory for `--merge`.
        #[arg(long)]
        text: Option<String>,
    },
    /// Browse, filter by seat or date, and prune memories in a terminal viewer.
    ///
    /// Example: `ljos memories tui`
    Tui {
        #[command(flatten)]
        f: MemFilter,
    },
    /// Name duplicates and memories that were never recalled. `--stale` is required.
    ///
    /// Without `--apply`, the command only lists them.
    #[command(after_help = "Example: `ljos memories prune --stale`")]
    Prune {
        /// Name duplicates, and memories never recalled in `--days`. Required.
        #[arg(long)]
        stale: bool,
        /// How many days without a recall counts as never recalled. The default is 30.
        #[arg(long, default_value_t = 30)]
        days: i64,
        /// Remove the memories the report names. Without this, the command only lists them.
        #[arg(long)]
        apply: bool,
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Args, Clone)]
struct MemFilter {
    /// Show only memories from this seat.
    #[arg(long)]
    seat: Option<String>,
    /// Show only memories written on or after this day, YYYY-MM-DD.
    #[arg(long)]
    since: Option<String>,
    /// Include memories a person has already reviewed. Without this, only unreviewed ones are shown.
    #[arg(long)]
    all: bool,
    /// Include memories the person wrote. Without this, only an agent's memories are shown.
    #[arg(long)]
    user: bool,
}

impl MemFilter {
    fn filter(&self) -> ljos_cli::memories::Filter {
        ljos_cli::memories::Filter {
            seat: self.seat.clone(),
            since: self.since.clone(),
            reviewed: self.all,
            user: self.user,
        }
    }
}

fn export_memories(out: &std::path::Path, machine: Option<&str>, json: bool) -> Result<()> {
    let said = ljos_cli::memory_port::export_cmd(out, machine, json)?;
    if out == std::path::Path::new("-") {
        eprint!("{said}");
    } else {
        print!("{said}");
    }
    Ok(())
}

fn memory_cmd(cmd: MemoryCmd) -> Result<()> {
    match cmd {
        MemoryCmd::Export { out, machine, json } => {
            export_memories(&out, machine.as_deref(), json)?
        }
        MemoryCmd::Import {
            path,
            source_machine,
            source_seat,
            as_machine,
            json,
        } => {
            print!(
                "{}",
                ljos_cli::memory_port::import_cmd(
                    &path,
                    source_machine.as_deref(),
                    source_seat.as_deref(),
                    as_machine.as_deref(),
                    json,
                )?
            );
        }
        MemoryCmd::Integrate {
            import_id,
            all,
            dry_run,
            json,
        } => {
            print!(
                "{}",
                ljos_cli::memory_port::integrate_cmd(import_id.as_deref(), all, dry_run, json)?
            );
        }
        MemoryCmd::Imports { json } => {
            print!("{}", ljos_cli::memory_port::imports_cmd(json)?);
        }
    }
    Ok(())
}

fn memories_cmd(cmd: MemCmd) -> Result<()> {
    use ljos_cli::memories as mem;
    let everything = mem::Filter {
        reviewed: true,
        user: true,
        ..mem::Filter::default()
    };
    match cmd {
        MemCmd::List { f, json } => {
            let shown = mem::list(&mem::live_atoms()?, &mem::reviews(), &f.filter());
            if json {
                println!("{}", serde_json::to_string_pretty(&shown)?);
            } else {
                print!(
                    "{}",
                    ljos_cli::scan::for_reader(&mem::list_page(&shown), human_color())
                );
            }
        }
        MemCmd::Review {
            f,
            json,
            keep,
            drop_ids,
            edit,
            merge,
            text,
        } => {
            let all = mem::list(&mem::live_atoms()?, &mem::reviews(), &everything);
            let scripted =
                !keep.is_empty() || !drop_ids.is_empty() || edit.is_some() || !merge.is_empty();
            if scripted {
                for id in &keep {
                    print!("{}", mem::keep(id, &all)?);
                }
                for id in &drop_ids {
                    print!("{}", mem::drop_memory(id, &all)?);
                }
                if let Some(id) = edit {
                    let text = text
                        .as_deref()
                        .context("memories review --edit needs --text")?;
                    print!("{}", mem::edit(&id, text, &all)?);
                } else if !merge.is_empty() {
                    print!("{}", mem::merge(&merge, text.as_deref(), &all)?);
                }
                return Ok(());
            }
            let shown: Vec<_> = all.into_iter().filter(|m| f.filter().admits(m)).collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&shown)?);
            } else if std::io::stdin().is_terminal() {
                mem::review_interactive(&shown)?;
            } else {
                for m in &shown {
                    print!("{}", mem::describe(m));
                }
            }
        }
        MemCmd::Tui { f } => mem::tui(&f.filter())?,
        MemCmd::Prune {
            stale,
            days,
            apply,
            json,
        } => {
            if !stale {
                bail!("memories prune: pass --stale");
            }
            let all = mem::list(
                &mem::live_atoms()?,
                &mem::reviews(),
                &mem::Filter {
                    reviewed: true,
                    ..mem::Filter::default()
                },
            );
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let found = mem::stale(&all, days, &today);
            if json {
                println!("{}", serde_json::to_string_pretty(&found)?);
            } else {
                for s in &found {
                    println!("{}  {}  {}", s.id.get(..8).unwrap_or(&s.id), s.why, s.text);
                }
                if !apply {
                    println!("{} stale; dry run, pass --apply to drop them", found.len());
                }
            }
            if apply {
                for s in &found {
                    print!("{}", mem::drop_memory(&s.id, &all)?);
                }
            }
        }
    }
    Ok(())
}

#[derive(Subcommand)]
enum CloudCmd {
    /// Install the release binaries, write the tool command a runner starts, and clone `LJOS_TRACKER_REMOTE`.
    ///
    /// That command is the Model Context Protocol (MCP) entry, the tool server the runner starts.
    ///
    /// Example: `ljos cloud up`
    Up {
        /// Download the release again.
        #[arg(long)]
        force: bool,
        /// Replace the repository an earlier `cloud up` recorded.
        #[arg(long)]
        repoint: bool,
    },
}

#[derive(Subcommand)]
enum MeshCmd {
    /// Join the private network and print this machine and the others on it.
    ///
    /// Headscale is the login server. On success, the line names this machine, its two addresses, and each peer. Any failure is one line on stderr and a non-zero status.
    ///
    /// Example: `ljos mesh up --key-file ~/.config/ljos/mesh.key`
    Up {
        /// Path of the join key. `LJOS_MESH_KEY_FILE` when omitted.
        #[arg(long, value_name = "PATH", env = "LJOS_MESH_KEY_FILE")]
        key_file: Option<PathBuf>,
        /// Headscale login server. `LJOS_MESH_LOGIN_SERVER` when omitted.
        /// The default is `https://hs.grund.rgoswami.me`.
        #[arg(long, value_name = "URL", env = "LJOS_MESH_LOGIN_SERVER")]
        login_server: Option<String>,
        /// Name of this node. The system hostname when omitted.
        #[arg(long, value_name = "NAME")]
        hostname: Option<String>,
    },
    /// Log out and stop the tailscaled `mesh up` started.
    ///
    /// Example: `ljos mesh down`
    Down,
    /// Print this node and the peers `tailscale status` reports, with their tags.
    ///
    /// Example: `ljos mesh status`
    Status,
    /// Print who on the private network may connect to whom.
    ///
    /// Headscale is the login server. The rules are the access policy `mesh.toml` implies.
    ///
    /// Example: `ljos mesh acl`
    Acl {
        /// Extra group names. Each one is given the same pair of connection rules.
        swarms: Vec<String>,
    },
    /// Read another machine's memory files into a seat.
    ///
    /// A lane is one machine's memory file, named `<lane>.jsonl`. The command keeps a copy of the rows it brought in. A missing directory, a path that is not a directory, or a memory store that stops the pull exits 1.
    ///
    /// Example: `ljos mesh pull --seat alice --from handoff`
    Pull {
        /// Seat that stores the memories.
        #[arg(long)]
        seat: String,
        /// Directory of `<lane>.jsonl` exports.
        #[arg(long, value_name = "DIR")]
        from: PathBuf,
    },
    /// Write a seat's memories into a handoff directory as one lane file.
    ///
    /// Example: `ljos mesh push --seat alice --to handoff`
    Push {
        /// Seat whose memories are written.
        #[arg(long)]
        seat: String,
        /// Directory that receives `<lane>.jsonl`.
        #[arg(long, value_name = "DIR")]
        to: PathBuf,
        /// File name, without `.jsonl`. `LJOS_SYNC_HOST`, then the hostname, when omitted.
        #[arg(long, value_name = "NAME")]
        lane: Option<String>,
    },
}

#[derive(Subcommand)]
enum RecordCmd {
    /// Check a directory of ballot records, or with `--tracker` every ballot under a tracker.
    ///
    /// Example: `ljos record check records`
    Check {
        /// Directory containing question and ballot JSON entries.
        #[arg(required_unless_present = "tracker", conflicts_with = "tracker")]
        dir: Option<PathBuf>,
        /// Verify the signature on every ballot under this tracker root, reading only. Exits 1 when one does not stand.
        #[arg(long, value_name = "ROOT")]
        tracker: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum SeatAction {
    /// Add a seat name to the tracker's seat list.
    ///
    /// The list is the `seats` table in the tracker's `vissue.toml`. A seat that is not on it cannot write once the list exists.
    ///
    /// Example: `LJOS_SEAT=alice ljos seat add alice`
    Add {
        /// The seat to add.
        name: String,
    },
    /// Record the owner's yes that a seat may close tickets it filed or holds, or the tickets named here.
    ///
    /// Example: `ljos seat grant bob close=own --quote yes --at 2026-11-13T12:00:00Z --via alice`
    Grant {
        /// The seat the grant names.
        seat: String,
        /// The grant, written close=own or close=ID[,ID...].
        #[arg(required_unless_present = "ids_file")]
        spec: Option<String>,
        /// Tickets this grant covers, one id per line.
        #[arg(long, value_name = "PATH")]
        ids_file: Option<PathBuf>,
        /// The owner's yes, quoted as they said it.
        #[arg(long)]
        quote: String,
        /// When the owner said it, as RFC 3339.
        #[arg(long)]
        at: String,
        /// The seat writing the grant down. It has to be this process's seat (`LJOS_SEAT`), not the seat the grant names.
        #[arg(long)]
        via: String,
    },
}

#[derive(Subcommand)]
enum ProposalCmd {
    /// Record a revised text on a proposal and run the readability checks, without accepting it. The original text stays.
    ///
    /// Example: `ljos proposals amend a6ae2751 --text "two sentences."`
    Amend {
        /// Id or prefix of the proposal.
        id: String,
        /// The revised text. `-` reads it from stdin.
        #[arg(long, value_name = "TEXT")]
        text: String,
    },
}

/// Flags of `ljos hook`. A separate args type so a hook call can parse
/// them without building every other command.
#[derive(Args, Debug)]
struct HookArgs {
    /// Most memories to inject per call; each is injected once between compactions.
    #[arg(long, default_value_t = 5)]
    limit: usize,
    /// The event, for a runner whose payload does not name it
    /// (`PreToolUse`, `PostToolUse`, `PreInvocation`, `Stop`).
    #[arg(long)]
    event: Option<String>,
    /// Read stdin as the person's prompt, including when it is plain text.
    /// Mail it shows is marked read.
    #[arg(long, conflicts_with = "event")]
    prompt: bool,
    /// With --prompt, print what the agent would be told as plain text
    /// instead of the runner's JSON.
    #[arg(long, requires = "prompt")]
    text: bool,
    /// Exit 1 only when the answer denies the cloud seat push rule.
    /// Any other rule stays a warning. Without this flag the exit stays 0,
    /// so a runner that reads the JSON is unchanged. A memory store that does not
    /// answer still exits 0: a hook must not fail the runner.
    #[arg(long)]
    fail_on_deny: bool,
    /// Read the payload and answer in this runner's hook contract: copilot,
    /// gemini, windsurf, factory, kiro, cline, qwen or crush.
    #[arg(long, conflicts_with_all = ["prompt", "fail_on_deny"])]
    runner: Option<String>,
    /// Book the commit git just created. The hook scripts pass this.
    /// `post-commit` reads no stdin. `post-rewrite` reads the old and new shas.
    #[arg(long, conflicts_with_all = ["prompt", "runner", "fail_on_deny"])]
    git_commit: bool,
    /// Which git hook called `--git-commit`: `post-commit` or `post-rewrite`.
    #[arg(long, requires = "git_commit")]
    git_hook: Option<String>,
    /// Print the SessionStart brief on stderr once for this conversation.
    #[arg(long, conflicts_with_all = ["prompt", "runner", "git_commit"])]
    brief_once: bool,
    /// Apply the bookkeeping that hook calls queued, and wait until none is left.
    #[arg(long, conflicts_with_all = ["prompt", "runner", "git_commit", "brief_once"])]
    drain: bool,
    /// With --drain, leave the work to a drainer that already runs.
    #[arg(long, requires = "drain", hide = true)]
    no_wait: bool,
    /// Apply these queued book jobs, in order. The drainer runs this.
    #[arg(long, num_args = 1.., hide = true, conflicts_with = "drain")]
    apply_book: Vec<std::path::PathBuf>,
    /// Finish a session off the runner's clock. The SessionEnd hook starts this.
    #[arg(
        long,
        hide = true,
        value_name = "FILE",
        conflicts_with_all = ["prompt", "runner", "git_commit", "brief_once", "drain"]
    )]
    finish_session: Option<std::path::PathBuf>,
    /// Evaluate one prompt judgment off the hook process so a late reply persists after exit.
    #[arg(
        long,
        hide = true,
        conflicts_with_all = ["prompt", "runner", "git_commit", "brief_once", "drain", "finish_session"]
    )]
    judge_worker: bool,
}

/// `ljos hook` on its own, so the process does not build every other command.
#[derive(Parser)]
#[command(name = "ljos", disable_help_flag = true, disable_version_flag = true)]
struct HookFast {
    #[command(subcommand)]
    cmd: HookFastCmd,
}

#[derive(Subcommand)]
enum HookFastCmd {
    #[command(disable_help_flag = true)]
    Hook(HookArgs),
}

/// The flags `ljos ready` and `ljos list` take.
#[derive(Args, Debug)]
struct ReadyArgs {
    /// Limit the list to tickets in one project.
    #[arg(short = 'p', long)]
    project: Option<String>,
    /// Print `shown`, `total` and `rows`. The same page and order as the text.
    #[arg(long, conflicts_with = "mine")]
    json: bool,
    /// The next unfinished item of each plan this seat holds.
    /// The sitting is the ticket this seat has open.
    /// That ticket is named when it is not a plan.
    #[arg(long, conflicts_with = "json")]
    mine: bool,
    /// How many tickets to print. `-n` is this flag. The default page prints 20.
    #[arg(short = 'n', long, value_name = "N", conflicts_with_all = ["all", "mine"])]
    limit: Option<usize>,
    /// Print every ticket on the page.
    #[arg(long, conflicts_with_all = ["limit", "mine"])]
    all: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Remember one lesson, stored as written.
    ///
    /// Typed by the person, it is theirs. Under a runner, words from the
    /// person's latest prompt are theirs; any other text is a standing
    /// lesson labelled agent-derived (an agent wrote it, and a person has
    /// not accepted it), until a review keeps it. With no seat named, the
    /// write uses the login name, or a short stable name when that login is
    /// missing, and the first such write says to set `LJOS_SEAT`.
    ///
    /// Example: `ljos remember "The lexical default is BM25+."`
    ///
    /// `-p PROJECT` records that project on the lesson. An unknown name exits 1 and names the projects.
    ///
    /// One or two sentences are one lesson. The sentence break is the pack's,
    /// so an abbreviation is not a second lesson. A longer text is stored as
    /// one claim per pair of sentences. The command says it was split and
    /// prints each stored id and its text.
    ///
    /// A long lesson may print a hint after it is saved. The hint names the
    /// sentence to change and the reading score. A short lesson prints no hint.
    ///
    /// An empty text exits 2. A text past 1000 characters exits 2.
    /// When the pack is not answering, the lesson is queued and the command exits 0.
    /// A busy pack is asked again for `LJOS_SEARCH_BUSY_WAIT` seconds (2 when unset, `0` does not wait).
    /// The wait follows `Retry-After` or a JSON `retry_ms`.
    /// When the pack stays busy, nothing was stored. The line says to run `ljos remember` again, and the command exits 1.
    /// Starting is waited out on `LJOS_SEARCH_WARMUP_WAIT` (5 seconds when unset, `0` does not wait), and not past `LJOS_CALL_TIMEOUT`.
    /// The pack is starting when `/health` is `not-ready` or `not ready`, or the JSON says `ready` is false or `code` is `starting`.
    /// Words in the lesson are not that signal.
    /// When that budget runs out the line is `` Memory is starting, so try again in a few seconds. If this persists, run `ljos doctor`. `` and the command exits non-zero.
    /// The pack is this seat's memory store.
    /// Turned off on purpose, the command exits 1.
    #[command(after_help = "Example: `ljos remember \"The lexical default is BM25+.\"`")]
    Remember {
        /// The lesson to store.
        text: Vec<String>,
        /// Remember as this persona: the lesson comes back to it first in its next brief.
        #[arg(long = "as")]
        as_persona: Option<String>,
        /// Store this as an episode. The hook shows it when a prompt bears on it.
        #[arg(long)]
        transient: bool,
        /// Store this as a standing rule now. The default.
        #[arg(long)]
        standing: bool,
        /// Print a warning when a sentence is over 29 words. The memory is still stored.
        #[arg(long)]
        verbose: bool,
        /// Print the stored lesson as JSON. The grade, the aim and the sentence that raised the grade are fields on it.
        #[arg(long)]
        json: bool,
        /// The day (YYYY-MM-DD) the claim starts to hold in the world. `ljos search --valid-at` leaves it out before then.
        #[arg(long, value_name = "DATE")]
        valid_from: Option<String>,
        /// The last day (YYYY-MM-DD) the claim holds. A plain search from the next day on leaves it out; `--valid-at` asks about another day.
        #[arg(long, value_name = "DATE")]
        valid_to: Option<String>,
        /// Record this project on the lesson.
        /// One name. A leading segment that repeats the tracker directory or its org prefix is dropped. Any other slash is refused.
        /// An unknown name exits 1 and names the projects.
        #[arg(short = 'p', long, value_name = "PROJECT")]
        project: Option<String>,
    },
    /// Save one standing preference for how work should be done.
    ///
    /// Stored at once from any caller. Typed by the person, it is theirs.
    /// Under a runner, only words from the person's latest prompt are theirs;
    /// any other text is agent-derived, meaning an agent wrote it and a person
    /// has not accepted it. `--user` writes a user rule instead: shared by every
    /// seat, never reviewed, and listed by `ljos rules` with whose it is.
    ///
    /// Example: `ljos prefer "CombMNZ over RRF for fusing two ballots."`
    Prefer {
        /// The preference to store.
        text: Vec<String>,
        /// Prefer as this persona.
        #[arg(long = "as")]
        as_persona: Option<String>,
        /// Also write a deny rule over this command pattern, with the preference as its reason, so the hook warns on what the preference forbids. Repeatable.
        #[arg(long, value_name = "PATTERN")]
        deny: Vec<String>,
        /// Also write an ask rule over this command pattern, with the preference as its reason. Repeatable.
        #[arg(long, value_name = "PATTERN")]
        ask: Vec<String>,
        /// Write a user rule in the user config, shared by every seat on this machine. It does not decay and it is not also stored as a seat preference.
        #[arg(long)]
        user: bool,
        /// Why this user rule stands. Required with `--user`.
        #[arg(long)]
        why: Option<String>,
        /// Copy this lesson into a user rule. The lesson stays stored. Pass `--user`.
        #[arg(long, value_name = "ID")]
        promote: Option<String>,
        /// Deny an outbound post whose body mentions this GitHub login. Repeatable. Used with `--user`.
        #[arg(long = "deny-mention", value_name = "HANDLE")]
        deny_mention: Vec<String>,
        /// Deny an outbound post whose body names this person. Repeatable. Used with `--user`.
        #[arg(long = "deny-name", value_name = "NAME")]
        deny_name: Vec<String>,
        /// Deny a reply aimed at this GitHub login. Repeatable. Used with `--user`.
        #[arg(long = "deny-reply-to", value_name = "HANDLE")]
        deny_reply_to: Vec<String>,
        /// Deny a reply aimed at this person. Repeatable. Used with `--user`.
        #[arg(long = "deny-reply-name", value_name = "NAME")]
        deny_reply_name: Vec<String>,
        /// Remove this user rule. The row stays as a tombstone so a sync does not bring it back. Pass `--user`.
        #[arg(long, value_name = "ID")]
        forget: Option<String>,
        /// Pull and push user rules through the tracker repository. Pass `--user`.
        #[arg(long)]
        sync: bool,
        /// List preferences stored before a basis was kept. With `--user`,
        /// list user rules stored before one. With `--keep`, keep one: as
        /// the person's from a terminal with no runner, else as the agent's.
        #[arg(long)]
        review: bool,
        /// Keep this older preference, or with `--user` this older user
        /// rule. From a terminal with no runner it is the person's; any
        /// other keep is the agent's, and `ljos rules` lists the rule as
        /// unverified. Pass with `--review`.
        #[arg(long, value_name = "ID", requires = "review")]
        keep: Option<String>,
    },
    /// Write this seat's memories to a file. The same as `ljos memory export`.
    ///
    /// Example: `ljos export --out memories.json`
    Export {
        /// Where the file is written. `-` writes the file to stdout.
        #[arg(long)]
        out: PathBuf,
        /// Machine name in the file. The default is `LJOS_SYNC_HOST` when that is set, then the hostname.
        #[arg(long)]
        machine: Option<String>,
        /// Print `memories`, `unchanged` and `path` as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Copy memories to a file, or bring a file from another machine into this seat.
    ///
    /// Example: `ljos memory export --out memories.json`
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
    },
    /// Review the memories agents wrote on their own: list, keep, edit, merge, drop, prune, or browse them in a terminal viewer.
    ///
    /// Example: `ljos memories list`
    Memories {
        #[command(subcommand)]
        cmd: MemCmd,
    },
    /// List lesson proposals waiting on a review.
    ///
    /// Open rows not already stored come first, then refused rows, then a
    /// count of how many open rows are new and how many are already stored.
    /// A refused row quotes the sentence and names `ljos proposals amend`.
    /// An empty queue does not ask the memory service. When that service is
    /// busy, the list is the local snapshot, and one line says so.
    ///
    /// Example: `ljos proposals`
    Proposals {
        #[command(subcommand)]
        cmd: Option<ProposalCmd>,
        /// Only proposals filed by this seat. `unknown` is a row that names none.
        #[arg(long)]
        seat: Option<String>,
        /// Keep only this status: open, accepted, refused, or already-in-pack. `in-pack` lists rows already stored. Unknown values exit 2.
        #[arg(long)]
        status: Option<String>,
        /// Mark each open row that is already stored as already-in-pack, and record the time. A second run writes nothing and deletes nothing.
        #[arg(long)]
        settle: bool,
    },
    /// Accept a stored memory or a waiting proposal, so a later search treats it as kept.
    ///
    /// One id or prefix, or several. One result line each: accepted, already stored, not found, or refused. A readability refusal quotes the sentence, after the value and the limit.
    ///
    /// Example: `ljos accept a6ae2751`
    Accept {
        /// Ids or prefixes. Each one gets its own result line.
        #[arg(required = true)]
        ids: Vec<String>,
        /// The person's own words accepting this lesson, as they said them. Needs --at and --via, and takes the place of a terminal of the person's own.
        #[arg(long)]
        quote: Option<String>,
        /// When the person said those words, RFC 3339, such as 2026-10-28T14:02:11Z.
        #[arg(long)]
        at: Option<String>,
        /// The seat that carried the words here.
        #[arg(long)]
        via: Option<String>,
    },
    /// Retire the note memories an older `ljos note` wrote. Runs once per seat; the tracker copies stay.
    ///
    /// Example: `ljos retire-notes`
    #[command(hide = true)]
    RetireNotes,
    /// Retire one memory by id. The text stays, marked retired.
    ///
    /// `ljos search` prints the id. Eight hex characters are enough, and
    /// more when several memories share them. `--why` names the stored file
    /// that withdrew it, an id from `deedar create`. Omit `--why` when there
    /// is no such file.
    ///
    /// Example: `ljos forget a6ae2751`
    Forget {
        /// The memory id `ljos search` prints. Eight hex characters are enough.
        id: String,
        /// Id of the stored file that withdrew the memory. Refused when `deedar` does not know it.
        #[arg(long)]
        why: Option<String>,
        /// Print the atom JSON. The default is one line: id, kind, due, text.
        #[arg(long)]
        json: bool,
    },
    /// Show what ljos remembers about a topic, best match first.
    ///
    /// Example: `ljos search "lexical default"`
    ///
    /// `-p PROJECT` keeps that project's tickets and lessons, and lessons that name no project.
    /// An unknown name exits 1 and names the projects.
    ///
    /// This seat's hits come first. Another seat's hit is kept when it names
    /// the subject, or when the encoder's cosine reaches 0.66. The cosine is
    /// how close the stored text is to the query, from 0 to 1. A generic word
    /// is not the subject. Another seat then needs a high similarity. A short list
    /// is not padded with a weaker cross-seat hit. When those were the only
    /// rows, the line is `Nothing relevant was found.` No match-mode line is
    /// printed on that empty page. A tracker ticket whose
    /// title matches is included, and then one whose body or notes match. An exact or
    /// near-exact title is the first row, while the encoder is loading and
    /// after. A ticket this seat filed, claimed, or noted, whose title, body
    /// or notes cover two query terms, is placed with this seat's rows. The
    /// newest note weighs more than an older one. A ticket in a project this
    /// seat works in comes before another project's ticket that only shares a
    /// generic word such as docs, release, or video. A ticket with no seat
    /// tag is not this seat's, unless the tracker roster names only this seat.
    /// The row names the ticket id and the title, so `ljos recall ID`
    /// opens it. The page names the place and the checks. It does not print
    /// the ranker score.
    /// `--explain` says how the rows are ordered. `--json` keeps the score.
    /// A search that found
    /// nothing prints no match-mode line. The line is `Nothing matched "QUERY"`.
    /// When a content word is shorter than QUERY, it adds
    /// `Try a shorter query, such as: ljos search WORD`. Stop words are
    /// skipped. The longest content word is the one named, and the earlier
    /// word wins a tie. The command is plain text, on its own line when the
    /// sentence would split it. A query that is already that word does not
    /// add the command. When nothing is stored, a shorter search would miss
    /// too, and the line names `ljos remember "LESSON"`.
    /// `--json` adds `suggestion`
    /// with the command the line names, and leaves the other fields as they
    /// are. A row that does not name a subject word is left out when its
    /// score is under `--min-score`. A score runs from 0 to 100. A row that only
    /// shares a stopword or a common weak word is left out. `--min-score 0`
    /// prints every row. When that floor removes every row, the line is
    /// `No relevant matches for "QUERY".` and it names `--min-score 0`.
    /// A meaning row from another seat that only shares a generic word is
    /// left out unless the encoder's similarity is high.
    /// A busy memory store is asked again for `LJOS_SEARCH_BUSY_WAIT` seconds (2 when unset, `0` does not wait).
    /// The wait follows `Retry-After` or a JSON `retry_ms`.
    /// When it answers, the rows are ranked by meaning. When it stays
    /// busy, has not opened after the start wait below, or is not running,
    /// the rows are ranked by words from the listing a hook last kept on
    /// this machine and this seat's memory slot, and a hit that names the
    /// subject stays. Search fell back, and one line on stderr says which
    /// of the three it was:
    /// `` Memory is busy, so these results are ranked by words. If this persists, run `ljos doctor`. ``
    /// It reads `starting` or `not running` in the other two. `ljos recall`
    /// and `ljos sitting` say the same line, once. While the encoder is
    /// still loading, a remaining time that fits in `LJOS_SEARCH_WARMUP_WAIT`
    /// seconds (5 when unset, `0` does not wait) is waited out, inside
    /// `LJOS_CALL_TIMEOUT`, and the search then ranks by meaning. The pack
    /// is starting when `/health` is `not-ready` or `not ready`, or the JSON
    /// says `ready` is false or `code` is `starting`. Words in the query are
    /// not that signal. That same budget is waited out once, however many
    /// times the search asks again. When it runs out, search ranks by words
    /// and says so on stderr. A dated search has no word fallback, so
    /// the line is `` Memory is starting, so try again in a few seconds. If this persists, run `ljos doctor`. `` and
    /// the command exits non-zero. A remaining
    /// time that is absent or longer than that budget keeps `LJOS_SEARCH_WAIT`
    /// (3 when unset). If the encoder is still not ready and rows
    /// are shown, the line says they are word matches and names the reported
    /// load time when one is sent. When nothing is shown, one line names the command
    /// to rerun and, when a remaining time is reported, about how many
    /// seconds are left. A failed or missing encoder
    /// names the command that fixes it. `--json` puts
    /// `semantic` or `lexical` in `ranking`. `--porcelain` prints one
    /// row per hit, the id, the kind and the text, separated by tabs, and
    /// leaves this page off. A receipt, a mail row and another seat's
    /// bookkeeping stay off the page, the JSON and the porcelain rows.
    /// A pack that is down ranks by words from what is stored locally,
    /// prints the `not running` line, and exits 0. A pack that stays busy
    /// ranks by words and prints the `busy` line. When no words match on a
    /// busy pack, the command exits 1.
    /// The pack is this seat's memory store.
    #[command(after_help = "Example: `ljos search \"lexical default\"`")]
    Search {
        /// Words to look for.
        query: Vec<String>,
        /// Most hits to print.
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: u32,
        /// Score the top hits again, slower and sharper, by reading the text itself.
        #[arg(long)]
        rerank: bool,
        /// Read the memory store as it stood at this time (YYYY-MM-DD or RFC 3339): what the seat knew then, withdrawn memories included, later ones left out.
        #[arg(long, value_name = "TIME")]
        as_of: Option<String>,
        /// What held in the world on this day (YYYY-MM-DD): claims whose validity window excludes it are left out. Without it, today; `--valid-at any` keeps every window.
        #[arg(long, value_name = "DATE")]
        valid_at: Option<String>,
        /// Open with how the rows are ordered and what the checks mean.
        #[arg(long)]
        explain: bool,
        /// Only memories exported by this machine. Applied before the limit.
        #[arg(long)]
        source_machine: Option<String>,
        /// Only memories from this source seat. Applied before the limit.
        #[arg(long)]
        source_seat: Option<String>,
        /// Only this import. The id is the input digest. Applied before the limit.
        #[arg(long)]
        import_id: Option<String>,
        /// Print JSON. The ranking is a field, and the mode line is left off stderr. An empty page may add `suggestion`.
        #[arg(long)]
        json: bool,
        /// Print one row per hit: id, kind and text, separated by tabs. The formatted page is left off.
        #[arg(long, conflicts_with = "json")]
        porcelain: bool,
        /// Include failed commands. They are stored as attempts and left out of the default list.
        #[arg(long)]
        attempts: bool,
        /// Lowest score to keep. 34 when omitted. 0 prints every row, including a weak word match.
        #[arg(long, value_name = "SCORE", default_value = "34")]
        min_score: f64,
        /// Only this project's tickets and lessons, plus lessons that name no project.
        /// One name. A leading segment that repeats the tracker directory or its org prefix is dropped. Any other slash is refused.
        /// An unknown name exits 1 and names the projects.
        #[arg(short = 'p', long, value_name = "PROJECT")]
        project: Option<String>,
    },
    /// List memories that contradict each other.
    ///
    /// The pairs are the closest memories that still disagree, read by the
    /// optional `landscape` program. That program is not part of this
    /// repository. When it is not on `PATH` the command says so and writes nothing.
    ///
    /// Example: `ljos conflicts`
    Conflicts {
        /// Most pairs to print.
        #[arg(short = 'n', long, default_value_t = 12)]
        limit: usize,
    },
    /// Close a memory that a later one rewrites.
    ///
    /// The pairs are printed. `--apply` writes the closures. Without it nothing changes.
    ///
    /// Example: `ljos consolidate`
    Consolidate {
        /// Write the closures. Without it the pairs are reported and nothing changes.
        #[arg(long)]
        apply: bool,
    },
    /// Copy this seat's memory to the other machines that share the tracker.
    ///
    /// The command pulls other machines' sealed logs, then writes and pushes this machine's.
    ///
    /// Example: `ljos sync`
    Sync {
        /// Print this machine's age public key (made on first use) for a scope's recipients, and stop.
        #[arg(long)]
        key: bool,
        /// Only pull and take other machines' logs.
        #[arg(long, conflicts_with = "export")]
        import: bool,
        /// Only write and push this machine's log.
        #[arg(long)]
        export: bool,
        /// Fold a gone host's atoms into this host's log and drop that host's log file.
        #[arg(long, value_name = "HOST", conflicts_with_all = ["key", "import", "export"])]
        adopt: Option<String>,
        /// Adopt a host whose log was committed within the last 14 days.
        #[arg(long, requires = "adopt")]
        force: bool,
        /// Print what --adopt would post, retire, and delete, and write nothing.
        #[arg(long, requires = "adopt")]
        dry_run: bool,
        /// Read HOST's logs from before the scope header: pin the blobs filed under HOST now, and import only those bytes as HOST's until HOST writes a headed log.
        #[arg(long, value_name = "HOST", conflicts_with_all = ["key", "import", "export", "adopt"])]
        adopt_legacy: Option<String>,
        /// Fold this machine's atom segments into one snapshot. A sync without this writes a segment only, and leaves every sealed file as it is.
        #[arg(long, conflicts_with_all = ["key", "adopt", "adopt_legacy"])]
        compact: bool,
    },
    /// Rebuild this machine's memory log when git finds a conflict in it.
    ///
    /// git calls this as the `ljos-atoms` merge driver (`%A` then `%P`).
    ///
    /// Example: `ljos merge-atoms current path`
    #[command(hide = true)]
    MergeAtoms {
        /// The file git keeps (`%A`).
        current: PathBuf,
        /// The path in the tracker (`%P`).
        path: String,
    },
    /// Merge one issue file when git finds two edits.
    ///
    /// git calls this as the `ljos-org` merge driver (`%O` `%A` `%B`). The merged text replaces `ours`.
    ///
    /// Example: `ljos merge-org base ours theirs`
    #[command(hide = true)]
    MergeOrg {
        /// The common ancestor (`%O`).
        base: PathBuf,
        /// The file git keeps (`%A`).
        ours: PathBuf,
        /// The other side (`%B`).
        theirs: PathBuf,
    },
    /// Install the release binaries and clone the shared tracker.
    ///
    /// Example: `ljos cloud up`
    Cloud {
        #[command(subcommand)]
        cmd: CloudCmd,
    },
    /// Join or leave the private network, print who may connect, or move memories through a handoff directory.
    ///
    /// Headscale is the login server. A lane is one machine's memory file.
    ///
    /// Example: `ljos mesh up --key-file ~/.config/ljos/mesh.key`
    Mesh {
        #[command(subcommand)]
        cmd: MeshCmd,
    },
    /// List the memories most other memories point at.
    ///
    /// The order is a weighted PageRank: a memory ranks higher when the
    /// memories that point at it are themselves highly linked. PageRank
    /// is that count of links.
    ///
    /// Example: `ljos hubs`
    Hubs {
        /// Most hubs to print.
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: usize,
        /// Print JSON. The human page is left off.
        #[arg(long)]
        json: bool,
    },
    /// List the memories a task wakes up.
    ///
    /// The words you give are looked up, and the command then walks the links from those memories to others.
    ///
    /// Example: `ljos island codec`
    Island {
        /// Words that name the task.
        cue: Vec<String>,
        /// Strengthen the links among the strongest eight memories.
        #[arg(long)]
        fire: bool,
        /// Use this persona's link weights. A strengthened link is saved for the persona, not the seat.
        #[arg(long = "as")]
        as_persona: Option<String>,
        /// Print JSON. The human page is left off.
        #[arg(long)]
        json: bool,
    },
    /// Check that a stored file is intact, and that the files it came from are too.
    ///
    /// The id names that stored file. `at=` is the produced time in ISO 8601
    /// with this machine's offset. `record -- COMMAND` runs the repo's test
    /// command and stores a passing record for the current tip, which
    /// `git push` requires.
    ///
    /// Example: `ljos evidence ACCESSION`
    Evidence {
        /// The stored file's id, or `record` followed by the test command.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true, num_args = 1..)]
        args: Vec<String>,
    },
    /// Check ballot records.
    ///
    /// Example: `ljos record check records`
    Record {
        #[command(subcommand)]
        cmd: RecordCmd,
    },
    /// Say whether a stored file is still the latest copy.
    ///
    /// A later copy supersedes it.
    ///
    /// Example: `ljos current ACCESSION`
    Current {
        /// The stored file's id.
        accession: String,
    },
    /// Name a stored file on a ticket, or list the ones named.
    ///
    /// The bytes stay in `deedar`, the program that keeps those files. Citing records the id.
    /// `--show ACCESSION` prints a stored file's bytes once `deedar evidence`
    /// passes and the copy in the store hashes to the digest it was stored
    /// under.
    /// A missing or altered copy exits 1 and prints nothing.
    ///
    /// Example: `ljos deed proj-1a2b`
    Deed {
        /// The issue: cited on, listed, or dropped from.
        #[arg(required_unless_present_any = ["ticket", "show"])]
        issue: Option<String>,
        /// Print the bytes of this stored file, checked first.
        #[arg(long, value_name = "ACCESSION", conflicts_with_all = ["issue", "add", "remove", "ticket"])]
        show: Option<String>,
        /// With `--show`, write the bytes to FILE instead of stdout.
        #[arg(long, value_name = "FILE", requires = "show")]
        out: Option<PathBuf>,
        /// An accession to cite, from `deedar create`; repeat for several.
        #[arg(long, conflicts_with = "remove")]
        add: Vec<String>,
        /// An accession to drop. Repeat for several. The removal is appended.
        #[arg(long)]
        remove: Vec<String>,
        /// The ticket `--remove` drops the accession from, in place of the positional issue.
        #[arg(long, conflicts_with = "issue")]
        ticket: Option<String>,
        /// Print only the new id. Everything else goes to stderr.
        #[arg(short = 'q', long = "porcelain")]
        porcelain: bool,
    },
    /// Keep a CSV table on a ticket, list its tables, or print one.
    ///
    /// `--add FILE` checks the CSV (a header row, the same number of
    /// columns on every row, UTF-8, at most 8 MiB), stores it in `deedar`
    /// and cites it on ISSUE in one tracker commit. Adding a name again
    /// makes the next version, which supersedes the one before, so
    /// `ljos current` on an older accession names the newer one. Bytes the
    /// latest version already has write nothing. With no flag, the latest
    /// version of each table is listed with its rows, columns, version and
    /// accession. `--show NAME` prints the latest version, `--version N` an
    /// older one, read and checked as `ljos deed --show` reads.
    ///
    /// Example: `ljos table proj-1a2b --add scores.csv --name scores`
    #[command(after_help = "Example: `ljos table proj-1a2b --add scores.csv --name scores`")]
    Table {
        /// The ticket the tables are cited on.
        issue: String,
        /// A CSV file to check, store and cite.
        #[arg(long, value_name = "FILE", conflicts_with = "show")]
        add: Option<PathBuf>,
        /// The table's name with `--add`; the file name without its extension when omitted.
        #[arg(long, requires = "add")]
        name: Option<String>,
        /// Print this table's latest version.
        #[arg(long, value_name = "NAME")]
        show: Option<String>,
        /// With `--show`, this version instead of the latest.
        #[arg(long, value_name = "N", requires = "show")]
        version: Option<u32>,
        /// With `--show`, print the CSV as it was stored. The default.
        #[arg(long, requires = "show", conflicts_with = "json")]
        csv: bool,
        /// Print JSON: the list, or the table's columns and rows.
        #[arg(long, conflicts_with = "add")]
        json: bool,
        /// Print the tracker and sync lines. Without it, a commit that landed
        /// is quiet. A refusal is still printed.
        #[arg(long)]
        verbose: bool,
    },
    /// File a ticket. The new id is the only line on stdout.
    ///
    /// A child of the issue this conversation holds, unless `--parent`
    /// names another or `--top` none, in the parent's project. `-p` naming
    /// another project files at its top level. A held issue the tracker
    /// does not have is dropped, and the ticket is filed at the top of the
    /// project. With no held issue, no `--parent`, no `-p`, and no project
    /// in checkout config or `LJOS_PROJECT`, the command exits 1 and names
    /// `-p NAME`. The heading and the path go
    /// to stderr. A commit that landed is not printed, and neither is a push
    /// that landed. A refused commit, a refused push, or a push that was
    /// skipped is one sentence naming the command to run. `--verbose` prints
    /// the tracker and sync lines. `--json` keeps those lines in fields.
    ///
    /// `--from FILE` files one ticket per row of a JSONL or CSV file with
    /// the columns title, body, tags, parent, priority and type, in one
    /// tracker commit, and prints the new ids one per line. `-p`, `-t`,
    /// `--tags`, `--priority` and `--parent` fill a column a row leaves
    /// empty, and each row is placed as `ljos file` places one: a row with
    /// a parent goes to that parent's project. A row that cannot be filed
    /// files none of them.
    ///
    /// Example: `LJOS_SEAT=alice ljos file "Pick a codec" -p demo`
    #[command(
        after_help = "Example: `LJOS_SEAT=alice ljos file \"Pick a codec\" -p demo`\nExample: `LJOS_SEAT=alice ljos file --from rows.jsonl -p demo`"
    )]
    File {
        /// The issue's title.
        #[arg(required_unless_present = "from")]
        title: Option<String>,
        /// File one ticket per row of this JSONL or CSV file (`-` reads
        /// stdin) in one tracker commit. Prints the new ids one per line.
        #[arg(
            long,
            value_name = "FILE",
            conflicts_with_all = ["title", "body", "options", "quorum", "deadline", "json"]
        )]
        from: Option<PathBuf>,
        /// The parent issue; the held issue when omitted.
        #[arg(long)]
        parent: Option<String>,
        /// No parent, even while an issue is held.
        #[arg(long, conflicts_with = "parent")]
        top: bool,
        /// The project; the parent's when omitted. One name. A leading
        /// segment that repeats the tracker directory or its org prefix is
        /// dropped. Any other slash is refused. A `--parent` files in
        /// its own project whatever this names. Another project than the
        /// held issue's files at its top level.
        #[arg(short, long)]
        project: Option<String>,
        /// The type tag: bug, task, feature, decision.
        #[arg(short = 't', long = "type")]
        kind: Option<String>,
        /// Comma-separated tags.
        #[arg(long)]
        tags: Option<String>,
        /// Priority stored on the ticket: A, B or C.
        #[arg(long)]
        priority: Option<String>,
        /// Body prose under the heading. A decision's body names `Options: A, B`.
        #[arg(long)]
        body: Option<String>,
        /// Comma-separated options. Two or more become the first body line `Options: A, B`.
        #[arg(long, value_name = "A,B")]
        options: Option<String>,
        /// Ballots the decision needs. With `--deadline`, `ljos reap --apply`
        /// lapses the vote once the deadline passes short of it.
        #[arg(long, value_name = "N", requires = "deadline")]
        quorum: Option<usize>,
        /// RFC 3339 time the ballots are due, with `--quorum`.
        #[arg(long, value_name = "RFC3339", requires = "quorum")]
        deadline: Option<String>,
        /// The id is already the only line on stdout. Kept for scripts
        /// that pass it.
        #[arg(short = 'q', long = "porcelain", alias = "quiet", alias = "id-only")]
        porcelain: bool,
        /// Print the tracker and sync lines. Without it, a commit that landed
        /// is quiet, and so is a push that landed. A refusal is still printed.
        #[arg(long)]
        verbose: bool,
        /// Print `id`, `text`, `tracker_git` and `sync` as JSON. `text` is
        /// what a person sees. The other fields keep the commit and sync lines.
        #[arg(long)]
        json: bool,
    },
    /// Append `Options: A, B` to an issue. Two or more, separated by commas.
    ///
    /// Example: `ljos options proj-1a2b keep,drop`
    Options {
        /// The issue id.
        issue: String,
        /// The options, separated by commas.
        options: String,
    },
    /// Open a panel for a decision prompt: file or reuse the issue, sit it, and start one headless member per brief.
    ///
    /// Example: `ljos open-panel --prompt-file prompt.txt --log panel.log`
    #[command(hide = true)]
    OpenPanel {
        /// The prompt, already written to a file so the hook can return.
        #[arg(long)]
        prompt_file: PathBuf,
        /// Where the opener writes what it did.
        #[arg(long)]
        log: PathBuf,
        /// The repository the members read.
        #[arg(long)]
        cwd: Option<String>,
        /// The conversation that asked, so a second opener is not a second panel.
        #[arg(long)]
        session: Option<String>,
    },
    /// Note progress on an issue, dated, and commit the tracker.
    ///
    /// The note stays on the logbook. It is not written into memory, so
    /// recall on a later task does not hand it back as something the seat knows.
    ///
    /// `ljos note TEXT...` notes on the issue this conversation holds.
    /// With none open, it exits 1 and names `ljos sitting ISSUE`.
    /// An empty id exits 1 and writes nothing.
    /// `ljos note ID TEXT...` notes on ID. When ID is not that issue, the
    /// note is still written and stderr names both. `--quiet` leaves that
    /// line off. A first word that is an id `vissue create` mints is the
    /// id; any other first word is part of the note.
    ///
    /// Example: `ljos note "the door seats when it is cold"`
    #[command(after_help = "Example: `ljos note \"the door seats when it is cold\"`")]
    Note {
        /// The note, or an issue id and then the note.
        #[arg(value_name = "WORDS", required = true, num_args = 1..)]
        words: Vec<String>,
        /// Print the tracker and sync lines. Without it, a commit that landed
        /// is quiet, and so is a push that landed. A refusal is still printed.
        #[arg(long)]
        verbose: bool,
        /// Print `text`, `tracker_git` and `sync` as JSON. `text` is what a
        /// person sees. The other fields keep the commit and sync lines.
        #[arg(long)]
        json: bool,
        /// Leave off the line that names another issue.
        #[arg(long)]
        quiet: bool,
    },
    /// List one seat's tracker commits.
    ///
    /// The subject is `chore(issues): ID VERB by SEAT`.
    ///
    /// Example: `ljos log --seat alice`
    Log {
        /// The seat whose commits to list.
        #[arg(long)]
        seat: String,
    },
    /// Count the tracker commits each seat made since a day.
    ///
    /// Example: `ljos usage --since 2026-11-01`
    Usage {
        /// YYYY-MM-DD. Commits from that day on.
        #[arg(long)]
        since: String,
    },
    /// Show what every seat did, newest first.
    ///
    /// The list covers tickets, votes, consensus, stored files, claims, the command guard, and memory.
    ///
    /// Example: `ljos digest`
    Digest {
        /// A day (`YYYY-MM-DD`), a time, or `last` (since the previous digest).
        #[arg(long)]
        since: Option<String>,
        /// Print one JSON document. JSON does not move the marker.
        #[arg(long)]
        json: bool,
        /// Move the marker. A plain digest does this. JSON does not, unless this is set.
        #[arg(long)]
        mark: bool,
    },
    /// Write to one seat (`SEAT TEXT...`), or to a group with `--group NAME`.
    ///
    /// Example: `ljos send bob "the codec is chosen"`
    Send {
        /// The seat and the message, or with `--group` the message alone.
        #[arg(value_name = "WORDS", required = true, num_args = 1..)]
        words: Vec<String>,
        /// Write to every other member of this group.
        #[arg(long)]
        group: Option<String>,
        /// Show this ahead of other mail on the next prompt.
        #[arg(long)]
        interrupt: bool,
        /// The vissue issue this message is about. A reply keeps it.
        #[arg(long)]
        issue: Option<String>,
        /// Write to a seat that has not stored a memory yet. Without it, an
        /// unknown seat is refused, and the line names the seats already known.
        #[arg(long)]
        new_seat: bool,
    },
    /// List mail this seat has not read.
    ///
    /// Each message shown is marked read, and a receipt is written.
    /// `ljos inbox --all` shows mail already read. `ljos reply` still
    /// answers it. `ljos hook --prompt` writes a receipt for each message
    /// it shows. A memory store that does not answer is an error and the exit is
    /// non-zero. A memory store that runs out of time prints `pack busy, try again`
    /// and exits 75. The pack is this seat's memory store.
    ///
    /// Example: `ljos inbox`
    Inbox {
        /// Include mail this seat has already read.
        #[arg(long)]
        all: bool,
        /// Fetch the tracker and import its sealed logs before listing.
        /// `COMMIT` fetches until the remote has that commit, for up to
        /// `LJOS_INBOX_SYNC_WAIT` seconds (30). Without it, use
        /// `LJOS_WAKE_COMMIT` when that is set, otherwise the remote head.
        /// A commit the remote does not have imports the remote head. No
        /// remote: list only.
        #[arg(
            long,
            value_name = "COMMIT",
            num_args = 0..=1,
            default_missing_value = ""
        )]
        sync: Option<String>,
    },
    /// Write a read receipt for one message.
    ///
    /// Example: `ljos read 1`
    Read {
        /// The id `send` printed.
        id: String,
    },
    /// Answer a message. The reply goes to its sender and keeps its issue.
    ///
    /// Example: `ljos reply 1 agreed`
    Reply {
        /// The id `send` printed.
        id: String,
        /// The reply; several words are one reply.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
        /// Show this ahead of other mail on the next prompt.
        #[arg(long)]
        interrupt: bool,
    },
    /// List a group of seats, or change who is in it.
    ///
    /// With `--add` or `--remove`, change the group. With neither, list it.
    ///
    /// Example: `ljos group review --add alice`
    Group {
        /// The group's name.
        name: String,
        /// A seat to add. Repeat for several.
        #[arg(long)]
        add: Vec<String>,
        /// A seat to drop. Repeat for several.
        #[arg(long)]
        remove: Vec<String>,
    },
    /// Set how an idle seat is told that mail arrived.
    ///
    /// The wake is a command this machine runs, or a webhook it posts to.
    /// `ljos send` and `ljos reply` start it and do not wait. A name ending
    /// in `-` matches every longer seat that starts with it. Alone, show
    /// the seat's wake.
    ///
    /// Example: `ljos wake alice`
    Wake {
        /// The seat. Absent: this seat. A name ending in `-` is a prefix:
        /// mail to a longer seat that starts with it uses this wake unless
        /// that seat has its own. The longest prefix wins.
        seat: Option<String>,
        /// Run this shell line, with `LJOS_SEAT` set to the seat, when mail
        /// reaches it. The guard judges it now and on every run.
        #[arg(long, conflicts_with_all = ["webhook", "clear", "fire"])]
        command: Option<String>,
        /// POST the message's envelope (never its body) to this URL, such as a runner's webhook. https, or http on localhost, an IPv4 loopback address, or ::1.
        #[arg(long, conflicts_with_all = ["clear", "fire"])]
        webhook: Option<String>,
        /// A file holding the webhook's bearer key.
        #[arg(long, requires = "webhook")]
        key_file: Option<String>,
        /// An environment variable holding the webhook's bearer key.
        #[arg(long, requires = "webhook")]
        key_env: Option<String>,
        /// Seconds before a wake command is stopped (default 30).
        #[arg(long, requires = "command")]
        timeout: Option<u64>,
        /// Remove the seat's wake.
        #[arg(long, conflicts_with = "fire")]
        clear: bool,
        /// Run the seat's wake now, as a send does, and print what happened.
        #[arg(long)]
        fire: bool,
        /// With --fire: the seat the message is from.
        #[arg(long, requires = "fire")]
        from: Option<String>,
        /// With --fire: the message id.
        #[arg(long, requires = "fire")]
        msg: Option<String>,
        /// With --fire: the issue the message is about.
        #[arg(long, requires = "fire")]
        issue: Option<String>,
        /// With --fire: the message is an interrupt.
        #[arg(long, requires = "fire")]
        interrupt: bool,
    },
    /// Record that a ticket waits on another, or drop that edge.
    ///
    /// `ljos block ISSUE OTHER` is `ljos block ISSUE --on OTHER`.
    /// Further ids are further tickets it waits on.
    /// `--off ID` drops that edge.
    /// A following id beside `--on` or `--off` is refused.
    ///
    /// Example: `ljos block proj-1a2b proj-9c3d`
    Block {
        /// The ticket that waits.
        issue: String,
        /// A ticket it waits on. The same as `--on`. Repeat for several.
        #[arg(value_name = "OTHER")]
        also: Vec<String>,
        /// The ticket it waits on.
        #[arg(long, value_name = "ID", conflicts_with = "off")]
        on: Option<String>,
        /// Drop the edge to this ticket.
        #[arg(long, value_name = "ID", conflicts_with = "on")]
        off: Option<String>,
    },
    /// Move a ticket under another parent ticket.
    ///
    /// The write changes only that ticket's parent link, records a dated
    /// note on the ticket, and commits the tracker.
    ///
    /// Example: `ljos parent proj-1a2b proj-9c3d`
    Parent {
        /// The ticket to move.
        issue: String,
        /// The new parent ticket.
        parent: String,
    },
    /// Show one ticket, or one memory.
    ///
    /// A ticket id prints the full ticket page: its id, status, priority
    /// and full title wrapped to 80 columns, its claim seat and age when
    /// claimed, its parent and children, the tickets it waits on, the full
    /// body, and the eight most recent notes, newest last. Each note carries
    /// its time, age and the seat that wrote it. A longer log names how many
    /// notes are earlier and the command that prints the rest. `--all`
    /// prints every note. `--verbose` is the same flag.
    /// A memory id prints that memory. The id may be the full atom id, the
    /// short prefix `ljos search` and `ljos remember` print, or the `local:`
    /// form. The page wraps the text to 80 columns and names the kind, the
    /// level, the origin, the seat, the session, when it was written, whether
    /// it is accepted, retired or superseded, the tickets it cites, and the
    /// memories it links to.
    /// An unknown id exits 1 and says it is neither a ticket nor a memory.
    /// An id that matches more than one memory lists those ids.
    /// An id that matches more than one ticket exits 1 and names one `ljos show` command.
    ///
    /// Example: `ljos show proj-1a2b`
    Show {
        /// A ticket id, or a memory id.
        issue: String,
        /// Print one JSON object with claim, parent, children, blockers, body and every note. The human page is left off.
        #[arg(long)]
        json: bool,
        /// Print every note. `--verbose` is the same flag.
        #[arg(long, visible_alias = "verbose")]
        all: bool,
    },
    /// List tickets that are open and not waiting on another open ticket.
    ///
    /// A ticket this seat holds comes first.
    /// Unclaimed tickets follow, ordered by higher priority and then the
    /// newest note, claim or filing.
    /// Another seat's claims follow, with claims untouched for
    /// `LJOS_STALE_DAYS` (7 when unset) last.
    /// The id breaks the last tie.
    /// The page prints the first 20, headed `ready  shown X of Y`.
    /// `-n N` prints N tickets. `--limit N` is the same flag. `--all` prints every ticket.
    /// A limited page ends with one line naming `--limit`, `--all`, `-p` and `--mine`.
    /// `--json` prints `shown`, `total` and `rows` for that same page and that same order.
    /// `--mine` names the next unfinished item of each plan this seat holds.
    /// When the open sitting's ticket is not a plan, the line names that ticket and says it has no plan items.
    /// `ljos list` and `ljos ls` are the same command.
    ///
    /// Example: `ljos ready`
    Ready(ReadyArgs),
    /// List the open tickets. The same command as `ljos ready`.
    ///
    /// `list` takes the flags `ready` takes and prints the page `ready` prints.
    /// A ticket this seat holds comes first, then higher priority, then the newest note, claim or filing.
    /// The page prints the first 20, headed `ready  shown X of Y`.
    /// `-n N` prints N tickets. `--limit N` is the same flag. `--all` prints every ticket.
    /// `-p PROJECT` keeps one project.
    /// `--json` prints `shown`, `total` and `rows` for that same page and that same order.
    /// `--mine` names the next unfinished item of each plan this seat holds.
    /// When the open sitting's ticket is not a plan, the line names that ticket and says it has no plan items.
    /// `ls` is the short spelling.
    ///
    /// Example: `ljos list -p proj`
    #[command(visible_alias = "ls")]
    List(ReadyArgs),
    /// Show who holds which ticket.
    ///
    /// This is `vissue claims`, with its flags. `--mine` lists this seat's
    /// claims that have had no note, stored file or finish for a day.
    /// `--stale` lists the ones untouched for longer than `--days`
    /// (`LJOS_STALE_DAYS`, 7 when unset), oldest first.
    ///
    /// Example: `ljos claims`
    Claims {
        /// This seat's claims with no note, stored file or finish for a day.
        #[arg(long)]
        mine: bool,
        /// This seat's claims untouched for `--days` or longer.
        #[arg(long, conflicts_with = "mine")]
        stale: bool,
        /// Days untouched before a claim is stale. The default is `LJOS_STALE_DAYS`, or 7.
        #[arg(long, value_name = "N", requires = "stale")]
        days: Option<i64>,
        /// Print the stale claims as a JSON array. With neither `--mine` nor `--stale`, passed to `vissue claims`.
        #[arg(long)]
        json: bool,
        /// Only claims held by this name. Passed to `vissue claims`.
        #[arg(long, conflicts_with = "agent")]
        by: Option<String>,
        /// Same as `--by`. Passed to `vissue claims`.
        #[arg(long)]
        agent: Option<String>,
        /// Only claims in this project. Passed to `vissue claims`.
        #[arg(short = 'p', short_alias = 'P', long)]
        project: Option<String>,
        /// Further arguments passed to `vissue claims`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show the plan, the cited files and the notes for one ticket.
    ///
    /// That set is the working set. A graphify `graph.json` in the repository adds a code context section.
    ///
    /// Example: `ljos recall proj-1a2b`
    ///
    /// When no lesson is stored on the issue, the line names `ljos remember`.
    /// Free text or an argument without a ticket id exits 2 and points at `ljos search`.
    /// Several unquoted words exit 2 and print `ljos search "WORDS"`.
    /// A busy memory store is asked again for `LJOS_SEARCH_BUSY_WAIT` seconds (2 when unset, `0` does not wait).
    /// The wait follows `Retry-After` or a JSON `retry_ms`.
    /// When it stays busy, the rows are ranked by words and one line says the memory pack was busy.
    /// The pack is this seat's memory store.
    /// When nothing is storing memories, the command exits 75 when the line names `packset ensure`, and exits 1 when packset is not on PATH.
    Recall {
        /// The ticket id.
        issue: String,
        /// Print JSON. The ranking is a field, and the mode line is left off stderr.
        #[arg(long)]
        json: bool,
        /// Leave out the graphify code context section.
        #[arg(long)]
        no_graph: bool,
    },
    /// Release claims idle past a window and mail each seat. Without `--apply` it prints what would happen.
    ///
    /// Example: `ljos reap`
    Reap {
        /// How long a claim may sit with no claim or tracker commit: 6h, 90m, 2d.
        #[arg(long, default_value = "6h")]
        idle: String,
        /// Release and mail. Absent: print what would happen.
        #[arg(long)]
        apply: bool,
    },
    /// Cast this seat's vote, or show the tally when `--for` is omitted.
    ///
    /// The tracker records the voter as `VISSUE_AGENT`, which is this seat's name.
    ///
    /// Example: `ljos vote proj-1a2b --for keep --used none`
    Vote {
        /// The ticket to vote on. With no `--for`, the tally is printed.
        issue: String,
        /// The option to vote for.
        #[arg(long = "for")]
        choice: Option<String>,
        /// Probability in (0, 1] that the choice is the outcome. Recorded;
        /// `ljos learn` scores it once the outcome is named, and no settle
        /// or push cite weighs it.
        #[arg(long)]
        confidence: Option<f64>,
        /// Ids of stored files this ballot used, comma-separated, or `none`.
        /// Required when `--for` is set.
        #[arg(long)]
        used: Option<String>,
        /// The option you expect the others to pick, or a JSON object of
        /// option to share. Same command as the ballot, so the surprisingly
        /// popular reading has a forecast without a second verb.
        #[arg(long)]
        expect: Option<String>,
        /// The persona this ballot records. The casting seat and the parent
        /// session are stored with it. Consensus counts the ballot.
        /// A standing seat or the person is the caster's persona ballot.
        /// With `--jev`, the persona Jev answers for.
        #[arg(long = "as")]
        as_persona: Option<String>,
        /// Ask Jev for the persona's ballot (needs `--as`, not `--for`):
        /// cast with its confidence and forecast when it is sure, handed to
        /// a subagent when it is not.
        #[arg(long, requires = "as_persona", conflicts_with = "choice")]
        jev: bool,
        /// Take back this seat's ballot and its forecast: a ballot cast on
        /// the wrong issue or with no basis stops counting, and the issue's
        /// logbook keeps what it was.
        #[arg(long, conflicts_with_all = ["choice", "jev", "confidence", "used", "expect", "against", "child"])]
        withdraw: bool,
        /// Vote no. The text is why. A no ballot is cast only with `--child`
        /// naming an open child of this issue (`ljos file --parent ISSUE`).
        #[arg(long, conflicts_with_all = ["choice", "jev"], value_name = "REASON")]
        against: Option<String>,
        /// The open child a no ballot links. Filed with `ljos file --parent ISSUE`.
        #[arg(long, value_name = "ID", conflicts_with_all = ["choice", "jev"])]
        child: Option<String>,
    },
    /// Print the brief a subagent playing a persona starts from.
    ///
    /// The brief is that persona's view, its domains, what the seat knows there, and the work.
    ///
    /// Example: `ljos brief editor proj-1a2b`
    Brief {
        /// The persona's name.
        name: String,
        /// The tracker id of the issue.
        issue: String,
    },
    /// List an issue's history, oldest first.
    ///
    /// The list joins the tracker's logbook, the stored files the issue cites, and the memories it wakes. Each line has its age and the gap since the one before.
    ///
    /// Example: `ljos timeline proj-1a2b`
    Timeline {
        /// The tracker id of the issue.
        issue: String,
        /// Most events to print, the latest kept.
        #[arg(short = 'n', long, default_value_t = 60)]
        limit: usize,
        /// Print JSON. The human page is left off.
        #[arg(long)]
        json: bool,
    },
    /// Write one brief per persona for an issue, or start those personas.
    ///
    /// `ljos panel ISSUE` writes the briefs. `ljos panel run ISSUE` starts each designer and persona in its own runner session. `--cloud` writes one prompt file per persona and starts nothing. MCP, the tool server a runner starts, is not required.
    ///
    /// Example: `ljos panel proj-1a2b`
    Panel {
        /// The issue id. `run` spawns the panel (`ljos panel run ISSUE`).
        issue: String,
        /// The issue id when the first argument is `run`.
        #[arg(value_name = "ISSUE")]
        run_issue: Option<String>,
        /// Where the briefs go, one `<persona>.md` each. With `--cloud`, the prompt files.
        #[arg(long, default_value = "panel")]
        out: PathBuf,
        /// Ask Jev for every ballot first; only the personas it is unsure
        /// for get a brief.
        #[arg(long)]
        jev: bool,
        /// Which runner starts the panel. Overrides `LJOS_PANEL_HARNESS`.
        #[arg(long)]
        harness: Option<String>,
        /// How many designers `run` spawns. Each files one candidate from the brief alone.
        #[arg(long, default_value_t = 3)]
        designers: usize,
        /// Print the sessions and prompts and do not spawn.
        #[arg(long)]
        dry_run: bool,
        /// Write one prompt file per persona for a cloud seat: the seat
        /// environment, the slot and the push rules. Does not spawn.
        #[arg(long, conflicts_with_all = ["jev", "harness", "designers", "dry_run"])]
        cloud: bool,
    },
    /// Define a persona, a voter that keeps its own view.
    ///
    /// NAME holds its ballot by ANCHOR in [0, 1]. 0 never moves off that view.
    ///
    /// Example: `ljos persona editor --anchor 0.5 --view "reads the diff first"`
    Persona {
        /// Name of the voter.
        name: String,
        /// How far the persona moves off its ballot in a settle; 1 is a plain voter.
        #[arg(long, default_value_t = 0.5)]
        anchor: f64,
        /// How this persona reads the work, in a sentence or two.
        #[arg(long, required_unless_present = "retire")]
        view: Option<String>,
        /// Domains it speaks to, comma-separated, repeated, or several after one flag; a trust row scoped to one of them applies when the issue is about it.
        #[arg(long, value_delimiter = ',', num_args = 1..)]
        about: Vec<String>,
        /// The runner that thinks as this persona in a session it keeps:
        /// a runner named in harnesses.toml.
        #[arg(long)]
        runner: Option<String>,
        /// Take over a name another seat, or another holder of this seat, defined.
        #[arg(long)]
        replace: bool,
        /// Say what the define would do (free, yours, refused, taken over, and which personas share its view or domains) and write nothing.
        #[arg(long, conflicts_with = "retire")]
        check: bool,
        /// Withdraw the persona, so it casts no more ballots. Only the conversation that defined it may; --replace retires one someone else defined.
        #[arg(long, conflicts_with_all = ["view", "about", "runner"])]
        retire: bool,
    },
    /// Install a published release next to this ljos.
    ///
    /// The ljos archive is checked against its sha256, then each other seat
    /// program comes from the repository that publishes it. A missing release
    /// or a bad download is a warning and the rest still move. The old binary
    /// is kept as NAME.bak-pre-VERSION.
    ///
    /// Example: `ljos upgrade`
    Upgrade {
        /// The ljos release; the newest when absent. Other programs take their own latest release.
        version: Option<String>,
        /// Install here instead of beside the running ljos.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Move this one program.
        #[arg(long)]
        only: Option<String>,
    },
    /// Say whether a runner that exited should start again.
    ///
    /// The line is `restart`, `stop` or `give-up`. On a restart it also prints the herdr line that would start the child. It prints the line and does not run it.
    ///
    /// Example: `ljos supervise worker --harness codex --exit clean --at 0`
    #[command(hide = true)]
    Supervise {
        /// The child's name. The herdr agent is `persona-NAME`.
        name: String,
        /// The runner this child runs.
        #[arg(long)]
        harness: String,
        /// `permanent`, `transient`, or `temporary`.
        #[arg(long, default_value = "permanent")]
        restart: String,
        /// Restarts allowed inside the window before the supervisor gives up.
        #[arg(long, default_value_t = 3)]
        max_restarts: u32,
        /// The intensity window, in seconds.
        #[arg(long, default_value_t = 60)]
        window: u64,
        /// `clean` or `crash`.
        #[arg(long)]
        exit: String,
        /// Seconds since the epoch. The window is read from this clock.
        #[arg(long)]
        at: u64,
        /// Earlier exits already counted, comma-separated seconds.
        #[arg(long, default_value = "")]
        failures: String,
        /// The persona home herdr starts in.
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        /// The script herdr runs.
        #[arg(long, default_value = "run.sh")]
        script: PathBuf,
    },
    /// Hand a persona a question or a task in its own session, opening its pane when it is closed.
    ///
    /// Example: `ljos ask editor "which codec"`
    Ask {
        /// The persona to ask.
        name: String,
        /// What to ask, in words.
        text: Vec<String>,
    },
    /// List each persona: name, anchor, domains, view, and who defined it.
    ///
    /// Example: `ljos personas`
    Personas,
    /// Write every persona as an agent a runner can start by name.
    ///
    /// The file is `ljos-NAME.md` in each runner's agents directory. The runner starts it as a subagent that casts that persona's ballot before it reads the others.
    ///
    /// Example: `ljos agents`
    Agents {
        /// Write one runner's agents only. The name is a runner in harnesses.toml, or one ljos ships. Absent: every runner in harnesses.toml that has an agents directory.
        #[arg(long)]
        harness: Option<String>,
        /// Report what would be written and removed, and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Print one status line for a runner: the seat, the ticket it holds, and what is due.
    ///
    /// The runner's status JSON is read on stdin. A build runner reads this from `[ui.status_line]`. A terminal runner reads it from `statusLine`.
    ///
    /// Example: `ljos statusline`
    #[command(hide = true)]
    Statusline,
    /// Bind a playbook to an issue and copy its recipe into the working set, before personas enter.
    ///
    /// Example: `ljos playbook proj-1a2b sit`
    Playbook {
        /// The tracker id of the issue.
        issue: String,
        /// The recipe name: sit, arena, land, company-panel, overnight, feature.
        name: String,
    },
    /// List the recipes on offer: sit, arena, land, company-panel, overnight, feature.
    ///
    /// Example: `ljos playbooks`
    Playbooks,
    /// Show where a parent ticket's workflow stands, and the next step.
    ///
    /// `--start` puts the parent under a workflow and files its design child. `--round` files the next panel round or a blocking fix. A round count does not close the parent.
    ///
    /// Example: `ljos workflow proj-1a2b`
    Workflow {
        /// The tracker id of the parent issue.
        issue: String,
        /// Put the parent under this workflow (feature) and file its design child. Running it again files nothing new.
        #[arg(long, value_name = "NAME", conflicts_with = "round")]
        start: Option<String>,
        /// File the next panel round, or one `fix:` for a blocking finding. A nit stays a note. The parent stays open. A person closes it.
        #[arg(long)]
        round: bool,
    },
    /// Take a ticket so this seat is the one working on it.
    ///
    /// One live claim per name. The tracker stores that claim as a session
    /// node, which is its record of who holds the ticket.
    ///
    /// When another seat has the ticket, the refusal says whether that holder
    /// looks stale: the claim quiet at least `LJOS_LEASE_SECS` (900 seconds
    /// when unset) with no running conversation named for it on this host.
    /// `--takeover` takes a stale claim over, notes the old holder on the
    /// ticket, and refuses a holder that is running or active within the
    /// lease, changing nothing.
    ///
    /// Example: `LJOS_SEAT=alice ljos claim demo-2f5j`
    Claim {
        /// A tracker id, or a 32-hex claim-graph id. Omit when claiming next.
        #[arg(value_parser = named_node)]
        node: Option<String>,
        /// Take the ticket over from a holder that looks stale. Needs an id.
        #[arg(long, requires = "node", conflicts_with = "next")]
        takeover: bool,
        /// Your name; mapped to one actor id. Absent: this conversation's holder (`ljos seat`).
        #[arg(long)]
        assignee: Option<String>,
        /// Claim the first ready node in the claim graph's order for this name.
        #[arg(long)]
        next: bool,
        /// Desired role affinity when claiming next work ('explore', 'architect', 'implementor', 'verifier', 'orchestrator', 'general').
        #[arg(long)]
        role: Option<String>,
        /// How far from the best next ticket the claim may reach. A larger number considers more tickets.
        #[arg(long)]
        slack: Option<usize>,
    },
    /// Give a ticket back. It is open again, and the claim generation moves.
    ///
    /// `--stale` releases every claim this seat holds that has been quiet
    /// longer than `--older-than`. The default window is `LJOS_LEASE_SECS`,
    /// 900 seconds when unset, the same window used to hand a quiet claim
    /// back. A claim whose conversation is still running on this host stays.
    /// Another seat's claim stays. The ticket stays open.
    /// `--dry-run` prints the count and changes nothing. `--verbose` prints
    /// one line per released ticket. `--days` lists claims untouched for
    /// that many days and writes nothing. `--yes` hands those back.
    /// An id together with `--stale` is refused.
    ///
    /// Example: `ljos release proj-1a2b`
    Release {
        /// A tracker id, or a 32-hex claim-graph id. Leave it off with `--stale`.
        #[arg(required_unless_present = "stale")]
        node: Option<String>,
        /// The name that holds it. Absent: this conversation's holder (`ljos seat`).
        #[arg(long)]
        assignee: Option<String>,
        /// Release this seat's quiet claims, or with `--days` list the older ones.
        #[arg(long)]
        stale: bool,
        /// Days untouched before a claim is stale. The default is `LJOS_STALE_DAYS`, or 7.
        #[arg(long, value_name = "N", requires = "stale")]
        days: Option<i64>,
        /// Hand the day-counted claims back. Without this, `--days` only prints them.
        #[arg(long, requires = "stale")]
        yes: bool,
        /// Quiet longer than this before a claim is released: `900`, `15m`, `6h` or `2d`. The default is `LJOS_LEASE_SECS`, 900 seconds.
        #[arg(long, value_name = "DURATION", requires = "stale")]
        older_than: Option<String>,
        /// Print what would be released and change nothing.
        #[arg(long, requires = "stale")]
        dry_run: bool,
        /// Print one line per released ticket.
        #[arg(long, requires = "stale")]
        verbose: bool,
    },
    /// Mark the claim finished. The ticket stays open.
    ///
    /// Example: `ljos complete proj-1a2b`
    Complete {
        /// A tracker id, or a 32-hex claim-graph id.
        node: String,
        /// Status to record: done, failed, or cancelled. The default is done.
        #[arg(long)]
        status: Option<String>,
        /// Claim generation recorded when the work was taken. Absent: the live one. A stale generation is refused.
        #[arg(long)]
        gen: Option<u64>,
        /// The name that holds it. Absent: this conversation's holder (`ljos seat`).
        #[arg(long)]
        assignee: Option<String>,
    },
    /// Print USER.md and MEMORY.md from this directory.
    ///
    /// ljos reads them and does not write them.
    ///
    /// The default directory is the working directory. `--dir` or
    /// `LJOS_CARDS_DIR` names another. With neither file, the line names
    /// the directory.
    ///
    /// Example: `ljos cards`
    Cards {
        /// Directory of USER.md and MEMORY.md. `--cards` is the same flag as `ljos sitting --cards`.
        #[arg(long, visible_alias = "cards", default_value = ".")]
        dir: PathBuf,
    },
    /// Forecast how the others will vote on an issue; two or more forecasts let the settle name the surprisingly popular answer.
    ///
    /// Example: `ljos predict proj-1a2b --expect keep`
    Predict {
        /// The ticket the forecast is about.
        issue: String,
        /// The option you expect to win, or a JSON object of option to share.
        #[arg(long)]
        expect: String,
        /// The persona this forecast records. Absent, the casting seat.
        #[arg(long = "as")]
        as_persona: Option<String>,
    },
    /// Add a command-guard rule: a pattern over the command line and a verdict.
    ///
    /// The verdict is deny, ask or allow. It is a label. A matching command
    /// still runs, with the rule's reason as a warning. `--user` keeps the
    /// rule in the user config, shared by every agent and never reviewed.
    /// An allow is the person's: written with `--user` at your own terminal,
    /// it clears the seat's warnings for the lines it matches. policyd, the
    /// guard program, still reads those lines.
    ///
    /// Example: `ljos rule "git push *" --why "name the branch"`
    Rule {
        /// A glob over the whole command line: `rm -rf *`, `*--force*`, `git push --force*`.
        pattern: String,
        /// Label the warning deny or ask. With `--user`, at your own terminal, allow clears
        /// the seat's warnings for the matching lines.
        #[arg(long, default_value = "ask")]
        verdict: String,
        /// The reason a reader sees when the rule fires.
        #[arg(long)]
        why: String,
        /// Print the atom JSON. The default is one line: id, kind, due, text.
        #[arg(long)]
        json: bool,
        /// Write the rule into the user config, shared by every seat, instead of this seat's memories.
        #[arg(long)]
        user: bool,
    },
    /// List the command guard's rules: verdict, pattern and reason, one a line.
    ///
    /// Example: `ljos rules`
    Rules {
        /// Print pattern, verdict and reason as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show what the command guard says about a command. It runs nothing.
    ///
    /// Put `--` before the command when that command has its own flags.
    /// The line after the command is what ljos remembers about it.
    #[command(after_help = "Example: `ljos policy -- git push --force`")]
    Policy {
        /// Exit 1 only when the answer denies the cloud seat push rule
        /// (a push outside the allowlist, a force push, or a branch delete).
        /// Any other rule stays a warning, and without this flag the exit stays 0.
        #[arg(long)]
        fail_on_deny: bool,
        /// The command to judge. Put `--` before it when the command has flags of its own.
        argv: Vec<String>,
    },
    /// Answer the hook a coding runner calls before it acts.
    ///
    /// Hook JSON or plain text goes on stdin. The memories it bears on, and
    /// the guard's verdict, come out. Nothing matched is empty output.
    /// `--prompt --text` prints them plainly.
    ///
    /// Example: `echo "cargo test" | ljos hook --prompt --text`
    #[command(hide = true)]
    Hook(HookArgs),
    /// Run the ljos part of one git hook. The block `ljos git-hooks install` appends calls this.
    ///
    /// Example: `ljos git-hook post-commit`
    #[command(hide = true)]
    GitHook {
        /// Hook name, for example `post-commit`.
        name: String,
        /// Arguments git passed the hook. They are not read.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Install, remove, or show the global git hooks that book commits.
    ///
    /// Example: `ljos git-hooks status`
    GitHooks {
        /// `install`, `uninstall`, or `status`.
        action: String,
    },
    /// Set, clear, or show the passphrase that unlocks this person's key.
    ///
    /// `set` and `clear` read the terminal. They are refused when a runner variable is set.
    ///
    /// Example: `ljos person-key status`
    PersonKey {
        /// `set`, `clear`, or `status`.
        action: String,
    },
    /// Show what the votes decide, and whether a push may proceed.
    ///
    /// First the seat ballots alone, which are what the push check uses,
    /// then the tracker's weighted settle over every ballot as advice:
    /// the count, each seat's social power, and the option with its share
    /// of the group's weight. That advice does not clear a push. The model
    /// crate's reading, with shares and influence, is `--json`. An anchor
    /// is how far a persona keeps its own view, from 0 to 1.
    ///
    /// The model crate's settle weighs each model family as one voter. A
    /// ballot's family is the model its verified slot records, else the
    /// model a judge ballot (`judge:MODEL`) names, else the runner its
    /// persona runs on. Ballots that name none share one `unknown` family,
    /// so ten of them weigh as much as one labelled ballot. The tracker's
    /// settle weighs no family. The push gate counts families on its own
    /// terms: there a seat with no slot is a family of its own.
    ///
    /// Example: `ljos consensus proj-1a2b`
    Consensus {
        /// The ticket whose votes to read.
        id: String,
        /// Print one JSON document and nothing else. The push cite stays at
        /// the top level under `push_cite`. The model crate's settle is under
        /// `settle`. The tracker's settle is under `tracker` when `--compare`
        /// ran. `surprising` and `reputation` are present when those readings ran.
        #[arg(long)]
        json: bool,
        /// Also run the tracker's own settle (`vissue consensus`), whose
        /// numbers can differ from the model crate's once trust rows exist.
        #[arg(long)]
        compare: bool,
        /// Read only: do not note an objection, file a child, or sit a
        /// round. By default a blocking objection with a reason is filed
        /// once (`Dissent holds TEXT`), and so is a minority a voter still
        /// holds after a settled majority (`Dissent holds OPTION`, once). A
        /// tie files nothing. A nit, a wording note or a style note is
        /// written on the issue. The same objection is not filed again.
        /// Each filing reading is one panel round, noted as `panel-round:
        /// N` (a `panel rN` title is round N). A round count does not
        /// change the ticket. When the seats still split, the reading
        /// names the open objection children.
        #[arg(long)]
        no_file: bool,
        /// Ballots the decision needs. Short of it the reading says so and
        /// exits 1: nothing is decided yet.
        #[arg(long, value_name = "N")]
        quorum: Option<usize>,
        /// With `--quorum`, the RFC 3339 time the ballots were due. Past it
        /// and still short, the vote has lapsed: the tracker notes `quorum
        /// timed out` once and this exits 3.
        #[arg(long, value_name = "RFC3339", requires = "quorum")]
        deadline: Option<String>,
    },
    /// Sign the record that lets a cloud run vote on a ticket.
    ///
    /// The record names the run's seat, the model, the run id and the run's public key, under this seat's pinned key. It is written before the run's first ballot.
    ///
    /// Example: `ljos slot proj-1a2b alice --model composer --run run-1 --key aa`
    #[command(hide = true)]
    Slot {
        /// The issue the run is launched for.
        issue: String,
        /// The `LJOS_SEAT` the run boots with. The run id is appended the
        /// way `ljos cloud up` appends it.
        seat: String,
        /// The model the run was launched on.
        #[arg(long)]
        model: String,
        /// The run id, the run's `CURSOR_AGENT_ID` or `GITHUB_RUN_ID`.
        #[arg(long)]
        run: String,
        /// The run's public seat key, the hex `ljos cloud up` printed on
        /// the run. Only ballots under this key count for the run.
        #[arg(long, value_name = "HEX")]
        key: String,
    },
    /// Record how much one voter counts another's view.
    ///
    /// FROM weighs TO at WEIGHT, a number from just above 0 up to 1. The row is stored with this seat's memories.
    ///
    /// Example: `ljos trust alice bob 0.5`
    Trust {
        /// Who is weighing. A seat or a persona name.
        from: String,
        /// Who is being weighed. A seat or a persona name.
        to: String,
        /// How much FROM counts TO, from just above 0 up to 1.
        weight: f64,
        /// Ids of stored files this row stands on. Repeat for several.
        #[arg(long)]
        why: Vec<String>,
        /// Domains this row is scoped to; none means it applies everywhere.
        #[arg(long, value_delimiter = ',')]
        about: Vec<String>,
    },
    /// Check that ljos is ready, and print the rows that need action.
    ///
    /// Exit 1 when a required program, the memory store or its token does not
    /// answer, the tool server a runner starts or the cloud signing key does
    /// not, the note queue is damaged, or the tracker directory is not
    /// writable. A tracker that has not been created yet does not fail this
    /// command.
    #[command(after_help = "Example: `ljos doctor`")]
    Doctor {
        /// Print every row, the ones that answer included.
        #[arg(long, visible_alias = "verbose")]
        all: bool,
        /// Refresh runner skills and runner env files this ljos wrote, and replace a stale cloud guard rule file. Also create a missing ljos home and config file, move a stale lock or pid file aside, start packsetd with `packset ensure`, and record a runner session seat. A skill edited by hand, an env file ljos did not write, a cloud guard file that is missing or not a known older version, git hooks, runner hooks, policy, credentials and keys are left in place. A stale file is moved aside with a `.bak` suffix.
        #[arg(long)]
        fix: bool,
    },
    /// Open the due, claims and trust window.
    ///
    /// Needs the `ljos-hud` program. A missing program exits 127. `--hide`
    /// when that program is not installed exits 0. `ljos statusline` prints
    /// the same seat in one line without that program.
    ///
    /// Example: `ljos hud`
    Hud {
        /// Stay on the terminal. Default detaches.
        #[arg(long)]
        foreground: bool,
        /// Show or hide a running HUD.
        #[arg(long, group = "summon")]
        toggle: bool,
        /// Show a running HUD.
        #[arg(long, group = "summon")]
        show: bool,
        /// Hide a running HUD.
        #[arg(long, group = "summon")]
        hide: bool,
        /// Write a user-local .desktop launcher and Sway overlay include.
        #[arg(long)]
        install_desktop: bool,
    },
    /// Print how work on one ticket is meant to run.
    ///
    /// `--full` prints every store and every reason that work is refused.
    ///
    /// Example: `ljos protocol`
    Protocol {
        /// Print the whole protocol instead of the short form.
        #[arg(long)]
        full: bool,
    },
    /// Show the seat in use, or add one to the tracker.
    ///
    /// With `LJOS_SEAT` unset, a session id in this process names the seat.
    /// Claude Code, Codex and Cursor each sit as the runner plus a short id,
    /// and the source line names that variable. An exported `LJOS_SEAT` is
    /// the seat as written. With no seat and no session id, `ljos seat` prints
    /// `none` (reads use every seat) and a write uses the login name, or a short stable name
    /// when that login is missing. A session with no id that would share the
    /// bare runner name says so.
    /// `add` writes a row in the tracker's vissue.toml. The first `add`
    /// starts the list, and a seat that is not on it cannot write.
    /// `grant` records a quoted yes to close. `--via` has to be this
    /// process's seat.
    ///
    /// Example: `ljos seat`
    ///
    /// Add one with `LJOS_SEAT=alice ljos seat add alice`.
    #[command(after_help = "Example: `ljos seat`")]
    Seat {
        #[command(subcommand)]
        action: Option<SeatAction>,
    },
    /// Start the tool server a runner connects to.
    ///
    /// This runs the `ljos-mcp` installed beside this `ljos` and passes the arguments through. MCP is that tool server. A client that starts a server by its crate's name uses this command.
    ///
    /// Example: `ljos mcp`
    Mcp {
        /// Arguments passed to `ljos-mcp` as they stand.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Connect ljos to a coding runner, or print the MCP entry to paste into one.
    ///
    /// With no `--harness`, ljos onboards every runner it finds.
    /// A pipe prints the MCP entry on stdout and the setup lines on stderr.
    /// MCP is the tool server the runner starts.
    /// `--example` prints harnesses.toml. A step that does not land exits 1.
    ///
    /// Example: `ljos onboard`
    Onboard {
        /// A runner named in ~/.config/ljos/harnesses.toml or shipped with ljos; `all` onboards every runner found on this machine, and `json` prints the entry to paste anywhere. Absent: `all`. A pipe prints the entry on stdout and the setup lines on stderr. `json` sets nothing up.
        #[arg(long)]
        harness: Option<String>,
        /// Report what would be written and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Print an example harnesses.toml and stop.
        #[arg(long)]
        example: bool,
        /// Skills directory. A shell runner that names none needs one.
        /// On any runner the skill is written here for this run.
        #[arg(long)]
        skills: Option<PathBuf>,
    },
    /// Remove runner configuration written by onboard.
    ///
    /// Example: `ljos offboard`
    Offboard {
        /// Remove one runner only. Without this, remove every recorded runner.
        /// `--harness` is the same flag `ljos onboard` uses.
        #[arg(long, visible_alias = "harness")]
        runner: Option<String>,
        /// Report what would be restored or removed and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove ljos from this machine.
    ///
    /// Offboard every runner, put the global git hooks back, remove the
    /// ljos programs, and leave a stub named ljos that answers a cached hook
    /// with an allow. packset, vissue, deedar and claimdag stay unless --all
    /// is set. Stop packsetd with `packset stop`. State, data and configuration
    /// stay unless --purge is set.
    ///
    /// Example: `ljos uninstall --dry-run`
    Uninstall {
        /// Report every step and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Leave the programs in place; runner files and git hooks still go.
        #[arg(long)]
        keep_programs: bool,
        /// Remove the other seat programs too: packset, vissue, deedar, claimdag.
        #[arg(long)]
        all: bool,
        /// Remove the seat's state, data, cache and configuration directories.
        #[arg(long)]
        purge: bool,
        /// Leave nothing where ljos was. A runner that cached the hook path
        /// then gets "not found", which every onboarded runner lets through.
        #[arg(long)]
        no_stub: bool,
        /// The directory the programs are in; the running ljos's own by default.
        #[arg(long)]
        bin_dir: Option<PathBuf>,
    },
    /// Write a signed directory another seat can receive.
    ///
    /// The directory holds this seat's memories, the stored files they cite, and
    /// a signed manifest. The other seat runs `ljos receive` on it.
    ///
    /// Example: `ljos handover --out /tmp/bag --issue proj-1a2b`
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
        /// The log size the receiver kept last time (`size=` in its kept head): the bag then carries the bridge from it.
        #[arg(long)]
        since_size: Option<u64>,
        /// Carry only the atoms tagged `scope:NAME`, sealed with age to that scope's recipients in `.ljos/sync.toml`.
        #[arg(long, value_name = "NAME")]
        scope: Option<String>,
    },
    /// Check a handover directory. `--import` stores its memories here.
    ///
    /// Example: `ljos receive /tmp/bag --keep ~/.local/share/ljos/peer.head`
    Receive {
        /// The handover directory to check.
        dir: PathBuf,
        /// The head file `--keep` wrote at the last receive from the same sender.
        #[arg(long)]
        since: Option<PathBuf>,
        /// Write this bag's log head here, for `--since` next time.
        #[arg(long)]
        keep: Option<PathBuf>,
        /// Store the directory's memories here. Without this, the directory is only checked.
        #[arg(long)]
        import: bool,
    },
    /// List the memories due for review soonest.
    ///
    /// Eight of them, then one line on the review clock.
    ///
    /// Example: `ljos due`
    Due {
        /// List every due atom to read; none of them is put up for grading.
        #[arg(long)]
        all: bool,
        /// Put the page to the review judges. A claim they find holds is
        /// named for `ljos graded`. A contradicted one is named. The rest stay due.
        #[arg(long, conflicts_with = "all")]
        judge: bool,
        /// Print JSON. The human page is left off. The ids are the full ones `ljos graded` takes.
        #[arg(long, conflicts_with = "judge")]
        json: bool,
    },
    /// Score the judge log against outcomes: accuracy and calibration per decision.
    ///
    /// Example: `ljos judge-score`
    #[command(hide = true)]
    JudgeScore,
    /// File one ticket per module in a bundle, under a parent ticket.
    ///
    /// The same ids come back on every run. Each module waits on the modules it depends on. `vissue ready` then lists what can be built now.
    ///
    /// Example: `ljos bump-plan bundle --project demo --parent proj-1a2b`
    #[command(hide = true)]
    BumpPlan {
        /// The bundle directory: `locks/default.lock.json` and `package.sbom.cdx.json` inside it.
        bundle: PathBuf,
        /// The tracker project the issues go in.
        #[arg(long)]
        project: String,
        /// The bump ticket every module issue is a child of.
        #[arg(long)]
        parent: String,
        /// The generation named in titles and ids; default the lock's toolchain (`foss/2026.1`).
        #[arg(long)]
        generation: Option<String>,
        /// Print the rows and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// List the findings in a campaign file, one per line.
    ///
    /// `--remember` stores one lesson per finding a person or a seat resolved. `--issue` records the file on that ticket.
    ///
    /// Example: `ljos findings campaign.json`
    #[command(hide = true)]
    Findings {
        /// The campaign state file (`campaign.json`).
        state: PathBuf,
        /// Store one lesson per resolved finding with this seat's memories.
        #[arg(long)]
        remember: bool,
        /// With --remember, every finding, the ones a later attempt superseded too.
        #[arg(long)]
        all: bool,
        /// Record the state file as a stored file on this ticket.
        #[arg(long)]
        issue: Option<String>,
    },
    /// Record a number you measure again later, or list the ones you keep.
    ///
    /// A habit is that number. `ljos habit NAME VALUE` stores a reading and closes the one before. `ljos habit NAME` shows one. `ljos habit` lists them, the change since the last reading, and when the next is due.
    ///
    /// Example: `ljos habit`
    Habit {
        /// The habit's name; absent, list every habit.
        name: Option<String>,
        /// The reading; absent, show the habit.
        value: Option<f64>,
        /// The unit the reading is in, for the reader.
        #[arg(long, default_value = "")]
        unit: String,
        /// How often a reading is taken: 7d, 24h, 2w, 30m. The next is due one cadence on.
        #[arg(long, default_value = "7d")]
        every: String,
        /// Where the reading came from: a job id, a run, a file.
        #[arg(long, default_value = "")]
        source: String,
    },
    /// Start work on a ticket, or file a plan and sit it.
    ///
    /// `ljos sitting ISSUE` claims that ticket. `ljos sitting --plan TITLE -- ITEM...`
    /// files the parent and the items in one tracker commit, claims the parent,
    /// and prints the checklist. `ljos ready --mine` names the next unfinished
    /// item. `ljos finish ITEM` ticks that item off. Once the tracker lists
    /// seats, this seat has to be one of them.
    ///
    /// Example: `ljos sitting proj-1a2b`
    ///
    /// `ljos sitting --plan` files a checklist. Example: `LJOS_SEAT=alice ljos sitting --plan "Ship the codec" -p day -- "measure the files"`
    #[command(alias = "start", after_help = "Example: `ljos sitting proj-1a2b`")]
    Sitting {
        /// The tracker id of the issue. Absent when `--plan` files a new one.
        #[arg(
            value_name = "ISSUE",
            required_unless_present = "plan",
            conflicts_with = "plan"
        )]
        issue: Option<String>,
        /// File this parent and the items after `--`, then sit the parent.
        #[arg(long, value_name = "TITLE")]
        plan: Option<String>,
        /// No parent for the plan, even while an issue is held.
        #[arg(long, requires = "plan")]
        top: bool,
        /// The project the plan is filed in. Pass it with `--plan`. Absent: this checkout's prefix, else Software.
        /// One name. A leading segment that repeats the tracker directory or its org prefix is dropped. Any other slash is refused.
        #[arg(short = 'p', long)]
        project: Option<String>,
        /// Checklist items, after `--`.
        #[arg(
            last = true,
            value_name = "ITEM",
            requires = "plan",
            allow_hyphen_values = true
        )]
        items: Vec<String>,
        /// Occupancy name, taken as given, the seat's own name too. Absent: this conversation's holder (`ljos seat`). Always scoped to the issue.
        #[arg(long)]
        assignee: Option<String>,
        /// Where USER.md and MEMORY.md are read from. `--dir` is the same flag as `ljos cards --dir`.
        #[arg(long, visible_alias = "dir", default_value = ".")]
        cards: PathBuf,
        /// Accepted and ignored. A blocked issue is sat and labelled `blocked-by`.
        #[arg(long)]
        anyway: bool,
        /// The recipe copied in before the ticket is recalled. Absent, a word from the closed set in the title, else sit. Kept until finish or release.
        #[arg(long)]
        playbook: Option<String>,
        /// Print every section in full, including the tracker and sync lines.
        /// Without it the command prints the claim, one line for claims still
        /// held, what the seat remembers, and the close. A commit that landed
        /// is quiet, and so is a push that landed. A refusal is one sentence.
        #[arg(long)]
        verbose: bool,
        /// Print `text`, `tracker_git` and `sync` as JSON. `text` is what a
        /// person sees. The other fields keep the commit and sync lines.
        #[arg(long)]
        json: bool,
    },
    /// Finish work on a ticket: remember the lesson and hand the claim back. Without `--close` the ticket stays open.
    ///
    /// A sitting is one conversation's claim on an issue.
    /// With no id, this finishes that issue.
    /// With none open, it exits 1 and names `ljos sitting ISSUE`.
    /// An empty id exits 1 and writes nothing.
    /// A named id that is not that issue is still finished. stderr names
    /// that id and the issue this conversation holds.
    /// `--quiet` leaves that line off.
    /// Another conversation of this seat is not that issue.
    /// Without `--close` and with no lesson, a ticket that is still TODO and that nobody has claimed is left as it is.
    /// The line names `ljos sitting ISSUE`.
    ///
    /// Example: `ljos finish --lesson "Cold metal seats."`
    #[command(after_help = "Example: `ljos finish --lesson \"Cold metal seats.\"`")]
    Finish {
        /// The tracker id of the issue. Absent: the issue this conversation holds.
        issue: Option<String>,
        /// Status: done, failed, or cancelled. Only `--close` with done or cancelled records it. Any other finish checks it and records nothing.
        #[arg(long, default_value = "done")]
        status: String,
        /// The lesson this work taught, two sentences at most.
        #[arg(long)]
        lesson: Option<String>,
        /// The lesson given after the issue without `--lesson`, as agents often write it.
        #[arg(value_name = "LESSON", conflicts_with = "lesson")]
        lesson_text: Option<String>,
        /// The option that turned out right, when the ballots are in and the world has said.
        #[arg(long)]
        outcome: Option<String>,
        /// The outcome is none of the ballot's options: the person answered with a new one.
        #[arg(long, requires = "outcome")]
        new_option: bool,
        /// The factor a refuted voter shrinks by when an outcome is named.
        #[arg(long, default_value_t = LEARN_BETA)]
        beta: f64,
        /// Claim generation recorded when the work was taken. Absent: the live one. A stale generation is refused.
        #[arg(long)]
        gen: Option<u64>,
        /// The name that holds it. Absent: this conversation's holder (`ljos seat`).
        #[arg(long)]
        assignee: Option<String>,
        /// Complete the claim and close the tracker ticket. A runner is refused unless a standing grant covers this seat and this ticket. CANCELLED with `--status cancelled`. With `--status failed` the claim is handed back and the ticket stays open. Only when the work is accepted or dropped, not when the work session ends.
        #[arg(long)]
        close: bool,
        /// Link the memories the ticket's title activates, say whether they
        /// were linked, and print the tracker and sync lines. Without it,
        /// finish links nothing and says the ticket and the next
        /// command. A commit that landed is quiet, and so is a push that
        /// landed or a sealed log. A refusal is one sentence.
        #[arg(long)]
        verbose: bool,
        /// Print `text`, `tracker_git` and `sync` as JSON. `text` is what a
        /// person sees. The other fields keep the commit and sync lines.
        #[arg(long)]
        json: bool,
        /// Leave off the line that names another issue.
        #[arg(long)]
        quiet: bool,
    },
    /// Put a finished or cancelled issue back to the state it had before it was closed.
    ///
    /// A person has to be present. The close stays on the logbook, with the reopen under it. The caller does not take the claim.
    ///
    /// Example: `ljos reopen proj-1a2b`
    Reopen {
        /// The tracker id of the issue.
        issue: String,
    },
    /// Take back this seat's own ballot or note on an issue. Nothing is deleted: the withdrawal is a dated note beside it.
    ///
    /// Example: `ljos withdraw proj-1a2b`
    Withdraw {
        /// The tracker id of the issue.
        issue: String,
        /// The note to take back, whole or the part of it that names one note. Without this, the ballot is taken back.
        #[arg(long, value_name = "TEXT")]
        note: Option<String>,
        /// The name a ballot was cast under, for a persona's ballot.
        #[arg(long = "as", value_name = "NAME")]
        as_persona: Option<String>,
    },
    /// Estimate how often each voter has been right, from the project's past votes.
    ///
    /// The estimate is Dawid-Skene: it uses the ballots and no separate answer key.
    ///
    /// Example: `ljos calibrate --project demo`
    Calibrate {
        /// The tracker project whose settled issues to read.
        #[arg(short, long)]
        project: String,
        /// Expectation-maximisation rounds.
        #[arg(long, default_value_t = 20)]
        rounds: usize,
    },
    /// Record that one due review was graded.
    ///
    /// The memory counts as recalled, unless `--lapsed` says the review was missed.
    ///
    /// Example: `ljos graded a6ae2751`
    Graded {
        /// The memory id from `ljos due`.
        id: String,
        /// Count the review as missed. Without this, the memory counts as recalled.
        #[arg(long)]
        lapsed: bool,
    },
    /// Update how much each voter's ballot counts, from the option that was right.
    ///
    /// Example: `ljos learn proj-1a2b --outcome keep`
    Learn {
        /// The ticket whose ballots to reweigh.
        id: String,
        /// The option that turned out right.
        #[arg(long)]
        outcome: String,
        /// The outcome is none of the ballot's options: the person answered with a new one.
        #[arg(long)]
        new_option: bool,
        /// The factor a refuted voter shrinks by.
        #[arg(long, default_value_t = LEARN_BETA)]
        beta: f64,
        /// How far a voter who was wrong for a long time moves back toward a full vote after this step. 0 leaves the new weight as the step set it.
        #[arg(long, default_value_t = 0.0)]
        share: f64,
        /// `record` (the default) counts each voter's hits and misses so far. `hedge` shrinks a voter who was wrong by `--beta`, then moves `--share` of the way back.
        #[arg(long, default_value = "record")]
        rule: String,
    },
    /// Print a shell completion script for bash, zsh, fish, powershell or elvish.
    ///
    /// The script completes subcommands and flags. Seat names and open ticket
    /// ids are filled in when the tracker on this machine is small enough to
    /// read while the shell waits. A larger tracker completes commands and
    /// flags only. The script does not edit a shell startup file. Bash reads
    /// ~/.bashrc, zsh reads ~/.zshrc, fish reads config.fish, PowerShell reads
    /// $PROFILE and Elvish reads rc.elv. The command reference gives the line.
    ///
    /// Example: `ljos completions bash`
    Completions {
        /// `bash`, `zsh`, `fish`, `powershell` or `elvish`.
        #[arg(required_unless_present = "candidates")]
        shell: Option<shell_complete::ShellName>,
        /// `seats` or `tickets`. The completion scripts ask for these.
        #[arg(long, hide = true, conflicts_with = "shell")]
        candidates: Option<shell_complete::Candidates>,
    },
    /// Print this message, or the help of one command.
    ///
    /// `--all` lists every command, including the ones left off the short list.
    ///
    /// Example: `ljos help search`
    Help {
        /// List every command, including those left off the short list.
        #[arg(long)]
        all: bool,
        /// The command to explain. Further words name a nested command.
        #[arg(value_name = "COMMAND")]
        command: Vec<String>,
    },
}

/// The status clap uses when the arguments could not be read.
const USAGE_STATUS: i32 = 2;

/// What `ljos statusline` prints at a terminal. The runner pipes JSON.
fn statusline_tty_hint() -> &'static str {
    "ljos: statusline reads the runner's status JSON on stdin, so pipe that JSON to it."
}

/// One sentence for a mistyped command or flag. Other parse errors stay
/// with clap. Exit [`USAGE_STATUS`].
///
/// The command tree is built on a fresh thread. `run_command`'s frame is
/// already the whole command match, and building the tree again on that
/// stack is the cost a hook call is kept away from. A typo is not a hook
/// call. The thread is the 8 MiB a normal process gets.
fn usage_sentence(err: &clap::Error) -> Option<String> {
    use clap::error::{ContextKind, ErrorKind};
    let args: Vec<String> = std::env::args().skip(1).collect();
    match err.kind() {
        ErrorKind::InvalidSubcommand => {
            let typed = context_one(err, ContextKind::InvalidSubcommand)?;
            Some(command_sentence(&typed, &args))
        }
        ErrorKind::UnknownArgument => {
            let typed = context_one(err, ContextKind::InvalidArg)?;
            if typed.starts_with('-') {
                Some(flag_sentence(&typed, &args))
            } else {
                Some(command_sentence(&typed, &args))
            }
        }
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => {
            value_sentence(err, &args).or_else(|| bad_value(err, &args))
        }
        ErrorKind::MissingRequiredArgument => {
            missing_shell(&args).or_else(|| missing_argument(err, &args))
        }
        _ => None,
    }
}

/// The page `ljos ready` and `ljos list` print.
fn print_ready(args: ReadyArgs) -> Result<()> {
    let ReadyArgs {
        project,
        json,
        mine,
        limit,
        all,
    } = args;
    if mine {
        print!("{}", ljos_cli::plan::ready_mine(project.as_deref())?);
    } else {
        print!(
            "{}",
            ljos_cli::scan::for_reader(
                &ljos_cli::ready_view(project.as_deref(), json, limit, all)?,
                human_color()
            )
        );
    }
    Ok(())
}

/// A word a person types at the root instead of the command.
///
/// The command is not run. `find` is the verb beside `ljos search` on the
/// bare screen. `add`, `save` and `store` are `remember`. `note-add` is
/// `note`. A real command spelled this way still runs, because this is
/// only asked after the word missed every command.
fn root_synonym(typed: &str) -> Option<&'static str> {
    match typed.to_ascii_lowercase().as_str() {
        "find" | "query" | "lookup" => Some("search"),
        "add" | "save" | "store" => Some("remember"),
        "note-add" | "add-note" => Some("note"),
        "close" => Some("finish"),
        "create" | "new" => Some("file"),
        "delete" | "rm" | "remove" => Some("forget"),
        "status" => Some("doctor"),
        "stats" => Some("digest"),
        "edit" => Some("memories"),
        "memorize" | "memorise" => Some("remember"),
        "review" => Some("proposals"),
        _ => None,
    }
}

/// `lessons` is what a person calls the thing ljos keeps. It is not a
/// command, and no one verb is the right answer: finding one is `search`,
/// saving one is `remember`, and the ones waiting on a yes are `proposals`.
fn lessons_topic(typed: &str) -> bool {
    matches!(
        typed.to_ascii_lowercase().replace('_', "-").as_str(),
        "lessons" | "lesson" | "learnings" | "learning" | "learned" | "lore"
    )
}

/// More than one word after `recall`. It takes one ticket id, so the
/// words are a search the person forgot to quote.
///
/// A first word that is a ticket id is the ticket, and the line names
/// the one-id form. Otherwise the line prints the search that runs.
fn recall_words_sentence(args: &[String]) -> String {
    let split = args.iter().position(|arg| arg == "--");
    let before = command_words_rest(&args[..split.unwrap_or(args.len())]);
    let after = split.map_or(&[][..], |at| &args[at + 1..]);
    let words: Vec<&str> = before
        .iter()
        .skip(1)
        .chain(after)
        .map(String::as_str)
        .collect();
    match words.first() {
        Some(first) if !first.starts_with('-') && looks_like_ticket_id(first) => {
            format!("ljos: recall takes one ticket id, so run `ljos recall {first}`.")
        }
        _ => {
            let search = recall_search_command(&words.join(" "));
            if search.contains('`') {
                format!("ljos: recall takes one ticket id, so run {search} to look up words.")
            } else {
                format!("ljos: recall takes one ticket id, so run `{search}` to look up words.")
            }
        }
    }
}

/// The three verbs for a lessons word, each as a line that runs.
///
/// Words typed straight after it are the query: `ljos lessons kiln` is
/// `ljos search kiln`. A flag ends them.
fn lessons_sentence(typed: &str, args: &[String]) -> String {
    let after: Vec<&String> = args
        .iter()
        .skip_while(|arg| arg.as_str() != typed)
        .skip(1)
        .take_while(|arg| !arg.starts_with('-'))
        .collect();
    let search = if after.is_empty() {
        "ljos search \"lexical default\"".to_string()
    } else {
        let words: Vec<&str> = after.iter().map(|word| word.as_str()).collect();
        recall_search_command(&words.join(" "))
    };
    format!(
        "ljos: no command {typed}. To find lessons run `{search}`, to save one run `ljos remember \"The lexical default is BM25+.\"`, and to review the ones waiting run `ljos proposals`."
    )
}

/// The verb a word that is not a command was reaching for.
///
/// The start of exactly one verb wins first, so `prop` is `proposals`
/// and not the nearest short spelling. Then the nearest spelling, then
/// [`near_verb`].
fn pick_verb(typed: &str, names: &[String]) -> Option<String> {
    let folded = typed.trim().to_ascii_lowercase().replace('_', "-");
    start_of_verb(&folded, names)
        .or_else(|| closest(typed, names))
        .or_else(|| near_verb(typed, names))
        .map(str::to_string)
}

/// A word that is not a command but names one by its stem or a part.
///
/// `remembering` and `searches` are the verb with an ending on it, and
/// `search-lessons` carries one verb as a part. Aliases count as verbs.
fn near_verb<'a>(typed: &str, names: &'a [String]) -> Option<&'a str> {
    let folded = typed.trim().to_ascii_lowercase().replace('_', "-");
    if folded.is_empty() {
        return None;
    }
    let mut words = vec![folded.as_str()];
    words.extend(
        folded
            .split('-')
            .filter(|part| !part.is_empty() && *part != folded),
    );
    words.into_iter().find_map(|word| {
        names
            .iter()
            .map(String::as_str)
            .find(|name| *name == word && word.chars().count() >= 3)
            .or_else(|| stem_verb(word, names))
            .filter(|name| !name.eq_ignore_ascii_case(typed))
    })
}

fn stem_verb<'a>(word: &str, names: &'a [String]) -> Option<&'a str> {
    for ending in ["ing", "ed", "es", "er", "s"] {
        let Some(stem) = word.strip_suffix(ending) else {
            continue;
        };
        if stem.chars().count() < 3 {
            continue;
        }
        let with_e = format!("{stem}e");
        if let Some(name) = names
            .iter()
            .find(|name| name.as_str() == stem || name.as_str() == with_e)
        {
            return Some(name);
        }
    }
    None
}

fn start_of_verb<'a>(word: &str, names: &'a [String]) -> Option<&'a str> {
    if word.chars().count() < 3 {
        return None;
    }
    let mut found = names
        .iter()
        .filter(|name| name.len() > word.len() && name.starts_with(word));
    let first = found.next()?;
    found.next().is_none().then_some(first.as_str())
}

/// `issues`, `tickets` and `todo` are not commands.
///
/// A person reaching for a board means the ready list. Claims is where a
/// ticket already in hand shows up. The word is not run. `list` and `ls`
/// are commands, and run.
fn listing_sentence(typed: &str) -> Option<String> {
    match typed.to_ascii_lowercase().as_str() {
        "issues" | "tickets" | "todo" => Some(format!(
            "ljos: no command {typed}, and the closest is ready, so run `ljos ready`. `ljos claims` lists what is held."
        )),
        _ => None,
    }
}

fn command_sentence(typed: &str, args: &[String]) -> String {
    let path = command_words(args, typed);
    let here = words_at(&path);
    if here.at == ["recall"] && here.commands.is_empty() {
        return recall_words_sentence(args);
    }
    if let Some(line) = ticket_id_sentence(typed, &here.at, here.form.as_ref()) {
        return line;
    }
    if let Some(line) = listing_sentence(typed).filter(|_| here.at.is_empty()) {
        return line;
    }
    if here.at.is_empty() && lessons_topic(typed) {
        return lessons_sentence(typed, args);
    }
    let picked = if here.at.is_empty() {
        root_synonym(typed).map(str::to_string)
    } else {
        None
    };
    let picked = picked.or_else(|| pick_verb(typed, &here.commands));
    match picked {
        Some(name) => {
            let rewritten = rewrite_arg(args, typed, &name);
            format!("ljos: no command {typed}, and the closest is {name}, so run `{rewritten}`.")
        }
        None => {
            let help = if here.at.is_empty() {
                "ljos help".to_string()
            } else {
                format!("ljos {} --help", here.at.join(" "))
            };
            format!("ljos: no command {typed}, so run `{help}`.")
        }
    }
}

fn flag_sentence(typed: &str, args: &[String]) -> String {
    let path = command_words(args, typed);
    let here = words_at(&path);
    let bare = typed.trim_start_matches('-');
    let names = flag_names(&here.flags);
    let where_ = if here.at.is_empty() {
        "ljos".to_string()
    } else {
        here.at.join(" ")
    };
    if project_flag_typed(typed) {
        if matches!(where_.as_str(), "sitting" | "start") && !args.iter().any(|arg| arg == "--plan")
        {
            return sitting_project_line();
        }
        if !here.flags.iter().any(|flag| flag == "-p") {
            let shown = project_flag_shown(typed);
            return format!(
                "ljos: {where_} has no flag {shown}. The verbs that take -p are {}.",
                english_and(&here.project_verbs)
            );
        }
    }
    match closest(bare, &names) {
        Some(name) => {
            let flag = flag_text(typed, name, &here.flags);
            let rewritten = rewrite_arg(args, typed, flag);
            format!(
                "ljos: {where_} has no flag {typed}, and the closest is {flag}, so run `{rewritten}`."
            )
        }
        None => {
            let help = if here.at.is_empty() {
                "ljos --help".to_string()
            } else {
                format!("ljos {} --help", here.at.join(" "))
            };
            format!("ljos: {where_} has no flag {typed}, so run `{help}`.")
        }
    }
}

fn value_sentence(err: &clap::Error, args: &[String]) -> Option<String> {
    use clap::error::ContextKind;
    let typed = context_one(err, ContextKind::InvalidValue)?;
    if typed.starts_with('-') || args.iter().any(|arg| arg == "--candidates") {
        return None;
    }
    let path = command_words(args, &typed);
    if path != ["completions"] {
        return None;
    }
    let mut options = context_list(err, ContextKind::ValidValue);
    if options.is_empty() {
        options = context_list(err, ContextKind::SuggestedValue);
    }
    if options.is_empty() {
        return None;
    }
    // `pwsh` is the program PowerShell installs. It is not an alias: the
    // line names `powershell`, and that is the script the person runs.
    let picked = (typed.eq_ignore_ascii_case("pwsh"))
        .then_some("powershell")
        .filter(|name| options.iter().any(|have| have == *name))
        .map(str::to_string)
        .or_else(|| closest(&typed, &options).map(str::to_string));
    let line = match picked {
        Some(name) => {
            let rewritten = rewrite_arg(args, &typed, &name);
            format!("ljos: no shell {typed}, and the closest is {name}, so run `{rewritten}`.")
        }
        None => {
            let rewritten = rewrite_arg(args, &typed, "bash");
            format!("ljos: no shell {typed}, so run `{rewritten}`.")
        }
    };
    Some(line)
}

fn bad_value(err: &clap::Error, args: &[String]) -> Option<String> {
    use clap::error::ContextKind;
    let value = context_one(err, ContextKind::InvalidValue)?;
    let flag = context_one(err, ContextKind::InvalidArg).unwrap_or_default();
    let flag = flag.split_whitespace().next().unwrap_or(flag.as_str());
    let flag = if flag.is_empty() {
        "value".to_string()
    } else if flag.starts_with('-') {
        flag.to_string()
    } else {
        format!("--{flag}")
    };
    let cmd = command_path_head(args);
    let help = help_of(&cmd);
    Some(format!(
        "{cmd}: {flag} does not accept {value}. The values it takes are in `{help}`."
    ))
}

fn missing_argument(err: &clap::Error, args: &[String]) -> Option<String> {
    use clap::error::ContextKind;
    let missing = context_one(err, ContextKind::InvalidArg)
        .or_else(|| {
            context_list(err, ContextKind::InvalidArg)
                .into_iter()
                .next()
        })
        .unwrap_or_else(|| "an argument".to_string());
    let cmd = command_path_head(args);
    let help = help_of(&cmd);
    if cmd == "usage" {
        return Some(
            "usage needs a start day. Pass it as `ljos usage --since YYYY-MM-DD`.".to_string(),
        );
    }
    if matches!(cmd.as_str(), "sitting" | "start")
        && args_name_project(args)
        && !args.iter().any(|arg| arg == "--plan")
    {
        return Some(sitting_project_line());
    }
    Some(format!(
        "{cmd} needs {missing}. `{help}` lists what to pass."
    ))
}

/// `-p` on `sitting` files a plan. Without `--plan` the line says so and
/// names every verb that takes `-p`.
fn sitting_project_line() -> String {
    format!(
        "ljos: sitting takes -p with --plan. The verbs that take -p are {}.",
        project_verbs_sentence()
    )
}

fn project_verbs_sentence() -> String {
    english_and(&words_at(&[]).project_verbs)
}

fn project_flag_typed(typed: &str) -> bool {
    matches!(project_flag_shown(typed), "-p" | "-P" | "--project")
}

fn project_flag_shown(typed: &str) -> &str {
    typed.split('=').next().unwrap_or(typed)
}

fn args_name_project(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "-p" || arg == "-P" || arg == "--project" || arg.starts_with("--project=")
    })
}

fn english_and(items: &[String]) -> String {
    match items {
        [] => "none".to_string(),
        [one] => one.clone(),
        [one, two] => format!("{one} and {two}"),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn project_verb_names(root: &clap::Command) -> Vec<String> {
    let mut names = Vec::new();
    for sub in root.get_subcommands() {
        if sub.is_hide_set() {
            continue;
        }
        let takes = sub.get_arguments().any(|arg| arg.get_short() == Some('p'));
        if takes {
            names.push(sub.get_name().to_string());
        }
    }
    names.sort();
    names.dedup();
    names
}

fn command_path_head(args: &[String]) -> String {
    let path = words_at(&command_words_rest(args)).at;
    if path.is_empty() {
        "ljos".to_string()
    } else {
        path.join(" ")
    }
}

fn help_of(cmd: &str) -> String {
    if cmd == "ljos" {
        "ljos --help".to_string()
    } else {
        format!("ljos {cmd} --help")
    }
}

fn missing_shell(args: &[String]) -> Option<String> {
    if command_words_rest(args) != ["completions"] {
        return None;
    }
    let mut line = vec!["ljos".to_string()];
    line.extend(args.iter().map(|arg| shell_word(arg)));
    line.push("bash".to_string());
    Some(format!(
        "ljos: completions needs a shell, so run `{}`.",
        line.join(" ")
    ))
}

struct UsageWords {
    at: Vec<String>,
    commands: Vec<String>,
    flags: Vec<String>,
    /// What an id-taking verb expected, and the command from its example line.
    form: Option<(String, String)>,
    /// Verbs whose short flag `-p` names a project, in name order.
    project_verbs: Vec<String>,
}

fn words_at(path: &[String]) -> UsageWords {
    let path = path.to_vec();
    std::thread::Builder::new()
        .name("ljos-usage".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || words_at_inner(&path))
        .expect("usage hint thread")
        .join()
        .expect("usage hint")
}

fn words_at_inner(path: &[String]) -> UsageWords {
    let mut cmd = Cli::command();
    cmd.build();
    let project_verbs = project_verb_names(&cmd);
    let mut at = Vec::new();
    for name in path {
        let Some(sub) = cmd.find_subcommand(name).cloned() else {
            break;
        };
        at.push(name.clone());
        cmd = sub;
    }
    UsageWords {
        at,
        commands: child_names(&cmd),
        flags: flag_spellings(&cmd),
        form: ticket_form(&cmd),
        project_verbs,
    }
}

/// An extra word that is a ticket id, on a verb that takes one.
///
/// The line names what that verb's id argument says, and the command from
/// its example. A word that is not an id stays a missing command.
fn ticket_id_sentence(
    typed: &str,
    at: &[String],
    form: Option<&(String, String)>,
) -> Option<String> {
    if at.is_empty() || !vissue_id(typed) {
        return None;
    }
    let (expected, example) = form?;
    let verb = at.join(" ");
    Some(format!(
        "ljos: {verb} expected {expected}, so run `{example}`."
    ))
}

fn ticket_form(cmd: &clap::Command) -> Option<(String, String)> {
    let mut expected = None;
    for arg in cmd.get_arguments() {
        if arg.get_index().is_none() || !arg_takes_ticket(arg) {
            continue;
        }
        let help = arg.get_help().map(plain_styled).unwrap_or_default();
        let phrase = expected_phrase(&help);
        if !phrase.is_empty() {
            expected = Some(phrase);
            break;
        }
    }
    let expected = expected?;
    let example = example_form(cmd)?;
    Some((expected, example))
}

fn arg_takes_ticket(arg: &clap::Arg) -> bool {
    if let Some(names) = arg.get_value_names() {
        for name in names {
            if name.eq_ignore_ascii_case("issue") || name.eq_ignore_ascii_case("node") {
                return true;
            }
        }
    }
    let Some(help) = arg.get_help() else {
        return false;
    };
    let help = plain_styled(help).to_ascii_lowercase();
    help.contains("tracker id")
        || help.contains("ticket id")
        || help.contains("issue id")
        || help.contains("the ticket")
}

fn expected_phrase(help: &str) -> String {
    let sentence = help.split(['.', '\n']).next().unwrap_or(help).trim();
    let mut chars = sentence.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let rest: String = chars.collect();
    let lower = match rest.chars().next() {
        Some(next)
            if first.is_ascii_uppercase() && (next.is_lowercase() || next.is_whitespace()) =>
        {
            first.to_ascii_lowercase()
        }
        None if first.is_ascii_uppercase() => first.to_ascii_lowercase(),
        _ => first,
    };
    format!("{lower}{rest}")
}

fn example_form(cmd: &clap::Command) -> Option<String> {
    let blobs = [cmd.get_long_about(), cmd.get_about()];
    for blob in blobs.into_iter().flatten() {
        if let Some(form) = example_in(&plain_styled(blob)) {
            return Some(form);
        }
    }
    None
}

fn example_in(text: &str) -> Option<String> {
    let at = text.find("Example:")?;
    let rest = text[at + "Example:".len()..].trim_start();
    let start = rest.find('`')?;
    let tail = &rest[start + 1..];
    let end = tail.find('`')?;
    let form = &tail[..end];
    (!form.is_empty()).then(|| form.to_string())
}

fn plain_styled(text: &clap::builder::StyledStr) -> String {
    text.to_string()
}

fn child_names(cmd: &clap::Command) -> Vec<String> {
    let mut names = Vec::new();
    for sub in cmd.get_subcommands() {
        names.push(sub.get_name().to_string());
        names.extend(sub.get_all_aliases().map(str::to_string));
    }
    names.sort();
    names.dedup();
    names
}

fn flag_spellings(cmd: &clap::Command) -> Vec<String> {
    let mut names = Vec::new();
    for arg in cmd.get_arguments() {
        if let Some(long) = arg.get_long() {
            names.push(format!("--{long}"));
        }
        if let Some(aliases) = arg.get_all_aliases() {
            for alias in aliases {
                names.push(format!("--{alias}"));
            }
        }
        if let Some(short) = arg.get_short() {
            names.push(format!("-{short}"));
        }
        if let Some(shorts) = arg.get_all_short_aliases() {
            for short in shorts {
                names.push(format!("-{short}"));
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

fn flag_text<'a>(typed: &str, bare: &'a str, flags: &'a [String]) -> &'a str {
    let long = typed.starts_with("--");
    if let Some(flag) = flags
        .iter()
        .find(|flag| flag.trim_start_matches('-') == bare && flag.starts_with("--") == long)
    {
        return flag;
    }
    flags
        .iter()
        .find(|flag| flag.trim_start_matches('-') == bare)
        .map(String::as_str)
        .unwrap_or(bare)
}

fn context_one(err: &clap::Error, kind: clap::error::ContextKind) -> Option<String> {
    let value = err.get(kind)?;
    let text = match value {
        clap::error::ContextValue::String(text) => text.clone(),
        other => other.to_string(),
    };
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn context_list(err: &clap::Error, kind: clap::error::ContextKind) -> Vec<String> {
    let Some(value) = err.get(kind) else {
        return Vec::new();
    };
    match value {
        clap::error::ContextValue::Strings(list) => list.clone(),
        clap::error::ContextValue::String(text) => vec![text.clone()],
        other => {
            let text = other.to_string();
            if text.is_empty() {
                Vec::new()
            } else {
                vec![text]
            }
        }
    }
}

fn flag_names(options: &[String]) -> Vec<String> {
    options
        .iter()
        .map(|option| option.trim_start_matches('-').to_string())
        .collect()
}

/// How many edits still count as one mistype.
///
/// One letter is every other short flag, so a one-letter flag is not
/// guessed. Two or three letters can miss one. A longer word can be off
/// by two, and a long one by three.
fn max_edits(typed: &str) -> usize {
    match typed.chars().count() {
        0 | 1 => 0,
        2 | 3 => 1,
        4..=7 => 2,
        _ => 3,
    }
}

fn closest<'a>(typed: &str, options: &'a [String]) -> Option<&'a str> {
    let typed = typed.trim();
    if typed.is_empty() {
        return None;
    }
    for option in options {
        let name = option.trim();
        if name.eq_ignore_ascii_case(typed) && name != typed {
            return Some(name);
        }
    }
    let limit = max_edits(typed);
    if limit == 0 {
        return None;
    }
    let folded = typed.to_ascii_lowercase();
    let mut best: Option<(&str, usize)> = None;
    for option in options {
        let name = option.trim();
        if name.is_empty() || name.eq_ignore_ascii_case(typed) {
            continue;
        }
        let distance = edits(&folded, &name.to_ascii_lowercase());
        if distance == 0 || distance > limit {
            continue;
        }
        match best {
            Some((prev, have)) if distance > have || (distance == have && name > prev) => {}
            _ => best = Some((name, distance)),
        }
    }
    best.map(|(name, _)| name)
}

fn edits(left: &str, right: &str) -> usize {
    let a: Vec<char> = left.chars().collect();
    let b: Vec<char> = right.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut next = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        next[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            next[j + 1] = (prev[j + 1] + 1).min(next[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut next);
    }
    prev[b.len()]
}

/// Command words before `stop`. `--seat` and the name after it are this
/// process, not a command. Any other flag is skipped, and its separate
/// value is left for [`words_at`] to ignore when it is not a command.
fn command_words(args: &[String], stop: &str) -> Vec<String> {
    command_words_until(args, Some(stop))
}

fn command_words_rest(args: &[String]) -> Vec<String> {
    command_words_until(args, None)
}

fn command_words_until(args: &[String], stop: Option<&str>) -> Vec<String> {
    let mut words = Vec::new();
    let mut skip_seat = false;
    for arg in args {
        if stop.is_some_and(|stop| arg == stop) {
            break;
        }
        if skip_seat {
            skip_seat = false;
            continue;
        }
        if arg == "--seat" {
            skip_seat = true;
            continue;
        }
        if arg.starts_with("--seat=") || arg.starts_with('-') {
            continue;
        }
        words.push(arg.clone());
    }
    words
}

fn rewrite_arg(args: &[String], from: &str, to: &str) -> String {
    let mut words = vec!["ljos".to_string()];
    let mut replaced = false;
    for arg in args {
        if !replaced && arg == from {
            words.push(shell_word(to));
            replaced = true;
        } else {
            words.push(shell_word(arg));
        }
    }
    if !replaced {
        words.push(shell_word(to));
    }
    words.join(" ")
}

fn shell_word(word: &str) -> String {
    if word.is_empty()
        || word
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | '\\'))
    {
        format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        word.to_string()
    }
}

/// Print a show lookup failure and exit 1. Any other error stays a fault.
fn show_error(err: anyhow::Error) -> anyhow::Error {
    if ljos_cli::show_memory::is_show_fault(&err) {
        eprintln!("{err}");
        std::process::exit(1);
    }
    ticket_error(err)
}

/// The held issue, as the parent `ljos file` uses.
struct FileHold {
    held: Option<String>,
    /// A hold the tracker does not have. It was cleared.
    dropped: Option<String>,
    /// A hold recorded for another tracker. It is kept for that tracker.
    other_tracker: Option<String>,
}

/// The hold, resolved before any line says a ticket was filed. A hold the
/// tracker does not have is cleared, and an alias becomes the id on disk,
/// so the parent vissue is asked for exists. `skip` is a parent the seat
/// chose, or `--top`.
fn file_hold(skip: bool) -> Result<FileHold> {
    let mut hold = FileHold {
        held: if skip { None } else { ljos_cli::held_issue() },
        dropped: None,
        other_tracker: None,
    };
    if let Some(id) = hold.held.clone() {
        if ljos_cli::hold_names_other_tracker(&id) {
            hold.held = None;
            hold.other_tracker = Some(id);
        }
    }
    if let Some(id) = hold.held.clone() {
        match ljos_cli::resolve_held_parent(&id)? {
            Some(canonical) => hold.held = Some(canonical),
            None => {
                ljos_cli::clear_hold_for(&id);
                hold.held = None;
                hold.dropped = Some(id);
            }
        }
    }
    Ok(hold)
}

/// Print a ticket lookup failure and exit 1.
fn stop_ticket(id: &str) {
    if let Some(line) = ljos_cli::ticket_refusal(id) {
        eprintln!("{line}");
        std::process::exit(1);
    }
}

fn stop_ticket_node(id: &str) {
    let trimmed = id.trim();
    let is_claim_graph = trimmed.len() == 32 && trimmed.bytes().all(|b| b.is_ascii_hexdigit());
    if !trimmed.is_empty() && !is_claim_graph {
        stop_ticket(trimmed);
    }
}

fn ticket_error(err: anyhow::Error) -> anyhow::Error {
    if ljos_cli::is_ticket_fault(&err) {
        eprintln!("{err}");
        std::process::exit(1);
    }
    err
}

/// Where a `ljos note` line goes, and whether the issue was inferred.
///
/// A default mint is the issue. Any other vissue id is the issue when
/// this seat holds nothing, so `ljos note demo-n hello` still reaches the
/// tracker without asking vissue who holds what. While a hold record is
/// present, a word that is not a default mint is text: `ljos-hud` is not
/// taken for an id. A first word that is not an id at all is text on the
/// issue [`ljos_cli::held_issue`] names. An empty or whitespace first word
/// is a blank id, not a missing one.
fn note_target(words: &[String]) -> Result<(String, String, bool)> {
    let Some((first, rest)) = words.split_first() else {
        bail!("{}", ljos_cli::NO_OPEN_SITTING);
    };
    if first.trim().is_empty() {
        bail!("{}", ljos_cli::EMPTY_TICKET_ID);
    }
    if rest.is_empty() && first.contains('\n') {
        bail!(
            "note: an id argument must be one ticket id, and {:?} is not one.",
            first
        );
    }
    if minted_issue_id(first) {
        return explicit_note(first, rest);
    }
    // The hold record is a file. Asking vissue whether a claim is open
    // is the wait a note must not do before it knows which issue it has.
    if vissue_id(first) {
        if let Some(issue) = ljos_cli::seat_held_issue() {
            return Ok((issue, words.join(" "), true));
        }
        return explicit_note(first, rest);
    }
    let Some(issue) = ljos_cli::held_issue() else {
        bail!("{}", ljos_cli::NO_OPEN_SITTING);
    };
    Ok((issue, words.join(" "), true))
}

/// One stderr line when `wrote` is not the open sitting. An inferred id
/// is that sitting. `--quiet` leaves the line off, and does not ask.
/// `finish` names the finished id. `note` says a note was written.
fn warn_other_sitting(wrote: &str, quiet: bool, inferred: bool, finish: bool) {
    if quiet || inferred {
        return;
    }
    let Some(open) = ljos_cli::recorded_sitting() else {
        return;
    };
    if open != wrote {
        let line = if finish {
            ljos_cli::other_finish_line(wrote, &open)
        } else {
            ljos_cli::other_sitting_line(wrote, &open)
        };
        eprintln!("{line}");
    }
}

/// A `block` line that names the edge twice, or names a word that is not an id.
///
/// Following ids are `--on`. `--on` or `--off` on the same line is the other
/// way to name that edge. A word that is not an id is still not a command.
fn block_usage_line(cmd: &Cmd) -> Option<String> {
    let Cmd::Block { also, on, off, .. } = cmd else {
        return None;
    };
    if !also.is_empty() && (on.is_some() || off.is_some()) {
        return Some(
            "block: a following id stands in for --on, so drop --on and --off.".to_string(),
        );
    }
    let word = also.iter().find(|word| !vissue_id(word))?;
    if ticket_like(word) {
        return Some(format!(
            "block: {word} looks like a ticket id, so run `ljos block ISSUE --on {word}`."
        ));
    }
    Some(format!(
        "ljos: no command {word}, so run `ljos block --help`."
    ))
}

/// `ljos sitting TITLE` without `--plan`, when TITLE is not a ticket id.
///
/// The line names `--plan` and a command that files the title. An id
/// vissue will keep is left for the tracker, including an id it does not
/// have.
fn sitting_plan_line(cmd: &Cmd) -> Option<String> {
    let Cmd::Sitting {
        issue: Some(issue),
        plan: None,
        ..
    } = cmd
    else {
        return None;
    };
    let issue = issue.trim();
    if issue.is_empty() || vissue_id(issue) {
        return None;
    }
    let shown = issue.replace('`', "'");
    let quoted = format!("\"{}\"", shown.replace('\\', "\\\\").replace('"', "\\\""));
    Some(format!(
        "ljos: {shown} is not a ticket id, so run `ljos sitting --plan {quoted} -- \"measure the files\"`."
    ))
}

/// An empty or whitespace id on `note` or `finish`.
///
/// This is the id a script captured from `ljos file` when filing timed
/// out. It is not "no id", which means the open sitting.
fn blank_ticket_line(cmd: &Cmd) -> Option<&'static str> {
    let blank = match cmd {
        Cmd::Note { words, .. } => words.first().is_some_and(|word| word.trim().is_empty()),
        Cmd::Finish {
            issue: Some(issue), ..
        } => issue.trim().is_empty(),
        _ => false,
    };
    blank.then_some(ljos_cli::EMPTY_TICKET_ID)
}

fn explicit_note(id: &str, rest: &[String]) -> Result<(String, String, bool)> {
    let text = rest.join(" ");
    if text.is_empty() {
        bail!("ljos note: pass the note after {id}");
    }
    Ok((id.to_string(), text, false))
}

/// Whether `word` is an id `vissue create` mints by default.
///
/// At the vissue commit `.github/sibling-pin` names
/// (`42b15346589fee20cdf62e5984452e099fe67b35`), `generate_id` in
/// `crates/vissue-core/src/store.rs` builds `{project}-` plus `id_length`
/// characters from `0123456789abcdefghijklmnopqrstuvwxyz`, and
/// `IssuesSection` in `config.rs` defaults `id_length` to 4. A shorter
/// hyphenated word such as `ljos-hud` is not that shape.
fn minted_issue_id(word: &str) -> bool {
    let Some((project, suffix)) = vissue_id_parts(word) else {
        return false;
    };
    // Default `IssuesSection.id_length` at the pinned vissue commit, not a
    // length chosen to fit one fixture. `generate_id` clamps a configured
    // length to at least 2, and the unset value is 4.
    const MINTED_SUFFIX_LEN: usize = 4;
    !project.is_empty() && suffix.len() == MINTED_SUFFIX_LEN
}

/// Whether `word` is an id vissue will keep.
///
/// `is_vissue_id` in `crates/vissue-core/src/org.rs` at the same pinned
/// commit, and `validate_explicit_id` in `ops.rs`: `{project}-` plus one
/// or more `0-9a-z`. `ljos note demo-n hello` is this form. It is shorter
/// than the default mint, and it is still the issue when nothing is held.
fn vissue_id(word: &str) -> bool {
    vissue_id_parts(word).is_some()
}

fn ticket_like(word: &str) -> bool {
    let Some((project, suffix)) = word.trim().split_once('-') else {
        return false;
    };
    !project.trim().is_empty()
        && !suffix.trim().is_empty()
        && project.chars().all(|c| c.is_ascii_alphanumeric())
        && suffix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn vissue_id_parts(word: &str) -> Option<(&str, &str)> {
    let (project, suffix) = word.rsplit_once('-')?;
    (!project.is_empty()
        && !suffix.is_empty()
        && suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()))
    .then_some((project, suffix))
}

fn looks_like_ticket_id(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return false;
    }
    // A claim-graph node is 32 hex, the same form `ljos release` accepts.
    if trimmed.len() == 32 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return true;
    }
    if let Some((project, suffix)) = vissue_id_parts(trimmed) {
        return project.split('/').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        }) && !suffix.is_empty();
    }
    ticket_like(trimmed)
}

fn recall_search_command(text: &str) -> String {
    let arg = if text
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '"' | '\\' | '$' | '`' | '!'))
    {
        if text.chars().any(char::is_control) {
            let escaped = text
                .replace('\\', "\\\\")
                .replace('\'', "\\'")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            format!("$'{escaped}'")
        } else {
            format!("'{}'", text.replace('\'', "'\\''"))
        }
    } else {
        format!("\"{text}\"")
    };
    if text.starts_with('-') {
        format!("ljos search -- {arg}")
    } else {
        format!("ljos search {arg}")
    }
}

/// A node named on the command line. A blank one is an error at once,
/// not the next ready node: only an omitted id claims next.
fn named_node(raw: &str) -> Result<String, String> {
    let node = raw.trim();
    if node.is_empty() {
        return Err(ljos_cli::EMPTY_CLAIM_ID.to_string());
    }
    Ok(node.to_string())
}

/// The write acknowledgement: the plain sentence for a person at a
/// terminal, the tab-separated row for a pipe.
/// Color on a terminal, unless `NO_COLOR` is set. A pipe stays plain.
fn human_color() -> bool {
    ljos_cli::scan::color_allowed(std::io::stdout().is_terminal())
}

/// `--verbose`, or `LJOS_DEBUG`, prints the tracker and sync lines.
fn git_detail(verbose: bool) -> bool {
    verbose || ljos_cli::fault_detail()
}

/// The lines a write shows. `--json` is one object whose `text` is that
/// view and whose `tracker_git` and `sync` fields keep every original line.
fn render_write(full: &str, detail: bool, json: bool, id: Option<&str>) -> String {
    if json {
        let text = if detail {
            full.to_string()
        } else {
            ljos_cli::without_routine_git(full)
        };
        ljos_cli::format_rendered_json(&text, full, id)
    } else if detail {
        full.to_string()
    } else {
        ljos_cli::without_routine_git(full)
    }
}

/// The id `ljos search` prints, and the text that was stored.
fn stored_claim(body: &serde_json::Value, fallback: &str) -> (String, String) {
    let id = body["id"].as_str().unwrap_or("");
    let mut short: String = id.chars().take(8).collect();
    if short.is_empty() {
        short = "?".to_string();
    }
    let text = body["text"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or(fallback.trim());
    (short, text.to_string())
}

/// Stdout when one remember stored several claims. The first line says it
/// was split. Each claim then names its id and its text, so the first
/// sentence is not dropped from the page.
fn split_claims_page(n: usize, rows: &[(String, String)]) -> String {
    let mut out = ljos_cli::remember_split_line(n);
    out.push('\n');
    for (id, text) in rows {
        out.push_str("id ");
        out.push_str(id);
        out.push('\n');
        out.push_str(text);
        out.push('\n');
    }
    out
}

fn write_ack(body: &serde_json::Value, verbose: bool) -> String {
    // The pack keeps a person's hard sentence and says why it would have
    // refused it; that goes to stderr, so the one line a script reads
    // stays one line. A sentence past 29 words is printed only with
    // `--verbose`.
    if let Some(note) = ljos_cli::prose_warning_shown(body, verbose) {
        eprintln!("{note}");
    }
    // A pipe prints the same lines as a terminal. `--json` is the stored row.
    ljos_cli::format_write_ack_plain(body, &ljos_cli::local_when)
}

/// Print a relay fault in the runner's field names, an allow with a
/// note, and exit 0.
fn exit_relay_fault(runner: &str, event: &str, what: &str) -> ! {
    eprintln!("ljos: the guard could not judge ({what}); the call is allowed");
    let (out, err) = ljos_cli::runner_hooks::relay_fault_answer(runner, event, what);
    print!("{out}");
    eprint!("{err}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::Write::flush(&mut std::io::stderr());
    std::process::exit(0);
}

fn main() {
    // Both run before the seat's environment, the tracker or the pack is
    // read: the server stays small, and a hook call waits its turn before
    // it costs anything.
    if std::env::args_os().nth(1).is_some_and(|a| a == "hookd") {
        std::process::exit(ljos_cli::hookd::serve());
    }
    ljos_cli::hookd::admit();
    let ran = run_command();
    if let Err(e) = &ran {
        if e.to_string().contains(ljos_cli::MEMORY_STARTING_LINE) {
            eprintln!("{}", ljos_cli::MEMORY_STARTING_LINE);
            std::process::exit(1);
        }
        let args: Vec<String> = std::env::args().skip(1).collect();
        ljos_cli::memory_port::record_memory_error(&args, &format!("{e:#}"));
        if let Some(label) = ljos_cli::policy_label::as_policy(e) {
            if label.label == "close-unverified" {
                eprintln!("{}", label.detail);
            } else {
                println!("label {}: {}", label.label, label.detail);
            }
            // A seat that was not resolved or is not on the roster, and
            // an unseen mail recipient, are failed commands. 2 is the
            // status a bad invocation already uses, so a script that
            // checks the status sees the failure. Other labels are notes
            // and still exit 0.
            if matches!(
                label.label,
                "seat-unresolved" | "seat-unlisted" | "recipient-unseen"
            ) || (label.label == "reopen-warning"
                && label.detail.contains("name different seats"))
            {
                std::process::exit(USAGE_STATUS);
            }
            return;
        }
        if ljos_cli::is_read_shed(e) || ljos_cli::is_deadline_miss(e) {
            // A hook must not fail the runner. A read that was shed, or a
            // request the writer dropped because its deadline passed, is
            // one line and is not exit 75.
            let hook_cmd = std::env::args_os().nth(1).is_some_and(|arg| arg == "hook");
            if hook_cmd {
                std::process::exit(0);
            }
            eprintln!(
                "{}",
                ljos_cli::shed_queue_line(&ljos_cli::pack_fault_line(e))
            );
            std::process::exit(1);
        }
        if ljos_cli::is_pack_busy(e) || ljos_cli::is_write_shed(e) {
            // `ljos hook` answers the runner and must not fail it: a
            // silent pack is not a refused command. Every other command,
            // including remember with `LJOS_IN_HOOK` still set, exits 75
            // so a script can see the pack did not take the write.
            let hook_cmd = std::env::args_os().nth(1).is_some_and(|arg| arg == "hook");
            if hook_cmd {
                std::process::exit(0);
            }
            let line = if ljos_cli::is_write_shed(e) {
                ljos_cli::shed_queue_line(&ljos_cli::pack_fault_line(e))
            } else if let Some(line) = ljos_cli::busy_queue_line() {
                line.to_string()
            } else {
                ljos_cli::PACK_BUSY_LINE.to_string()
            };
            eprintln!("{line}");
            std::process::exit(ljos_cli::PACK_BUSY_EXIT);
        }
        if let Some(line) = ljos_cli::plain_server_fault(e) {
            ljos_cli::say_fault(&line);
            std::process::exit(if ljos_cli::writer_retries(&line) {
                ljos_cli::PACK_BUSY_EXIT
            } else {
                1
            });
        }
        // A gate whose hook fails answers allow in the runner's JSON with
        // the note that says why, and exits 0: a broken ljos blocks nothing.
        ljos_cli::answer_failed_gate(&format!("{e:#}"));
        let line = ljos_cli::user_fault_line(e);
        eprintln!("{line}");
        if ljos_cli::fault_detail() {
            let detail = format!("{e:#}");
            if ljos_cli::fault_detail_worth_showing(&detail, &line) {
                eprintln!("{detail}");
            }
        }
        std::process::exit(ljos_cli::user_fault_exit(e, &line));
    }
}

/// What ljos is. The same words as the command's about line.
const ABOUT_LINE: &str = "Memory, a work tracker and a command guard for coding runners";

/// The command reference a bare `ljos` names.
const DOCS_URL: &str = "https://leidarljos.github.io/ljos/reference-commands.html";

/// The first commands, in the order the getting-started page runs them.
const FIRST_COMMANDS: &[(&str, &str)] = &[
    ("sh install.sh --no-embed", "install ljos"),
    ("ljos doctor", "check that ljos is ready"),
    ("ljos remember TEXT", "store one lesson"),
    ("ljos search QUERY", "find what is stored"),
];

fn installed_ljos() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let path = std::fs::canonicalize(&exe).unwrap_or(exe);
    !ljos_cli::exe_from_cargo_build(&path)
}

/// What `ljos` with no arguments prints.
///
/// One line for what ljos is, the seat a read would use and where that
/// came from, the first commands, and where the docs are. Load the env
/// file and prepare a read before calling this, so the seat line is the
/// one [`ljos_cli::doctor_seat_source`] prints. A read that matched no
/// seat says that in plain words, not the doctor's rule phrase.
fn opening_screen() -> String {
    let seat = seat_screen_line(&ljos_cli::doctor_seat_source().state);
    let commands_src: Vec<(&str, &str)> = FIRST_COMMANDS
        .iter()
        .copied()
        .filter(|(cmd, _)| *cmd != "sh install.sh --no-embed" || !installed_ljos())
        .collect();
    let width = commands_src
        .iter()
        .map(|(cmd, _)| cmd.len())
        .max()
        .unwrap_or(0);
    let mut commands = String::new();
    for (cmd, about) in commands_src {
        let pad = width + 2 - cmd.len();
        commands.push_str("  ");
        commands.push_str(cmd);
        commands.push_str(&" ".repeat(pad));
        commands.push_str(about);
        commands.push('\n');
    }
    format!("{ABOUT_LINE}.\n\n{seat}\n\n{commands}\nThe docs are at {DOCS_URL}.\n")
}

/// The seat line on the bare screen.
///
/// A named seat keeps the doctor's `named by` phrase. A read that matched
/// no seat says which seat it uses and why, and names the next command.
fn seat_screen_line(state: &str) -> String {
    if state.starts_with("no seat rule matched; reading every seat")
        || state.starts_with("no seat is set; reads use every seat")
    {
        return "No seat is set. Reads use every seat, so run `export LJOS_SEAT=NAME`.".to_string();
    }
    if let Some((name, source)) = state.split_once(" named by ") {
        if !name.is_empty() && !name.contains(' ') && !source.is_empty() {
            return format!("This seat is {name}, named by {source}.");
        }
    }
    if state.ends_with('.') {
        state.to_string()
    } else {
        format!("{state}.")
    }
}

/// True when the only argument is the help flag. A subcommand's own
/// `--help`, and `ljos help`, are left for clap.
fn root_help_only() -> bool {
    let mut args = std::env::args_os().skip(1);
    let Some(first) = args.next() else {
        return false;
    };
    (first == "--help" || first == "-h") && args.next().is_none()
}

/// `ljos help`, and `ljos help --all`. A named command prints that
/// command's own help, hidden or not. A word that is not a command
/// names the closest one and exits 2.
fn show_help(all: bool, topic: &[String]) -> Result<()> {
    let mut cmd = Cli::command();
    cmd.build();
    if topic.is_empty() {
        print!("{}", help_groups::render(&cmd, all));
        return Ok(());
    }
    let mut current = &cmd;
    for (i, name) in topic.iter().enumerate() {
        if let Some(sub) = current.find_subcommand(name) {
            current = sub;
            continue;
        }
        if let Some(line) = listing_sentence(name).filter(|_| current.get_name() == "ljos") {
            eprintln!("{line}");
            std::process::exit(USAGE_STATUS);
        }
        if current.get_name() == "ljos" && lessons_topic(name) {
            eprintln!("{}", lessons_sentence(name, &[]));
            std::process::exit(USAGE_STATUS);
        }
        let verb: Vec<String> = topic[..i].to_vec();
        if let Some(line) = ticket_id_sentence(name, &verb, ticket_form(current).as_ref()) {
            eprintln!("{line}");
            std::process::exit(USAGE_STATUS);
        }
        let names = child_names(current);
        let picked = if current.get_name() == "ljos" {
            root_synonym(name).map(str::to_string)
        } else {
            None
        };
        let picked = picked.or_else(|| pick_verb(name, &names));
        let line = match picked {
            Some(real) => {
                let mut fixed: Vec<&str> = topic.iter().map(String::as_str).collect();
                fixed[i] = &real;
                format!(
                    "ljos: no command {name}, and the closest is {real}, so run `ljos help {}`.",
                    fixed.join(" ")
                )
            }
            None => format!("ljos: no command {name}, so run `ljos help`."),
        };
        eprintln!("{line}");
        std::process::exit(USAGE_STATUS);
    }
    current.clone().print_help()?;
    println!();
    Ok(())
}

/// A queued lesson may go out on this command. Reads, dry runs, onboard,
/// offboard, uninstall and doctor do not create the queue or its lock,
/// and they do not rewrite a queue that is already there.
fn queued_lesson_may_go(cmd: &Cmd) -> bool {
    // A search with no query and no filters exits as usage. Keep that one
    // line clean and skip queue drain noise.
    if let Cmd::Search {
        query,
        source_machine,
        source_seat,
        import_id,
        ..
    } = cmd
    {
        if join(query).trim().is_empty()
            && source_machine.is_none()
            && source_seat.is_none()
            && import_id.is_none()
        {
            return false;
        }
    }
    // A read-only proposal list must not drain the note queue. That drain
    // takes the queue lock and waits on the writer.
    if let Cmd::Proposals {
        cmd: None,
        settle: false,
        ..
    } = cmd
    {
        return false;
    }
    // A stale release writes only after it has found a claim to hand
    // back. The check is a tracker read. An empty list does not open
    // the pack, so the lesson queue stays where it is.
    if let Cmd::Release { stale: true, .. } = cmd {
        return false;
    }
    // An unchanged import, integrate, export or mesh pull must not wait
    // on a lesson queued for a busy pack. The queue still drains on the
    // next search, remember or session start.
    if matches!(
        cmd,
        Cmd::Memory { .. }
            | Cmd::Export { .. }
            | Cmd::Mesh {
                cmd: MeshCmd::Pull { .. },
            }
    ) {
        return false;
    }
    !matches!(
        cmd,
        Cmd::Remember { .. }
            | Cmd::Note { .. }
            | Cmd::File { .. }
            | Cmd::Parent { .. }
            | Cmd::Sitting { .. }
            | Cmd::Finish { .. }
            | Cmd::Hook(_)
            | Cmd::Help { .. }
            | Cmd::Mcp { .. }
            | Cmd::Completions { .. }
            | Cmd::Doctor { .. }
            | Cmd::Record { .. }
            | Cmd::Onboard { .. }
            | Cmd::Offboard { .. }
            | Cmd::Uninstall { .. }
            | Cmd::Sync { dry_run: true, .. }
            | Cmd::Panel { dry_run: true, .. }
            | Cmd::Agents { dry_run: true, .. }
            | Cmd::BumpPlan { dry_run: true, .. }
            | Cmd::Memory {
                cmd: MemoryCmd::Integrate { dry_run: true, .. },
            }
    )
}

/// Arguments for `ljos-mcp` when this process is `ljos mcp` and should
/// serve. `None` for every other command, and for `ljos mcp --help`, which
/// keeps the command page.
fn mcp_serve_args() -> Option<Vec<std::ffi::OsString>> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let mut index = 0;
    while index < args.len() {
        let word = args[index].to_string_lossy();
        if word == "--" {
            return None;
        }
        if word == "--seat" {
            index += 2;
            continue;
        }
        if let Some(value) = word.strip_prefix("--seat=") {
            if value.is_empty() {
                return None;
            }
            index += 1;
            continue;
        }
        if word.starts_with('-') {
            return None;
        }
        break;
    }
    match args.get(index) {
        Some(arg) if arg == "mcp" => {}
        _ => return None,
    }
    let rest = &args[index + 1..];
    if rest.iter().any(|arg| arg == "--help" || arg == "-h") {
        return None;
    }
    Some(rest.to_vec())
}

/// Pull `--seat NAME` off the command line. It names this process, and
/// it wins over `LJOS_SEAT`. `log`, `memories` and `proposals` keep a
/// `--seat` that comes after the command, because there it filters.
fn take_seat_flag(args: &[std::ffi::OsString]) -> (Option<String>, Vec<std::ffi::OsString>) {
    let mut command_at = None;
    for (index, arg) in args.iter().enumerate().skip(1) {
        if !arg.to_string_lossy().starts_with('-') {
            command_at = Some(index);
            break;
        }
    }
    let command = command_at
        .and_then(|index| args.get(index))
        .map(|arg| arg.to_string_lossy().into_owned());
    // These commands already use `--seat` for their own argument, after the command name.
    let keep_later = matches!(
        command.as_deref(),
        Some("log" | "memories" | "proposals" | "mesh")
    );
    let mut seat = None;
    let mut kept = Vec::with_capacity(args.len());
    if let Some(first) = args.first() {
        kept.push(first.clone());
    }
    let mut index = 1;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let after = command_at.is_some_and(|at| index > at);
        let steal = !(keep_later && after) && (arg == "--seat" || arg.starts_with("--seat="));
        if steal {
            if let Some(value) = arg.strip_prefix("--seat=") {
                if !value.is_empty() {
                    seat = Some(value.to_string());
                }
            } else if let Some(value) = args.get(index + 1) {
                seat = Some(value.to_string_lossy().into_owned());
                index += 1;
            }
            index += 1;
            continue;
        }
        kept.push(args[index].clone());
        index += 1;
    }
    (seat, kept)
}

/// Reads look at every seat when none of the rules match. The writes
/// that used to stop, and remember, note and file, use the login name,
/// or a short stable name when that login is missing.
fn seat_kind(cmd: &Cmd) -> ljos_cli::seat_resolve::Kind {
    use ljos_cli::seat_resolve::Kind;
    match cmd {
        Cmd::Search { .. }
        | Cmd::Recall { .. }
        | Cmd::Doctor { .. }
        | Cmd::Statusline
        | Cmd::Seat { action: None }
        | Cmd::Mesh {
            cmd: MeshCmd::Status,
        } => Kind::Read,
        Cmd::Note { .. }
        | Cmd::File { .. }
        | Cmd::Parent { .. }
        | Cmd::Sitting { .. }
        | Cmd::Claim { .. }
        | Cmd::Slot { .. }
        | Cmd::Finish { .. }
        | Cmd::Release { .. }
        | Cmd::Complete { .. }
        | Cmd::Options { .. }
        | Cmd::Deed { .. }
        | Cmd::Table { .. }
        | Cmd::Seat { action: Some(_) } => Kind::Write,
        _ => Kind::Keep,
    }
}

fn run_hook(hook: HookArgs) -> Result<()> {
    let HookArgs {
        limit,
        event,
        prompt,
        text,
        fail_on_deny,
        runner,
        git_commit,
        git_hook,
        brief_once,
        drain,
        no_wait,
        apply_book,
        finish_session,
        judge_worker,
    } = hook;
    if judge_worker {
        return ljos_cli::run_jev_judge_worker();
    }
    if !brief_once {
        ljos_cli::config_diag::emit();
    }
    if text {
        return prompt_as_text(limit);
    }
    if drain {
        std::process::exit(ljos_cli::book_queue::drain(!no_wait));
    }
    if !apply_book.is_empty() {
        std::process::exit(ljos_cli::book_queue::apply(&apply_book));
    }
    if git_commit {
        let _wall = ljos_cli::arm_hook_wall_budget();
        return ljos_cli::git_hooks::run_git_commit(git_hook.as_deref().unwrap_or("post-commit"));
    }
    if let Some(path) = finish_session {
        return ljos_cli::extract::run_finish_job(&path);
    }
    if brief_once {
        ljos_cli::bookkeep::print_brief_once();
        return Ok(());
    }
    use std::io::Read;
    let bytes = match ljos_cli::hookd::take_stdin() {
        Some(bytes) => bytes,
        None => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes)?;
            bytes
        }
    };
    if !prompt {
        if let Some(code) = ljos_cli::answer_cursor_read_bytes(&bytes) {
            std::process::exit(code);
        }
    }
    let input = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            // The queue path judges a side event from a lossy copy.
            // Do the same here, so invalid UTF-8 still prints an
            // answer instead of exiting with an empty stdout.
            let raw = err.into_bytes();
            let lossy = String::from_utf8_lossy(&raw);
            if !prompt {
                if let Some(code) = ljos_cli::emit_cursor_side_answer(&lossy, event.as_deref()) {
                    std::process::exit(code);
                }
            }
            // The relay reads stdin before the gate's panic hook is
            // armed. Invalid text used to exit 1 with no output,
            // which nine runners read as an allow.
            if let Some(name) = runner.as_deref() {
                if ljos_cli::runner_hooks::is_runner(name) {
                    exit_relay_fault(name, event.as_deref().unwrap_or(""), "stdin is not utf-8");
                }
            }
            anyhow::bail!("hook: stdin is not utf-8");
        }
    };
    let _wall = ljos_cli::arm_hook_wall_budget();
    ljos_cli::log_recall("hook", event.as_deref().unwrap_or("-"));
    if let Some(runner) = runner.as_deref() {
        if !ljos_cli::runner_hooks::is_runner(runner) {
            anyhow::bail!(
                "hook: no runner {runner:?}; --runner takes {}",
                ljos_cli::runner_hooks::RUNNERS.join(", ")
            );
        }
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ljos_cli::runner_hooks::relay(runner, &input, limit, event.as_deref())
        }));
        match caught {
            Ok(code) => std::process::exit(code),
            Err(_) => exit_relay_fault(runner, event.as_deref().unwrap_or(""), "panicked"),
        }
    }
    let mut call = if prompt {
        ljos_cli::prompt_call(&input)
    } else {
        ljos_cli::hook_call_as(&input, event.as_deref())
    };
    // Arm before bind or the spawn note. Both can take
    // tmux-sock.lock, and a blocking take runs past the cut-off.
    if !prompt {
        ljos_cli::hook_timed("arm_gate", || ljos_cli::arm_gate(&input, event.as_deref()));
    }
    ljos_cli::bind_call_conversation(&input, &call);
    ljos_cli::adopt_blank_conversation(&input, &mut call);
    ljos_cli::ballot_sig::export_hook_session(call.session.as_deref());
    if ljos_cli::subagent::note_hook_call(&input, &call.event) {
        // the editor harness blocks a permission hook that prints nothing.
        // A seat tool named mcp__SERVER__ljos_NAME is that hook
        // when SERVER is a key in the seat's MCP config.
        if call.shape.is_cursor() {
            print!("{}", ljos_cli::hook_output_ruled(&call, "", None));
        }
        return Ok(());
    }
    // The queue is off, or this process was already inside a hook.
    // Answer before the pack search. The deadline is already armed.
    // The seat env was loaded above. A promoted rule may ask the
    // pack once from inside that answer. The call is recorded
    // first, so a bare seat can still bind the conversation.
    if !prompt {
        if let Some(code) = ljos_cli::emit_cursor_side_answer(&input, event.as_deref()) {
            std::process::exit(code);
        }
    }
    // A runner the seat asked for a judgment hears nothing from the
    // seat, so the judgment cannot open sittings or judge again;
    // the law on its tool calls still holds.
    if std::env::var_os("LJOS_JUDGE").is_some() && call.event != "PreToolUse" {
        print!("{}", ljos_cli::hook_output_ruled(&call, "", None));
        return Ok(());
    }
    // A context event (a prompt, a tool result) says what the seat
    // knows, and saying nothing is a correct answer; a runner that
    // cuts the hook off throws the answer away and says it failed.
    // So those events answer inside a deadline, whatever the pack
    // does, and a call a second registration of the same hook makes
    // at the same moment is answered once. A tool gate is exempt
    // from both: its verdict must not be lost to a clock. Exiting
    // at the deadline writes no deny, and the runner then allows
    // the command; a shell hook's deadline, armed above, writes one.
    // The same holds for an event said once, such as a stop or the
    // end of a session: its twin is answered with nothing.
    if ljos_cli::hook_twin(&call, &input) {
        return Ok(());
    }
    if matches!(call.event.as_str(), "UserPromptSubmit" | "PostToolUse") {
        if ljos_cli::hook_already_running(&call, &input) {
            if ljos_cli::replays_its_copy(&call) {
                let wait = std::time::Duration::from_millis(ljos_cli::HOOK_DEADLINE_MS);
                if let Some(answer) = ljos_cli::replay_hook_answer(&call, &input, wait) {
                    print!("{answer}");
                }
            }
            return Ok(());
        }
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(ljos_cli::HOOK_DEADLINE_MS));
            // Taking the lock waits out an answer being written.
            let _held = std::io::stdout().lock();
            std::process::exit(0);
        });
    }
    // A hook answers within the runner's timeout: lookups that walk
    // the whole tracker are skipped from here on.
    // SAFETY: single-threaded here, before anything reads the environment.
    unsafe { std::env::set_var("LJOS_IN_HOOK", "1") };
    let _pack_clock = ljos_cli::arm_hook_pack();
    let _side_bound = ljos_cli::arm_side_bound(&call);
    let (subagent, stop_active, agent) = ljos_cli::hook_subagent(&input);
    ljos_cli::hook_trace(&input, &call, subagent.as_deref());
    // A subagent about to stop is held once while its parent holds
    // an issue, so its result reaches the issue as a ballot or a
    // lesson instead of ending in the parent's context.
    if call.event == "SubagentStop" {
        let kind = subagent.as_deref().unwrap_or("subagent");
        let key = format!("subagent-gate:{agent}");
        let seen = ljos_cli::seen_ids(call.session.as_deref());
        if !seen.contains(&key) {
            let issue = ljos_cli::held_issue();
            let decision = issue.as_deref().is_some_and(|i| {
                ljos_cli::tracker_show_json(i).is_ok_and(|v| ljos_cli::is_decision(&v))
            });
            if let Some(reason) =
                ljos_cli::subagent_stop_reason(kind, issue.as_deref(), decision, stop_active)
            {
                println!("{}", ljos_cli::block_output(call.shape, &reason));
                let _ = std::io::Write::flush(&mut std::io::stdout());
                ljos_cli::mark_seen(call.session.as_deref(), &[key]);
                return Ok(());
            }
        }
        if let Some(reason) = ljos_cli::stop_audit(&input, stop_active) {
            println!("{}", ljos_cli::block_output(call.shape, &reason));
        }
        return Ok(());
    }
    // An agent about to end its turn is audited once: a done claim
    // beside a red test run, or asked work put off, holds it for
    // one more round. An agy stop with `fullyIdle` has ended its loop
    // and skips that audit. It finishes no claim: the session finish is
    // the SessionEnd branch below.
    let agy_idle = call.shape == ljos_cli::HookShape::Steps
        && call.event == "Stop"
        && ljos_cli::stop_is_idle(&input);
    if call.event == "Stop" && call.shape != ljos_cli::HookShape::Context && !agy_idle {
        // At a usage limit the stores hear what the conversation
        // knows before the runner cuts it off.
        if let Some(reason) = ljos_cli::limit_stop(&input, call.session.as_deref()) {
            println!("{}", ljos_cli::block_output(call.shape, &reason));
            return Ok(());
        }
        if let Some(reason) = ljos_cli::seat_stop_reason(&input, stop_active, subagent.is_some()) {
            println!("{}", ljos_cli::block_output(call.shape, &reason));
            return Ok(());
        }
        if let Some(reason) = ljos_cli::stop_audit(&input, stop_active) {
            println!("{}", ljos_cli::block_output(call.shape, &reason));
            return Ok(());
        }
        // Interrupt mail that came in during the turn holds the
        // stop once, so a running session reads it before it goes
        // idle. Each letter is receipted once delivered, so the
        // next stop is not held for it again.
        if subagent.is_none() {
            let (mail, ids) = ljos_cli::mail::stop_note();
            if !mail.is_empty() {
                println!("{}", ljos_cli::block_output(call.shape, &mail));
                let _ = std::io::Write::flush(&mut std::io::stdout());
                ljos_cli::mail::ack_delivered(&ids);
                return Ok(());
            }
        }
    }
    // At the end of a session the memories it used fire together,
    // and the transcript is read, in a detached process. This
    // process does not wait on the pack. It waits on the tracker
    // only to commit writes the batch window still holds.
    if call.event == "SessionEnd" {
        let _ = ljos_cli::flush_held_tracker_writes();
        ljos_cli::extract::at_session_end(&input, call.session.as_deref());
        return Ok(());
    }
    // On a tool call the pack's rules give a verdict; on a prompt
    // there is nothing to stop, only something to know. A force
    // push, a seat deny, and an `rm -rf` are judged in this
    // process first, so a slow pack or a slow guard is not what
    // the runner waits on.
    let on_tool = call.event == "PreToolUse" || call.event == "argv";
    if on_tool {
        let _ = ljos_cli::take_guard_note();
    }
    // The tool shell does not exist yet. Record the session, cgroup
    // and this hook's start time while the runner that spawned the
    // guard is still the parent, so a later detach is still the call.
    if call.event == "PreToolUse" {
        match ljos_cli::proc_tree::hook_runner() {
            Ok((runner, _)) => {
                let _ = ljos_cli::proc_tree::note_tool_call(&runner);
            }
            Err(why) if why.contains("cannot be read") => {
                ljos_cli::proc_tree::seal_unreadable_tool_call();
            }
            Err(_) => {}
        }
    }
    let resolved = ljos_cli::command_directory(input.trim());
    let cwd = match &resolved {
        Ok(dir) => Some(dir.clone()),
        Err(_) => ljos_cli::fallback_directory(input.trim()),
    };
    let posted = matches!(call.event.as_str(), "PreToolUse" | "beforeMCPExecution")
        .then(|| ljos_cli::user_rules::guard_payload(&input))
        .flatten();
    // The static guard includes the commit, push, and merge gates,
    // in the directory the tool would run in.
    let dir = cwd.as_deref();
    // LJOS_OFF and a temporary HOME switch the guard off. The push
    // gate and the pack's rules stay out too, as the shim does.
    // The sitting gate does not.
    let suspended = on_tool && ljos_cli::bookkeep::guard_suspended();
    let fast = if suspended {
        ljos_cli::sitting_gate::guard_line(&call.cue, dir)
    } else {
        on_tool
            .then(|| ljos_cli::static_guard_in(&call.cue, dir))
            .flatten()
            .or(posted)
    };
    let (rules, preferences) = if on_tool && fast.is_none() && !suspended {
        ljos_cli::gate_read()
    } else {
        (Vec::new(), Vec::new())
    };
    let argv: Vec<String> = call.cue.split_whitespace().map(String::from).collect();
    // A tool call with no command line (a file read, a search) has
    // no argv for the law to judge; the TCB sees only shell lines.
    // A command whose directory does not resolve does not run in
    // the fallback, so what the guard reads there does not judge it.
    let unresolved_dir = resolved.is_err()
        && on_tool
        && !suspended
        && !argv.is_empty()
        && !ljos_cli::subagent::payload_names_mcp_tool(&input);
    if unresolved_dir {
        eprintln!(
            "ljos: the guard could not judge (the directory does not resolve); the call is allowed"
        );
    }
    let tcb_rule = if unresolved_dir {
        None
    } else if let Some(rule) = fast {
        // ljos's own rule warns. A named policyd config may still
        // block the line, and that block is the verdict.
        (!suspended && !argv.is_empty())
            .then(|| ljos_cli::policyd_block(&call.cue, dir))
            .flatten()
            .or(Some(rule))
    } else if suspended {
        None
    } else if on_tool && !argv.is_empty() {
        ljos_cli::tcb_after_static(&call.cue, dir, true)
    } else if on_tool {
        ljos_cli::seat_guard(&call.cue)
    } else {
        None
    };
    // An ordinary branch or tag push is allowed. Force, a
    // +refspec, a remote-branch delete and a mirror push run with
    // a warning.
    let gated = ljos_cli::redirect_seat_verb(
        ljos_cli::gate_push(
            ljos_cli::ruled_verdict(&rules, &call.cue).as_ref(),
            &call.cue,
            cwd.as_ref().and_then(|p| p.to_str()),
        ),
        &call.cue,
    );
    // The call runs. One rule is its warning: the guard's, else the
    // push gate's or a seat rule's. The cloud seat push rule is the
    // one rule that stops it.
    let verdict = tcb_rule.as_ref().or(gated.as_ref());
    // A command the ask pack would have asked about runs because a
    // setting turned that class off: the bypass log keeps a row.
    if verdict.is_none() && call.event == "PreToolUse" {
        if let Some((rule, how)) = ljos_cli::ask_pack::ask_pack_switched_off(&call.cue) {
            ljos_cli::log_bypass("switch", &how, &call.cue, &rule);
        }
    }
    // Ids are marked after stdout is flushed. A runner that kills
    // the hook before it reads the answer must see the same note
    // on the next call.
    let mut seen_later: Vec<String> = Vec::new();
    let mut ack_nudge = false;
    // A lesson queued while the pack was down, or left as a
    // proposal by an older ljos, is sent before this session reads
    // memory, so the next prompt can recall it.
    if matches!(call.event.as_str(), "SessionStart" | "UserPromptSubmit") {
        let _ = ljos_cli::flush_note_queue();
        ljos_cli::admit::import_open(ljos_cli::admit::IMPORT_SLICE_MS);
    }
    // Search on the prompt. A camel-case runner discards that
    // stdout, so each held note goes out on the next tool result.
    // Stop additionalContext starts another round, so that runner
    // does not print a held note on Stop. PreToolUse only decides.
    // A bare command is an argv. The guard above is unchanged, and
    // the note is the recall a prompt would hand that line.
    // Printed after the recalled lessons, once. A label, not a refusal.
    let mut origin_note: Option<String> = None;
    let mut context = match call.event.as_str() {
        "PreToolUse" | "argv" => {
            // A seat verb about to run resets the work count.
            let _ = ljos_cli::work_nudge(&call, subagent.is_some());
            // The preferences that name the command about to run
            // go with it, once a session each, read from the
            // listing the rules came from.
            let note = if call.event == "PreToolUse" {
                let (note, keys) = ljos_cli::command_preference_note(
                    &preferences,
                    &call.cue,
                    call.session.as_deref(),
                );
                seen_later.extend(keys);
                note
            } else {
                let (note, keys) = hook_note(&call, limit);
                seen_later.extend(keys);
                note
            };
            // A timed-out or crashed guard allowed this call. Say so.
            let note = match ljos_cli::take_guard_note() {
                Some(guard) if note.is_empty() => guard,
                Some(guard) => format!("{guard}\n{note}"),
                None => note,
            };
            // A push whose tip has no passing test record runs,
            // and the agent reads why it was not held.
            match ljos_cli::gates::take_push_warning() {
                Some(warning) if note.is_empty() => warning,
                Some(warning) => format!("{warning}\n{note}"),
                None => note,
            }
        }
        "PostToolUse" => {
            let (mut ctx, ids) = post_hook_stdout(call.shape, call.session.as_deref());
            seen_later.extend(ids);
            if let Some(nudge) = ljos_cli::work_nudge(&call, subagent.is_some()) {
                ack_nudge = true;
                ctx = if ctx.is_empty() {
                    nudge
                } else {
                    format!("{ctx}\n{nudge}")
                };
            }
            if let Some(kind) = subagent.as_deref() {
                let key = format!("subagent-brief:{agent}");
                if !ljos_cli::seen_ids(call.session.as_deref()).contains(&key) {
                    if let Some(issue) = ljos_cli::held_issue() {
                        let decision = ljos_cli::tracker_show_json(&issue)
                            .is_ok_and(|v| ljos_cli::is_decision(&v));
                        seen_later.push(key);
                        let brief = ljos_cli::subagent_brief(kind, &issue, decision);
                        ctx = if ctx.is_empty() {
                            brief
                        } else {
                            format!("{brief}\n{ctx}")
                        };
                    }
                }
            }
            ctx
        }
        // A turn with no tool never fired PostToolUse. Taking the
        // hold here keeps it off the next turn. A camel-case runner
        // does not print it: Stop additionalContext starts another
        // round. The audit block above is the hold that still speaks.
        "Stop" => {
            let (ctx, ids) = stop_hook_stdout(call.session.as_deref(), stop_active);
            let ctx = ljos_cli::stop_context_for_runner(call.shape, ctx);
            if call.shape != ljos_cli::HookShape::CamelCase {
                seen_later.extend(ids);
            }
            ctx
        }
        // The pack is searched on a failed tool's command and its
        // error; what bears on the failure goes back with the
        // result.
        "PostToolUseFailure" => {
            let error = ljos_cli::tool_error(&input);
            let (ctx, ids) = ljos_cli::failure_note(&call, &error, 3);
            seen_later.extend(ids);
            ctx
        }
        // Compaction drops what the seat handed the conversation:
        // those memories leave the seen list, and up to eight
        // fire together when two or more were handed over. A held
        // issue is said again on the next delivery.
        "PreCompact" => {
            let _ = ljos_cli::rearm_after_compaction(call.session.as_deref());
            String::new()
        }
        // A session started from a compaction hears what it holds,
        // and a fresh one hears that ljos is on, where the runner
        // takes SessionStart context.
        "SessionStart" => {
            let compacted = serde_json::from_str::<serde_json::Value>(input.trim())
                .ok()
                .and_then(|v| {
                    v.get("source")
                        .and_then(|s| s.as_str())
                        .map(|s| s == "compact")
                })
                .unwrap_or(false);
            let ctx = if compacted && call.shape != ljos_cli::HookShape::CamelCase {
                if ljos_cli::peek_hook_context(call.session.as_deref()).is_empty() {
                    let _ = ljos_cli::rearm_after_compaction(call.session.as_deref());
                }
                let (ctx, ids) = ljos_cli::take_hook_note(call.session.as_deref());
                seen_later.extend(ids);
                ctx
            } else if !compacted && call.shape != ljos_cli::HookShape::CamelCase {
                // A fresh session hears that ljos is on, in one
                // line, so the person and the agent know the seat
                // is there before the first prompt. The line names
                // the issue this session already holds.
                ljos_cli::session_start_for(call.session.as_deref())
            } else {
                String::new()
            };
            // After the preface is read. The model load is its own
            // process and does not hold this line, and it does not
            // occupy the pack until the atoms for the line are in.
            let _ = ljos_cli::encoder_warming();
            ctx
        }
        // A turn ending is not a session ending, and has nothing
        // to say either.
        "TurnEnd" => String::new(),
        _ => {
            // Only the person's own prompt is kept as theirs: a
            // subagent's brief or an unattended run's prompt is an
            // agent's words in that channel. A tool call can run `ljos hook`
            // with a payload of its own; only a hook the runner
            // itself spawned carries the person's prompt. What the
            // hook saves from any prompt is labelled with whose it is.
            if call.event == "UserPromptSubmit" {
                ljos_cli::capture::forget_latest_prompt();
            }
            let spawner = if call.event == "UserPromptSubmit" {
                if let Some(why) = ljos_cli::capture::not_the_person(&input) {
                    origin_note = Some(ljos_cli::proc_tree::prompt_not_person_label(why));
                    None
                } else {
                    match ljos_cli::proc_tree::hook_runner() {
                        Ok((runner, _)) => Some(runner),
                        Err(why) => {
                            origin_note = Some(ljos_cli::proc_tree::prompt_origin_label(&why));
                            None
                        }
                    }
                }
            } else {
                None
            };
            let person = spawner.is_some();
            ljos_cli::capture::set_persons_prompt(person);
            if person {
                let mut sessions: Vec<String> = call.session.iter().cloned().collect();
                for s in ljos_cli::payload_sessions(&input) {
                    if !sessions.contains(&s) {
                        sessions.push(s);
                    }
                }
                ljos_cli::capture::note_latest_prompt_from(
                    &call.cue,
                    &sessions,
                    spawner.as_ref(),
                    ljos_cli::capture::payload_transcript(&input).as_deref(),
                );
            }
            // A standing choice the prompt states is saved in its
            // words before the note is read, so the note does not
            // repeat it and the correction hook does not store it
            // again.
            let captured = (call.event == "UserPromptSubmit")
                .then(|| {
                    ljos_cli::hook_timed("capture", || ljos_cli::capture::capture_stated(&call))
                })
                .flatten();
            if call.event == "UserPromptSubmit" {
                ljos_cli::hook_timed("correction", || ljos_cli::store_correction(&call));
            }
            let (mut ctx, mut ids) = ljos_cli::hook_timed("hook_note", || hook_note(&call, limit));
            if let Some((line, keys)) = captured {
                ctx = if ctx.is_empty() {
                    line
                } else {
                    format!("{line}\n{ctx}")
                };
                ids.extend(keys);
            }
            if call.shape == ljos_cli::HookShape::CamelCase && call.event == "UserPromptSubmit" {
                let line = ljos_cli::PACK_LINE;
                ctx = if ctx.is_empty() {
                    line.to_string()
                } else {
                    format!("{line}\n{ctx}")
                };
            }
            if call.event == "UserPromptSubmit" && ljos_cli::asks_decision(&call.cue) {
                let line = ljos_cli::start_decision_panel(
                    &call.cue,
                    call.session.as_deref(),
                    cwd.as_ref().and_then(|p| p.to_str()),
                )
                .unwrap_or_else(|e| {
                    format!("{}\npanel did not open: {e:#}", ljos_cli::decision_hold())
                });
                ctx = if ctx.is_empty() {
                    line
                } else {
                    format!("{line}\n{ctx}")
                };
            }
            if call.event == "UserPromptSubmit" {
                let (mail, mail_ids) = ljos_cli::hook_timed("mail", ljos_cli::mail::prompt_note);
                if !mail.is_empty() {
                    ctx = if ctx.is_empty() {
                        mail
                    } else {
                        format!("{mail}\n{ctx}")
                    };
                    ids.extend(mail_ids);
                }
            }
            if !call.shape.holds_prompt_note() {
                seen_later.extend(ids.clone());
            }
            if call.event == "UserPromptSubmit" {
                if let Some(line) = ljos_cli::prompt_writer_line() {
                    ctx = line;
                }
            }
            prompt_hook_stdout(call.shape, call.session.as_deref(), &ctx, &ids)
        }
    };
    // A receipt line or a tab separated dump from a search merge stays
    // out of the note. The formatted sentences stay.
    context = ljos_cli::without_raw_rows(&context);
    // The book is kept here, after the answer is decided, so a
    // tracker that does not answer cannot hold the call. A call a
    // rule warned on runs, so it is booked too; a blocked call does
    // not run and is not booked.
    // A Stop the audit held never reaches this line. An idle agy stop
    // skipped the audit and is booked; a mid-loop agy stop, fullyIdle
    // absent, is not. Booking queues no job for any event, so neither
    // files or finishes a claim. A gate's book is queued once its answer
    // is out.
    let agy_mid_loop = call.shape == ljos_cli::HookShape::Steps
        && call.event == "Stop"
        && !ljos_cli::stop_is_idle(&input);
    let book = !agy_mid_loop && !verdict.is_some_and(ljos_cli::Rule::blocks);
    let gate = ljos_cli::is_gate_event(&call.event);
    if book && !gate {
        let extra = ljos_cli::hook_timed("bookkeep", || {
            ljos_cli::book_queue::book_later(&input, &call)
        });
        if !extra.is_empty() {
            context = if context.is_empty() {
                extra
            } else {
                format!("{context}\n{extra}")
            };
        }
    }
    let output = ljos_cli::rewrite::answer(&input, &call, &context, verdict, |line| {
        ljos_cli::tcb_verdict_in(line, dir).or_else(|| ljos_cli::ruled_verdict(&rules, line))
    })
    .unwrap_or_else(|| ljos_cli::hook_output_ruled(&call, &context, verdict));
    ljos_cli::answer_gate(&output);
    if let Some(note) = origin_note {
        eprintln!("{}", ljos_cli::scan::wrap(&note, ljos_cli::scan::COLS));
    }
    if let Some(rule) = verdict {
        let seat = std::env::var("LJOS_SEAT").unwrap_or_default();
        let kind = if rule.blocks() { "refusal" } else { "warning" };
        ljos_cli::digest::note_guard(kind, &seat, &rule.reason);
    }
    if book && gate {
        ljos_cli::hook_timed("bookkeep", || {
            ljos_cli::book_queue::enqueue(&input, &call);
        });
    }
    ljos_cli::keep_hook_answer(&call, &input, &output);
    if !seen_later.is_empty() {
        mark_seen(call.session.as_deref(), &seen_later);
        ljos_cli::mail::ack_delivered(&seen_later);
    }
    if ack_nudge {
        ljos_cli::work_nudge_delivered(call.session.as_deref());
    }
    if ljos_cli::shell_exit(fail_on_deny, &output) != 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// `ljos hook` parsed without the rest of the command tree.
/// `None` when this is not a hook call, or when help, version, or a
/// rejected flag should be printed by the full parser.
fn hook_parsed_alone(args: &[std::ffi::OsString]) -> Option<HookArgs> {
    let mut saw_hook = false;
    for arg in args.iter().skip(1) {
        if arg == "--help" || arg == "-h" || arg == "--version" || arg == "-V" {
            return None;
        }
        if !saw_hook && !arg.to_string_lossy().starts_with('-') {
            if arg != "hook" {
                return None;
            }
            saw_hook = true;
        }
    }
    if !saw_hook {
        return None;
    }
    // `ljos hook` with no flags is the call a runner makes. The defaults
    // are the command's defaults, and building the flag parser faults
    // code this call does not read.
    if args.len() == 2 {
        return Some(HookArgs {
            limit: 5,
            event: None,
            prompt: false,
            text: false,
            fail_on_deny: false,
            runner: None,
            git_commit: false,
            git_hook: None,
            brief_once: false,
            drain: false,
            no_wait: false,
            apply_book: Vec::new(),
            finish_session: None,
            judge_worker: false,
        });
    }
    match HookFast::try_parse_from(args) {
        Ok(HookFast {
            cmd: HookFastCmd::Hook(hook),
        }) => Some(hook),
        Err(_) => None,
    }
}

/// The seat setup `run_command` does before it reaches a hook, then the hook.
fn dispatch_hook(seat_flag: Option<String>, hook: HookArgs) -> Result<()> {
    if hook.judge_worker {
        return ljos_cli::run_jev_judge_worker();
    }
    ljos_cli::seat_resolve::prepare(seat_flag, ljos_cli::seat_resolve::Kind::Keep);
    ljos_cli::runner_skills::refresh_on_new_version();
    ljos_cli::runner_env::refresh_on_new_version();
    let checked = ljos_cli::hook_timed("write_seat", || {
        None::<&str>.map(ljos_cli::write_seat).transpose()
    })?;
    let _seated = checked.as_ref().map(ljos_cli::Seated::enter);
    run_hook(hook)
}

fn run_command() -> Result<()> {
    // The tool server answers before the seat, the tracker or the pack is
    // read. Those can exit, block or write, and `initialize` needs none of
    // them. `--help` stays here so the command page is this binary's.
    if let Some(args) = mcp_serve_args() {
        let err = ljos_cli::exec_server(&args);
        eprintln!("ljos: {err:#}");
        std::process::exit(1);
    }
    // `--help` exits before the seat is read. `ljos help` is a command.
    if root_help_only() {
        print!("{}", help_groups::render(&Cli::command(), false));
        return Ok(());
    }
    if std::env::args_os().len() == 1 {
        // The same seat a read names, including where it came from.
        ljos_cli::load_seat_env();
        ljos_cli::seat_resolve::prepare(None, ljos_cli::seat_resolve::Kind::Read);
        print!("{}", opening_screen());
        return Ok(());
    }
    let raw: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let (seat_flag, args) = take_seat_flag(&raw);
    ljos_cli::load_seat_env();
    ljos_cli::normalize_tracker_env();
    // A closed pipe ends the run quietly: `ljos learn | head` is not a panic.
    // SAFETY: resetting a signal disposition before any thread is spawned.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    // The full command tree builds every other command into one function.
    // That function's frame is 2592 KB, and walking it faults code and
    // help text a hook never reads. Help and a flag this parser rejects
    // fall through, so those messages stay as they are.
    if let Some(hook) = hook_parsed_alone(&args) {
        return dispatch_hook(seat_flag, hook);
    }
    // Help, version and a missing argument exit here, before any write.
    // A memory subcommand that clap cannot parse still records the error.
    let cmd = match Cli::try_parse_from(args) {
        Ok(cli) => cli.cmd,
        Err(err) => {
            if !matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let args: Vec<String> = std::env::args().skip(1).collect();
                ljos_cli::memory_port::record_memory_error(&args, &err.to_string());
            }
            if let Some(line) = usage_sentence(&err) {
                eprintln!("{line}");
                std::process::exit(USAGE_STATUS);
            }
            err.exit();
        }
    };
    // A blank id is a failed `ljos file` capture. Refuse before the seat
    // check, the note queue and the tracker.
    if let Some(line) = blank_ticket_line(&cmd) {
        eprintln!("{line}");
        std::process::exit(1);
    }
    // A title is not a ticket id. Say how to file it as a plan before the
    // tracker is asked, so a product checkout is not initialised.
    if let Some(line) = sitting_plan_line(&cmd) {
        eprintln!("{line}");
        std::process::exit(USAGE_STATUS);
    }
    // A following id on `block` is `--on`. A mix with `--on` or `--off`,
    // or a word that is not an id, is a usage error. Refuse before the seat.
    if let Some(line) = block_usage_line(&cmd) {
        eprintln!("{line}");
        std::process::exit(USAGE_STATUS);
    }
    ljos_cli::seat_resolve::prepare(seat_flag, seat_kind(&cmd));
    // A lesson left while the pack was busy goes out on the next call
    // that writes. A read, a dry run, onboard, offboard, uninstall and
    // doctor leave the queue where it is and do not create its lock.
    // `ljos hook` keeps its own budget. Remember is still waiting on the
    // pack, so it does not send the queue it may be about to fill.
    if queued_lesson_may_go(&cmd) {
        ljos_cli::flush_queued_lessons();
    }
    let mut sitting_budget = match &cmd {
        Cmd::Sitting {
            issue,
            plan,
            verbose,
            json,
            ..
        } => {
            // The line is how a held lock shows life before the claim.
            // The short form waits for the claim itself. JSON is one
            // object at the end, so this line would break it.
            if *verbose && !*json && plan.is_none() {
                println!(
                    "sitting {}: checking blockers",
                    issue.as_deref().unwrap_or("plan")
                );
                std::io::Write::flush(&mut std::io::stdout())?;
            }
            Some(begin_sitting_preclaim())
        }
        _ => None,
    };
    // The first run of a new version refreshes skills and runner env
    // files ljos wrote. Doctor stays read-only unless `--fix` is set, and
    // a record check, which CI runs, always stays read-only.
    match &cmd {
        Cmd::Doctor { fix: true, .. } => {
            ljos_cli::runner_skills::refresh_for_fix();
            ljos_cli::runner_env::refresh_for_fix();
            ljos_cli::cloud::replace_stale_guard();
            ljos_cli::doctor_apply_fixes();
        }
        Cmd::Doctor { .. }
        | Cmd::Record { .. }
        | Cmd::Onboard { .. }
        | Cmd::Offboard { .. }
        | Cmd::Uninstall { .. }
        | Cmd::Help { .. }
        | Cmd::Completions { .. }
        | Cmd::Mcp { .. } => {}
        _ => {
            ljos_cli::runner_skills::refresh_on_new_version();
            ljos_cli::runner_env::refresh_on_new_version();
        }
    }
    // These act on the claims the hooks keep, and the hooks of the tool
    // calls before this one may still be queued.
    if matches!(
        cmd,
        Cmd::Sitting { .. }
            | Cmd::Claim { .. }
            | Cmd::Slot { .. }
            | Cmd::Finish { .. }
            | Cmd::Release { .. }
            | Cmd::Complete { .. }
    ) {
        ljos_cli::book_queue::catch_up();
    }
    let checked = ljos_cli::hook_timed("write_seat", || {
        tracker_write(&cmd).map(ljos_cli::write_seat).transpose()
    })?;
    let _seated = checked.as_ref().map(ljos_cli::Seated::enter);
    let assignee_for = |verb: &str, passed: Option<&str>, delegates: bool| -> Result<String> {
        let name = cli_assignee(passed);
        match &checked {
            Some(seat) => ljos_cli::write_assignee(verb, seat, name, delegates),
            None => Ok(name),
        }
    };
    match cmd {
        Cmd::Help { all, command } => show_help(all, &command)?,
        Cmd::Completions { shell, candidates } => {
            let mut cmd = Cli::command();
            shell_complete::emit(shell, candidates, &mut cmd)?;
        }
        Cmd::Remember {
            text,
            as_persona,
            transient,
            standing,
            verbose,
            valid_from,
            valid_to,
            json,
            project,
        } => {
            ljos_cli::config_diag::emit();
            if transient && standing {
                eprintln!("remember: pass --transient or --standing, not both.");
                std::process::exit(USAGE_STATUS);
            }
            let text = join(&text);
            if text.trim().is_empty() {
                eprintln!("remember: the text is empty, so run `ljos remember \"TEXT\"`.");
                std::process::exit(USAGE_STATUS);
            }
            let chars = text.chars().count();
            if chars > ljos_cli::REMEMBER_TEXT_CAP {
                eprintln!(
                    "remember: the text is {chars} characters, past {}, and it needs one or two sentences, so pass a shorter text to `ljos remember`.",
                    ljos_cli::REMEMBER_TEXT_CAP
                );
                std::process::exit(USAGE_STATUS);
            }
            let project = match project {
                Some(name) => Some(ljos_cli::require_known_project(&name)?),
                None => None,
            };
            let _remember_project = ljos_cli::RememberProject::set(project);
            if let Err(err) = ljos_cli::set_validity(valid_from.as_deref(), valid_to.as_deref()) {
                eprintln!("{err}");
                std::process::exit(USAGE_STATUS);
            }
            if let Some(line) = ljos_cli::sandbox_memory_off() {
                eprintln!("{line}");
                std::process::exit(1);
            }
            ljos_cli::seat_resolve::mark_write();
            // A paragraph or a session is several claims: each goes in on
            // its own, where the whole was refused at the pack's cap.
            // Two sentences stay one row. A longer text is split, and
            // stdout names every stored id and its text.
            let chunks = ljos_cli::claim_chunks(&text);
            let split_n = chunks.len();
            let split = split_n > 1;
            if split {
                eprintln!("{}", ljos_cli::remember_split_line(chunks.len()));
            }
            let mut stored: Vec<(String, String)> = Vec::new();
            for text in chunks {
                // A person at the terminal, and words from their latest
                // prompt, are their lesson. Any other text is a standing
                // lesson too, sitting or not, labelled agent-derived.
                // A busy pack queues the lesson and this command exits 0.
                let body = ljos_cli::remember_with_retry(&text, || {
                    if ljos_cli::person_present() {
                        let horizon = ljos_cli::remember_horizon(transient, standing, false);
                        packset_write_as("Remember", &text, as_persona.as_deref(), horizon)
                    } else if ljos_cli::capture::said_in_latest_prompt(&text) {
                        let horizon = transient.then_some(true);
                        ljos_cli::packset_write_mcp(
                            "Remember",
                            &text,
                            as_persona.as_deref(),
                            horizon,
                        )
                    } else {
                        let horizon = if transient { Some(true) } else { Some(false) };
                        ljos_cli::packset_write_agent(
                            "Remember",
                            &text,
                            as_persona.as_deref(),
                            horizon,
                        )
                    }
                })?;
                let duplicate = body["duplicate"].as_bool() == Some(true);
                let queued = body["queued"].as_bool() == Some(true);
                if split && (duplicate || !queued) {
                    stored.push(stored_claim(&body, &text));
                }
                if duplicate {
                    continue;
                }
                if body["queued"].as_bool() == Some(true) {
                    println!("{}", ljos_cli::REMEMBER_QUEUED_LINE);
                    continue;
                }
                if json {
                    if let Some(note) = ljos_cli::prose_warning_shown(&body, verbose) {
                        eprintln!("{note}");
                    }
                    let mut printed = body.clone();
                    ljos_cli::readability::attach(&mut printed);
                    println!("{}", serde_json::to_string_pretty(&printed)?);
                } else if !split {
                    println!("{}", write_ack(&body, verbose));
                }
                eprintln!("{}", ljos_cli::remember_note(&body));
                if !ljos_cli::person_present() && !ljos_cli::person_stored(&body) {
                    eprintln!("{}", ljos_cli::STORED_ORIGIN_LINE);
                }
                if let Some(line) = ljos_cli::memories::reinforced_line(&body) {
                    eprintln!("{line}");
                } else if let Some(ids) =
                    body["supersedes"].as_array().filter(|ids| !ids.is_empty())
                {
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
            if split && !json && !stored.is_empty() {
                print!("{}", split_claims_page(split_n, &stored));
            }
        }
        Cmd::Prefer {
            text,
            as_persona,
            deny,
            ask,
            user,
            why,
            promote,
            deny_mention,
            deny_name,
            deny_reply_to,
            deny_reply_name,
            forget,
            sync,
            review,
            keep,
        } => {
            // A person-side prefer records the user-rule cutoff. Under a
            // runner it does not, so a hook cannot decide which old rows
            // are new.
            ljos_cli::user_rules::note_cutoff();
            if review && user {
                if sync
                    || promote.is_some()
                    || forget.is_some()
                    || why.is_some()
                    || !text.is_empty()
                    || !deny.is_empty()
                    || !ask.is_empty()
                    || as_persona.is_some()
                    || !deny_mention.is_empty()
                    || !deny_name.is_empty()
                    || !deny_reply_to.is_empty()
                    || !deny_reply_name.is_empty()
                {
                    bail!(
                        "prefer: --user --review lists older user rules and takes none of the other flags"
                    );
                }
                if let Some(id) = keep {
                    print!("{}", ljos_cli::user_rules::keep_held(&id)?);
                } else {
                    print!("{}", ljos_cli::user_rules::review_list()?);
                }
                return Ok(());
            }
            if review {
                if user
                    || sync
                    || promote.is_some()
                    || forget.is_some()
                    || why.is_some()
                    || !text.is_empty()
                    || !deny.is_empty()
                    || !ask.is_empty()
                    || as_persona.is_some()
                    || !deny_mention.is_empty()
                    || !deny_name.is_empty()
                    || !deny_reply_to.is_empty()
                    || !deny_reply_name.is_empty()
                {
                    bail!(
                        "prefer: --review lists older preferences and takes none of the other flags"
                    );
                }
                if let Some(id) = keep {
                    print!("{}", ljos_cli::user_rules::keep_held(&id)?);
                    return Ok(());
                }
                println!("no legacy preferences to review");
                return Ok(());
            }
            if user {
                if sync {
                    if !text.is_empty()
                        || promote.is_some()
                        || forget.is_some()
                        || why.is_some()
                        || !deny.is_empty()
                        || !ask.is_empty()
                        || as_persona.is_some()
                        || !deny_mention.is_empty()
                        || !deny_name.is_empty()
                        || !deny_reply_to.is_empty()
                        || !deny_reply_name.is_empty()
                    {
                        bail!("prefer: --sync pulls and pushes user rules, and takes none of the other flags");
                    }
                    print!("{}", ljos_cli::user_rules::sync_now()?);
                    return Ok(());
                }
                if !deny.is_empty() || !ask.is_empty() {
                    bail!(
                        "prefer: a user rule takes no --deny or --ask. \
                         `ljos rule --user PATTERN --verdict deny --why REASON` writes the command pattern"
                    );
                }
                if as_persona.is_some() {
                    bail!("prefer: a user rule is this user's, not one persona's. Drop --as");
                }
                if let Some(id) = forget {
                    if !text.is_empty() || promote.is_some() {
                        bail!("prefer: --forget takes the id, not the words or --promote");
                    }
                    let rule = ljos_cli::user_rules::forget(&id)?;
                    println!("{}", ljos_cli::user_rules::ack(&rule));
                    return Ok(());
                }
                let why = why.as_deref().unwrap_or("");
                let mention = ljos_cli::user_rules::People {
                    handles: deny_mention,
                    names: deny_name,
                };
                let reply = ljos_cli::user_rules::People {
                    handles: deny_reply_to,
                    names: deny_reply_name,
                };
                let rule = if let Some(id) = promote {
                    if !text.is_empty() {
                        bail!("prefer: pass the lesson id with --promote, or the words, not both");
                    }
                    ljos_cli::user_rules::promote_lesson(&id, why, &mention, &reply)?
                } else {
                    let words = join(&text);
                    if words.trim().is_empty() {
                        eprintln!(
                            "prefer: the text is empty, so run `ljos prefer --user --why REASON \"TEXT\"`."
                        );
                        std::process::exit(USAGE_STATUS);
                    }
                    ljos_cli::user_rules::write_preference(&words, why, &mention, &reply)?
                };
                println!("{}", ljos_cli::user_rules::ack(&rule));
                return Ok(());
            }
            if why.is_some()
                || promote.is_some()
                || forget.is_some()
                || sync
                || !deny_mention.is_empty()
                || !deny_name.is_empty()
                || !deny_reply_to.is_empty()
                || !deny_reply_name.is_empty()
            {
                bail!(
                    "prefer: --why, --promote, --forget, --sync and the mention flags write a user rule. Pass --user"
                );
            }
            let text = join(&text);
            if text.trim().is_empty() {
                eprintln!("prefer: the text is empty, so run `ljos prefer \"TEXT\"`.");
                std::process::exit(USAGE_STATUS);
            }
            let rules = ljos_cli::preference_rules(&text, &deny, &ask)?;
            // A preference that forbids a command it can name carries an
            // ask over it. The person typing it gets the rule with the
            // line that takes it back; an agent is shown the command.
            let proposed = if deny.is_empty() && ask.is_empty() {
                ljos_cli::command_prefs::rule_candidates(&text)
            } else {
                Vec::new()
            };
            let basis = ljos_cli::Basis::of_rule();
            // The same unknown-seat line `ljos remember` prints. A review
            // or a user-rule flag above has already returned.
            ljos_cli::seat_resolve::mark_write();
            ljos_cli::seat_resolve::announce_named_unknown(&ljos_cli::whoami());
            let body = ljos_cli::packset_prefer(&text, as_persona.as_deref())?;
            println!("{}", write_ack(&body, false));
            if let Some(line) = ljos_cli::memories::reinforced_line(&body) {
                eprintln!("{line}");
            }
            for rule in &rules {
                println!("{}", write_ack(&write_rule(rule, basis)?, false));
            }
            if !proposed.is_empty() && !ljos_cli::person_present() {
                for rule in &proposed {
                    println!(
                        "this preference can carry an ask over {}: `ljos rule {} --verdict ask --why {}`",
                        rule.pattern,
                        ljos_cli::tools::sh_quote(&rule.pattern),
                        ljos_cli::tools::sh_quote(&rule.reason)
                    );
                }
            } else {
                for rule in &proposed {
                    let written = write_rule(rule, basis)?;
                    let id = written
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    println!(
                        "the hook now asks before {}, with this preference as the reason; `ljos forget {id}` takes the rule back",
                        rule.pattern
                    );
                }
            }
        }
        Cmd::Proposals {
            cmd,
            seat,
            status,
            settle,
        } => {
            if let Some(raw) = status.as_deref() {
                let trimmed = raw.trim();
                if !ljos_cli::admit::is_valid_status(trimmed) {
                    let bad = if trimmed.is_empty() {
                        "an empty value"
                    } else {
                        trimmed
                    };
                    eprintln!(
                        "proposals: --status takes {}, not {bad}.",
                        ljos_cli::admit::proposal_status_list()
                    );
                    std::process::exit(USAGE_STATUS);
                }
            }
            if let Some(ProposalCmd::Amend { id, text }) = cmd {
                let text = if text == "-" {
                    let mut buf = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
                    buf
                } else {
                    text
                };
                print!("{}", ljos_cli::admit::amend_proposal(&id, &text)?);
            } else if settle {
                print!("{}", ljos_cli::admit::settle_proposals(seat.as_deref())?);
            } else {
                print!(
                    "{}",
                    ljos_cli::admit::list_proposals(seat.as_deref(), status.as_deref())
                );
            }
        }
        Cmd::Accept {
            ids,
            quote,
            at,
            via,
        } => {
            let yes = ljos_cli::quoted_yes::read(quote.as_deref(), at.as_deref(), via.as_deref())?;
            print!("{}", ljos_cli::admit::accept_ids(&ids, yes.as_ref()));
        }
        Cmd::Export { out, machine, json } => export_memories(&out, machine.as_deref(), json)?,
        Cmd::Memory { cmd } => memory_cmd(cmd)?,
        Cmd::Memories { cmd } => memories_cmd(cmd)?,
        Cmd::RetireNotes => print!("{}", ljos_cli::note_retire::sweep()?),
        Cmd::Forget { id, why, json } => {
            let body = packset_forget(&id, why.as_deref())?;
            println!("{}", ljos_cli::atom_out(&body, json)?);
        }
        Cmd::MergeAtoms { current, path } => {
            ljos_cli::merge_atoms(&current, &path)?;
        }
        Cmd::MergeOrg { base, ours, theirs } => {
            ljos_cli::cloud::merge_org_cli(&base, &ours, &theirs)?;
        }
        Cmd::Cloud { cmd } => match cmd {
            CloudCmd::Up { force, repoint } => {
                print!("{}", ljos_cli::cloud::up_at(force, repoint)?)
            }
        },
        Cmd::Mesh { cmd } => match cmd {
            MeshCmd::Up {
                key_file,
                login_server,
                hostname,
            } => {
                let out = ljos_cli::mesh::up(ljos_cli::mesh::UpArgs {
                    key_file,
                    login_server,
                    hostname,
                });
                let failed = out.failed();
                out.write();
                if failed {
                    std::process::exit(1);
                }
            }
            MeshCmd::Down => {
                let out = ljos_cli::mesh::down();
                let failed = out.failed();
                out.write();
                if failed {
                    std::process::exit(1);
                }
            }
            MeshCmd::Status => {
                let (text, failed) = ljos_cli::mesh::status_text();
                println!("{text}");
                if failed {
                    std::process::exit(1);
                }
            }
            MeshCmd::Acl { swarms } => println!("{}", ljos_cli::mesh::acl(&swarms)?),
            MeshCmd::Pull { seat, from } => {
                let store = ljos_cli::mesh_lane::SeatStore::open(&seat)?;
                // Print the line here. A down writer names itself in the
                // message, and the shared fault path would replace that
                // line with a bare connection failure.
                match ljos_cli::mesh_lane::pull(&store, &from) {
                    Ok(summary) => print!("{}", summary.line()),
                    Err(err) => {
                        eprintln!("Error: {err}");
                        std::process::exit(1);
                    }
                }
            }
            MeshCmd::Push { seat, to, lane } => {
                let store = ljos_cli::mesh_lane::SeatStore::open(&seat)?;
                let lane = lane.unwrap_or_else(ljos_cli::memory_port::export_host);
                print!("{}", ljos_cli::mesh_lane::push(&store, &to, &lane)?);
            }
        },
        Cmd::Sync {
            key,
            import,
            export,
            adopt,
            force,
            dry_run,
            adopt_legacy,
            compact,
        } => {
            if let Some(host) = adopt {
                print!("{}", ljos_cli::sync::adopt(&host, force, dry_run)?);
            } else if let Some(host) = adopt_legacy {
                print!("{}", ljos_cli::sync::adopt_legacy(&host)?);
            } else if key {
                println!("{}", ljos_cli::sync::public_key()?);
            } else {
                let _compact = compact.then(ljos_cli::sync::compact_for_this_call);
                let both = !import && !export;
                print!(
                    "{}",
                    ljos_cli::sync::sync_repo(both || import, both || export)?
                );
            }
        }
        Cmd::Hubs { limit, json } => {
            let body = packset_hubs(limit)?;
            if json {
                print!("{}", ljos_cli::hubs_json(&body));
            } else {
                print!(
                    "{}",
                    ljos_cli::scan::for_reader(&format_hubs(&body), human_color())
                );
            }
        }
        Cmd::Conflicts { limit } => print!("{}", conflicts(limit)?),
        Cmd::Consolidate { apply } => {
            print!("{}", format_consolidation(&packset_consolidate(apply)?))
        }
        Cmd::Island {
            cue,
            fire,
            as_persona,
            json,
        } => {
            let body = packset_island_as(&join(&cue), fire, as_persona.as_deref())?;
            if json {
                print!("{}", ljos_cli::island_json(&body));
            } else {
                print!(
                    "{}",
                    ljos_cli::scan::for_reader(&format_island(&body), human_color())
                );
            }
        }
        Cmd::Search {
            query,
            limit,
            rerank,
            as_of,
            valid_at,
            explain,
            source_machine,
            source_seat,
            import_id,
            json,
            porcelain,
            attempts,
            min_score,
            project,
        } => {
            if !min_score.is_finite() || min_score < 0.0 {
                eprintln!("search: --min-score takes a number from 0 up, not {min_score}.");
                std::process::exit(USAGE_STATUS);
            }
            if join(&query).trim().is_empty()
                && source_machine.is_none()
                && source_seat.is_none()
                && import_id.is_none()
            {
                eprintln!("search: the query is empty, so run `ljos search \"QUERY\"`.");
                std::process::exit(USAGE_STATUS);
            }
            if let Some(line) = ljos_cli::sandbox_memory_off() {
                eprintln!("{line}");
                std::process::exit(1);
            }
            if let Some(at) = as_of.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                if !ljos_cli::valid_as_of(at) {
                    eprintln!(
                        "search: --as-of {at:?} is not a date; write YYYY-MM-DD or RFC 3339."
                    );
                    std::process::exit(USAGE_STATUS);
                }
            }
            let day = match valid_at.as_deref().map(str::trim) {
                Some("any") => None,
                Some(d) => {
                    if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
                        eprintln!("search: --valid-at takes YYYY-MM-DD or any, not {d}.");
                        std::process::exit(USAGE_STATUS);
                    }
                    Some(d.to_string())
                }
                None => Some(chrono::Local::now().format("%Y-%m-%d").to_string()),
            };
            let project = match project {
                Some(name) => Some(ljos_cli::require_known_project(&name)?),
                None => None,
            };
            ljos_cli::log_recall("search", &query.join(" "));
            // A cold encoder gets long enough to load and be asked again.
            // A warm pack keeps the short budget. Past it the word ranking
            // prints. Rows say they are word matches and name any reported
            // load time. An empty page names the command to run again.
            // Held across the status probe that sizes the budget, so a pack
            // that accepts and stays silent spends one request, not one per
            // later read.
            let _quiet_pack = ljos_cli::pack_link::QuietPack::enter();
            let _call = ljos_cli::SearchCall::enter();
            let dated_ahead = as_of.is_some();
            let budget = if !dated_ahead && !rerank {
                ljos_cli::search_budget()
            } else {
                ljos_cli::SEARCH_PACK_BUDGET
            };
            let _deadline = ljos_cli::pack_link::CommandDeadline::enter(budget);
            let _pack_clock = ljos_cli::pack_link::PackClock::enter(budget);
            // Expired claims are dropped after the pack answers, so ask for
            // more than the limit and cut to it once they are gone. A day
            // filter is what drops them. With `--valid-at any` the fetch
            // stays the printed limit.
            let fetch = if day.is_some() || project.is_some() {
                ljos_cli::valid_fetch(limit)
            } else {
                limit
            };
            // A dated search is the pack as it stood then. The live
            // snapshot is not that pack, so a busy writer stays a failure
            // there. A search of now ranks the snapshot by words instead.
            let dated = as_of.is_some();
            let asked = join(&query);
            // A search of now asks for relevance, so a plain-language task
            // surfaces the lesson that names its concept. A dated search
            // stays the pack as it stood, and `--rerank` keeps the
            // cross-encoder.
            let fetched = if dated {
                packset_search_as_of(&asked, fetch, as_of.as_deref(), rerank)
                    .map(|hits| (hits, None))
            } else if !rerank {
                ljos_cli::search_hits(&asked, fetch).map(|(hits, ranking)| (hits, Some(ranking)))
            } else {
                packset_search_as_of(&asked, fetch, None, true).map(|hits| (hits, None))
            };
            let (mut hits, mut ranking) = match fetched {
                Ok((hits, Some(ranking))) => (hits, ranking),
                Ok((hits, None)) => (
                    hits,
                    ljos_cli::ranking_now().unwrap_or(ljos_cli::Ranking {
                        mode: "semantic",
                        reason: "",
                    }),
                ),
                // A 401 names `packset` in the command that replaces the
                // writer. That word is also how a down pack is recognised,
                // and this refusal is not a down pack: the writer answered.
                Err(err) if err.to_string().contains(ljos_cli::WRITER_PORT_FIX) => {
                    return Err(err);
                }
                // A dated search has no live snapshot to rank, so a pack
                // that has not opened exits with the starting line.
                Err(err) if dated && ljos_cli::is_memory_starting(&err) => return Err(err),
                // The pack has not opened, is busy, or is not running. A
                // search of now ranks the words it has on this machine, and
                // one line on stderr says which of the three it was.
                Err(err) if !dated && ljos_cli::degraded::of_error(&err).is_some() => {
                    let state = ljos_cli::degraded::of_error(&err)
                        .unwrap_or(ljos_cli::degraded::State::Busy);
                    (
                        ljos_cli::search_by_words(&asked, fetch),
                        ljos_cli::Ranking {
                            mode: "lexical",
                            reason: state.reason(),
                        },
                    )
                }
                Err(err) if !dated && ljos_cli::pack_turned_off(&err) => (
                    ljos_cli::search_by_words(&asked, fetch),
                    ljos_cli::Ranking {
                        mode: "lexical",
                        reason: ljos_cli::PACK_OFF_REASON,
                    },
                ),
                Err(err) => return Err(err),
            };
            if let Some(day) = day {
                hits.retain(|h| ljos_cli::valid_on(&h.entities, &day));
            }
            // Rank before the cut so another seat's higher score does not
            // push this seat's rows out of the printed limit. The slot
            // merge keeps those pack rows in front and appends its own.
            let sitting = ljos_cli::recorded_sitting();
            ljos_cli::prefer_search_context(&mut hits, &asked, sitting.as_deref());
            let filter = ljos_cli::memory_port::SourceFilter {
                machine: source_machine,
                seat: source_seat,
                import_id,
            };
            let mut rerun_flags: Vec<String> = Vec::new();
            if let Some(p) = project.as_deref() {
                rerun_flags.push("-p".to_string());
                rerun_flags.push(p.to_string());
            }
            if let Some(m) = filter.machine.as_deref() {
                rerun_flags.push("--source-machine".to_string());
                rerun_flags.push(m.to_string());
            }
            if let Some(s) = filter.seat.as_deref() {
                rerun_flags.push("--source-seat".to_string());
                rerun_flags.push(s.to_string());
            }
            if let Some(id) = filter.import_id.as_deref() {
                rerun_flags.push("--import-id".to_string());
                rerun_flags.push(id.to_string());
            }
            if attempts {
                rerun_flags.push("--attempts".to_string());
            }
            if rerank {
                rerun_flags.push("--rerank".to_string());
            }
            if let Some(t) = as_of.as_deref() {
                rerun_flags.push("--as-of".to_string());
                rerun_flags.push(t.to_string());
            }
            if let Some(d) = valid_at.as_deref() {
                rerun_flags.push("--valid-at".to_string());
                rerun_flags.push(d.to_string());
            }
            if (min_score - 34.0).abs() > 1e-9 {
                rerun_flags.push("--min-score".to_string());
                rerun_flags.push(min_score.to_string());
            }
            if !dated || filter.active() {
                ljos_cli::memory_port::merge_search(
                    &mut hits,
                    &asked,
                    limit as usize,
                    &filter,
                    attempts,
                );
            }
            // A word ranking re-scores the rows it was given. The fused
            // path keeps the pack's order, and this seat's rows come
            // before another seat's.
            let (mut close_miss, unrelated) = if ranking.mode == "lexical" {
                let missed = match ljos_cli::word_rank::order_hits(&mut hits, &asked, attempts) {
                    ljos_cli::word_rank::WordList::NoClose { suggest } => Some(suggest),
                    ljos_cli::word_rank::WordList::Kept | ljos_cli::word_rank::WordList::None => {
                        None
                    }
                };
                (missed, false)
            } else {
                ljos_cli::refine_default_hits(&mut hits, &asked, attempts);
                ljos_cli::drop_word_floor(&mut hits, &asked, false);
                let unrelated = ljos_cli::drop_weak_cross_seat_for(
                    &mut hits,
                    &asked,
                    false,
                    sitting.as_deref(),
                );
                (None, unrelated)
            };
            ljos_cli::prefer_search_context(&mut hits, &asked, sitting.as_deref());
            // A dated search is the pack as it stood. A source filter asked
            // for slot rows only. Tickets are the live tracker, so neither
            // of those pages grows one. The merge runs after both rankers,
            // and after a short load waited out inside the call deadline,
            // so a title match is present while the encoder loads and after.
            // `capped` is the pack's page, counted before tickets are added.
            // A full pack page can still have more memories past `fetch`.
            let capped = fetch > 0 && hits.len() == usize::try_from(fetch).unwrap_or(usize::MAX);
            if !dated && !filter.active() {
                ljos_cli::ticket_search::merge(&mut hits, &asked);
            }
            let receipt_rows = ljos_cli::search_receipt_rows(&hits);
            // After the pack, the slot merge and the ticket merge. A
            // receipt, a mail row and the other bookkeeping kinds are not
            // part of the page, in any of those paths.
            ljos_cli::conceal_search_noise(&mut hits, attempts);
            if let Some(project) = project.as_deref() {
                ljos_cli::keep_project(&mut hits, project);
            }
            let floor_emptied =
                ljos_cli::word_rank::apply_relevance_floor(&mut hits, &asked, min_score);
            if !hits.is_empty() {
                close_miss = None;
            }
            let matched = hits.len();
            hits.truncate(limit as usize);
            ljos_cli::note_lookups(&hits);
            let id_space = ljos_cli::search_id_space(&hits);
            ranking = ljos_cli::ranking_for_printed(ranking, &hits);
            // The word fallback already answered. One line names that.
            // An empty page does not also print the ordinary miss, and it
            // does not say to try again as if no rows were coming.
            let busy_words = ranking.reason == ljos_cli::MEMORY_SEARCH_BUSY_REASON;
            let busy_miss = busy_words && hits.is_empty();
            let close_line = if busy_miss {
                None
            } else {
                close_miss.as_ref().map(|suggest| {
                    ljos_cli::word_rank::no_close_line_with(&asked, suggest.as_deref())
                })
            };
            // A shorter search would miss too when the store is empty, so
            // that page names remember. A store that already holds a lesson
            // names a shorter query instead.
            let empty_store = !busy_miss
                && hits.is_empty()
                && close_line.is_none()
                && !unrelated
                && ljos_cli::nothing_stored();
            if porcelain {
                print!("{}", ljos_cli::search_porcelain(&hits, &id_space));
            } else if json {
                let relevant = (!busy_miss && floor_emptied)
                    .then(|| ljos_cli::word_rank::no_relevant_line(&asked));
                let empty = if busy_miss {
                    None
                } else {
                    relevant
                        .as_deref()
                        .or(close_line.as_deref())
                        .or(unrelated.then_some(ljos_cli::NOTHING_RELEVANT))
                };
                let suggestion = if busy_miss {
                    None
                } else if empty_store {
                    Some(ljos_cli::scan::store_lesson_command(&asked))
                } else if floor_emptied {
                    Some(ljos_cli::word_rank::min_score_off_command(&asked))
                } else if hits.is_empty() {
                    close_miss
                        .clone()
                        .unwrap_or_else(|| ljos_cli::word_rank::shorter_search(&asked))
                } else {
                    None
                };
                print!(
                    "{}",
                    ljos_cli::search_json_receipts_stored(
                        &hits,
                        Some(&ranking),
                        empty,
                        suggestion.as_deref(),
                        &id_space,
                        &receipt_rows,
                    )
                );
            } else if busy_miss {
                ljos_cli::say_search_rank(&ranking, &hits);
            } else {
                ljos_cli::say_search_rank(&ranking, &hits);
                if floor_emptied {
                    print!(
                        "{}",
                        ljos_cli::scan::for_reader(
                            &ljos_cli::scan::no_relevant_page(&asked),
                            human_color()
                        )
                    );
                } else if let Some(line) = close_line {
                    let mut page = ljos_cli::scan::wrap(&line, ljos_cli::scan::COLS);
                    if !page.ends_with('\n') {
                        page.push('\n');
                    }
                    print!("{}", ljos_cli::scan::for_reader(&page, human_color()));
                } else if empty_store {
                    print!(
                        "{}",
                        ljos_cli::scan::for_reader(
                            &ljos_cli::scan::empty_store_page(&asked),
                            human_color()
                        )
                    );
                } else if hits.is_empty() && unrelated {
                    print!("{}", ljos_cli::NOTHING_RELEVANT);
                } else {
                    print!(
                        "{}",
                        ljos_cli::without_raw_rows(&ljos_cli::scan::for_reader(
                            &ljos_cli::format_search_stored_with_flags(
                                &hits,
                                matched,
                                capped,
                                explain,
                                &asked,
                                ranking.mode != "lexical",
                                &id_space,
                                &rerun_flags,
                            ),
                            human_color()
                        ))
                    );
                }
            }
            if busy_miss {
                std::process::exit(1);
            }
        }
        Cmd::Evidence { args } => {
            if args.len() == 1 {
                let said = run_captured("deedar", &["evidence", &args[0]])?;
                print!("{}", ljos_cli::evidence_here(&said.stdout));
            } else {
                bail!("evidence: pass one deed accession");
            }
        }
        Cmd::Record { cmd } => match cmd {
            RecordCmd::Check {
                tracker: Some(root),
                ..
            } => {
                let report = ljos_cli::record::check_tracker(&root)?;
                print!("{}", ljos_cli::record::format_tracker_report(&report));
                if !report.bad.is_empty() {
                    std::process::exit(1);
                }
            }
            RecordCmd::Check { dir, tracker: None } => {
                let dir = dir.context("record check: name a directory, or --tracker ROOT")?;
                let report = ljos_cli::record::check_dir(&dir)?;
                print!("{}", ljos_cli::record::format_check_report(&report));
            }
        },
        Cmd::Current { accession } => run("deedar", &["current", &accession])?,
        Cmd::Deed {
            issue,
            show,
            out,
            add,
            remove,
            ticket,
            porcelain,
        } => {
            if let Some(accession) = show {
                let read = ljos_cli::deed_bytes::read_deed(&accession)?;
                match out {
                    Some(path) => {
                        std::fs::write(&path, &read.bytes)
                            .with_context(|| format!("deed: could not write {}", path.display()))?;
                        println!(
                            "wrote {} bytes of {} to {} (sha256:{})",
                            read.bytes.len(),
                            read.accession,
                            path.display(),
                            read.sha256
                        );
                    }
                    None => std::io::Write::write_all(&mut std::io::stdout(), &read.bytes)?,
                }
            } else if !remove.is_empty() {
                let ticket = ticket
                    .as_deref()
                    .or(issue.as_deref())
                    .context("deed: name the issue to drop it from")?;
                stop_ticket(ticket);
                for a in &remove {
                    let said = uncite_deed(ticket, a)?;
                    if porcelain {
                        println!("{a}");
                        eprint!("{said}");
                    } else {
                        print!("{said}");
                    }
                }
                let persisted = ljos_cli::persist_tracker(ticket, "removed a deed citation");
                if porcelain {
                    eprint!("{persisted}");
                } else {
                    print!("{persisted}");
                }
            } else if add.is_empty() {
                let issue = issue.as_deref().context("deed: an issue is required")?;
                stop_ticket(issue);
                if porcelain {
                    let said = run_captured("vissue", &["deed", issue])?;
                    for line in said.stdout.lines() {
                        if let Some(id) = line.strip_prefix("  ") {
                            let id = id.trim();
                            if !id.is_empty() {
                                println!("{id}");
                            }
                        }
                    }
                    eprint!("{}", said.stdout);
                    eprint!("{}", said.stderr);
                } else {
                    run("vissue", &["deed", issue])?;
                }
            } else {
                let issue = issue.as_deref().context("deed: an issue is required")?;
                stop_ticket(issue);
                let mut cited = Vec::with_capacity(add.len());
                for a in &add {
                    cited.push(ljos_cli::resolve_deed(a)?);
                }
                let mut argv = vec!["deed".to_string(), issue.to_string()];
                for id in &cited {
                    argv.push("--add".into());
                    argv.push(id.clone());
                }
                let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
                if porcelain {
                    let said = run_captured("vissue", &refs)?;
                    for id in &cited {
                        println!("{id}");
                    }
                    eprint!("{}", said.stdout);
                    eprint!("{}", said.stderr);
                    eprint!("{}", ljos_cli::persist_tracker(issue, "cited a deed"));
                } else {
                    run("vissue", &refs)?;
                    print!("{}", ljos_cli::persist_tracker(issue, "cited a deed"));
                }
            }
        }
        Cmd::Table {
            issue,
            add,
            name,
            show,
            version,
            csv: _,
            json,
            verbose,
        } => {
            stop_ticket(&issue);
            if let Some(file) = add {
                let added = ljos_cli::table::add(&issue, &file, name.as_deref())?;
                let mut said = added.text;
                if added.wrote {
                    said.push_str(&ljos_cli::persist_tracker(&issue, &added.verb));
                }
                print!("{}", render_write(&said, git_detail(verbose), false, None));
            } else if let Some(name) = show {
                let (version, read) = ljos_cli::table::show(&issue, &name, version)?;
                if json {
                    print!(
                        "{}",
                        ljos_cli::table::as_json(&issue, &version, &read.bytes)?
                    );
                } else {
                    std::io::Write::write_all(&mut std::io::stdout(), &read.bytes)?;
                }
            } else {
                print!("{}", ljos_cli::table::list(&issue, json)?);
            }
        }
        Cmd::OpenPanel {
            prompt_file,
            log,
            cwd,
            session: _,
        } => {
            let prompt = std::fs::read_to_string(&prompt_file)
                .with_context(|| format!("read {}", prompt_file.display()))?;
            print!(
                "{}",
                ljos_cli::open_decision_panel(&prompt, cwd.as_deref(), &log)?
            );
        }
        Cmd::File {
            title,
            from,
            parent,
            top,
            project,
            kind,
            tags,
            priority,
            body,
            options,
            quorum,
            deadline,
            porcelain: _,
            verbose,
            json,
        } => {
            // The same seat line `ljos remember` prints. A seat taken from
            // the most recent write that is still the fallback, and the
            // commit line alone does not say to name one.
            ljos_cli::seat_resolve::announce_named_unknown(&ljos_cli::whoami());
            if let Some(from) = from {
                let rows = ljos_cli::bulk_file::read_rows(&from)?;
                if let Some(parent) = parent.as_deref() {
                    stop_ticket(parent);
                }
                let defaults = ljos_cli::bulk_file::Defaults {
                    project,
                    parent,
                    kind,
                    tags,
                    priority,
                };
                // A row with no parent of its own is placed as `ljos file`
                // places one, under the held issue.
                let skip =
                    top || defaults.parent.is_some() || rows.iter().all(|row| row.parent.is_some());
                let hold = file_hold(skip)?;
                let (placed, mut notes) =
                    ljos_cli::bulk_file::place_rows(&rows, &defaults, top, hold.held)?;
                if let Some(id) = hold.dropped {
                    notes.insert(
                        0,
                        format!("held issue {id} is gone; rows with no parent are filed at the top of their project\n"),
                    );
                } else if let Some(id) = hold.other_tracker {
                    notes.insert(
                        0,
                        format!("held issue {id} is on another tracker; rows with no parent are filed at the top of their project\n"),
                    );
                }
                let filed = ljos_cli::bulk_file::file_rows(&placed, &defaults)?;
                ljos_cli::seat_resolve::note_seat_write(&ljos_cli::seat_name());
                for id in &filed.ids {
                    println!("{id}");
                }
                let mut rest = notes.concat();
                let _ = std::fmt::Write::write_fmt(
                    &mut rest,
                    format_args!(
                        "filed {} tickets in {}\n",
                        filed.ids.len(),
                        filed.projects.join(", ")
                    ),
                );
                rest.push_str(&filed.persisted);
                if !git_detail(verbose) {
                    rest = ljos_cli::without_routine_git(&rest);
                }
                eprint!("{rest}");
                return Ok(());
            }
            let title = title.unwrap_or_default();
            if let Some(parent) = parent.as_deref() {
                stop_ticket(parent);
            }
            if let Some(d) = &deadline {
                chrono::DateTime::parse_from_rfc3339(d.trim())
                    .map_err(|e| anyhow::anyhow!("--deadline {d:?} is not RFC 3339: {e}"))?;
            }
            let body = ljos_cli::body_with_options(body, &options.into_iter().collect::<Vec<_>>())?;
            let named_parent = parent.is_some();
            let named_project = project.is_some();
            // `--top` and `--parent` are a parent the seat chose. Only the
            // plain command asks what this conversation holds.
            let FileHold {
                held,
                dropped,
                other_tracker,
            } = file_hold(top || named_parent)?;
            let no_hold = held.is_none();
            let (parent, mut project, passed_over) =
                ljos_cli::file_placement(top, parent, project, held);
            if project.is_none()
                && parent.is_none()
                && passed_over
                    .as_deref()
                    .is_some_and(|line| line.starts_with("project: "))
            {
                anyhow::bail!("{}", passed_over.unwrap_or_default().trim_end());
            }
            // A plain file with no hold and no `-p` may still name one
            // project through `LJOS_PROJECT`.
            if project.is_none() && no_hold && !top && !named_parent && !named_project {
                project = std::env::var("LJOS_PROJECT")
                    .ok()
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty());
            }
            let project = match project {
                Some(project) => project,
                // `--top` still needs a project the caller named. A plain
                // file, with no checkout prefix and no hold, uses the
                // seat's default project. Nothing is asked. The line is
                // printed once the ticket exists, so a missing tracker
                // stays one error line.
                None if top => ljos_cli::seat_tracker_project_no_cwd().ok_or_else(|| {
                    anyhow::anyhow!(
                        "no project: name one with -p, or a parent with --parent, so run `ljos file \"TITLE\" -p NAME`."
                    )
                })?,
                // A plain file still needs a known project from the sitting,
                // config, or `LJOS_PROJECT`.
                None => anyhow::bail!(
                    "no project: name one with -p NAME, so run `ljos file \"TITLE\" -p NAME`."
                ),
            };
            let qd = match (quorum, deadline.as_deref()) {
                (Some(n), Some(d)) => Some((n, d)),
                _ => None,
            };
            let (id, text, persisted, candidate) = match ljos_cli::file_issue(
                &project,
                parent.as_deref(),
                kind.as_deref(),
                tags.as_deref(),
                priority.as_deref(),
                body.as_deref(),
                &title,
                &ljos_cli::seat_name(),
                false,
                qd,
            ) {
                Ok(filed) => filed,
                Err(err) => {
                    // A tracker that did not answer has written nothing.
                    // The id must not appear on stdout, and the status is
                    // a failure. `call_deadline::stop` already exits for
                    // the line it prints; this is the same for a line that
                    // came back as an error.
                    let raw = err.to_string();
                    if ljos_cli::call_deadline::stalled(&raw) {
                        eprintln!("{}", ljos_cli::user_fault_line(&err));
                        std::process::exit(1);
                    }
                    return Err(ticket_error(err));
                }
            };
            // A failure above printed nothing that says the ticket was filed.
            if !id.is_empty() {
                if let Some(gone) = &dropped {
                    eprintln!("held issue {gone} is gone; filed at the top of {project}");
                } else if let Some(id) = &other_tracker {
                    eprintln!(
                        "held issue {id} is on another tracker; filed at the top of {project}"
                    );
                } else if let Some(line) = &passed_over {
                    eprint!("{line}");
                }
            }
            let mut rest = ljos_cli::ensure_project_line(&text, &project);
            if let (Some((n, d)), false) = (qd, id.is_empty()) {
                let note = format!("{}{n} by {}", ljos_cli::reap::QUORUM_NOTE, d.trim());
                let _ = std::fmt::Write::write_fmt(
                    &mut rest,
                    format_args!("{id}: {note}; `ljos reap --apply` lapses it past the deadline\n"),
                );
            }
            rest.push_str(&persisted);
            match candidate {
                Ok(said) => rest.push_str(&said),
                Err(e) => eprintln!("ljos file: {e:#}"),
            }
            if !id.is_empty() && parent.is_none() {
                let _ = std::fmt::Write::write_fmt(
                    &mut rest,
                    format_args!("next: `ljos sitting {id}` starts work.\n"),
                );
            }
            let detail = git_detail(verbose);
            // The id is the only line on stdout, so `ID=$(ljos file ...)`
            // captures the id the docs name. The heading and the path go
            // to stderr. A commit that landed is printed with `--verbose`.
            // A refusal is still one sentence. `--json` keeps the commit
            // and sync lines in fields. `--porcelain` is the same as the
            // plain id.
            if !id.is_empty() {
                ljos_cli::seat_resolve::note_seat_write(&ljos_cli::seat_name());
            }
            if json {
                let text = if detail {
                    rest.clone()
                } else {
                    ljos_cli::without_routine_git(&rest)
                };
                print!(
                    "{}",
                    ljos_cli::format_rendered_json(
                        &text,
                        &rest,
                        (!id.is_empty()).then_some(id.as_str())
                    )
                );
            } else {
                if !detail {
                    rest = ljos_cli::without_routine_git(&rest);
                }
                if !id.is_empty() {
                    println!("{id}");
                }
                eprint!("{rest}");
            }
        }
        Cmd::Options { issue, options } => {
            stop_ticket(&issue);
            print!("{}", ljos_cli::append_issue_options(&issue, &[options])?);
        }
        Cmd::Note {
            words,
            verbose,
            json,
            quiet,
        } => {
            let (issue, text, inferred) = note_target(&words)?;
            // A product checkout is not a tracker. `vissue note` would
            // create one there when the id is missing.
            ljos_cli::refuse_product_init(&issue)?;
            stop_ticket(&issue);
            // The note is the tracker write. Reconciling a claim the
            // graph did not take waits until that write has landed, and
            // only for a short bound. The tracker prints `{id}: noted`
            // and nothing else is added to that confirmation: a second
            // `noted on` line used to follow it.
            let captured = ljos_cli::run_for(
                "vissue",
                &["note", &issue, &text],
                ljos_cli::tracker_verb_wait(),
            )?;
            eprint!("{}", captured.stderr);
            warn_other_sitting(&issue, quiet, inferred, false);
            let mut said = format!("{issue}: noted\n");
            said.push_str(&ljos_cli::retry_pending_claim(&issue));
            if let Some(line) = ljos_cli::assist_note_line(&issue) {
                said.push_str(&line);
            }
            match ljos_cli::ballot_sig::sign_own_ballot(&issue) {
                Ok(line) => said.push_str(&line),
                Err(e) => eprintln!("ljos note: {e:#}"),
            }
            said.push_str(&ljos_cli::persist_tracker(&issue, "noted"));
            ljos_cli::stamp_note_seat(&issue);
            ljos_cli::seat_resolve::note_seat_write(&ljos_cli::seat_name());
            print!("{}", render_write(&said, git_detail(verbose), json, None));
        }
        Cmd::Slot {
            issue,
            seat,
            model,
            run,
            key,
        } => {
            print!(
                "{}",
                ljos_cli::slot::write(&issue, &seat, &model, &run, &key)?
            );
            print!("{}", ljos_cli::persist_tracker(&issue, "slotted"));
        }
        Cmd::Log { seat } => print!("{}", ljos_cli::tracker_log(Some(&seat))?),
        Cmd::Usage { since } => print!("{}", ljos_cli::tracker_usage(&since)?),
        Cmd::Digest { since, json, mark } => {
            print!(
                "{}",
                ljos_cli::digest::digest_cli(since.as_deref(), json, mark)?
            );
        }
        Cmd::Send {
            words,
            group,
            interrupt,
            issue,
            new_seat,
        } => {
            let (seat, text) = if group.is_some() {
                (None, words.join(" "))
            } else {
                let mut words = words;
                if words.len() < 2 {
                    bail!("send: name a seat and the message");
                }
                let seat = words.remove(0);
                (Some(seat), words.join(" "))
            };
            print!(
                "{}",
                ljos_cli::mail::send(&ljos_cli::mail::Send {
                    seat: seat.as_deref(),
                    group: group.as_deref(),
                    text: &text,
                    interrupt,
                    issue: issue.as_deref(),
                    new_seat,
                })?
            );
        }
        Cmd::Inbox { all, sync } => {
            print!("{}", ljos_cli::mail::inbox_sync(all, sync.as_deref())?)
        }
        Cmd::Read { id } => print!("{}", ljos_cli::mail::read(&id)?),
        Cmd::Reply {
            id,
            text,
            interrupt,
        } => print!(
            "{}",
            ljos_cli::mail::reply(&id, &text.join(" "), interrupt)?
        ),
        Cmd::Group { name, add, remove } => {
            print!("{}", ljos_cli::mail::group(&name, &add, &remove)?)
        }
        Cmd::Wake {
            seat,
            command,
            webhook,
            key_file,
            key_env,
            timeout,
            clear,
            fire,
            from,
            msg,
            issue,
            interrupt,
        } => {
            let seat = seat.unwrap_or_else(ljos_cli::seat_name);
            let said = if let Some(command) = command {
                ljos_cli::wake::set_command(&seat, &command, timeout)?
            } else if let Some(url) = webhook {
                ljos_cli::wake::set_webhook(&seat, &url, key_file.as_deref(), key_env.as_deref())?
            } else if clear {
                ljos_cli::wake::clear(&seat)?
            } else if fire {
                ljos_cli::wake::fire(&ljos_cli::wake::Event {
                    seat,
                    from: from.unwrap_or_else(ljos_cli::seat_name),
                    msg: msg.unwrap_or_default(),
                    issue,
                    interrupt,
                    // Push mode fills this after the sealed log is committed.
                    commit: None,
                })?
            } else {
                ljos_cli::wake::show(&seat)
            };
            print!("{said}");
        }
        Cmd::Block {
            issue,
            also,
            on,
            off,
        } => {
            stop_ticket(&issue);
            for id in &also {
                stop_ticket(id);
            }
            if let Some(id) = &on {
                stop_ticket(id);
            }
            let said = if !also.is_empty() {
                let mut out = String::new();
                for id in &also {
                    out.push_str(&ljos_cli::block_edge(&issue, id, false)?);
                }
                out
            } else {
                match (on, off) {
                    (Some(on), None) => ljos_cli::block_edge(&issue, &on, false)?,
                    (None, Some(off)) => ljos_cli::block_edge(&issue, &off, true)?,
                    _ => bail!("block: pass --on ID to add an edge, or --off ID to drop one"),
                }
            };
            print!("{said}");
        }
        Cmd::Parent { issue, parent } => {
            stop_ticket(&issue);
            stop_ticket(&parent);
            print!("{}", ljos_cli::reparent_issue(&issue, &parent)?);
        }
        Cmd::Show { issue, json, all } => {
            let said = ljos_cli::show_memory::show_any(&issue, json, all).map_err(show_error)?;
            if json {
                print!("{said}");
            } else {
                print!("{}", ljos_cli::scan::for_reader(&said, human_color()));
            }
        }
        Cmd::Ready(args) | Cmd::List(args) => print_ready(args)?,
        Cmd::Claims {
            mine,
            stale,
            days,
            json,
            by,
            agent,
            project,
            args,
        } => {
            let forwarded =
                by.is_some() || agent.is_some() || project.is_some() || !args.is_empty();
            if (mine || stale) && forwarded {
                bail!(
                    "claims: --by and -p belong to `vissue claims`, so drop them with --mine and --stale"
                );
            }
            if mine {
                if json {
                    bail!("claims: --json is for --stale");
                }
                print!("{}", ljos_cli::quiet_claims::list_mine()?);
                return Ok(());
            }
            if stale {
                print!("{}", ljos_cli::quiet_claims::list_stale(days, json)?);
                return Ok(());
            }
            if days.is_some() {
                bail!("claims: --days is for --stale");
            }
            // Extra words stay on the vissue binary, which has flags this
            // command forwards. With none, the age is counted here from the
            // claim instant. The binary subtracts calendar dates.
            if args.is_empty() {
                print!(
                    "{}",
                    ljos_cli::claims_view(by.or(agent).as_deref(), project.as_deref(), json)?
                );
                return Ok(());
            }
            // vissue names the holder filter `--by`; `--agent` is ljos's word.
            let mapped: Vec<String> = args
                .iter()
                .map(|a| match a.as_str() {
                    "--agent" => "--by".to_string(),
                    a => a
                        .strip_prefix("--agent=")
                        .map_or_else(|| a.to_string(), |v| format!("--by={v}")),
                })
                .collect();
            let mut argv = vec!["claims".to_string()];
            if json {
                argv.push("--json".to_string());
            }
            if let Some(name) = by.or(agent) {
                argv.push("--by".to_string());
                argv.push(name);
            }
            if let Some(project) = project {
                argv.push("--project".to_string());
                argv.push(project);
            }
            argv.extend(mapped);
            let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
            run("vissue", &refs)?;
        }
        Cmd::Recall {
            issue,
            json,
            no_graph,
        } => {
            ljos_cli::config_diag::emit();
            let trimmed = issue.trim();
            if trimmed.is_empty() {
                eprintln!("recall: the issue id is empty, so run `ljos recall proj-1a2b`.");
                std::process::exit(USAGE_STATUS);
            }
            if !looks_like_ticket_id(trimmed) {
                let search_cmd = recall_search_command(trimmed);
                if search_cmd.contains('`') {
                    eprintln!("recall: that text is not a ticket id, so run {search_cmd}.");
                } else {
                    let shown =
                        format!("\"{}\"", trimmed.replace('\\', "\\\\").replace('"', "\\\""));
                    eprintln!("recall: {shown} is not a ticket id, so run `{search_cmd}`.");
                }
                std::process::exit(USAGE_STATUS);
            }
            stop_ticket(&issue);
            ljos_cli::log_recall("recall", &issue);
            let said = ljos_cli::tracker_recall(&issue).map_err(ticket_error)?;
            eprint!("{}", ljos_cli::without_raw_rows(&said.stderr));
            let memories = ljos_cli::recall_for_command(&issue)?;
            let code = if no_graph {
                String::new()
            } else {
                ljos_cli::graphify::recall_section(&issue)
            };
            let ranking = ljos_cli::ranking_now();
            if json {
                print!(
                    "{}",
                    ljos_cli::recall_json(&format!("{memories}{code}"), ranking.as_ref())
                );
            } else {
                if let Some(ranking) = ranking.as_ref() {
                    ljos_cli::say_ranking(ranking);
                }
                let page = format!("{}{memories}{code}", said.stdout);
                print!(
                    "{}",
                    ljos_cli::without_raw_rows(&ljos_cli::scan::for_reader(&page, human_color()))
                );
            }
        }
        Cmd::Reap { idle, apply } => {
            print!(
                "{}",
                ljos_cli::reap::reap(ljos_cli::reap::parse_idle(&idle)?, apply)?
            )
        }
        Cmd::Vote {
            issue,
            choice,
            confidence,
            used,
            expect,
            as_persona,
            jev,
            withdraw,
            against,
            child,
        } => {
            stop_ticket(&issue);
            let against_note = if let Some(reason) =
                against.as_deref().map(str::trim).filter(|s| !s.is_empty())
            {
                let id = ljos_cli::against_child(&issue, child.as_deref())?;
                Some((reason.to_string(), id))
            } else if child.is_some() {
                bail!(
                    "vote: `--child` belongs to a no ballot. Pass `--against REASON --child ID`. Nothing was cast"
                );
            } else {
                None
            };
            let choice = if against_note.is_some() {
                Some("no".to_string())
            } else {
                choice
            };
            match choice {
                None if withdraw => {
                    let who = ljos_cli::ballot_voter_on(&issue, as_persona.as_deref())?;
                    print!("{}", ljos_cli::undo::withdraw_ballot(&issue, &who)?);
                }
                None if jev => {
                    let name = as_persona.as_deref().unwrap_or_default();
                    let cast = ljos_cli::panel_gate::claim_session_ballot(&issue, name)?;
                    match ljos_cli::jev_vote(name, &issue)? {
                        ljos_cli::JevVote::Cast(b) => {
                            cast.keep();
                            let voter = if b.model.is_empty() {
                                name.to_string()
                            } else {
                                ljos_cli::judge_voter(&b.model)
                            };
                            println!(
                                "{voter}: cast {} for {name} at confidence {:.2}",
                                b.choice, b.confidence
                            );
                            print!(
                                "{}",
                                ljos_cli::hook_timed("vote_persist", || {
                                    ljos_cli::persist_tracker(&issue, "ballot cast")
                                })
                            );
                        }
                        ljos_cli::JevVote::Escalated(b) => {
                            drop(cast);
                            println!(
                            "{name}: Jev leaned {} at confidence {:.2}, under the {:.2} cut; not cast.",
                            b.choice, b.confidence, b.escalate_below
                        );
                            let persona = ljos_cli::personas_from_pack()
                                .unwrap_or_default()
                                .into_iter()
                                .find(|p| p.name == name);
                            match persona.as_ref().and_then(|p| ljos_cli::hand_ballot(p, &issue)) {
                            Some(pane) => println!(
                                "  {name} reasons in its own session in {pane}; then `ljos consensus {issue}`"
                            ),
                            None => println!("Start a subagent from `ljos brief {name} {issue}`"),
                        }
                            print!(
                                "{}",
                                ljos_cli::persist_tracker(&issue, "escalated a ballot")
                            );
                        }
                    }
                }
                Some(c) => {
                    let who = ljos_cli::ballot_voter_on(&issue, as_persona.as_deref())?;
                    let cast = ljos_cli::hook_timed("vote_claim", || {
                        ljos_cli::panel_gate::claim_session_ballot(&issue, &who)
                    })?;
                    let used = used.as_deref().unwrap_or("");
                    if used.is_empty() {
                        bail!(
                        "a ballot records what it used (doi:10.1007/3-540-44503-X_20); pass --used deed-... or --used none"
                    );
                    }
                    let mut args = vec![
                        "vote".to_string(),
                        issue.clone(),
                        "--for".into(),
                        c,
                        "--used".into(),
                        used.to_string(),
                    ];
                    if let Some(p) = confidence {
                        args.push("--confidence".into());
                        args.push(format!("{p}"));
                    }
                    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                    ljos_cli::hook_timed("vote_cast", || run_as("vissue", &refs, Some(&who)))?;
                    // Read the ballot back: only the tracker is proof it exists.
                    let rows: Vec<serde_json::Value> =
                        ljos_cli::hook_timed("vote_readback", || {
                            ljos_cli::raw_ballot_rows(&issue).unwrap_or_default()
                        });
                    if !ljos_cli::tracker_holds_ballot(&rows, &who, &args[3]) {
                        bail!(
                        "the tracker does not hold a ballot from {who} for {} on {issue}: nothing was cast",
                        args[3]
                    );
                    }
                    println!("recorded on the tracker: {who} for {} on {issue}", args[3]);
                    cast.keep();
                    // Every ballot, a persona's included, is signed and bound to
                    // this connection. The push gate still counts only seat
                    // ballots; the panel gate counts the session, not the seat.
                    match ljos_cli::hook_timed("vote_sign", || {
                        ljos_cli::ballot_sig::sign_ballot(&issue, &who, &args[3])
                    }) {
                        Ok(said) => print!("{said}"),
                        Err(e) => eprintln!("ljos vote: {e:#}"),
                    }
                    if let Some(expect) = expect.as_deref().map(str::trim).filter(|e| !e.is_empty())
                    {
                        write_prediction(&issue, &who, expect).with_context(|| {
                            format!("ballot cast; forecast for {who} was not recorded")
                        })?;
                        println!("forecast recorded for {who}");
                    }
                    print!("{}", ljos_cli::persist_tracker(&issue, "ballot cast"));
                    if let Some((reason, child_id)) = &against_note {
                        let note = format!("{who}: against: {reason} (child {child_id})");
                        run_as("vissue", &["note", &issue, &note], Some(&who))?;
                        println!(
                        "no ballot linked to child {child_id}. {issue} stays open while {child_id} is open. After `ljos finish {child_id} --close`, replace the ballot: `ljos vote {issue} --withdraw --as {who}` then `ljos vote {issue} --for OPTION --used none --as {who}`, then `ljos consensus {issue}`."
                    );
                    }
                }
                None => print!("{}", ljos_cli::panel_gate::tally_report(&issue)?),
            }
        }
        Cmd::Brief { name, issue } => {
            ljos_cli::panel_gate::note_tracker_read(&issue);
            print!("{}", brief(&name, &issue)?);
        }
        Cmd::Timeline { issue, limit, json } => {
            ljos_cli::panel_gate::note_tracker_read(&issue);
            if json {
                print!("{}", ljos_cli::timeline_json(&issue, limit)?);
            } else {
                print!(
                    "{}",
                    ljos_cli::scan::for_reader(&timeline(&issue, limit)?, human_color())
                );
            }
        }
        Cmd::Panel {
            issue,
            run_issue,
            out,
            jev,
            harness,
            designers,
            dry_run,
            cloud,
        } => {
            if cloud {
                if issue == "run" || run_issue.is_some() {
                    bail!(
                        "panel --cloud writes one prompt file per persona for one issue; it does not spawn"
                    );
                }
                print!("{}", ljos_cli::panel_cloud::panel_cloud(&issue, &out)?);
            } else if issue == "run" {
                let issue = run_issue.context("`ljos panel run` needs an issue")?;
                print!(
                    "{}",
                    ljos_cli::panel_gate::panel_run(
                        &issue,
                        harness.as_deref(),
                        designers,
                        dry_run
                    )?
                );
            } else if run_issue.is_some() {
                bail!("panel: one issue, or `ljos panel run ISSUE`");
            } else if jev {
                print!("{}", ljos_cli::panel_jev(&issue, &out)?);
                print!("{}", ljos_cli::persist_tracker(&issue, "panel through Jev"));
            } else {
                print!("{}", panel(&issue, &out)?);
            }
        }
        Cmd::Persona {
            name,
            anchor,
            view,
            about,
            runner,
            replace,
            check,
            retire,
        } => {
            if retire {
                print!("{}", ljos_cli::retire_persona(&name, replace)?);
                ljos_cli::refresh_agents();
                return Ok(());
            }
            let persona = Persona {
                name,
                anchor,
                view: view.unwrap_or_default(),
                entities: about,
                runner,
            };
            let how = if replace {
                PersonaWrite::Replace
            } else {
                PersonaWrite::Define
            };
            if check {
                print!("{}", ljos_cli::check_persona(&persona, how)?);
                return Ok(());
            }
            let body = write_persona_as(&persona, how)?;
            println!("{}", write_ack(&body, false));
            ljos_cli::refresh_agents();
        }
        Cmd::Upgrade { version, dir, only } => {
            print!(
                "{}",
                ljos_cli::upgrade::upgrade(version.as_deref(), dir.as_deref(), only.as_deref())?
            );
        }
        Cmd::Supervise {
            name,
            harness,
            restart,
            max_restarts,
            window,
            exit,
            at,
            failures,
            cwd,
            script,
        } => {
            let restart = ljos_cli::plugin::Restart::parse(&restart).with_context(|| {
                format!("restart {restart}: permanent, transient, or temporary")
            })?;
            let exit = ljos_cli::plugin::ExitKind::parse(&exit).ok_or_else(|| {
                anyhow::anyhow!("supervise: --exit takes clean or crash, not {exit}.")
            })?;
            let failures = failures
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| {
                    s.trim()
                        .parse::<u64>()
                        .with_context(|| format!("failure time {s}"))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut sup = ljos_cli::plugin::Supervisor {
                child: ljos_cli::plugin::Child {
                    name: name.clone(),
                    harness: harness.clone(),
                    restart,
                    max_restarts,
                    window_secs: window,
                },
                failures,
            };
            let action = sup.on_exit(exit, at);
            let herdr = if action == ljos_cli::plugin::Action::Restart {
                ljos_cli::plugin::herdr_argv(&name, &cwd, &script)
            } else {
                Vec::new()
            };
            let decision = ljos_cli::plugin::Decision {
                action,
                harness,
                herdr,
                failures: sup.failures,
            };
            println!("{}", serde_json::to_string_pretty(&decision)?);
        }
        Cmd::Ask { name, text } => {
            println!("{}", ljos_cli::ask_persona(&name, &text.join(" "))?);
        }
        Cmd::Personas => {
            print!("{}", ljos_cli::personas_listing()?);
        }
        Cmd::Agents { harness, dry_run } => {
            print!(
                "{}",
                ljos_cli::format_steps(&ljos_cli::export_agents(harness.as_deref(), dry_run)?)
            );
        }
        Cmd::Statusline => {
            // A runner pipes its status JSON. At a terminal, with nothing
            // piped, reading stdin waits forever.
            if std::io::stdin().is_terminal() {
                eprintln!("{}", statusline_tty_hint());
                std::process::exit(USAGE_STATUS);
            }
            let mut input = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
            println!("{}", ljos_cli::statusline(&input));
        }
        Cmd::Playbook { issue, name } => {
            stop_ticket(&issue);
            print!("{}", copy_playbook(&issue, &name)?);
            print!("{}", ljos_cli::persist_tracker(&issue, "bound a playbook"));
        }
        Cmd::Playbooks => {
            print!("{}", format_playbooks(&playbooks_from_pack()?));
        }
        Cmd::Workflow {
            issue,
            start,
            round,
        } => {
            if let Some(name) = start {
                print!("{}", ljos_cli::workflow_start(&issue, &name)?);
            } else if round {
                print!("{}", ljos_cli::workflow_round(&issue)?);
            } else {
                match ljos_cli::workflow_status(&issue)? {
                    Some(s) => print!("{}", ljos_cli::format_workflow_status(&s)),
                    None => bail!(
                        "workflow: {issue} carries no workflow; `ljos workflow {issue} --start feature` starts one"
                    ),
                }
            }
        }
        Cmd::Claim {
            node,
            takeover,
            assignee,
            next,
            role,
            slack,
        } => {
            let assignee = assignee_for("claim", assignee.as_deref(), false)?;
            match node.filter(|_| !next) {
                Some(node) => {
                    let node = ljos_cli::on_disk(&node);
                    stop_ticket_node(&node);
                    ljos_cli::require_ticket_to_claim(&node)?;
                    let said = if takeover {
                        ljos_cli::takeover::claim_takeover(&node, &assignee)
                    } else {
                        claim(&node, &assignee)
                    };
                    let said = match said {
                        Ok(said) => said,
                        Err(err) => {
                            eprintln!("{}", ljos_cli::user_fault_line(&err));
                            std::process::exit(1);
                        }
                    };
                    print!("{said}");
                    if !takeover {
                        print!("{}", ljos_cli::persist_tracker(&node, "claimed"));
                    }
                }
                None => {
                    let said = claim_next(&assignee, role.as_deref(), slack)?;
                    let taken = said.split_whitespace().next().unwrap_or("").to_string();
                    print!("{said}");
                    if !taken.is_empty() {
                        print!("{}", ljos_cli::persist_tracker(&taken, "claimed"));
                    }
                }
            }
        }
        Cmd::Release {
            node,
            assignee,
            stale,
            days,
            yes,
            older_than,
            dry_run,
            verbose,
        } => {
            // `release HEX --assignee NAME` hands back a claim another
            // name still holds. `--stale` with an id is refused here, in
            // one line, rather than as a flag conflict.
            if stale && node.is_some() {
                bail!("release: pass an id or --stale, not both");
            }
            if stale && (older_than.is_some() || dry_run || verbose) && (days.is_some() || yes) {
                bail!("release: pass --days and --yes, or --older-than and --dry-run, not both");
            }
            let quiet = (stale && older_than.is_some())
                || dry_run
                || verbose
                || (stale && days.is_none() && !yes);
            if quiet {
                if let Some(name) = assignee.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    let seat = ljos_cli::seat_name();
                    if ljos_cli::seat_slug(name) != ljos_cli::seat_slug(&seat)
                        && !ljos_cli::same_holder(name, &seat)
                    {
                        bail!(
                            "release: --stale releases this seat's claims, and {name} is another seat"
                        );
                    }
                }
                let window = match older_than.as_deref() {
                    Some(raw) => Some(ljos_cli::quiet_claims::parse_older_than(raw)?),
                    None => None,
                };
                print!(
                    "{}",
                    ljos_cli::quiet_claims::release_quiet(
                        &ljos_cli::seat_name(),
                        window,
                        dry_run,
                        verbose,
                    )?
                );
            } else if stale {
                let assignee = assignee_for("release", assignee.as_deref(), true)?;
                print!(
                    "{}",
                    ljos_cli::quiet_claims::release_stale(&assignee, days, yes)?
                );
            } else if let Some(node) = node {
                let assignee = assignee_for("release", assignee.as_deref(), true)?;
                stop_ticket_node(&node);
                let said = release(&node, &assignee)?;
                print!("{said}");
                if !said.is_empty() {
                    print!("{}", ljos_cli::persist_tracker(&node, "released"));
                }
            } else {
                bail!("release: pass an id, or --stale");
            }
        }
        Cmd::Complete {
            node,
            status,
            gen,
            assignee,
        } => {
            let assignee = assignee_for("complete", assignee.as_deref(), false)?;
            stop_ticket_node(&node);
            print!("{}", complete(&node, status.as_deref(), &assignee, gen)?)
        }
        Cmd::Cards { dir } => {
            let out = cards(&dir)?;
            if out.is_empty() {
                // `.` plus the sentence period read as the parent directory.
                let joined = if dir.is_absolute() {
                    dir.clone()
                } else {
                    std::env::current_dir()
                        .map(|cwd| cwd.join(&dir))
                        .unwrap_or_else(|_| dir.clone())
                };
                let shown = joined.canonicalize().unwrap_or(joined);
                println!(
                    "no USER.md or MEMORY.md in {}. Write them there, or pass --dir. ljos reads them and does not write them.",
                    shown.display()
                );
            } else {
                print!("{out}");
            }
        }
        Cmd::Predict {
            issue,
            expect,
            as_persona,
        } => {
            stop_ticket(&issue);
            let who = ljos_cli::ballot_voter("predict", as_persona.as_deref())?;
            let body = write_prediction(&issue, &who, &expect)?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Cmd::Rule {
            pattern,
            verdict,
            why,
            json,
            user,
        } => {
            if user {
                let basis = if verdict == "allow" {
                    ljos_cli::allow_basis()?
                } else {
                    ljos_cli::Basis::of_rule()
                };
                let rule = ljos_cli::user_rules::write_argv(
                    &Rule {
                        pattern,
                        verdict,
                        reason: why,
                    },
                    basis,
                )?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&ljos_cli::user_rules::to_value(&rule))?
                    );
                } else {
                    println!("{}", ljos_cli::user_rules::ack(&rule));
                }
            } else {
                let basis = ljos_cli::Basis::of_rule();
                let body = write_rule(
                    &Rule {
                        pattern,
                        verdict,
                        reason: why,
                    },
                    basis,
                )?;
                println!(
                    "{}",
                    if json {
                        ljos_cli::atom_out(&body, true)?
                    } else {
                        write_ack(&body, false)
                    }
                );
            }
        }
        Cmd::Rules { json } => {
            match ljos_cli::rules_from_pack() {
                Ok(rules) => print!("{}", ljos_cli::format_rules(&rules, json)),
                Err(err) => {
                    if ljos_cli::user_rules::load().unwrap_or_default().is_empty() {
                        return Err(err);
                    }
                    eprintln!("ljos: seat rules: {err:#}");
                }
            }
            if !json {
                ljos_cli::user_rules::note_cutoff();
                print!("{}", ljos_cli::user_rules::format_list());
            }
        }
        Cmd::Policy { argv, fail_on_deny } => {
            if argv.is_empty() {
                eprintln!(
                    "policy: pass the command to check, so run `ljos policy -- git push --force`."
                );
                std::process::exit(USAGE_STATUS);
            }
            let text = policy_with_memory(&argv)?;
            print!("{text}");
            if ljos_cli::shell_exit(fail_on_deny, &text) != 0 {
                std::process::exit(1);
            }
        }
        Cmd::GitHook { name, .. } => {
            ljos_cli::config_diag::emit();
            ljos_cli::git_hooks::run_named(&name)?;
        }
        Cmd::GitHooks { action } => ljos_cli::git_hooks::run(&action)?,
        Cmd::PersonKey { action } => ljos_cli::person_key::run(&action)?,
        Cmd::Hook(hook) => return run_hook(hook),
        Cmd::Consensus {
            id,
            json,
            compare,
            no_file,
            quorum,
            deadline,
        } => {
            if let Some(line) = ljos_cli::sandbox_memory_off() {
                eprintln!("{line}");
                std::process::exit(1);
            }
            stop_ticket(&id);
            // One silent pack is waited out once, inside the command budget.
            // Trust, the roster and the listing each ask, and a second wait
            // on the thirty-second default would hold the reading past it.
            let _pack_clock = ljos_cli::arm_cmd_pack();
            let _quiet = ljos_cli::pack_link::QuietPack::enter();
            // Each ticket of the family is read once for the whole reading.
            // A dissent filed below is a write, and is read afresh.
            let reading = ljos_cli::TrackerReading::enter();
            // Rows scoped to a domain apply when the issue is about it; the
            // personas' anchors go to both settles; the issue's tags pick
            // the model.
            let (topic, tags) =
                ljos_cli::hook_timed("consensus_issue", || issue_topic_and_tags(&id));
            // One read of the kinds the reading uses: trust rows, outcomes
            // and forecasts, not the whole pack twice.
            let atoms =
                ljos_cli::hook_timed("consensus_trust", || match ljos_cli::consensus_atoms() {
                    Ok(atoms) => atoms,
                    Err(e) => {
                        eprintln!("ljos: no trust rows ({e:#}); settling with equal weights");
                        Vec::new()
                    }
                });
            let trust = rows_about(&ljos_cli::trust_rows(&atoms), &topic);
            let personas = ljos_cli::hook_timed("consensus_personas", || {
                personas_from_pack().unwrap_or_default()
            });
            // A persona on the ballot settles under its anchor, and every
            // other voter, the seat too, under the default one; a ballot
            // of seats alone stays plain DeGroot.
            let ballots = ljos_cli::hook_timed("consensus_ballots", || ljos_cli::ballot_rows(&id));
            ljos_cli::require_ballots(&id, &ballots)?;
            let provenance = ljos_cli::panel_gate::panel_provenance(&id);
            if let Some(msg) = ljos_cli::hook_timed("consensus_refusal", || {
                ljos_cli::panel_gate::settle_refusal(&id)
            }) {
                println!("ljos: session-authored: {msg}");
            }
            for line in ljos_cli::panel_gate::blind_lines_for(&id) {
                println!("blind design: {line}");
            }
            if let Some(n) = quorum {
                quorum_or_exit(&id, n, deadline.as_deref(), json)?;
            }
            let anchors = ljos_cli::settle_anchors(&personas, &ljos_cli::voters_of(&ballots));
            // The model crate's settle is the one printed; the tracker's
            // runs under --compare, or when the model crate is not there.
            let have_model = on_path("ljos-consensus");
            let mut steps = consensus_steps_for(
                &id,
                have_model,
                on_path("vissue") && (compare || !have_model),
                &trust,
                &anchors,
                &tags,
            )?;
            // The model crate's `--issue` re-reads the drawer, where a
            // session's later personas still sit. The counted ballots are
            // the tally.
            ljos_cli::bind_settle_ballots(&mut steps, &ballots);
            // First what the push gate decides: seat ballots only.
            let (gate, gate_json) = ljos_cli::hook_timed("consensus_cite", || {
                ljos_cli::push_cite_reading_on_roster(&id, &ballots, &ljos_cli::seat_name())
            });
            let (protected, protected_json) = ljos_cli::hook_timed("consensus_protected", || {
                ljos_cli::protected_push_reading(&id, std::env::current_dir().ok().as_deref())
            });
            // The named outcomes show which voters err together, and
            // those voters are discounted. Each model family weighs as one
            // voter too, and ballots no record labels share one family.
            let families = ljos_cli::hook_timed("consensus_families", || {
                ljos_cli::slot::ballot_families(&id, &ballots, &personas)
            });
            let correlation = settle_discount(&atoms);
            let mut weights = ljos_cli::slot::family_weights(&families);
            if let Some((discount, _)) = &correlation {
                for (voter, d) in discount {
                    *weights.entry(voter.clone()).or_insert(1.0) *= d;
                }
            }
            if !weights.is_empty() {
                with_discount(&mut steps, &weights);
            }
            let discount = correlation.map(|(_, line)| line);
            // Beside the settle: the surprisingly popular answer when two
            // or more voters forecast, and the voters' standing when rows
            // exist.
            let predictions = predictions_of(&atoms, &id);
            let panel = panel_steps(&id, on_path("ljos-consensus"), &trust, &predictions);
            // A blocking objection with a reason is filed once, as
            // `Dissent holds TEXT`. A nit, a wording note or a style note
            // is written on the issue. A minority a voter still holds after
            // a settled majority is filed once, as `Dissent holds OPTION`.
            // A tie files nothing. Each filing reading sits one panel round.
            // `--no-file` keeps this a pure read. Filing never fails the
            // reading.
            drop(reading);
            let dissent = if no_file || !on_path("vissue") {
                Vec::new()
            } else {
                ljos_cli::hook_timed("consensus_dissent", || {
                    ljos_cli::converge::record_dissents(&id)
                })
            };
            if json {
                // One document on stdout. The push cite stays at the top
                // level; each reading is under its own key. A human tally
                // stays off stdout.
                let mut said = Vec::new();
                for step in steps.iter().chain(&panel) {
                    let captured = run_captured(step.bin, &step.args)?;
                    if !captured.stderr.is_empty() {
                        eprint!("{}", captured.stderr);
                    }
                    said.push((ljos_cli::consensus_step_label(step), captured.stdout));
                }
                let mut doc = ljos_cli::consensus_document(&gate_json, &said);
                doc["protected_push"] = protected_json;
                doc["families"] = serde_json::json!({"voices": families.len(), "of": families});
                if !provenance.is_empty() {
                    doc["provenance"] = serde_json::json!(provenance);
                }
                if !dissent.is_empty() {
                    doc["dissent"] = serde_json::json!(dissent);
                }
                println!("{}", serde_json::to_string_pretty(&doc)?);
                return Ok(());
            }
            print!(
                "{}",
                ljos_cli::consensus_verdict(&gate_json, &protected_json)
            );
            print!("{gate}");
            print!("{protected}");
            print!("{provenance}");
            for line in &dissent {
                println!("dissent: {line}");
            }
            println!("advice, which does not decide a push: the weighted settle over every ballot");
            if let Some(line) = discount {
                println!("{line}");
            }
            if let Some(line) = ljos_cli::slot::families_in_words(&families, have_model) {
                print!("{line}");
            }
            // The page a person reads is the tracker's weighted settle.
            // One ask, whether or not the model crate is installed.
            // The model crate's shares and influence stay on `--json`.
            // A stub or a refusal leaves that model reading.
            let tracker_in_steps = steps.iter().any(|step| step.bin == "vissue");
            let mut printed_tracker = false;
            for step in steps {
                let settle = step.bin == "ljos-consensus"
                    && step.args.first().map(String::as_str) == Some("settle");
                if step.bin == "vissue" {
                    // The tracker's settle text stays. A dissent it filed is
                    // named, rather than left as the create line on stderr.
                    let said = run_captured(step.bin, &step.args)?;
                    print!("{}", said.stdout);
                    for line in
                        ljos_cli::dissent_lines(&format!("{}\n{}", said.stdout, said.stderr))
                    {
                        println!("dissent: {line}");
                    }
                    printed_tracker = true;
                    continue;
                }
                if !settle {
                    run(step.bin, &step.args)?;
                    continue;
                }
                // The model settle still runs, so a stall is the same wait
                // the call deadline already names. The page a person reads
                // is the tracker's weighted settle when that command answers.
                let said = run_captured(step.bin, &step.args)?;
                if on_path("vissue") && !tracker_in_steps && !printed_tracker {
                    if let Ok(tracker) =
                        ljos_cli::consensus_steps_anchored(&id, false, true, &trust, &personas)
                    {
                        let mut page = String::new();
                        let mut answered = true;
                        for tracker_step in tracker {
                            match run_captured(tracker_step.bin, &tracker_step.args) {
                                Ok(tracker_said) => {
                                    page.push_str(&tracker_said.stdout);
                                    for line in ljos_cli::dissent_lines(&format!(
                                        "{}\n{}",
                                        tracker_said.stdout, tracker_said.stderr
                                    )) {
                                        page.push_str(&format!("dissent: {line}\n"));
                                    }
                                }
                                Err(_) => {
                                    answered = false;
                                    break;
                                }
                            }
                        }
                        // The tracker's own settle is the page a person reads.
                        // A stub or a refusal leaves the model crate's reading.
                        if answered && page.contains("social power") {
                            print!("{page}");
                            printed_tracker = true;
                            continue;
                        }
                    }
                }
                if printed_tracker || tracker_in_steps {
                    continue;
                }
                match ljos_cli::settle_in_words(&said.stdout, &anchors) {
                    Some(words) => print!("{words}"),
                    None => print!("{}", said.stdout),
                }
            }
            for step in panel {
                if step.args.first().map(String::as_str) != Some("reputation") {
                    run(step.bin, &step.args)?;
                    continue;
                }
                // The rows name every voter on the topic; beside this
                // settle only the issue's voters are read.
                let said = run_captured(step.bin, &step.args)?;
                let voters = ljos_cli::voters_of(&ballots);
                match ljos_cli::standing_in_words(&said.stdout, &voters) {
                    Some(words) => print!("{words}"),
                    None => print!("{}", said.stdout),
                }
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
            println!("{}", ljos_cli::atom_out(&v, false)?);
        }
        Cmd::Doctor { all, .. } => {
            // Rows go out as each check finishes. `--all` prints every row.
            // The short report prints the rows that need action as they
            // are known, then the ready line, which needs every row.
            // `doctor()` with no sink is what a JSON caller collects and
            // prints in one document.
            let rows = ljos_cli::doctor_stream(move |row| {
                if all || ljos_cli::doctor_shows_in_short(row) {
                    print!("{}", format_doctor(std::slice::from_ref(row)));
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                }
            });
            if !all {
                print!("{}", ljos_cli::doctor_summary(&rows));
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            if !healthy(&rows) {
                std::process::exit(1);
            }
        }
        Cmd::Hud {
            foreground,
            toggle,
            show,
            hide,
            install_desktop,
        } => {
            std::process::exit(ljos_cli::hud::run(ljos_cli::hud::HudLaunch {
                foreground,
                toggle,
                show,
                hide,
                install_desktop,
            }));
        }
        Cmd::Protocol { full } => {
            if full {
                print!("{}", protocol_text());
            } else {
                print!("{}", protocol_brief());
            }
        }
        Cmd::Seat { action } => match action {
            None => print!("{}", format_seat_command()),
            Some(SeatAction::Add { name }) => print!("{}", ljos_cli::seat_roster::add(&name)?),
            Some(SeatAction::Grant {
                seat,
                spec,
                ids_file,
                quote,
                at,
                via,
            }) => print!(
                "{}",
                ljos_cli::seat_roster::grant(
                    &seat,
                    spec.as_deref().unwrap_or(""),
                    &quote,
                    &at,
                    &via,
                    ids_file.as_deref(),
                )?
            ),
        },
        Cmd::Mcp { args } => return Err(ljos_cli::exec_server(&args)),
        Cmd::Onboard {
            harness,
            dry_run,
            example,
            skills,
        } => {
            if example {
                print!("{HARNESSES_EXAMPLE}");
                return Ok(());
            }
            // A pipe used to print the entry and skip the stores, so the
            // next `ljos file` had no tracker. The stores are set up
            // either way. The entry stays the only text on stdout.
            let piped = harness.is_none() && !std::io::stdout().is_terminal();
            let harness = harness.unwrap_or_else(|| "all".to_string());
            let steps = if harness == "all" {
                ljos_cli::onboard_detected(dry_run, skills.as_deref())?
            } else {
                onboard_to(&harness, dry_run, skills.as_deref())?
            };
            // `--harness json` prints the entry alone, so it parses as it stands.
            if harness == "json" {
                for s in &steps {
                    println!("{}", s.detail);
                }
                return Ok(());
            }
            let lines = format_steps(&steps);
            if piped {
                let entry = ljos_cli::server_entry()?;
                println!("{}", serde_json::to_string_pretty(&entry)?);
                eprint!("{lines}");
            } else {
                print!("{lines}");
            }
            if steps.iter().any(|s| !s.ok) {
                std::process::exit(1);
            }
        }
        Cmd::Offboard { runner, dry_run } => {
            let steps = ljos_cli::offboard::offboard(runner.as_deref(), dry_run)?;
            print!("{}", format_steps(&steps));
            if steps.iter().any(|step| !step.ok) {
                std::process::exit(1);
            }
        }
        Cmd::Uninstall {
            dry_run,
            keep_programs,
            all,
            purge,
            no_stub,
            bin_dir,
        } => {
            let steps = ljos_cli::uninstall::uninstall(&ljos_cli::uninstall::Options {
                dry: dry_run,
                keep_programs,
                all,
                purge,
                stub: !no_stub,
                bin_dir,
            })?;
            print!("{}", format_steps(&steps));
            if steps.iter().any(|step| !step.ok) {
                std::process::exit(1);
            }
        }
        Cmd::Handover {
            out,
            project,
            issue,
            to,
            since_size,
            scope,
        } => {
            for id in &issue {
                stop_ticket(id);
            }
            for line in handover_scoped(&out, &project, &issue, since_size, scope.as_deref())? {
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
        Cmd::Receive {
            dir,
            since,
            keep,
            import,
        } => {
            for line in receive(&dir, since.as_deref(), keep.as_deref(), import)? {
                println!("{line}");
            }
        }
        Cmd::Due { all, judge, json } => {
            if judge {
                print!("{}", ljos_cli::judge_due_page()?);
            } else if json {
                print!("{}", ljos_cli::due_json(all)?);
            } else {
                print!(
                    "{}",
                    ljos_cli::scan::for_reader(&due_report(all)?, human_color())
                );
            }
        }
        Cmd::JudgeScore => {
            print!("{}", ljos_cli::judge_score_report());
        }
        Cmd::BumpPlan {
            bundle,
            project,
            parent,
            generation,
            dry_run,
        } => {
            let (generation, rows) =
                bump_plan(&bundle, &project, &parent, generation.as_deref(), dry_run)?;
            print!("{}", format_bump_rows(&generation, &rows));
        }
        Cmd::Findings {
            state,
            remember,
            all,
            issue,
        } => {
            if remember || issue.is_some() {
                print!(
                    "{}",
                    format_remembered(&remember_findings(&state, issue.as_deref(), all)?)
                );
            } else {
                print!("{}", format_findings(&read_campaign(&state)?));
            }
        }
        Cmd::Habit {
            name,
            value,
            unit,
            every,
            source,
        } => match (name, value) {
            (Some(name), Some(value)) => {
                let every_s = parse_every(&every)?;
                let (body, prev) = habit(&name, value, &unit, every_s, &source)?;
                println!("{}", write_ack(&body, false));
                if let Some(p) = prev {
                    eprintln!(
                        "was {}{}{} ({}), now closed",
                        trim_num(p.value),
                        if p.unit.is_empty() { "" } else { " " },
                        p.unit,
                        age_of(p.ts.as_deref(), &now_utc())
                    );
                }
            }
            (None, Some(_)) => {
                anyhow::bail!("habit: a reading needs a name; `ljos habit NAME VALUE`")
            }
            (name, None) => {
                let now = now_utc();
                let rows: Vec<Reading> = habits()?
                    .into_iter()
                    .filter(|r| name.as_deref().is_none_or(|n| r.name == n.trim()))
                    .collect();
                if rows.is_empty() {
                    println!(
                        "no readings{}; `ljos habit NAME VALUE` takes the first",
                        name.map(|n| format!(" of {n}")).unwrap_or_default()
                    );
                } else {
                    print!("{}", format_readings(&rows, &now));
                }
            }
        },
        Cmd::Sitting {
            issue,
            plan,
            top,
            project,
            items,
            assignee,
            cards: cards_dir,
            anyway,
            playbook,
            verbose,
            json,
        } => {
            if plan.is_none() && project.is_some() {
                eprintln!("{}", sitting_project_line());
                std::process::exit(USAGE_STATUS);
            }
            // A sitting on an unknown id names that id. Onboard would hide it.
            let mut memories = Vec::new();
            let mut full = String::new();
            let mut shown = String::new();
            let mut opened_next = None;
            let detail = git_detail(verbose);
            let issue = if let Some(title) = plan {
                let opened = ljos_cli::plan::open_plan(project.as_deref(), &title, &items, top)?;
                let quiet_plan = ljos_cli::without_routine_git(&opened.text);
                opened_next = quiet_plan
                    .lines()
                    .find(|line| line.starts_with("next:"))
                    .map(str::to_string);
                if json {
                    full.push_str(&opened.text);
                    if !detail {
                        shown.push_str(&quiet_plan);
                    }
                } else {
                    print!("{}", if detail { &opened.text } else { &quiet_plan });
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                    if detail {
                        println!("sitting {}: checking blockers", opened.parent);
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                    }
                }
                opened.parent
            } else {
                let issue = issue.context("sitting: pass an issue, or --plan TITLE -- ITEM...")?;
                ljos_cli::on_disk(&issue)
            };
            ljos_cli::begin_quiet_sitting();
            let assignee = assignee_for("sitting", assignee.as_deref(), false)?;
            let printed = sitting_stream_started(
                &issue,
                &assignee,
                &cards_dir,
                anyway,
                playbook.as_deref(),
                sitting_budget
                    .take()
                    .expect("a sitting starts its claim budget before setup"),
                |piece| {
                    let piece = if let Some(next) = opened_next.as_deref() {
                        piece
                            .split_inclusive('\n')
                            .filter(|line| line.trim_end_matches('\n') != next)
                            .collect::<String>()
                    } else {
                        piece.to_string()
                    };
                    if json {
                        full.push_str(&piece);
                        if !detail {
                            shown.push_str(&compact_sitting_block(&piece, &mut memories));
                        }
                    } else if detail {
                        print!("{piece}");
                    } else {
                        print!("{}", compact_sitting_block(&piece, &mut memories));
                    }
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                },
            );
            // A refused sitting used to fall through a gate path that
            // exits 0. The refusal is the answer, and it is not success.
            if let Err(e) = printed {
                let line = ljos_cli::user_fault_line(&e);
                eprintln!("{line}");
                if ljos_cli::fault_detail() {
                    let detail = format!("{e:#}");
                    if ljos_cli::fault_detail_worth_showing(&detail, &line) {
                        eprintln!("{detail}");
                    }
                }
                std::process::exit(ljos_cli::user_fault_exit(&e, &line));
            }
            if !detail {
                let mut tail = String::new();
                if !memories.is_empty() {
                    tail.push_str("What the seat remembers for this task:\n");
                    for memory in &memories {
                        tail.push_str(&format!("- {memory}\n"));
                    }
                }
                tail.push_str(&sitting_close(&issue));
                if json {
                    shown.push_str(&tail);
                } else {
                    print!("{tail}");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                }
            }
            if json {
                let text = if detail { &full } else { &shown };
                print!("{}", ljos_cli::format_rendered_json(text, &full, None));
            }
        }
        Cmd::Finish {
            issue,
            status,
            lesson,
            lesson_text,
            outcome,
            new_option,
            beta,
            gen,
            assignee,
            close,
            verbose,
            json,
            quiet,
        } => {
            let (issue, inferred) = match issue {
                Some(raw) => (raw, false),
                None => {
                    let Some(open) = ljos_cli::held_issue() else {
                        if close {
                            bail!("{}", ljos_cli::NO_OPEN_SITTING_TO_CLOSE);
                        }
                        bail!("{}", ljos_cli::NO_OPEN_SITTING);
                    };
                    (open, true)
                }
            };
            stop_ticket(&issue);
            {
                let _deadline =
                    ljos_cli::pack_link::CommandDeadline::enter(std::time::Duration::from_secs(2));
                let _clock =
                    ljos_cli::pack_link::PackClock::enter(std::time::Duration::from_secs(2));
                let _quiet = ljos_cli::pack_link::QuietPack::enter();
                match ljos_cli::flush_note_queue() {
                    Ok(done) => print!("{}", ljos_cli::note_queue::drained_lines(&done)),
                    Err(e) => eprintln!("ljos finish: note queue: {e:#}"),
                }
            }
            let said = finish(
                &issue,
                &status,
                lesson.or(lesson_text).as_deref(),
                outcome.as_deref(),
                new_option,
                beta,
                &assignee_for("finish", assignee.as_deref(), false)?,
                gen,
                close,
                verbose,
            )
            .map_err(ticket_error)?;
            warn_other_sitting(&issue, quiet, inferred, true);
            print!("{}", render_write(&said, git_detail(verbose), json, None));
        }
        Cmd::Reopen { issue } => {
            print!("{}", reopen(&issue)?);
        }
        Cmd::Withdraw {
            issue,
            note,
            as_persona,
        } => {
            stop_ticket(&issue);
            match note {
                Some(text) => {
                    let seat = ljos_cli::seat_name();
                    print!("{}", ljos_cli::undo::withdraw_note(&issue, &seat, &text)?);
                }
                None => {
                    let who = ljos_cli::ballot_voter("withdraw", as_persona.as_deref())?;
                    print!("{}", ljos_cli::undo::withdraw_ballot(&issue, &who)?);
                }
            }
        }
        Cmd::Calibrate { project, rounds } => {
            let rows = calibrate(&project, rounds)?;
            for row in &rows {
                println!("{} weighs {} at {:.3}", row.from, row.to, row.weight);
            }
        }

        Cmd::Graded { id, lapsed } => {
            let v = graded(&id, !lapsed)?;
            let due = v["due_at"].as_str().unwrap_or("graded");
            if std::io::stdout().is_terminal() {
                println!("graded; next review {}", ljos_cli::local_when(due));
            } else {
                println!("{due}");
            }
        }
        Cmd::Learn {
            id,
            outcome,
            new_option,
            beta,
            share,
            rule,
        } => {
            stop_ticket(&id);
            let forecasts = forecasts_from_json(&ljos_cli::raw_ballot_json(&id)?)?;
            let (outcome, off) = ljos_cli::named_outcome(
                &id,
                &outcome,
                &forecasts,
                &ljos_cli::options_of_issue(&id),
                new_option,
            )?;
            if let Some(line) = off {
                print!("{line}");
            }
            let ballots: Vec<(String, String)> = forecasts
                .iter()
                .map(|f| (f.agent.clone(), f.choice.clone()))
                .collect();
            // The rows written are scoped to what the issue's island is
            // about, so a voter wrong here keeps its standing elsewhere.
            let about = island_entities(&id).unwrap_or_default();
            let (rows, moved, calibration) = if rule == "hedge" {
                let rows =
                    learn_shared(&ballots, &outcome, &trust_from_pack()?, beta, &about, share)?;
                let moved = learn_anchors(&personas_from_pack()?, &ballots, &outcome, beta);
                for row in &rows {
                    write_trust(row, &[])?;
                }
                for p in &moved {
                    write_persona_as(p, PersonaWrite::Learned)?;
                }
                write_outcome(&id, &outcome)?;
                (rows, moved, std::collections::BTreeMap::new())
            } else {
                learn_and_write(&id, &ballots, &outcome, beta, &about, &forecasts)?
            };
            println!(
                "{}",
                learn_reading(rows.len(), moved.len(), &forecasts, &outcome, &calibration)
            );
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
    let Ok(v) = tracker_show_json(id) else {
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

/// The verb of a command that writes the tracker under this seat. Listing
/// a deed's citations, a workflow's status, a consensus read with
/// `--no-file`, a dry-run reap and a dry-run bump plan are reads; a slot
/// names its seat on the line.
fn tracker_write(cmd: &Cmd) -> Option<&'static str> {
    Some(match cmd {
        Cmd::Sitting { .. } => "sitting",
        Cmd::Claim { .. } => "claim",
        Cmd::Note { .. } => "note",
        Cmd::Finish { .. } => "finish",
        Cmd::Reopen { .. } => "reopen",
        Cmd::Withdraw { .. } => "withdraw",
        Cmd::Release { stale: true, .. } => return None,
        Cmd::Release { .. } => "release",
        Cmd::Complete { .. } => "complete",
        Cmd::File { .. } => "file",
        Cmd::Options { .. } => "options",
        Cmd::Deed { add, remove, .. } if !add.is_empty() || !remove.is_empty() => "deed",
        Cmd::Table { add: Some(_), .. } => "table",
        Cmd::Workflow { start, round, .. } if start.is_some() || *round => "workflow",
        Cmd::Consensus {
            no_file, deadline, ..
        } if !no_file || deadline.is_some() => "consensus",
        Cmd::Playbook { .. } => "playbook",
        Cmd::Reap { apply: true, .. } => "reap",
        Cmd::Vote {
            choice,
            jev,
            withdraw,
            ..
        } if choice.is_some() || *jev || *withdraw => "vote",
        Cmd::Predict { .. } => "predict",
        Cmd::Slot { .. } => "slot",
        Cmd::Findings {
            remember, issue, ..
        } if *remember || issue.is_some() => "findings",
        Cmd::Panel { jev: true, .. } => "panel",
        Cmd::OpenPanel { .. } => "open-panel",
        Cmd::BumpPlan { dry_run: false, .. } => "bump-plan",
        Cmd::Block { .. } => "block",
        Cmd::Parent { .. } => "parent",
        _ => return None,
    })
}

/// The quorum gate on `id`. Met, the reading goes on. Short, it says so
/// and exits 1; short past the deadline, the tracker notes the lapse once
/// and this exits 3. The count is the ballots on the tracker. The published
/// `vissue consensus` has no `--quorum` flag, so this does not ask it for one.
fn quorum_or_exit(id: &str, n: usize, deadline: Option<&str>, json: bool) -> Result<()> {
    let gate = ljos_cli::reap::gate_quorum(id, n, deadline)?;
    match gate {
        ljos_cli::reap::QuorumGate::Met { .. } => Ok(()),
        ljos_cli::reap::QuorumGate::Short { have, needed } => {
            let line = format!("{have} of {needed} needed for a quorum");
            if json {
                println!(
                    "{}",
                    serde_json::json!({"quorum": {"issue": id, "needed": n, "lapsed": false, "said": line}})
                );
            } else {
                println!("quorum: short, nothing is decided yet: {line}");
            }
            std::process::exit(1);
        }
        ljos_cli::reap::QuorumGate::Lapsed { have, needed } => {
            let line = format!("quorum timed out: {have} of {needed}");
            if json {
                println!(
                    "{}",
                    serde_json::json!({"quorum": {"issue": id, "needed": n, "lapsed": true, "said": line}})
                );
            } else {
                println!("quorum: lapsed, nothing was decided: {line}");
            }
            std::process::exit(3);
        }
    }
}

/// `ljos hook --prompt --text`: the same prompt hook, run as its own
/// process so it books and marks mail exactly as a runner's call does,
/// then its answer as the plain text the agent would read.
fn prompt_as_text(limit: usize) -> Result<()> {
    use std::io::{Read, Write};
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .args(["hook", "--prompt", "--limit", &limit.to_string()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    let said = ljos_cli::hook_text(&String::from_utf8_lossy(&out.stdout));
    if said.is_empty() {
        println!("Nothing remembered bears on this.");
    } else {
        print!("{}", ljos_cli::scan::for_reader(&said, human_color()));
    }
    if !out.status.success() {
        std::process::exit(out.status.code().unwrap_or(1));
    }
    Ok(())
}

#[cfg(test)]
mod help_list {
    use super::*;

    fn command_lines(help: &str) -> Vec<&str> {
        help.lines()
            .filter_map(|line| {
                let rest = line.strip_prefix("  ")?;
                if rest.starts_with('-') || rest.starts_with(' ') {
                    return None;
                }
                rest.split_whitespace().next()
            })
            .collect()
    }

    #[test]
    fn every_command_is_in_one_group() {
        // A test thread's stack is 2 MiB. Building this command tree needs
        // the 8 MiB a normal `ljos` process gets.
        let ran = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(every_command_is_in_one_group_inner)
            .expect("spawn");
        ran.join().expect("the help check panicked");
    }

    fn every_command_is_in_one_group_inner() {
        let cmd = Cli::command();
        help_groups::groups_match(&cmd).unwrap_or_else(|err| panic!("{err}"));
        let short = help_groups::render(&cmd, false);
        let all = help_groups::render(&cmd, true);
        for heading in [
            "Start:",
            "Daily:",
            "Tracker:",
            "Memory:",
            "Coordination:",
            "Admin:",
        ] {
            assert!(short.contains(heading), "{heading} missing\n{short}");
        }
        let start = short.find("Start:").expect("Start");
        let daily = short.find("Daily:").expect("Daily");
        assert!(start < daily);
        let first = command_lines(&short[start..daily]);
        assert_eq!(
            first,
            [
                "doctor", "onboard", "protocol", "prefer", "remember", "search", "sitting",
                "finish", "due", "help",
            ]
        );
        let admin = short.find("Admin:").expect("Admin");
        assert_eq!(
            command_lines(&short[admin..]).first().copied(),
            Some("policy")
        );
        for name in ["merge-org", "hook", "bump-plan", "retire-notes", "slot"] {
            assert!(
                !command_lines(&short).contains(&name),
                "{name} is on the short list"
            );
            assert!(
                command_lines(&all).contains(&name),
                "{name} is missing from help --all"
            );
        }
        assert!(short.contains("`ljos help --all` lists every command."));
        assert_eq!(
            cmd.get_about().map(|text| text.to_string()).as_deref(),
            Some(ABOUT_LINE)
        );
    }
}

#[cfg(test)]
mod usage_hint {
    use super::*;

    #[test]
    fn statusline_at_a_terminal_names_the_pipe() {
        assert_eq!(
            statusline_tty_hint(),
            "ljos: statusline reads the runner's status JSON on stdin, so pipe that JSON to it."
        );
    }

    #[test]
    fn edits_count_a_missing_letter_and_a_swap() {
        assert_eq!(edits("serch", "search"), 1);
        assert_eq!(edits("serach", "search"), 2);
        assert_eq!(edits("finsh", "finish"), 1);
        // Two neighbouring letters swapped is two edits, and a four-letter
        // flag still accepts that.
        assert_eq!(edits("josn", "json"), 2);
        assert_eq!(edits("lesons", "lesson"), 2);
        assert_eq!(edits("limt", "limit"), 1);
        assert_eq!(edits("find", "file"), 2);
    }

    #[test]
    fn a_one_letter_flag_is_not_guessed() {
        let flags = vec!["json".into(), "limit".into(), "n".into(), "h".into()];
        assert_eq!(closest("josn", &flags), Some("json"));
        assert_eq!(closest("limt", &flags), Some("limit"));
        assert_eq!(closest("x", &flags), None);
        assert_eq!(closest("n", &flags), None);
    }

    #[test]
    fn find_add_and_note_add_name_real_commands() {
        assert_eq!(root_synonym("find"), Some("search"));
        assert_eq!(root_synonym("FIND"), Some("search"));
        assert_eq!(root_synonym("add"), Some("remember"));
        assert_eq!(root_synonym("save"), Some("remember"));
        assert_eq!(root_synonym("store"), Some("remember"));
        assert_eq!(root_synonym("note-add"), Some("note"));
        assert_eq!(root_synonym("add-note"), Some("note"));
        assert!(root_synonym("serch").is_none());
        assert!(root_synonym("finsh").is_none());
        let here = words_at(&[]);
        for word in [
            "find", "query", "lookup", "add", "save", "store", "note-add", "add-note",
        ] {
            let real = root_synonym(word).unwrap();
            assert!(
                here.commands.iter().any(|name| name == real),
                "{word} -> {real} is not a command"
            );
        }
        assert_eq!(closest("finsh", &here.commands), Some("finish"));
        assert_eq!(closest("serch", &here.commands), Some("search"));
        let search = words_at(&["search".to_string()]);
        let bare = flag_names(&search.flags);
        assert_eq!(closest("josn", &bare), Some("json"));
        assert_eq!(closest("x", &bare), None);
    }

    #[test]
    fn a_verb_with_an_ending_or_a_start_is_picked() {
        let here = words_at(&[]);
        let pick = |word: &str| pick_verb(word, &here.commands);
        assert_eq!(pick("prop").as_deref(), Some("proposals"));
        assert_eq!(pick("sear").as_deref(), Some("search"));
        assert_eq!(pick("remembering").as_deref(), Some("remember"));
        assert_eq!(pick("searches").as_deref(), Some("search"));
        assert_eq!(pick("search-lessons").as_deref(), Some("search"));
        assert_eq!(pick("note_add").as_deref(), Some("note"));
        assert_eq!(pick("serch").as_deref(), Some("search"));
        assert_eq!(pick("mem"), None);
        assert_eq!(pick("zzzzzz"), None);
    }

    #[test]
    fn lessons_words_are_a_topic_and_other_words_are_not() {
        for word in ["lessons", "Lesson", "learnings", "LORE"] {
            assert!(lessons_topic(word), "{word}");
        }
        for word in ["learn", "search", "remember", "lessen"] {
            assert!(!lessons_topic(word), "{word}");
        }
        let args: Vec<String> = ["lessons", "kiln", "-n", "3"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let line = lessons_sentence("lessons", &args);
        assert!(line.contains("`ljos search \"kiln\"`"), "{line}");
        assert!(line.contains("`ljos remember "), "{line}");
        assert!(line.contains("`ljos proposals`"), "{line}");
    }
}
