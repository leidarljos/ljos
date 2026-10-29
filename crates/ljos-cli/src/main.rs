//! `ljos`: one seat over the habitats. Each habitat keeps its own crate.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use ljos_cli::{
    age_of, brief, bump_plan, calibrate, cards, claim, claim_next, cli_assignee, compact_sitting,
    complete, conflicts, consensus_steps_for, copy_playbook, doctor, due_report, finish,
    forecasts_from_json, format_bump_rows, format_consolidation, format_doctor,
    format_doctor_short, format_findings, format_hits, format_hubs, format_island,
    format_playbooks, format_readings, format_remembered, format_seat_command, format_steps,
    format_write_ack, graded, habit, habits, handover, healthy, hook_note, island_entities, join,
    learn_anchors, learn_and_write, learn_reading, learn_shared, mark_seen, now_utc, on_path,
    onboard_to, pack, packset_consolidate, packset_forget, packset_hubs, packset_island_as,
    packset_search_as_of, packset_write_as, panel, panel_steps, parse_every, personas_from_pack,
    playbooks_from_pack, policy_with_memory, post_hook_stdout, predictions_of, prompt_hook_stdout,
    protocol_text, read_campaign, receive, release, remember_findings, reopen, rows_about, run,
    run_as, run_captured, session_end, settle_discount, sitting_gated, stop_hook_stdout, timeline,
    topic_words, tracker_show_json, trim_num, trust_from_pack, uncite_deed, with_discount,
    withdraw_prediction, write_outcome, write_persona_as, write_prediction, write_rule,
    write_trust, Persona, PersonaWrite, Reading, Rule, Trust, HARNESSES_EXAMPLE, LEARN_BETA,
};
use std::io::IsTerminal;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ljos",
    version,
    about = "Memory, a work tracker and a command guard for coding agents",
    after_help = "Start with `ljos doctor`, `ljos prefer \"...\"`, `ljos policy -- COMMAND` and \
                  `ljos onboard`. `ljos protocol` explains the rest."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum MemCmd {
    /// List memories newest first with their provenance. Unreviewed agents' memories by default.
    List {
        #[command(flatten)]
        f: MemFilter,
        #[arg(long)]
        json: bool,
    },
    /// Review unreviewed memories: interactive on a terminal; with --keep/--drop/--edit/--merge, for scripts.
    Review {
        #[command(flatten)]
        f: MemFilter,
        #[arg(long)]
        json: bool,
        /// Keep this memory as it stands. Repeatable.
        #[arg(long, value_name = "ID")]
        keep: Vec<String>,
        /// Drop this memory (the pack tombstones it). Repeatable.
        #[arg(long = "drop", value_name = "ID")]
        drop_ids: Vec<String>,
        /// Rewrite this memory with --text.
        #[arg(long, value_name = "ID")]
        edit: Option<String>,
        /// Merge these memories into one, with --text or their texts joined. Repeatable, two or more.
        #[arg(long, value_name = "ID")]
        merge: Vec<String>,
        #[arg(long)]
        text: Option<String>,
    },
    /// Browse, filter by seat or date, and prune memories in a terminal viewer.
    Tui {
        #[command(flatten)]
        f: MemFilter,
    },
    /// Name duplicates and never-recalled memories. A dry run unless --apply.
    Prune {
        /// Duplicates and memories never recalled in --days.
        #[arg(long)]
        stale: bool,
        #[arg(long, default_value_t = 30)]
        days: i64,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Args, Clone)]
