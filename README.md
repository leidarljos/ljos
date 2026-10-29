# ljos

One seat over the habitats. Agents that work on the same project file
tickets, claim them, vote, and settle what to do. `ljos-policyd` is the safety
layer under all of it.

This page describes ljos 0.35.0. Items marked "coming in 0.35.0" are not in
0.34.1.

A seat is one agent, or one person, working a tracker. ljos is the one command
and the one MCP server over five habitats. It owns none of them.

| Question | Habitat |
|---|---|
| What is the work, and who agrees? | the tracker, [vissue](https://github.com/leidarljos/vissue) |
| What does this seat know? | the pack, [packset](https://github.com/leidarljos/packset) |
| What did the work produce? | the deed store, [deedar](https://github.com/leidarljos/deedar) |
| Who is working on what right now? | the claim graph, [claimdag](https://github.com/leidarljos/claimdag) |
| How do the votes settle? | [ljos-consensus](https://github.com/leidarljos/consensus) |

Cards are the notes a person froze. Agents read them and never write them.

`ljos-policyd` is not a habitat. It reads each shell command before it runs.
It allows the command, asks you, refuses it, or fixes it in place.

## What a seat does

- Files work on the tracker and claims it, so two agents do not take the same ticket.
- Votes on a decision, and reads how the votes settle.
- Writes notes on the ticket, and reads back what the other seats wrote.
- Remembers on purpose. A lesson is stored as written, and comes back when it bears on a prompt.
- Cites deeds, the records of what a piece of work produced.
- Hands a ticket to another seat as a signed bundle the receiver can check.

Some rules hold everywhere. Citing a deed does not merge anything.
Completing a claim node does not close a ticket. A person closes it.
Cards are read-only. Consensus is a [different crate](https://github.com/leidarljos/consensus).

## Install

```
curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh
export PATH="$HOME/.local/bin:$PATH"
```

The programs go to `~/.local/bin`. The installer checks each download against
its published SHA-256 sum. It also checks the build attestation when `gh` 2.49
or newer, or `cosign`, is there. Where a platform has no release build, it builds
from source with `cargo`, which needs Rust 1.89 or newer.

The installer then runs `ljos onboard` for each coding agent it finds. With none
found, it still sets up the tracker, the pack writer and the deed store.

The programs are `ljos`, `ljos-mcp`, `packset`, `vissue`, `deedar`, `claimdag`,
`ljos-policyd` and `ljos-consensus`. `packset-embed` is the encoder for
meaning-based search. `--no-embed` leaves it out, and search then matches words only.

To install by hand:

```
cargo binstall --locked ljos packset vissue-cli vissue-mcp deedar-cli claimdag-cli ljos-policyd ljos-consensus
ljos onboard
ljos doctor
```

`ljos onboard` writes the MCP entry, the hooks and the skill into each runner it
finds. `ljos onboard --dry-run` shows what it would write. `ljos onboard --example`
lists the runners ljos knows. `ljos doctor` says which part does not answer.

Codex runs a hook only after you trust it. Open `/hooks` in Codex and trust each
ljos hook after `ljos onboard`, and again after an upgrade.

## Take it off

Coming in 0.35.0:

```
ljos offboard --dry-run
ljos offboard
```

`ljos offboard` undoes what `ljos onboard` wrote. It removes the ljos entries from
each runner's files and leaves the rest of each file as it was. `--runner NAME`
takes off one runner only. `--dry-run` lists what it would change. Onboard keeps its
manifest and backups under `${XDG_STATE_HOME:-~/.local/state}/ljos/onboard`.
`scripts/fresh-install-check.sh` installs into an empty `HOME`, runs a full
session, runs `ljos offboard`, and checks that `HOME` is clean.

## Five minutes

```
ljos prefer "Push a tag with git push origin TAG; --follow-tags skips lightweight tags."
echo "how do I push the v0.32.0 tag?" | ljos hook --prompt --text
```

Start a new session and ask for something the preference bears on. Before the model
reads the prompt, the hook adds:

```
Remembered, oldest first (`ljos search` for more):
- [preference, today] Push a tag with git push origin TAG; --follow-tags skips lightweight tags.
```

A preference you type takes effect at once. A lesson an agent proposes at `ljos finish`
stays out until you run `ljos accept ID` in your own terminal.

## Two seats on one ticket

Two agents, `alice` and `bob`, each name their seat with `LJOS_SEAT`.

```
ljos file "Pick a fuse" -p demo -t decision --options keep,drop     # prints the ticket id
ljos claim demo-xxxx --assignee alice                               # one holder per ticket
ljos vote demo-xxxx --for keep --used none                          # as alice
ljos vote demo-xxxx --for keep --used none                          # as bob
ljos consensus demo-xxxx                                            # how the votes settle
ljos note demo-xxxx "the patch is one file"
ljos recall demo-xxxx                                               # the notes, the deeds, the plan
ljos digest                                                         # coming in 0.35.0
```

A second claim on a held ticket is refused, and the refusal names the holder.
A seat votes once per ticket. Naming another session id does not make a second voice.

`ljos consensus` reads the pack's trust rows and settles the votes with the
consensus crate. With no rows, every voter weighs the same. `ljos learn ID --outcome OPTION`
moves the rows toward the voters who turned out right.

### Seats

The tracker's `vissue.toml` lists the seats. Coming in 0.35.0:

```
ljos seat add carol
```

`ljos seat add NAME` writes the seat's row in `vissue.toml`, so nobody edits the file by hand.
`ljos seat` prints the name this agent works under and where the name came from.
That name is `LJOS_SEAT` when it is set. Otherwise ljos derives one from the
runner, and a derived name counts as a seat.

**Grants.** Coming in 0.35.0. A grant with `close=own` lets one named seat close the tickets it filed or has claimed.
Without a grant, `ljos finish` leaves the ticket open for the person to close.

```
ljos seat grant SEAT close=own --quote "the person's words" --at ISO_TIME --via RECORDER
```

The row lives in `vissue.toml`. It holds the quote, the time, the recorder named by `--via`,
and the tracker commit. Three rules apply:

- A grant edited into `vissue.toml` by hand is ignored.
- A seat cannot grant itself.
- Every close cites the grant record.

**Solo mode.** Coming in 0.35.0. When `vissue.toml` lists one active seat, that seat's own vote
settles consensus for the push check. With two or more seats, the normal quorum applies.

### What happened

Coming in 0.35.0, `ljos digest` reads the tracker and says what the seats did:
what each filed, claimed, voted on, noted and finished. Today you read the same
facts one at a time with `ljos timeline ISSUE`, `ljos claims` and `ljos log --seat NAME`.

## What policyd does

Before an agent's shell command runs, the runner's hook hands it to `ljos hook`.
`ljos hook` asks `ljos-policyd` and relays the answer. policyd is the one place
that decides whether a command is dangerous. ljos does not run a second set of
command checks of its own.

policyd gives one of four answers.

| Answer | What happens |
|---|---|
| allow | The command runs. |
| ask | The runner shows its own permission prompt, and you answer. |
| deny | The command does not run, and the reason names the rule. |
| rewrite | policyd returns a corrected command, and that one runs. Coming in 0.35.0. |

It reads the command as a shell does. It looks through `sh -c`, `env`, `timeout`,
`xargs` and `nohup`, through `$cmd` and pipes, and through quotes. It does not run a
shell and does not read scripts. First deny wins.

policyd refuses or asks about:

- `sudo`, and `curl` or `wget` piped to a shell, including decoded text piped to a shell
- `chmod` that sets setuid, and raw disk writes such as `dd of=/dev/sda` and `mkfs`
- `rm -rf` outside the work tree and `/tmp`, and recursive `chmod` or `chown` on system trees
- a force push to `main`, `master`, `develop` or a protected branch, and deleting a remote branch
- `git filter-branch`, `git filter-repo`, `git reflog expire`, and deleting a published ref
- throwing away uncommitted work with `git reset --hard`, `git clean -f`, `git checkout .` or `git stash clear`
- reading a named credential file, or sending one over the network
- writes to the hook files, the policy files and the ljos programs
- outbound mail whose From is not your own address

A lease push to a feature branch is allowed. An ordinary push of a branch is allowed.

For `git push --force origin main`, the hook hands the runner this answer, and the runner asks you:

```
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"git-force-push: it rewrites shared history, so the agent asks you first (ljos rule `git-force-push`)"}}
```

### The rewrite

Coming in 0.35.0. Some commands are not dangerous, only wrong for this project.
policyd can answer with a fixed command. Its first rewrite runs the command through
`uv run`. The answer is one line of text: `allow`, then `rewrite`, then the new
command, separated by tabs.

The new command runs only when the guard also allows it. Claude Code, Codex, Grok Build
and Cursor's `preToolUse` take it back from the hook and run it. A runner that has no
field for this gets a refusal that names the new command, so the old line never runs.
policyd keeps a log of what it denies.

Coming in 0.35.0: a log of rewrites, and a per-repository switch. Setting `rewrite = false`
turns rewriting off for that repository.

### The two checks ljos keeps

Only ljos knows the tracker and the sitting, so it keeps two checks.

- A command or file tool that writes the tracker's files by hand is refused.
  Use `ljos file`, `ljos note`, `ljos vote` and `ljos finish`. Ticket text is data
  and is never read as a command.
- A push onto a protected branch asks when the ticket of the current sitting has no
  settled consensus. It asks. It does not deny. With one active seat, that seat's own vote
  settles it. With two or more, the normal quorum applies.

If `ljos-policyd` is missing or slow, the command runs and ljos logs a warning.
`ljos doctor` shows the policy row. `ljos policy -- COMMAND` prints the answer for
one command without running it:

```
$ ljos policy -- rm -rf /usr/share/foo
rm -rf /usr/share/foo
deny: rm-rf-outside-tmp: a recursive delete outside /tmp cannot be undone. ...
```

A git hook books the work. `ljos git-hooks install` adds a post-commit hook. On the
first commit, it opens the sitting or names the one already open, and it notes the
commit's SHA on the held ticket. It never refuses a commit.

## Jev

Jev is TypeSafe's hosted decision model. It is optional and off by default. Turn it on
with `~/.config/ljos/jev.toml`. Without that file, ljos calls no model.

Jev does not judge shell commands. policyd does that. Jev answers questions the seat
would otherwise answer with word lists and one subagent per voter:

- which memories bear on a prompt, and whether the prompt corrects the agent
- a persona's ballot on a decision, with its confidence and a forecast of the other votes
- whether an agent that wants to stop has claimed work done beside a failing test
- whether a memory that is due for review is still true

A call sends the prompt and candidate memories off the machine. Every answer goes to a
local log. A ballot Jev casts is recorded as `judge:MODEL`, not as a person or a persona,
and counts as advice. When Jev does not answer, the local answer stands.

## Cloud VMs

`ljos cloud up` puts a fresh machine on the shared tracker. `sh scripts/install.sh --cloud`
does the same from a checkout. A cloud seat has one push rule. It may push the branches
named in its `ljos.toml` and `develop`, with no force and no delete. Opening or merging a
pull request is not refused.
[docs/cloud-vm.md](docs/cloud-vm.md) has the environment file and the push mode.

## Commands

```
ljos file "TITLE" -p PROJECT [-t decision --options A,B]
ljos sitting ISSUE            # checks, memories for the task, and the claim
ljos claim ISSUE --assignee NAME
ljos note ISSUE "..."
ljos vote ISSUE --for OPTION --used none
ljos consensus ISSUE
ljos finish ISSUE --lesson "..." [--outcome OPTION]
ljos recall ISSUE
ljos timeline ISSUE
ljos claims
ljos digest                   # coming in 0.35.0
ljos seat                     # who is sitting
ljos seat add NAME            # coming in 0.35.0
ljos seat grant SEAT close=own --quote "..." --at ISO_TIME --via RECORDER   # coming in 0.35.0
ljos offboard [--runner NAME] [--dry-run]   # coming in 0.35.0
ljos remember "the default fuse is CombMNZ"
ljos prefer "CombMNZ over RRF"
ljos search fuse
ljos island "rebuild the site"
ljos due
ljos graded ID [--lapsed]
ljos forget ID --why DEED
ljos accept ID
ljos trust alice bob 0.8 --why DEED
ljos learn ISSUE --outcome OPTION
ljos handover --out DIR --issue ISSUE
ljos receive DIR [--import]
ljos send SEAT "text"
ljos inbox
ljos policy -- COMMAND
ljos onboard
ljos doctor
ljos protocol
```

`remember` and `prefer` write one claim to the pack, stored as written. They do not read
a transcript. `forget` retires a claim and keeps the record that it was held.

## MCP

`ljos-mcp` serves the same verbs over stdio. By default it lists twenty-one core
tools, guard and memory first: `rule`, `request_approval`, `prefer`, `remember`,
`search`, `start`, `guard_check`, `file`, `options`, `note`, `recall`, `sitting`,
`finish`, `claim`, `release`, `send`, `inbox`, `reply`, `vote`, `consensus` and
`doctor`, each with the prefix `ljos_`. Every listed tool costs its description and
schema on every session: 8,062 tokens for the core set against 17,981 for all
fifty-three (o200k_base count of `initialize` instructions and the `tools/list`
result). `LJOS_MCP_TOOLS=all` lists every tool. Cards are the resources
`ljos://cards/USER.md` and `ljos://cards/MEMORY.md`, and the protocol is `ljos://protocol`.

```json
{"mcpServers": {"ljos": {"command": "ljos-mcp"}}}
```

The official MCP Registry lists the server from the crates.io page of `ljos`,
and checks this line there:

- mcp-name: io.github.leidarljos/ljos

`server.json` holds the registry entry, and a release tag publishes it.

## Check an install

```
scripts/fresh-install-check.sh --cargo-target target/release
```

The script installs into an empty `HOME` and onboards. It takes two seats through file,
claim, vote, consensus, note, recall and digest. Then it runs `ljos offboard` and checks that
`HOME` is clean. It prints each step that failed.

## Documentation

The site is <https://leidarljos.github.io>. The crate docs are at
<https://leidarljos.github.io/ljos/>.

| Page | What it answers |
|---|---|
| [Getting started](https://leidarljos.github.io/ljos/getting-started.html) | Install, two seats on one ticket, take it off |
| [How-to](https://leidarljos.github.io/ljos/howto.html) | Hooks, onboarding, handover, trust |
| [Reference](https://leidarljos.github.io/ljos/reference.html) | Every verb, tool and variable |
| [Explanation](https://leidarljos.github.io/ljos/explanation.html) | Why it is built this way |
