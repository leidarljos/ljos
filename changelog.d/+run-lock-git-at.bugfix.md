`workflow --start` finds the per-run lock directory of a tracker whose `.git` is a file without following a `GIT_DIR` exported to it.
Two starts under different exported repositories now share one lock.
