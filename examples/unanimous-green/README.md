# Unanimous green

Two implementers, two reviewers, four ballots. GREEN is `ljos consensus`
holding accept at 1.000 after those seats have drawer ballots, and a
clean descendant ledger. A count of four accepts is not the model.

The loop is harness-neutral. One host has a Rhai runner for it. A
Task host and a procedure-file host run the same seats as their own
workers and still call `ljos`. Completing a claimdag node does not
close the ticket.

## Seats

| Seat | Identity |
|---|---|
| Implementer | `impl-A`, `impl-B` |
| Reviewer | `rev-1`, `rev-2` |

`VISSUE_AGENT` is that identity on every `ljos vote`.

## Join

1. `claimdag upsert` then `ljos claim --assignee <32-hex> <node>`.
2. Implement in isolation. File every defect as a child before voting.
3. Freeze the product: `deedar create file|patch ...` then
   `ljos deed <id> --add <accession>`.
4. `ljos recall <id>` is the working set. `ljos evidence` / `ljos current`
   on every cited accession.
5. Each seat: `VISSUE_AGENT=<seat> ljos vote <id> --for accept|reject`.
6. GREEN: ledger `open_count=0` and `ljos consensus <id>` holds accept
   at 1.000. `vissue consensus` is the tracker half of the same verb.
7. `ljos complete <node>` ends the session lease. It does not close
   the ticket.

## Hosts

- Rhai host: disk script `unanimous-green.rhai` in the local workflows
  directory. Launch with `script_path`.
- Task host: `task-host.md`.
- Procedure-file host: `procedure-host.md`.

Wire the seat the same way on every host:

```
<host> mcp add ljos -- ljos-mcp
```

## What is not GREEN

- Schema `accept=true` with no drawer ballot.
- Four accepts in the drawer and no `holds: accept (1.000 ...)`.
- `ljos consensus` failing because `vissue vote --json` is missing.
  Install a vissue that prints that document.
- Open TODO/STARTED/BLOCKED descendants.
- A DONE stamp on a child whose claimed fix is not in the files.
