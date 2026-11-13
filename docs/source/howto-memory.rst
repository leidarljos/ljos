:description: Keep the cards, record a number that is measured again and again, and retire a memory with its reason.
:cover: https://leidarljos.github.io/ljos/_static/og.png

===================
Keep memory current
===================


Remember a lesson
=================

``ljos remember`` stores one lesson as written. A claim is two sentences at
most. A longer text is stored as one claim per pair of sentences, and the
line names that and ``ljos search``.
The pack's limit is 29 words a sentence. A sentence the person types is
kept, and ``ljos remember --verbose`` names the count and the limit.
A sentence from anywhere else that passes that limit is kept on this
machine only. The command says the pack refused it, that ``ljos search``
still finds the copy, and that the hook does not show it. Split the
sentence and remember each part.
``ljos recall ISSUE`` with no lesson on that issue says so and names
``ljos search`` and ``ljos remember``. Lessons that matched the title or
the issue's tags, and were not filed on the issue, are listed under
``related``. An id the tracker does not hold names ``ljos search``,
``ljos ready`` and ``ljos file``.
With no ``LJOS_SEAT``, the command says no seat was named and uses
``unknown-`` plus the host name. Set the seat before a lesson you want
under your own name.

A pack that answers 503, including ``packsetd is busy: memory pressure``,
queues the lesson in ``~/.local/state/ljos/note-queue.jsonl``. The next
``ljos search`` or ``ljos sitting`` sends it under the same deadline as
``ljos remember``, and says the lesson is safe and will be sent
automatically. Load is not a reason to drop the lesson. A refusal on
content names the case: size, or policy. A lesson that duplicates an atom
already in the pack prints that atom id.

Keep the cards
==============

Cards are two files you write in the working directory: ``USER.md`` and
``MEMORY.md``. ``LJOS_CARDS_DIR``, or ``ljos cards --dir``, names another directory.
``ljos cards`` prints the files. It does not write them. With neither file
it names the directory. The pack is where the seat writes.

Keep a number
=============

A number the seat measures again and again, a benchmark score, a hook
latency, a count of open tickets, is a habit. A reading is one verb.
The one below is an example:

.. code:: console

   $ ljos habit mab-cr-all 0.535 --unit acc --every 7d --source example-0000
   7b1c... habit   due 2026-09-26T10:00:00.000Z    habit mab-cr-all stands at 0.535 acc (example-0000).
   $ ljos habit mab-cr-all 0.579 --unit acc --every 7d --source example-0001
   9e0a... habit   due 2026-10-03T09:12:41.000Z    habit mab-cr-all stands at 0.579 acc (example-0001).
   was 0.535 acc (8 days ago), now closed
   $ ljos habit
   mab-cr-all  0.579 acc   +0.044 since 0.535 (8 days ago) today   next reading in 7 days  example-0001

The second reading supersedes the first, so the pack keeps one live value
a habit and ``ljos search mab-cr-all --as-of 2026-09-15`` still answers
what it stood at then. The cadence is the reading's review clock: when a
week passes with no new reading, ``ljos due`` lists the habit and the hook
says so once a session. The sheet a paper's table is written from is the
pack, and it is dated.

Close what a later lesson replaced
==================================

.. code:: console

   $ ljos consolidate
   closes 3f9c…  The default fuse is Borda.
       for 51ee…  The default fuse is CombMNZ.
   1 of 97 live memories would close; `ljos consolidate --apply` closes them
   $ ljos consolidate --apply
   closes 3f9c…  The default fuse is Borda.
       for 51ee…  The default fuse is CombMNZ.
   1 of 97 live memories closed

A write closes the earlier claim it rewrites on arrival (same opening
words, a new object; a correction; an explicit supersedes). A pack written
before that rule, or filled by a handover, has pairs the rule never saw;
``consolidate`` reports them, ``--apply`` closes them. The closed claim keeps
its window: ``ljos search --as-of`` finds it at the time it was live.

Retire a claim with its reason
==============================

.. code:: console

   $ ljos search "lexical default"
   $ ljos forget a6ae2751 --why deed-review-2026-09

Copy the id from ``ljos search``. A prefix of 8 or more characters is enough.
``--why`` is a deed id. Omit it when you have no deed. An id the pack does
not hold names ``ljos search`` again.
``ljos memories review --edit ID --text TEXT`` writes a new row and keeps
it. ``ljos memories list`` leaves that row out. ``ljos memories list --all``
shows it, and the edit line names ``ljos forget`` for the new id. An id
that matches nothing names ``ljos memories list --all --user`` and
``ljos search``.
The pack tombstones the claim and records the deed that withdrew it.
Disagreeing with a claim is not showing it wrong; a retraction names its
evidence.
