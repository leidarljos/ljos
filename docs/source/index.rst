:description: ljos is memory, a shared tracker and a vote for coding agents. A person writes what the agents should remember. Every agent files and claims work on one git repository of tickets.
:cover: https://leidarljos.github.io/ljos/_static/og.png

.. raw:: html

   <div class="vi-hero">
     <div class="vi-hero-brand">
       <img class="vi-hero-mark" src="_static/mark.svg" width="64" height="64" alt="" />
       <div>
         <p class="vi-hero-name">ljos</p>
         <p class="vi-hero-tag">What does this seat do next?</p>
       </div>
     </div>
     <p class="vi-hero-tagline">Remember, agree, hand over: memory for a working seat that a person can read on disk.</p>
     <div class="vi-hero-pills">
       <span>Model agnostic</span>
       <span>Harness agnostic</span>
       <span>CLI + MCP</span>
     </div>
     <div class="vi-hero-actions">
       <a class="vi-btn vi-btn-gold" href="getting-started.html">Get started</a>
       <a class="vi-btn vi-btn-ghost" href="reference.html">Reference</a>
     </div>
   </div>

ljos is memory, a shared tracker and a vote for coding agents. A
person writes what the agents should remember. Every agent files and
claims work on one git repository of tickets. When agents disagree, they
vote, and ljos settles the vote. It calls no model, and every store is a
file a person can open.

|image1|

What it has done
================

============ ==============================================================================================================================================
10,496 tokens for the 22 core Model Context Protocol (MCP) tools, against 24,553 for all 57
139.1 ms     warm prompt hook median on a pack of 10,000 lessons, down from 196.3 ms; ``prompt-hook.rows-10000.after`` in hook-latency.txt
0 of 186     near-topic pairs merged; 12 of 78 duplicate pairs found
2,293        tests passed on the 0.40.0 release commit, 0 failed
MIT          licence; Rust; Linux and macOS
============ ==============================================================================================================================================

:doc:`What ljos does <introduction>` lists each feature and the file
that measured each number.

Mem0 and Zep run a model over what they ingest. ljos stores the sentence as written.
beads keeps a ticket queue in Dolt and an agent can claim one. ljos keeps Org headings in git, and one seat holds a ticket.
:doc:`How ljos compares <compare>` sets ljos beside beads, Mem0, Letta, Zep, Basic Memory and git-bug.

Install
=======

.. code:: console

   $ curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh --no-embed
   $ export PATH="$HOME/.local/bin:$PATH"
   $ cd "$HOME"
   $ ljos doctor
   warn	seat source	no seat is set; reads use every seat, so run `export LJOS_SEAT=NAME`.
   info	encoder	not installed. Search matches words, so run `cargo install --locked packset-embed`.
   info	sitting gate	not active here: no agent runner is above this shell, so the person's git is not gated; under one, a commit, merge, rebase, cherry-pick or push waits for a live sitting
   ok	config	no ljos.toml key and no LJOS_ variable is set; defaults stand
   ljos needs action: 1 row needs action. Optional extras: `ljos doctor --all`.

With no seat named, the ``seat source`` row warns that the read uses every seat. When the encoder is not installed, an ``encoder`` row follows and names the install command this machine can run. That is ``cargo binstall --locked packset-embed`` when cargo-binstall is installed, ``cargo install --locked packset-embed`` when cargo is, and the release install line otherwise. The install command above skips the encoder, so the sample shows the cargo line. Leave out ``--no-embed`` and the encoder is installed, and that row is not printed. The ``sitting-gate`` row is ``info`` and says ``not active here`` when no agent runner is above the shell. The config row says the defaults apply when nothing is set. A missing ``ljos.toml`` is not a warning. Run ``ljos doctor`` from your home directory. In a checkout, the config row names ``.ljos/gates.toml`` instead.

