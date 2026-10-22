`ljos sync` keeps the record of what it took from each host under the
state directory's `sync/` without following a symlinked `sync`,
`sync/<tracker>` or `sync/<scope>` directory. A linked one is named and
the import stops.
