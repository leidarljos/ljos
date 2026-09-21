Command line
============

=============================================================================== ========================= ================================================================================================================================================================================================================================================================================================
Verb                                                                            Habitat                   Does
=============================================================================== ========================= ================================================================================================================================================================================================================================================================================================
``sitting ISSUE [--assignee NAME] [--cards DIR] [--playbook NAME] [--anyway]``   all                       open a sitting: doctor, cards, due, island, playbook, recall, timeline, claim; stops at the first store down; the name defaults to ``LJOS_SEAT``; ``--playbook`` copies a recipe before recall; absent, title-match else ``sit``; tracker ``playbook:`` note until finish or release
``finish ISSUE [--status S] [--lesson TEXT] [--outcome OPTION] [--beta B] [--close]``   all                  close a sitting: remember, fire the island, complete, learn; ``--close`` closes the ticket when the work is accepted |
``calibrate -p PROJECT [--rounds N]``                                           consensus, pack           trust rows from the project's voting history (Dawid-Skene accuracy)
``remember TEXT [--as PERSONA]``                                                pack                      one lesson, stored as written; as a persona, it carries that persona's entity and opens its next brief
``prefer TEXT``                                                                 pack                      one standing preference
``forget ID [--why DEED]``                                                      pack                      tombstone a claim, naming the deed that withdrew it
``search QUERY [-n N] [--rerank] [--as-of TIME]``                               pack                      ranked claims with how many scorers named each and its age; ``--rerank`` scores the top hits with the writer's cross-encoder; ``--as-of`` asks the pack as it stood then
``timeline ISSUE [-n N]``                                                       tracker, deed store, pack the logbook, the cited deeds and the activated memories as one dated list, oldest first, each line with its age and the gap since the one before
``consolidate [--apply]``                                                       pack                      the pairs where a later claim rewrites an earlier one; ``--apply`` closes the earlier ones' windows
``conflicts [-n N]``                                                            pack, landscape           candidate contradictions by geometry: the lowest passes between single memories in the embedding landscape, from the optional ``landscape`` habitat
``hubs [-n N]``                                                                 pack                      the claims this seat's memory turns on, highest first: a weighted PageRank over the links
``island CUE [--fire]``                                                         pack                      the memories a task activates: search hits spread along the links; ``--fire`` wires the strongest eight together
``due``                                                                         pack                      claims whose review is due, unreviewed ones first, then one line on the clock
``graded ID [--lapsed]``                                                        pack                      one review graded
``trust FROM TO WEIGHT [--why DEED]... [--about DOMAIN]...``                    pack                      one trust row; scoped when ``--about`` is given
``persona NAME --anchor A --view TEXT [--about DOMAIN]...``                     pack                      a voter with a view; writes one unscoped inbound trust row
``playbook ISSUE NAME``                                                         pack                      bind a recipe to the issue and copy its full body; tracker ``playbook:`` note until finish or release
``playbooks``                                                                   pack                      the five shipped recipes (sit, arena, land, company-panel, overnight) and any others
``brief NAME ISSUE``                                                            pack, tracker             view, bound playbook body, five principles, arena rubric, domains, working set
``panel ISSUE [--out DIR]``                                                     pack, tracker             one brief per persona as ``DIR/<name>.md``; refused until a playbook is bound
``learn ID --outcome OPTION [--rule record\vert hedge] [--beta B] [--share S]`` tracker, pack             reweigh voters by what turned out right: by default each voter's record of hits and misses as log-odds weights; ``hedge`` shrinks refuted voters by ``B`` with ``S`` recovery
``evidence ACCESSION`` / ``current ACCESSION``                                  deed store                intact; still the tip
``deed ID [--add ACCESSION]``                                                   tracker                   cite a deed on a node, or list citations
``recall ID``                                                                   tracker                   the working set
``vote ID [--for OPTION] [--as PERSONA]``                                       tracker                   cast, as the seat or as a persona, or read the tally
``predict ID --expect OPTION\vert JSON [--as PERSONA]``                         pack                      forecast the others' shares; two or more and ``consensus`` names the surprisingly popular answer
``rule PATTERN [--verdict deny\vert ask] --why TEXT``                           pack                      argv law: a glob over the command line the hook and ``policy`` enforce
``consensus ID``                                                                consensus, tracker        settle under the pack's rows that apply to the issue, with every persona's anchor; an issue tagged ``broad`` runs bounded confidence
``claim ID [--assignee NAME]``                                                  claim graph               a session node for a tracker id; a busy refusal names what the name still holds; a node this name holds is a sitting resumed
``release ID [--assignee NAME]``                                                claim graph               hand the session node back unfinished: ready, generation moved
``complete ID [--status done\vert failed\vert cancelled]``                      claim graph               finish the session node
``cards [--dir DIR]``                                                           cards                     print ``USER.md`` and ``MEMORY.md``
``policy ARGV...``                                                              policy, pack              the line as it would run, the TCB verdict if ``ljos-policyd`` answered, then a matching pack rule, then what the pack knows that bears on it
``hook [--limit N]``                                                            policy, pack              hook JSON or argv on stdin; on ``PreToolUse`` the TCB and pack rules, a deny is ``permissionDecision``; on a prompt the hits scoring at least 0.6 of the best that two scorers named, preferences first then lessons oldest first, each with its age, five at most, each once per runner session
``handover --out DIR [--project P]... [--issue I]... [--to user@host:path]``    all                       pack, seal, sign; ``--to`` copies the bag to another seat over ssh
``receive DIR [--since BRIDGE] [--import]``                                     all                       check, and import the atoms
``doctor``                                                                      all                       which habitats answer, and whether the harnesses found are onboarded
``protocol``                                                                    none                      print the sitting protocol
``onboard [--harness NAME\vert json] [--dry-run] [--example]``                  runner                    register ``ljos-mcp`` with a runner named in ``~/.config/ljos/harnesses.toml`` and install the protocol as its skill; ``json`` prints the entry
=============================================================================== ========================= ================================================================================================================================================================================================================================================================================================

