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

   $ ljos mesh pull --seat box --from /srv/handoff
   imported 12, skipped 3 duplicates

``--from`` is the handoff directory. Each ``*.jsonl`` file is one lane, and the
lane name is the file name with ``.jsonl`` removed. A file whose name ends in
``.seed.jsonl`` is a seed already written for a seat, and ``pull`` leaves it
unread. A line that is a header, or a line that is not a lesson, is left
unread as well.

The text's SHA-256 is the identity of a lesson. The first copy is imported.
Every later copy of that text is a duplicate. The command prints one line,
``imported N, skipped M duplicates``.

The atom stored for a lesson carries a packset tag ``lane:<name>`` for each
lane that held the text in that pull, and ``seat:<seat>`` for the seat that
received it. A second pull of the same directory imports nothing and skips
every row. The seed bytes stay as they were.

The seed is ``<tracker>/.ljos/memory/mesh/<seat>/seed.jsonl``. Rows are sorted
by the content hash. The file carries no clock, so the same lessons write
the same bytes.

``pull`` takes an exclusive lock on the handoff directory and on the seat.
Two pulls of one seat wait, and the seed holds one copy of each text.

A writer that answers 503 is asked again once, after the ``Retry-After`` it
sent, with the same deadline. A second 503 or a 408 stops the pull. Lessons
already stored stay in the seed, and the next pull continues with the rows
still missing.

Push a seat to the handoff
==========================

.. code:: console

   $ ljos mesh push --seat lane-a --lane lane-a --to /srv/handoff
   pushed 12 memories to /srv/handoff/lane-a.jsonl

``--lane`` is the file name, without ``.jsonl``. When the flag is omitted the
name is ``LJOS_SYNC_HOST``, then the hostname. The file is one
``ljos-memory-1`` export of the seat's seed and the lessons already stored
for that seat, with one row per text. A second push of the same memories
writes the same bytes.

``push`` takes the same pair of locks. A pull that is reading the directory
waits until the file is in place.
