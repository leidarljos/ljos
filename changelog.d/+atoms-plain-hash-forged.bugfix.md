A sync skipped this machine's atom log whenever `plain.sha256` matched its atoms, so a changed hash in the checkout hid an added or dropped atom and was committed as it was.
A sync now skips the log only when this home noted, in a signed note keyed to the bytes of the snapshots and segments, that they open to those atoms, and never reads what `plain.sha256` says.
