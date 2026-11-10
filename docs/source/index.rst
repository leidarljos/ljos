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
7,901 tokens for the 20 core Model Context Protocol (MCP) tools, against 17,855 for all 52
196.3 ms     warm prompt hook median on a pack of 10,000 lessons, down from 602.5 ms
0 of 186     near-topic lesson pairs merged by the duplicate check
0.949        recall@5 in packset's LongMemEval\ :sub:`S` session retrieval run, BM25+ fused with e5-large-v2, 470 questions; retrieval, not answer accuracy
2,293        tests passed on the 0.40.0 release commit, 0 failed
10 runners   with an onboarding shape: Claude Code, Codex, Cursor, Grok, OpenCode, Windsurf, Antigravity, Hermes, omp, shell
MIT          licence; Rust; Linux and macOS
============ ==============================================================================================================================================

:doc:`What ljos does <introduction>` lists each feature and the file
that measured each number. :doc:`How ljos compares <compare>` sets it
beside beads, Mem0, Letta, Zep, Basic Memory and git-bug.

Install
=======

.. code:: console

   $ curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh
   $ export PATH="$HOME/.local/bin:$PATH"
   $ ljos doctor
   ljos is ready: memory and command guard work. Optional extras: `ljos doctor --all`.

The installer checks each download against its published SHA-256 sum.
It then runs ``ljos onboard`` for each coding runner it finds. When ``ljos doctor``
prints ``ljos is ready``, you are done. A row it prints instead names the
part that needs action. To install by hand instead:

.. code:: console

   $ cargo binstall --locked ljos packset deedar-cli claimdag-cli vissue-cli vissue-mcp ljos-policyd ljos-consensus
   $ ljos onboard
   $ ljos doctor

:doc:`Getting started <getting-started>` says what each program is.

First minute
============

.. code:: console

   $ ljos remember "The lexical default is BM25+. It beat BM25 by two points on turns."
   remembered (lesson): The lexical default is BM25+. It beat BM25 by two points on turns.
     id 07be8829, next review 2026-09-26 23:27 +00:00
   $ ljos search lexical default
   1.0000  3/3 lesson  07be8829... today   The lexical default is BM25+. It beat BM25 by two points on turns.
   $ ljos due
   0 due; 1 scheduled, next at 2026-09-26T23:27:31.528Z

Where to go next
================

- :doc:`Getting started <getting-started>`: a ten-minute tutorial with memory, two seats on one ticket, a vote and offboard.
- :doc:`How-to guides <howto>`: one task a section, from hooks and onboarding to handover, trust and sync.
- :doc:`Integration <integration>` and :doc:`Grok Build <grok-build>`: the runners ljos sits in.
- :doc:`Explanation <explanation>` and :doc:`Architecture <architecture>`: why the design is what it is.
- :doc:`How ljos compares <compare>`: the other memory tools and agent trackers.
- :doc:`Reference <reference>`: every verb, server tool and variable.

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
   getting-started

.. toctree::
   :maxdepth: 1
   :caption: Use
   :hidden:

   howto
   integration
   grok-build

.. toctree::
   :maxdepth: 1
   :caption: Understand
   :hidden:

   explanation
   architecture
   compare

.. toctree::
   :maxdepth: 1
   :caption: Reference
   :hidden:

   reference
   seat

.. |image1| image:: _static/seat.svg
   :width: 100.0%
