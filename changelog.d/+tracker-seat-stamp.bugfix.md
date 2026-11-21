A tracker write whose change another writer's commit already carried, such as a board split, now commits the write's seat line instead of saying nothing to commit.
A write that misses the tracker lock, or meets a tracker checkout git cannot read, keeps its seat line in the file for the commit that lands it.
Persisting the same write again does not add a second seat line.
