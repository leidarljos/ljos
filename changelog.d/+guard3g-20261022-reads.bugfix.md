`curl -LO`, `-fsSLO`, `-JLO`, `-C - -O`, `-i`, and `-4` were
refused, and so were `--write-out`, `--retry-all-errors`,
`gh run download`, `gh label list`, `gh cache list`, and a
GraphQL query. Those are reads this seat runs.

A short or long flag that only names a read stays allowed. A
GraphQL operation is a read when it is not a mutation.
