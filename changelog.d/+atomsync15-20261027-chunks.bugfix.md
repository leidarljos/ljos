ljos sync sealed and committed each host's whole atoms log on every
change, so the tracker grew by a full copy of the log each sync. A sync
now seals only the atoms that were added, changed or dropped, as one
small chunk beside the log, and leaves the base file alone. The first
sync after upgrading seals each log whole once, and the syncs after it
write chunks.
