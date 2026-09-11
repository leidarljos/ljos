# Unanimous green (procedure-file host)

Same loop as the Rhai host. This host has no Rhai runner. Two
isolated implementer turns, two reviewer turns. `ljos` is the seat.

## Setup

```
<host> mcp add ljos -- ljos-mcp
```

`ljos`, `vissue`, `deedar`, `claimdag`, and `ljos-consensus` on PATH.

## Seats

`impl-A`, `impl-B`, `rev-1`, `rev-2`. Export `VISSUE_AGENT` to that
name on every vote.

## Host loop

1. `ljos recall <id>`.
2. Two implementers in isolation (separate worktrees or sequential
   isolated turns). Each freezes a deedar product and runs
   `ljos deed <id> --add <accession>`.
3. Apply one tree. Conventional commit if you commit.
4. `vissue tree <id>` and `vissue children <id>`. Open descendants
   are not GREEN.
5. Four voters. Each: `VISSUE_AGENT=<seat> ljos vote <id> --for accept|reject`.
6. `ljos consensus <id>` must hold accept at 1.000 with all four
   seats present. Do not report a vote count as consensus.
7. `ljos complete` on the session node. Completing does not close
   the ticket.

Point the host procedure file at this page. Do not put the loop in a
public repository as process narration.
