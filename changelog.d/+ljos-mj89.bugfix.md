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
`ljos git-hooks install` refuses a hook that is a symlink, and `ljos git-hooks uninstall` refuses one whose target contains the ljos block.
Both refuse before they write any hook, so neither one changes a link or its target, and the error names the link and its target and says to replace the link with a copy.
