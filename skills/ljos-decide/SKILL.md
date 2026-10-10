---
name: ljos-decide
description: Put a choice with more than one defensible answer to a vote and read the result with ljos vote and ljos consensus. Use for design choices, trade-offs and any decision the person wants recorded with reasons, and when a pushed change must cite a decision.
---

# Decide by vote

A decision lives on a ticket. Each voter casts one ballot, and
`ljos consensus` reads the result, weighting voters by how often they were
right before.

1. File the decision: `vissue q -p PROJECT -t decision "Choose X or Y for Z"`.
   List the options on a body line that starts `Options:`.
2. Open a sitting on it (`ljos sitting ISSUE`). A decision ticket binds the
   panel recipe and writes one brief per persona.
3. Cast ballots. Your own:

   ```
   ljos vote ISSUE --for OPTION --confidence 0.7 --used none
   ```

   `--used` names the deeds (records of work) the ballot drew on, or `none`.
   Persona ballots come from subagents, one per brief, each ending with
   `ljos vote ISSUE --for OPTION --as NAME`. The ljos-panelist agent does this.
4. Read it: `ljos consensus ISSUE`. Act on the shares when the voters have
   come together (polarization near zero). When they have not, the result is
   not a position the group reached; say so.
5. When the world shows which option was right:
   `ljos finish ISSUE --outcome OPTION`. Voters who were wrong weigh less next time.

Personas help think a choice through; their ballots do not settle a decision
a push cites. A push that needs a decision needs a ballot from a seat other
than the one pushing.
