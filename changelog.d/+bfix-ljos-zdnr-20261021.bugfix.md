Tracker setup now treats a directory as an existing checkout only when
that directory is itself the git toplevel, so scratch tracker paths
inside another repository are initialized as their own trackers and no
longer write lock, exclude or merge-driver state into the enclosing
checkout.
