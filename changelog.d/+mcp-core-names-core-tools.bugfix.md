The core MCP tool descriptions no longer send an agent to a tool the default set does not list.
`ljos_release` now points finished work to `ljos_finish` rather than `ljos_complete`, and `ljos_claim` says `ljos_finish` frees the slot.
`ljos_vote` says `outcome` on `ljos_finish` scores the confidence, `ljos_send` names only `ljos_reply`, and `ljos_search` drops its pointer to `ljos_island`.
`ljos_doctor` now asks whether this client is onboarded.
