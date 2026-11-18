:description: Pull lane exports into a seat and push that seat's memories to the shared handoff.
:cover: https://leidarljos.github.io/ljos/_static/og.png

========================
Move memory across lanes
========================

A lane writes its memories into a shared directory. The seat that keeps the
copy reads that directory, stores one lesson per text, and writes a seed
file whose bytes follow from those texts.

Pull lane exports into a seat
=============================

.. code:: console

   $ LJOS_SEAT=box ljos mesh pull --seat box --from /srv/handoff
   imported 12, unchanged 0, skipped 3 duplicates
   exports skipped 0, imported 4

``--from`` is the handoff directory. A missing directory prints
``Error: mesh: PATH does not exist. Create it, or pass a directory of <lane>.jsonl files``
and exits 1. Create it, or pass a directory that exists. A path that is a
file prints
``Error: mesh: PATH is not a directory. Pass a directory of <lane>.jsonl files``
and exits 1. An empty directory prints
``imported 0, unchanged 0, skipped 0 duplicates`` and
``exports skipped 0, imported 0``. Each ``*.jsonl`` file is one lane, and the
lane name is the file name with ``.jsonl`` removed. A file whose name ends in
``.seed.jsonl`` is a seed already written for a seat, and ``pull`` leaves it
unread. A line that is a header, or a line that is not a lesson, is left
unread as well.

The text's SHA-256 is the identity of a lesson. So is the lane plus the row
id, when the line carries ``atom_id``, ``row_id`` or ``id``. The first copy
is imported. A copy already in the pack is unchanged, and is not
imported again. A repeated text in this same pull is a duplicate. The
command prints ``imported N, unchanged U, skipped M duplicates``, then
``exports skipped S, imported I``.
``S`` is exports this run did not import.
``I`` is exports whose bytes were new to the seat's record of that file.

A new row is stored in the seat's import slot with the labels
``imported,unintegrated``. ``pull`` then integrates the seat's imports.
That is the same work as a full ``ljos memory integrate``, and the pull
script does not run that verb itself. Integrate keeps one copy of a text already in the pack,
posts a new one once, and relabels the row
``imported,integrated``.

The atom stored for a lesson carries a packset tag ``lane:<name>`` for each
lane that held the text, and ``seat:<seat>`` for the seat that received it.
A lesson with no horizon becomes standing, so search ranks it with this
seat's own memories. ``LJOS_SEAT`` is that seat, the same name as ``--seat``.

After an export is stored, the seat records that file's size, its
modification time and a hash of its bytes.
The next pull stats each export.
When the size and the modification time match, the file is not read.
When either has changed, the pull reads the file and hashes the bytes.
The same bytes are not imported again, so a copy that only touched the file
does not add a row.
A second pull of an unchanged directory imports nothing and reads no export.
The seed bytes stay as they were.

Each export is stored before the next one is read.
A pull that is killed, or that stops because the writer missed its deadline,
leaves the finished exports in the seed.
The export in progress is left out, so the seed never holds half of it.
The next pull continues with the exports that are not stored yet.
A row the writer already took is recorded once and is not posted again.

On a terminal, pull prints one line per export,
``mesh pull: NAME ACTION (N/TOTAL)``.
``ACTION`` is ``imported N`` or ``skipped``.
When standard error is not a terminal, those lines are omitted.
The two summary lines are the output.

The seed is ``<tracker>/.ljos/memory/mesh/<seat>/seed.jsonl``. Rows are sorted
by the content hash. The file carries no clock, so the same lessons write
the same bytes.
The bytes land under a temporary name in that folder and take the real name
by rename.
The file is rewritten only when its bytes would change.

``pull`` takes an exclusive lock on the handoff directory and on the seat.
Two pulls of one seat wait, and the seed holds one copy of each text.

A writer that answers 503 is asked again once, after the ``Retry-After`` it
sent, with the same deadline. A second 503, a 408, or a writer that is not
answering stops the pull. The line is one line, and it exits 1:

``Error: mesh: the pull stopped: CAUSE, and lessons already stored stay in the seed with rows still missing, so run `ljos mesh pull --seat SEAT --from DIR`.``

A 408 uses ``the deadline passed before this request ran`` as ``CAUSE``. A second
503 uses the writer's line, or ``packsetd busy, retrying later``. A writer that
is not answering uses the reason this seat prints for a down writer, and
that reason names ``cargo install --locked packset`` or ``packset ensure``.
Exports already stored stay in the seed.
The export in progress is left out, and the next pull reads it again.
A row the writer did not take is one of the rows in that export.

Push a seat to the handoff
==========================

.. code:: console

   $ ljos mesh push --seat lane-a --lane lane-a --to /srv/handoff
   pushed 12 memories to /srv/handoff/lane-a.jsonl

``--lane`` is the file name, without ``.jsonl``. When the flag is omitted the
name is ``LJOS_SYNC_HOST``, then the hostname. A missing directory is created.
The file is one
``ljos-memory-1`` export of the seat's seed and the lessons already stored
for that seat, with one row per text. A second push of the same memories
writes the same bytes.

``push`` takes the same pair of locks. A pull that is reading the directory
waits until the file is in place.

A folder that has gone away
===========================

A handoff on a network mount or a FUSE mount can answer
``Transport endpoint is not connected``, an input/output error, or a stale
file handle, or a write can sit and never return. ``pull`` and ``push`` stop,
and so does an export or an import that writes that folder. The line names
the folder, the error in words, and the command to run once the mount
answers. The exit is 1. The count is printed only when that write finished.
The bytes land under a temporary name in the same folder and take the real
name when the write has finished, so a later pull reads a whole seed. One
operation waits 60 seconds. ``LJOS_SHARE_WAIT`` sets that wait, in seconds.

``Error: mesh: /srv/handoff: Transport endpoint is not connected. Check the mount, then rerun `ljos mesh pull --seat box --from /srv/handoff`.``
