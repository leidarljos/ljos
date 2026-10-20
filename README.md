# ljos

Tell your coding agent a rule once. It comes back on every prompt it bears
on, and the hook refuses the command that breaks it.

Coding agents forget what they learned when the session ends, and they run
whatever shell command they pick. ljos fixes both on your own machine. It
keeps what you tell the agent in a local store and hands the matching notes
back before the model reads each prompt. It checks each shell command against
your rules and a built-in list of destructive commands before the command
runs. It plugs into the agent through the agent's own hook and MCP settings.
Nothing leaves the machine unless you turn on a remote judge.

It works with Claude Code, Codex, Cursor, GitHub Copilot CLI, Gemini CLI,
Windsurf, Factory's Droid, Kiro, Cline, Qwen Code, Crush, Auggie, Amp, goose,
OpenHands, Continue, Roo Code, Kilo Code, Grok Build, opencode, omp, hermes
and Antigravity's `agy`.

## Five minutes

```
curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh
ljos prefer "Tag a release with git push origin TAG; --follow-tags leaves v-tags behind."
ljos rule 'terraform destroy*' --verdict deny --why "Destroy only from the release runbook."
```

The installer downloads each program from its GitHub release, checks it
against the published SHA-256 sum, and builds from source with `cargo` where a
platform has no release. It then runs `ljos onboard` for every coding agent it
finds and shows the guard refusing a force push. Its programs are ljos,
packset (memory), vissue (the tracker), deedar (records of work), claimdag
(who works on what), ljos-policyd (the command check) and ljos-consensus
(voting), plus `packset-embed`, the encoder for meaning-based search.
`--no-embed` leaves the encoder out; search then matches words only and misses
a paraphrase that shares no word with the memory. Where a platform has no
encoder build, the installer warns and goes on without it. `--with-embed` is
still accepted and changes nothing. `ljos doctor` names any part that is missing.
`--version 0.29.0` installs that ljos release instead of the latest, and `--pin packset=0.14.0` holds another repository at a version; a pinned version with no release for the platform builds that version's crate, never a newer one.
With the `gh` CLI signed in, the installer also checks each tarball's GitHub build attestation and prints whether it verified; `--attest` stops the install when one does not.
`ljos-hud`, the desktop pane, is optional, and the doctor lists it as `info`.
An `ljos-policyd` built without phronesis checks commands with its table alone, and the doctor marks its policy row `warn`.
Building from source (`cargo install`, or binstall on a platform with no release tarball) needs Rust 1.89 or newer; `rustc --version` says which you have, and `rustup update stable` gets a current one.
An older Rust fails on several crates and can leave an `ljos-policyd` older than 0.3.0 installed without a word, so the doctor marks that row `no` and names the fix.

Each ljos release from 0.29.0 on attaches the script as `install.sh`.
Read the script before you run it; it is
`scripts/install.sh` in this repository. To install by hand:

```
cargo binstall --locked ljos packset vissue-cli vissue-mcp deedar-cli claimdag-cli ljos-policyd ljos-consensus   # or: cargo install --locked, same list
ljos onboard                                # every coding tool found on this machine: MCP server, hooks and skill
ljos prefer "Tag a release with git push origin TAG; --follow-tags leaves v-tags behind."
ljos rule 'terraform destroy*' --verdict deny --why "Infra teardown is mine to run."
```

`--harness` takes `claude`, `codex`, `grok`, `cursor`, `antigravity`,
`opencode`, `omp`, `hermes`, `windsurf`, `zed`, `vscode`, `claude-desktop`,
`gemini`, `amazonq`, `kiro`, `copilot`, `factory`, `qwen`, `crush`, `cline`,
`auggie`, `amp`, `goose`, `openhands`, `continue`, `roo` or `kilo`.
Cursor's IDE and its `agent` CLI share `~/.cursor/mcp.json`.

Start a new session and ask for something the preference bears on, such as
tagging a release. Before the model reads the prompt, the hook adds:

```
What this seat already knows that bears on this (from the pack, each with its age, lessons oldest first; `ljos search` for more):
- [preference, today] Tag a release with git push origin TAG; --follow-tags leaves v-tags behind.
```

