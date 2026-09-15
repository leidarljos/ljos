The pack only helps a Grok session if it reaches the model.

Grok discards ``UserPromptSubmit`` stdout. It delivers ``PostToolUse``
``additionalContext`` after the first tool. ``ljos hook`` searches on the
prompt, holds the text, and emits it once on ``PostToolUse``.
``PreToolUse`` is TCB and pack rules; a deny blocks. There is no ``sync.sh``.

Install
=======

.. code:: console

   $ ljos onboard --harness grok

That writes ``~/.grok/hooks/ljos.json`` once (``ljos hook`` on PATH). Then
``/hooks`` then ``r`` **once**. Later ``cargo binstall ljos`` is live on the
next event. Name ``ljos-mcp`` on PATH in ``~/.grok/config.toml``. Bump
``LJOS_MCP_GENERATION`` in that table when you want Grok's config
watcher to respawn the server.

Why these events
================

==================== ================================= ==========================
Event                What ``ljos hook`` does           What Grok does with stdout
==================== ================================= ==========================
``UserPromptSubmit`` searches, holds the text          discarded
``PostToolUse``      emits the held text once          delivered after the tool
``PreToolUse``       TCB and pack rules; a deny blocks a turn has many tool calls
``SessionEnd``       fires injected memories           ignored
==================== ================================= ==========================

The frozen file is `scripts/grok/ljos.json <../../scripts/grok/ljos.json>`__. It only names ``ljos hook``.

Smoke
=====

.. code:: console

   $ echo '{"hook_event_name":"PreToolUse","tool_input":{"command":"echo"}}' | ljos hook
   $ echo '{"hook_event_name":"PreToolUse","tool_input":{"command":"git push --force"}}' | ljos hook
   {"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"git-force-push (seat rule `ljos-policyd`)"}}
   $ echo '{"hook_event_name":"PostToolUse","session_id":"s"}' | ljos hook

An allow prints nothing unless a prompt has already held pack text.
A TCB deny still blocks.

harnesses.toml
==============

.. code:: toml

   [[harness]]
   name = "grok"
   config = "~/.grok/config.toml"
   marker = "[mcp_servers.ljos]"
   skills = "~/.config/ljos/skills"
   hooks = "~/.grok/hooks/ljos.json"
   hook_events = ["UserPromptSubmit", "PostToolUse", "PreToolUse", "SessionEnd"]
