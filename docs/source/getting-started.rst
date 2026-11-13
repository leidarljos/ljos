.. [[[cog import ljos_version as v; v.lines("", ":description: A plan is how a seat opens a day's work: one parent, an ordered checklist, and the next unfinished item. This page installs ljos, stores a lesson, opens that plan, and puts a second seat on a ticket. It describes ljos {version}.", ":cover: https://leidarljos.github.io/ljos/_static/og.png", "", "A plan is how a seat opens a day's work: one parent, an ordered checklist, and the next unfinished item. This page installs ljos, stores a lesson, opens that plan, and puts a second seat on a ticket. It describes ljos {version}.", "")]]]

:description: A plan is how a seat opens a day's work: one parent, an ordered checklist, and the next unfinished item. This page installs ljos, stores a lesson, opens that plan, and puts a second seat on a ticket. It describes ljos 0.41.8.
:cover: https://leidarljos.github.io/ljos/_static/og.png

A plan is how a seat opens a day's work: one parent, an ordered checklist, and the next unfinished item. This page installs ljos, stores a lesson, opens that plan, and puts a second seat on a ticket. It describes ljos 0.41.8.

.. [[[end]]]

Do the sections in order. They install ljos, store a lesson, search it, and open a day's work as a plan.
The last section puts a second seat on a single ticket. The how-to guides cover one task each after that.
Mem0 stores memory by running a model over what it ingests. The lesson below is stored as written.
beads keeps a ticket queue in Dolt. The ticket below is an Org heading in a git repository.

Install
=======

Linux and macOS are supported. Windows works through Windows Subsystem for Linux (WSL) and is untested.

From a checkout of this repository:

.. code:: console

   $ sh scripts/install.sh --no-embed
   $ export PATH="$HOME/.local/bin:$PATH"

A release attaches that script:

.. code:: console

   $ curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh --no-embed

The ``install.sh`` attached to the 0.41.1 release asked for ``vissue-cli-<target>.tar.xz``. The 0.41.2 script fetches ``vissue-v<version>-<target>.tar.gz``.
The vissue release publishes ``vissue-v<version>-<target>.tar.gz``. Use the script in this tree until a later release.

The programs go to ``~/.local/bin``. They are ``ljos``, ``ljos-mcp``, ``packset``,
``packsetd``, ``vissue``, ``deedar``, ``claimdag``, ``ljos-policyd`` and ``ljos-consensus``.
The installer checks each download against its published SHA-256 sum.
It builds from source with ``cargo`` where a platform has no release.
``--no-embed`` leaves out the encoder. Search then matches words, and a search prints ``ranking is lexical: encoder unavailable``.

Read the script before you run it. It is ``scripts/install.sh`` in this repository.
It runs ``ljos onboard`` for each coding runner it finds. With none found, it still sets up
the tracker, the pack writer and the deed store.

The tracker commits each change, so git needs an identity:

.. code:: console

   $ git config --global user.name "Your Name"
   $ git config --global user.email "you@example.com"
   $ ljos doctor
   ljos is ready: memory, tracker and command guard work. Optional extras: `ljos doctor --all`.

On a clean install the ready line names memory, the tracker and the command guard.
A row under it names anything that still needs action. Add ``--all`` to see every row.

To install by hand, or to see what onboarding would write:

.. code:: console

   $ cargo binstall --locked ljos packset deedar-cli claimdag-cli vissue-cli vissue-mcp ljos-policyd ljos-consensus
   $ ljos onboard --dry-run
   $ ljos onboard
   $ ljos doctor

``ljos onboard`` writes the MCP entry, the hooks and the skill into each runner it finds.
``ljos onboard --example`` lists the runners ljos knows.

Nothing else needs a variable set. ``ljos onboard`` makes a deed store, a tracker and a
config file for ``vissue``, and it starts the pack writer.

The command harness runs a hook only after you trust it. Open ``/hooks`` in the command harness and trust each ljos hook.
Trust it again after an upgrade rewrites one.

A lesson and a search
=====================

