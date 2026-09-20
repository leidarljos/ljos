# The seat protocol

One seat, five stores, five questions. Ask the store that owns the question.
`ljos` is the one command in front of them; `ljos-mcp` serves the same verbs
over the Model Context Protocol (MCP). Every verb below has a tool of the same
name with the prefix `ljos_`.

| question | store | verbs |
|---|---|---|
| what did the human freeze | cards | `cards` (read-only; never extract-on-write) |
| what is the work, what blocks it, who agrees | tracker (vissue) | `recall`, `vote`, `consensus`, `deed`; `vissue create`, `vissue note`, `vissue update` |
| what does this seat know, standing | pack (packset) | `search`, `island`, `remember`, `prefer`, `forget`, `due`, `graded` |
| what did the work produce | deed store (deedar) | `evidence`, `current`; `deedar create` |
| which work is claimable right now | claim graph (claimdag) | `claim`, `release`, `complete` |
| how do the voters weigh each other | pack, trust rows | `trust`, `learn`, `calibrate` |
| who votes with a view of its own | pack, persona atoms | `persona`, `vote --as` |

A failure is a store not answering. It is never an empty answer. When a verb
fails, run `doctor` before drawing any conclusion.

## Before the work: a sitting

One verb runs the whole opening in order and stops at the first store that
does not answer:

    ljos sitting ISSUE --assignee NAME

It prints eight sections, and each one is a step you would otherwise run by
hand. Each answers something the next one needs. Between the island and the
recall it reads the issue's blockers from the tracker: an issue whose
blockers are still open is refused before anything is claimed, because the
graph says it is not workable; `--anyway` sits on it regardless and says so.

1. `ljos doctor`. A `no` on `tracker`, `deed store` or `pack` is the answer;
   `packsetd` on a scratch port starts a pack writer. Do not proceed on a `no`.
2. `ljos cards`. What the human froze. Read, never write.
3. `ljos due`. What is due for review. The verb prints the list; it does
   not grade. After the sitting, read each claim and `ljos graded ID`
   (`--lapsed` when you had to look it up). The review clock moves only
   when you grade.
4. `ljos island` on the issue's title. The sitting takes the strongest
   eight. `ljos search TOPIC` and a full `ljos island TASK` are during
   the work, not this opening.
5. `ljos recall ISSUE`. The plan, the inputs' deeds, and what the issue has
   cited so far.
6. `ljos timeline ISSUE`. The last twelve dated events across the three
   stores; `ljos timeline` without a sitting prints them all.
7. `ljos claim ISSUE --assignee NAME`. Occupancy is `{name}:{issue}`:
   two conversations hold two tickets. The same issue is still one
   holder. `busy` on a named worker means that name still holds another
   node (`ljos complete` or `ljos release`).

No issue yet? `vissue q -p PROJECT "TITLE"` mints one and prints its id.
Every piece of work has an issue before it has a claim.

## During the work

- Every artefact the work produces is a deed, then a citation:
  `deedar create file --name NAME --path PATH --agent NAME` prints an
  accession; `ljos deed ISSUE --add ACCESSION` cites it on the issue.
  Citing a deed names it; the bytes stay in deedar.
- Every lesson that will still be true next sitting is one `ljos remember`
  of two short sentences at most. A standing choice between two ways is one
  `ljos prefer`. Never a transcript, never a summary of the session. A
  correction from the person ("you should have", "do you not remember")
  is a preference the pack does not hold: write it with `ljos prefer`
  before the work it corrects, not after. A
  lesson that rewrites an earlier one closes the earlier one's window; the
  verb says `revises N earlier memories` when it did. `ljos consolidate`
  reports the pairs the rule would close across what is held, and
  `--apply` closes them; run it after a handover is imported.
  `ljos conflicts` lists the likeliest contradictions by distance rather
  than by words, when the `landscape` habitat is installed.
- Every number the seat keeps measuring is a habit: `ljos habit NAME VALUE
  [--unit U] [--every 7d] [--source JOB]` takes a reading, closes the one
  before it (kept as what it was), and puts the next reading on the review
  clock one cadence on, so `ljos due` and the hook say when it is late.
  `ljos habit` lists the habits as they stand with the change since the
  last reading; `ljos search --as-of` answers what one stood at then. A
  benchmark score, a latency, a count of open tickets: readings, not
  lessons.