A tracker id maps to one claim-graph node (FNV-1a 128 of the id) and a name
to one actor; a 32-hex id passes through.

Model Context Protocol (MCP) tools
==================================

Writers: ``ljos_sitting``, ``ljos_finish``, ``ljos_calibrate``, ``ljos_persona``, ``ljos_playbook``, ``ljos_remember``, ``ljos_prefer``, ``ljos_forget``, ``ljos_trust``,
``ljos_learn``, ``ljos_graded``, ``ljos_island``, ``ljos_deed``, ``ljos_vote``, ``ljos_predict``, ``ljos_rule``, ``ljos_claim``,
``ljos_release``, ``ljos_complete``, ``ljos_consolidate``, ``ljos_handover``, ``ljos_receive``. Readers: ``ljos_search`` (with ``as_of``), ``ljos_conflicts``,
``ljos_timeline``, ``ljos_due``, ``ljos_evidence``, ``ljos_current``, ``ljos_recall``,
``ljos_consensus``, ``ljos_brief``, ``ljos_personas``, ``ljos_playbooks``, ``ljos_cards``, ``ljos_policy``, ``ljos_doctor``. Resources:
``ljos://protocol``, ``ljos://cards/USER.md``, ``ljos://cards/MEMORY.md``. Prompts:
``start_a_sitting``, ``run_a_panel``, ``check_a_handover``. Every tool description opens with
when to call it.

Environment
===========

