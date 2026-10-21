`ljos sync --adopt HOST` now keeps a gone host's live atoms by adopting them
into this machine's log before dropping that host's log file from the tracker.
The command removes the gone host tag from adopted atoms, re-seals this host's
log, and then commits the log update with the removed host files.
