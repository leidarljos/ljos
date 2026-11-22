ljos opens the destination directory the user configured, following a symlink there, and keeps a descriptor to it.
ljos creates, marks, syncs, renames and removes its own names relative to that descriptor and never follows a symlink at them.
Temporary stage names use operating-system randomness and exclusive creation.
ljos returns an error when a file or directory sync fails.
The error says the new file is in place when a directory sync fails after a rename.
`ljos upgrade` then says it replaced the binary or kept the rollback.
On failure, cleanup removes the staged file and removes the stage directory when its original name still refers to it.
Mode changes no longer retry through a pathname after an open is denied.
