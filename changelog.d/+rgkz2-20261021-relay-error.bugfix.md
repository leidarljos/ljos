A relay that cannot read its stdin, or whose hook panics, now denies
a tool call in that runner's own contract. Exit 1 with no output used
to let the command run on nine of the twelve runners. Gemini, Copilot,
Cline and Auggie get a deny object, because exit 2 alone allows the
command there. A prompt error still exits 1.
