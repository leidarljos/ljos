`ljos sync --adopt-legacy HOST` reads a host's logs from before the scope
header, such as a gone box's cloud log sealed to this machine's key. It
pins the SHA-256 of each such blob filed under HOST now, in this
machine's state, and import and `--adopt` read exactly those bytes as
HOST's. A copy under another name and any other blob stay unread, and
the pin lapses once HOST writes a headed log.
