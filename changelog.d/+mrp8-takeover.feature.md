`ljos claim ID --takeover` takes a ticket over from another seat whose claim looks stale, and `ljos_claim` takes `takeover` for the same step through `ljos mcp`.
A claim is stale when it has been quiet for at least `LJOS_LEASE_SECS` (900 seconds when unset) and no running conversation on this host is named for the ticket.
A running or recently active holder is refused with a non-zero exit and nothing changes.
A takeover that goes ahead leaves a note on the ticket naming the old holder.
The refusal a plain `ljos claim` prints for a stale holder says so and names the command.
