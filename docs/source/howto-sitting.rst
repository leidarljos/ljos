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
   warn    crates.io   did not answer, so the version check was skipped; run `ljos doctor` again once it answers.
   matched by words (encoder not installed)
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

``ljos note "TEXT"`` writes on the open ticket. ``ljos finish --lesson "..."`` ends it. With no ticket open, each exits 1 and names ``ljos sitting ISSUE``. ``ljos ready --mine`` names the next unfinished item, or the open sitting's ticket when that ticket is not a plan. ``ljos finish ITEM`` ticks that item off. A sitting without ``--verbose`` prints the claim, one line when claims were handed back or are still held, what the seat remembers, and the close. A degraded pack is one line, ``matched by words (encoder not installed)``. A pack that answers 503, or does not answer, is one line, ``These are word matches; the pack did not answer.``, and the sitting continues. That search waits at most fifteen seconds when the encoder is cold, and about a second otherwise. Quiet claims are that one line, ``N quiet claims are still yours (ljos release --stale)``. A hand-back is the same line, ``handed back N quiet claims``. Each claim is on ``--verbose``. A claim untouched for longer than ``LJOS_STALE_DAYS`` (7) is left out of that count. The line is ``N claims untouched for 7+ days: ljos claims --stale``. ``ljos claims --stale`` lists them, oldest first. ``ljos release --stale`` releases this seat's claims that have been quiet longer than the hand-back window and whose conversation is not running. ``--dry-run`` prints the count and changes nothing. ``--days`` with ``--yes`` hands back claims untouched for that many days. The ticket stays open.

Start a sitting, and end one
============================

.. code:: console

   $ ljos sitting proj-1a2b --verbose
   sitting proj-1a2b: checking blockers
   == blockers
   none open; the issue is workable
   == reclaim
   no claim quiet longer than 900s
   == claim
   gen=2
   tracker: proj-1a2b STARTED under alice
   == doctor
   ok  vissue  ...
   ok  seat    alice, holding as alice (from LJOS_SEAT)
   == cards
   == due
   unreviewed  conclusion  3f9c... The lexical default is BM25+.
   1 due; 4 scheduled, next at 2026-09-13 09:12 +00:00
   == island: Ship the fuse change?
   1.000   seed    ab8c... 3 weeks ago CombMNZ over RRF for fusing two ballots.
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

The due section, the timeline and the recall list share one subject rule on this issue's text and history. Weak words such as review and order do not count. The project name is not a subject word, in the title or on its own line, and the older word list does not put that name back. A lesson that shares only that name stays out. The name comes out before weak words are judged, so ``Review the kiln`` on project ``kiln`` has review as its subject. An identifier in the title, such as review_order, keeps a lesson that contains that name. A title word is the project name when its pieces, split at a hyphen, an underscore or a change of case and taken in lower case, are the project's pieces, so ``oak-tree``, ``OAK_TREE`` and ``OakTree`` on project ``oak-tree``, and ``a-b`` on project ``a-b``, are neither subject words nor identifiers. A word with any other piece still counts, such as ``sha-2`` on project ``sha-1`` or ``oak_2`` on project ``oak``. The same rule applies to the issue's body. A word is a run of letters and digits in any script, or such runs joined by a hyphen or an underscore, and every other character ends it, so ``sha-2`` in ``sha-2.txt``, ``sha-2:8080``, ``sha-2@example.com`` or ``https://example.com/sha-2/issues`` still counts, and ``école-2`` is kept as written. When the issue's text and history together have no strong word, the older word list is the subject. A weak title with a strong body still uses those strong words, so a lesson that only shares the weak title stays out. The score is that share times the memory's weight, and it has to reach 0.34. This seat's own lesson weighs 1.15, so a share of about 30 percent clears it. A lesson of weight 1 needs 34 percent. A lesson filed on this issue is kept. A lesson filed on another issue, including one in the same project, has to clear the rule. A memory that names another ticket of this project and not this issue stays out, such as ``On café-1-wxyz the damper needs oil.`` on ``café-1-lk1e``. That ticket id is read as the same word, so ``école-2`` is not a ticket of project ``cole``. The weak list is the English list lesson search uses. A ``playbook:`` note is not part of that history. The due count covers those related memories only. When nothing relevant is due, the verbose due section says ``nothing relevant is due``. The short form leaves that line, and the count, for ``--verbose``. A pack that has remembered nothing yet still says so. A sitting's timeline lists the related memories with the tracker's events. The short form's line of what the seat remembers is that recall list. A pack read that does not answer still applies the same rule to the slot's word matches.

