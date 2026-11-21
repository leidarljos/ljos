Hash regular files in the `/tmp` scan of `scripts/ci-test-hermetic.sh`, so a same-length overwrite with a restored mtime, or a same-size replace that reuses the inode, fails the run.
