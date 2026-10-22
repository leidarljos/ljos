`ljos record check` now applies seat pins by key fingerprint, so hex-case
differences and small-order-part variants of the same key are treated as
the same signer.
An explicit voter-id pin row now takes precedence over a conflicting seat
row, while seat rows still reject a second key on that seat.
Seat rows now cover run/judge aliases of a seat (`run-inky`,
`judge-inky`) and keep true run/model forms on their own rows.

Tally is now per question and key-based: one signing key is one voter on
that question. A seat that appears under a second key on the same
question keeps the first key's ballot and refuses the later key on that
seat. Same-hour ambiguity drops only that voter's ballots on that
question, so unrelated questions and voters still count.
`replaces` now follows the effective voter identity (signing key), not
the raw voter-id string.
