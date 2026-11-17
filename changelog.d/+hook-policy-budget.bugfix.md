Policy checks run before waiting for memory context, so a slow lookup leaves time for the command verdict.
Hook admission reserves time for the verdict even when its requested wait exceeds the wall budget.
A slow repository read cannot consume the command checker's entire wait; one bounded reserve is shared by all checks in the hook.
