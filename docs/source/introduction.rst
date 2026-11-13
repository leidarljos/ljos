:description: ljos gives coding agents what a new session does not keep: memory a person wrote on purpose, one tracker in git that every agent shares, and a record of who decided what. It calls no model.
:cover: https://leidarljos.github.io/ljos/_static/og.png

==============
What ljos does
==============


ljos gives coding agents what a new session does not keep: memory a
person wrote on purpose, one tracker in git that every agent shares, and
a record of who decided what. It calls no model. Each store is a file a
person can open.

Features
========

- **Memory written on purpose.** ``ljos remember`` and ``ljos prefer`` store
  one claim as written. No model reads the transcript to pick facts. A
  lesson an agent writes is labelled agent-derived until a person accepts it.
- **Memory at the prompt.** The prompt hook adds the memories a prompt
  bears on before the model reads it. On a pack of 10,000 lessons the
  warm hook takes a median of 139.1 ms
  (``prompt-hook.rows-10000.after`` in ``hook-latency.txt``).
- **A review clock.** ``ljos due`` lists the claims due for review on a Free
  Spaced Repetition Scheduler (FSRS) clock. ``ljos graded`` records the
  answer, and a claim nobody reviews sinks in search.
- **One tracker for every agent.** Tickets are Org headings in a git
  repository. ``ljos sitting`` claims a ticket. A second claim on a held
  ticket is refused, and the refusal names the holder.
- **Votes that settle.** ``ljos vote`` records a ballot. ``ljos consensus``
  settles the ballots with DeGroot weights, and ``ljos learn`` moves trust
  toward the voters who turned out right.
- **Mail between seats.** ``ljos send``, ``ljos inbox`` and ``ljos reply`` carry
  messages between agents through the same pack.
- **Deeds.** ``deedar`` records what a piece of work produced in a
  content-addressed store, and a ticket cites the deed.
- **Handover a receiver can check.** ``ljos handover`` writes a BagIt bag of
  tickets, memories and deeds, signed when the sender has a key.
  ``ljos receive`` checks it before import.
- **One command, one Model Context Protocol (MCP) server.** ``ljos`` and
  ``ljos-mcp`` serve the same verbs. The server lists 20 core tools by default.
- **Many runners.** ``ljos onboard --example`` lists them, among them Claude Code, Codex and Cursor.

Measured
========

Every number below comes from a file in this repository or in packset,
the memory writer ljos drives. Each file names the machine, the method and
the commands that reproduce it.

========================================================================== ======================================================================================================================================= ==============================================================================================================
What                                                                       Result                                                                                                                                  Source
========================================================================== ======================================================================================================================================= ==============================================================================================================
MCP tool list, tokens a session pays (o200k\ :sub:`base`)                  8,057 for the 20 core tools; 18,011 for all 52                                                                                          `mcp-size.txt <https://github.com/leidarljos/ljos/blob/develop/measurements/mcp-size.txt>`__
Prompt hook, 10,000 live lessons, warm median                              196.3 ms before, 139.1 ms after, with the same output (``prompt-hook.rows-10000.after``)                                                `hook-latency.txt <https://github.com/leidarljos/ljos/blob/develop/measurements/hook-latency.txt>`__
Prompt hook, 10,000 live lessons, cold median                              958.4 ms before, 456.4 ms after (``prompt-hook.scale-10000``)                                                                           `hook-latency.txt <https://github.com/leidarljos/ljos/blob/develop/measurements/hook-latency.txt>`__
Prompt hook, 1,000 live lessons, warm median                               89.2 ms before, 36.9 ms after (``prompt-hook.scale-1000``)                                                                              `hook-latency.txt <https://github.com/leidarljos/ljos/blob/develop/measurements/hook-latency.txt>`__
Prompt hook on a seat of about 2,500 atoms                                 970 ms and 27 pack requests (0.35.1); 247 to 303 ms and 8 requests (0.35.9)                                                             `hook-latency.txt <https://github.com/leidarljos/ljos/blob/develop/measurements/hook-latency.txt>`__
``ljos doctor``, warm                                                      9.0 s (0.35.1); 0.79 to 0.82 s (0.35.7)                                                                                                 `hook-latency.txt <https://github.com/leidarljos/ljos/blob/develop/measurements/hook-latency.txt>`__
Duplicate lessons, 78 duplicate pairs and 186 near-topic pairs             0 near-topic pairs merged (precision 1.00), 12 duplicates found (recall 0.15); the pack's own rewrite test: precision 0.30, recall 0.12 `lesson-duplicates.txt <https://github.com/leidarljos/ljos/blob/develop/measurements/lesson-duplicates.txt>`__
Recall by ticket tags, 6 tickets, 48 runs a build                          labelled memories shown: median 0 before, 5 after; wall median 99.1 ms before, 99.5 ms after                                            `recall-tags.txt <https://github.com/leidarljos/ljos/blob/develop/measurements/recall-tags.txt>`__
ljos scorer, LongMemEval\ :sub:`S` session retrieval, 470 questions, BM25+ hit@5 0.959574, recall@5 0.909113, recall@10 0.949894; 71.881 s on one CPU                                                              :doc:`explanation <explanation>`
packset, LongMemEval\ :sub:`S` session retrieval, 470 questions            recorded on the packset page; this tree does not state the figures. Retrieval, not answer accuracy.                                     `packset explanation <https://leidarljos.github.io/packset/explanation.html>`__
Tests on the 0.40.0 release commit, Linux                                  2,293 passed, 0 failed, 2 ignored                                                                                                       `CI run 42801686714 <https://github.com/leidarljos/ljos/actions/runs/42801686714>`__
packset under load                                                         0 errors at 32 clients; 32 clients writing 6,400 claims into one workspace kept 6,382 live and 18 closed                                `packset README <https://github.com/leidarljos/packset#readme>`__
========================================================================== ======================================================================================================================================= ==============================================================================================================

The LongMemEval\ :sub:`S` rows score retrieval: whether the sessions that hold
the evidence come back near the top. They are not answer accuracy, and
they do not measure the same thing as a question-answering score. The
before and after columns compare two ljos builds on the same machine
and the same input. No row compares ljos with another memory tool on a
shared benchmark: ``hook-latency.txt`` says so in its ``third-party`` line.
:doc:`How ljos compares <compare>` sets the designs side by side instead.

What it does not do
===================

- It does not read a conversation to find facts nobody said to remember.
  `Mem0 <https://docs.mem0.ai/core-concepts/memory-operations/add>`__ sends
  what it ingests through a model to do that, and so does
  `Graphiti <https://github.com/getzep/graphiti>`__, the graph framework inside Zep.
- It is not a sandbox. The command check warns and the command runs.
- It does not host anything. Sync goes through a git remote you own.
