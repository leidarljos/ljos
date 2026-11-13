:description: Wire ljos into a Model Context Protocol (MCP) runner, an editor or a shell-only agent. Show the seat in the status bar.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=======================
Connect an agent runner
=======================


Wire the seat into an agent runner
==================================

One entry, for any runner that speaks the Model Context Protocol (MCP)
over stdio:

.. code:: console

   $ ljos onboard
   {"mcpServers": {"ljos": {"type": "stdio", "command": "ljos-mcp", "args": [], "env": {}}}}

Paste it where the runner keeps its MCP servers. The ``env`` is empty on
purpose. The server does not take the seat from the client that connects.
Set ``LJOS_SEAT`` in that entry's ``env``, or a ``seat=`` line in
``~/.config/ljos/env``, or a row in ``~/.config/ljos/seats.toml``.
A shell the runner opens finds the same seat through ``LJOS_SEAT``.
The process tree is not a seat. Each conversation keeps its claims as
the seat tagged with its session, so two conversations of one runner hold two tickets.

.. code:: console

   $ ljos seat
   seat    every-seat
   holder  every-seat
   source  no seat rule matched; reading every seat

A read with no seat named looks at every seat. Set one with ``export LJOS_SEAT=NAME``.
After a client connects, ``ljos seat`` in that server's environment still
reads ``LJOS_SEAT``. The name the client sent is not the source. A write
with none set uses ``unknown-`` and the host name, and says to set ``LJOS_SEAT``.

The protocol goes where the agent reads its instructions: ``ljos protocol``
prints it, the server serves the same text at ``ljos://protocol`` and names
it in its instructions, so a runner that loads neither skills nor
resources still reads it first. Each tool description opens with when to
call it.

A runner that registers servers by a command of its own, or reads them
from a config file, can be described once in
``~/.config/ljos/harnesses.toml`` and onboarded in one verb.
``ljos onboard --example`` prints the shape:

.. code:: toml

   [[harness]]
   name = "runner-with-a-command"
   register = ["runner", "mcp", "add", "-s", "user", "ljos", "--", "{server}"]
   registered = ["runner", "mcp", "get", "ljos"]
   skills = "~/.runner/skills"

   [[harness]]
   name = "runner-with-a-config-file"
   config = "~/.other/config.toml"
   marker = "[mcp_servers.ljos]"
   snippet = "\n[mcp_servers.ljos]\ncommand = \"{server}\"\nargs = []\n"
   skills = "~/.other/skills"

.. code:: console

   $ ljos onboard --harness runner-with-a-command
   ok  runner-with-a-command mcp   ran runner mcp add -s user ljos -- ljos-mcp
   ok  skill   wrote /home/you/.runner/skills/ljos/SKILL.md

``--dry-run`` reports what would be written. The name ``runner`` in the example
is not a program. Until that program is on ``PATH``, the mcp line is
``no`` and says ``runner not on PATH``, and the command exits 1. The skill
line can still be ``ok``. Copy the tables from ``ljos onboard --example``.
That file also passes ``XDG_RUNTIME_DIR`` for a runner that rebuilds its
environment.

The command harness runs a hook only after you trust it. After ``ljos onboard --harness codex``,
open ``/hooks`` and trust each ljos hook before it runs.
Trust it again after an upgrade rewrites one. Until then the hook does not run,
and policyd sees none of the command harness's commands. ``ljos doctor`` reports an untrusted hook when
the trust file has no ``trusted_hash``, or is older than the hooks file.

The example carries the runners this seat has carried through one piece
of work, each in the shape it takes the server: a runner with an ``mcp add`` of its own by that command; a runner with a TOML config
by a table appended to it; ``opencode`` and ``omp`` by a JSON pointer set in
their MCP file; ``hermes`` by its ``mcp add`` with the tool question
answered. The same verb takes ``cursor`` (the IDE and the ``agent`` CLI share
``~/.cursor/mcp.json``), ``windsurf``, ``zed``, ``vscode``, ``claude-desktop``,
``gemini``, ``amazonq`` and ``kiro``. Copy the tables for the runners on the
machine into ``harnesses.toml``, then one verb each:

.. code:: console

   $ for h in opencode hermes omp; do ljos onboard --harness $h; done
   $ ljos doctor --all | grep runner
   ok  runner mcp  opencode: ljos registered
   ok  runner skill    opencode: /home/you/.config/opencode/skills/ljos/SKILL.md
   ok  runner mcp  hermes: ljos registered
   ok  runner skill    hermes: /home/you/.hermes/skills/ljos/SKILL.md
   ...

A runner with a hooks file (``hooks`` in its table) takes the memory hook
too; for the others the skill and the ``ljos://protocol`` resource carry
what the seat knows, and the sitting's island brings the memories in.

Wire the seat into the editor harness
=====================================

.. code:: console

   $ ljos onboard --harness cursor

The command prints one ok line for the MCP entry, the hooks, the skill and the agent
definitions, and each line names the file it wrote.

The editor harness's IDE and its ``agent`` command line take the server from the same
``~/.cursor/mcp.json`` and the hooks from ``~/.cursor/hooks.json``. It
does not hand a prompt hook's context to the model, so the memories a
prompt activates arrive with the first tool result instead, as on the build harness.
A shell command that a rule names is allowed, with the rule's
warning in the editor harness's ``user_message``. The editor harness runs the terminal harness hooks itself where
``ljos onboard --harness claude`` already ran. The events the terminal harness's file
names stay there. ``beforeShellExecution``, ``preToolUse``,
``beforeMCPExecution``, ``beforeReadFile`` and ``postToolUseFailure`` are
still written to ``~/.cursor/hooks.json``. If a hook fails, the command runs. The seat answers
both files in the editor harness's shape.

Seat a shell-only agent
=======================

Example Host runs shell commands on a Linux box. It is not an MCP runner and
it has no command hook, so the env file names its seat.

.. code:: console

   $ ljos onboard --harness example-host --skills ~/.example-host/skills
   ok  skill   wrote /home/you/.example-host/skills/ljos/SKILL.md
   ok  env wrote /home/you/.config/ljos/example-host.env; source /home/you/.config/ljos/example-host.env
   $ source ~/.config/ljos/example-host.env
   $ echo "$prompt" | ljos hook --prompt
   $ ljos policy -- git push --force

``--skills DIR`` writes the protocol there. ``ljos onboard --harness shell --skills DIR``
is the same shape under the name ``shell``. The env file sets
``LJOS_SEAT`` to the runner's name. Nothing is registered as an MCP server
or a hook. ``ljos hook`` and ``ljos policy`` exit 0 on every answer, and a rule
that fires is one ``warning:`` line in the text. The command runs either way,
unless the person turned on blocking; then ``--fail-on-deny`` exits 1 on
policyd's ``deny``.

Give an agent the protocol without a runner
===========================================

.. code:: console

   $ ljos protocol --full > instructions.md

The text is plain markdown: which store answers which question, the order
of verbs before, during and after the work, and the refusals worth knowing.
Put it where the agent reads its instructions.

Show the seat in the status bar
===============================

In ``~/.grok/config.toml``:

.. code:: toml

   [ui.status_line]
   type = "command"
   command = "ljos statusline"

In ``~/.claude/settings.json``:

.. code:: json

   {"statusLine": {"type": "command", "command": "ljos statusline"}}

The row reads the seat, the issue the conversation has open, and the claims due
for review. A session's line is cached
for fifteen seconds. A pack that does not answer in 300 ms shows as
``pack down``.
