# ljos

One seat over five habitats and argv law. Each habitat keeps its own crate.

```
cargo binstall ljos
```

That installs `ljos` and `ljos-mcp`.

Citing a deed names it; the bytes stay in deedar. A finish with status done closes the ticket; completing a node alone does not. Cards are read-only. Consensus is a [different crate](https://github.com/leidarljos/consensus).

```
ljos sitting vissue-xxxx                       # doctor, cards, due, island, recall, timeline, claim
ljos finish vissue-xxxx --lesson "..." [--outcome ship]   # remember, fire, complete, learn
ljos calibrate -p project                      # trust rows from the voting history, no truth labels
ljos remember "the default fuse is CombMNZ"
ljos prefer "CombMNZ over RRF"
ljos search fuse
ljos island "rebuild the packset site"
ljos evidence deed-…
ljos deed vissue-xxxx --add deed-…
ljos recall vissue-xxxx
ljos claim <node>
ljos release <node>
ljos seat                                      # who is sitting: the seat, this conversation's holder, and where the names came from
ljos complete <node> --status done
ljos cards
ljos policy -- ls
ljos consensus vissue-xxxx
ljos trust alice bob 0.8 --why deed-… [--about docs]
ljos persona reviewer --anchor 0.2 --view "Reads for what breaks in production." --about release
ljos predict vissue-xxxx --expect ship          # forecast the others; two forecasts and consensus names the surprisingly popular answer
ljos rule '*--force*' --verdict deny --why "Never force push."   # argv law in the pack; the hook and policy enforce it
ljos brief reviewer vissue-xxxx                # what a subagent playing reviewer starts from
ljos remember --as reviewer "..."              # a lesson the persona keeps; its next brief opens with it
ljos panel vissue-xxxx --out panel             # every persona's brief as a file, for a runner without MCP
ljos vote vissue-xxxx --for hold --as reviewer
ljos learn vissue-xxxx --outcome ship
ljos due
ljos hud                                       # execs sibling ljos-hud (overlay; P pops out)
ljos habit mab-cr-all 0.579 --unit acc --every 7d --source 11793   # a reading; the one before closes and is kept as what it was
ljos habit                                     # every habit as it stands, the change since the last reading, when the next is due
ljos graded <atom-id> [--lapsed]
ljos handover --out bag --project x --issue vissue-xxxx
ljos receive bag [--since bridge.txt] [--import]
ljos doctor
ljos protocol
ljos onboard                                   # the one MCP server entry any runner takes
ljos onboard --harness hermes                  # register with a runner named in harnesses.toml; opencode, hermes, omp, grok ship as shapes
ljos findings out/campaign.json --remember     # an eb-stack campaign's typed findings, one lesson each under the module that failed
```

## The loop, in two verbs

`ljos sitting ISSUE` opens a sitting in the protocol's order and stops at the first store that does not answer: doctor, cards, the review clock, the island the issue's title activates, the working set, the timeline, the claim. `ljos finish ISSUE --lesson "..." [--outcome OPTION]` closes it: the lesson is remembered, the island fires, the session node completes, the ticket closes, and a named outcome shrinks the voters it refuted. The loop that makes the seat a memory runs every time, not only when somebody remembers it.

## Who is sitting

The seat is the program that connected. `ljos-mcp` names it after the client that initialised it, and `ljos` in a shell that runner opened finds the same name through the process tree or a shared session id, so a runner's tools and its verbs are one seat with nothing set. Claims are held per conversation; memory, ballots and trust accrue to the seat. `ljos seat` prints both names and where they came from. `ljos onboard` prints the one MCP entry any runner takes; `ljos protocol` prints the text an agent reads first.

Nothing needs a variable set: the pack answers on `127.0.0.1:8761`, the seat's memory is the one workspace `seat` from any directory, and every claim carries the seat that wrote it.

## The smoke test

`scripts/smoke.sh` runs every loop on scratch stores: memory, agreement, learning, a sitting and its finish, a rewrite that closes its earlier claim, habits, an as-of read, a signed handover received and then refused after one byte is altered, a rule through the hook and through `policy`, then `scripts/herd.sh` (eight sittings, one contended ticket, eight finishes, one island fire). `scripts/terra/` holds the Slurm scripts that build, smoke and herd the seat on the build host, as run.

## MCP

`ljos-mcp` serves the same verbs over stdio: thirty-six tools, each opening with when to call it, the protocol at `ljos://protocol`, the cards read-only at `ljos://cards/`, and three prompts. Paste this where the runner keeps its servers:

```json
{"mcpServers": {"ljos": {"command": "ljos-mcp"}}}
```

Unanimous green (two implementers, two reviewers, `ljos consensus` is GREEN) lives in [`examples/unanimous-green/`](examples/unanimous-green/).

## Documentation

The org site teaches sitting: <https://leidarljos.github.io>. The crate site is Org plus Sphinx: <https://leidarljos.github.io/ljos/>.

| Page | What it answers |
|---|---|
| [First write](https://leidarljos.github.io/docs/start/) | Install, remember one sentence, search it back |
| [Sit](https://leidarljos.github.io/docs/sit/) | `sitting` opens, `finish` closes |
| [Harnesses](https://leidarljos.github.io/docs/harnesses/) | One entry for any runner; the seat names itself |
| [Policy](https://leidarljos.github.io/docs/policy/) | `ljos-policyd` is the TCB |
| [Getting started](https://leidarljos.github.io/ljos/getting-started.html) | Memory, agreement, and a sitting on scratch stores |
| [How-to](https://leidarljos.github.io/ljos/howto.html) | Hook, onboard, habits, handover, trust |
| [Reference](https://leidarljos.github.io/ljos/reference.html) | Every verb, tool and variable |
| [Explanation](https://leidarljos.github.io/ljos/explanation.html) | The contracts, the memory model, time as data, agreement that learns |
