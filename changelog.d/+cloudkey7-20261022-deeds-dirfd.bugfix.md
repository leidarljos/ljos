Publishing a deed bag keeps the directory fd from the open of `.ljos/deeds`
and stages, swaps, and publishes with mkdirat, openat, and renameat on that
fd, then fsyncs the directory. A symlink swapped in for `.ljos` or
`.ljos/deeds` after the check is not where the bag is written. Atom logs
are classified and written the same way, and the atoms merge driver writes
through that fd.
