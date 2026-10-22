`ljos cloud up` recorded whichever repository it was run in.
A second run from another clone rewrote the allowlist, and a
push of develop then landed there. Doctor still reported ok.

The first repository stays recorded. A later `cloud up` leaves
it in place, and the shell guard refuses `--repoint`.
