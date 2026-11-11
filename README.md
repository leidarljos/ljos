# ljos

ljos gives coding agents a shared memory and a shared tracker on one person's machines.
A person writes what the agents should remember, and the prompt hook hands it back when a prompt bears on it.
Agents file and claim tickets in one git repository, and a held ticket refuses a second claim.
When agents disagree, they vote, and ljos settles the vote.
It calls no model, and every store is a file a person can open.
`ljos onboard` knows ten runners, among them Claude Code, Codex and Cursor.

[//]: # ([[[cog import ljos_version as v; v.lines("These notes describe ljos {version}.")]]])
These notes describe ljos 0.41.1.
[//]: # ([[[end]]])

## What it has done

- 7,900 tokens for the 20 core Model Context Protocol (MCP) tools, against 17,854 for all 52 ([measurements/mcp-size.txt](measurements/mcp-size.txt)).
- 139.1 ms warm prompt hook median on 10,000 live lessons, down from 196.3 ms, with the same output ([measurements/hook-latency.txt](measurements/hook-latency.txt), `prompt-hook.rows-10000.after`).
- The duplicate check merged 0 of 186 near-topic lesson pairs and found 12 of 78 duplicate pairs ([measurements/lesson-duplicates.txt](measurements/lesson-duplicates.txt)).
- packset, the memory writer, on LongMemEval_S session retrieval over 470 questions: recall@5 0.914 with BM25+ alone, 0.949 with BM25+ fused with e5-large-v2 ([packset explanation](https://leidarljos.github.io/packset/explanation.html)). This scores retrieval, not answer accuracy.
- 2,293 tests passed, 0 failed, 2 ignored on the 0.40.0 release commit ([CI run 42801686714](https://github.com/leidarljos/ljos/actions/runs/42801686714)).

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
```

The hook prints the preference ahead of the prompt:

```
Remembered, oldest first (`ljos search` for more):
- [preference, today] Push a tag with git push origin TAG; --follow-tags skips lightweight tags.
```

The installer puts the programs in `~/.local/bin` and checks each one against its published SHA-256 sum.
It sets up the tracker, the memory writer and the deed store. Then it runs `ljos onboard` for each runner it finds.
`--no-embed` leaves out the encoder, and search then matches words.
Claude Code runs a hook only after you trust it: open `/hooks` there after `ljos onboard`.

To install by hand:

```
cargo binstall --locked ljos packset vissue-cli vissue-mcp deedar-cli claimdag-cli ljos-policyd ljos-consensus
ljos onboard
ljos doctor
```

`ljos offboard` takes ljos out of each runner, and `ljos uninstall` takes it off the machine.
[Getting started](https://leidarljos.github.io/ljos/getting-started.html) has both, and a ten-minute tour.

## Two seats on one ticket

```
LJOS_SEAT=alice ljos seat add alice
LJOS_SEAT=alice ljos seat add bob
id=$(LJOS_SEAT=alice ljos file "Pick a fuse" -p demo -t decision --options keep,drop | awk 'NR==1 { print $1 }')
LJOS_SEAT=alice ljos claim "$id" --assignee alice
LJOS_SEAT=alice ljos vote "$id" --for keep --used none
LJOS_SEAT=bob ljos note "$id" "the patch is one file"
LJOS_SEAT=bob ljos recall "$id"
LJOS_SEAT=alice ljos consensus "$id"
```

A second claim on a held ticket prints `label claim-conflict:` and names the holder.
`ljos recall` shows bob's note, labelled as unverified text from bob.
`ljos consensus` settles the ballots by the trust rows in the pack, and `ljos learn ID --outcome OPTION` moves those rows toward the voters who turned out right.

## The command check

Before an agent's shell command runs, the runner's hook asks `ljos-policyd`, which answers allow, warn or rewrite.
A warning names the rule and the command runs; ljos is not a sandbox.
For `git push --force` on a topic branch the hook answers:

```
{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"allow\twarn\tgit-force-push\ta force push rewrites a published branch"}}
```

## The MCP server

`ljos-mcp` serves the same verbs over stdio. By default it lists twenty core
tools, guard and memory first: `rule`, `prefer`, `remember`,
`search`, `start`, `guard_check`, `file`, `options`, `note`, `recall`, `sitting`,
`finish`, `claim`, `release`, `send`, `inbox`, `reply`, `vote`, `consensus` and
`doctor`, each with the prefix `ljos_`. `measurements/mcp-size.txt` records
7,900 tokens for the core set against 17,854 for all fifty-two.
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
| [Getting started](https://leidarljos.github.io/ljos/getting-started.html) | Install, two seats on one ticket, offboard |
| [How-to guides](https://leidarljos.github.io/ljos/howto.html) | One task a page: runners, memory, sittings, votes, handover |
| [Reference](https://leidarljos.github.io/ljos/reference.html) | Commands, MCP tools, hooks, rules and variables |
| [Explanation](https://leidarljos.github.io/ljos/explanation.html) | The reason for the design |
| [How ljos compares](https://leidarljos.github.io/ljos/compare.html) | beads, Mem0, Letta, Zep, Basic Memory, git-bug |

Questions and bugs go to [GitHub issues](https://github.com/leidarljos/ljos/issues).
[CONTRIBUTING.md](CONTRIBUTING.md) covers changes, and [CITATION.cff](CITATION.cff) says how to cite ljos.
The licence is in [LICENSE](LICENSE).
