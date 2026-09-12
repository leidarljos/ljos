# Changelog

Versions follow semver at 0.x: a minor bump is a feature, a patch is a fix.

## Unreleased

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
