`ljos sync` refuses a tracker whose `.ljos` or `.ljos/atoms` is a
symlink. The scope file, the atoms logs and this host's export are read
and written under directory fds opened with `O_NOFOLLOW` from the
tracker root, so a linked directory cannot bring a local log sealed to
this machine into the import or take this host's log out of the tracker.
