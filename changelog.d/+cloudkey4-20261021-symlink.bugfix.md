`ljos sync --adopt` refuses a scope log that is not a regular file.
A symlink, hard link, or fifo checked in as a gone host's log is left
in place, so a link to a machine-key file is not decrypted and resealed
to the cloud recipients. Ciphertext is passed to age on stdin.
