A tracker write whose change another writer's commit already carried, such as a board split, now commits the write's seat line instead of saying nothing to commit.
Even inside the batch window, a note whose only change is its seat line commits at once. With other changes in the file, it waits for the batch as before.
If its first git status read fails, a note commits instead of holding the line for the batch commit with no error.
A single-file write that misses the tracker lock keeps its seat line in the file until a later commit carries it.
A write that spans more than one project adds no seat line until it gets the lock.
A tracker checkout git cannot read gets no seat line.
Persisting the same write again does not add a second seat line.
When a board split is kept because its commit was refused and could not be rolled back, a waiting single-file write puts its seat line in the kept issue file.
