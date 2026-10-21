Every ljos write to a tracker checkout's index now holds one lock, `.ljos-commit.lock` at the top of the worktree, kept out of `git status` through `info/exclude`.
A tracker commit holds it from the status read through the logbook stamp, the add, the commit and the seat-key pin; sync's commits, the push's rebase or merge, and the merge-attributes commit take it too.
A seat's first key pin is written under it and by rename instead of being rewritten in place and staged with no lock, so parallel first ballots no longer lose a pin or meet on `index.lock`.