struct MemFilter {
    /// Only this seat's.
    #[arg(long)]
    seat: Option<String>,
    /// Only those written on or after YYYY-MM-DD.
    #[arg(long)]
    since: Option<String>,
    /// Reviewed ones too.
    #[arg(long)]
    all: bool,
    /// The person's own claims too.
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
            } else if shown.is_empty() {
                println!("no memories to review");
            } else {
                for m in &shown {
                    print!("{}", mem::describe(m));
                }
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
    /// Install the release binaries, write the harness MCP command, and clone `LJOS_TRACKER_REMOTE`.
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
enum RecordCmd {
    /// Validate a directory of schema-v1 record entries, or with `--tracker` every ballot on a tracker.
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
    /// Add NAME to the seat list in the tracker's vissue.toml.
    Add {
        /// The seat to add.
        name: String,
    },
    /// Record the owner's yes that a seat may close tickets it filed or holds.
    Grant {
        /// The seat the grant names.
        seat: String,
        /// The grant, written close=own.
        spec: String,
        /// The owner's words.
        #[arg(long)]
        quote: String,
        /// When the owner said it, as RFC 3339.
        #[arg(long)]
        at: String,
        /// The seat writing the grant down. Not the seat the grant names.
        #[arg(long)]
        via: String,
    },
}

#[derive(Subcommand)]
enum Cmd {
    /// Remember one lesson. Typed by the person it takes effect at once.
    /// Under a runner, words from the person's latest prompt are theirs.
    /// Any other text is an unreviewed note, which the prompt does not treat as a fact.
    Remember {
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
        /// The day (YYYY-MM-DD) the claim starts to hold in the world. `ljos search --valid-at` leaves it out before then.
        #[arg(long, value_name = "DATE")]
        valid_from: Option<String>,
        /// The last day (YYYY-MM-DD) the claim holds. A plain search from the next day on leaves it out; `--valid-at` asks about another day.
        #[arg(long, value_name = "DATE")]
        valid_to: Option<String>,
    },
    /// Prefer one way over another, as a standing preference. Typed by the
    /// person, it is theirs. Under a runner, only words from the person's
    /// latest prompt are theirs; any other text is agent-derived.
    /// `--user` writes a user rule instead: shared by every seat, never reviewed.
    Prefer {
        text: Vec<String>,
        /// Prefer as this persona.
        #[arg(long = "as")]
        as_persona: Option<String>,
        /// Also write a deny rule over this command pattern, with the preference as its reason, so the hook refuses what the preference forbids. Repeatable.
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
        /// Promote this lesson id into a user rule. The lesson stays in the pack. Pass `--user`.
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
        /// list user rules stored before one. With `--keep`, keep one as
        /// the person's, from a terminal with no runner.
        #[arg(long)]
        review: bool,
        /// Keep this older preference, or with `--user` this older user
        /// rule, as the person's. Pass with `--review`.
        #[arg(long, value_name = "ID", requires = "review")]
        keep: Option<String>,
    },
    /// Review the memories agents wrote on their own: list, keep, edit, merge, drop, prune, or browse them in a terminal viewer.
    Memories {
        #[command(subcommand)]
        cmd: MemCmd,
    },
    /// Write one agent proposal into the pack as a standing lesson. Under an agent runner, only a proposal another conversation filed, such as a worker's.
    Accept { id: String },
    /// Retire one memory by id. The record stays, marked retired.
    Forget {
        id: String,
        /// The deed accession that withdrew the claim. Refused if it is not one.
        #[arg(long)]
        why: Option<String>,
        /// Print the atom JSON. The default is one line: id, kind, due, text.
        #[arg(long)]
        json: bool,
    },
    /// What ljos remembers about a topic, best match first. Empty means nothing is remembered on it.
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
        /// What held in the world on this day (YYYY-MM-DD): claims whose validity window excludes it are left out. Without it, today; `--valid-at any` keeps every window.
        #[arg(long, value_name = "DATE")]
        valid_at: Option<String>,
        /// Open with what the score and the fraction mean, and what an empty answer means.
        #[arg(long)]
        explain: bool,
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
    /// Share the seat's memory across machines through the tracker repository: pull and take other machines' sealed logs, then write and push this machine's.
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
    },
    /// Re-export this machine's atoms over a conflicted tracker log. git calls
    /// this as the `ljos-atoms` merge driver (`%A` then `%P`).
    MergeAtoms {
        /// The file git keeps (`%A`).
        current: PathBuf,
        /// The path in the tracker (`%P`).
        path: String,
    },
    /// Field-merge one issue org file. git calls this as the `ljos-org`
    /// merge driver (`%O` `%A` `%B`). The merged text replaces `ours`.
    #[command(hide = true)]
    MergeOrg {
        /// The common ancestor (`%O`).
        base: PathBuf,
        /// The file git keeps (`%A`).
        ours: PathBuf,
        /// The other side (`%B`).
        theirs: PathBuf,
    },
    /// Put this machine on the shared tracker.
    Cloud {
        #[command(subcommand)]
        cmd: CloudCmd,
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
        /// Walk it as this persona: its own weights on the way in, and a fire writes its weights, not the seat's.
        #[arg(long = "as")]
        as_persona: Option<String>,
    },
    /// Whether a deed's bytes are intact and its sources are too. `at=` is
    /// the produced time in ISO 8601 with this machine's offset.
    /// `record -- COMMAND` runs the repo's test command and stores a passing
    /// record for the current tip, which `git push` requires.
    Evidence {
        /// A deed accession, or `record` followed by the test command.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true, num_args = 1..)]
        args: Vec<String>,
    },
    /// Record entry helpers.
    Record {
        #[command(subcommand)]
        cmd: RecordCmd,
    },
    /// Whether a deed is still the tip, or a later take superseded it.
    Current { accession: String },
    /// Cite a deed on an issue, drop one, or list what it cites. Citing a deed names it; the bytes stay in deedar.
    Deed {
        /// The issue: cited on, listed, or dropped from.
        #[arg(required_unless_present = "ticket")]
        issue: Option<String>,
        /// An accession to cite, from `deedar create`; repeat for several.
        #[arg(long, conflicts_with = "remove")]
        add: Vec<String>,
        /// An accession to drop. Repeat for several. The removal is appended.
        #[arg(long)]
        remove: Vec<String>,
        /// The ticket `--remove` drops the accession from, in place of the positional issue.
        #[arg(long, conflicts_with = "issue")]
        ticket: Option<String>,
    },
    /// File work found while sitting: a child of the issue this
    /// conversation holds unless `--parent` names another or `--top` none,
    /// in the parent's project. `-p` naming another project files at its
    /// top level, for a bug in a tool found mid-sitting. Prints the new id.
    File {
        /// The issue's title.
        title: String,
        /// The parent issue; the held issue when omitted.
        #[arg(long)]
        parent: Option<String>,
        /// No parent, even while an issue is held.
        #[arg(long, conflicts_with = "parent")]
        top: bool,
        /// The project; the parent's when omitted. A `--parent` files in
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
        /// A, B or C.
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
    },
    /// Append `Options: A, B` to an issue. Two or more, separated by commas.
    Options {
        /// The issue id.
        issue: String,
        /// The options, separated by commas.
        options: String,
    },
    /// Open a panel for a decision prompt: file or reuse the issue, sit it, and start one headless member per brief.
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
    Note {
        issue: String,
        /// The note; several words are one note.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// Tracker commits by one seat. The subject is `chore(issues): ID VERB by SEAT`.
    Log {
        /// The seat whose commits to list.
        #[arg(long)]
        seat: String,
    },
    /// How many tracker commits each seat made since a day.
    Usage {
        /// YYYY-MM-DD. Commits from that day on.
        #[arg(long)]
        since: String,
    },
    /// Write to one seat (`SEAT TEXT...`), or to a group with `--group NAME`.
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
        /// Write to a seat this pack has not heard from yet. Without it, a
        /// seat the pack does not know is refused, with the seats it knows.
        #[arg(long)]
        new_seat: bool,
    },
    /// Mail this seat has not read. Listing does not write a receipt.
    /// `ljos hook --prompt` writes a receipt for each message it shows.
    /// A pack that does not answer is an error.
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
    Read {
        /// The id `send` printed.
        id: String,
    },
    /// Answer a message. The reply goes to its sender and keeps its issue.
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
    /// A group of seats. With `--add` or `--remove`, change it; alone, list it.
    Group {
        name: String,
        /// A seat to add. Repeat for several.
        #[arg(long)]
        add: Vec<String>,
        /// A seat to drop. Repeat for several.
        #[arg(long)]
        remove: Vec<String>,
    },
    /// How mail wakes a seat that is idle: a command this machine runs or a
    /// webhook it posts to, which `ljos send` and `ljos reply` start without
    /// waiting. A name ending in `-` matches every longer seat that starts
    /// with it. Alone, show the seat's wake.
    Wake {
        /// The seat. Absent: this seat. A name ending in `-` is a prefix:
        /// mail to a longer seat that starts with it uses this wake unless
        /// that seat has its own. The longest prefix wins.
        seat: Option<String>,
        /// Run this shell line, with `LJOS_SEAT` set to the seat, when mail
        /// reaches it. The guard judges it now and on every run.
        #[arg(long, conflicts_with_all = ["webhook", "clear", "fire"])]
        command: Option<String>,
        /// POST the message's envelope (never its body) here, such as a Grok
        /// Bot routine's webhook. https, or http on localhost, an IPv4
        /// loopback address, or ::1.
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
    /// Who holds what on the tracker: `vissue claims`, with its flags.
    Claims {
        /// Passed to `vissue claims` as given (`--by NAME`, `--json`, `-p PROJECT`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// The working set for an issue: plan, its inputs' deeds, its own citations.
    Recall { issue: String },
    /// Release claims idle past a window and mail each seat. A dry run without --apply.
    Reap {
        /// How long a claim may sit with no claim or tracker commit: 6h, 90m, 2d.
        #[arg(long, default_value = "6h")]
        idle: String,
        /// Release and mail. Absent: print what would happen.
        #[arg(long)]
        apply: bool,
    },
    /// Cast this identity's ballot (VISSUE_AGENT), or read the tally with no --for.
    Vote {
        issue: String,
        /// The option to vote for.
        #[arg(long = "for")]
        choice: Option<String>,
        /// Probability in (0, 1] that the choice is the outcome. Recorded;
        /// `ljos learn` scores it once the outcome is named, and no settle
        /// or push cite weighs it.
        #[arg(long)]
        confidence: Option<f64>,
        /// Deed accessions this ballot used, comma-separated, or `none`.
        /// Required when `--for` is set.
        #[arg(long)]
        used: Option<String>,
        /// The option you expect the others to pick, or a JSON object of
        /// option to share. Same command as the ballot, so the surprisingly
        /// popular reading has a forecast without a second verb.
        #[arg(long)]
        expect: Option<String>,
        /// This seat's own name. A ballot or a withdrawal `--as` another
        /// seat is refused: a persona votes from a session seated as it.
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
        #[arg(long, conflicts_with_all = ["choice", "jev", "confidence", "used", "expect"])]
        withdraw: bool,
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
    /// A panel without MCP: one brief per persona, `run` to spawn each designer and persona in its own harness session, or `--cloud` for one prompt file per persona.
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
        /// `cursor-agent` or `claude`. Overrides `LJOS_PANEL_HARNESS`.
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
    /// A voter with a view: NAME holds its ballot by ANCHOR in [0, 1] (0 never moves).
    Persona {
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
    /// Install a published release's binaries beside this ljos: fetched from the release, checked against its sha256 and its version, the old ones kept.
    Upgrade {
        /// The release version; the newest when absent.
        version: Option<String>,
        /// Install here instead of beside the running ljos.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Classify one harness exit under the restart policy, and print the
    /// herdr line that would start the child again. This does not start it.
    Supervise {
        /// The child's name. The herdr agent is `persona-NAME`.
        name: String,
        /// The harness this child runs.
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
    Ask {
        name: String,
        /// What to ask, in words.
        text: Vec<String>,
    },
    /// The personas the pack holds: name, anchor, domains, view, and the seat that defined each, one per line.
    Personas,
    /// Write every persona as an agent definition in each runner's agents
    /// directory. A runner starts it as a subagent that casts that
    /// persona's ballot before it reads the others.
    Agents {
        /// One runner, from harnesses.toml or the shipped shapes; every runner in harnesses.toml with an agents directory when absent.
        #[arg(long)]
        harness: Option<String>,
        /// Report what would be written and removed, and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// One line for a runner's status bar (Grok's [ui.status_line], Claude Code's statusLine): the seat, the issue held, what is due. Reads the runner's status JSON on stdin.
    Statusline,
    /// Bind a playbook to an issue and copy its recipe into the working set, before personas enter.
    Playbook {
        /// The tracker id of the issue.
        issue: String,
        /// The recipe name: sit, arena, land, company-panel, overnight, feature.
        name: String,
    },
    /// The playbooks the pack holds: the closed set sit, arena, land, company-panel, overnight, feature.
    Playbooks,
    /// A parent issue's workflow: where it stands and the next step. `--start` puts the parent under a workflow and files its design child; `--round` files the next panel round or a blocking fix. A round count does not close the parent.
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
    /// Take a session node for an issue. One live claim per assignee.
    Claim {
        /// A tracker id, or a 32-hex claim-graph id. Omit when claiming next.
        #[arg(default_value = "")]
        node: String,
        /// Your name; mapped to one actor id. Absent: this conversation's holder (`ljos seat`).
        #[arg(long)]
        assignee: Option<String>,
        /// Claim the first ready node in the claim graph's order for this name.
        #[arg(long)]
        next: bool,
        /// Desired role affinity when claiming next work ('explore', 'architect', 'implementor', 'verifier', 'orchestrator', 'general').
        #[arg(long)]
        role: Option<String>,
        /// Critical depth slack for candidate dispersion.
        #[arg(long)]
        slack: Option<usize>,
    },
    /// Hand a session node back unfinished: ready again, generation moved.
    Release {
        /// A tracker id, or a 32-hex claim-graph id.
        node: String,
        /// The name that holds it. Absent: this conversation's holder (`ljos seat`).
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
        /// Generation from the sitting's claim. Absent: the live one. A stale gen is refused.
        #[arg(long)]
        gen: Option<u64>,
        /// The name that holds it. Absent: this conversation's holder (`ljos seat`).
        #[arg(long)]
        assignee: Option<String>,
    },
    /// The notes the person froze. Read-only.
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
        /// This seat's own name; another seat is refused.
        #[arg(long = "as")]
        as_persona: Option<String>,
    },
    /// Add a command guard rule: a glob over the command line and a verdict (deny or ask).
    /// `--user` keeps it in the user config, shared by every agent and never reviewed.
    Rule {
        /// A glob over the whole command line: `rm -rf *`, `*--force*`, `git push --force*`.
        pattern: String,
        /// deny stops the action; ask hands it to the person.
        #[arg(long, default_value = "ask")]
        verdict: String,
        /// The reason a stopped reader sees.
        #[arg(long)]
        why: String,
        /// Print the atom JSON. The default is one line: id, kind, due, text.
        #[arg(long)]
        json: bool,
        /// Write the rule into the user config instead of the seat pack.
        #[arg(long)]
        user: bool,
    },
    /// The command guard's rules: verdict, pattern and reason, one a line.
    Rules {
        /// Print them as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Record the person's explicit consent for one pending hook request.
    Approve {
        /// The request id printed by the hook. Approve only after the person agrees.
        id: String,
    },
    /// What the command guard says about a command, then what ljos remembers about it. Runs nothing.
    Policy {
        /// Exit 1 when the answer is a deny. Without this flag the exit stays 0.
        #[arg(long)]
        fail_on_deny: bool,
        argv: Vec<String>,
    },
    /// The hook a coding agent calls before it acts: hook JSON or plain text on stdin,
    /// the memories it bears on and the guard's verdict out. `--prompt --text` prints them plainly.
    Hook {
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
        /// Exit 1 when the answer is a deny. Without this flag the exit stays 0,
        /// so a runner that reads the JSON is unchanged.
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
    },
    /// Run the ljos part of one git hook. The block `ljos git-hooks install` appends calls this.
    GitHook {
        /// Hook name, for example `post-commit`.
        name: String,
        /// Arguments git passed the hook. They are not read.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Install, remove, or show the global git hooks that book commits.
    GitHooks {
        /// `install`, `uninstall`, or `status`.
        action: String,
    },
    /// Set, clear, or show the person's passphrase. `set` and `clear` read
    /// from the terminal and are the person's, with no runner variable set.
    PersonKey {
        /// `set`, `clear`, or `status`.
        action: String,
    },
    /// What the push gate decides on the issue, from seat ballots alone,
    /// then the weighted settle over every ballot as advice: shares, who
    /// leads, influence, and what each anchor did.
    Consensus {
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
    /// Sign a launch slot on the issue a cloud run is launched for: the
    /// run's seat, the model asked for, the run id and the run's public
    /// key, under this seat's pinned key. Written before the run's first
    /// ballot.
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
    /// Whether ljos is ready, and the rows that need action. Exit 1 when a required part does not answer or the note queue is damaged.
    Doctor {
        /// Print every row, the ones that answer included.
        #[arg(long, visible_alias = "verbose")]
        all: bool,
        /// Refresh runner skills and runner env files this ljos wrote, and replace a stale cloud guard rule file. A skill edited by hand, an env file ljos did not write, and a cloud guard file that is missing or not a known older version are left in place.
        #[arg(long)]
        fix: bool,
    },
    /// Read-only icedtea pane over due, claims, and trust. Execs sibling `ljos-hud`.
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
    /// Print the sitting protocol: which store answers what, and the order of verbs.
    Protocol,
    /// The name this agent works under, the name its claims go under, and where each came from.
    /// `add` writes a seat into the tracker's vissue.toml. `grant` records a quoted yes to close.
    Seat {
        #[command(subcommand)]
        action: Option<SeatAction>,
    },
    /// Serve MCP over stdio: run the `ljos-mcp` installed beside this `ljos`, with these arguments. For a client that starts a server by its crate's name.
    Mcp {
        /// Arguments passed to `ljos-mcp` as they stand.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Connect ljos to a coding agent with `--harness NAME`, or print the MCP entry to paste into any agent.
    Onboard {
        /// A runner named in ~/.config/ljos/harnesses.toml or shipped with ljos; `all` onboards every runner found on this machine, and `json` prints the entry to paste anywhere. Absent: `all` at a terminal, `json` into a pipe.
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
        /// The log size the receiver kept last time (`size=` in its kept head): the bag then carries the bridge from it.
        #[arg(long)]
        since_size: Option<u64>,
    },
    /// Check a satchel that arrived; --import puts its atoms in this seat's pack.
    Receive {
        dir: PathBuf,
        /// The head file `--keep` wrote at the last receive from the same sender.
        #[arg(long)]
        since: Option<PathBuf>,
        /// Write this bag's log head here, for `--since` next time.
        #[arg(long)]
        keep: Option<PathBuf>,
        #[arg(long)]
        import: bool,
    },
    /// The eight memories due for review soonest, then one line on the review clock.
    Due {
        /// List every due atom to read; none of them is put up for grading.
        #[arg(long)]
        all: bool,
        /// Put the page to the review judges. A claim they find holds is
        /// named for `ljos graded`. A contradicted one is named. The rest stay due.
        #[arg(long, conflicts_with = "all")]
        judge: bool,
    },
    /// Score the judge log against outcomes: accuracy and calibration per decision.
    JudgeScore,
    /// Put an eb-stack bundle's modules on the tracker: one child issue per module under the parent, blockers along the dependency edges, the same ids on every run. `vissue ready` then lists what a seat can build now.
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
    /// The typed findings of an eb-stack campaign state file, one per line; `--remember` writes one lesson per finding a person or a seat resolved, and `--issue` cites the state file on the issue.
    Findings {
        /// The campaign state file (`campaign.json`).
        state: PathBuf,
        /// Write one lesson per resolved finding to the pack.
        #[arg(long)]
        remember: bool,
        /// With --remember, every finding, the ones a later attempt superseded too.
        #[arg(long)]
        all: bool,
        /// Cite the state file as a deed on this issue.
        #[arg(long)]
        issue: Option<String>,
    },
    /// A habit is a number the seat keeps measuring. `ljos habit NAME VALUE` takes a reading and closes the one before; `ljos habit NAME` shows one; `ljos habit` lists them all with the change since the last reading and when the next is due.
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
    /// Start work on a ticket: the checks, what ljos remembers for it, and the claim.
    Sitting {
        /// The tracker id of the issue.
        issue: String,
        /// Occupancy name, taken as given, the seat's own name too. Absent: this conversation's holder (`ljos seat`). Always scoped to the issue.
        #[arg(long)]
        assignee: Option<String>,
        /// Where the cards are read from.
        #[arg(long, default_value = ".")]
        cards: PathBuf,
        /// Sit even when the issue's blockers are open. Without it a blocked issue is refused before anything is claimed.
        #[arg(long)]
        anyway: bool,
        /// The recipe this sitting copies before recall. Absent, a closed-set token in the title else sit. Held until finish or release.
        #[arg(long)]
        playbook: Option<String>,
        /// Print every section in full: the doctor's rows, recall, timeline, reclaim. Without it the sitting prints what needs action and what the seat remembers.
        #[arg(long)]
        verbose: bool,
    },
    /// Finish work on a ticket: remember the lesson, strengthen the memories it used, hand the claim back.
    Finish {
        /// The tracker id of the issue.
        issue: String,
        /// done, failed, or cancelled.
        #[arg(long, default_value = "done")]
        status: String,
        /// The lesson this sitting taught, two sentences at most.
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
        /// Generation from the sitting's claim. Absent: the live one. A stale gen is refused.
        #[arg(long)]
        gen: Option<u64>,
        /// The name that holds it. Absent: this conversation's holder (`ljos seat`).
        #[arg(long)]
        assignee: Option<String>,
        /// Close the tracker ticket. A runner is refused. CANCELLED with `--status cancelled`. Only when the work is accepted or dropped, not when the sitting ends.
        #[arg(long)]
        close: bool,
    },
    /// Put an issue back to the state it had before a close this program wrote.
    /// A person has to be present. A note is not that record. The caller does not take the claim.
    Reopen {
        /// The tracker id of the issue.
        issue: String,
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
        /// The outcome is none of the ballot's options: the person answered with a new one.
        #[arg(long)]
        new_option: bool,
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

/// The write acknowledgement: the plain sentence for a person at a
/// terminal, the tab-separated row for a pipe.
fn write_ack(body: &serde_json::Value) -> String {
    // The pack keeps a person's hard sentence and says why it would have
    // refused it; that goes to stderr, so the one line a script reads
    // stays one line.
    if let Some(note) = ljos_cli::prose_warning_line(body) {
        eprintln!("{note}");
    }
    if std::io::stdout().is_terminal() {
        ljos_cli::format_write_ack_plain(body, &ljos_cli::local_when)
    } else {
        format_write_ack(body)
    }
}

/// Print a relay fault in the runner's contract and exit with its code.
/// A prompt has no such deny; the caller then returns the error.
fn exit_relay_fault(runner: &str, event: &str, what: &str) -> Result<()> {
    let Some((out, err, code)) = ljos_cli::runner_hooks::relay_fault_deny(runner, event, what)
    else {
        anyhow::bail!("hook: {what}");
    };
    print!("{out}");
    eprint!("{err}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::Write::flush(&mut std::io::stderr());
    std::process::exit(code);
}

fn main() -> Result<()> {
    // Both run before the seat's environment, the tracker or the pack is
    // read: the server stays small, and a hook call waits its turn before
    // it costs anything.
    if std::env::args_os().nth(1).is_some_and(|a| a == "hookd") {
        std::process::exit(ljos_cli::hookd::serve());
    }
    ljos_cli::hookd::admit();
    let ran = run_command();
    if let Err(e) = &ran {
        // A gate whose hook fails answers deny in the runner's JSON and
        // exits 2, so a runner that reads silence as allow still refuses.
        ljos_cli::refuse_failed_gate(&format!("{e:#}"));
    }
    ran
}

fn run_command() -> Result<()> {
    ljos_cli::load_seat_env();
    ljos_cli::normalize_tracker_env();
    // A closed pipe ends the run quietly: `ljos learn | head` is not a panic.
    // SAFETY: resetting a signal disposition before any thread is spawned.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cmd = Cli::parse().cmd;
    // The first run of a new version refreshes skills and runner env
    // files ljos wrote. Doctor stays read-only unless `--fix` is set, and
    // a record check, which CI runs, always stays read-only.
    match &cmd {
        Cmd::Doctor { fix: true, .. } => {
            ljos_cli::runner_skills::refresh_for_fix();
            ljos_cli::runner_env::refresh_for_fix();
            ljos_cli::cloud::replace_stale_guard();
        }
        Cmd::Doctor { .. } | Cmd::Record { .. } => {}
        _ => {
            ljos_cli::runner_skills::refresh_on_new_version();
            ljos_cli::runner_env::refresh_on_new_version();
        }
    }
    // A subagent with no seat would hold under its process tree ("bash").
    // The commands that name a holder refuse instead.
    if matches!(
        cmd,
        Cmd::Seat { .. }
            | Cmd::Sitting { .. }
            | Cmd::Claim { .. }
            | Cmd::Note { .. }
            | Cmd::Slot { .. }
            | Cmd::Finish { .. }
            | Cmd::Release { .. }
            | Cmd::Complete { .. }
            | Cmd::File { .. }
            | Cmd::Options { .. }
            | Cmd::Deed { .. }
    ) && ljos_cli::unseated_subagent()
    {
        bail!("{}", ljos_cli::SUBAGENT_NO_SEAT);
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
    let checked = tracker_write(&cmd).map(ljos_cli::write_seat).transpose()?;
    let _seated = checked.as_ref().map(ljos_cli::Seated::enter);
    let assignee_for = |verb: &str, passed: Option<&str>, delegates: bool| -> Result<String> {
        let name = cli_assignee(passed);
        match &checked {
            Some(seat) => ljos_cli::write_assignee(verb, seat, name, delegates),
            None => Ok(name),
        }
    };
    match cmd {
        Cmd::Remember {
            text,
            as_persona,
            transient,
            standing,
            valid_from,
            valid_to,
        } => {
            if transient && standing {
                bail!("remember: pass --transient or --standing, not both");
            }
            ljos_cli::set_validity(valid_from.as_deref(), valid_to.as_deref())?;
            let text = join(&text);
            // Unless the person is at the terminal, the agent's own words
            // are an unreviewed note. Words from the person's latest prompt
            // are theirs and stand. The seat guard lets remember through,
            // so this is the check.
            let body = if ljos_cli::person_present() {
                let horizon = ljos_cli::remember_horizon(transient, standing, false);
                packset_write_as("Remember", &text, as_persona.as_deref(), horizon)?
            } else if ljos_cli::capture::said_in_latest_prompt(&text) {
                let horizon = transient.then_some(true);
                ljos_cli::packset_write_mcp("Remember", &text, as_persona.as_deref(), horizon)?
            } else {
                ljos_cli::packset_write_remember_note(&text, as_persona.as_deref())?
            };
            println!("{}", write_ack(&body));
            eprintln!("{}", ljos_cli::remember_note(&body));
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
                let _ = keep;
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
                    ljos_cli::user_rules::promote_lesson(&id, why, &mention, &reply, false)?
                } else {
                    ljos_cli::user_rules::write_preference(&join(&text), why, &mention, &reply)?
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
            let body = ljos_cli::packset_prefer(&text, as_persona.as_deref())?;
            println!("{}", write_ack(&body));
            for rule in &rules {
                println!("{}", write_ack(&write_rule(rule, basis)?));
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
        Cmd::Accept { id } => {
            let id = ljos_cli::admit::resolve_id(&id)?;
            // The agent proposed the lesson; accepting it is a review by
            // someone else. Under a runner that is the conversation that
            // started a worker, reviewing the worker's proposal; an agent
            // that accepted its own proposal would skip the review.
            if ljos_cli::under_a_runner()
                && !ljos_cli::accepts_another_conversation(
                    ljos_cli::admit::proposed_by(&id).as_deref(),
                    &ljos_cli::holder_name(),
                )
            {
                bail!(
                    "accept: this conversation filed {id}, or the proposal names no conversation, \
                     and an agent can't accept its own proposal. The person runs `ljos accept {id}` \
                     in a terminal of their own, or the conversation that started this worker runs it"
                );
            }
            print!("{}", ljos_cli::admit::accept(&id)?);
        }
        Cmd::Memories { cmd } => memories_cmd(cmd)?,
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
        Cmd::Sync {
            key,
            import,
            export,
            adopt,
            force,
            dry_run,
            adopt_legacy,
        } => {
            if let Some(host) = adopt {
                print!("{}", ljos_cli::sync::adopt(&host, force, dry_run)?);
            } else if let Some(host) = adopt_legacy {
                print!("{}", ljos_cli::sync::adopt_legacy(&host)?);
            } else if key {
                println!("{}", ljos_cli::sync::public_key()?);
            } else {
                let both = !import && !export;
                print!(
                    "{}",
                    ljos_cli::sync::sync_repo(both || import, both || export)?
                );
            }
        }
        Cmd::Hubs { limit } => print!("{}", format_hubs(&packset_hubs(limit)?)),
        Cmd::Conflicts { limit } => print!("{}", conflicts(limit)?),
        Cmd::Consolidate { apply } => {
            print!("{}", format_consolidation(&packset_consolidate(apply)?))
        }
        Cmd::Island {
            cue,
            fire,
            as_persona,
        } => {
            print!(
                "{}",
                format_island(&packset_island_as(
                    &join(&cue),
                    fire,
                    as_persona.as_deref()
                )?)
            );
        }
        Cmd::Search {
            query,
            limit,
            rerank,
            as_of,
            valid_at,
            explain,
        } => {
            ljos_cli::log_recall("search", &query.join(" "));
            let day = match valid_at.as_deref().map(str::trim) {
                Some("any") => None,
                Some(d) => {
                    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| {
                        anyhow::anyhow!("--valid-at {d:?} is not YYYY-MM-DD or any")
                    })?;
                    Some(d.to_string())
                }
                None => Some(chrono::Local::now().format("%Y-%m-%d").to_string()),
            };
            // Expired claims are dropped after the pack answers, so ask for
            // more than the limit and cut to it once they are gone.
            let fetch = if day.is_some() {
                ljos_cli::valid_fetch(limit)
            } else {
                limit
            };
            let mut hits = packset_search_as_of(&join(&query), fetch, as_of.as_deref(), rerank)?;
            if let Some(day) = day {
                hits.retain(|h| ljos_cli::valid_on(&h.entities, &day));
            }
            hits.truncate(limit as usize);
            print!("{}", format_hits(&hits, explain));
        }
        Cmd::Evidence { args } => {
            if args.first().map(String::as_str) == Some("record") {
                let mut cmd = args[1..].to_vec();
                if cmd.first().map(String::as_str) == Some("--") {
                    cmd.remove(0);
                }
                println!("{}", ljos_cli::gates::record_evidence(&cmd)?);
            } else if args.len() == 1 {
                let said = run_captured("deedar", &["evidence", &args[0]])?;
                print!("{}", ljos_cli::evidence_here(&said.stdout));
            } else {
                bail!(
                    "evidence: pass one deed accession, or `ljos evidence record -- <test command>`"
                );
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
            add,
            remove,
            ticket,
        } => {
            if !remove.is_empty() {
                let ticket = ticket
                    .as_deref()
                    .or(issue.as_deref())
                    .context("deed: name the issue to drop it from")?;
                for a in &remove {
                    print!("{}", uncite_deed(ticket, a)?);
                }
                print!(
                    "{}",
                    ljos_cli::persist_tracker(ticket, "removed a deed citation")
                );
            } else if add.is_empty() {
                let issue = issue.as_deref().context("deed: an issue is required")?;
                run("vissue", &["deed", issue])?;
            } else {
                let issue = issue.as_deref().context("deed: an issue is required")?;
                let mut argv = vec!["deed".to_string(), issue.to_string()];
                for a in &add {
                    argv.push("--add".into());
                    argv.push(ljos_cli::resolve_deed(a)?);
                }
                let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
                run("vissue", &refs)?;
                print!("{}", ljos_cli::persist_tracker(issue, "cited a deed"));
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
        } => {
            if let Some(d) = &deadline {
                chrono::DateTime::parse_from_rfc3339(d.trim())
                    .map_err(|e| anyhow::anyhow!("--deadline {d:?} is not RFC 3339: {e}"))?;
            }
            let body = ljos_cli::body_with_options(body, &options.into_iter().collect::<Vec<_>>())?;
            let held = if top || parent.is_some() {
                None
            } else {
                ljos_cli::held_issue()
            };
            let (parent, project, passed_over) =
                ljos_cli::file_placement(top, parent, project, held);
            if let Some(line) = passed_over {
                eprint!("{line}");
            }
            let Some(project) = project else {
                anyhow::bail!("no project: name one with -p, or a parent with --parent");
            };
            let qd = match (quorum, deadline.as_deref()) {
                (Some(n), Some(d)) => Some((n, d)),
                _ => None,
            };
            let (id, text, persisted, candidate) = ljos_cli::file_issue(
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
            )?;
            print!("{text}");
            if let (Some((n, d)), false) = (qd, id.is_empty()) {
                let note = format!("{}{n} by {}", ljos_cli::reap::QUORUM_NOTE, d.trim());
                println!("{id}: {note}; `ljos reap --apply` lapses it past the deadline");
            }
            print!("{persisted}");
            match candidate {
                Ok(said) => print!("{said}"),
                Err(e) => eprintln!("ljos file: {e:#}"),
            }
        }
        Cmd::Options { issue, options } => {
            print!("{}", ljos_cli::append_issue_options(&issue, &[options])?);
        }
        Cmd::Note { issue, text } => {
            let text = text.join(" ");
            run("vissue", &["note", &issue, &text])?;
            match ljos_cli::packset_write_note(&issue, &text) {
                Ok(mem) => {
                    if let Some(body) = mem.body.as_ref() {
                        let id = body["id"].as_str().unwrap_or("?");
                        let short: String = id.chars().take(8).collect();
                        println!("noted in memory (id {short})");
                    }
                    if let Some(e) = mem.error.as_ref() {
                        print!("{}", ljos_cli::note_memory_warning(&issue, e));
                    }
                    match mem.queued {
                        Some(true) => println!(
                            "memory: queued the copy; the next note or finish that reaches the pack, or a session start, sends it"
                        ),
                        Some(false) => println!("memory: the same note is already queued"),
                        None => {}
                    }
                    if mem.body.is_some() {
                        match ljos_cli::flush_note_queue() {
                            Ok(done) => print!("{}", ljos_cli::note_queue::drained_lines(&done)),
                            Err(e) => eprintln!("ljos note: note queue: {e:#}"),
                        }
                    }
                }
                Err(e) => print!("{}", ljos_cli::note_memory_warning(&issue, &e)),
            }
            if let Some(line) = ljos_cli::assist_note_line(&issue) {
                print!("{line}");
            }
            match ljos_cli::ballot_sig::sign_own_ballot(&issue) {
                Ok(line) => print!("{line}"),
                Err(e) => eprintln!("ljos note: {e:#}"),
            }
            print!("{}", ljos_cli::persist_tracker(&issue, "noted"));
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
        Cmd::Claims { args } => {
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
            let mut argv = vec!["claims"];
            argv.extend(mapped.iter().map(String::as_str));
            run("vissue", &argv)?;
        }
        Cmd::Recall { issue } => {
            ljos_cli::log_recall("recall", &issue);
            let said = ljos_cli::tracker_recall(&issue)?;
            eprint!("{}", said.stderr);
            print!("{}", said.stdout);
            print!("{}", ljos_cli::recall_memories(&issue));
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
        } => match choice {
            None if withdraw => {
                let who = ljos_cli::ballot_voter("vote", as_persona.as_deref())?;
                let ballot_issue = ljos_cli::panel_gate::standing_ballot_issue(&issue, &who);
                // A ballot already taken back still leaves its forecast to
                // take back, so a second withdraw finishes the first.
                match ljos_cli::run_captured_as(
                    "vissue",
                    &["vote", &issue, "--withdraw"],
                    Some(&who),
                ) {
                    Ok(said) => {
                        print!("{}", said.stdout);
                        if let Some(ballot_issue) = ballot_issue.as_deref() {
                            if let Err(e) =
                                ljos_cli::ballot_sig::note_withdrawn_on(ballot_issue, &who, &issue)
                            {
                                eprintln!("ljos vote: {e:#}");
                            }
                        }
                    }
                    Err(e) if format!("{e:#}").contains("holds no ballot") => {
                        println!("{issue}: {who} holds no ballot; withdrawing the forecast");
                        if let Some(ballot_issue) = ballot_issue.as_deref() {
                            if let Err(e) =
                                ljos_cli::ballot_sig::note_withdrawn_on(ballot_issue, &who, &issue)
                            {
                                eprintln!("ljos vote: {e:#}");
                            }
                        }
                    }
                    Err(e) => return Err(e),
                }
                let n = withdraw_prediction(&issue, &who)
                    .with_context(|| format!("ballot withdrawn; the forecast for {who} was not"))?;
                println!("{n} forecast(s) withdrawn for {who}");
                print!("{}", ljos_cli::persist_tracker(&issue, "ballot withdrawn"));
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
                        print!("{}", ljos_cli::persist_tracker(&issue, "ballot cast"));
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
                let who = ljos_cli::ballot_voter("vote", as_persona.as_deref())?;
                let cast = ljos_cli::panel_gate::claim_session_ballot(&issue, &who)?;
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
                run_as("vissue", &refs, Some(&who))?;
                // Read the ballot back: only the tracker is proof it exists.
                let rows: Vec<serde_json::Value> =
                    ljos_cli::run_captured("vissue", &["vote", &issue, "--json"])
                        .ok()
                        .and_then(|o| serde_json::from_str(&o.stdout).ok())
                        .unwrap_or_default();
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
                match ljos_cli::ballot_sig::sign_ballot(&issue, &who, &args[3]) {
                    Ok(said) => print!("{said}"),
                    Err(e) => eprintln!("ljos vote: {e:#}"),
                }
                if let Some(expect) = expect.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
                    write_prediction(&issue, &who, expect).with_context(|| {
                        format!("ballot cast; forecast for {who} was not recorded")
                    })?;
                    println!("forecast recorded for {who}");
                }
                print!("{}", ljos_cli::persist_tracker(&issue, "ballot cast"));
            }
            None => print!("{}", ljos_cli::panel_gate::tally_report(&issue)?),
        },
        Cmd::Brief { name, issue } => {
            ljos_cli::panel_gate::note_tracker_read(&issue);
            print!("{}", brief(&name, &issue)?);
        }
        Cmd::Timeline { issue, limit } => {
            ljos_cli::panel_gate::note_tracker_read(&issue);
            print!("{}", timeline(&issue, limit)?);
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
            println!("{}", write_ack(&body));
            ljos_cli::refresh_agents();
        }
        Cmd::Upgrade { version, dir } => {
            print!(
                "{}",
                ljos_cli::upgrade::upgrade(version.as_deref(), dir.as_deref())?
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
            let exit = ljos_cli::plugin::ExitKind::parse(&exit)
                .with_context(|| format!("exit {exit}: clean or crash"))?;
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
            let mut input = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
            println!("{}", ljos_cli::statusline(&input));
        }
        Cmd::Playbook { issue, name } => {
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
            assignee,
            next,
            role,
            slack,
        } => {
            let assignee = assignee_for("claim", assignee.as_deref(), false)?;
            if next || node.is_empty() {
                let said = claim_next(&assignee, role.as_deref(), slack)?;
                let taken = said.split_whitespace().next().unwrap_or("").to_string();
                print!("{said}");
                if !taken.is_empty() {
                    print!("{}", ljos_cli::persist_tracker(&taken, "claimed"));
                }
            } else {
                print!("{}", claim(&node, &assignee)?);
                print!("{}", ljos_cli::persist_tracker(&node, "claimed"));
            }
        }
        Cmd::Release { node, assignee } => {
            // `release HEX --assignee NAME` hands back a claim another
            // name still holds.
            let assignee = assignee_for("release", assignee.as_deref(), true)?;
            print!("{}", release(&node, &assignee)?);
            print!("{}", ljos_cli::persist_tracker(&node, "released"));
        }
        Cmd::Complete {
            node,
            status,
            gen,
            assignee,
        } => print!(
            "{}",
            complete(
                &node,
                status.as_deref(),
                &assignee_for("complete", assignee.as_deref(), false)?,
                gen
            )?
        ),
        Cmd::Cards { dir } => print!("{}", cards(&dir)?),
        Cmd::Predict {
            issue,
            expect,
            as_persona,
        } => {
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
                let basis = ljos_cli::Basis::of_rule();
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
                        write_ack(&body)
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
        Cmd::Approve { id } => {
            print!("{}", ljos_cli::approval::approve(&id)?);
        }
        Cmd::Policy { argv, fail_on_deny } => {
            let text = policy_with_memory(&argv)?;
            print!("{text}");
            if ljos_cli::shell_exit(fail_on_deny, &text) != 0 {
                std::process::exit(1);
            }
        }
        Cmd::GitHook { name, .. } => ljos_cli::git_hooks::run_named(&name)?,
        Cmd::GitHooks { action } => ljos_cli::git_hooks::run(&action)?,
        Cmd::PersonKey { action } => ljos_cli::person_key::run(&action)?,
        Cmd::Hook {
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
        } => {
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
                return ljos_cli::git_hooks::run_git_commit(
                    git_hook.as_deref().unwrap_or("post-commit"),
                );
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
                        if let Some(code) =
                            ljos_cli::emit_cursor_side_answer(&lossy, event.as_deref())
                        {
                            std::process::exit(code);
                        }
                    }
                    // The relay reads stdin before the gate's panic hook is
                    // armed. Invalid text used to exit 1 with no output,
                    // which nine runners read as an allow.
                    if let Some(name) = runner.as_deref() {
                        if ljos_cli::runner_hooks::is_runner(name)
                            && ljos_cli::runner_hooks::relay_event_is_gate(event.as_deref())
                        {
                            return exit_relay_fault(
                                name,
                                event.as_deref().unwrap_or(""),
                                "stdin is not utf-8",
                            );
                        }
                    }
                    anyhow::bail!("hook: stdin is not utf-8");
                }
            };
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
                    Ok(Ok(code)) => std::process::exit(code),
                    Ok(Err(err))
                        if ljos_cli::runner_hooks::relay_input_is_gate(
                            &input,
                            event.as_deref(),
                        ) =>
                    {
                        return exit_relay_fault(
                            runner,
                            event.as_deref().unwrap_or(""),
                            &format!("{err:#}"),
                        );
                    }
                    Ok(Err(err)) => return Err(err),
                    Err(_)
                        if ljos_cli::runner_hooks::relay_input_is_gate(
                            &input,
                            event.as_deref(),
                        ) =>
                    {
                        return exit_relay_fault(
                            runner,
                            event.as_deref().unwrap_or(""),
                            "panicked",
                        );
                    }
                    Err(_) => anyhow::bail!("hook: relay panicked"),
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
                // Cursor blocks a permission hook that prints nothing.
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
                    std::thread::sleep(std::time::Duration::from_millis(
                        ljos_cli::HOOK_DEADLINE_MS,
                    ));
                    // Taking the lock waits out an answer being written.
                    let _held = std::io::stdout().lock();
                    std::process::exit(0);
                });
            }
            // A hook answers within the runner's timeout: lookups that walk
            // the whole tracker are skipped from here on.
            // SAFETY: single-threaded here, before anything reads the environment.
            unsafe { std::env::set_var("LJOS_IN_HOOK", "1") };
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
                    if let Some(reason) = ljos_cli::subagent_stop_reason(
                        kind,
                        issue.as_deref(),
                        decision,
                        stop_active,
                    ) {
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
            // one more round. An agy stop with `fullyIdle` is the session
            // end: it finishes even when a mid-loop stop would have held.
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
                if let Some(reason) =
                    ljos_cli::seat_stop_reason(&input, stop_active, subagent.is_some())
                {
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
            // and there is nothing to say.
            if call.event == "SessionEnd" {
                session_end(call.session.as_deref());
                ljos_cli::extract::at_session_end(&input, call.session.as_deref());
                // Finish or release after extract. A bookkeeping error stays
                // off the answer: this event has nothing to say.
                let _ = ljos_cli::bookkeep::observe(&input, &call);
                let _ = ljos_cli::drain_tracker_pushes();
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
            let unresolved = match &resolved {
                Err(e)
                    if on_tool
                        && !suspended
                        && !argv.is_empty()
                        && !ljos_cli::guard_fail_open()
                        && !ljos_cli::subagent::payload_names_mcp_tool(&input) =>
                {
                    Some(ljos_cli::Rule {
                        pattern: "cwd-unresolved".into(),
                        verdict: "deny".into(),
                        reason: format!(
                            "cwd-unresolved: the directory this command would run in does not \
                             resolve ({e:#}), so the guard cannot judge it there. Run it from a \
                             directory that exists and can be entered"
                        ),
                    })
                }
                _ => None,
            };
            let tcb_rule = if unresolved.is_some() {
                unresolved
            } else if let Some(rule) = fast {
                Some(rule)
            } else if on_tool && !argv.is_empty() {
                ljos_cli::tcb_verdict_in(&call.cue, dir)
            } else if on_tool {
                ljos_cli::seat_guard(&call.cue)
            } else {
                None
            };
            // An ordinary branch or tag push is allowed. Force, a
            // +refspec, a remote-branch delete and a mirror push ask by
            // default.
            let gated = ljos_cli::redirect_seat_verb(
                ljos_cli::gate_push(
                    ljos_cli::ruled_verdict(&rules, &call.cue).as_ref(),
                    &call.cue,
                    cwd.as_ref().and_then(|p| p.to_str()),
                ),
                &call.cue,
            );
            // A seat rule that denies this push still denies it. Otherwise
            // the policyd ask (a force push) is the verdict, then the gate.
            let verdict = if gated.as_ref().is_some_and(|rule| rule.verdict == "deny") {
                gated.as_ref()
            } else {
                tcb_rule.as_ref().or(gated.as_ref())
            };
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
            // A lesson or a note queued while the pack was down is sent
            // before this session reads memory, so the next prompt can recall it.
            if matches!(call.event.as_str(), "SessionStart" | "UserPromptSubmit") {
                let _ = ljos_cli::flush_note_queue();
            }
            // Search on the prompt. A camel-case runner discards that
            // stdout, so each held note goes out on the next tool result.
            // Stop additionalContext starts another round, so that runner
            // does not print a held note on Stop. PreToolUse / argv only decide.
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
                        String::new()
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
                    // The person's prompt is the chat's consent channel; the
                    // grant goes first, ahead of the hook's deadline.
                    // Only the person's own prompt grants, saves or
                    // corrects: a subagent's brief or an unattended run's
                    // prompt is an agent's words in that channel.
                    // A tool call can run `ljos hook` with a payload of its
                    // own; only a hook the runner itself spawned carries
                    // the person's prompt.
                    if call.event == "UserPromptSubmit" {
                        ljos_cli::capture::forget_latest_prompt();
                    }
                    let spawner = (call.event == "UserPromptSubmit"
                        && ljos_cli::capture::not_the_person(&input).is_none())
                    .then(|| match ljos_cli::proc_tree::hook_runner() {
                        Ok((runner, _)) => Some(runner),
                        Err(why) => {
                            eprintln!("ljos hook: this prompt is not kept as the person's: {why}.");
                            None
                        }
                    })
                    .flatten();
                    let person = spawner.is_some();
                    let granted = person
                        .then(|| {
                            ljos_cli::hook_timed("approve", || {
                                ljos_cli::approval::approve_from_prompt(
                                    &call.cue,
                                    call.session.as_deref(),
                                )
                            })
                        })
                        .flatten();
                    // A standing choice the person states is saved in their
                    // words before the note is read, so the note does not
                    // repeat it and the correction hook does not file it
                    // again as a proposal.
                    let captured = if person {
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
                        ljos_cli::hook_timed("capture", || ljos_cli::capture::capture_stated(&call))
                    } else {
                        None
                    };
                    if person {
                        ljos_cli::hook_timed("correction", || ljos_cli::store_correction(&call));
                    }
                    let (mut ctx, mut ids) =
                        ljos_cli::hook_timed("hook_note", || hook_note(&call, limit));
                    if let Some((line, keys)) = captured {
                        ctx = if ctx.is_empty() {
                            line
                        } else {
                            format!("{line}\n{ctx}")
                        };
                        ids.extend(keys);
                    }
                    if call.shape == ljos_cli::HookShape::CamelCase
                        && call.event == "UserPromptSubmit"
                    {
                        let line = ljos_cli::GROK_PACK_LINE;
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
                    if let Some(granted) = granted {
                        ctx = if ctx.is_empty() {
                            granted
                        } else {
                            format!("{granted}\n{ctx}")
                        };
                    }
                    if call.event == "UserPromptSubmit" {
                        let (mail, mail_ids) =
                            ljos_cli::hook_timed("mail", ljos_cli::mail::prompt_note);
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
                    prompt_hook_stdout(call.shape, call.session.as_deref(), &ctx, &ids)
                }
            };
            // The book is kept here, after a deny has already been decided,
            // so a tracker that does not answer cannot turn into a refusal.
            // A Stop the audit held never reaches this line, except an agy
            // stop that is idle: that one is the session end and finishes.
            // A mid-loop agy stop, fullyIdle absent, does not finish.
            // The book is queued, not kept in this call: it waits on a lock
            // every seat shares, and a claim reads the whole tracker. A
            // gate's book is queued once its answer is out.
            let ask = verdict
                .filter(|rule| rule.verdict == "ask")
                .map(|rule| rule.reason.clone());
            let agy_mid_loop = call.shape == ljos_cli::HookShape::Steps
                && call.event == "Stop"
                && !ljos_cli::stop_is_idle(&input);
            let book = verdict.is_none() && !agy_mid_loop;
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
            let output = ljos_cli::approval::hook_output(&input, &call, &context, verdict);
            ljos_cli::answer_gate(&output);
            if let Some(reason) = ask {
                ljos_cli::book_queue::enqueue_ask(&input, &call, &reason);
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
        }
        Cmd::Consensus {
            id,
            json,
            compare,
            no_file,
            quorum,
            deadline,
        } => {
            // Rows scoped to a domain apply when the issue is about it; the
            // personas' anchors go to both settles; the issue's tags pick
            // the model.
            let (topic, tags) = issue_topic_and_tags(&id);
            let trust = rows_about(&pack_trust_or_none(), &topic);
            let personas = personas_from_pack().unwrap_or_default();
            let atoms = pack()
                .and_then(|c| {
                    ljos_cli::atoms_lean(&c, &c.workspace())
                        .context("consensus: GET /v1/atoms failed")
                })
                .unwrap_or_default();
            // A persona on the ballot settles under its anchor, and every
            // other voter, the seat too, under the default one; a ballot
            // of seats alone stays plain DeGroot.
            let ballots = ljos_cli::ballot_rows(&id);
            ljos_cli::require_ballots(&id, &ballots)?;
            if let Some(msg) = ljos_cli::panel_gate::settle_refusal(&id) {
                let flags = ljos_cli::panel_gate::blind_lines_for(&id);
                if json {
                    let doc = serde_json::json!({
                        "settled": false,
                        "panel": {"settled": false, "refusal": msg, "blind": flags},
                        "push_cite": {"issue": id, "stands": false, "refusal": msg},
                    });
                    println!("{}", serde_json::to_string_pretty(&doc)?);
                } else {
                    println!("{msg}");
                    for line in flags {
                        println!("blind design: {line}");
                    }
                }
                std::process::exit(1);
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
            let (gate, gate_json) =
                ljos_cli::push_cite_reading_on_roster(&id, &ballots, &ljos_cli::seat_name());
            let (protected, protected_json) =
                ljos_cli::protected_push_reading(&id, std::env::current_dir().ok().as_deref());
            // The named outcomes show which voters err together, and
            // those voters are discounted.
            let discount = settle_discount(&atoms).map(|(discount, line)| {
                with_discount(&mut steps, &discount);
                line
            });
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
            let dissent = if no_file || !on_path("vissue") {
                Vec::new()
            } else {
                ljos_cli::converge::record_dissents(&id)
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
                if !dissent.is_empty() {
                    doc["dissent"] = serde_json::json!(dissent);
                }
                println!("{}", serde_json::to_string_pretty(&doc)?);
                return Ok(());
            }
            print!("{gate}");
            print!("{protected}");
            for line in &dissent {
                println!("dissent: {line}");
            }
            println!("advice, which does not decide a push: the weighted settle over every ballot");
            if let Some(line) = discount {
                println!("{line}");
            }
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
                    continue;
                }
                if !settle {
                    run(step.bin, &step.args)?;
                    continue;
                }
                let said = run_captured(step.bin, &step.args)?;
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
            println!("{v}");
        }
        Cmd::Doctor { all, .. } => {
            let rows = doctor();
            if all {
                print!("{}", format_doctor(&rows));
            } else {
                print!("{}", format_doctor_short(&rows));
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
        Cmd::Protocol => print!("{}", protocol_text()),
        Cmd::Seat { action } => match action {
            None => print!("{}", format_seat_command()),
            Some(SeatAction::Add { name }) => print!("{}", ljos_cli::seat_roster::add(&name)?),
            Some(SeatAction::Grant {
                seat,
                spec,
                quote,
                at,
                via,
            }) => print!(
                "{}",
                ljos_cli::seat_roster::grant(&seat, &spec, &quote, &at, &via)?
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
            let harness = harness.unwrap_or_else(|| {
                if std::io::stdout().is_terminal() {
                    "all".to_string()
                } else {
                    "json".to_string()
                }
            });
            let steps = if harness == "all" {
                ljos_cli::onboard_detected(dry_run, skills.as_deref())?
            } else {
                onboard_to(&harness, dry_run, skills.as_deref())?
            };
            // With no runner named, the entry is for pasting: print the
            // JSON alone, so it parses as it stands.
            if harness == "json" {
                for s in &steps {
                    println!("{}", s.detail);
                }
                return Ok(());
            }
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
            since_size,
        } => {
            for line in handover(&out, &project, &issue, since_size)? {
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
        Cmd::Due { all, judge } => {
            if judge {
                print!("{}", ljos_cli::judge_due_page()?);
            } else {
                print!("{}", due_report(all)?);
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
                println!("{}", write_ack(&body));
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
            assignee,
            cards: cards_dir,
            anyway,
            playbook,
            verbose,
        } => {
            let full = sitting_gated(
                &issue,
                &assignee_for("sitting", assignee.as_deref(), false)?,
                &cards_dir,
                anyway,
                playbook.as_deref(),
            )?;
            if verbose {
                print!("{full}");
            } else {
                print!("{}", compact_sitting(&full, &issue));
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
        } => {
            match ljos_cli::flush_note_queue() {
                Ok(done) => print!("{}", ljos_cli::note_queue::drained_lines(&done)),
                Err(e) => eprintln!("ljos finish: note queue: {e:#}"),
            }
            print!(
                "{}",
                finish(
                    &issue,
                    &status,
                    lesson.or(lesson_text).as_deref(),
                    outcome.as_deref(),
                    new_option,
                    beta,
                    &assignee_for("finish", assignee.as_deref(), false)?,
                    gen,
                    close
                )?
            );
        }
        Cmd::Reopen { issue } => {
            print!("{}", reopen(&issue)?);
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
            let said = run_captured("vissue", &["vote", &id, "--json"])?;
            let forecasts = forecasts_from_json(&said.stdout)?;
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
        Cmd::Release { .. } => "release",
        Cmd::Complete { .. } => "complete",
        Cmd::File { .. } => "file",
        Cmd::Options { .. } => "options",
        Cmd::Deed { add, remove, .. } if !add.is_empty() || !remove.is_empty() => "deed",
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
        _ => return None,
    })
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
        print!("{said}");
    }
    if !out.status.success() {
        std::process::exit(out.status.code().unwrap_or(1));
    }
    Ok(())
}
