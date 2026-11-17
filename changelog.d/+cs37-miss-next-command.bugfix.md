A search that matches nothing and has no shorter word to try, such as `ljos search xyzzy` in a store that has memories, names the next command.
The line is `Nothing matched xyzzy. Try browsing memories, such as: ljos memories list`.
`--json` puts `ljos memories list` in `suggestion`, and the search tool in `ljos mcp` reads the same line.
