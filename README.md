# ljos

Claude Code, Codex, Cursor and other coding agents work on one person's repositories. A new session does not keep the last session's lessons. Two agents can claim the same work. Nobody can see who decided what.

ljos is the command you install once on a machine. Each agent gets a seat on a tracker. The tracker is a git repository of tickets. A seat files a ticket and opens a sitting on it. A sitting claims that ticket and gathers the checks and memories for the task.

Seats cast ballots, and `ljos consensus` settles them. With two or more active seats, a protected push needs votes from two model families. A model family is the model a seat's verified record names. A seat with no such record is a family of its own. One active seat settles that check with its own vote. A panel seats each persona the ticket speaks to. Each persona casts its own ballot.

Ballots and votes are separate from memory. A person stores a lesson with `ljos remember`, or a preference with `ljos prefer`. The words are kept as written. At the end of a session, ljos reads the transcript. It keeps up to five sentences that state a rule, a cause, or a fix. Each sentence is a note on the open sitting. The prompt hook does not show that note as a fact. Set `LJOS_EXTRACT=off` to skip the read. With no open sitting, it writes nothing.

A person can also record a grant that lets a seat close tickets. The grant stores the person's quote.

`ljos-policyd` checks a shell command against a short list of dangerous actions. It is not a sandbox. A command that runs as you, outside a tool call, is not checked. If policyd is missing, slow, or crashes, the command runs and ljos logs a warning.

These notes describe ljos 0.35.1.

## Try it

```
curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh
export PATH="$HOME/.local/bin:$PATH"
ljos doctor
ljos prefer "The lexical default is BM25+."
ljos search lexical
ljos due
```

The installer puts the programs in `~/.local/bin`. It sets up the tracker and the memory writer, then runs `ljos onboard`. `ljos doctor` names any part that does not answer. The preference is stored as written. `ljos search` prints what matches. `ljos due` lists what is ready to grade.

## What a seat does

- Two agents do not take the same ticket, because the seat files it and then claims it.
- A seat votes on a decision, and reads how the votes settle.
- A seat writes notes on the ticket, and reads what the other seats wrote.
- A person stores a lesson on purpose. It is kept as written, and it comes back when it bears on a prompt.
- A seat cites deeds, the records of what a piece of work produced.
- A seat hands another seat a signed bundle. The receiver checks the bundle.

