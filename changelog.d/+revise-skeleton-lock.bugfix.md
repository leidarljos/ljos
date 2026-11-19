While `workflow revise --from` reads the stored skeleton, files the proposal and writes the index back, it keeps the skeleton lock.
A `define` or `--apply` that ran in between is no longer overwritten by the older copy.
