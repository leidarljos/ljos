# Changelog

Versions follow semver at 0.x: a minor bump is a feature, a patch is a fix.

## Unreleased

## 0.13.5 (2026-09-20)

- A finish says when the island fired already this hour (the pack holds
  a second fire of the same claims for an hour, so several seats or
  personas closing sittings on one issue tighten its links once).
  `scripts/herd.sh` runs eight sittings from four seats at once, contends
  one ticket between two seats, and closes all eight in parallel.

## 0.13.4 (2026-09-19)

- The roster, a brief and the panel prompt read only the persona atoms
  (`packset-client` 0.9.17), not every atom in the workspace.
- `ljos doctor` has a memory row: the live count against the cap and
  the claims forgotten, by reason (packset 0.9.17 counts them).
- Writing a persona that the pack already holds supersedes its previous
  atom, so moving an anchor or a view leaves one live persona of that
  name; the roster showed one, the pack kept both.

## 0.13.3 (2026-09-19)

- `ljos personas` and the `ljos_personas` tool print the roster the pack
  holds: name, anchor, the domains each speaks to, its view. The only
  way to see it was the panel prompt.
- `ljos due` sweeps the pack first and says what the sweep did: reviews
  left due past twice their interval lapse, never-recalled claims missed
  three times are forgotten by neglect, and the list that follows is the
  one after that. `packset-client` 0.9.14.

## 0.13.2 (2026-09-19)

- A panel seats only the personas whose domains the issue speaks to,
  read from its title's words and the island it activates; every persona
  sits when none speaks to it. The `ljos panel` brief and the
  `run_a_panel` prompt agree on the roster, and the prompt says how many
  of the pack it seated.
- `ljos onboard --harness grok` bumps `LJOS_MCP_GENERATION` in the runner
  config when the crate version moved, so Grok's watcher respawns
  `ljos-mcp` and a session restart is not required.

## 0.13.1 (2026-09-19)

- The holder is any `*_SESSION_ID` the runner set, the full value, ahead
  of the process tag. Two ids that share an eight-character prefix occupy
  different slots. A server sitting and a shell sitting of one session
  are one occupancy name.
- `ljos finish ISSUE` with status done (the default) closes the ticket
  in the tracker, so a board never shows TODO over a completed claim and
  hands the work out again. Failed or cancelled leaves the ticket where
  it is. Completing a node alone still does not close a ticket.
- The server's seat record is also kept under each conversation id the
  runner stamped, and a shell reads it by any id it shares with the
  server. A line editor that stamps a session id of its own into the
  shell no longer makes that shell a second holder.
