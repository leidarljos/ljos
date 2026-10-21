The MCP server answers `tools/list` when the home directory is empty and no tracker or seat is set.
Each listed schema is a JSON Schema object, with the draft identifier and schemars-only formats left off, so a strict client can load the list.
A tool call with no seat still returns that error.