=========================================== =============================================================================================================
Variable                                    Read by
=========================================== =============================================================================================================
``PACKSET_URL``                             the pack client; unset, ``http://127.0.0.1:8761`` (``PACKSET_PORT`` moves the port); ``off`` means no pack
``PACKSET_WORKSPACE``                       the pack workspace; unset, ``seat``
``VISSUE_ROOT``, ``VISSUE_AGENT``           the tracker
``DEEDAR_URL``, ``DEEDAR_HOST_SIGNING_KEY`` the deed store; the key signs handovers, and ``~/.config/deedar/host.key`` is used when the variable is unset
``CLAIMDAG_DIR``                            the claim graph
``LJOS_CARDS_DIR``                          the server's cards directory
``XDG_CONFIG_HOME``                         where ``ljos/harnesses.toml`` is read from; ``~/.config`` when unset
=========================================== =============================================================================================================

A runner's table in ``harnesses.toml`` may name ``hooks``, a JSON settings
file of the shape ``{"hooks": {"<Event>": [{"matcher": "...", "hooks":
[{"type": "command", "command": "..."}]}]}}``. ``onboard`` merges
``ljos hook`` into it on the prompt and session-end events (matcher ``*``) by
default, or on the events the table's ``hook_events`` lists (``PreToolUse``
takes the matcher ``Bash``), once each, and drops it from events no longer
listed. On ``SessionEnd`` the hook fires the memories it injected during the
session together and clears the session's record. The hook reads the runner's JSON on stdin
(``hook_event_name``, ``tool_input.command``, ``prompt``) and answers
``{"hookSpecificOutput": {"hookEventName": ..., "additionalContext": ...}}``,
or nothing when the pack holds nothing on the cue. On ``PreToolUse`` a TCB
or pack deny is ``permissionDecision`` ``deny`` and blocks. Plain text on
stdin is an argv line and answered in plain lines.

The learning rules
==================

After an outcome, every voter whose ballot it refuted shrinks in every
other voter's row by ``beta`` (default 0.5), floored at 0.01. A vindicated
voter keeps its weight, and a missing row starts at 1. Rows are written
complete, so the settle sees the whole graph. The update is the
multiplicative weights rule of Hedge (doi:10.1006/jcss.1997.1504).

A row carries the domains it is scoped to. An unscoped row applies to
every issue; a scoped row applies when one of its domains is a word of the
issue's title. ``learn`` writes its rows scoped to the entities of the island
the issue's title activates, eight at most, so a voter refuted on one topic
keeps its standing on the rest; a scoped learn starts from the unscoped row
when it has none of its own and leaves the unscoped row standing. When
both apply, the scoped row is the one the settle sees. With ``--share S``
every row then moves toward one by ``S`` of the gap, the fixed-share rule
of Herbster and Warmuth (doi:10.1023/A:1007424614876), so a voter refuted
long ago is not held down forever and the best voter can change; the
default is zero, plain Hedge. An outcome also
moves the anchor of every persona it refuted toward one by ``1 - beta`` of
the gap: a persona that keeps being wrong listens more.

A persona is a ``persona`` atom: a name, an anchor in [0, 1], a view, and the
domains it speaks to. ``consensus`` passes every persona's anchor to both
settles as ``--susceptibility-of``; at 0 the persona never moves off its
ballot and pulls the rest toward it (Acemoglu, Como, Fagnani and Ozdaglar,
doi:10.1287/moor.1120.0570), at 1 it is a plain voter.

Without an outcome, ``calibrate`` estimates each voter's accuracy from the
project's issues with two or more ballots by expectation maximisation over
the items' hidden answers (Dawid and Skene, doi:10.2307/2346806), then
writes each accuracy, floored at 0.01, as the weight every other voter
gives that voter. That is the weight a linear opinion pool assigns a source
believed that reliable (Genest and Zidek, doi:10.1214/ss/1177013825).

Crates
======

================== =========================================================================
Crate              Carries
================== =========================================================================
``ljos-cli``       the library and ``ljos``
``ljos-mcp``       the MCP server
``ljos-consensus`` DeGroot and Friedkin-Johnsen settles, Seldon export (separate repository)
================== =========================================================================
