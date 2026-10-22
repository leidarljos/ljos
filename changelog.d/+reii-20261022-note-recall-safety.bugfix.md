Prompt recall no longer injects `note` memories as facts: hook recall and
issue recall injected into prompts now leave notes out.
`ljos recall` still lists notes, but labels them as unverified agent notes
and includes provenance tags where known.
The MCP refusal for non-person `remember`/`prefer` text is now neutral and
no longer tells the model to resave that text through `ljos_note`.