- Every decision with more than one defensible answer is a ballot:
  `ljos vote ISSUE --for OPTION` once per identity (`VISSUE_AGENT`), then
  `ljos consensus ISSUE`. A tally is a count; the consensus is the settle
  under the trust rows. On a hard question add a forecast beside the
  ballot, `ljos predict ISSUE --expect OPTION`; with two or more forecasts
  the settle also names the surprisingly popular answer, the option whose
  actual share most exceeds its forecast, and shows each voter's standing.
  When the world says which option was right, `ljos finish ISSUE
  --outcome OPTION` (or `ljos learn`) writes every voter's record of
  outcomes as its weight, so the next settle weighs a voter by what it
  got right.
- When the work has shown that a kind of command must never run, or must
  be asked about first, write the law: `ljos rule 'PATTERN' --verdict
  deny|ask --why "..."`. The hook stops or asks at the point of action and
  `ljos policy` says the same; the rule is memory and travels in handovers.
- When the work wants readers with views of their own, such as a reviewer
  for a broad audience beside a domain expert, write each once:
  `ljos persona NAME --anchor A --view "..." --about DOMAIN...`, and
  `ljos personas` prints the roster the pack holds. Then
  `ljos vote ISSUE --for OPTION --as NAME` casts as it. The anchor in
  `[0, 1]` is how far it moves off its ballot in the settle; 0 never moves.
  A trust row scoped with `--about DOMAIN` applies when the issue's title
  carries that word; `learn` writes its rows scoped to what the issue's
  island is about, so being wrong on one topic costs nothing elsewhere.
  A panel is one subagent per persona, each started from
  `ljos brief NAME ISSUE` (the view, what the seat knows on its domains,
  the working set), each casting one ballot as itself, then
  `ljos consensus`; over MCP the `run_a_panel` prompt orders it, and
  without MCP `ljos panel ISSUE --out DIR` writes one brief per persona.
  Both seat only the personas whose `--about` domains the issue's title
  or island names, and every persona when none does; a panel that seats
  everyone on everything is a count. Each brief is a file to start a
  subagent from. A panel
  member's own lesson goes in with `ljos remember --as NAME "..."` and
  comes back to it first in its next brief; the seat still reads it. The kind of work sets the dynamics: tag
  the issue `broad` when the panel is a broad audience, and the settle runs
  bounded confidence, so clusters are allowed and reported instead of being
  averaged into one position.
- Progress goes on the issue, dated: `vissue note ISSUE "..."`.

### A bump, and a build campaign

A toolchain or version bump with eb-stack is one sitting on the ticket
and one island per recipe. Before a recipe is touched, `ljos island
"<name> <version> <toolchain>"` (the MCP `ljos_island` with that cue):
what the last bump of it taught, the patch it needed, the step it failed
in. Then the ladder in order, each rung its own claim with its own
artifact: `eb_recipe_check`, `eb_package_bump` (the lock under
`out/locks` is `resolves`), `eb_recipe_lint`, `eb_target_doctor`,
`eb_campaign_run` and `eb_campaign_status` (`builds`,
`binary-verified`). Say a rung only when its artifact exists.

Every typed finding the campaign records is a lesson once somebody
resolved it: `ljos findings out/campaign.json --remember --issue ISSUE`
(the MCP `ljos_findings`) writes one lesson per resolved finding under
the recipe's name, the package and the failure class, and cites the
state file on the issue. A finding a later attempt merely got past is
not a lesson; `--all` takes those too. A lesson the seat writes by hand
names the recipe, the step, the error line and the fix: "GCCcore-15.2.0
on terra: compile failed in the build step with linux/scc.h missing.
Fix: the GCC 14 libsanitizer kernel headers patch." Not "verify the
lock exists before proceeding": the next seat cannot act on that.

## After the work

One verb closes the sitting:

    ljos finish ISSUE --status done --lesson "..." [--outcome OPTION]

It remembers the lesson, fires the island, completes the session node, and
learns from the outcome when one is named. Without `--lesson` it says so;
a sitting that taught nothing worth two sentences is rare. By hand, the
same four steps are:

