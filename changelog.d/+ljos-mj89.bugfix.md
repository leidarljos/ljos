A freshly written executable is published by rename from a temporary file written in another process, so a fork in this process does not keep a write descriptor on it.
Starting that file no longer fails with "Text file busy" while another thread is still forking.
`ljos offboard` publishes each file it puts back the same way, so it no longer writes and chmods a hook that was not executable through a symlink at its name.
Removing the ljos block from a git hook that keeps user lines publishes the hook the same way and keeps its mode, and a copied executable is flushed to disk before the rename and its directory after it.
