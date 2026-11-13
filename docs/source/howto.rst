:description: Task pages for ljos: connect a runner, check the seat, bring memory into the prompt, sit an issue, vote, hand over and read the command check.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=============
How-to guides
=============

.. raw:: html

   <span id="how-to"></span>

Each page is one task. Start with the first two; the rest stand alone.

- :doc:`Connect an agent runner <howto-runner>`: wire ljos into a runner, an editor or a shell-only agent. The page also puts the seat in the status bar.
- :doc:`Run two agents on one repository <howto-agents>`: each session gets a seat, they mail each other, and they split the tickets.
- :doc:`Check the seat <howto-check>`: read what ljos doctor prints, which rows fail, and when it exits 1.
- :doc:`Troubleshoot doctor <howto-troubleshoot>`: the row ljos doctor prints for a missing writer, seat, tracker, binary, Model Context Protocol entry or registry, and the command that fixes it.
- :doc:`Add a seat and let it close tickets <howto-seats>`: add a seat to the tracker and grant it the right to close named tickets.
- :doc:`Bring memory into the prompt <howto-recall>`: turn on the prompt hook, ask what a task touches, and read an issue's history as one dated list.
- :doc:`Keep memory current <howto-memory>`: keep the cards, record a number that is measured again and again, and retire a memory with its reason.
- :doc:`Move memory across lanes <howto-mesh>`: pull lane exports into a seat and push that seat's memories to the shared handoff.
- :doc:`Sit an issue <howto-sitting>`: open a sitting, end it with a lesson, and hold a done claim that sits beside a failing test.
- :doc:`Send mail between seats <howto-mail>`: write to another seat, read the inbox, and wake an idle seat when mail reaches it.
- :doc:`Hand work to another seat <howto-handover>`: sign a handover bundle and receive from the same sender again.
- :doc:`Vote and settle a decision <howto-vote>`: cast ballots as seats and personas, settle them, and set or move the trust rows.
- :doc:`Run personas in panes and subagents <howto-personas>`: open persona panes in another tool and spawn a persona as a subagent.
- :doc:`Put the judgments to a model <howto-judge>`: turn on Jev, or point the same judgments at a local model or a runner, and cast persona ballots through it.
- :doc:`Read the command check <howto-command-check>`: what the command check warns on, the rewrite, block-on-warning, and which branches the push check covers.
- :doc:`Bump a recipe with eb-stack <howto-eb-stack>`: bump an EasyBuild recipe with eb-stack in a sitting, plan a whole generation, and keep what the campaign taught.
- :doc:`Use ljos with graphify <howto-graphify>`: install graphify, run /graphify, and read the code context ljos recall adds.

.. toctree::
   :maxdepth: 1
   :hidden:

   howto-runner
   howto-agents
   howto-check
   howto-troubleshoot
   howto-seats
   howto-recall
   howto-memory
   howto-mesh
   howto-sitting
   howto-mail
   howto-handover
   howto-vote
   howto-personas
   howto-judge
   howto-command-check
   howto-eb-stack
   howto-graphify
