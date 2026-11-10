:description: The variables the seat reads, a temporary home, and the files ljos sync carries.
:cover: https://leidarljos.github.io/ljos/_static/og.png

==========================
Environment and sync files
==========================


Environment
===========

=========================================================== =========================================================================================================================================================================================================================================================================================================================
Variable                                                    Read by
=========================================================== =========================================================================================================================================================================================================================================================================================================================
``PACKSET_URL``                                             the pack client; unset, ``http://127.0.0.1:8761`` (``PACKSET_PORT`` moves the port), or this user's own port when 8761 is another user's; ``off`` means no pack
``PACKSET_WORKSPACE``                                       the pack workspace; unset, ``seat``
``LJOS_SEAT``                                               override for who is sitting. Unset, the seat is the MCP client that connected, a runner's own seat variable, or the login user. The process tree is not a seat. The holder is that seat tagged with the conversation's session. When ``vissue.toml`` lists seats, a tracker write from a seat it does not list is refused
``VISSUE_ROOT``, ``VISSUE_AGENT``                           the tracker
``DEEDAR_URL``, ``DEEDAR_HOST_SIGNING_KEY``                 the deed store; the key signs handovers, and ``~/.config/deedar/host.key`` is used when the variable is unset
``CLAIMDAG_DIR``                                            the claim graph
``LJOS_CARDS_DIR``                                          the server's cards directory
``LJOS_MCP_TOOLS``                                          the tools ``ljos-mcp`` lists; unset, the core set (see MCP tools); ``all`` lists every tool; else names split on commas or spaces
``LJOS_HUD_BIN``                                            path to ``ljos-hud``; else sibling of ``ljos``, then PATH
``LJOS_HUD_SOCKET``                                         summon socket; else ``$XDG_RUNTIME_DIR/ljos/hud.sock``
``LJOS_TRACKER_GIT``                                        ``commit`` commits the ticket's ``issues.org``, and the sitting's and finish's sealed sync log, without pushing; ``off`` skips the commit and push, and the sync in passing; unset, commit and push. ``ljos sync`` by hand always pushes
``LJOS_TRACKER_PUSH_WAIT``                                  seconds the tracker push after a claim or finish waits; 5 when unset
``LJOS_INBOX_SYNC_WAIT``                                    seconds ``ljos inbox --sync COMMIT`` keeps fetching until the remote has the commit; 30 when unset, ``0`` fetches once
``LJOS_USER_RULES_SYNC``                                    ``off`` stops user rules from being pulled or pushed through any tracker, and ``ljos prefer --user --sync`` refuses; unset, they sync through a tracker whose ``.ljos/sync.toml`` names a scope
``LJOS_USER_RULES_TRACKER``                                 the tracker checkout user rules sync through; unset, ``repos`` in ``~/.config/ljos/sync.toml``, then ``ISSUE_ROOT`` or ``VISSUE_ROOT``, then the seat's vissue config, then the working directory when it is a tracker
``LJOS_USER_RULES_SYNC_MS``                                 milliseconds each user-rule sync may take; 4000 after a write, 1500 at session start, 8000 for ``--sync`` when unset
``LJOS_TRACK_WORK``                                         ``on`` makes the hooks ask a conversation with no issue to open one and sit; read from the environment, else ``~/.config/ljos/env``; unset, they say nothing until it sits
``XDG_CONFIG_HOME``                                         where ``ljos/harnesses.toml`` is read from; ``~/.config`` when unset
``LJOS_PANE_TOOL``                                          the one ``[[tool]]`` persona panes open in; unset, the first of the file's tools and the built-in herdr and tmux shapes that answers
``LJOS_PANEL_RUNNER``                                       the runner a decision panel's members run on, through its ``headless`` argv; unset, the seat's own runner, else the build harness
``LJOS_PANEL_CONCURRENCY``                                  how many decision-panel members start at once; unset, the machine's parallelism clamped to 4; ``0`` starts every member; ``LJOS_MAX_PARALLEL`` is the same knob
``XDG_STATE_HOME``, ``XDG_CACHE_HOME``, ``XDG_RUNTIME_DIR`` where the Jev state, cache and runtime key live (see Jev settings); ``XDG_RUNTIME_DIR`` also keeps the hud socket and the seat records
=========================================================== =========================================================================================================================================================================================================================================================================================================================

``~/.config/ljos/env`` is loaded for the keys above that the shell leaves unset.

A temporary HOME or working directory
-------------------------------------

ljos treats a ``HOME`` under ``/tmp``, ``/var/tmp``, ``/private/tmp``, ``/dev/shm`` or ``$TMPDIR`` as a test sandbox. There the hook's command check is off, and the memory writer does not start on its own either. A memory command then says so and names ``packset ensure``, which starts the writer by hand. A working directory under one of those, with a ``HOME`` outside them, is not a sandbox: ``cd /tmp && rm -rf ~/proj`` is judged. ``LJOS_GUARD=on`` keeps the check on under a temporary ``HOME``, which is how a test suite exercises it; ``LJOS_OFF`` still switches it off. ``ljos doctor`` shows a ``guard`` row marked ``warn`` whenever the check is off, from ``LJOS_OFF`` or from a temporary ``HOME``, the session start line says ``command guard off`` with the reason, and ``install.sh`` says so at the end of an install into a temporary ``HOME``.

