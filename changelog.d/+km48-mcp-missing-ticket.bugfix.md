`ljos_claim`, `ljos_release` and `ljos_complete` over `ljos mcp` now refuse a ticket id the tracker does not have, as `ljos claim`, `ljos release` and `ljos complete` do.
`ljos_claim` used to mint a session node for a mistyped id, and `ljos_release` told the caller to `ljos claim` a ticket that does not exist.
