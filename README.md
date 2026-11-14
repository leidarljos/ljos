# ljos

ljos stores a lesson. A later prompt shows it when the prompt bears on that lesson.

[//]: # ([[[cog import ljos_version as v; v.lines("These notes describe ljos {version}.")]]])
These notes describe ljos 0.43.0.
[//]: # ([[[end]]])

ljos is installed with two commands. The first fetches install.sh. The second is ljos onboard.

```
curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh --no-embed --no-onboard
export PATH="$HOME/.local/bin:$PATH" && ljos onboard
```

The second command, `ljos onboard`, finds Claude Code, Cursor and Codex on this machine, and any other runner it knows, and registers the MCP server and the skill for each.
One runner is one command.
Each command prints the files it wrote and the next command.
Run it again and those files stay as they are.

Claude Code: `ljos onboard --harness claude`. It writes `~/.claude.json`, the hooks and the skill. Check with `ljos doctor --all`.

Cursor: `ljos onboard --harness cursor`. It writes `~/.cursor/mcp.json`, the hooks and the skill. Check with `ljos doctor --all`.

Codex: `ljos onboard --harness codex`. It writes `~/.codex/config.toml`, the hooks and the skill in `~/.agents/skills`. Check with `ljos doctor --all`.

`ljos onboard --dry-run` prints the steps and writes nothing.
`ljos offboard` removes the registration.
`ljos uninstall` removes the programs.
A pipe with no `--harness` prints the MCP entry as JSON.
A script that should register runners passes `--harness all`.

If install stops, the script names the cause and the command that fixes it.

- No network, or the release download fails: check the connection and run `sh install.sh` again. A version you name is `sh install.sh --version VER`. Archives already on disk are `sh install.sh --asset-dir DIR`.
- The OS or CPU is not Linux or macOS on x86_64 or aarch64: the script prints one line naming those systems and `cargo install --locked` for the seat programs. On Windows, run the script inside WSL.
- `curl` or `wget`, `tar`, or `sha256sum` is missing. On macOS the checksum tool is `shasum`. The script names the missing tool. Install it and run `sh install.sh` again. Programs already unpacked are `sh install.sh --from-dir DIR`.
- A checksum does not match, or the download stops early: the script stops, deletes the partial file, and says to run `sh install.sh` again.
- The install directory is not writable: the script names it and `sh install.sh --bin-dir DIR`.
- The install directory is not on PATH: the script prints the line for bash (`~/.bashrc`), zsh (`~/.zshrc`) and fish (`~/.config/fish/config.fish`). It does not edit those files. `--modify-path` writes the bash and zsh lines. Run the line it prints, then `ljos doctor`.
- An older `ljos` is earlier on PATH: the script names both binaries and prints the `export PATH` line that puts the new one first.
- Running the script again over the same version says it reinstalled that version. An older install says it updated, and names both versions.
- `--version VER` or `--pin REPO=VER` for a release that does not exist says so, names the releases page, and the command `sh install.sh --version VER` or `sh install.sh --pin REPO=VER`. A version the script itself names, when that release is not published and Rust is not installed, installs the latest release of that program and says so. With Rust installed, it builds that version from source.
- A running `ljos-mcp` keeps the previous binary. Restart the assistant, or `kill` the pid the script prints, then run `ljos doctor`.

`ljos onboard` with no coding tool installed says so. It still sets up the ticket tracker, the memory writer, and the deed store. The deed store keeps signed records of the work. Install an agent, then run `ljos onboard`. `ljos onboard --example` lists the tools.

`ljos offboard` after a hand edit that it cannot undo names the file and leaves the ljos entry. Edit that file, delete the ljos entry, and run `ljos offboard` again.

`ljos uninstall` while an assistant still points at ljos names that file. Edit it and delete the ljos entry. The programs are still removed. A stub named `ljos` answers a hook that a running session cached.

After that registration, a first recall does not need a seat, a sitting or a ticket.
One person and one repository are enough.
With no seat named, a write uses `unknown-` and the host name. A session id in this process names the seat instead: the runner plus a short id, such as `claude-k3j9x0a1b2`. An id that exists only on a parent does not.

## Thirty seconds

`kiln.sh` in an empty directory:

```
#!/bin/sh
if [ "${KILN_DOOR:-}" != cold ]; then
  echo "kiln.sh: KILN_DOOR is unset. The door seats only when it is cold." >&2
  exit 1
fi
echo "kiln door seated"
```

`packset ensure` had already started the memory writer.
The shell prompt is `dev@laptop`, and `LJOS_SEAT` is `demo`.
No sitting and no ticket.
The `prompt-origin` label names the process that ran the hook.
The lesson is the indented text under `Remembered`.

![Recall of the kiln lesson, then the door seated](docs/demo/recall.svg)

```
dev@laptop:~$ ./kiln.sh
kiln.sh: KILN_DOOR is unset. The door seats only when it is cold.

dev@laptop:~$ ljos remember 'Seat the kiln door only when KILN_DOOR=cold. A missing setting is a config gap.'
remembered (lesson): Seat the kiln door only when KILN_DOOR=cold. A missing setting is a config gap.
  id fcd167c8, next review 2026-11-14 15:08 +00:00
standing: the hook shows it from the next prompt it bears on

dev@laptop:~$ printf '%s\n' 'seat the kiln door' | ljos hook --prompt --text
label prompt-origin: not stored as the person's words (parent process is `ljos`,
not a known runner)
Remembered, shown 1 of 1, oldest first, so run `ljos search`.
  lesson, from an agent, today
    Seat the kiln door only when KILN_DOOR=cold. A missing setting is a config
    gap.

dev@laptop:~$ KILN_DOOR=cold ./kiln.sh
kiln door seated
```

## Achievements

ljos gives coding agents a shared memory and a shared tracker on one person's machines.
A person writes what the agents should remember, and the prompt hook hands it back when a prompt bears on it.
Agents file and claim tickets in one git repository, and a held ticket refuses a second claim.
When agents disagree, they vote, and ljos settles the vote.
It stores the sentence as written. Search uses a local encoder when one is installed, and a judge sends text out only when a person configures one.
`ljos onboard --example` lists the runners, among them Claude Code, Codex and Cursor.


- 8,057 tokens for the 20 core Model Context Protocol (MCP) tools, against 18,011 for all 52 ([measurements/mcp-size.txt](measurements/mcp-size.txt)).
- 139.1 ms warm prompt hook median on 10,000 live lessons, down from 196.3 ms, with the same output ([measurements/hook-latency.txt](measurements/hook-latency.txt), `prompt-hook.rows-10000.after`).
- The duplicate check merged 0 of 186 near-topic lesson pairs and found 12 of 78 duplicate pairs ([measurements/lesson-duplicates.txt](measurements/lesson-duplicates.txt)).
- packset, the memory writer, publishes a LongMemEval_S session retrieval run ([packset explanation](https://leidarljos.github.io/packset/explanation.html)). That page records the figures. This tree does not. The run scores retrieval, not answer accuracy.
- The ljos BM25+ CPU run on that file is repeated with `sh bench/hf/run.sh`. The summary and the per-question rows are the dataset [ljos LongMemEval_S retrieval](https://huggingface.co/datasets/rgoswami/ljos-longmemeval-s).
- 2,293 tests passed, 0 failed, 2 ignored on the 0.40.0 release commit ([CI run 42801686714](https://github.com/leidarljos/ljos/actions/runs/42801686714)).

[What ljos does](https://leidarljos.github.io/ljos/introduction.html) lists every measured number with its method.
No row compares ljos with another memory tool on a shared benchmark. `hook-latency.txt` says so on its `third-party` line.

## Comparison

Mem0 and Zep run a model over what they ingest. ljos stores the sentence as written.
beads lets an agent claim a ticket and keeps the queue in Dolt. ljos keeps tickets as Org headings in git, and one seat holds a ticket.
git-bug keeps issues in the project's own git objects. ljos keeps a separate tracker repository.
Basic Memory is a shared notebook of Markdown. ljos's memory is short claims with a review clock.
Letta gives the agent tools to edit its own memory. ljos has no model in that loop.
Of these, the ljos docs are the ones that describe a ballot per agent, a settle, and a handover the receiver checks before import.

[How ljos compares](https://leidarljos.github.io/ljos/compare.html) has the full table, the licences, and a link to each tool's own docs, read on 2026-11-10.

## Tutorial

Linux and macOS are supported. Windows works through Windows Subsystem for Linux (WSL) and is untested.

From a checkout of this repository:

<!-- ljos-quickstart:start -->
```
sh scripts/install.sh --no-embed
export PATH="$HOME/.local/bin:$PATH"
cd "$HOME"
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

`--no-embed` leaves out the encoder. Search then matches words, and `ljos search` prints `matched by words (encoder not installed)` and names `cargo binstall --locked packset-embed`. That ranking splits identifiers (`list_scans` matches `list scans`), weighs a title or a tag above the body, and leaves out a row that only shares a common word. When nothing is close, the line is `No close matches for "..."`.
A cold encoder, one the writer has not loaded yet, gets `LJOS_SEARCH_WAIT` seconds (3 when unset). The search is asked again once the model has answered, and the ranking line names the ranker that ordered the printed rows. If the encoder is still loading, one line names the command to rerun and, when the writer sends a remaining time, about how many seconds are left. A failed encoder names `packset stop && packset ensure`. `--json` puts `semantic` or `lexical` in `ranking`. Starting the memory writer starts that load, so the first search is not what starts it. A row whose score is only the word-rank floor, 0.20, is left out.
A lesson `ljos remember` queued because the pack was busy is sent again on `ljos search` and `ljos sitting`. When it is still waiting, that command says so.
The installer checks each download against its published SHA-256 sum. It sets up the ticket tracker, the memory writer and the deed store, then runs `ljos onboard` for each runner it finds. The deed store keeps signed records of the work.
Git needs `user.name` and `user.email`, because the tracker commits each change. Without them the heading is still written, and the line is `tracker git: commit refused: Author identity unknown`.

Run `ljos doctor` from your home directory. In a checkout that has `.ljos/gates.toml`, the config row names that file. On a clean install, `ljos doctor` prints `ljos is ready: memory, tracker and command guard work`, then `Optional extras: ljos doctor --all`. A `seat source` row says where the seat came from. With no seat named and no earlier write, it warns that the read uses every seat. A write that had to pick a seat is `unknown-` plus the host name, and the row names that seat. A `sitting gate` row follows. With no agent runner above the shell it says `not active here`. A missing `ljos.toml` is a warn that names `ljos doctor --fix`. That file is empty, and the defaults stand. The next command is still `ljos remember`.
`ljos remember` prints the lesson and the first eight characters of its id. With no seat set and no session id in this process, the write uses `unknown-` plus the host name, and the line says so. A conversation that already has a seat record writes as that seat. A session id only on a parent does not name the seat.
`ljos search "lexical default"` prints that lesson. A query that shares no word with the stored text, such as `which fusion`, prints `` No hits for "which fusion", so run `ljos search fusion`. ``
This seat's hits come first. Another seat's lesson is kept when it names the subject, or when the encoder's cosine with the query is at least 0.66. A weaker cross-seat hit does not fill a short list. When those were the only rows, the line is `Nothing relevant was found.` A busy pack ranks by words and still returns a hit that names the subject.
`ljos file` prints the new id, then `project:`, the heading, the path, and `tracker git: committed ... no remote, kept local`.

Mem0's default add runs a model over the text. The sentence above is stored as written.
The ticket is an Org heading in a git repository, which you can open. beads stores the same kind of queue in Dolt.

[Getting started](https://leidarljos.github.io/ljos/getting-started.html) has the output from one run of these commands, then a second seat on the ticket.

To install by hand, with `cargo-binstall` already on `PATH`:

```
cargo binstall --locked ljos packset deedar-cli claimdag-cli vissue-cli vissue-mcp ljos-policyd ljos-consensus
ljos onboard
ljos doctor
```
<!-- ljos-quickstart:end -->

`ljos offboard` takes ljos out of each runner. With no runner onboarded it prints `ok`, `offboard`, `nothing installed by ljos onboard`. `ljos uninstall` takes the ljos programs off the machine and leaves a stub named `ljos` that allows hook calls.

## A second seat

```
LJOS_SEAT=alice ljos claim ID --assignee alice
LJOS_SEAT=bob ljos claim ID --assignee bob
```

Bob's claim prints `label claim-conflict:`, names alice, and tells him to run `ljos send`. The ticket stays with alice.
`ljos note` writes on the ticket. `ljos timeline ID` prints that note as unverified tracker text. `vissue show ID` prints the logbook.
Votes cast from one shell are one voice. `ljos consensus` says they were folded on that shell. A vote from each seat's own runner is its own voice. The getting-started page shows the fold.

## How-to

[How-to guides](https://leidarljos.github.io/ljos/howto.html) is one task a page: runners, memory, sittings, votes, handover. [Two agents on one repository](https://leidarljos.github.io/ljos/howto-agents.html) is the page for more than one session on that repository.

## The command check

Before an agent's shell command runs, the runner's hook asks `ljos-policyd`, which answers allow, warn or rewrite.
A warning names the rule and the command runs. ljos is not a sandbox.
`ljos policy -- git push --force origin topic` prints:

```
git push --force origin topic
allow	warn	git-force-push	a force push rewrites a published branch
```

A push onto `main` or `develop` warns under `sitting-gate` until the ticket of the sitting has a settled consensus. When `vissue.toml` lists one active seat, that seat's own vote is enough for the push. With no seat list, the push waits for two seats.

## The MCP server

`ljos-mcp` serves the same verbs over stdio. By default it lists twenty core
tools, guard and memory first: `rule`, `prefer`, `remember`,
`search`, `start`, `guard_check`, `file`, `options`, `note`, `recall`, `sitting`,
`finish`, `claim`, `release`, `send`, `inbox`, `reply`, `vote`, `consensus` and
`doctor`, each with the prefix `ljos_`. `measurements/mcp-size.txt` records
8,057 tokens for the core set against 18,011 for all fifty-two.
`LJOS_MCP_TOOLS=all` lists every tool.

```json
{"mcpServers": {"ljos": {"type": "stdio", "command": "ljos-mcp", "args": [], "env": {}}}}
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
| [Two agents on one repository](https://leidarljos.github.io/ljos/howto-agents.html) | The seat each session gets, mail between them, and how they split the tickets |
| [Reference](https://leidarljos.github.io/ljos/reference.html) | Commands, MCP tools, hooks, rules and variables |
| [Explanation](https://leidarljos.github.io/ljos/explanation.html) | The reason for the design |

Questions and bugs go to [GitHub issues](https://github.com/leidarljos/ljos/issues).
[CONTRIBUTING.md](CONTRIBUTING.md) covers changes, and [CITATION.cff](CITATION.cff) says how to cite ljos.
The licence is in [LICENSE](LICENSE).