1. `ljos island TASK --fire` when the island served: the strongest memories
   fire together and their links gain weight.
2. `ljos complete ISSUE --status done` (`failed`, `cancelled`). Completing
   the session node does not close the ticket: `vissue update ISSUE -s DONE`
   does, when the work is accepted.
3. `ljos learn ISSUE --outcome OPTION` when the world says which option was
   right. Every voter it refuted shrinks in every other voter's row, and a
   persona it refuted holds its next ballot less firmly.
4. `ljos handover --out DIR --issue ISSUE [--to user@host:path]` when
   another seat takes over; the receiver runs `ljos receive DIR`, then
   `--import`.

## When nobody names an outcome

Most issues close without anyone saying which option was right, and then
`learn` never runs and every voter keeps the same weight. `ljos calibrate
--project PROJECT` reads every issue of the project with two or more
ballots and estimates each voter's accuracy from how often it agrees with
the answer the other voters make likely (Dawid and Skene), then writes
those accuracies back as trust rows. Run it once per project after a few
issues have been voted on, and again when many more have. A consensus
under equal weights is a count; under calibrated rows it is not.

## Refusals worth knowing

- `claim: assignee busy HEX`: that name still holds that node.
  `ljos release HEX --assignee NAME` hands it back, `ljos complete HEX`
  finishes it. Occupancy is per issue, so a second ticket does not take
  this path.
- `already held by NAME; the sitting resumes`: not a refusal. A second
  `sitting` on the issue you hold renews the lease and goes on. Held by
  another seat, the claim names that actor and the two verbs that free it.
- `complete: status not terminal`: the statuses are `done`, `failed`,
  `cancelled`. To stop without finishing, `release`.
- A claim on an issue whose earlier sitting finished reopens its session
  node and takes it: a new sitting on old work, with the ledger kept.
- `not a deed accession`: `--why` on `forget` and `trust` takes accessions
  from `deedar`, never free text.
- `the pack writer did not answer`: the pack is down, not empty.
  `packset ensure`.
- `ljos due` prints `0 due; nothing scheduled`: the seat has remembered
  nothing, and the review loop has nothing to run on. Remember something.
  `0 due; N scheduled, next at T` is a clock that is running.
- `ljos policy ARGV` prints the line a command would run under argv law,
  then what the pack knows that bears on it. It is not a check.
- `ljos hook` is the memory hook: a runner or a policy layer pipes the
  action about to happen (its hook JSON, or the plain argv) and gets back
  the memories that action activates, what two of the pack's scorers
  agreed on, preferences first, then lessons
  oldest to newest, each with its age (`[lesson, 3 weeks ago]`), so a
  later lesson reads as a revision of an earlier one. When the session
  ends, the memories it injected fire together, so what served one sitting
  is wired for the next. `ljos onboard`
  installs it on the runner's tool-call and prompt events, so the seat's
  memory reaches the agent at the point of action without being asked.

## Identity and environment

Nothing here needs a variable set. The pack is found on `127.0.0.1:8761`
and the seat's memory is one workspace, `seat`, whatever directory you
stand in (`PACKSET_WORKSPACE` names another). The deed store and claim
graph live in the user's state directories, the tracker at the root
`vissue identity` prints, and the host key at `~/.config/deedar/host.key`
when it exists. The seat is the program that connected: `ljos-mcp` names
it after the client that initialised it, and a shell the same runner opens
finds the same name through the process tree, so a runner's tools and its
command-line verbs claim and vote as one. Two names come from that: the
seat (`acme-cli`), which memory, ballots and trust rows accrue to across
every conversation of that runner, and the holder (`acme-cli-39u`),
which this conversation's claims are held under; any `*_SESSION_ID` the
runner stamped is the holder ahead of the process tag, and occupancy is
`{holder}:{issue}`, so two conversations of one runner hold two tickets and
a second sitting does not release the first. `ljos seat` prints both names
and where they came from. `LJOS_SEAT` names the seat; a `*_SESSION_ID` still
names the holder. `VISSUE_AGENT` is
the tracker's own name for the same thing; `--as` names a persona over
both; a person at a terminal is their login user.
