Stop `scripts/ci-test-hermetic.sh` from following a symlink to the scan root or out of it, so a link from `~/.ljos` or `/tmp/ljos-$UID` to an unreadable or foreign directory no longer fails a clean run or reports foreign files.
Record such a link by its target string, and record an unreadable directory inside the root instead of failing the scan.
