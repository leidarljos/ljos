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
ljos due
ljos graded <atom-id> [--lapsed]
ljos handover --out bag --project x --issue vissue-xxxx
ljos receive bag [--since bridge.txt] [--import]
ljos doctor
```

`remember` / `prefer` POST `/v1/atoms` against `PACKSET_URL` (`INSIDE_MEMORY_URL` is an alias). They write one explicit claim. They do not extract from a transcript.

`forget ID` POSTs `/v1/atoms/delete` and is the other direction. The pack tombstones rather than erases, so the claim stops being recalled and the record that it was held and withdrawn stays; `packset atoms --as-of TS` reads it back through the window it was live in. `--why` names the deed the retraction stands on, citing deeds the same way `trust` does, and the pack refuses free text in its place.

`cards` prints `USER.md` and `MEMORY.md` only. It never writes them.

`ljos claim ID --assignee NAME` and `ljos complete ID` take a tracker id or a 32-hex claimdag id. A tracker id maps to one node (FNV-1a 128 of the id, minted with the id as its summary on first use) and a name to one actor the same way, so the session graph stays outside the accession join while the seat speaks tracker ids.

`ljos policy` prints the argv. It never calls `grokos policy reload`. Reloading a Janet pack is not a check. When `grok-policyd` exists it is the TCB; this binary is not.

`ljos consensus` reads the pack's live `trust` rows, execs `ljos-consensus settle --issue --trust` with them, then `vissue consensus`. No rows: every voter weighs the same.

`ljos trust FROM TO WEIGHT` writes one row as a `trust` atom, citing deeds with `--why`. `ljos learn ID --outcome OPTION` reads the ballots, shrinks every refuted voter in every other voter's row by `--beta` (default 0.5, Hedge, doi:10.1006/jcss.1997.1504), floors at 0.01, and writes the complete set back. Trust is memory: rows carry a validity window, a later row supersedes, and `packset export` carries them in a handover.

## MCP

`ljos-mcp` serves the same verbs over stdio. Writers: `ljos_remember`, `ljos_prefer`, `ljos_forget`, `ljos_trust`, `ljos_learn`, `ljos_graded`, `ljos_deed`, `ljos_vote`, `ljos_claim`, `ljos_complete`, `ljos_handover`, `ljos_receive`. `ljos_forget` is the only one annotated destructive, because it is the only one that takes something away. The rest read. Cards are the resources `ljos://cards/USER.md` and `ljos://cards/MEMORY.md`, from `LJOS_CARDS_DIR`. Prompts: `start_a_sitting`, `check_a_handover`.

```json
{"mcpServers": {"ljos": {"command": "ljos-mcp", "env": {"PACKSET_URL": "http://127.0.0.1:8761"}}}}
```

`ljos due` lists the atoms whose review clock has run out; `ljos graded ID` marks one recalled (`--lapsed` for the other answer) and the pack reschedules it. This is the spaced-review loop the pack already keeps, reached from the seat.

`ljos handover --out DIR` runs the tracker's satchel, `packset export` into `DIR/data/atoms`, `deedar export` of every deed the satchel needs or the pack cites into `DIR/data/deeds`, then seals, and signs the manifest when `DEEDAR_HOST_SIGNING_KEY` is set. `ljos receive DIR` verifies the manifest, checks the deed receipts (`--since` for the bridge from a kept head), checks the signature when there is one, counts the atoms and trust rows, and with `--import` posts the atoms into this seat's pack.

`ljos doctor` says which habitats answer and exits 1 when the tracker, the deed store, or the pack does not. It also says whether `DEEDAR_HOST_SIGNING_KEY` names a 32-byte seed; without one a handover goes out unsigned.

Other projects may still speak packset, deedar, vissue, or claimdag alone.

Site: <https://leidarljos.github.io>
