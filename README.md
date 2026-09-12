# ljos

One seat over five habitats and argv law. It does not own them.

Citation is not a merge. Completing a claimdag node does not close a ticket. Cards are read-only. Consensus is a [different crate](https://github.com/leidarljos/consensus).

```
ljos sitting vissue-xxxx --assignee you        # doctor, cards, due, island, recall, claim
ljos finish vissue-xxxx --lesson "..." [--outcome ship]   # remember, fire, complete, learn
ljos calibrate -p project                      # trust rows from the voting history, no truth labels
ljos remember "the default fuse is CombMNZ"
ljos prefer "CombMNZ over RRF"
ljos search fuse
ljos island "rebuild the packset site"
ljos evidence deed-…
ljos deed vissue-xxxx --add deed-…
ljos recall vissue-xxxx
ljos claim <node> --assignee you
ljos release <node> --assignee you
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
ljos graded <atom-id> [--lapsed]
ljos handover --out bag --project x --issue vissue-xxxx
ljos receive bag [--since bridge.txt] [--import]
ljos doctor
ljos protocol
ljos onboard --harness RUNNER
```

## The loop, in two verbs

`ljos sitting ISSUE --assignee NAME` opens a sitting in the protocol's order and stops at the first store that does not answer: doctor, cards, the review clock, the island the issue's title activates, the working set, the claim. `ljos finish ISSUE [--status done] --lesson "..." [--outcome OPTION]` closes it: the lesson is remembered, the island fires, the session node completes, and when an outcome is named the voters it refuted shrink. A finish without a lesson says so. The point is that the loop that makes the seat a memory runs every time, not only when somebody remembers it.

`ljos due` ends with one line on the clock: `0 due; 12 scheduled, next at ...` is a clock that runs; `0 due; nothing scheduled` is a seat that has remembered nothing. A claim that never entered the clock is due now.

`ljos calibrate -p PROJECT` moves the trust rows when nobody names an outcome: Dawid and Skene's estimate of each voter's accuracy from the project's voting history (doi:10.2307/2346806), written back as the weight every other voter gives that voter (a linear pool's weight for a source that reliable, Genest and Zidek, doi:10.1214/ss/1177013825). Under equal rows a consensus is a count; after `calibrate` or `learn` it is not.

## The smoke test

`scripts/smoke.sh` runs every loop on scratch stores under one temporary directory: memory, agreement, learning, a sitting and its finish, a second sitting that reopens, release, personas with forecasts and the surprisingly popular reading, a brief, calibration, a rule through the hook and through `policy`, a signed handover and its receipt. It needs the seat binaries on `PATH` and starts a pack writer if none answers. It found its first bug on its first run. `scripts/terra/build.sbatch` and `scripts/terra/smoke.sbatch` are the Slurm scripts that build the seat and consensus crates and run the smoke on the build host, as run.

## For an agent, or the person running one

`ljos protocol` prints the sitting protocol: which store answers which
question, the order of verbs before, during and after the work, and the
refusals worth knowing. `ljos onboard --harness RUNNER` registers `ljos-mcp`
with an agent runner and installs the protocol as its `ljos` skill. The
runners are described in `~/.config/ljos/harnesses.toml`, one table each,
either as a command that registers servers or as a config file to append an
entry to, plus the directory the runner loads skills from;
`ljos onboard --example` prints the file's shape, and `--harness json` prints
the server entry to paste into any runner by hand. `--dry-run` reports
without writing. `onboard` also starts a pack writer when none answers (`packset ensure`)
and writes the seat's host key at `~/.config/deedar/host.key` when there
is none, so memory verbs answer and handovers go out signed from the
first. `ljos doctor` then shows whether each runner named is
onboarded. The server serves the same text at `ljos://protocol`.

