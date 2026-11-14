A handoff that has gone away or stopped answering stops `ljos mesh pull`, `ljos mesh push`, and a memory export or import that writes that folder.
The line names the folder, the error in words, and the command to run once the mount answers, and the command exits 1.
The count is printed only when the write finished.
The file is written under a temporary name in that folder and renamed into place.
`LJOS_SHARE_WAIT` is how long one operation may sit, 60 seconds when unset.
