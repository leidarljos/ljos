The pin-ignore test names its decoy `GIT_DIR` on the git command and then drops it.
Other tests in the same process no longer inherit that directory.
