A search that already holds the environment lock migrates legacy preferences by taking that lock before the migration gate, so two searches cannot each wait for the lock the other holds.
