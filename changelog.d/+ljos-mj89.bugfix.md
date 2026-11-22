A freshly written executable is published by rename from a temporary file written in another process, so a fork in this process does not keep a write descriptor on it.
Starting that file no longer fails with "Text file busy" while another thread is still forking.
`ljos offboard` publishes each file it puts back the same way, so it no longer writes and chmods a hook that was not executable through a symlink at its name.
Removing the ljos block from a git hook that keeps user lines publishes the hook the same way and keeps its mode, and a copied executable is flushed to disk before the rename and its directory after it.
Adding the ljos block to a git hook of your own keeps that hook's mode, so a 700 or 750 hook is not widened to 755.
An executable is written inside a new directory of its own, beside the destination, before the rename, so a symlink another user left at a temporary name is never written through or made executable, and a failed publish names the path it could not create or rename onto.