A runner's table may also name ``start`` and ``resume``, the argv that open
a session in a persona's home and resume the latest one there; a
persona whose ``--runner`` is that table's name reasons in that session
(``ljos ask``, ``vote --jev``). ``hooks_named`` names the key a hooks file of
named hooks takes the seat's hooks under, each command told its event
with ``ljos hook --event``. A steps runner reads a permission answer that omits
``decision`` as a deny, so the seat always answers ``allow``, with the rule's warning
when one matches.

The pane script resumes a runner that exits non-zero, at most three times
a minute, and leaves one that exits 0. ``agents`` names the directory the
runner reads agent definitions from: ``~/.grok/agents``, ``~/.claude/agents``
or ``~/.cursor/agents``. ``headless`` is the argv a decision panel's member
runs, with ``{prompt_file}``, ``{prompt}``, ``{cwd}`` and ``{persona}`` filled;
when it is empty, the member runs ``grok`` with the seat's fallback flags.

``hooks_format = "cursor"`` writes ``hooks`` in the editor harness's flat shape: ``version``
1, and one ``{command, timeout}`` entry an event. The seat registers the
permission check, the prompt, both tool results, compaction, stop and session end
there. Events ``~/.claude/settings.json`` already runs stay in that file.
``beforeShellExecution``, ``preToolUse``, ``beforeMCPExecution``,
``beforeReadFile`` and ``postToolUseFailure`` are still written here, since
the editor harness runs that file's hooks too and the terminal harness's file does not name these.
If one of these hooks fails, the command runs. ``ljos doctor`` counts the
hook installed when those five are present and the terminal harness's file carries
the seat hook, or when the editor harness's file carries every event.

The hook reads an editor harness call off ``cursor_version`` in its stdin and answers
in the editor harness's fields: ``permission`` on a permission check, with ``user_message`` and
``agent_message``; ``additional_context`` on a tool result; ``followup_message``
on a held stop. It answers ``allow`` on every permission check, since
the editor harness blocks the command when the answer to a permission check is not JSON. A rule
that fires is ``allow`` with its warning in ``user_message`` and ``agent_message``.
The IDE and the ``agent`` CLI read the same ``~/.cursor/mcp.json``.

A table with ``shell = true`` registers no server and no hook.
``ljos onboard --harness example-host`` writes the skill and an env file beside
``harnesses.toml``. Source that file. ``LJOS_SEAT`` is the runner's name,
because a shell-only agent has no MCP client, so the env file
names its seat. ``--skills DIR`` is where the skill goes.
``ljos hook --prompt`` reads stdin as the prompt.
``ljos hook`` and ``ljos policy`` exit 0 on a warning.
``ljos hook --fail-on-deny`` and ``ljos policy --fail-on-deny``
exit 1 when the answer is a deny, which is the cloud seat push rule.
Without the flag the exit stays 0.

``[[tool]]`` tables in the same file declare the tools a runner lives in.
Two come as shapes, herdr and tmux. A table of the same name replaces one.
Every verb but ``pane_pointer`` is an argv or a list of argvs, with
``{name}``, ``{label}``, ``{session}``, ``{home}``, ``{script}``, ``{script_q}``,
``{pane}`` and ``{line}`` filled. A refusal carries the tool's own words.

================ ====================================================================
Verb             What it does
================ ====================================================================
``detect``       exits 0 when the tool answers
``open``         ways to open the persona's pane; the first that exits 0 wins
``pane_pointer`` a JSON pointer to the pane id in ``open``'s output
``run``          types the pane script into a pane that starts as a shell
``respawn``      restarts the script in a pane whose runner exited
``prompt``       hands the runner a line
``type_line``    types the line raw when ``prompt`` is refused or absent
``alive``        exits 0 while the pane is there
``ready``        exits 0 once the runner takes input, retried for ``ready_s`` seconds
================ ====================================================================

A runner's table in ``harnesses.toml`` may name ``hooks``, a JSON settings
file of the shape
``{"hooks": {"<Event>": [{"matcher": "...", "hooks": [{"type": "command", "command": "..."}]}]}}``.
``onboard`` merges ``ljos hook`` into it on the prompt and session-end events
(matcher ``*``) by default, or on the events the table's ``hook_events`` lists
(``PreToolUse`` takes the matcher ``Bash|Edit|Write|MultiEdit|NotebookEdit|apply_patch``,
and a second group ``^mcp__.+__ljos_.+$`` for the seat's MCP tools, and a
third group for a vissue write tool),
once each, and drops it from events no longer listed. A hook that fails, times out or cannot
start lets the action run, and ljos logs a warning. On ``SessionEnd`` the
hook fires the memories it injected during the session together and clears
the session's record. The hook reads the runner's JSON on stdin
(``hook_event_name``, ``tool_input.command``, ``prompt``) and answers
``{"hookSpecificOutput": {"hookEventName": ..., "additionalContext": ...}}``,
or nothing when the pack has nothing on the cue. On ``PreToolUse`` a TCB
or pack rule is one warning line in ``additionalContext``, and the call runs.
The hook never answers ``deny``, ``ask`` or ``block``. Plain text on
stdin is an argv line and answered in plain lines.