.. code:: console

   $ ljos remember "The lexical default is BM25+. It beat BM25 by two points on turns."
   ljos: no seat was named; this write uses unknown-cursor. Set one with --seat NAME or export LJOS_SEAT=NAME.
   remembered (lesson): The lexical default is BM25+. It beat BM25 by two points on turns.
     id a6ae2751, next review 2026-11-12 21:40 +00:00
   standing: the hook shows it from the next prompt it bears on
   $ ljos search "lexical default"
   ranking is lexical: encoder unavailable
   1.0000	2/2	lesson, agent-derived	a6ae2751564de44a99c5915a6d9ea7f2	today	The lexical default is BM25+. It beat BM25 by two points on turns.

The id and the review time are the ones that run printed. The name after ``unknown-`` is the host name.
This run sat under an agent runner, so the search row says ``lesson, agent-derived``.
Typed at a terminal with no runner above it, the same command is the person's sentence.
Search matches words when the encoder is absent. ``ljos search fusing`` finds a preference that contains that word.
``ljos search "which fusion"`` shares no word with either sentence and prints ``No hits.``
This seat's hits come first. Another seat's lesson is printed when it names the subject, or when the encoder's cosine with the query is at least 0.66.
That line is the one already used for a hit the encoder lists at the top: on the measured set, an unrelated prompt's best cosine stayed at 0.653.
A weaker cross-seat hit does not fill a short list. When those were the only rows, the line is ``Nothing relevant was found.``
A busy pack ranks by words and still returns a hit that names the subject.

.. code:: console

   $ ljos prefer "CombMNZ over RRF for fusing two ballots."
   remembered (preference): CombMNZ over RRF for fusing two ballots.
     id f8b725a9, next review 2026-11-12 21:40 +00:00
   $ ljos search fusing
   ranking is lexical: encoder unavailable
   1.0000	2/2	unverified agent preference (origin agent-derived)	f8b725a9c82783a79a3a01d185244770	today	CombMNZ over RRF for fusing two ballots.

``ljos due`` lists claims when their review time arrives. A claim you stop reviewing sinks in search.
The how-to :doc:`Keep memory current <howto-memory>` is the review clock, ``ljos graded`` and ``ljos forget``.

A ticket
========

