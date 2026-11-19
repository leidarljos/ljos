A project can keep a versioned workflow skeleton and run it as a ticket graph.
`ljos workflow define` stores the Org file as a deed.
`ljos workflow` with `--start` files one ticket per step, with block edges for dependencies.
A signoff step waits for the other steps, open blockers, a consensus on `ship` and any required deed.
`ljos workflow revise` folds notes from a run back into the next skeleton version.
`ljos workflow render` writes a public Org page and checks it.
A parent that already runs a skeleton cannot start the feature workflow.
A dependency listed twice is refused when the skeleton is defined.
Step kind is read from the ticket tags, so a later note cannot change it.
A `--var` argument that is not key=value is refused without repeating the argument.
