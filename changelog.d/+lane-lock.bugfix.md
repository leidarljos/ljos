A tracker commit no longer stops when git still holds `index.lock`, including while a background push runs.
The write stays on disk, the line says the commit is deferred to the next write or push, and that next write or push commits it.
ljos does not remove an `index.lock` it did not create.
`ljos finish` warns when this machine's log cannot take a chunk, leaves the atoms in the pack, and names `ljos sync` as the command that seals the log whole.
