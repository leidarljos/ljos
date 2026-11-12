# ljos

ljos gives coding agents a shared memory and a shared tracker on one person's machines.
A person writes what the agents should remember, and the prompt hook hands it back when a prompt bears on it.
Agents file and claim tickets in one git repository, and a held ticket refuses a second claim.
When agents disagree, they vote, and ljos settles the vote.
It calls no model, and every store is a file a person can open.
`ljos onboard` knows ten runners, among them Claude Code, Codex and Cursor.

[//]: # ([[[cog import ljos_version as v; v.lines("These notes describe ljos {version}.")]]])
These notes describe ljos 0.41.2.
[//]: # ([[[end]]])

## What it has done

- 7,946 tokens for the 20 core Model Context Protocol (MCP) tools, against 17,900 for all 52 ([measurements/mcp-size.txt](measurements/mcp-size.txt)).
- 139.1 ms warm prompt hook median on 10,000 live lessons, down from 196.3 ms, with the same output ([measurements/hook-latency.txt](measurements/hook-latency.txt), `prompt-hook.rows-10000.after`).
- The duplicate check merged 0 of 186 near-topic lesson pairs and found 12 of 78 duplicate pairs ([measurements/lesson-duplicates.txt](measurements/lesson-duplicates.txt)).
- packset, the memory writer, on LongMemEval_S session retrieval over 470 questions: recall@5 0.914 with BM25+ alone, 0.949 with BM25+ fused with e5-large-v2 ([packset explanation](https://leidarljos.github.io/packset/explanation.html)). This scores retrieval, not answer accuracy.
- 2,293 tests passed, 0 failed, 2 ignored on the 0.40.0 release commit ([CI run 42801686714](https://github.com/leidarljos/ljos/actions/runs/42801686714)).

[What ljos does](https://leidarljos.github.io/ljos/introduction.html) lists every measured number with its method.
No row compares ljos with another memory tool on a shared benchmark. `hook-latency.txt` says so on its `third-party` line.

## Where it sits

Mem0 and Zep run a model over what they ingest. ljos stores the sentence as written.
beads lets an agent claim a ticket and keeps the queue in Dolt. ljos keeps tickets as Org headings in git, and one seat holds a ticket.
git-bug keeps issues in the project's own git objects. ljos keeps a separate tracker repository.
Basic Memory is a shared notebook of Markdown. ljos's memory is short claims with a review clock.
Letta gives the agent tools to edit its own memory. ljos has no model in that loop.
Of these, the ljos docs are the ones that describe a ballot per agent, a settle, and a handover the receiver checks before import.

[How ljos compares](https://leidarljos.github.io/ljos/compare.html) has the full table, the licences, and a link to each tool's own docs, read on 2026-11-10.

## Try it

Linux and macOS are supported. Windows works through Windows Subsystem for Linux (WSL) and is untested.

From a checkout of this repository:

```
sh scripts/install.sh --no-embed
export PATH="$HOME/.local/bin:$PATH"
git config --global user.name "Your Name"
git config --global user.email "you@example.com"
ljos doctor
ljos remember "The lexical default is BM25+. It beat BM25 by two points on turns."
ljos search "lexical default"
ljos file "Pick a codec" -p demo
```

A release attaches that same script:

```
curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh --no-embed
```

The `install.sh` attached to the 0.41.1 release asked for `vissue-cli-<target>.tar.xz`. The 0.41.2 script fetches `vissue-v<version>-<target>.tar.gz`, the archive the vissue release publishes.

`--no-embed` leaves out the encoder. Search then matches words, and `ljos search` prints `ranking is lexical: encoder unavailable`.
The installer checks each download against its published SHA-256 sum. It sets up the tracker, the memory writer and the deed store, then runs `ljos onboard` for each runner it finds.
Git needs `user.name` and `user.email`, because the tracker commits each change. Without them the heading is still written, and the line is `tracker git: commit refused: Author identity unknown`.

On a clean install, `ljos doctor` prints `ljos is ready: memory, tracker and command guard work`.
`ljos remember` prints the lesson and the first eight characters of its id. With no seat set, the write uses `unknown-` plus the host name.
`ljos search "lexical default"` prints that lesson. A query that shares no word with the stored text, such as `which fusion`, prints `No hits.`
`ljos file` prints the new id, then `project:`, the heading, the path, and `tracker git: committed ... no remote, kept local`.

Mem0's default add runs a model over the text. The sentence above is stored as written.
The ticket is an Org heading in a git repository, which you can open. beads stores the same kind of queue in Dolt.

[Getting started](https://leidarljos.github.io/ljos/getting-started.html) has the output from one run of these commands, then a second seat on the ticket.

To install by hand:

```
cargo binstall --locked ljos packset deedar-cli claimdag-cli vissue-cli vissue-mcp ljos-policyd ljos-consensus
ljos onboard
ljos doctor
```

`ljos offboard` takes ljos out of each runner. With no runner onboarded it prints `ok offboard nothing installed by ljos onboard`. `ljos uninstall` takes the programs off the machine.

## A second seat

```
LJOS_SEAT=alice ljos claim ID --assignee alice
LJOS_SEAT=bob ljos claim ID --assignee bob
```

Bob's claim prints `label claim-conflict:` and names alice. The ticket stays with alice.
`ljos note` writes on the ticket. `ljos timeline ID` prints that note as unverified tracker text. `vissue show ID` prints the logbook.
Votes cast from one shell share a session, so `ljos consensus` folds them into one voice. A vote from each seat's own runner is its own voice. The getting-started page shows the fold.

## The command check

Before an agent's shell command runs, the runner's hook asks `ljos-policyd`, which answers allow, warn or rewrite.
A warning names the rule and the command runs. ljos is not a sandbox.
`ljos policy -- git push --force origin topic` prints:

```
git push --force origin topic
allow	warn	git-force-push	a force push rewrites a published branch
```

A push onto `main` or `develop` warns under `sitting-gate` until the ticket of the sitting has a settled consensus. With one active seat, that seat's own vote settles it.

## The MCP server

`ljos-mcp` serves the same verbs over stdio. By default it lists twenty core
tools, guard and memory first: `rule`, `prefer`, `remember`,
`search`, `start`, `guard_check`, `file`, `options`, `note`, `recall`, `sitting`,
`finish`, `claim`, `release`, `send`, `inbox`, `reply`, `vote`, `consensus` and
`doctor`, each with the prefix `ljos_`. `measurements/mcp-size.txt` records
7,946 tokens for the core set against 17,900 for all fifty-two.
`LJOS_MCP_TOOLS=all` lists every tool.

```json
{"mcpServers": {"ljos": {"command": "ljos-mcp"}}}
```

The official MCP Registry lists the server from the crates.io page of `ljos`, and checks this line there:

- mcp-name: io.github.leidarljos/ljos

## Documentation

| Page | Answer |
|---|---|
| [What ljos does](https://leidarljos.github.io/ljos/introduction.html) | Features and every measured number |
| [How ljos compares](https://leidarljos.github.io/ljos/compare.html) | beads, Mem0, Letta, Zep, Basic Memory, git-bug |
| [Getting started](https://leidarljos.github.io/ljos/getting-started.html) | Tutorial: install, remember, search, a ticket, a second seat |
| [How-to guides](https://leidarljos.github.io/ljos/howto.html) | One task a page: runners, memory, sittings, votes, handover |
| [Reference](https://leidarljos.github.io/ljos/reference.html) | Commands, MCP tools, hooks, rules and variables |
| [Explanation](https://leidarljos.github.io/ljos/explanation.html) | The reason for the design |

Questions and bugs go to [GitHub issues](https://github.com/leidarljos/ljos/issues).
[CONTRIBUTING.md](CONTRIBUTING.md) covers changes, and [CITATION.cff](CITATION.cff) says how to cite ljos.
The licence is in [LICENSE](LICENSE).
