`CommitLock::acquire_by` keeps its `Option` return and its callers keep their behaviour.
`CommitLock::try_acquire_by` returns the open or `flock` error for a caller that must not go on without the lock, and `workflow --start` uses it.
