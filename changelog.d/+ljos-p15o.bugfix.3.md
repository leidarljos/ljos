The `doctor --fix` tests no longer leave seat homes under `~/.cache/ljos-test-seats`.
Each one stops the pack server it started, removes its seat directory when it is done, and removes `~/.cache/ljos-test-seats` once it is empty.
