# The seat protocol

One seat, five stores, five questions. Ask the store that owns the question.
`ljos` is the one command in front of them; `ljos-mcp` serves the same verbs
over the Model Context Protocol (MCP). Every verb below has a tool of the same
name with the prefix `ljos_`.

| question | store | verbs |
|---|---|---|
| what is the work, what blocks it, who agrees | tracker (vissue) | `recall`, `vote`, `consensus`, `deed`; `vissue create`, `vissue note`, `vissue update` |
| what does this seat know, standing | pack (packset) | `search`, `island`, `remember`, `prefer`, `forget`, `due`, `graded` |
| what did the work produce | deed store (deedar) | `evidence`, `current`; `deedar create` |
| which work is claimable right now | claim graph (claimdag) | `claim`, `release`, `complete` |
| how do the voters weigh each other | pack, trust rows | `trust`, `learn` |

A failure is a store not answering. It is never an empty answer. When a verb
fails, run `doctor` before drawing any conclusion.

## Before the work: a sitting

One verb runs the whole opening in order and stops at the first store that
does not answer:

    ljos sitting ISSUE --assignee NAME

It prints six sections, and each one is a step you would otherwise run by
hand. Each answers something the next one needs.

1. `ljos doctor`. A `no` on `tracker`, `deed store` or `pack` is the answer;
   `packset ensure` starts a pack writer. Do not proceed on a `no`.
2. `ljos cards`. What the human froze. Read, never write.
3. `ljos due`. Read every claim listed, then `ljos graded ID` for each one,
   `--lapsed` when you had to look it up. The review clock moves only when
   you grade.
4. `ljos search TOPIC`, then `ljos island TASK` with the task in your own
   words. The island is the cluster of memories this task touches, the hits
   are only its seeds.
5. `ljos recall ISSUE`. The plan, the inputs' deeds, and what the issue has
   cited so far.
6. `ljos claim ISSUE --assignee NAME`. One live claim per name. `busy` means
   you still hold another node: `ljos complete` it, or `ljos release` it.

No issue yet? `vissue q -p PROJECT "TITLE"` mints one and prints its id.
Every piece of work has an issue before it has a claim.

## During the work

- Every artefact the work produces is a deed, then a citation:
  `deedar create file --name NAME --path PATH --agent NAME` prints an
  accession; `ljos deed ISSUE --add ACCESSION` cites it on the issue.
  Citation is not a merge, and the product is never pasted into the ticket.
- Every lesson that will still be true next sitting is one `ljos remember`
  of two short sentences at most. A standing choice between two ways is one
  `ljos prefer`. Never a transcript, never a summary of the session.
- Every decision with more than one defensible answer is a ballot:
  `ljos vote ISSUE --for OPTION` once per identity (`VISSUE_AGENT`), then
  `ljos consensus ISSUE`. A tally is a count; the consensus is the settle
  under the trust rows.
- Progress goes on the issue, dated: `vissue note ISSUE "..."`.

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
   right. Every voter it refuted shrinks in every other voter's row.
4. `ljos handover --out DIR --issue ISSUE` when another seat takes over;
   the receiver runs `ljos receive DIR`, then `--import`.

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

- `claim: assignee busy HEX`: you hold that node. `ljos release HEX
  --assignee NAME` hands it back, `ljos complete HEX` finishes it.
- `complete: status not terminal`: the statuses are `done`, `failed`,
  `cancelled`. To stop without finishing, `release`.
- `not a deed accession`: `--why` on `forget` and `trust` takes accessions
  from `deedar`, never free text.
- `the pack writer did not answer`: the pack is down, not empty.
  `packset ensure`.
- `ljos due` prints `0 due; nothing scheduled`: the seat has remembered
  nothing, and the review loop has nothing to run on. Remember something.
  `0 due; N scheduled, next at T` is a clock that is running.
- `ljos policy ARGV` prints the line a command would run under argv law. It
  is not part of a sitting and it is not a check.

## Identity and environment

Nothing here needs a variable set. The pack is found on `127.0.0.1:8761`
and the seat's memory is one workspace, `seat`, whatever directory you
stand in (`PACKSET_WORKSPACE` names another). The deed store and claim
graph live in the user's state directories, the tracker at the root
`vissue identity` prints, and the host key at `~/.config/deedar/host.key`
when it exists. `VISSUE_AGENT` names the identity
a ballot or claim is recorded under; set it when you vote as more than one.
