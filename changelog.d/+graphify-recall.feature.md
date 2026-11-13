`ljos recall` adds a Code context section when `graphify-out/graph.json` is in the repository.
The section lists matching nodes with file and line, and a few edges tagged EXTRACTED or INFERRED.
`--no-graph` and `LJOS_GRAPHIFY=off` leave it out. A schema major other than 1 is refused in that section.
`ljos_recall` returns the same section.