One verb, in the protocol's order; it stops at the first store that does
not answer and claims nothing. ``--playbook NAME`` copies a recipe
(``sit``, ``arena``, ``land``, ``company-panel``, ``overnight``, ``feature``) into the
playbook section before recall; absent a name, a closed-set token in the
title else ``sit``. The name is a tracker ``playbook:`` note until finish or
release. ``ljos panel`` refuses until one is bound. The ``start_a_sitting`` prompt
gives the same order to a runner that prefers single tools. A claim
refused as busy names the issue you still hold; ``ljos complete`` completes it
and ``ljos release`` hands it back.

.. code:: console

   $ ljos finish proj-1a2b --lesson "CombMNZ held on turns. RRF lost two points." --outcome ship
   remembered the lesson as agent-derived, unreviewed (id 51778929); search finds it now
   Learned. 6 trust rows rewritten. ... persona anchors moved. The outcome is kept: once 6 issues have one, ljos consensus discounts voters who err together. ... No stated probability, so there is no Brier score. A hard vote is not a claim of certainty.
   sitting ended; ticket proj-1a2b is still open

Memories, a no vote, and a child
================================

A seat remembers with no sitting open. ``--as NAME`` stores the lesson in
the set ``persona-NAME``. Define each persona first with ``ljos persona`` before calling ``ljos brief``. Each persona's brief then carries that persona's
lesson, and the lists are not the same.

.. code:: console

   $ ljos remember --as release-engineer "Cut a release only after the changelog names the search index."
   remembered (lesson): Cut a release only after the changelog names the search index.
     id 21e499e4, next review 2026-11-02 14:29 +00:00
   saved as a standing lesson; run `ljos search "QUERY"` to read it
   $ ljos remember --as docs-reader "A newcomer looks for the search index in the notes."
   remembered (lesson): A newcomer looks for the search index in the notes.
     id bac3e6e5, next review 2026-11-02 14:29 +00:00
   saved as a standing lesson; run `ljos search "QUERY"` to read it
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

   $ ljos vote demo-0gmm --against "the notes omit the index" --used deed-9f3a --as perf-reviewer
   label ballot-eligibility-warning: vote: a no ballot on demo-0gmm has to name a child ticket. `ljos file --parent demo-0gmm "TITLE"` files one, then `ljos vote demo-0gmm --against REASON --child ID` links it. Nothing was cast
   $ ljos file --porcelain "Name the search index in the notes" --parent demo-0gmm
   demo-1zcv
   $ ljos vote demo-0gmm --against "the notes omit the index" --used deed-9f3a --as perf-reviewer --child demo-1zcv
   demo-0gmm: perf-reviewer voted no
   demo-0gmm: 1 vote from 1 option
     no                       1 (perf-reviewer)
     one ballot only: no, which nobody has agreed with yet
   recorded on the tracker: perf-reviewer for no on demo-0gmm
   ballot signed by perf-reviewer with key d1e96e866f17… session alice
   unverified session: nothing named the run but seat alice, so every run of that seat is one session
   perf-reviewer cast by alice, parent alice
   tracker git: committed chore(issues): demo-0gmm ballot cast by alice; no remote, kept local
   demo-0gmm: noted
   no ballot linked to child demo-1zcv. demo-0gmm stays open while demo-1zcv is open. After `ljos finish demo-1zcv --close`, replace the ballot: `ljos vote demo-0gmm --withdraw --as perf-reviewer` then `ljos vote demo-0gmm --for OPTION --used DEED --as perf-reviewer`, then `ljos consensus demo-0gmm`.

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
