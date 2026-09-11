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
```

`remember` / `prefer` POST `/v1/atoms` against `PACKSET_URL` (`INSIDE_MEMORY_URL` is an alias). They write one explicit claim. They do not extract from a transcript.

`cards` prints `USER.md` and `MEMORY.md` only. It never writes them.

`ljos policy` prints the argv. It never calls `grokos policy reload`. Reloading a Janet pack is not a check. When `grok-policyd` exists it is the TCB; this binary is not.

`ljos consensus` execs `ljos-consensus settle --issue` first, then `vissue consensus`.

Other projects may still speak packset, deedar, vissue, or claimdag alone.


Unanimous green (two implementers, two reviewers, `ljos consensus`
is GREEN) lives in [`examples/unanimous-green/`](examples/unanimous-green/).
A Rhai host, a Task host, and a procedure-file host run the same seats.

Site: <https://leidarljos.github.io>
