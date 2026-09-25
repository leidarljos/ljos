Everything below runs on a scratch tracker and a scratch deed store. By the
end you will have remembered a lesson and been told when to review it. You
will have settled a vote with learned trust, and handed the work to a
second seat that checked it.

0. Point the seat at scratch stores
===================================

.. code:: console

   $ mkdir -p /tmp/seat && cd /tmp/seat && git init -q
   $ export VISSUE_ROOT=/tmp/seat/tracker DEEDAR_URL=file:///tmp/seat/deeds
   $ export CLAIMDAG_DIR=/tmp/seat/claims LJOS_SEAT=alice
   $ packsetd --port 18761 --home /tmp/seat-pack &
   $ export PACKSET_URL=http://127.0.0.1:18761
   $ ljos doctor
   ok  vissue  ...
   ok  seat    alice (from LJOS_SEAT)
   ok  pack    http://127.0.0.1:18761 workspace default
   ok  deed store  size=0 root=...

1. The memory loop
==================

.. code:: console

   $ ljos remember "The lexical default is BM25+. It beat BM25 by two points on turns."
   $ ljos prefer "CombMNZ over RRF for fusing two ballots."
   $ ljos search which fusion
   9.1000  preference  3f9c... CombMNZ over RRF for fusing two ballots.

A claim is two sentences at most, stored as written. The pack never mines a
transcript. Tomorrow, ``ljos due`` lists both claims; read each and grade it:

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
   ... shares: hold 0.333, ship 0.667 ...

Two settles print: the consensus crate's DeGroot or Friedkin-Johnsen
model, then the tracker's own verb, both under the same trust rows. With no
rows every voter weighs the same. Now suppose ``hold`` turned out right:

.. code:: console

   $ ljos learn $id --outcome hold
   alice weighs bob at 0.500
   alice weighs carol at 1.000
   ...
   $ ljos consensus $id
   ... shares: hold 0.429, ship 0.571 ...

Every voter the outcome refuted shrank in every other voter's row. The rows
are ``trust`` atoms in the pack, so they carry a validity window, can be
superseded, and travel in a handover. A person can also set one by hand:
``ljos trust alice carol 0.9 --why deed-...``.

3. The work loop
================

.. code:: console

   $ ljos sitting $id --assignee alice
   == doctor
   ...
   == due
   2 due; 0 scheduled
   == island: Ship the fuse change?
   1.000   seed    ... CombMNZ over RRF for fusing two ballots.
   == claim
   gen=2
   $ echo 'fn main() {}' > patch.rs
   $ deedar create file --name "the fuse patch" --path patch.rs --agent alice
   id=deed-file-the-fuse-patch ...
   $ ljos deed $id --add deed-file-the-fuse-patch
   $ ljos finish $id --assignee alice --lesson "The fuse patch shipped as one file. Nothing else moved." --outcome hold
   remembered ...
   fired the island for "Ship the fuse change?": 3 memories
   completed the session node for demo-... as done
   learned from outcome "hold": 6 trust rows rewritten

``sitting`` opens: it runs ``doctor``, prints the cards and what is due,
activates the island the issue's title touches, recalls the working set,
and claims a session node. ``deed`` cites what the work produced. ``finish``
closes: the lesson is remembered, the island fires, the node completes,
and the outcome reweighs the voters as in step 2. Completing the session
node does not close the ticket. To stop without finishing,
``ljos release $id --assignee alice`` hands the node back; until then a second
``claim`` under the same name is refused, and the refusal names this issue.

4. The handover loop
====================

.. code:: console

   $ ljos handover --out /tmp/bag --issue $id
   issues=1 deeds=1 files=6
   2 atoms to /tmp/bag/data/atoms/default.jsonl
   exported 1 deeds, 4 files
   /tmp/bag/manifest-sha256.txt.sig

On the receiving seat, with its own stores:

.. code:: console

   $ ljos receive /tmp/bag
   the payload matches the manifest ...
   1 deeds proven against a log of 1 entries ...
   8 atoms enclosed, 6 trust rows
   $ ljos receive /tmp/bag --import
   8 atoms imported, 0 refused

The receiver now knows what the sender learned, who the sender trusts, and
which deeds the work produced, and checked all of it before importing. The
handover is signed when ``~/.config/deedar/host.key`` exists, a 32-byte seed;
the last line is that signature beside the manifest. The receiver accepts
the key in its store's ``layout``.

5. Hand the protocol to an agent
================================

.. code:: console

   $ ljos protocol | head -3
   # The seat protocol

   One seat, five stores, five questions. Ask the store that owns the question.
   $ ljos onboard --harness json
   { "mcpServers": { "ljos": { "type": "stdio", "command": "/home/you/.cargo/bin/ljos-mcp", ... } } }

That entry goes into any runner that registers MCP servers by hand. A
runner described in ``~/.config/ljos/harnesses.toml`` is onboarded in one
verb, ``ljos onboard --harness NAME``, which registers the server and installs
the protocol as the runner's skill; the :doc:`how-to <howto>` shows the
file. The agent's next sitting then runs in the order above.

Where next
==========

-  :doc:`How-to <howto>`: wire the server into an agent runner, sign handovers, read the cards.
-  :doc:`Reference <reference>`: every verb and tool.
-  :doc:`Explanation <explanation>`: the contracts, and the memory model with its sources.
