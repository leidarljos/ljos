:description: Cast ballots as seats and personas, settle them, and set or move the trust rows.
:cover: https://leidarljos.github.io/ljos/_static/og.png

==========================
Vote and settle a decision
==========================


Vote with personas
==================

.. code:: console

   $ ljos persona reviewer --anchor 0.2 --view "Reads for what breaks in production." --about release
   $ ljos persona reader --anchor 0.8 --view "Reads as a first-time user of the docs." --about docs
   $ ljos brief reviewer $id
   $ ljos brief reader $id

Each brief goes to its own subagent or run, which casts as its persona:
the reviewer's runs ``ljos vote $id --for hold --as reviewer`` and the
reader's ``ljos vote $id --for ship --as reader``. One session that cast
both would be refused at the second, since a seat argument does not
make another voice. The seat then runs ``ljos consensus $id``.

Each persona is one atom in the pack; its ballots carry its name. The
settle takes its anchor, never below the 0.5 every other voter on the
ballot, the seat too, settles at: the reviewer at 0.2 settles at 0.5 and
counts as much as a seat, and the reader at 0.8 moves most of the way
toward the others. A trust row scoped with ``--about docs`` weighs only on issues whose title says ``docs``, and ``--about`` is
also what seats a persona: ``ljos panel ISSUE`` and the ``run_a_panel``
prompt brief only the personas whose domains the issue's title or its
island names, and every persona when none does.

A persona keeps its own tree over the seat's facts. ``ljos remember --as NAME`` writes into the set ``persona-NAME``, where the duplicate and
replacement rules run among its own conclusions and never against the
seat's or another persona's; the seat still reads every set. ``ljos island CUE --as NAME`` walks the pack through that persona's link
weights, and with ``--fire`` tightens the paths it walked under its own
name (``link_weights_by``), leaving the seat's weights as they were. Two
personas that read the same island differently end up with different
islands over the same claims.

Cast a reviewer's ballot from its own run
=========================================

Each reviewer is its own subagent or run, such as a cloud runner or a CI job, and casts its own ballot rather than reporting a verdict in prose.
A subagent's ballot binds the parent session plus the subagent, ``SESSION:AGENT``: the ``agent_id`` the terminal harness's hook sees, or the command harness thread under its root session.
An opencode subagent is a session of its own.
Two subagents of one session count twice, the parent and a subagent count as two, and a subagent that wrote a candidate cannot vote.
A run on another machine needs ljos and the tracker there.
``ljos cloud up`` installs ljos on that machine and clones the shared tracker repository that ``LJOS_TRACKER_REMOTE`` names, so the ballot is pushed where the other runs fetch it.

.. code:: console

   $ ljos cloud up
   $ ljos vote $id --for ship --confidence 0.8 --used none

The run's harness id is the ballot's session, so two runs count twice and a run or subagent that votes again counts once.
When nothing names the run but the seat, ``ljos vote`` and the tally call the session unverified.
Persona ballots and Jev ballots are advisory.
The push check reads seat ballots only. See *Choose which branches the push check covers*.

Settle under the pack's trust
=============================

``ljos consensus ID`` reads the live trust rows and passes them to the
settle (and to the tracker's, with ``--compare``). To anchor voters to their own ballots (Friedkin-Johnsen) on the
model crate alone, pass the rows as JSON tuples:

.. code:: console

   $ ljos-consensus settle --issue ID --susceptibility 0.8 --trust '[["alice","carol",0.9]]'

Set trust by hand
=================

.. code:: console

   $ ljos trust alice carol 0.9 --why deed-postmortem-2026-09

One row, one weight in (0, 1], the deeds it rests on. A later row for the
same pair supersedes it.

Move the trust rows without an outcome
======================================

.. code:: console

   $ ljos calibrate -p demo
   alice weighs bob at 0.917
   alice weighs carol at 0.643
   ...

The command needs one project issue with two or more ballots. With fewer it
names that and exits 1.
Every issue of the project with two or more ballots is an item; Dawid and
Skene's estimate gives each voter an accuracy from how often it agrees with
the answer the others make likely, and each accuracy becomes the weight
every other voter gives that voter. Run it once a project has a few voted
issues, and again when it has many more.
