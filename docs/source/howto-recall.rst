:description: Turn on the prompt hook, ask what a task touches, and read an issue's history as one dated list.
:cover: https://leidarljos.github.io/ljos/_static/og.png

============================
Bring memory into the prompt
============================


Inject memory before the agent acts
===================================

Add ``hooks`` to the runner's table and onboard again:

.. code:: toml

   [[harness]]
   name = "runner-with-a-command"
   register = ["runner", "mcp", "add", "-s", "user", "ljos", "--", "{server}"]
   registered = ["runner", "mcp", "get", "ljos"]
   skills = "~/.runner/skills"
   hooks = "~/.runner/settings.json"

.. code:: console

   $ ljos onboard --harness runner-with-a-command
   ok  host key    /home/you/.config/deedar/host.key exists
   ok  runner-with-a-command mcp   ljos registered
   ok  hook    memory hook: add it on UserPromptSubmit in /home/you/.runner/settings.json
   ok  skill   /home/you/.runner/skills/ljos/SKILL.md is current

From then on, before each prompt, the runner pipes it to ``ljos hook`` and
the memories it activates return as context, preferences first.
A prompt that matches nothing prints nothing.
Do not add ``PreToolUse`` as a search event. A turn issues many tool
calls and one prompt. On a tool call the hook asks ``ljos-policyd``, the
Trusted Computing Base (TCB), and then applies pack rules. A rule that fires
is one warning line the runner shows, and the command runs. The hook never
answers deny, ask or block.
If policyd is missing, slow or crashes, the command runs and ljos logs a warning.
A force push or a recursive delete outside ``/tmp`` runs, and the answer carries its rule and warning.
The bypass log keeps the label.

.. code:: console

   $ echo '{"hook_event_name":"PreToolUse","tool_input":{"command":"git push --force"}}' | ljos hook
   {"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"allow\twarn\tgit-force-push\ta force push rewrites a published branch"}}

That JSON is what the runner reads. The same warning is also written on standard error, for the person at the terminal.
The sample is from inside a repository, on a branch other than ``develop`` or ``main``.
Outside a repository the JSON also names ``sitting-gate``, because the branch cannot be read.

A warning is the only thing between a runner and a destructive command.
Keep backups, protect branches on the forge, and keep a runner where it
cannot reach what it must not touch.

**The build harness.** ``ljos onboard --harness grok`` writes
``~/.grok/hooks/ljos.json``. The hook searches on the prompt, keeps the
text, and emits it on ``PostToolUse``. The :doc:`build harness <grok-build>` page
is the install.

A policy daemon does the same with the argv:

.. code:: console

   $ echo "cargo test --release" | ljos hook
   Remembered, shown 2 of 2, oldest first. `ljos search` lists the pack.
     preference
       Build and test on the cluster through Slurm, never on the laptop.
     lesson
       The lme dense run needs 40G and sessions only; windows and sessions ran past four hours.
   $ ljos policy -- cargo test --release
   cargo test --release
   Remembered, shown 1 of 1, oldest first. `ljos search` lists the pack.

Ask what a task involves
========================

.. code:: text

   $ ljos island "write the handbook tutorial for the seat"
   1.000  seed  ab8cb4d1bfca05ca60730141343dc177  3 weeks ago  Encoding under the write lock capped the pack at
   0.899  seed  51778929d855529fcb8df3f4d1c4d692  yesterday    A new claim enters the review clock on write. Tr
   0.890  seed  27e39df0198b67538f8487b1b44e9dfd  today        The decay slot reads the review clock when PACKS

The command prints two lines before the rows: what an island is, and that
this one was not fired. The first column is activation relative to the strongest; ``seed`` is the
claim the search found, the rest were reached along the pack's links;
the age column says when each was written. Add ``--fire`` when you go on to
use the island: the strongest eight fire together and their links gain
weight.

Ask when
========

.. code:: console

   $ ljos timeline proj-1a2b
   timeline of proj-1a2b: Ship the fuse change?
   2026-08-30  13 days ago     tracker created
   2026-09-02 09:14    10 days ago +3 d    memory  [lesson] RRF lost two points to CombMNZ on turns.
   2026-09-02 11:48    10 days ago same day    tracker TODO -> STARTED
   2026-09-02 11:48    10 days ago same day    tracker claimed by you
   2026-09-05 16:02    7 days ago  +3 d    deed    deed-run-fuse-at-ten produced by you
   2026-09-12 08:30    today   +7 d    tracker note: the fused panel held at ten conversations

``proj-1a2b`` is a sample id ``ljos file`` printed. An unknown id says
``issue ID not found, so run ljos ready``.
Three stores, one dated list, oldest first: the tracker's logbook, the
deeds the issue cites with the time each was produced, and the memories the
issue's title activates with the time each was written. Each line carries
its date, its age, the gap since the line before, and the store it came
from. The reader gets time as data rather than timestamps to subtract, and a
later line supersedes an earlier one on the same matter. ``ljos sitting``
prints the last twelve as its ``timeline`` section.

``ljos search TOPIC --as-of 2026-09-01`` asks the pack as it was then:
memories withdrawn since included, memories written since left out. The
pack tombstones rather than erases, so the question has an answer.
