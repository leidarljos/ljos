:description: Open persona panes in another tool and spawn a persona as a subagent.
:cover: https://leidarljos.github.io/ljos/_static/og.png

===================================
Run personas in panes and subagents
===================================


Open persona panes in another tool
==================================

``ljos doctor`` names the tool persona panes open in:

.. code:: console

   $ ljos doctor --all | grep panes
   ok  panes   tmux opens persona panes

With herdr installed the same row says ``herdr opens persona panes; tmux also here``.
herdr opens one workspace a persona, in the persona's home and as its
seat, and types the pane script into the root pane. It hands the runner
each task through ``herdr agent prompt``, or as typed keys when herdr does
not know the runner as an agent. tmux gives each persona a window in the
session ``ljos-personas``. ``LJOS_PANE_TOOL`` narrows the choice to the one it
names. Any other tool is a ``[[tool]]`` table in
``~/.config/ljos/harnesses.toml``, with each verb but ``pane_pointer`` an argv
or a list of argvs. A table named like a built-in shape replaces it. A
terminal multiplexer with a command line takes a table like this:

.. code:: toml

   [[tool]]
   name = "mytool"
   detect = ["mytool", "--version"]
   open = [["mytool", "new-pane", "--cwd", "{home}", "--name", "{label}", "--", "sh", "{script}"]]
   prompt = [["mytool", "send", "--pane", "{label}", "{line}"], ["mytool", "send-key", "--pane", "{label}", "Enter"]]
   alive = ["mytool", "has-pane", "{label}"]
   ready_s = 8

A tool that refuses says so in the line ``ljos ask`` prints, with the
tool's own words, and the next tool is tried.

Spawn a persona as a subagent
=============================

.. code:: console

   $ ljos agents --harness grok

The ok line names the agents directory it wrote.

Every persona becomes ``~/.grok/agents/ljos-NAME.md``, and the build harness's main agent
can spawn it by name on a decision with ``spawn_subagent`` and
``subagent_type`` ``ljos-reviewer``. The persona reads ``ljos brief``, casts one
ballot before reading the others, notes why, and stops. The build harness runs it in
``capabilityMode: execute``, with commands and no edit tool. The terminal harness
reads the same definitions from ``~/.claude/agents``, and the editor harness's copies go
to ``~/.cursor/agents``; both ignore that mode, and the body tells the
persona not to edit. ``onboard`` writes the definitions for a runner whose
table names ``agents``, and a persona written later refreshes each such
directory that exists.
