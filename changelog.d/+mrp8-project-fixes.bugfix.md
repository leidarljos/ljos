`ljos remember` with `-p` or `project` through `ljos mcp` keeps the project name when the pack is down so the flushed lesson still carries its `project:` entity.
`ljos search` with `-p` and `ljos_search` with `project` filter hits by project before applying `--min-score`, so another project's weak match no longer suggests `--min-score 0` on an empty project search.
`-P` on a verb that takes `-p` suggests `-p`, and `ljos start -p` without `--plan` prints the `sitting` plan hint.
