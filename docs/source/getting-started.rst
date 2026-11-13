.. [[[cog import ljos_version as v; v.lines("", ":description: A plan is how a seat opens a day's work: one parent, an ordered checklist, and the next unfinished item. This page installs ljos, stores a lesson, opens that plan, and puts a second seat on a ticket. It describes ljos {version}.", ":cover: https://leidarljos.github.io/ljos/_static/og.png", "", "A plan is how a seat opens a day's work: one parent, an ordered checklist, and the next unfinished item. This page installs ljos, stores a lesson, opens that plan, and puts a second seat on a ticket. It describes ljos {version}.", "")]]]

:description: A plan is how a seat opens a day's work: one parent, an ordered checklist, and the next unfinished item. This page installs ljos, stores a lesson, opens that plan, and puts a second seat on a ticket. It describes ljos 0.41.11.
:cover: https://leidarljos.github.io/ljos/_static/og.png

A plan is how a seat opens a day's work: one parent, an ordered checklist, and the next unfinished item. This page installs ljos, stores a lesson, opens that plan, and puts a second seat on a ticket. It describes ljos 0.41.11.

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

``ljos`` with no arguments prints what ljos is, the seat it would use and where that came from, then ``search``, ``remember``, ``file`` and ``doctor``, and where the command reference is. ``ljos help`` lists the rest.

Shell completion is one line in the shell's startup file. ``install.sh`` prints that line and does not write the file.

.. code:: console

   $ source <(ljos completions bash)

The same line with ``zsh``, or ``ljos completions fish | source`` for fish, is what the script prints for those shells.

The ``install.sh`` attached to the 0.41.1 release asked for ``vissue-cli-<target>.tar.xz``. The 0.41.2 script fetches ``vissue-v<version>-<target>.tar.gz``.
The vissue release publishes ``vissue-v<version>-<target>.tar.gz``.

When the install stops
----------------------

The script names the cause and the command that fixes it.

- No network, or the release download fails. Check the connection and run ``sh install.sh`` again. A version you name is ``sh install.sh --version VER``. Archives already on disk are ``sh install.sh --asset-dir DIR``.
- The OS or arch is not Linux or macOS on x86_64 or aarch64. On Windows, run the script inside WSL. To build from source, install Rust from https://rustup.rs and run ``sh install.sh`` again.
- ``curl`` or ``tar`` is missing. Install it and run ``sh install.sh`` again. Programs already unpacked are ``sh install.sh --from-dir DIR``.
- The install directory is not on ``PATH``. The script prints ``export PATH="$HOME/.local/bin:$PATH"``. Run that, then ``ljos doctor``.
- An older ``ljos`` is earlier on ``PATH``. The script names both binaries and prints the ``export PATH`` line that puts the new one first.
- Running the script again replaces the programs in place and says what was there.
- A running ``ljos-mcp`` keeps the previous binary. Restart the assistant, or ``kill`` the pid the script prints, then run ``ljos doctor``.

``ljos onboard`` with no coding tool installed says so. It still sets up the tracker, the memory writer and the deed store. Install an agent, then run ``ljos onboard``. ``ljos onboard --example`` lists the names.

``ljos offboard`` after a hand edit that it cannot undo names the file and leaves the ljos entry. Edit that file, delete the ljos entry, and run ``ljos offboard`` again.

``ljos uninstall`` while an assistant still points at ljos names that file. Edit it and delete the ljos entry. The programs are still removed. A stub named ``ljos`` answers a hook that a running session cached. Later, upgrade with ``ljos upgrade``. It names a download that failed, a machine with no release archive, and a ``ljos-mcp`` that is still running the previous binary.

