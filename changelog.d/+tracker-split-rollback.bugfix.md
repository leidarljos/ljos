A board split whose commit is refused is rolled back only when it is still exactly what the migrate wrote: the marker digest matches `issues.org`, the issues directory holds one file per heading and nothing more, and no issue file has a record appended.
A note or a new issue that landed in the split before the rollback no longer gets deleted with it; the split is kept and the write waits with it.