.. code:: console

   $ LJOS_SEAT=alice ljos file "Pick a codec" -p demo -t decision --options keep,drop
   demo-2f5j
   project: demo
   demo-2f5j  TODO  [#C]  Pick a codec
   file: ~/.local/share/vissue/tracker/Software/demo/issues.org
   tracker git: committed chore(issues): demo-2f5j filed by alice; no remote, kept local

The id is the only line on standard output, so ``id=$(LJOS_SEAT=alice ljos file ...)`` captures it.
The heading, the path and the commit line are on standard error. The id in the heading is the same one.
Under a runner that already recorded a seat, ljos names both and writes as ``LJOS_SEAT``.
With no git identity the heading is still written, and the last line is ``tracker git: commit refused: Author identity unknown. Set git user.name and user.email, then run the command again``.
``ljos doctor`` then warns that the issue file is uncommitted.

The file is Org. ``vissue show demo-2f5j`` prints the heading, and the tracker repository is the one ``ljos doctor --all`` names under ``tracker``.

Open the day's work
===================

A plan files one parent and its checklist in a single tracker commit, claims the parent, and prints the items with their ids. The next unfinished item is the one to do. ``ljos finish`` on that id marks it done and names the one after it. ``ljos ready --mine`` names the same next item later, without opening the sitting again.

.. code:: console

   $ LJOS_SEAT=alice ljos sitting --plan "Ship the codec" -p day -- "measure the files" "pick a default" "write the note"
   day-ab12  Ship the codec
     day-cd34  measure the files
     day-ef56  pick a default
     day-gh78  write the note
   next: day-cd34  measure the files
   tracker git: committed chore(issues): day-ab12 filed, day-cd34 filed, day-ef56 filed, day-gh78 filed by alice; no remote, kept local
   sitting day-ab12: checking blockers
   day-ab12 is yours. `ljos finish day-ab12 --lesson "..."` closes the sitting; `ljos sitting day-ab12 --verbose` prints every section.
   next: day-cd34  measure the files
   $ LJOS_SEAT=alice ljos finish day-cd34
   ticked day-cd34
   next: day-ef56  pick a default
   $ LJOS_SEAT=alice ljos ready --mine
   next: day-ef56  pick a default

The ids are the ones that run printed. The parent is claimed. Each item stays open until ``ljos finish`` ticks it. A sitting with no ``--verbose`` prints what needs action. A degraded pack is one line, ``memory degraded: ranked by words``. Quiet claims are a count that points at ``ljos claims --mine``. ``ljos sitting ISSUE --verbose`` prints every section.

A second seat
=============

One seat holds a ticket at a time. Bob's claim is refused, and the refusal names alice:

.. code:: console

   $ LJOS_SEAT=alice ljos claim demo-2f5j --assignee alice
   gen=2
   tracker: demo-2f5j STARTED under alice
   $ LJOS_SEAT=bob ljos claim demo-2f5j --assignee bob
   label claim-conflict: claim: demo-2f5j is held by another (alice, seat alice, still running, ...), not by bob (this one).

The rest of bob's line names ``ljos release``, ``ljos note`` and ``ljos send``. The ticket stays with alice.
Bob can file another ticket. To help on this one, bob writes a note.

.. code:: console

   $ LJOS_SEAT=alice ljos note demo-2f5j "the codec is one file"
   demo-2f5j: noted
   tracker git: committed chore(issues): demo-2f5j noted by alice; no remote, kept local
   $ ljos timeline demo-2f5j

``ljos timeline`` prints the note as unverified tracker text, fenced, oldest first.
``vissue show demo-2f5j`` prints the same logbook. ``ljos recall`` prints the working set for the ticket: the plan, the deeds it cites, and memories. The note is tracker text, so it is on the timeline, not in that working set.
``ljos digest`` reads the tracker and says what each seat filed, claimed and voted on.

Both seats can vote. From one shell the two ballots share a session, and ``ljos consensus`` folds them into one voice:

.. code:: console

   $ LJOS_SEAT=alice ljos vote demo-2f5j --for keep --used none
   $ LJOS_SEAT=bob ljos vote demo-2f5j --for keep --used none
   $ LJOS_SEAT=alice ljos consensus demo-2f5j --no-file
   folded into one voice (same session ...): alice, bob
   counted bob
   settle (DeGroot, 1 voters): keep 1.000
   every voter is on keep: the settle is unanimous
   polarization 0.000, disagreement 0.000

A ballot is its own voice when the cast has no shell, no agent runner and no earlier row that shares a session, a parent or the root session of one runner.
The how-to :doc:`Vote and settle a decision <howto-vote>` is that case, and ``ljos learn``.
The docs of beads, Mem0, Letta, Zep, Basic Memory and git-bug, read on 2026-11-10, describe no vote. The comparison is :doc:`How ljos compares <compare>`.

Take it off
===========

.. code:: console

   $ ljos offboard

``ljos offboard`` removes the ljos entries from each runner's files and leaves the rest of each file as it was.
With no runner onboarded it prints ``ok``, ``offboard``, ``nothing installed by ljos onboard``.

.. code:: console

   $ ljos uninstall

``ljos uninstall`` goes further: every runner is offboarded, the global git hooks go back,
the ljos programs are removed, and a stub named ``ljos`` stays behind to answer every hook
with an allow until each running agent session restarts. With the binary already gone,
``curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/uninstall.sh && sh uninstall.sh``
does the same.

Where next
==========

- :doc:`How-to guides <howto>`: one task a page. Runners, the review clock, a sitting, mail, a signed handover, votes, and the command check.
- :doc:`Reference <reference>`: every verb, the MCP tools, the hooks and the variables.
- :doc:`Explanation <explanation>`: why the stores are split the way they are.
- :doc:`How ljos compares <compare>`: the tables behind the comparisons above.
