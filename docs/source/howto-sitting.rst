:description: Open a sitting, end it with a lesson, and hold a done claim that sits beside a failing test.
:cover: https://leidarljos.github.io/ljos/_static/og.png

============
Sit an issue
============


Open the day's work
====================

A plan files the parent and its checklist in one tracker commit, claims the parent, and prints each item with its id.
Set ``LJOS_SEAT`` to the seat that files it. Once ``vissue.toml`` lists seats, that seat has to be on the list. ``ljos seat add`` writes the row.

.. BEGIN ljos-sample sitting-plan

.. code:: console

   $ ljos sitting --plan "Ship the codec" -p day -- "measure the files" "pick a default"
   day-ab12  Ship the codec
     day-cd34  measure the files
     day-ef56  pick a default
   next: day-cd34  measure the files
   tracker: day-ab12 STARTED under ada
   These are word matches; the pack did not answer.
   warn    crates.io   did not answer, so the version check was skipped
   matched by words (encoder not installed)
   next: day-cd34  measure the files
   day-ab12 is yours. `ljos finish day-ab12 --lesson "..."` closes the sitting; `ljos sitting day-ab12 --verbose` prints every section.
   $ ljos finish day-cd34
   finished day-cd34; your open sitting is day-ab12
   ticked day-cd34
   next: day-ef56  pick a default
   $ ljos ready --mine
   next: day-ef56  pick a default
   $ ljos ready
   ready  shown 20 of 21
   open and not waiting on an open ticket, so run `ljos show ID`.

   day-ab12  STARTED  [#C]  Ship the codec
   ...
   See more with `--limit` or `--all`. Narrow with `-p` or `--mine`.

.. END ljos-sample sitting-plan

Plain ``ljos ready`` prints 20 tickets and ends a short page with the ``--limit`` and ``--all`` hint shown above.

``ljos note "TEXT"`` writes on the open ticket. ``ljos finish --lesson "..."`` ends it. With no ticket open, each exits 1 and names ``ljos sitting ISSUE``. ``ljos ready --mine`` names the next unfinished item. ``ljos finish ITEM`` ticks that item off. A sitting without ``--verbose`` prints the claim, one line when claims were handed back or are still held, what the seat remembers, and the close. A degraded pack is one line, ``matched by words (encoder not installed)``. A pack that answers 503, or does not answer, is one line, ``These are word matches; the pack did not answer.``, and the sitting continues. That search waits at most fifteen seconds when the encoder is cold, and about a second otherwise. Quiet claims are that one line, ``N quiet claims are still yours (ljos release --stale)``. A hand-back is the same line, ``handed back N quiet claims``. Each claim is on ``--verbose``. A claim untouched for longer than ``LJOS_STALE_DAYS`` (7) is left out of that count. The line is ``N claims untouched for 7+ days: ljos claims --stale``. ``ljos claims --stale`` lists them, oldest first. ``ljos release --stale`` releases this seat's claims that have been quiet longer than the hand-back window and whose conversation is not running. ``--dry-run`` prints the count and changes nothing. ``--days`` with ``--yes`` hands back claims untouched for that many days. The ticket stays open.

Start a sitting, and end one
============================

.. code:: console

   $ ljos sitting proj-1a2b --verbose
   == doctor
   ok  vissue  ...
   ok  seat    alice, holding as alice (from LJOS_SEAT)
   == cards
   == due
   unreviewed  conclusion  3f9c... The lexical default is BM25+.
   1 due; 4 scheduled, next at 2026-09-13T09:12:00Z
   == island: Ship the fuse change?
   1.000   seed    ab8c... 3 weeks ago CombMNZ over RRF for fusing two ballots.
   == blockers
   none open; the issue is workable
   == playbook
   sit
   A sitting on one issue. Name this recipe at open (`ljos sitting ISSUE --playbook sit` or `ljos playbook ISSUE sit`). The sitting prints this body before recall and holds the name until finish or release.
   ...
   == recall
   ...
   == timeline
   timeline of proj-1a2b: Ship the fuse change?
   events  shown 12 of 12, oldest first

   2026-08-30, 13 days ago, tracker
     created
   ...
   == reclaim
   no claim quiet longer than 900s
   == claim
   gen=2

One verb, in the protocol's order; it stops at the first store that does
not answer and claims nothing. ``--playbook NAME`` copies a recipe
(``sit``, ``arena``, ``land``, ``company-panel``, ``overnight``, ``feature``) into the
playbook section before recall; absent a name, a closed-set token in the
title else ``sit``. The name is a tracker ``playbook:`` note until finish or
release. ``ljos panel`` refuses until one is bound. The ``start_a_sitting`` prompt
gives the same order to a runner that prefers single tools. A claim
refused as busy names the issue you still hold; ``ljos complete`` finishes it
and ``ljos release`` hands it back.

.. code:: console

   $ ljos finish proj-1a2b --lesson "CombMNZ held on turns. RRF lost two points." --outcome ship
   finished proj-1a2b as done
   remembered the lesson (id 51778929) as agent-derived; the hook shows it from the next prompt, and `ljos memories review` prunes it
   Learned. 6 trust rows rewritten. ... persona anchors moved. The outcome is kept: once 6 issues have one, ljos consensus discounts voters who err together. ... No stated probability, so there is no Brier score. A hard vote is not a claim of certainty.
   the ticket proj-1a2b stays open and nobody holds it; a person closes it when the work is accepted

Memories, a no vote, and a child
================================

A seat remembers with no sitting open. ``--as NAME`` stores the lesson in
the set ``persona-NAME``. Each persona's brief then carries that persona's
lesson, and the lists are not the same.

.. code:: console

   $ ljos remember --as release-engineer "Cut a release only after the changelog names the search index."
   21e499e4b8487a3abc7ff7479e9da102    lesson  due 2026-11-02T14:29:43.738Z    Cut a release only after the changelog names the search index.
   saved as a standing lesson; run `ljos search` to read it
   $ ljos remember --as docs-reader "A newcomer looks for the search index in the notes."
   bac3e6e51514703c76672d163cc38cde    lesson  due 2026-11-02T14:29:43.770Z    A newcomer looks for the search index in the notes.
   saved as a standing lesson; run `ljos search` to read it
   $ ljos brief release-engineer demo-0gmm
   What you remembered yourself:
   - [lesson, from an agent, today] Cut a release only after the changelog names the search index.
   $ ljos brief docs-reader demo-0gmm
   What you remembered yourself:
   - [lesson, from an agent, today] A newcomer looks for the search index in the notes.

A no vote has to name an open child. ``ljos file --parent`` links it in the
tracker. The parent stays open while that child is open. After the child
is closed, withdraw the no ballot and vote again, then ``ljos consensus``.

.. code:: console

   $ ljos vote demo-0gmm --against "the notes omit the index" --used none --as perf-reviewer
   Error: vote: a no ballot on demo-0gmm has to name a child ticket. `ljos file --parent demo-0gmm "TITLE"` files one, then `ljos vote demo-0gmm --against REASON --child ID` links it. Nothing was cast
   $ ljos file --id-only "Name the search index in the notes" --parent demo-0gmm
   demo-1zcv
   $ ljos vote demo-0gmm --against "the notes omit the index" --used none --as perf-reviewer --child demo-1zcv
   recorded on the tracker: perf-reviewer for no on demo-0gmm
   no ballot linked to child demo-1zcv. demo-0gmm stays open while demo-1zcv is open. After `ljos finish demo-1zcv --close`, replace the ballot: `ljos vote demo-0gmm --withdraw --as perf-reviewer` then `ljos vote demo-0gmm --for OPTION --used none --as perf-reviewer`, then `ljos consensus demo-0gmm`.

Hold a done claim beside a red test
===================================

``Stop`` also blocks one turn, with no model in the loop, when the
conversation has no issue and the turn used tools without touching
the seat. The agent is told to file an issue and sit. The next stop of
that same turn goes through.

With Jev on, the ``Stop`` and ``SubagentStop`` hooks audit the turn once
before the agent stops. The audit reads four parts of the turn, bounded
to a few thousand tokens:

- the person's last request
- the shell commands the turn ran since that request
- the latest test output, or the last output when no test ran
- the final message from the agent

Jev answers three questions about it:

=================== ==========================================================================
question            blocks the stop at
=================== ==========================================================================
``claims_complete`` 0.9 or above, with ``tests_green`` 0.1 or under and a test run in the turn
``tests_green``     (with the above)
``deferral``        0.9 or above
=================== ==========================================================================

Whether a test ran is decided in code, from the command line. A held
agent gets the reason and one more round; its second attempt to stop is
never audited, so it cannot loop. A final message under 80 characters is
not sent. The cuts sit in ``jev.rs`` beside the questions. A stop costs
about 3,000 input tokens, or $0.00013.
