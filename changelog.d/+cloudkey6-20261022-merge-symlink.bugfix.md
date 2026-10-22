The atoms merge driver reads this host's fresh log, and writes git's
merge result, without following a symlink. A tracker that committed
this host's log as a link to a file outside it had that file copied into
the merge result and pushed. Resolving a rebase writes the log the same
way, and the conflict-marker check reads `.ljos` files without following
a link.
