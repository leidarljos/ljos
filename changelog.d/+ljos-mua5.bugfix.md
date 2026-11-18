Tracker git operations pass `gc.auto=0` so commits, syncs, and fetches do not stall on inline repacks, and `ljos finish` and `ljos sync` invoke `git -c gc.autoDetach=true gc --auto -q` out of band.
