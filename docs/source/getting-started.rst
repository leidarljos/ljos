:description: Everything below runs on a scratch tracker and a scratch deed store. By the end you will have remembered a lesson and been told when to review it.
:cover: https://leidarljos.github.io/ljos/_static/og.png

Everything below runs on a scratch tracker and a scratch deed store. By the
end you will have remembered a lesson and been told when to review it. You
will have settled a vote with learned trust, and handed the work to a
second seat that checked it.

0. Point the seat at scratch stores
===================================

Install the binaries once:

.. code:: console

   $ cargo binstall --locked ljos packset deedar-cli claimdag-cli vissue-cli vissue-mcp ljos-policyd ljos-consensus

They provide ``ljos``, ``ljos-mcp``, ``ljos-policyd``, ``ljos-consensus``, ``packset``,
``packsetd``, ``deedar``, ``claimdag`` and ``vissue``. ``VISSUE_ROOT`` must name an existing
directory with a prefix directory inside it, and ``packset ensure`` starts the
pack writer. ``mktemp -d`` gives a directory only you own: a fixed ``/tmp`` path can be
another user's or left from an earlier run. The tracker commits each change,
so git needs an identity (``git config --global user.email``).

.. code:: console

   $ seat=$(mktemp -d) && mkdir -p "$seat/tracker/Software" "$seat/deeds"
   $ git -C "$seat/tracker" init -q && cd "$seat"
   $ export VISSUE_ROOT="$seat/tracker" DEEDAR_URL="file://$seat/deeds"
   $ export CLAIMDAG_DIR="$seat/claims"
   $ packsetd --port 18761 --home "$seat/pack" &
   $ export PACKSET_URL=http://127.0.0.1:18761
   $ ljos doctor --all
   ok  vissue  ...
   ok  seat    alice, holding as alice (from the login user)
   ok  pack    http://127.0.0.1:18761 workspace default
   ok  deed store  size=0 root=...

1. The memory loop
==================

.. code:: console

   $ ljos remember "The lexical default is BM25+. It beat BM25 by two points on turns."
   $ ljos prefer "CombMNZ over RRF for fusing two ballots."
   $ ljos search "which fusion"
   2.0000  1/3 lesson  1fe4b916d956798196df680ade594ac5    today   The lexical default is BM25+. It beat BM25 by two points on turns.
   1.0000  1/3 preference  85dde12b553372dafb3d50e3b4b63fec    today   CombMNZ over RRF for fusing two ballots.

A claim is two sentences at most, stored as written. At ``SessionEnd`` the
hook keeps up to five cue-word sentences from the person's and the agent's
prose as unverified agent notes. The prompt hook never injects them.
``LJOS_EXTRACT=off`` turns that off. A lesson you type takes effect at once, like the preference, so the
hook shows both from the next prompt they bear on. Tomorrow, ``ljos due`` lists both claims; read each and grade it:

.. code:: console

   $ ljos graded 3f9c...            # recalled: comes back later
   $ ljos graded 3f9c... --lapsed   # had to look it up: comes back sooner

A claim you stop reviewing sinks in search as its retrievability falls, and
a claim shown wrong is retired with the deed that showed it:
``ljos forget 3f9c... --why deed-...``.

2. The agreement loop
=====================

Three identities vote on one issue.

.. code:: console

   $ id=$(vissue create -p demo "Ship the fuse change?" -q)
   $ VISSUE_AGENT=alice ljos vote $id --for ship --used none
   $ VISSUE_AGENT=bob   ljos vote $id --for ship --used none
   $ VISSUE_AGENT=carol ljos vote $id --for hold --used none
   $ ljos consensus $id
   ... ship 0.667, hold 0.333 ...

The first line says what a push cite on the issue decides from seat
ballots. Then the consensus crate's DeGroot or Friedkin-Johnsen settle
prints under the trust rows, as advice; ``--compare`` adds the tracker's own
settle. With no
rows every voter weighs the same. Now suppose ``hold`` turned out right:

.. code:: console

   $ ljos learn $id --outcome hold
   Learned. 6 trust rows rewritten. ...
   alice weighs bob at 0.010
   alice weighs carol at 0.010
   ...
   $ ljos consensus $id
   ... ship 0.667, hold 0.333 ...

By default ``learn`` uses the ``record`` rule, which weighs each voter by its hits and
misses across outcomes. One outcome cannot tell a lucky voter from a good
one. Every row sits at the floor, and the settle stays the equal-weight
count. The rows separate once more issues have outcomes. With
``--rule hedge``, only the voters the outcome refuted shrink, by ``--beta``
(0.5 unset):

.. code:: console

   $ ljos learn $id --outcome hold --rule hedge
   alice weighs bob at 0.500
   alice weighs carol at 1.000
   ...
   $ ljos consensus $id
   settle (DeGroot, 3 voters): hold 0.500, ship 0.500