- Every write names the seat that wrote it (`seat:<name>` first among
  the entities; a persona's, a habit's or a trust row's entities join it,
  a trust row's stay the deeds it cites). A hit written by another seat
  says so: `[lesson, 3 days ago, from brio]` in the hook and a brief,
  `(from brio)` in `ljos search`, `from` on the `ljos_search` row. Many
  seats share one pack; a reader now sees whose lesson it is reading.
- `packset-client` 0.9.12, whose hits carry entities and whose writer
  holds a workspace at a live cap.

## 0.13.0 (2026-09-19)

- The doctor keeps each crates.io answer on disk for a day, so a herd of
  seats opening sittings asks the registry once a day per binary rather
  than once a sitting each. A busy refusal names this conversation's
  holder and the release that frees a conversation that is gone.
- Habits: `ljos habit NAME VALUE [--unit U] [--every 7d] [--source S]`
  takes a reading of a number the seat keeps measuring, as a claim of
  kind `habit` that supersedes the earlier reading and carries it as
  `was`, due for its next reading one cadence on; `ljos habit` lists
  them with the change since the last reading and when the next is due.
  `ljos_habit` is the same over MCP.
- The seat names itself. `ljos-mcp` takes the client's name at initialize
  (`acme-cli`, `brio`, whatever the runner says) as the seat and
  leaves a record under the runtime directory keyed by the runner's
  process; `ljos` in a shell that runner opened walks its own process tree
  to the same record, or to the first ancestor that is not a shell, so a
  runner's tools and its verbs are one seat with nothing set and nothing
  in `env`. Two names: the seat, which memory, ballots and trust accrue to
  across conversations, and the holder (`acme-cli-39u`), which this
  conversation's claims are held under. `ljos seat` prints both; the
  doctor's `seat` row does too. `LJOS_SEAT` still overrides. The runners
  file no longer passes `LJOS_SEAT={name}`; `ljos onboard` alone prints
  the one entry any runner takes. A ballot cast without a persona is cast
  as the seat.
- `cargo install ljos` installs `ljos` and `ljos-mcp`.
- Occupancy is `{holder}:{issue}` and the holder is any `*_SESSION_ID` the
  runner stamped, else the seat tagged with the conversation's process,
  else `LJOS_SEAT`. No product list. Two conversations hold two tickets;
  the same ticket is still one holder. `doctor` prints the session and
  which variable it came from.

## 0.12.15 (2026-09-15)

- Sitting no longer treats a binary behind crates.io as a habitat
  that does not answer. Doctor still names the gap.

## 0.12.14 (2026-09-15)

- The README no longer says the hook never blocks. A TCB deny on
  PreToolUse still blocks.

## 0.12.13 (2026-09-14)

- `ljos remember` and `prefer` print one line (id, kind, due, text).
  They no longer dump the embedding.

## 0.12.12 (2026-09-14)

- `packset-client` 0.9.2, so the seat tracks the writer that prints
  `packset-mcp --version`.

## 0.12.11 (2026-09-14)

- Doctor times out MCP `--version` so a silent stdio server cannot
  hang the seat. `ljos-mcp` depends on the workspace ljos version.

## 0.12.10 (2026-09-14)

- `packset-client` 0.9.1. `ljos-mcp --version` prints and exits.
  Doctor can version the MCP binary. The door install includes
  `packset-embed`.

## 0.12.9 (2026-09-14)

- `ljos doctor` lists every seat binary (`ljos`, `packset-embed`,
  `packset-mcp`, …) with the version on PATH against crates.io. A
  part that is missing or behind is not ok. Encoder and policyd are
  required with the rest.

## 0.12.8 (2026-09-14)

- `ljos doctor` treats the grok harness hook as the frozen events
  (prompt, PostToolUse, Bash rules, SessionEnd), not a stale
  SessionStart list. `~/.config/ljos/env` is loaded when those
  keys are unset, so a shell `ljos` shares the MCP pack. Argv law
  runs only on PreToolUse and argv, not on PostToolUse.

## 0.12.7 (2026-09-14)

- `ljos onboard --harness grok` writes the frozen `~/.grok/hooks/ljos.json`.
  No table in harnesses.toml is required.

## 0.12.6 (2026-09-14)

- Grok hook is `ljos hook` only. The prompt's pack text is held and
  emitted once on `PostToolUse`, the event Grok delivers. No
  `sync.sh`. `PreToolUse` stays rules-only.

## 0.12.5 (2026-09-14)

- Grok `PreToolUse` no longer remaps to a pack search. The inject
  script exits on a tool call. `ljos hook` on Bash only decides
  rules. The due nudge no longer walks `consolidate` (that sitting
  is what timed the hook out at 20s).

- `finish` does not fire a weak island (seeds no two scorers agreed on)
  and says why; `island` marks one as weak and as the pack's
  best-connected cluster rather than what the cue is about; `doctor`
  shows the encoder row, since a down encoder is what makes seeds weak.

## 0.12.4 (2026-09-14)

- Grok onboard writes `GROK_SESSION_ID` into the MCP env so the
  occupancy split in 0.12.3 is live for that runner.

## 0.12.3 (2026-09-14)

- Occupancy uses the runner session when the assignee is a shared
  name (`grok`, `seat`, `you`). Two conversations hold two tickets.

## 0.12.0 (2026-09-13)

- `learn` writes rows from each voter's record: hits and misses so far
  as log-odds weights, the same scale `calibrate` writes, carried on the
  rows. On voters of known accuracy the record reaches batch calibration
  (0.929 against 0.934 over 8000 decisions) where Hedge reaches 0.831 and
  Hedge with recovery 0.877. `--rule hedge` keeps the shrink.
- `conflicts` leaves out passes between trust rows, personas, forecasts
  and rules: weighed, not recalled, so not contradictions to judge.

## 0.11.0 (2026-09-13)

- `conflicts` (and the `ljos_conflicts` tool): candidate contradictions
  by geometry, the lowest passes between single memories in the pack's
  embedding landscape, from the optional `landscape` habitat; nothing
  written.
- The hook reads a correction: a prompt that opens with "do you not
  remember", "you should have", "I told you" and the like gets one line,
  once per cue a session, to write the preference or lesson into the pack
  before the work. A correction the pack never held cannot fire.

## 0.10.0 (2026-09-12)

- `doctor` prints a `seat` row: the name this runner claims and votes
  under, and whether it came from `LJOS_SEAT`, `VISSUE_AGENT` or the
  default.
- `calibrate` writes log-odds weights (Nitzan and Paroush): a voter right
  nine times in ten now outweighs one right six times in ten five to one,
  where the linear rule gave three to two; chance earns the floor.
- `search` prints how many scorers named each hit; the prompt nudge counts
  the pairs `consolidate` would close beside the claims due.
- `consolidate` (and the `ljos_consolidate` tool): the pack's replacement
  rule run over what it holds, pairs reported, `--apply` to write.
- The hook injects only hits at least two of the pack's scorers named
  (`ballots` of `of` on a hit), when more than one ran; a claim one
  scorer alone matched on a command line stays in the pack.
- A second `sitting` on an issue this name already holds is a sitting
  resumed: the lease is renewed and the verb goes on, where it refused
  with `claim: status claimed`. Held by another seat, the refusal names
  the actor.

## 0.9.0 (2026-09-12)

- Every recalled memory carries its age: the hook's lines, a brief's
  lines, `search` and the island in `sitting` say `today`, `3 weeks ago`,
  `6 months ago` beside the kind, and the hook's lessons run oldest to
  newest behind the preferences. The reader lays what it recalls on a
  timeline instead of a bag.
- `timeline ISSUE` (and the `ljos_timeline` tool): the tracker's logbook,
  the cited deeds and the activated memories as one dated list, oldest
  first, each line with its age and the gap since the line before.
  `sitting` prints the last twelve as `== timeline`.
- Two runners on one host: `--assignee` defaults to `LJOS_SEAT`, then
  `VISSUE_AGENT`, then `seat`; a ballot cast without a persona is cast as
  `LJOS_SEAT` when set; the runners file may write `{name}` in `register`
  and `snippet`, so a registration passes `LJOS_SEAT={name}` to the server.
- `remember` and `finish` say when the pack closed earlier memories for
  the new one (`revises N earlier memories, now closed`), so a revision is
  seen as one.
- `search --as-of TIME` (and `as_of` on the `ljos_search` tool): the pack
  as it stood then, so "what did the seat know when it decided that" has
  an answer.

## 0.8.0 (2026-09-12)

- `scripts/smoke.sh`: every loop on scratch stores, as a check.
- `sitting` prints the island's strongest eight; `island` prints it all.
- A habitat cut off by the seat's own reader closing the pipe is not a
  refusal: `ljos consensus ID | head` ends quietly.

## 0.7.0 (2026-09-12)

- `learn --share S`: a fixed share of recovery toward one after the Hedge
  step (Herbster and Warmuth), so a voter refuted long ago can come back;
  zero, the default, is plain Hedge.
- `receive --import` tags every imported atom with its sender
  (`from:<signing key>`, or `from:handover` for an unsigned bag).
- `onboard` starts a pack writer when none answers, before wiring the
  runner to it.
- The hook fires the memories it injected during a session together when
  the session ends (the runner's `SessionEnd` event, on by default), so
  what served one sitting is wired for the next.

## 0.6.0 (2026-09-12)

- `ljos hubs`: the claims the pack's link graph turns on, highest first.
- `handover --to user@host:path` copies the sealed, signed bag to another
  seat over ssh; the receiver runs `ljos receive`.
- `predict ISSUE --expect OPTION` records a forecast of the others; with
  two or more, `consensus` prints the surprisingly popular answer (Prelec,
  Seung and McCoy) and, with trust rows, each voter's EigenTrust standing.
- `rule PATTERN --verdict deny|ask --why TEXT`: argv law in the pack. The
  hook returns the verdict as the runner's permission decision on tool
  calls; `policy` prints it beside the line. Over MCP: `ljos_predict`,
  `ljos_rule`.

## 0.5.0 (2026-09-12)

- `ljos search -n N --rerank`: the writer's cross-encoder over the top hits.
- `ljos panel ISSUE --out DIR`: every persona's brief as a file, so a runner
  without MCP can start one subagent per persona.
- `remember --as NAME` and `prefer --as NAME` (and `as` on the tools): a
  persona keeps lessons of its own, which open its next `brief`.
- `handover` signs the manifest with the seat's default host key, not only
  with one named by `DEEDAR_HOST_SIGNING_KEY`; it went out unsigned while
  `doctor` reported the key present.

## 0.4.0 (2026-09-12)

- The kind of work sets the dynamics: an issue tagged `broad` settles
  under bounded confidence on the model crate; untagged issues run the
  anchored model. On a prompt the hook says, once per session, how many
  claims are due for review. `sitting` no longer waits on the runners'
  command lines; `doctor` asks them beside the seat's own rows.
- `learn` moves a refuted persona's anchor toward one, and a scoped trust
  row that applies stands in for the unscoped row of its pair instead of
  adding to it. Islands print claims only; persona and trust atoms are
  weighed, not recalled.
- `ljos brief NAME ISSUE` and `ljos_brief`: the text a subagent playing a
  persona starts from; `run_a_panel` starts each member from it.
- `--version`. A claim on an issue whose earlier sitting finished reopens
  the session node (`claimdag reopen`) and takes it.

## 0.3.0 (2026-09-12)

Memory at the point of action:

- `ljos hook` reads a runner's hook JSON (or an argv line) on stdin and
  answers with the memories the action activates, preferences first, in the
  runner's `additionalContext` shape or plain lines. `onboard` installs it
  on a runner's prompt event when its table names a `hooks` file, or on the
  events `hook_events` lists, and drops it from the rest; `doctor` shows
  the row. The default was settled by a panel of the seat's personas. `ljos policy` prints the memory beside the
  argv line.

Personas and scoped trust:

- `ljos persona NAME --anchor A --view TEXT [--about DOMAIN]` writes a voter
  with a view; `ljos vote --as NAME` casts as it; `consensus` passes every
  persona's anchor to both settles as `--susceptibility-of`.
- Trust rows carry `about` domains: an unscoped row applies everywhere, a
  scoped one when the issue's title carries the word. `learn` writes rows
  scoped to the entities of the issue's island.
- Over MCP: `ljos_persona`, `as` on `ljos_vote`, `about` on `ljos_trust`,
  and the `run_a_panel` prompt: one subagent per persona, one ballot each,
  then the settle.

The loop runs every time:

- `ljos sitting ISSUE --assignee NAME` opens a sitting in the protocol's
  order (doctor, cards, due, island, recall, claim) and stops at the first
  store down; `ljos finish ISSUE [--lesson] [--outcome]` closes it
  (remember, fire, complete, learn) and says when no lesson was given.
  Both over MCP as `ljos_sitting` and `ljos_finish`.
- `ljos calibrate -p PROJECT` writes trust rows from the project's voting
  history with no truth labels (Dawid and Skene, through
  `ljos-consensus reliability`), so weights move when nobody names an
  outcome.
- `ljos due` lists claims that never entered the review clock as due, and
  ends with one line on the clock: due, scheduled, next.
- `ljos onboard` writes the seat's host key when there is none, so
  handovers go out signed.

For an agent, or the person running one:

- `ljos protocol` prints the sitting protocol: which store answers which
  question, the order of verbs before, during and after the work, and the
  refusals worth knowing. The server serves it at `ljos://protocol` and
  names it first in its instructions.
- `ljos onboard --harness NAME [--dry-run]` registers `ljos-mcp` with a
  runner described in `~/.config/ljos/harnesses.toml` and installs the
  protocol as its skill; `--harness json` prints the entry for any other,
  `--example` the file's shape. `doctor` reports whether each runner named
  is onboarded.
- `ljos release ID --assignee NAME` and `ljos_release` hand a session node
  back unfinished. A claim refused as busy now names the tracker id the
  name still holds and the two verbs that free it.
- Nothing needs a variable set: the pack is found on `127.0.0.1:8761`
  (`PACKSET_URL=off` means no pack), the seat's memory is the one
  workspace `seat` from any directory (`PACKSET_WORKSPACE` names another),
  and the host key at `~/.config/deedar/host.key`.
- Every verb and flag has help; every tool description opens with when to
  call it; the claim and finish arguments say they take tracker ids.

## 0.2.0 (2026-09-12)

The seat over four stores, as one command and one MCP server.

- Memory: `remember`, `prefer`, `forget --why DEED`, `search`, `island
  CUE [--fire]`, `due`, `graded ID [--lapsed]`.
- Agreement: `vote`, `consensus` under the pack's trust rows on both the
  model crate and the tracker's verb, `trust FROM TO W --why DEED`, `learn
  ID --outcome OPTION` (Hedge reweighing, rows written whole).
- Work: `claim` and `complete` take a tracker id or a name and map them to
  claim-graph ids; `deed`, `recall`.
- Handover: `handover --out DIR` packs the satchel, the atoms and the deeds
  both cite, seals and signs; `receive DIR [--since] [--import]` checks and
  imports.
- `doctor` reports every habitat, the host signing key included.
- MCP: twenty-two tools with read/write annotations, two resources, two
  prompts that sequence a sitting and a handover check.
- A documentation site and handbook at https://leidarljos.github.io/ljos/.

## 0.1.0

The first seat: remember, prefer, search, evidence, current, deed, recall,
vote, claim, complete, cards, policy, consensus.