Nothing needs a variable set: the pack is found on `127.0.0.1:8761`
(`PACKSET_URL` points elsewhere, `off` means no pack) and the seat's memory
is the one workspace `seat` from any directory (`PACKSET_WORKSPACE` names
another; the pack's own command line keys workspaces to repositories), the
host key at `~/.config/deedar/host.key` when it exists.

`remember` / `prefer` POST `/v1/atoms` against `PACKSET_URL` (`INSIDE_MEMORY_URL` is an alias). They write one explicit claim. They do not extract from a transcript.

`forget ID` POSTs `/v1/atoms/delete` and is the other direction. The pack tombstones rather than erases, so the claim stops being recalled and the record that it was held and withdrawn stays; `packset atoms --as-of TS` reads it back through the window it was live in. `--why` names the deed the retraction stands on, citing deeds the same way `trust` does, and the pack refuses free text in its place.

`cards` prints `USER.md` and `MEMORY.md` only. It never writes them.

`ljos claim ID --assignee NAME`, `ljos release ID --assignee NAME` and `ljos complete ID` take a tracker id or a 32-hex claimdag id. A claim refused as busy names the tracker id the assignee still holds and the two verbs that free it; `release` hands a node back unfinished; a claim on an issue whose earlier sitting finished reopens the node and takes it. A tracker id maps to one node (FNV-1a 128 of the id, minted with the id as its summary on first use) and a name to one actor the same way, so the session graph stays outside the accession join while the seat speaks tracker ids.

`ljos policy ARGV` prints the argv line, then the verdict of any rule in the pack that matches it (`deny` or `ask`, with the reason), then what the pack knows that bears on it. `ljos rule PATTERN --verdict deny|ask --why TEXT` writes such a rule: a glob over the whole command line, kept in the pack like any memory, enforced by the hook as the runner's permission decision on tool calls. This is the control the seat has over an action: what it knows arrives as context, what it has ruled arrives as a verdict, both from the same store and both at the point of action.

`ljos predict ISSUE --expect OPTION` (or a JSON object of option to share) records a voter's forecast of the others; with two or more forecasts `ljos consensus` also prints the surprisingly popular answer (Prelec, Seung and McCoy, doi:10.1038/nature21054) and, when trust rows exist, each voter's EigenTrust standing (doi:10.1145/775152.775242). It never calls `grokos policy reload`. Reloading a Janet pack is not a check. When `grok-policyd` exists it is the TCB; this binary is not.

`ljos hook` is the memory hook for a policy layer or a runner: it reads the action about to happen on stdin (the runner's hook JSON with `tool_input.command` or `prompt`, or a plain argv line) and prints the memories that action activates, standing preferences first and then lessons oldest to newest, each bracketed with its kind and its age (`[lesson, 3 weeks ago]`), as the runner's `additionalContext` JSON or as plain lines. The age is what lets the reader treat what it recalls as a timeline: a later lesson revises an earlier one, and a preference from yesterday outranks one from a year ago. `ljos search`, `ljos brief` and the island in `ljos sitting` carry the same age column. On a prompt the hook also says, once per session, how many claims are due for review. When the session ends, the memories the hook injected during it fire together (`packset fire`), so what served one sitting is wired for the next; the hook is installed on the runner's session-end event as well by default. Only hits scoring at least six tenths of the best hit are injected, five at most, and each memory once per runner session (the ids are kept under the runtime directory), so the same lesson does not arrive on every command. Nothing to say is no output, so the hook never blocks an action. `ljos onboard` merges it into a runner whose runners-file entry names a `hooks` settings file, on the prompt event by default and on the events `hook_events` lists (`["UserPromptSubmit", "PreToolUse"]` for tool calls too), and drops it from events no longer listed; `ljos doctor` shows a `runner hook` row. The default came out of a panel of this seat's personas (issue `ljos-t75s` on the seat's board): a turn issues many shell commands and one prompt. This is how what the seat knows is injected at the point of action rather than waiting to be searched for.

`ljos consensus` reads the pack's live `trust` rows and passes them to both `ljos-consensus settle --issue --trust` and `vissue consensus --trust`, so the two settles weigh the same graph. No rows: every voter weighs the same.

`ljos trust FROM TO WEIGHT` writes one row as a `trust` atom, citing deeds with `--why`; `--about DOMAIN` scopes the row, and a scoped row applies when the issue's title carries that word. `ljos persona NAME --anchor A --view TEXT [--about DOMAIN]` writes a voter with a view of its own; `ljos vote --as NAME` casts as it, and `ljos consensus` passes every persona's anchor to both settles as `--susceptibility-of`, so a persona holds its ballot as much as it says (Friedkin-Johnsen; a stubborn agent's pull is Acemoglu et al., doi:10.1287/moor.1120.0570). `learn` writes its rows scoped to what the issue's island is about, and a scoped row that applies stands in for the unscoped row of its pair. A persona the outcome refuted moves its anchor toward one by half the gap, so a persona that keeps being wrong listens more; a vindicated one keeps its anchor. The kind of work sets the dynamics: an issue tagged `broad` settles under bounded confidence (`--epsilon 1.0` on the model crate; Deffuant et al., doi:10.1142/S0219525900000078), so a broad audience is allowed to stay in clusters and the settle reports them, while an untagged issue runs the anchored model. `ljos learn ID --outcome OPTION` reads the ballots, shrinks every refuted voter in every other voter's row by `--beta` (default 0.5, Hedge, doi:10.1006/jcss.1997.1504), floors at 0.01, and writes the complete set back. Trust is memory: rows carry a validity window, a later row supersedes, and `packset export` carries them in a handover.

## MCP

`ljos-mcp` serves the same verbs over stdio. Writers: `ljos_sitting`, `ljos_finish`, `ljos_calibrate`, `ljos_persona`, `ljos_remember`, `ljos_prefer`, `ljos_forget`, `ljos_trust`, `ljos_learn`, `ljos_graded`, `ljos_island`, `ljos_deed`, `ljos_vote`, `ljos_claim`, `ljos_release`, `ljos_complete`, `ljos_handover`, `ljos_receive`. `ljos_forget` is the only one annotated destructive, because it is the only one that takes something away. The rest read. Resources: `ljos://protocol`, and the cards `ljos://cards/USER.md` and `ljos://cards/MEMORY.md` from `LJOS_CARDS_DIR`. Prompts: `start_a_sitting`, `run_a_panel` (one subagent per persona in the pack, each started from `ljos_brief`, one ballot each as itself, then the settle), `check_a_handover`. `ljos onboard` writes the registration; by hand it is

```json
{"mcpServers": {"ljos": {"command": "ljos-mcp"}}}
```

`ljos island CUE` asks the pack which memories a task activates: the top search hits seed a two-hop spread along the pack's entity links, and the cluster comes back strongest first. Not a persona or a view; the island this task touches. `--fire` says the seat went on to use it: the strongest eight fire together and their links gain weight, so the next cue like it walks a heavier path.

`ljos due` lists the atoms whose review clock has run out; `ljos graded ID` marks one recalled (`--lapsed` for the other answer) and the pack reschedules it. This is the spaced-review loop the pack already keeps, reached from the seat.

`ljos handover --out DIR` runs the tracker's satchel, `packset export` into `DIR/data/atoms`, `deedar export` of every deed the satchel needs or the pack cites into `DIR/data/deeds`, then seals, and signs the manifest when `DEEDAR_HOST_SIGNING_KEY` is set. `ljos receive DIR` verifies the manifest, checks the deed receipts (`--since` for the bridge from a kept head), checks the signature when there is one, counts the atoms and trust rows, and with `--import` posts the atoms into this seat's pack, each carrying the sender as an entity (`from:<signing key>` when the bag is signed, `from:handover` otherwise), so a search can ask for what one seat taught.

`ljos doctor` says which habitats answer and exits 1 when the tracker, the deed store, or the pack does not. It also says whether a host key is found (`~/.config/deedar/host.key`, or `DEEDAR_HOST_SIGNING_KEY`), without which a handover goes out unsigned, and whether each harness it finds on the machine has the server registered and the skill installed.

Other projects may still speak packset, deedar, vissue, or claimdag alone.

Site: <https://leidarljos.github.io>