The release is a complete install of ljos, the sibling tools and the encoder. The installer downloads the encoder model and onboards each coding runner it finds.
The copy on the 0.41.1 release asked for a vissue archive that project does not publish. The 0.41.2 script fetches the archive the vissue release publishes.
The flag ``--no-embed`` skips the encoder and its model download. The flag ``--no-onboard`` leaves runner registration for later.
The installer checks each download against its published SHA-256 sum.
It then runs ``ljos onboard`` for each coding runner it finds. When ``ljos doctor``
prints ``ljos is ready``, you are done. A row it prints instead names the
part that needs action. Git needs ``user.name`` and ``user.email`` before the first ticket. To install by hand instead:

.. code:: console

   $ cargo binstall --locked ljos packset deedar-cli claimdag-cli vissue-cli vissue-mcp ljos-policyd ljos-consensus
   $ ljos onboard
   $ ljos doctor

:doc:`Getting started <getting-started>` says what each program is.

First commands
==============

.. BEGIN ljos-sample tutorial-index

.. code:: console

   $ ljos remember "The lexical default is BM25+. It beat BM25 by two points on turns."
   remembered (lesson): The lexical default is BM25+. It beat BM25 by two points on turns.
     id a6ae2751, next review 2026-11-12 21:40 +00:00
   saved as a standing lesson; run `ljos search "QUERY"` to read it
   $ ljos search "lexical default"
   matched by words (encoder not installed), so read https://leidarljos.github.io/ljos/getting-started.html#install or run `ljos doctor`.
   shown 1 of 1.
   The first row is the closest match.

   lesson, today
     The lexical default is BM25+. It beat BM25 by two points on turns.
     place 1, 2 of 2 checks, id a6ae2751
   $ ljos file "Pick a codec" -p demo

.. END ljos-sample tutorial-index

``ljos search`` prints the lesson when the query shares its words. ``ljos show a6ae2751`` opens that id. A ``local:`` id opens the same way.
Typed at a terminal, the row is ``lesson, today``. With no terminal, a script or cron as well as a runner, the row is ``lesson, from an agent``.
A word ranking splits an identifier, so ``list_scans`` matches ``list scans``.
A word in every row is common only once at least four rows carry it.
Two lessons that share one word of the query still match.
A row that only shares a common word is left out, and the line is then
``No close matches for "..."``.
``ljos file`` prints the new id and commits the Org heading. :doc:`Getting started <getting-started>` has the full transcript, including a second seat.

Where to go next
================

- :doc:`Getting started <getting-started>`: the tutorial, from install to a ticket and a second seat.
- :doc:`How-to guides <howto>`: one task a page, from hooks and onboarding to handover, trust and sync.
- :doc:`Reference <reference>`: every verb, server tool and variable.
- :doc:`Explanation <explanation>` and :doc:`Architecture <architecture>`: why the design is what it is.
- :doc:`How ljos compares <compare>`: the other memory tools and agent trackers.
- :doc:`Integration <integration>` and :doc:`Grok Build <grok-build>`: the runners ljos sits in.

Community
=========

- Questions and bugs: `GitHub issues <https://github.com/leidarljos/ljos/issues>`__.
- Changes: `CONTRIBUTING.md <https://github.com/leidarljos/ljos/blob/develop/CONTRIBUTING.md>`__.
- Releases: `GitHub releases <https://github.com/leidarljos/ljos/releases>`__ and `CHANGELOG.md <https://github.com/leidarljos/ljos/blob/develop/CHANGELOG.md>`__.
- The other leiðarljós programs: `github.com/leidarljos <https://github.com/leidarljos>`__.

.. toctree::
   :maxdepth: 1
   :caption: Start
   :hidden:

   introduction
   compare

.. toctree::
   :maxdepth: 1
   :caption: Tutorial
   :hidden:

   getting-started

.. toctree::
   :maxdepth: 1
   :caption: How-to
   :hidden:

   howto
   integration
   grok-build

.. toctree::
   :maxdepth: 1
   :caption: Reference
   :hidden:

   reference
   seat

.. toctree::
   :maxdepth: 1
   :caption: Explanation
   :hidden:

   explanation
   architecture

.. |image1| image:: _static/seat.svg
   :width: 100.0%