Some rules hold everywhere. Citing a deed does not merge anything.
Finishing a claim does not close the ticket. A person closes it.
Cards are notes a person froze. Agents read them and do not write them.
Consensus is a [separate program](https://github.com/leidarljos/consensus).

## Install

```
curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh
export PATH="$HOME/.local/bin:$PATH"
```

The programs go to `~/.local/bin`. The installer checks each download against
its published SHA-256 sum. It also checks the build attestation when `gh` 2.49
or newer, or `cosign`, is there. Where a platform has no release build, cargo compiles
the programs from source. The compile needs Rust 1.89 or newer.

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

`ljos onboard` writes the Model Context Protocol (MCP) entry, the hooks and the skill into each runner it
finds. `ljos onboard --dry-run` shows what it would write. `ljos onboard --example`
lists the runners ljos knows. `ljos doctor` says which part does not answer.

The part that does not answer can be the tracker path. Onboard makes no tracker
inside a checkout of another project. A later `git add -A` commits a tracker left
there. Onboard names the checkout. It asks for a `VISSUE_ROOT` outside the checkout.
A path the checkout already ignores is fine.

Once the checkout path is fine, Codex runs a hook only after you trust it. Open `/hooks` in Codex and trust each
ljos hook after `ljos onboard`, and again after an upgrade.

## Take it off

```
ljos offboard --dry-run
ljos offboard
```

`ljos offboard` undoes what `ljos onboard` wrote. It removes the ljos entries from
each runner's files. It leaves the rest of each file as it was. `--runner NAME`
takes off one runner only. `--dry-run` lists what it would change. Onboard keeps its
manifest and backups under `${XDG_STATE_HOME:-~/.local/state}/ljos/onboard`.
`scripts/fresh-install-check.sh` installs into an empty `HOME`. A full
session runs next. Offboard runs after that session. The last check confirms that `HOME` is clean.

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

The hook uses a preference you type, and that preference takes effect at once. A lesson an agent proposes at `ljos finish`
stays out until you run `ljos accept ID` in your own terminal. A yes you said in chat
counts too. The seat that heard it runs `ljos accept ID --quote "..." --at TIME --via SEAT`,
and the acceptance keeps those words.

## Two seats on one ticket

Two agents, `alice` and `bob`, each name their seat with `LJOS_SEAT` on the command. Alice adds both seats. One shell can then file, claim, vote, note and read. That shell casts one ballot. Bob writes a note from this shell. His ballot has to come from his own runner.

```
LJOS_SEAT=alice ljos seat add alice
LJOS_SEAT=alice ljos seat add bob
id=$(LJOS_SEAT=alice ljos file "Pick a fuse" -p demo -t decision --options keep,drop | awk 'NR==1 { print $1 }')
printf '%s\n' "$id"
LJOS_SEAT=alice ljos claim "$id" --assignee alice
LJOS_SEAT=alice ljos vote "$id" --for keep --used none
LJOS_SEAT=bob ljos note "$id" "the patch is one file"
LJOS_SEAT=bob ljos recall "$id"
LJOS_SEAT=alice ljos consensus "$id"
LJOS_SEAT=alice ljos digest
```

A second claim on a held ticket is refused, and the refusal names the holder.
A persona ballot records the casting seat, the parent session and the persona.
Every ballot is recorded.
Ballots count as one voice when they share a session id, a parent, a calling shell or the root session of one agent runner.
When they disagree, the latest ballot is that voice.
`ljos consensus` names the ballots it folded and why, including that shell or agent runner.
A subagent of that root session is not bound by the agent runner's row.
A seat ballot is its own voice only when the cast has no shell, no agent runner and no earlier row.

The ballot list goes to `ljos consensus`. The command reads the pack's trust rows.
The consensus crate settles the votes. With no rows, every voter weighs the same. `ljos learn ID --outcome OPTION`
moves the rows toward the voters who turned out right.

### Seats

The tracker's `vissue.toml` lists the seats.

```
ljos seat add carol
```

`ljos seat add NAME` writes the seat's row in `vissue.toml`, so nobody edits the file by hand.
`ljos seat` prints the name this agent works under and where the name came from.
That name is `LJOS_SEAT` when it is set. Otherwise ljos derives one from the
runner, and a derived name counts as a seat.

**Grants.** A `close=own` grant lets one named seat close the tickets it filed or has claimed.
An id list in `close=ID[,ID...]` names those tickets, and that seat may close no others.
`--ids-file PATH` reads the ids, one per line.
Without a grant, `ljos finish` leaves the ticket open for the person to close.

```
ljos seat grant SEAT close=own --quote "the person's words" --at ISO_TIME --via RECORDER
ljos seat grant SEAT close=ID[,ID...] --quote "the person's words" --at ISO_TIME --via RECORDER
ljos seat grant SEAT --ids-file PATH --quote "the person's words" --at ISO_TIME --via RECORDER
```

The `vissue.toml` row records the quote, the time, the recorder named by `--via`,
and the tracker commit. Three rules apply:

- A grant edited into `vissue.toml` by hand is ignored.
- A seat cannot grant itself.
- Every close cites the grant record.

**Solo mode.** When `vissue.toml` lists one active seat, that seat's own vote
settles consensus for the push check. With two or more seats, the normal quorum applies.

### What happened

`ljos digest` reads the tracker and says what the seats did:
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
| rewrite | policyd returns a corrected command, and that one runs. |

policyd reads the command as a shell does. It looks through `sh -c`, `env`, `timeout`,
`xargs` and `nohup`, through `$cmd` and pipes, and through quotes. It does not run a
shell and does not read scripts. First deny wins.

policyd refuses some of these commands and asks about the others.

- `sudo`, and `curl` or `wget` piped to a shell, including decoded text piped to a shell.
- `chmod` that sets setuid, and raw disk writes such as `dd of=/dev/sda` and `mkfs`.
- `rm -rf` outside the work tree and `/tmp`, and recursive `chmod` or `chown` on system trees.
- a force push to `main`, `master`, `develop` or a protected branch, and deleting a remote branch.
- `git filter-branch`, `git filter-repo`, `git reflog expire`, and deleting a published ref.
- throwing away uncommitted work with `git reset --hard`, `git clean -f`, `git checkout .` or `git stash clear`.
- reading a named credential file, or sending one over the network.
- writes to the hook files, the policy files and the ljos programs.
- outbound mail whose From is not your own address.

A lease push to a feature branch is allowed. An ordinary push of a branch is allowed.

The hook hands the runner this answer for `git push --force origin main`. The runner asks you:

```
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"git-force-push: it rewrites shared history, so the agent asks you first (ljos rule `git-force-push`)"}}
```

### The rewrite

Some commands are not dangerous, only wrong for this project.
policyd can answer with a fixed command. Its first rewrite runs the command through
`uv run`. The answer is one line of text: `allow`, then `rewrite`, then the new
command, separated by tabs.

The new command runs only when the guard also allows it. Claude Code, Codex, Grok Build
and Cursor's `preToolUse` take it back from the hook and run it. A runner that has no
field for this gets a refusal that names the new command, so the old line never runs.
Each swap also shows a note naming both commands and leaves a row in
`$XDG_DATA_HOME/ljos/rewrite.jsonl`. Set `rewrite = false` in a repository's
`ljos.toml` to run its lines as written. The log keeps a row for a swap it declined too.
policyd keeps a log of what it denies.

### The two checks ljos keeps

Only ljos knows the tracker and the sitting, so it keeps two checks.

- A command or file tool that writes the tracker's files by hand is refused.
  Use `ljos file`, `ljos note`, `ljos vote` and `ljos finish`. Ticket text is data
  and is never read as a command.
- A protected-branch push asks when the current sitting's ticket has no
  settled consensus. It asks. It does not deny. With one active seat, that seat's own vote
  is the consensus for the push check. With two or more seats, the normal quorum applies.

If `ljos-policyd` is missing or slow, the command runs and ljos logs a warning.
`ljos doctor` shows the policy row. `ljos policy -- COMMAND` prints the answer for
one command without running it:

```
$ ljos policy -- rm -rf /usr/share/foo
rm -rf /usr/share/foo
deny: rm-rf-outside-tmp: a recursive delete outside /tmp cannot be undone. ...
```

The command check is separate from the git hook. A git hook books the work. `ljos git-hooks install` adds a post-commit hook. On the
first commit, it opens the sitting or names the one already open, and it notes the
commit's SHA on the held ticket. It never refuses a commit.

## Jev

Jev is TypeSafe's hosted decision model. It is optional and off by default. Turn it on
with `~/.config/ljos/jev.toml`. Without that file, ljos calls no model.

When ljos calls the model, the model does not judge shell commands. Jev answers questions the seat
would otherwise answer with word lists and one subagent per voter. Jev answers which memories, which ballots, and which claims:

- which memories bear on a prompt, and whether the prompt corrects the agent.
- a persona's ballot on a decision, with its confidence and a forecast of the other votes.
- whether an agent has claimed work done beside a failing test when the agent stops.
- whether a memory that is due for review is still true.

A call sends the prompt and candidate memories off the machine. Every answer goes to a
local log. A ballot Jev casts is recorded as `judge:MODEL`, not as a person or a persona,
and counts as advice. When Jev does not answer, ljos uses the local answer.

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
ljos digest
ljos seat                     # who is sitting
ljos seat add NAME
ljos seat grant SEAT close=own --quote "..." --at ISO_TIME --via RECORDER
ljos seat grant SEAT close=ID[,ID...] [--ids-file PATH] --quote "..." --at ISO_TIME --via RECORDER
ljos offboard [--runner NAME] [--dry-run]
ljos remember "the default fuse is CombMNZ"
ljos prefer "CombMNZ over RRF"
ljos search fuse
ljos island "rebuild the site"
ljos due
ljos graded ID [--lapsed]
ljos forget ID --why DEED
ljos accept ID [--quote "..." --at ISO_TIME --via SEAT]
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

## The MCP server

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

The official Model Context Protocol (MCP) Registry lists the server from the crates.io page of `ljos`,
and checks this line there:

- mcp-name: io.github.leidarljos/ljos

`server.json` stores the registry entry, and a release tag publishes it.

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

| Page | Answer |
|---|---|
| [Getting started](https://leidarljos.github.io/ljos/getting-started.html) | Install, two seats on one ticket, offboard |
| [How-to](https://leidarljos.github.io/ljos/howto.html) | Hooks, onboarding, handover, trust |
| [Reference](https://leidarljos.github.io/ljos/reference.html) | Every verb, tool and variable |
| [Explanation](https://leidarljos.github.io/ljos/explanation.html) | The reason for the design |
