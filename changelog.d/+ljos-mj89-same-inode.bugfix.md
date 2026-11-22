After the rename, a publish checks that the destination is the file it staged.
The publish reports a hard link to another file of the same owner put there in that gap, and does not take it for the published executable.
When two publishes go into one directory, each takes a lock on it from the rename to that check, so neither reads the other's file as a replacement.
A publish waits at most 5 seconds for that lock and then fails with an error that names the directory.