When the agent then reaches for `git push --force origin main`, the runner
asks before it runs. `ljos-policyd` answers first, with the rules it ships,
and asks the same way for `--force-with-lease` onto `main`, `master`,
`develop` or a protected branch:

```
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"git-force-push: it rewrites shared history, so the agent asks you first (seat rule `git-force-push`)"}}
```

`sh -c "cd repo && git push -f origin main"` gets the same answer. A command
only your own rule covers, such as `terraform destroy -auto-approve`, meets
that rule:

```
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Infra teardown is mine to run. (seat rule `terraform destroy*`)"}}
```

The hook shows what you write from the moment it is written: a preference
(`ljos prefer`), a rule (`ljos rule`), and a lesson you type with
`ljos remember`. A lesson the agent proposes at `ljos finish` stays out until
you run `ljos accept ID` in your own terminal; then it shows at once too. An
`ljos remember` that an agent runs from inside its runner is an episode: the
hook shows it when a prompt bears on it, and `ljos due` lists it until a
review promotes it to standing.

The hooks also keep the work on the tracker once a conversation sits on an
issue: after forty tool calls without a word to the seat, they remind it to
record the work. With `LJOS_TRACK_WORK=on` in the environment or in
`~/.config/ljos/env`, a conversation with no issue is told on the first tool
result to open one with `vissue q` and sit, and `Stop` blocks that turn once
if it used tools and never touched the seat. Without it, a conversation that
never sits hears nothing about the tracker. Of the shapes ljos carries, `grok` and
`antigravity` register `Stop`. For `claude` or `codex`, add `Stop` to that
runner's `hook_events` in `~/.config/ljos/harnesses.toml`.

## What you get

- Memory per prompt: one claim per memory, searched with the prompt. A claim
  reaches the agent only when it shares the prompt's words or a judge is sure
  it bears on the prompt. Lessons come back on a review clock, and a newer
  claim supersedes a wrong one instead of editing it.
- A tracker, [vissue](https://github.com/leidarljos/vissue): plain org files
  in a git repository. `ljos sitting ISSUE` opens the work and claims it;
  `ljos finish ISSUE --lesson "..."` closes the loop and files what it
  taught. `ljos accept ID` writes that lesson and records the acceptance.
- A check on shell commands: ljos-policyd's built-in refusals (force push,
  `rm -rf` outside temp, `git reset --hard` and the like, read through `env`,
  `timeout`, `xargs` and other wrappers) and your rules in the pack
  (`ljos rule`), tried on each command a line runs, plus a guard that keeps agents from rewriting the
  checker's own binaries and hook files. An ordinary `git push` of a branch
  is allowed, whichever remote it names and whether or not that remote's
  owner can be read, and so is `git push -u origin BRANCH`. Force
  (`--force`, `-f`, a `+refspec`), deleting a remote branch, and pushing
  tags ask by default, with or without a seat rule. A `--force-with-lease`
  push, with or without `--force-if-includes`, asks onto `main`, `master`,
  `develop` or a branch the checkout protects (`protected_branches` in
  `ljos.toml` or `.ljos/gates.toml`), and runs onto any other branch. With
  no refspec, the guard reads the current branch's upstream, and asks when
  it cannot tell.
  A pattern as wide as `git push*` is refused when a rule is written.
