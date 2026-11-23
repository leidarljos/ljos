A board split whose commit is refused is rolled back only when it is still exactly what the migrate wrote: the marker digest matches `issues.org`, the issues directory has one file per heading and nothing more, and no issue file has a record appended.
A rollback no longer deletes a note or a new issue that reached the split first. The split is kept, and the write waits with it.
