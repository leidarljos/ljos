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

   $ install -m 755 scripts/grok/ljos-inject.sh ~/.grok/hooks/ljos-inject.sh
   $ python3 - <<'PY'
   from pathlib import Path
   import os
   src = Path("scripts/grok/ljos.json").read_text()
   src = src.replace("${HOME}", os.path.expanduser("~"))
   dest = Path.home() / ".grok" / "hooks" / "ljos.json"
   dest.parent.mkdir(parents=True, exist_ok=True)
   dest.write_text(src)
   print(dest)
   PY

Grok needs an absolute ``command`` path. The JSON in the tree keeps
``${HOME}`` so it is not machine-specific; expand it on install.

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
           "matcher": "*",
           "hooks": [
             {
               "type": "command",
               "command": "${HOME}/.grok/hooks/ljos-inject.sh",
               "timeout": 20
             }
           ]
         }
       ]
     }
   }

Source of truth: `scripts/grok/ljos.json <../../scripts/grok/ljos.json>`__.

~/.grok/hooks/ljos-inject.sh
============================

.. code:: bash

   #!/bin/sh
   # Grok Build does not add UserPromptSubmit additionalContext to the model.
   # ljos hook is silent on PreToolUse. Remap stdin to UserPromptSubmit so the
   # pack injects, then emit PreToolUse additionalContext which Grok does deliver.
   set -eu
   LJOS_BIN="${LJOS_BIN:-$(command -v ljos)}"
   input=$(cat)
   event=$(printf '%s' "$input" | python3 -c 'import json,sys
   try:
       d=json.load(sys.stdin)
   except Exception:
       print("UserPromptSubmit")
       raise SystemExit
   print(d.get("hook_event_name") or d.get("hookEventName") or "UserPromptSubmit")')

   # Always query the pack as a prompt event so ljos hook speaks.
   query=$(printf '%s' "$input" | python3 -c 'import json,sys
   d=json.load(sys.stdin)
   bits=["Call ljos sitting. Never weaken tests. CI red is a defect.",
         d.get("prompt") or "",
         str(d.get("toolName") or ""),
         str((d.get("toolInput") or {}).get("command") or d.get("toolInput") or "")]
   print(" ".join(bits)[:2000])')

   payload=$(python3 -c 'import json,sys
   print(json.dumps({"hook_event_name":"UserPromptSubmit","prompt":sys.argv[1]}))' "$query")

   out=$(printf '%s' "$payload" | "$LJOS_BIN" hook --limit 8 || true)
   ctx=$(printf '%s' "$out" | python3 -c 'import json,sys
   raw=sys.stdin.read()
   try:
       d=json.loads(raw)
   except Exception:
       print("")
       raise SystemExit
   print((d.get("hookSpecificOutput") or {}).get("additionalContext") or "")')

   if [ -z "$ctx" ]; then
     exit 0
   fi

   # SessionStart stdout is ignored by Grok. Write a brief the sitting can open.
   if [ "$event" = "SessionStart" ] || [ "$event" = "session_start" ]; then
     printf '%s\n' "$ctx" > "${XDG_RUNTIME_DIR:-/tmp}/ljos-grok-brief.txt"
     exit 0
   fi

   python3 -c 'import json,sys
   ctx=sys.argv[1]
   event=sys.argv[2]
   print(json.dumps({
     "hookSpecificOutput": {
       "hookEventName": event if event in ("PreToolUse","UserPromptSubmit","Stop") else "PreToolUse",
       "additionalContext": ctx,
     }
   }))' "$ctx" "$event"
   exit 0

Source of truth: `scripts/grok/ljos-inject.sh <../../scripts/grok/ljos-inject.sh>`__.

Smoke
=====

.. code:: console

   $ echo '{"hook_event_name":"PreToolUse","toolName":"run_terminal_command","toolInput":{"command":"pytest"}}' \
       | ~/.grok/hooks/ljos-inject.sh

Stdout must be JSON with ``hookSpecificOutput.additionalContext`` and
pack lines. Empty stdout means the pack had nothing for that cue.

harnesses.toml
==============

.. code:: toml

   [[harness]]
   name = "grok"
   config = "~/.grok/config.toml"
   marker = "[mcp_servers.ljos]"
   skills = "~/.config/ljos/skills"
   hooks = "~/.grok/hooks/ljos.json"
   hook_events = ["SessionStart", "UserPromptSubmit", "PreToolUse"]

Keep the ``UserPromptSubmit`` file for runners that honor that event's
``additionalContext``. Grok is not one of them.
