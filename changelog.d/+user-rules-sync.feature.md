User rules sync through the tracker repository, one file per machine, and merge by id so the newest `updated_at` wins.
A deletion is a tombstone, and a hook that cannot reach the tracker still shows the rules on this machine.
