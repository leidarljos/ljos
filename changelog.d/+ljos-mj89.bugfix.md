A freshly written executable is published by rename from a temporary file written in another process, so a fork in this process does not keep a write descriptor on it.
While another thread is still forking, that file now starts without failing with "Text file busy".
`ljos offboard` publishes each file it puts back the same way.
It no longer writes and chmods through a symlink left at the name of a hook that was not executable.
When ljos removes its block from a git hook that keeps user lines, it publishes the hook the same way and keeps its mode.
ljos flushes a copied executable to disk before the rename and the destination directory after it.
Adding the ljos block to your own git hook now keeps its mode: a 700 or 750 hook stays 700 or 750.
ljos writes an executable inside a new directory of its own, beside the destination, before the rename.
It never writes through a symlink another user left at a temporary name or makes it executable.
A failed publish names the path it could not create or rename onto.
When ljos rewrites a git hook, it drops setuid, setgid and sticky.
When a hook lacks an execute bit, ljos gives one to its owner and to each class that can read it.
`ljos git-hooks install` refuses a hook that is a symlink before it writes any hook.
`ljos git-hooks uninstall` leaves a symlinked hook whose target has no ljos block and strips the other hooks.
It refuses a symlink whose target contains the ljos block before it changes a hook, and the error names the link and its target and says to replace the link with a regular file.
Rollback copies are flushed under unpredictable temporary names and published without replacing an existing final name, so a crash cannot leave a partial rollback at that final name.
`ljos upgrade` skips a binary that already reports the release.
It saves the old binary as `NAME.bak-pre-VERSION`.
When that name is taken, it uses the first free `NAME.bak-pre-VERSION.N`.
It reads the copy back before it replaces the binary, and names it in the output after the replace.
An entry already at one of those names, of any kind, is left where it is and never counts as the rollback.
Lint rejects rustix open, openat, chmod, link, symlink and rename calls.
Each call except chmod can create or replace a hook, and chmod can change its mode.
