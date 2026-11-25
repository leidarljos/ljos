The tracker-place test `a_trackers_own_repository_is_not_a_product_checkout` takes the environment lock and clears the tracker variables before it runs.
Without the lock it read the `VISSUE_PREFIX` another test sets for a moment, and its git steps could follow the `GIT_DIR` and `GIT_WORK_TREE` other tests export, so it failed now and then on a busy run.
