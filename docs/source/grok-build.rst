The pack only helps a Grok session if it reaches the model.

``ljos onboard --harness grok`` writes a ``UserPromptSubmit`` hook to
``~/.config/ljos/hooks.json``. Grok does not load that path. Grok also
discards ``UserPromptSubmit`` ``additionalContext`` (observe-only stdout).
Grok **does** deliver ``PreToolUse`` ``additionalContext`` after the tool.

Install the two files below under ``~/.grok/hooks/``, then ``/hooks``
then ``r``. A sitting (``ljos sitting ID``) is still a verb. The hook is
how the pack arrives without being asked. CI red is still a defect;
do not skip tests or pin a deleted wrap to go green.

Install
=======

.. code:: console

   $ scripts/grok/sync.sh

That copies ``ljos-inject.sh`` and writes ``ljos.json`` only when the
matchers changed. The script and ``ljos hook`` are exec'd each event, so
they are live without ``/hooks`` then ``r``. Grok does **not** watch hook
JSON (skills do; hooks do not). Reload matchers only when ``sync.sh``
prints ``ljos.json changed``. A replaced ``ljos-mcp`` that is still
running as a deleted inode is sent TERM so Grok respawns PATH.

Why this remap
==============

==================== =========================== =================================================
Event                What the hook does          What Grok does with stdout
==================== =========================== =================================================
``UserPromptSubmit`` inject searches the pack    discarded; remapped to a deliverable event
``PreToolUse``       **rules only**, no search   a turn has many tool calls; do not search
``SessionStart``     writes a brief file         stdout ignored
==================== =========================== =================================================

The inject script searches on a **prompt** or session start. On
``PreToolUse`` it exits 0 without talking to the pack. A previous
remap of every tool call into ``UserPromptSubmit`` made Grok wait
up to 20s per tool (``Ljos Ljos Due``, hook timed out).

~/.grok/hooks/ljos.json
=======================

.. code:: json

   {
     "hooks": {
       "SessionStart": [
         {
           "hooks": [
             {
               "type": "command",
               "command": "${HOME}/.grok/hooks/ljos-inject.sh",
               "timeout": 20
             }
           ]
         }
       ],
       "UserPromptSubmit": [
         {
           "hooks": [
             {
               "type": "command",
               "command": "${HOME}/.grok/hooks/ljos-inject.sh",
               "timeout": 20
             }
           ]
         }
       ],
       "PreToolUse": [
         {
           "matcher": "Bash",
           "hooks": [
             {
               "type": "command",
               "command": "${HOME}/.local/bin/ljos hook",
               "timeout": 5
             }
           ]
         }
       ]
     }
   }

Source of truth: `scripts/grok/ljos.json <../../scripts/grok/ljos.json>`__.

~/.grok/hooks/ljos-inject.sh
============================

The script searches on a prompt. On ``PreToolUse`` it exits 0 without
talking to the pack. Source of truth:
`scripts/grok/ljos-inject.sh <../../scripts/grok/ljos-inject.sh>`__.

Smoke
=====

.. code:: console

   $ echo '{"hook_event_name":"PreToolUse","toolName":"run_terminal_command"}' \
       | ~/.grok/hooks/ljos-inject.sh

Stdout must be empty. A tool call is not a sitting.

harnesses.toml
==============

.. code:: toml

   [[harness]]
   name = "grok"
   config = "~/.grok/config.toml"
   marker = "[mcp_servers.ljos]"
   skills = "~/.config/ljos/skills"
   hooks = "~/.grok/hooks/ljos.json"
   hook_events = ["SessionStart", "UserPromptSubmit"]

Keep the ``UserPromptSubmit`` file for runners that honor that event's
``additionalContext``. Grok is not one of them.