+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| Event                  | The hook does                                                                                                                                                                                                                                                                                                                                                                       |
+========================+=====================================================================================================================================================================================================================================================================================================================================================================================+
| ``UserPromptSubmit``   | answers with the memories the prompt activates, preferences first, each once between compactions; with Jev on, asks which claims bear on the prompt and whether it corrects the agent or puts a choice                                                                                                                                                                              |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``SessionEnd``         | fires the memories injected during the session together and clears the session's record                                                                                                                                                                                                                                                                                             |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``PostToolUse``        | emits the held prompt note once, on the first tool result, for a runner that discards prompt-hook stdout (the build harness sends this event)                                                                                                                                                                                                                                       |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``PostToolUseFailure`` | searches the pack on the failed command and its error and answers with up to three standing claims that two scorers named and that share at least two content words with the cue, each once between compactions                                                                                                                                                                     |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``PreCompact``         | fires up to eight of the memories injected so far, drops all of them from the session's record so a later prompt can bring them back, and keeps a note naming the issue the conversation still has claimed, for the next delivery                                                                                                                                                   |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``SessionStart``       | after a compaction (``source`` ``compact``), on a runner that takes its context, answers with that note; a fresh session hears one line. It says that ljos is on, the memory and due counts, the policy state, and up to three preferences                                                                                                                                          |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``PreToolUse``         | applies the TCB and pack rules to the command (matcher ``Bash|Edit|Write|MultiEdit|NotebookEdit|apply_patch``); a rule is a warning and the call runs. It keeps the call's ``agent_id`` for each ``ljos`` line, and on a seat MCP tool (``^mcp__.+__ljos_.+$``) only notes a subagent's call, so the ballot binds the session plus that agent. A vissue write tool gets the same    |
|                        | tracker write check's warning, which names ``ljos finish``                                                                                                                                                                                                                                                                                                                          |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``Stop``               | speaks only when the turn ran no tool; blocks one turn that was asked for a decision and cast no ballot; with no issue held, blocks one turn that used tools and never touched the seat; with Jev on, audits the turn once from the runner's transcript; then holds one turn for unread interrupt mail (``decision: block``, or ``followup_message`` in the editor harness) and     |
|                        | receipts it                                                                                                                                                                                                                                                                                                                                                                         |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``SubagentStop``       | names the issue the subagent's conversation has claimed and asks for the ballot on a decision; with Jev on, audits the turn once                                                                                                                                                                                                                                                    |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+

The hook's book names the runner that owns its sitting, and the next
session or MCP call finishes the sitting once that process is gone. The
owner is the nearest harness ljos knows by name above the hook, else the
nearest program that is neither a shell nor a wrapper, else the outermost
shell. The walk stops below a terminal, a multiplexer or init, none of
which owns a sitting. The book keeps the owner's pid and start time, so a
pid used again by another process is gone. The walk reads ``/proc`` and
holds on Linux only. On macOS and other systems the hook's parent owns
the sitting: a runner starts the hook through ``sh -c``, which exits with
it, so there the next MCP call finishes the hook's sitting, and
``ljos_start`` files a new claim instead of keeping the one a Write took.

Sync files
==========

+-------------------------------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| File                                            | Contents                                                                                                                                                                                                                                                                                                                                                   |
+=================================================+============================================================================================================================================================================================================================================================================================================================================================+
| ``.ljos/sync.toml`` (in the tracker repository) | the repository's scope and its age recipients; a ``[projects]`` table sends one project's lessons to another scope                                                                                                                                                                                                                                         |
+-------------------------------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``.ljos/atoms/``                                | one sealed log per machine. ``<host>/snap-<seq>.jsonl.age`` is a snapshot sealed once, ``<host>/seg-<seq>-<hash>.jsonl.age`` is a segment sealed once, and ``<host>/plain.sha256`` is the plaintext hash. A sync writes nothing when that hash matches. ``ljos sync --compact``, or more than 64 segments, folds them into one new snapshot. A log from    |
|                                                 | before this layout, ``<host>.jsonl.age`` and ``<host>.chunks/``, is copied once into ``snap-0`` and the segments, and the old path is removed                                                                                                                                                                                                              |
+-------------------------------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``.ljos/user-rules/``                           | one file of user rules per machine, sealed to the scope's recipients (``<host>.json.age``); a plaintext ``<host>.json`` is from before the seal                                                                                                                                                                                                            |
+-------------------------------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