- Decisions with more than one defensible answer go to personas, voters with
  a view of their own. They settle by trust-weighted
  [consensus](https://github.com/leidarljos/consensus) that learns from the
  outcome. A persona can reason in a runner session it keeps.
- Deeds ([deedar](https://github.com/leidarljos/deedar)): content-addressed
  records of what a piece of work produced, cited on the ticket.

## Words

| word | meaning |
|---|---|
| seat | one agent runner on one machine, with its memory and claims |
| pack | the memory store ([packset](https://github.com/leidarljos/packset)), one claim per atom |
| sitting | one piece of work on one ticket, opened by `ljos sitting` and closed by `ljos finish` |
| island | the memories a title activates, and the links between them |
| deed | a signed record of what a piece of work produced |
| card | a file the person froze; agents read it and never write it |
| persona | a voter with a view, an anchor and the domains it speaks to |

## The loop, in two verbs

`ljos sitting ISSUE [--playbook NAME]` opens a sitting. It runs the steps in
the protocol's order and stops at the first store that does not answer:
doctor, cards, the review clock, the island the issue's title activates, the
playbook, the working set, the timeline, and the claim. The playbook gets
copied before recall. Without `--playbook`, the sitting takes a name already
bound, then a closed-set token in the title, then `sit`. The playbook name
lives as a tracker note until finish or release, and `ljos panel` refuses to
run until one is bound.
The sitting prints a short report: whether ljos is ready, what needs action, what the seat remembers for the task, and the claim.
`--verbose` prints every section in full.

`ljos finish ISSUE --lesson "..." [--outcome OPTION]` closes it. The lesson
goes in as a proposal with origin `agent-derived`. `ljos accept` writes it as a
standing lesson and records that the person accepted it. Under an agent runner
it accepts only a proposal another conversation filed, such as a worker's, so
an agent can't accept its own proposal. A lesson you type at `ljos finish` in a
terminal of your own, with no agent runner above it, is written at once as
yours. `ljos accept`, `ljos forget` and `ljos graded` take the first eight
characters of an id. The island fires, the session node
completes, and a named outcome shrinks the weight of the voters it refuted. A
lesson typed with `ljos remember` stays `user-declared`, and the same words
satisfy the open proposal. The ticket stays open unless `--close` is given,
and `--close` closes it only when the work is accepted. The loop runs on
every finish, so the seat remembers whether or not somebody asks it to.

When the tracker is a git checkout, the claim and the finish each commit the
ticket's `issues.org` (that file only) and push it, so another host sees
both. `LJOS_TRACKER_GIT=commit` keeps it local; `off` skips
it. `ljos doctor` counts the commits origin lacks, and it fails the tracker
row when that count stays above zero through the push wait.

## Claude Code plugin

The plugin bundles the tool server (`ljos-mcp`), the skills (the seat protocol, setup,
remember, sitting, filing, decisions and mail), the sitting, finish, remember
and doctor commands, a panelist agent, and the hooks
`ljos onboard --harness claude` registers. The marketplace file is in this
repository, and the same tree is the Cursor plugin below.

```
claude plugin marketplace add leidarljos/ljos
claude plugin install ljos@leidarljos
claude plugin install vissue@leidarljos
```

`ljos@leidarljos` is this repository. `vissue@leidarljos` is the tracker
server, fetched from `leidarljos/vissue`. `ljos`, `ljos-mcp`, and `vissue-mcp`
stay on `PATH`; the `install.sh` line in Five minutes puts them and
packset, deedar, claimdag, consensus and ljos-policyd in `~/.local/bin`.
The plugins run those binaries and also look in `~/.cargo/bin` and
`~/.local/bin`. Start a new
session. `/ljos:sitting ISSUE` opens a sitting and
`/ljos:finish ISSUE --lesson "..."` closes it. In a checkout,
`claude plugin validate .` checks the marketplace and
`claude plugin validate .claude-plugin/plugin.json` checks the plugin. Pass
`--strict` to fail on warnings.

## Cursor plugin

This repository is also a Cursor plugin. It gives Cursor's agent the ljos
memory, the command guard, the `ljos_*` tools, the skills above, a rule that tells
the agent to use them, and four commands: `/sitting`, `/finish`, `/remember`
and `/doctor`.

Install the programs first (the `install.sh` line in Five minutes), then
type this in Cursor's agent chat:

```
/add-plugin leidarljos/ljos
```

Cursor pins a plugin added this way to the commit it first fetched. To follow
updates, put a clone in Cursor's local plugin folder instead and pull it:

```
git clone https://github.com/leidarljos/ljos ~/.cursor/plugins/local/ljos
```

Then reload the window. Cursor loads a symlink in that folder only when it
points inside the folder, so clone rather than link.

The hooks run the same `ljos hook` as `ljos onboard --harness cursor`. Two of
them, `beforeShellExecution` and `preToolUse`, check each command before it
runs. A third, `beforeSubmitPrompt`, looks up what the seat knows. Cursor does not pass a
prompt hook's text to the model, so that note reaches the agent with the next
tool result. With the plugin installed, `ljos onboard` sees it and leaves
Cursor to the plugin, so the hooks are not registered a second time. When the programs are missing, the
hooks say how to install them and let commands run, and the ljos-setup skill
walks the agent through the install.

## Command consent in chat

Grok Build and Claude show an ask rule as the runner's own permission
prompt. The hook answers `ask`, and approving that prompt runs the command.

On a client without a native hook prompt, an ask rule returns a request id.
Call the MCP tool `ljos_request_approval` with that id. A client with MCP form
elicitation displays the exact command, directory, conversation and rule.
Select **Allow this command once** and accept the form to authorize one
retry. The grant expires fifteen minutes after the request was created.

Declining, dismissing, leaving the checkbox clear, or a client error grants
nothing. The tool takes no approval flag as an argument. Each different
command, directory, conversation or rule needs its own consent, and deny
rules still apply.

For clients without form elicitation, reply `approve REQUEST_ID` in this
conversation, or run `ljos approve REQUEST_ID` in your own terminal. The
terminal command refuses to run under a coding runner, and the seat's guard
refuses it in any command an agent runs. A chat message that does not name
that id grants nothing.

A client must load the tool before using it, so reconnect after upgrading
from a version without `ljos_request_approval`. When other calls forward to an
upgraded binary, a connection that already has this tool keeps its own
consent handler.

Verify the protocol against a built server with:

```sh
python3 scripts/test_approval.py target/debug/ljos-mcp
```

## Who is sitting

The seat is the program that connected. `ljos-mcp` names it after the client
that initialised it. `ljos` in a shell that runner opened finds the same name
through the process tree or a shared session id. A runner's tools and its
verbs are then one seat, with no variable set.

A shell-only agent such as Grok Bot has no MCP client and no command hook,
and the process tree does not name its seat. `ljos onboard --harness grokbot`
writes the skill and an env file; source the file and `LJOS_SEAT` is the
name. `ljos hook --prompt` is the prompt hook. `ljos policy --fail-on-deny`
exits 1 on a deny. Without the flag, `ljos hook` still exits 0.

Claims belong to one conversation; memory, ballots and trust accrue to the
seat. `ljos seat` prints both names and where they came from. `ljos onboard`
prints the one MCP entry any runner takes, and `ljos protocol` prints the
text an agent reads first.

Nothing needs a variable set. `ljos onboard --harness NAME` makes the deed
store deedar falls back to (`~/.local/share/deedar/store`) and a tracker under
`~/.local/share/vissue/tracker` that `~/.config/vissue/config.toml` names,
unless `DEEDAR_URL` or `VISSUE_ROOT` already names one. `ljos remember`
starts the writer when none is answering. The seat's memory is the one
workspace `seat` from any directory, and every claim carries the seat that
wrote it.

## Mail

`ljos send SEAT TEXT` writes a message to a named seat.
`ljos send --group NAME TEXT` writes to the other members of that group.
`--interrupt` puts the message at the top of the next prompt. `--issue ID`
threads the message on a vissue issue. The atom then takes that issue's scope,
so `ljos sync` carries it in the sealed log.

`ljos inbox` lists unread mail and writes no receipt. A pack that does not
answer is an error, and `ljos send` names the packset version that keeps
mail. `ljos read ID` writes one receipt. `ljos reply ID TEXT` answers the
sender and keeps the issue. `ljos hook --prompt` prints unread mail and writes
a receipt for each message it shows. When the pack does not answer, it prints
`mail could not be checked` and still exits 0. A shell seat polls
`ljos inbox`.

## Decision panel concurrency

A decision panel runs at most `LJOS_PANEL_CONCURRENCY` members at once, and
the next starts as one exits. Unset, that is the machine's parallelism clamped
to 4. `LJOS_MAX_PARALLEL` is the same knob. `0` starts every member.

## Commands

Citing a deed names it; the bytes stay in deedar. A finish does not close the
ticket, and completing a node does not either. `finish --close` closes it,
when the work is accepted. Without `--close`, a finish drops the claim and
puts the ticket back to the state it had before the claim, as `ljos release`
does, so no ticket is left STARTED with nobody holding it.

```
ljos sitting vissue-xxxx --playbook sit        # doctor, cards, due, island, playbook, recall, timeline, claim
ljos finish vissue-xxxx --lesson "..." [--outcome ship]   # file the lesson, fire, complete, learn
ljos accept <id>                                   # write a filed proposal into the pack
ljos calibrate -p project                      # trust rows from the voting history, no truth labels
ljos remember "the default fuse is CombMNZ"
ljos prefer "CombMNZ over RRF"
ljos search fuse
ljos island "rebuild the packset site"
ljos evidence deed-…
ljos deed vissue-xxxx --add deed-…
ljos recall vissue-xxxx
ljos claim <node>                              # the session node, and the tracker issue to STARTED under the same name
ljos claim --next [--role <role>]              # the first ready node in claimdag's order for you
ljos release <node>
ljos seat                                      # who is sitting: the seat, this conversation's holder, and where the names came from
ljos complete <node> --status done
ljos cards
ljos policy -- ls
ljos approve <request-id>                       # record explicit consent for one stopped command
ljos consensus vissue-xxxx
ljos trust alice bob 0.8 --why deed-… [--about docs]
ljos persona reviewer --anchor 0.2 --view "Reads for what breaks in production." --about release
ljos playbooks                                 # sit, arena, land, company-panel, overnight, feature
ljos playbook vissue-xxxx company-panel        # bind a recipe; sitting copies the body before recall; panel refuses until then
ljos vote vissue-xxxx --for hold --expect ship --confidence 0.6 --used none
ljos predict vissue-xxxx --expect ship          # the same forecast on its own; two forecasts and consensus names the surprisingly popular answer
ljos rule 'terraform destroy*' --verdict deny --why "Destroy only from the runbook."   # argv law in the pack; the hook and policy enforce it (see "What a rule reads" in the reference)
ljos brief reviewer vissue-xxxx                # what a subagent playing reviewer starts from
ljos remember --as reviewer "..."              # a lesson the persona keeps; its next brief opens with it
ljos panel vissue-xxxx --out panel             # every persona's brief as a file, for a runner without MCP
ljos vote vissue-xxxx --for hold --as reviewer
ljos panel vissue-xxxx --jev                   # with Jev on: cast when every seat is sure and agrees, else briefs for subagents
ljos learn vissue-xxxx --outcome ship
ljos due
ljos hud                                       # execs sibling ljos-hud (overlay; P pops out)
ljos habit mab-cr-all 0.579 --unit acc --every 7d --source 11793   # a reading; the one before closes and is kept as what it was
ljos habit                                     # every habit as it stands, the change since the last reading, when the next is due
ljos graded <atom-id> [--lapsed]
ljos handover --out bag --project x --issue vissue-xxxx
ljos receive bag [--since bridge.txt] [--import]
ljos doctor                                    # whether ljos is ready, and only the rows that need action
ljos doctor --all                              # every row
ljos protocol
ljos mcp                                       # serve MCP over stdio through the ljos-mcp beside this ljos
ljos onboard                                   # at a terminal, every runner found on this machine; into a pipe, the one MCP server entry any runner takes
ljos onboard --harness all --dry-run           # what onboarding every runner found would write
ljos onboard --harness json                    # the one MCP server entry any runner takes
ljos onboard --harness hermes                  # register with a runner named in harnesses.toml; opencode, hermes, omp, grok, antigravity, cursor come as shapes
ljos onboard --harness grokbot --skills ~/.grokbot/skills   # shell-only: skill and an env file, no MCP server and no hook
source ~/.config/ljos/grokbot.env              # LJOS_SEAT=grokbot; the process tree is not this seat
echo "$prompt" | ljos hook --prompt            # the prompt hook, from a shell
ljos policy --fail-on-deny -- git push --force # exit 1 on a deny; without the flag the exit stays 0
ljos findings out/campaign.json --remember     # file each resolved finding as a proposal
ljos group ops --add bob                       # a group of seats; membership is a pack atom
ljos send bob "the build is red" --issue acme-4 --interrupt
ljos send --group ops "the build is red"
ljos inbox                                     # unread mail; listing writes no receipt. hook --prompt marks what it shows read
ljos read <id>                                 # the receipt the sender sees
ljos reply <id> "looking"                      # back to the sender, on the same issue
```

## MCP

`ljos-mcp` serves the same verbs over stdio, and so does `ljos mcp`, which runs
the `ljos-mcp` installed beside it. By default it lists twenty core tools,
guard and memory first: `rule`, `request_approval`, `prefer`, `remember`,
`search`, `start`, `guard_check`, `file`, `note`, `recall`, `sitting`,
`finish`, `claim`, `release`, `send`, `inbox`, `reply`, `vote`, `consensus` and
`doctor`, each with the prefix `ljos_`.
Every listed tool costs its description and schema on every session: about
8,200 tokens for the core set against 17,800 for all forty-nine (o200k_base
count of `initialize` instructions and the `tools/list` result). `LJOS_MCP_TOOLS=all` in the server's environment lists every tool;
`LJOS_MCP_TOOLS=core,island,due` adds named ones. A tool that fails answers
with a result marked `isError` that carries the reason, so the model reads
it. The server also has the protocol at `ljos://protocol`, the cards
read-only at `ljos://cards/`, and three prompts. `ljos_due` returns the
soonest eight claims and the total still due. Paste this where the runner
keeps its servers:

```json
{"mcpServers": {"ljos": {"command": "ljos-mcp"}}}
```

The official MCP Registry lists the server from the crates.io page of
`ljos`, and checks this line there:

- mcp-name: io.github.leidarljos/ljos

`server.json` holds the registry entry, and a release tag publishes it.
`Dockerfile` builds `ljos-mcp` and starts it, for a registry that runs the
server to list its tools.

[`examples/unanimous-green/`](examples/unanimous-green/) shows two
implementers and two reviewers reaching a unanimous `GREEN` from
`ljos consensus`.

A five-persona panel chose this seat's tool adapters. Its ballots, what each
persona measured and both readings of the settle, before and after `learn`,
are in [`examples/runtime-panel/`](examples/runtime-panel/).

## The smoke test

`scripts/smoke.sh` runs every loop on scratch stores. It covers memory,
agreement, learning, a sitting and its finish, and a rewrite that closes its
earlier claim. It also covers habits, an as-of read, and a signed handover,
received and then refused after one byte changes. A rule goes through the
hook and through `policy`.

Then two seats, each with its own `LJOS_SEAT` and `*_SESSION_ID`, hand one
ticket along in sequence. A sits, cites a deed and hands over; B imports; A
releases; B sits; both vote; consensus runs; the finish closes the ticket;
tracker git reports the commit. Last, `scripts/herd.sh` runs eight sittings
on one contended ticket, eight finishes and one island fire.
`scripts/terra/` has the Slurm scripts that build, smoke and herd the seat on
a cluster, as run.

## Optional: Jev

The seat makes every judgment locally by default. You can hand those
judgments to Jev, TypeSafe's hosted decision model, on a machine where that
is allowed: write `~/.config/ljos/jev.toml` with `enabled = true` and a key
command. Jev then decides which claims bear on a prompt and whether the prompt corrects
the agent or puts a choice. It also casts a persona's ballot, decides whether
an agent may stop, and checks whether a due claim is still true.

The ballot gets recorded as `judge:<model>`, not as the persona.
`ljos due --judge` names a claim the judges find still true; `ljos graded`
marks it recalled. `ljos judge-score` reads the judge log against outcomes.
The local answer stays as the fallback, and a machine without the file runs
as before. A prompt costs about $0.00004, and `ljos doctor` prints the
month's spend. A chat backend has to name `usd_per_mtok_in`. The section on
judgment in the explanation says how Jev fits with what the seat already
does; the how-to has the setup.

## Documentation

The org site teaches sitting: <https://leidarljos.github.io>. The crate site
is Org plus Sphinx: <https://leidarljos.github.io/ljos/>.

| Page | What it answers |
|---|---|
| [First write](https://leidarljos.github.io/docs/start/) | Install, remember one sentence, search it back |
| [Sit](https://leidarljos.github.io/docs/sit/) | `sitting` opens, `finish` closes |
| [Runners](https://leidarljos.github.io/docs/harnesses/) | One entry for any runner; the seat names itself |
| [Policy](https://leidarljos.github.io/docs/policy/) | Which shell commands are refused, by which layer, and how the person approves one |
| [Getting started](https://leidarljos.github.io/ljos/getting-started.html) | Memory, agreement, and a sitting on scratch stores |
| [How-to](https://leidarljos.github.io/ljos/howto.html) | Hook, onboard, habits, handover, trust |
| [Reference](https://leidarljos.github.io/ljos/reference.html) | Every verb, tool and variable |
| [Explanation](https://leidarljos.github.io/ljos/explanation.html) | The contracts, the memory model, time as data, agreement that learns |
