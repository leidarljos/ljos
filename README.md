# ljos

ljos is memory, a shared tracker and a vote for coding agents.
A person writes what the agents should remember.
The prompt hook hands it back when a prompt bears on it.
Agents file and claim work in one git repository of tickets.
A held ticket refuses a second claim.
When agents disagree, they vote, and ljos settles the vote.
It calls no model, and every store is a file a person can open.

[//]: # ([[[cog import ljos_version as v; v.lines("These notes describe ljos {version}.")]]])
These notes describe ljos 0.40.0.
[//]: # ([[[end]]])

## What it has done

- 7,901 tokens for the 20 core Model Context Protocol (MCP) tools, against 17,855 for all 52 ([measurements/mcp-size.txt](measurements/mcp-size.txt)).
- 196.3 ms warm prompt hook median on 10,000 live lessons, down from 602.5 ms, with the same output ([measurements/hook-latency.txt](measurements/hook-latency.txt)).
- The duplicate check merged 0 of 186 near-topic lesson pairs and found 12 of 78 duplicate pairs ([measurements/lesson-duplicates.txt](measurements/lesson-duplicates.txt)).
- Recall by ticket tags: labelled memories shown, median 0 before and 5 after; wall median 99.1 ms before, 99.5 ms after ([measurements/recall-tags.txt](measurements/recall-tags.txt)).
- packset, the memory writer, on LongMemEval_S session retrieval over 470 questions: recall@5 0.914 with BM25+ alone, 0.949 with BM25+ fused with e5-large-v2 ([packset explanation](https://leidarljos.github.io/packset/explanation.html)).
- 2,293 tests passed, 0 failed, 2 ignored on the 0.40.0 release commit ([CI run 42801686714](https://github.com/leidarljos/ljos/actions/runs/42801686714)).

The LongMemEval_S line scores retrieval, not answer accuracy.
[What ljos does](https://leidarljos.github.io/ljos/introduction.html) lists every measured number with its method.

## How it compares

- Memory: ljos, beads and Basic Memory keep what was written. Mem0 runs a model over what it ingests by default.
- Tickets: ljos and beads let an agent claim a ticket. git-bug keeps issues inside git.
- Votes: ljos records a ballot per agent and settles them. The docs of the other four describe no vote.
- Stores: ljos uses Org files and JSON lines in git, beads uses Dolt, Mem0 uses Qdrant and SQLite, Basic Memory uses Markdown files, and git-bug uses git objects.
- Licences: ljos and beads are `MIT`, Mem0 is `Apache-2.0`, Basic Memory is `AGPL-3.0`, git-bug is `GPL-3.0`.

[How ljos compares](https://leidarljos.github.io/ljos/compare.html) has the full table, Letta and Zep, and a link to each tool's own docs.

## Try it

Linux and macOS are supported. Windows works through Windows Subsystem for Linux (WSL) and is untested.

```
curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh --no-embed
export PATH="$HOME/.local/bin:$PATH"
ljos doctor
ljos prefer "Push a tag with git push origin TAG; --follow-tags skips lightweight tags."
echo "how do I push the v0.32.0 tag?" | ljos hook --prompt --text
ljos due
```

The installer puts the programs in `~/.local/bin`. It sets up the tracker and the memory writer, then runs `ljos onboard`. `ljos doctor` names any part that does not answer. The preference is stored as written. The hook then prints that preference ahead of the prompt. `ljos due` lists what is ready to grade.
No encoder is downloaded, and search matches words.

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

One command fetches the release programs.
It checks each published SHA-256 sum.
It sets up the tracker, the pack writer and the deed store.
Search matches words.
No encoder is downloaded.

```
curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh --no-embed
export PATH="$HOME/.local/bin:$PATH"
ljos doctor
```

`sh install.sh` with no flag still installs the encoder.
The encoder fetches its model on first use.
`--with-embed` does the same.

The programs go to `~/.local/bin`. The installer checks each download against
its published SHA-256 sum. It also checks the build attestation when `gh` 2.49
or newer, or `cosign`, is there. Where a platform has no release build, cargo compiles
the programs from source. The compile needs Rust 1.89 or newer.

The installer then runs `ljos onboard` for each coding runner it finds. With none
found, it still sets up the tracker, the pack writer and the deed store.

The programs are `ljos`, `ljos-mcp`, `packset`, `vissue`, `deedar`, `claimdag`,
`ljos-policyd` and `ljos-consensus`. `packset-embed` is the encoder for
meaning-based search. `--no-embed` leaves it out, and search then matches words only.

`ljos upgrade` moves each installed program to its latest published
release.
`ljos`, `ljos-mcp` and `ljos-hud` come from the ljos release.
Each other program comes from the repository that publishes it.
One line a program names the version it had and the version it has, or
`current` when those are the same.
A missing release, a failed download or a checksum that does not match
is a warning, and the other programs still move.
`ljos upgrade --only NAME` moves one program.
The previous binary is kept as `NAME.bak-pre-VERSION`.

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

A path outside the checkout still needs the hooks trusted. Claude Code runs a hook only after you trust it: open `/hooks` there and trust each
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

```
ljos uninstall --dry-run
ljos uninstall
```

`ljos uninstall` takes ljos off the machine: it offboards every runner, puts the
global git hooks back, stops the hook server and removes the ljos programs. Where
`ljos` was, it leaves a stub that answers every hook call with an allow, because a
runner that loaded its hook list while ljos was installed keeps calling that path
until it restarts. `--no-stub` leaves nothing, `--all` removes the other seat
programs too, `--purge` removes the state, data and configuration directories. A hand-deleted binary still has the same uninstall script from the release:

```
curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/uninstall.sh && sh uninstall.sh
```

The seat's hooks fail open on every runner: a `ljos` that is missing, crashes or
times out lets the call run, and a check whose hook fails answers allow with a note
that says why nothing judged the call.
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

The hook uses a preference you type, and that preference takes effect at once. A lesson an agent writes at `ljos finish`
is stored at once too, labelled agent-derived and unreviewed, and `ljos accept ID` marks it
reviewed. A yes you said in chat is kept with the acceptance. The seat that heard it runs
`ljos accept ID --quote "..." --at TIME --via SEAT`, and the acceptance keeps those words.

## Two seats on one ticket

Two agents, `alice` and `bob`, each name their seat with `LJOS_SEAT` on the command. Alice adds both seats. One shell can then file, claim, vote, note and read. That shell casts one ballot. Bob writes a note from this shell. His ballot has to come from his own runner.

Seats cast ballots, and `ljos consensus` settles them. With two or more active seats, a protected push needs votes from two model families. A model family is the model a seat's verified record names. A seat with no such record is a family of its own. One active seat settles that check with its own vote. A panel seats each persona the ticket speaks to. Each persona casts its own ballot.

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

A second claim on a held ticket prints `label claim-conflict:` and names the holder. The ticket stays with that holder.
A close with open children prints `label children-open:` and leaves the parent open.
A block that would loop prints `label graph-cycle:` and does not store the edge.
A persona ballot records the casting seat, the parent session and the persona.
Every ballot is recorded.
Ballots count as one voice when they share a session id, a parent, a calling shell or the root session of one agent runner.
When they disagree, the latest ballot is that voice.
`ljos consensus` names the ballots it folded and why, including that shell or agent runner.
A subagent of that root session counts with its parent.
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
what each filed, claimed, voted on, noted and finished. You read the same
facts one at a time with `ljos timeline ISSUE`, `ljos claims` and `ljos log --seat NAME`.

## The command check

Before an agent's shell command runs, the runner's hook hands it to `ljos hook`, which asks `ljos-policyd`.
policyd answers allow, warn or rewrite.
A warning names the rule and the command runs; ljos is not a sandbox.
`ljos policy -- COMMAND` prints the answer for one command without running it.
The hook hands the runner this answer for `git push --force` on a topic branch, and the push runs:

```
{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"allow\twarn\tgit-force-push\ta force push rewrites a published branch"}}
```

The [how-to guide](https://leidarljos.github.io/ljos/howto.html) lists the rules, the rewrite and the two checks ljos keeps for the tracker.

## Jev

Jev is an optional hosted model. It is off by default. The host named in
`~/.config/ljos/jev.toml` runs it. Without that file, ljos calls no model.
[What leaves the machine](docs/privacy.md) is the prompt and the candidate memories.

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
named in its `ljos.toml` and `develop`.
A cloud seat cannot force push, delete a remote branch or tag, or push to a branch outside its allowlist.
A local `git branch -d` or `git branch -D` runs and prints `allow\twarn\tgit-branch-delete`.
Every other push from a cloud seat runs with a warning.
Opening or merging a pull request is not refused.
`ljos mesh up` joins the Headscale mesh, and `ljos mesh down` leaves it.
[docs/mesh.md](docs/mesh.md) has the key file, the login server and what a failed join prints.
[docs/cloud-vm.md](docs/cloud-vm.md) has the environment file, the push mode and the mesh.

## Commands

```
ljos file "TITLE" -p PROJECT [-t decision --options A,B]
ljos sitting ISSUE            # checks, memories for the task, and the claim
ljos claim ISSUE --assignee NAME
ljos note [ISSUE] "..."
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
ljos memory export --out memories.jsonl
ljos memory import memories.jsonl
ljos memory integrate --all
ljos memory imports
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

`ljos-mcp` serves the same verbs over stdio. By default it lists twenty core
tools, guard and memory first: `rule`, `prefer`, `remember`,
`search`, `start`, `guard_check`, `file`, `options`, `note`, `recall`, `sitting`,
`finish`, `claim`, `release`, `send`, `inbox`, `reply`, `vote`, `consensus` and
`doctor`, each with the prefix `ljos_`. Every listed tool costs its description and
schema on every session. `measurements/mcp-size.txt` records
7,901 tokens for the core set against 17,855 for all fifty-two.
`LJOS_MCP_TOOLS=all` lists every tool. Cards are the resources
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
| [What ljos does](https://leidarljos.github.io/ljos/introduction.html) | Features and every measured number |
| [Getting started](https://leidarljos.github.io/ljos/getting-started.html) | Install, two seats on one ticket, offboard |
| [How-to](https://leidarljos.github.io/ljos/howto.html) | Hooks, onboarding, handover, trust |
| [Reference](https://leidarljos.github.io/ljos/reference.html) | Every verb, tool and variable |
| [Explanation](https://leidarljos.github.io/ljos/explanation.html) | The reason for the design |
| [How ljos compares](https://leidarljos.github.io/ljos/compare.html) | beads, Mem0, Letta, Zep, Basic Memory, git-bug |
