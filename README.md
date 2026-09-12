# ljos

One seat over five habitats and argv law. It does not own them.

Citation is not a merge. Completing a claimdag node does not close a ticket. Cards are read-only. Consensus is a [different crate](https://github.com/leidarljos/consensus).

```
ljos remember "the default fuse is CombMNZ"
ljos prefer "CombMNZ over RRF"
ljos search fuse
ljos evidence deed-…
ljos deed vissue-xxxx --add deed-…
ljos recall vissue-xxxx
ljos claim <node>
ljos cards
ljos policy -- ls
ljos consensus vissue-xxxx
ljos trust alice bob 0.8 --why deed-…
ljos learn vissue-xxxx --outcome ship
```

`remember` / `prefer` POST `/v1/atoms` against `PACKSET_URL` (`INSIDE_MEMORY_URL` is an alias). They write one explicit claim. They do not extract from a transcript.

`cards` prints `USER.md` and `MEMORY.md` only. It never writes them.

`ljos policy` prints the argv. It never calls `grokos policy reload`. Reloading a Janet pack is not a check. When `grok-policyd` exists it is the TCB; this binary is not.

`ljos consensus` reads the pack's live `trust` rows, execs `ljos-consensus settle --issue --trust` with them, then `vissue consensus`. No rows: every voter weighs the same.

`ljos trust FROM TO WEIGHT` writes one row as a `trust` atom, citing deeds with `--why`. `ljos learn ID --outcome OPTION` reads the ballots, shrinks every refuted voter in every other voter's row by `--beta` (default 0.5, Hedge, doi:10.1006/jcss.1997.1504), floors at 0.01, and writes the complete set back. Trust is memory: rows carry a validity window, a later row supersedes, and `packset export` carries them in a handover.

## MCP

`ljos-mcp` serves the same verbs over stdio. Writers: `ljos_remember`, `ljos_prefer`, `ljos_trust`, `ljos_learn`, `ljos_deed`, `ljos_vote`, `ljos_claim`, `ljos_complete`. The rest read. Cards are the resources `ljos://cards/USER.md` and `ljos://cards/MEMORY.md`, from `LJOS_CARDS_DIR`. Prompts: `start_a_sitting`, `check_a_handover`.

```json
{"mcpServers": {"ljos": {"command": "ljos-mcp", "env": {"PACKSET_URL": "http://127.0.0.1:8761"}}}}
```

Other projects may still speak packset, deedar, vissue, or claimdag alone.

Site: <https://leidarljos.github.io>
