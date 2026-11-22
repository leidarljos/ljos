A published executable is created, marked, and renamed into place through descriptors held on its parent directory and its stage directory, and written by a forked child instead of `/bin/sh`, `cat`, `cp` and `chmod`.
This stops the owner of a shared sticky directory from having another file overwritten or made executable, or a symlink published, by renaming the stage directory and putting a symlink at its name.
When the staged file cannot be flushed to disk, the publish fails before the rename.
A failed publish removes only the staged file and the stage directory it made, and leaves any name someone else put there.
A copied executable keeps the mode of its source without setuid, setgid and sticky, so `ljos upgrade` no longer turns a 700 binary into a 755 rollback copy.
`ljos upgrade` and `ljos cloud up` publish each new binary straight onto its installed name.
No copy waits at a guessable name such as `ljos.new` that another user could replace with a symlink.
