An empty or relative `XDG_STATE_HOME` is no longer a state directory under the working directory.
`cargo test` runs in the package directory, and that relative `ljos` wrote memory, sync and mail state into the crate.
A seat with no absolute `HOME` keeps that state in a new private directory, mode `0700`, removes it on exit, and is told the directory is temporary.
A directory that already exists or is a symlink is refused, and the refusal names `HOME` and `XDG_STATE_HOME`.
