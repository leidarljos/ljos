The pack only helps a Grok session if it reaches the model.

Grok discards ``UserPromptSubmit`` stdout. It delivers ``PostToolUse``
``additionalContext`` after the first tool. ``ljos hook`` searches on the
prompt, holds the text, and emits it once on ``PostToolUse``.
A conversation that holds no issue is told, on that first result, to
file one and sit. ``Stop`` holds such a turn once when it used tools and
never touched the seat. A prompt that asks which answer is right forks a panel opener before
the model speaks. The opener files or reuses the decision and starts
one headless member per brief. The model is told not to pick and not
to ssh. ``Stop`` still holds the turn once if it picks with no ballot. ``PreToolUse`` is TCB and pack rules; a deny
blocks. There is no ``sync.sh``.

Install
=======

.. code:: console

   $ ljos onboard --harness grok

That writes ``~/.grok/hooks/ljos.json`` once, naming ``ljos`` by the absolute
path beside ``ljos-mcp``, so a Grok started outside a login shell still finds
it. Then
``/hooks`` then ``r`` **once**. Later ``cargo binstall ljos`` is live on the
next event. Name ``ljos-mcp`` on PATH in ``~/.grok/config.toml``.
``ljos onboard --harness grok`` bumps ``LJOS_MCP_GENERATION`` in that table
when the crate version moved, so Grok's config watcher respawns the
server; a session restart is not required.

Why these events
================

====================== ============================================================================================================================================== ===================================================
Event                  What ``ljos hook`` does                                                                                                                        What Grok does with stdout
====================== ============================================================================================================================================== ===================================================
``UserPromptSubmit``   searches, holds the text                                                                                                                       discarded
``PostToolUse``        emits the held text once; with no issue held, the first result says to file and sit                                                            delivered after the tool
``PostToolUseFailure`` searches the pack on the failed command and its error; up to three standing claims that share at least two content words with them             delivered with the failed result
``PreToolUse``         TCB and pack rules; a deny blocks                                                                                                              a turn has many tool calls
``PreCompact``         fires up to eight memories injected so far and lets a later prompt bring them back; holds a note naming the issue the conversation still holds ignored; the next ``PostToolUse`` delivers the note
``Stop``               with no issue held, holds one turn that used tools and never touched the seat                                                                  ``decision`` ``block`` continues the turn once
``SessionEnd``         fires injected memories                                                                                                                        ignored
====================== ============================================================================================================================================== ===================================================

Grok's hook dispatcher, ``xai-grok-hooks/src/dispatcher.rs``, hands a hook's
added context to the model after the three tool events, ``Stop`` and
``SubagentStop``. It drops it after a prompt or at the start of a session.

The frozen file is
`crates/ljos-cli/assets/grok/ljos.json <../../crates/ljos-cli/assets/grok/ljos.json>`__.
Its ``{ljos}`` is filled in at onboard. ``PreToolUse`` gets 10 seconds, the
TCB check's budget, as do the two tool-result events; the prompt, ``Stop``
and ``SubagentStop`` get 15, and ``PreCompact`` and ``SessionEnd`` 5, Grok's
default.

Grok's stdin is camelCase (``hookEventName``, ``sessionId``, ``toolInput``), and
``ljos hook`` reads it as the snake\ :sub:`case` fields. A deny blocks on a top-level
``decision`` of ``deny`` beside ``hookSpecificOutput``. An ask rule is the same
pair with ``ask``: Grok shows that as the in-chat permission prompt. A runner
that cannot ask (stdin carrying ``turn_id``) still gets the ask rewritten to
a deny.

Smoke
=====

.. code:: console

   $ echo '{"hook_event_name":"PreToolUse","tool_input":{"command":"echo"}}' | ljos hook
   $ echo '{"hookEventName":"pre_tool_use","toolInput":{"command":"git push --force"}}' | ljos hook
   {"decision":"deny","hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"git-force-push (seat rule `ljos-policyd`)"},"reason":"git-force-push (seat rule `ljos-policyd`)"}
   $ echo '{"hook_event_name":"PostToolUse","session_id":"s"}' | ljos hook

An allow prints nothing unless a prompt has already held pack text.
A TCB deny still blocks.

Personas as Grok agents
=======================

Grok spawns a subagent by its agent definition, a Markdown file with YAML
front matter in ``~/.grok/agents``. ``ljos onboard --harness grok`` and
``ljos agents --harness grok`` write each persona there as ``ljos-NAME.md``.
Grok Build's own parser, ``AgentDefinition::parse`` in
``xai-grok-agent/src/config.rs``, accepts each file. Its front matter sets
``capabilityMode: execute``: it reads and runs commands and has no edit
tool. The persona runs ``ljos brief NAME ISSUE``, casts one ballot before
reading the others, notes why, and stops. A panel is one ``spawn_subagent``
call a persona, with ``subagent_type`` ``ljos-NAME`` and the issue id as its
prompt; the main agent then runs ``ljos consensus ISSUE``.

A panel the seat opens itself, on a prompt that asks which answer is
right, starts its members with the table's ``headless`` argv. Grok's is
``grok --prompt-file ... --yolo --max-turns 6 --effort low --disallowed-tools Agent --cwd ...``.

Status line
===========

.. code:: toml

   [ui.status_line]
   type = "command"
   command = "ljos statusline"

The row shows the seat, the issue this conversation holds and the claims
due; each session's row is cached for fifteen seconds.

harnesses.toml
==============

.. code:: toml

   [[harness]]
   name = "grok"
   config = "~/.grok/config.toml"
   marker = "[mcp_servers.ljos]"
   skills = "~/.config/ljos/skills"
   hooks = "~/.grok/hooks/ljos.json"
   agents = "~/.grok/agents"
   headless = ["grok", "--prompt-file", "{prompt_file}", "--yolo", "--max-turns", "6", "--effort", "low", "--disallowed-tools", "Agent", "--cwd", "{cwd}"]
   resume = ["grok", "--continue"]
