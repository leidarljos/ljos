`ljos record check` and entry signing now refuse voter ids whose part after `/`
is not already that seat's slug, so `alpha/Seat-a` cannot be counted beside
`alpha/seat-a` under one pinned key.