The programs go to ``~/.local/bin``. They are ``ljos``, ``ljos-mcp``, ``packset``,
``packsetd``, ``packset-mcp``, ``vissue``, ``vissue-mcp``, ``deedar``, ``claimdag``, ``ljos-policyd`` and ``ljos-consensus``.
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
   warn	seat source	no seat rule matched; reading every seat, so run `export LJOS_SEAT=NAME`.
   info	sitting gate	not active here: no agent runner is above this shell, so the person's git is not gated; under one, a commit, merge, rebase, cherry-pick or push waits for a live sitting
   ok	config	no ljos.toml key and no LJOS_ variable is set; defaults stand
   warn	ljos config	/home/you/.config/ljos/ljos.toml is missing, so run ljos doctor --fix

On a clean install the ready line names memory, the tracker and the command guard.
With no seat named, a ``seat source`` row warns that the read uses every seat.
The ``sitting-gate`` row follows it. With no agent runner above the shell the word is ``info``, and the person's commits, merges and pushes are not held.
Under a runner the same row names the seat and the branches a push waits on, ``develop`` or ``main``, and the number of seats that push waits for.
A fresh tracker has no seat list, so that number is 2. When ``vissue.toml`` lists one active seat, that seat's own vote settles the push.
A ``warn`` or ``no`` row under the ready line names anything that still needs action. The config row is always printed. With nothing set it says the defaults stand. The ``ljos config`` row is the empty config file, which a new install does not have yet. ``ljos doctor --fix`` creates it, and the defaults stand. The next command is still ``ljos remember``. Add ``--all`` to see every row.

Two sessions of one runner do not share a seat. With ``LJOS_SEAT`` unset, Claude Code, Codex and Cursor each take a name from the session id in this process, such as ``claude-k3j9x0a1b2``. An id that exists only on a parent does not count. Mail, claims and notes use that name, so one session can write to the other. Search and recall still read the whole runner: a lesson stored by one session is there for the next. Set ``LJOS_SEAT`` to a name of your own when the sessions should be one seat. ``ljos seat`` prints the name and where it came from. The how-to :doc:`Run two agents on one repository <howto-agents>` walks that case.

To install by hand, with ``cargo-binstall`` already on ``PATH``, or to see what onboarding would write:

.. code:: console

   $ cargo binstall --locked ljos packset deedar-cli claimdag-cli vissue-cli vissue-mcp ljos-policyd ljos-consensus
   $ ljos onboard --dry-run
   $ ljos onboard
   $ ljos doctor

``ljos onboard`` writes the MCP entry, the hooks and the skill into each runner it finds.
``ljos onboard --example`` lists the runners ljos knows.

Nothing else needs a variable set. ``ljos onboard`` makes a deed store, a tracker and a
config file for ``vissue``, and it starts the pack writer.

The runner runs a hook only after you trust it. Trust each ljos hook in the runner.
Trust it again after an upgrade rewrites one.

A lesson and a search
=====================

.. code:: console

   $ ljos remember "The lexical default is BM25+. It beat BM25 by two points on turns."
   ljos: no seat was named and this write uses unknown-cursor, so run `export LJOS_SEAT=NAME`.
   remembered (lesson): The lexical default is BM25+. It beat BM25 by two points on turns.
     id a6ae2751, next review 2026-11-12 21:40 +00:00
   standing: the hook shows it from the next prompt it bears on
   $ ljos search "lexical default"
   ranking is lexical: encoder unavailable
   shown 1 of 1.
   score: scorer order, not whether the claim is true.
   scorers: how many of them named the hit.

   lesson, agent-derived, today
     The lexical default is BM25+. It beat BM25 by two points on turns.
     score 1.00, scorers 2/2, id a6ae2751

The id and the review time are the ones that run printed. The name after ``unknown-`` is the host name.
That line is printed only when the write uses that seat. A conversation that already has a seat record writes as that seat and does not print the line.
This run sat under an agent runner, so the search row says ``lesson, agent-derived``.
Typed at a terminal with no runner above it, the same command is the person's sentence.
Search matches words when the encoder is absent. ``ljos search fusing`` finds a preference that contains that word.
``ljos search "which fusion"`` shares no word with either sentence and prints ``No hits for "which fusion", so run `ljos search fusion`.``
This seat's hits come first. Another seat's lesson is printed when it names the subject, or when the encoder's cosine with the query is at least 0.66.
That floor is the one the prompt hook uses for a hit the encoder lists at the top.
A weaker cross-seat hit does not fill a short list. When those were the only rows, the line is ``Nothing relevant was found.``
A busy pack ranks by words and still returns a hit that names the subject.

