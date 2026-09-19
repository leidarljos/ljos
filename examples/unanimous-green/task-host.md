# Unanimous green (Task host)

Same loop as the Rhai host. This host has no workflow runner for that
script. Two isolated implementer workers, then two reviewers. `ljos`
is the seat.

## Setup

```
<host> mcp add ljos -- ljos-mcp
```

`ljos`, `vissue`, `deedar`, `claimdag`, and `ljos-consensus` on PATH.

## Seats

`impl-A`, `impl-B`, `rev-1`, `rev-2`. Export `VISSUE_AGENT` to that
name on every vote.

## Host loop

1. `ljos recall <id>`. If Produced is empty after a tree change, the
   implementers have not cited a deed.
2. Launch two isolated implementers. They do not share a tree. Each
   reads the ticket, implements, freezes a deedar product, and runs
   `ljos deed <id> --add <accession>`.
3. Apply one tree into the checkout. Prefer the independently correct
   implementation.
4. `vissue tree <id>` and `vissue children <id>`. Any TODO, STARTED,
   or BLOCKED descendant is not GREEN. Descend those children first.
5. Four voters, `VISSUE_AGENT` set. Each:
   - reads the shipped files
   - files every defect as a child before voting
   - `ljos vote <id> --for accept` or `--for reject`
6. Host reads `ljos consensus <id>`. GREEN only when it holds accept
   at 1.000 and all four seats are in the drawer. A count is not the
   model.
7. `ljos complete` on the session node. Do not mark the ticket DONE
   from a child.

## Reject

A reject is a defect. The next implement round does that gap, then
the four seats vote again.
