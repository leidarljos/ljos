A tracker write whose change another writer's commit already carried, such as a board split, now commits the write's seat line instead of saying nothing to commit.
A note in that case commits its seat line even inside the batch window, since the line is the only change in its file; a note whose file was already changed still waits for the batch commit.
A write that misses the tracker lock, or meets a tracker checkout git cannot read, keeps its seat line in the file for the commit that lands it.
Persisting the same write again does not add a second seat line.
A board split that is kept because its commit was refused and it could not be rolled back stamps the waiting write's seat line in the kept issue file.