.. code:: console

   $ ljos prefer "CombMNZ over RRF for fusing two ballots."
   remembered (preference): CombMNZ over RRF for fusing two ballots.
     id f8b725a9, next review 2026-11-12 21:40 +00:00
   $ ljos search fusing
   ranking is lexical: encoder unavailable
   shown 1 of 1.
   score: scorer order, not whether the claim is true.
   scorers: how many of them named the hit.

   unverified agent preference (origin agent-derived), today
     CombMNZ over RRF for fusing two ballots.
     score 1.00, scorers 2/2, id f8b725a9

``ljos due`` lists claims when their review time arrives. A claim you stop reviewing sinks in search.
The how-to :doc:`Keep memory current <howto-memory>` is the review clock, ``ljos graded`` and ``ljos forget``.

A ticket
========

.. code:: console

   $ LJOS_SEAT=alice ljos file "Pick a codec" -p demo -t decision --options keep,drop
   demo-2f5j
   project: demo
   demo-2f5j  TODO  [#C]  Pick a codec
   file: /home/you/.local/share/vissue/tracker/Software/demo/issues.org
   tracker git: committed chore(issues): demo-2f5j filed by alice; no remote, kept local

The id is the only line on standard output, so ``id=$(LJOS_SEAT=alice ljos file ...)`` captures it.
The heading, the path and the commit line are on standard error. The id in the heading is the same one.
The ``file:`` line is the absolute path. ``/home/you`` above names the home directory.
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
   tracker: day-ab12 STARTED under alice
   tracker git: committed chore(issues): day-ab12 claimed by alice; no remote, kept local
   ljos is ready: memory, tracker and command guard work.
   memory degraded: ranked by words
   next: day-cd34  measure the files
   day-ab12 is yours. `ljos finish day-ab12 --lesson "..."` closes the sitting; `ljos sitting day-ab12 --verbose` prints every section.
   $ LJOS_SEAT=alice ljos finish day-cd34
   ticked day-cd34
   next: day-ef56  pick a default
   tracker git: committed chore(issues): day-cd34 ticked by alice; no remote, kept local
   $ LJOS_SEAT=alice ljos ready --mine
   next: day-ef56  pick a default

The ids are the ones that run printed. The parent is claimed. Each item stays open until ``ljos finish`` ticks it. A sitting with no ``--verbose`` prints what needs action. With ``--no-embed`` the pack has no encoder, so the sitting prints ``memory degraded: ranked by words``. Quiet claims are a count that points at ``ljos claims --mine``. ``ljos sitting ISSUE --verbose`` prints every section.
Under a runner the holder on the ``STARTED`` line can be ``alice`` plus ``@`` and a session tag. The claim command below prints the seat name.

A second seat
=============

One seat holds a ticket at a time. Bob's claim is refused, and the refusal names alice:

.. code:: console

   $ LJOS_SEAT=alice ljos claim demo-2f5j --assignee alice
   gen=2
   tracker: demo-2f5j STARTED under alice
   tracker git: committed chore(issues): demo-2f5j claimed by alice; no remote, kept local
   $ LJOS_SEAT=bob ljos claim demo-2f5j --assignee bob
   label claim-conflict: claim: demo-2f5j is held by another (alice, seat alice, still running, ...), not by bob (this one).

The rest of bob's line names ``ljos release``, ``ljos complete``, ``ljos note`` and ``ljos send``. The ticket stays with alice.
Bob can file another ticket. To help on this one, bob writes a note.

.. code:: console

   $ LJOS_SEAT=alice ljos note demo-2f5j "the codec is one file"
   demo-2f5j: noted
   tracker git: committed chore(issues): demo-2f5j noted by alice; no remote, kept local
   $ ljos timeline demo-2f5j

``ljos timeline`` prints the ticket's rows oldest first.
The note is one row: ``note: [unverified HASH]`` the text ``[end HASH]``. The hash is the fence. The text inside is tracker text from the ticket.
``vissue show demo-2f5j`` prints the heading and the log, newest first, including the note. ``ljos recall`` prints the issue as unverified tracker text. The note is on the timeline, not in that text.
``ljos digest`` reads the tracker and says what each seat filed, claimed and voted on.

Both seats can vote. From one shell the two ballots are one voice, and ``ljos consensus`` says they were folded on that shell:

..
   # proseguard:off
.. code:: console

   $ LJOS_SEAT=alice ljos vote demo-2f5j --for keep --used none
   demo-2f5j: alice voted keep
   demo-2f5j: 1 vote from 1 option
     keep                     1 (alice)
     one ballot only: keep, which nobody has agreed with yet
   recorded on the tracker: alice for keep on demo-2f5j
   ballot signed by alice with key … session alice
   unverified session: nothing named the run but seat alice, so every run of that seat is one session
   alice cast by alice, parent alice
   tracker git: committed chore(issues): demo-2f5j ballot cast by alice; no remote, kept local
   $ LJOS_SEAT=bob ljos vote demo-2f5j --for keep --used none
   demo-2f5j: bob voted keep
   demo-2f5j: 2 votes from 1 option
     keep                     2 (alice, bob)
     unanimous: keep (2 of 2); `vissue consensus demo-2f5j` weighs it
   recorded on the tracker: bob for keep on demo-2f5j
   ballot signed by bob with key … session alice
   claimed session bob, folded into alice (same shell)
   bob cast by bob, parent alice
   tracker git: committed chore(issues): demo-2f5j ballot cast by bob; no remote, kept local
   $ LJOS_SEAT=alice ljos consensus demo-2f5j --no-file
   verdict: the ballots pick keep. They do not clear a push onto `develop` or `main`.
   push cite: demo-2f5j decides keep for a push by alice; counted bob for keep
   protected push: demo-2f5j does not clear a push onto `develop` or `main`: the sitting has no settled consensus: 1 of the 2 seats a protected push needs voted on demo-2f5j; counted bob for keep
   provenance:
     alice cast by alice, parent alice
     bob cast by bob, parent alice
   shared parent alice: alice, bob
   folded into one voice (same shell alice): alice, bob
   counted bob
   advice, which does not decide a push: the weighted settle over every ballot
   settle (DeGroot, 1 voters): keep 1.000
   every voter is on keep: the settle is unanimous
   polarization 0.000, disagreement 0.000
   influence: bob 1.000

..
   # proseguard:on

The first line is the verdict: which option the ballots pick, and whether that clears a push onto ``develop`` or ``main``. ``push cite`` and ``protected push`` are the same checks in full.
From one shell the two ballots are one voice, so they fold and the protected-branch check still wants a second seat. The lines under ``advice`` are the weighted settle. They do not clear that push. ``influence`` names the weight of that one voice.
A signature line and ``tracker git:`` follow each vote. A ballot is its own voice when the cast has no shell, no agent runner and no earlier row that shares a session, a parent or the root session of one runner.
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

- :doc:`How-to guides <howto>`: one task a page. Runners, two agents on one repository, the review clock, a sitting, mail, a signed handover, votes, and the command check.
- :doc:`Run two agents on one repository <howto-agents>`: the seat each session gets, mail between them, and how they split the tickets.
- :doc:`Reference <reference>`: every verb, the MCP tools, the hooks and the variables.
- :doc:`Explanation <explanation>`: why the stores are split the way they are.
- :doc:`How ljos compares <compare>`: the tables behind the comparisons above.
