:description: ljos sits between agent memory and git-backed trackers for agents. The first table sets it beside memory tools that run a model: Mem0, Letta and Zep.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=================
How ljos compares
=================


ljos sits between agent memory and git-backed trackers for agents. The
first table sets it beside memory tools that run a model: Mem0, Letta and
Zep. The second sets it beside tools that keep plain files or git: Basic
Memory, beads and git-bug. Each claim about another tool links to
that tool's own documentation, read on 2026-11-10. A cell that says
"not documented" means the linked pages do not describe the feature. It
does not mean the tool cannot do it.

No shared benchmark covers both memory and coordination, so this page
compares designs, not scores. ljos's own numbers are on
:doc:`What ljos does <introduction>`.

Against model-driven memory
===========================

=================================== ====================================================== =========================================================== ================================================== =======================================================
\                                   ljos                                                   `Mem0 <https://docs.mem0.ai/open-source/overview>`__        `Letta <https://docs.letta.com/concepts/memgpt>`__ `Zep / Graphiti <https://github.com/getzep/graphiti>`__
=================================== ====================================================== =========================================================== ================================================== =======================================================
What it is                          memory, tracker and votes for agents                   memory layer for apps                                       stateful agents                                    temporal context graph
A model writes the memory           no: stored as written                                  yes by default (``infer=True``); ``infer=False`` stores raw the agent edits its own memory                     yes: ingestion runs through a model
Runs with no model or API key       yes                                                    no for the default ``add``                                  no: each agent names a model                       no: you supply a model and an embedder
Where the data lives                Org files, JSON lines and a content-addressed store    Qdrant and SQLite (library defaults)                        MemFS, tracked in git (Agent SDK)                  Neo4j, FalkorDB or Neptune
Review or forgetting                a Free Spaced Repetition Scheduler (FSRS) review clock the model updates or deletes                                "dreaming" in MemFS                                fact invalidation with a time
Record of what work produced        deeds with inclusion receipts                          not documented                                              not documented                                     provenance to source episodes
Agents vote and settle              yes: ``ljos vote``, ``ljos consensus``                 not documented                                              not documented                                     not documented
Model Context Protocol (MCP) server yes, 20 core tools                                     guides per coding agent                                     not documented                                     not documented
Licence                             MIT                                                    Apache-2.0                                                  Apache-2.0                                         Apache-2.0
=================================== ====================================================== =========================================================== ================================================== =======================================================

Against file and git tools
==========================

=================================== =================================================== =================================================================== =============================================== ================================================
\                                   ljos                                                `Basic Memory <https://github.com/basicmachines-co/basic-memory>`__ `beads <https://github.com/steveyegge/beads>`__ `git-bug <https://github.com/git-bug/git-bug>`__
=================================== =================================================== =================================================================== =============================================== ================================================
What it is                          memory, tracker and votes for agents                Markdown notes over MCP                                             issue tracker for agents                        bug tracker in git
A model writes the memory           no: stored as written                               no: people and agents write the files                               no: ``bd remember`` stores the text             not a memory tool
Where the data lives                Org files, JSON lines and a content-addressed store Markdown files                                                      Dolt, a version-controlled SQL database         git objects in the repository
Tickets with a claim                yes: one live claim a ticket                        not documented                                                      yes: ``bd update --claim``                      issues; claim not documented
Blockers between tickets            yes: ``ljos block``                                 not documented                                                      yes: ``bd dep add``                             not documented
Sync between machines               a git remote                                        optional cloud                                                      Dolt remotes                                    a git remote
Agents vote and settle              yes: ``ljos vote``, ``ljos consensus``              not documented                                                      not documented                                  not documented
Mail between agents                 yes: ``ljos send``                                  not documented                                                      yes: message issues with threads                not documented
Checked handover between machines   a BagIt bag, signed when the sender has a key       not documented                                                      not documented                                  not documented
Model Context Protocol (MCP) server yes, 20 core tools                                  yes                                                                 yes: ``beads-mcp``                              not documented
Licence                             MIT                                                 AGPL-3.0                                                            MIT                                             GPL-3.0
=================================== =================================================== =================================================================== =============================================== ================================================

Where each one is ahead
=======================

- **Mem0** extracts facts from a conversation nobody said to remember, then
  adds, updates or deletes them (`add <https://docs.mem0.ai/core-concepts/memory-operations/add>`__).
  ljos keeps only what someone wrote, so it misses those facts.
- **Zep and Graphiti** keep a temporal graph: each fact has the window it
  was true in, and a new fact invalidates an old one with its time kept
  (`concepts <https://help.getzep.com/concepts>`__). ljos keeps the same two
  times on a claim but has no entity graph.
- **Letta** gives the agent tools to edit its own memory and a git-tracked
  memory file system (`SDK page <https://docs.letta.com/concepts/memgpt>`__).
  ljos has no model in the loop, so nothing reorganises memory between turns.
- **Basic Memory** stores Markdown notes with wikilinks that people and
  agents edit together, with semantic search
  (`README <https://github.com/basicmachines-co/basic-memory>`__). ljos's
  memory is short claims, not documents.
- **beads** stores tickets in Dolt, a SQL database with cell-level merge
  and hash-based ids that avoid merge collisions
  (`README <https://github.com/steveyegge/beads>`__). ljos merges Org text
  through git.
- **git-bug** keeps issues inside the project's own git objects and bridges
  them to other trackers (`README <https://github.com/git-bug/git-bug>`__).
  ljos keeps its tracker in a separate repository and has no bridges.

What only ljos does here
========================

Of the seven tools in the table, ljos is the one whose docs describe all
three of these together:

- A ballot per agent and a settle that weighs voters by who turned out
  right (``ljos vote``, ``ljos consensus``, ``ljos learn``).
- A memory written as said, labelled by who said it, with a review clock
  and no model.
- A handover the receiving machine checks before it imports.

Pick by job
===========

===================================================================== ============
You want                                                              Look at
===================================================================== ============
an app that learns facts from chat logs                               Mem0, Zep
an agent that edits its own long-term memory                          Letta
a shared notebook of Markdown for you and the agent                   Basic Memory
a ticket queue for agents with a SQL store                            beads
bugs that travel with the code in git                                 git-bug
two or more agents on one tracker, voting, with memory a person wrote ljos
===================================================================== ============

Sources
=======

Read on 2026-11-10.

- beads: `README <https://github.com/steveyegge/beads>`__ (features, commands, licence), `docs <https://beads.gascity.com/>`__.
- Mem0: `open-source overview <https://docs.mem0.ai/open-source/overview>`__ (default components), `add <https://docs.mem0.ai/core-concepts/memory-operations/add>`__ (extraction, ``infer``), `licence <https://github.com/mem0ai/mem0/blob/main/LICENSE>`__.
- Letta: `SDK page <https://docs.letta.com/concepts/memgpt>`__ (memory blocks, MemFS, channels, self-hosting), `letta-code <https://github.com/letta-ai/letta-code>`__ (current source, Apache-2.0).
- Zep and Graphiti: `Zep concepts <https://help.getzep.com/concepts>`__ (fact invalidation, provenance), `Graphiti README <https://github.com/getzep/graphiti>`__ (graph databases, model and embedder, Apache-2.0).
- Basic Memory: `README <https://github.com/basicmachines-co/basic-memory>`__ (Markdown files, Model Context Protocol server, semantic search, optional cloud, AGPL-3.0).
- git-bug: `README <https://github.com/git-bug/git-bug>`__ (issues in git, bridges, GPL-3.0).
