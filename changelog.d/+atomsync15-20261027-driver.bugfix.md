The ljos-atoms merge driver exported this host's log into the work
tree and then copied the file git asked for. A rebase that replayed
this host's digest stopped on "Your local changes would be
overwritten". The driver now seals the log in memory from the pack and
writes only the file git hands it.
