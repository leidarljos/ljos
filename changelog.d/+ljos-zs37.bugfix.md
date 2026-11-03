Tracker git over ssh runs in batch mode.
An exit-128 error that did reach the remote is reported as a refused push.
A manual `ljos sync` in a checkout with no scope follows `LJOS_TRACKER_GIT`.
Git and ssh for a tracker push run with `LC_ALL=C`, so a refusal is read in English.
An empty `core.sshCommand` falls through to batch ssh.
