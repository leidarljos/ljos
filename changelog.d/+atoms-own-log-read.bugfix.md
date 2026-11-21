Each sync ran `age -d` on every snapshot and segment of this machine's own log to find what was new, and a large log that did not open in time left the sync unable to add atoms.
A sync now keeps the plaintext of each piece it seals, so its next sync reads its own log without `age -d`.
