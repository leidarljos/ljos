:description: The pack only helps a build harness session if it reaches the model.
:cover: https://leidarljos.github.io/ljos/_static/og.png

The pack only helps a build harness session if it reaches the model.

The build harness discards ``UserPromptSubmit`` stdout. It passes ``PostToolUse``
``additionalContext`` to the model after the first tool. ``ljos hook``
searches on the prompt, keeps the text, and emits it once on
``PostToolUse``. With ``LJOS_TRACK_WORK=on``, a conversation with no issue is
told, on that first result, to open one and sit, and ``Stop`` blocks such a
turn once when it used tools and never touched the seat. A prompt that asks which answer is right forks a panel opener before
the model speaks. The opener files or reuses the decision and starts
one headless member per brief. The model is told not to pick and not
to ssh. ``Stop`` still blocks the turn once if it picks with no ballot.
``PreToolUse`` runs the Trusted Computing Base (TCB) check and the pack
rules; a rule that fires is one warning line, and the call runs. Nothing
needs a ``sync.sh``.

Install
=======

.. code:: console

   $ ljos onboard --harness grok

That writes ``~/.grok/hooks/ljos.json`` once, naming ``ljos`` by the absolute
path beside ``ljos-mcp``, so a build harness started outside a login shell still finds
it. Then
``/hooks`` then ``r`` **once**. Later ``cargo binstall ljos`` is live on the
next event. Name ``ljos-mcp`` on PATH in ``~/.grok/config.toml``.
``ljos onboard --harness grok`` bumps ``LJOS_MCP_GENERATION`` in that table
when the crate version moved, so the build harness's config watcher respawns the
server; a session restart is not required.

Why these events
================

====================== ================================================================================================================================================= ====================================================
Event                  What ``ljos hook`` does                                                                                                                           What the build harness does with stdout
====================== ================================================================================================================================================= ====================================================
``UserPromptSubmit``   searches, keeps the text                                                                                                                          discarded
``PostToolUse``        emits the kept text once; with no issue open, the first result says to file and sit                                                               passed to the model after the tool
``PostToolUseFailure`` searches the pack on the failed command and its error; up to three standing claims that share at least two content words with them                passed with the failed result
``PreToolUse``         TCB and pack rules; a rule is a warning in ``additionalContext``, and the call runs                                                               a turn has many tool calls
``PreCompact``         fires up to eight memories injected so far and lets a later prompt bring them back; keeps a note naming the issue the conversation still has open ignored; the next ``PostToolUse`` passes the note on
``Stop``               with no issue open, blocks one turn that used tools and never touched the seat                                                                    ``decision`` ``block`` continues the turn once
``SessionEnd``         fires injected memories                                                                                                                           ignored
====================== ================================================================================================================================================= ====================================================

The build harness's hook dispatcher, ``the harness hook dispatcher``, hands a hook's
added context to the model after the three tool events, ``Stop`` and
``SubagentStop``. It drops it after a prompt or at the start of a session.

The frozen file is
``crates/ljos-cli/assets/grok/ljos.json``.
Its ``{ljos}`` is filled in at onboard. ``PreToolUse`` gets 10 seconds, the
TCB check's budget, as do the two tool-result events; the prompt, ``Stop``
and ``SubagentStop`` get 15, and ``PreCompact`` and ``SessionEnd`` 5, the build harness's
default.

The build harness's stdin is camelCase (``hookEventName``, ``sessionId``, ``toolInput``), and
``ljos hook`` reads it as the snake\ :sub:`case` fields. A rule that fires is its
warning in ``hookSpecificOutput`` ``additionalContext``, with no ``decision``, so
The build harness runs the call and shows the warning.

A panel member is a supervised child
====================================

A child that exits is classified before it is started again.
``ljos supervise NAME --harness grok --exit crash --at SECONDS`` prints
``restart``, ``stop`` or ``give-up``, and on a restart the herdr line
``herdr agent start persona-NAME --no-focus --cwd HOME -- sh SCRIPT``.
The same command with ``--harness claude`` or ``--harness codex`` covers the
other runners: the policy is one, and only the runner name changes. ``permanent`` restarts however it exited, ``transient`` only on a
crash, ``temporary`` never. Three restarts inside sixty seconds give up.
The command prints the decision. It does not start the pane.

Smoke
=====

.. code:: console

   $ echo '{"hook_event_name":"PreToolUse","tool_input":{"command":"echo"}}' | ljos hook
   $ echo '{"hookEventName":"pre_tool_use","toolInput":{"command":"git push --force"}}' | ljos hook
   {"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"allow\twarn\tgit-force-push\ta force push rewrites a published branch"}}
   $ echo '{"hook_event_name":"PostToolUse","session_id":"s"}' | ljos hook

An allow prints nothing unless a prompt has already kept pack text or a rule warned, as the force push does.
The bypass log keeps that label.
A TCB rule is a warning too; the call runs.
``block-on-warning`` does not block. It is a warning with the rule's label in ``hookSpecificOutput``. The build harness runs the call.

Personas as the build harness agents
====================================

The build harness spawns a subagent by its agent definition, a Markdown file with YAML
front matter in ``~/.grok/agents``. ``ljos onboard --harness grok`` and
``ljos agents --harness grok`` write each persona there as ``ljos-NAME.md``.
The build harness's own parser, ``AgentDefinition::parse`` in
``the harness agent config``, accepts each file. Its front matter sets
``capabilityMode: execute``: it reads and runs commands and has no edit
tool. The persona runs ``ljos brief NAME ISSUE``, casts one ballot before
reading the others, notes why, and stops. A panel is one ``spawn_subagent``
call a persona, with ``subagent_type`` ``ljos-NAME`` and the issue id as its
prompt; the main agent then runs ``ljos consensus ISSUE``.

On a prompt that asks which answer is right, the seat opens a panel
itself and starts its members with the table's ``headless`` argv.
``ljos onboard --example`` prints that argv.

Status line
===========

.. code:: toml

   [ui.status_line]
   type = "command"
   command = "ljos statusline"

The row shows the seat, the issue this conversation has open and the
claims due; each session's row is cached for fifteen seconds.

The runners file
================

The runners file has one table for this harness. The table names the
config path, the marker, the skills directory, the hooks file, the agents
directory, and the argv that starts a headless run and the argv that
resumes one. ``ljos onboard --example`` prints the table.