That is the same three ballots on a fresh pack, so the rows start at 1.
The rows are ``trust`` atoms in the pack, so they carry a validity window, can be
superseded, and travel in a handover. A person can also set one by hand:
``ljos trust alice carol 0.9 --why deed-...``.

3. The work loop
================

.. code:: console

   $ ljos sitting $id --assignee alice
   == doctor
   ...
   == due
   2 due; nothing else scheduled
   == island: Ship the fuse change?
   1.000   seed    ... CombMNZ over RRF for fusing two ballots.
   == blockers
   none open; the issue is workable
   == playbook
   ...
   == recall
   ...
   == timeline
   ...
   == reclaim
   no claim quiet longer than 900s
   == claim
   gen=2
   $ echo 'fn main() {}' > patch.rs
   $ deedar create file --name "the fuse patch" --path patch.rs --agent alice
   id=deed-file-the-fuse-patch ...
   $ ljos deed $id --add deed-file-the-fuse-patch
   $ ljos finish $id --assignee alice --lesson "The fuse patch shipped as one file. Nothing else moved." --outcome hold
   completed the session node for demo-... as done
   remembered your lesson (id 1735...); the hook shows it from the next prompt
   fired the island for "Ship the fuse change?": 3 memories. Those links gained weight under the seat, not under a persona. The next walk of this title follows them.
   Learned. 6 trust rows rewritten. ... persona anchors moved. The outcome is kept: once 5 issues have one, ljos consensus discounts voters who err together. ... No stated probability, so there is no Brier score. A hard vote is not a claim of certainty.
   the ticket demo-... stays open and nobody holds it; a person closes it when the work is accepted
   tracker git: committed chore(issues): demo-... finished by alice; no remote, kept local

``sitting`` opens in order: doctor, cards, what is due, the island the title touches, blockers, the playbook, recall, the timeline, reclaim, then the claim.
``deed`` cites what the work produced. ``finish``
writes the lesson at once when a person types it at a terminal, fires the island, completes the node,
and the outcome reweighs the voters as in step 2. Completing the session node does not close the ticket. A person closes it when the work is accepted. To stop without finishing,
``ljos release $id --assignee alice`` hands the node back; until then a second
``claim`` under the same name is refused, and the refusal names this issue.

4. The handover loop
====================

.. code:: console

   $ ljos handover --out /tmp/bag --issue $id
   issues=1 deeds=1 files=8
   /tmp/bag/manifest-sha256.txt.sig

On the receiving seat, with its own stores:

.. code:: console

   $ ljos receive /tmp/bag
   the payload matches the manifest ...
   1 deeds proven against a log of 1 entries ...
   0 atoms enclosed, 0 trust rows
   $ ljos receive /tmp/bag --import
   Error: receive: unsigned satchel; will not import

A scratch seat has no host key, so its bag goes out unsigned, and
``--import`` refuses an unsigned bag. ``ljos onboard --harness NAME`` writes the key.

The receiver now knows what the sender learned, who the sender trusts, and
which deeds the work produced, and checked all of it before importing. The
handover is signed when ``~/.config/deedar/host.key`` exists, a 32-byte seed;
the last line is that signature beside the manifest. The receiver accepts
the key in its store's ``layout``. Two seats on one ticket do this in
sequence, each named by ``LJOS_SEAT`` and a distinct ``*_SESSION_ID``: A sits,
cites a deed, hands over; B imports; A releases; B sits, both vote,
consensus, and finish closes. ``scripts/smoke.sh`` walks that handoff on
scratch stores. The herd is the other check: eight sittings at once on
one contended ticket.

5. Hand the protocol to an agent
================================

.. code:: console

   $ ljos protocol | head -3
   # The seat protocol

   One seat, five stores, five questions. Ask the store that owns the question.
   $ ljos onboard
   {
     "mcpServers": {
       "ljos": {
         "type": "stdio",
         "command": "ljos-mcp",
         "args": [],
         "env": {}
       }
     }
   }

That entry goes into any runner, where it keeps its
Model Context Protocol (MCP) servers. The
``env`` is empty on purpose: the server names the seat after the client
that connects, and ``ljos`` in a shell the runner opens finds the same name
through the process tree, so nothing is set per runner. A shell-only
agent has no such record. ``ljos onboard --harness grokbot`` writes an env
file; source it and ``LJOS_SEAT`` is the seat. The agent's next
sitting then runs in the order above. A runner that registers servers by
a command of its own can be described once in
``~/.config/ljos/harnesses.toml`` and onboarded with ``ljos onboard --harness
NAME``; the :doc:`how-to <howto>` shows the file.

Clean up
========

The scratch seat is one directory and the pack writer started in step 0.
Stop the writer and remove the directory, and the next run starts clean on
the same port:

.. code:: console

   $ kill %1 && rm -rf "$seat"

Where next
==========

-  :doc:`How-to <howto>`: wire the server into an agent runner, sign handovers, read the cards.
-  :doc:`Reference <reference>`: every verb and tool.
-  :doc:`Explanation <explanation>`: the contracts, and the memory model with its sources.
