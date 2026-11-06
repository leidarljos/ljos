# Portable memory

A seat can move its lessons to another machine as one file.

`ljos memory export` writes this seat's lessons to a file on the machine that runs it.
The file is not sent anywhere.
`ljos memory import` stores that file in the seat tracker.
`ljos memory integrate` maps the rows onto lessons already stored for this seat.
`ljos memory imports` lists the slots.

The tracker path is `<tracker>/.ljos/memory/imports/<seat>/<sha256>/`.
`manifest.json` records the format, the two seats, both times, and the input digest.
`payload.jsonl` is the file bytes.
`receipts/` stores one file per importing machine.
`integration/` stores one file per mapping.
A repeated import finds those files and does not write a second copy.

Those path segments are names, so they are percent-encoded.
`.` and `..` are encoded as well, so a seat name cannot choose a parent directory.
Linux allows 255 bytes in one directory entry.
An encoded name longer than 200 bytes is stored by its SHA-256, and the manifest keeps the name.

The file format is `ljos-memory-1`.
One header line names the exporting machine, seat, time, and the package version.
Each later line is one lesson.
The line carries the source atom id, a SHA-256 of the text, and a SHA-256 of the atom id, a newline, and the text.
That second digest names the source text.
A file with another format, or a line that is not JSON, is kept and labelled.
Search still finds the words.

Each of those times is stored as given, offset included.
A listing shows that stored timestamp.
It also shows the same instant in the process zone, from the system clock.
A value that does not parse is shown raw and labelled `time-unparsed`.

A labelled value stays in the listing as raw text; export of that file chooses which lessons go into it.
Another seat's lessons stay out.
A lesson whose source carries a password, token, key, or environment field stays out.
Text the pack rejects as a credential or as an instruction stays out.
Embeddings are not written.

Import labels each row `imported` and `unintegrated`.
Integrate then labels the mapping.
The same text maps to one canonical id and the event lists every source.
The same source id with different text keeps both rows and is labelled `version`.
Two texts are near copies when the words they share are at least four fifths of the words in the union.
They both stay, with reciprocal `near-duplicate` links.
A pair that shares those words and differs by one English negation is a `conflict`.
The negation words include `not`, `never`, `no`, and the contracted forms with the apostrophe removed.
Both statements stay.
Integrate does not pick a winner.

`--dry-run` prints the mappings and writes nothing.

`ljos search` reads the slot as well as the pack.
`--source-machine`, `--source-seat`, and `--import-id` match those fields exactly.
Those filters run before the result limit cuts the list.
With no filter, pack hits keep their order.
Slot hits that share a query word are added after them.

Slot hits also include a lesson the pack's prose check would split.
The original is stored whole.
The label is `prose-warning`, `oversize`, or `content-warning`, matching what the check reported.
The fitted copies can still be posted.
The original remains one record.

An empty write is stored with an empty text and the label `empty-input`.
No claim is invented.

A note with no sitting is stored with the label `unattached-note`.
It is not filed on a ticket.

A queued note that has to wait stays queued.
Later notes are still sent.
A copy the pack refuses leaves the queue and stays in the slot.
