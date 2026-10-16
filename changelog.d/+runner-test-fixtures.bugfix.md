The guard-shim, force-push, and harness-hook tests create the home and working directory they use.
Those directories sit beside the crate, mode `0o755`, because a clean runner has no `/workspace` and cannot create one.
