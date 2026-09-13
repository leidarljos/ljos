==========
Grok Build
==========

The pack only helps a Grok session if it reaches the model.

``ljos onboard --harness grok`` writes a ``UserPromptSubmit`` hook to
``~/.config/ljos/hooks.json``. Grok does not load that path. Grok also
discards ``UserPromptSubmit`` ``additionalContext`` (observe-only stdout).
Grok *does* deliver ``PreToolUse`` ``additionalContext`` after the tool.

Install the two files below under ``~/.grok/hooks/``, then ``/hooks``
then ``r``. A sitting (``ljos sitting ID``) is still a verb. The hook is
how the pack arrives without being asked. CI red is still a defect;
do not skip tests or pin a deleted wrap to go green.

Install
-------

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
--------------

================================== ======================================== ==========================================
Event                              What ``ljos hook`` does                  What Grok does with stdout
================================== ======================================== ==========================================
``UserPromptSubmit``               emits ``additionalContext``              discarded
``PreToolUse``                     silent                                   ``additionalContext`` after the tool
``SessionStart``                   silent                                   stdout ignored
================================== ======================================== ==========================================

The inject script therefore always calls ``ljos hook`` as
``UserPromptSubmit``, then re-emits the pack as ``PreToolUse``
``additionalContext``.

``~/.grok/hooks/ljos.json``
---------------------------

.. literalinclude:: ../../scripts/grok/ljos.json
   :language: json

``~/.grok/hooks/ljos-inject.sh``
--------------------------------

.. literalinclude:: ../../scripts/grok/ljos-inject.sh
   :language: sh

Smoke
-----

.. code:: console

    $ echo '{"hook_event_name":"PreToolUse","toolName":"run_terminal_command","toolInput":{"command":"pytest"}}' \
        | ~/.grok/hooks/ljos-inject.sh

Stdout must be JSON with ``hookSpecificOutput.additionalContext`` and
pack lines. Empty stdout means the pack had nothing for that cue.

harnesses.toml
--------------

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
