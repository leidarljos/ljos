`ljos remember --json` printed nothing when every chunk was already stored.
The command now prints one claims document, with those claims in the array.
When a later chunk fails, that same document lists what was stored and what failed, and the command exits 1.
