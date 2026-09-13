.. raw:: html

   <div class="vi-hero">
     <div class="vi-hero-brand">
       <img class="vi-hero-mark" src="_static/mark.svg" width="64" height="64" alt="" />
       <div>
         <p class="vi-hero-name">ljos</p>
         <p class="vi-hero-tag">One seat over four stores. It owns none of them.</p>
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

A seat is one agent, or one person, working a tracker. Four stores answer
four questions for it: the tracker knows what the work is and who agrees,
the deed store knows what the work produced, the pack knows what the seat
has learned, and the claim graph knows what this session is handing out.
``ljos`` is the one command and the one Model Context Protocol (MCP) server
over all four. It adds
three loops the stores do not have alone: memory that is reviewed and
forgotten on a clock, agreement that weighs voters by who turned out right,
and a handover another seat can check and import.

Everything is a file a person can open: Org headings, JSON lines, a
content-addressed store, a Cap'n Proto snapshot. No model is required to
run any of it, and any model or agent runner that can call a command or an
MCP tool can sit in the seat.

|image1|

Install
=======

.. code:: console

   $ cargo install --git https://github.com/leidarljos/ljos ljos ljos-mcp
   $ cargo install --git https://github.com/leidarljos/consensus
   $ cargo install --git https://github.com/leidarljos/vissue vissue-cli
   $ cargo install --git https://github.com/leidarljos/deedar deedar-cli
   $ cargo install --git https://github.com/leidarljos/packset packset-cli packset-daemon
   $ cargo install --git https://github.com/leidarljos/claimdag claimdag-cli
   $ packset ensure
   $ ljos doctor
   $ ljos onboard --harness json

``doctor`` names each habitat and whether it answers. The seat works with the
habitats it has; a missing one is reported, not guessed around. ``onboard``
registers the server with an agent runner and installs the sitting protocol
as its skill; ``ljos protocol`` prints that protocol for any other.

First minute
============

.. code:: console

   $ ljos remember "The lexical default is BM25+. It beat BM25 by two points on turns."
   $ ljos search lexical default
   $ ljos due

The :doc:`tutorial <getting-started>` runs all three loops on a scratch
tracker in about ten minutes.

.. toctree::
   :maxdepth: 1
   :caption: Guides
   :hidden:

   getting-started
   howto
   grok-build
   reference
   explanation

.. |image1| image:: _static/seat.svg
   :width: 100.0%
