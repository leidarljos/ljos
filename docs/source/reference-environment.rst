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
``LJOS_SEAT``                                               override for who is sitting. Unset, the seat is a ``seat=`` line, ``~/.config/ljos/seats.toml``, a parent seat record, or the most recent write on this machine. A harness default, such as ``claude`` or ``codex``, becomes that runner plus a short id of the session when this process has one from Claude Code, Codex or Cursor, so two sessions do not share a seat. An id that exists only on a parent does not name the seat. Search and recall still read the runner's family. The connecting client's name does not name the seat, and neither does the process tree. A read with none of those looks at every seat. A write uses the login name, or a short stable name when that login is missing, and the first write says to set it. The holder is that seat tagged with the conversation's session. When ``vissue.toml`` lists seats, a tracker write from a seat it does not list is refused. A session seat of a listed runner is that runner's seat
``VISSUE_ROOT``, ``VISSUE_AGENT``                           the tracker
``DEEDAR_URL``, ``DEEDAR_HOST_SIGNING_KEY``                 the deed store; the key signs handovers, and ``~/.config/deedar/host.key`` is used when the variable is unset
``CLAIMDAG_DIR``                                            the claim graph
``LJOS_CARDS_DIR``                                          the server's cards directory
``LJOS_MCP_TOOLS``                                          the tools ``ljos-mcp`` lists; unset, the core set (see :doc:`MCP tools <reference-mcp>`); ``all`` lists every tool; else names split on commas or spaces
``LJOS_HUD_BIN``                                            path to ``ljos-hud``; else sibling of ``ljos``, then PATH
``LJOS_HUD_SOCKET``                                         summon socket; else ``$XDG_RUNTIME_DIR/ljos/hud.sock``
``LJOS_TRACKER_GIT``                                        ``commit`` commits the ticket's ``issues.org``, and the sitting's and finish's sealed sync log, without pushing; ``off`` skips the commit and push, and the sync in passing; unset, commit and push. A landed commit or push is not printed. A refusal is one sentence. ``--verbose`` prints the lines. ``ljos sync`` by hand always pushes
``LJOS_TRACKER_PUSH_WAIT``                                  seconds the tracker push after a claim or finish waits; 5 when unset
``LJOS_TRACKER_BATCH_SECS``                                 seconds between tracker commits for notes. A plain note within that time of the checkout's last tracker commit stays on disk and on the pending list. The first note after it, any other tracker write, a push, the exit of the process that held it or the end of the session commits the held notes in one commit that names them. A claim, a ballot or a finish never waits. 60 when unset, ``0`` commits every write
``LJOS_TRACKER_SPLIT_BYTES``                                an ``issues.org`` at least this many bytes is split into one file per issue under ``issues/`` on its next tracker write, as ``vissue migrate-ledger`` does, in one commit with every issue file. That split also migrates every unsplit sibling board under the same prefix, in the same commit. A split whose commit does not land is rolled back; when another write lands in ``issues/`` first, the failed split is kept and waits for a later commit. Later writes append to that issue's file and commit it alone; ``issues.org`` stays as it was at the split. 1048576 when unset, ``0`` never splits
``LJOS_TRACKER_GC_BYTES``                                   loose object bytes past which ``ljos finish`` and ``ljos sync`` pack the tracker checkout. One ``git gc`` starts in its own session and holds the tracker commit lock and ``ljos-gc.flock`` until it ends, so a tracker write waits or the gc is skipped. The gc passes ``gc.pruneExpire=2.weeks.ago`` and ``gc.worktreePruneExpire=3.months.ago``, so a checkout config of ``now`` does not prune under it. The command returns at once, its deadline does not stop the gc, and no second gc starts while one holds the lock. 33554432 when unset, ``0`` never packs
``LJOS_PANDOC``                                          the program ``ljos note --file`` converts Markdown to org with; unset, ``pandoc`` on PATH; ``off`` uses the converter built into ljos, which a missing or failing pandoc also falls back to
``LJOS_CLAIM_NOW``                                          the instant claim ages are counted to, in RFC 3339 such as ``2026-11-16T12:00:00+00:00`` in any offset, for tests that print a claim age; unset or unparsable, the clock
``LJOS_SEARCH_BUSY_WAIT``                                   seconds ``ljos search``, ``ljos recall``, and ``ljos remember`` keep asking a pack that reports busy; 2 when unset, ``0`` does not wait. A ``Retry-After`` or a JSON ``retry_ms`` sets one pause when it is longer
``LJOS_SEARCH_WAIT``                                        seconds ``ljos search`` waits for an encoder that is still loading; 3 when unset, ``0`` does not wait. Rows shown while it loads are word matches, and the line reports the load time when known or says the encoder has not answered yet. When nothing is shown, the line names how long to wait before running the search again
``LJOS_SEARCH_WARMUP_WAIT``                                 seconds ``ljos search`` waits when the encoder is loading and the reported remaining time fits; 5 when unset, ``0`` does not wait. The wait stays inside ``LJOS_CALL_TIMEOUT``. A longer or unknown load keeps ``LJOS_SEARCH_WAIT``
``LJOS_INBOX_SYNC_WAIT``                                    seconds ``ljos inbox --sync COMMIT`` keeps fetching until the remote has the commit; 30 when unset, ``0`` fetches once
``LJOS_USER_RULES_SYNC``                                    ``off`` stops user rules from being pulled or pushed through any tracker, and ``ljos prefer --user --sync`` refuses; unset, they sync through a tracker whose ``.ljos/sync.toml`` names a scope
``LJOS_USER_RULES_TRACKER``                                 the tracker checkout user rules sync through; unset, ``repos`` in ``~/.config/ljos/sync.toml``, then ``ISSUE_ROOT`` or ``VISSUE_ROOT``, then the seat's vissue config, then the working directory when it is a tracker
``LJOS_USER_RULES_SYNC_MS``                                 milliseconds each user-rule sync may take; 4000 after a write, 1500 at session start, 8000 for ``--sync`` when unset
``LJOS_TRACK_WORK``                                         ``on`` makes the hooks ask a conversation with no issue to open one and sit; read from the environment, else ``~/.config/ljos/env``; unset, they say nothing until it sits
``LJOS_GRAPHIFY``                                           path to a graphify ``graph.json`` for ``ljos recall``; unset, ``graphify-out/graph.json`` from the working directory up to the repository root; ``off`` leaves the code context section out. A ``graphify`` key in ``ljos.toml`` is the same setting when the variable is unset
``XDG_CONFIG_HOME``                                         where ``ljos/harnesses.toml`` is read from; ``~/.config`` when unset
``LJOS_PANE_TOOL``                                          the one ``[[tool]]`` persona panes open in; unset, the first of the file's tools and the built-in herdr and tmux shapes that answers
``LJOS_PANEL_RUNNER``                                       the runner a decision panel's members run on, through its ``headless`` argv; unset, the seat's own runner, else the build harness
``LJOS_PANEL_CONCURRENCY``                                  how many decision-panel members start at once; unset, the machine's parallelism clamped to 4; ``0`` starts every member; ``LJOS_MAX_PARALLEL`` is the same knob
``XDG_STATE_HOME``, ``XDG_CACHE_HOME``, ``XDG_RUNTIME_DIR`` where the Jev state, cache and runtime key live (see :doc:`Jev settings <reference-jev>`); ``XDG_RUNTIME_DIR`` also keeps the hud socket and the seat records
=========================================================== =========================================================================================================================================================================================================================================================================================================================

``~/.config/ljos/env`` is loaded for the keys above that the shell leaves unset.

Config reference
----------------


ljos reads ``ljos.toml`` from the user config (~/.config/ljos/ljos.toml), then the seat file (~/.config/ljos/seats/$LJOS_SEAT/ljos.toml), then the project file (ljos.toml), then ``LJOS_CONFIG``, then .ljos/gates.toml. A later file overrides the keys it sets. ``graphify`` is read the other way: ``LJOS_GRAPHIFY``, then ``LJOS_CONFIG``, then the project file, then the seat file, then the user file, and the first value wins. ``[mail]`` is only the user's ljos.toml or mail.toml. ``[cloud] branches`` is only the seat file. A mistake is a warning. A hook and the MCP server print it and keep going. ``ljos doctor`` names the file or the environment variable that set each value.


ljos.toml keys
~~~~~~~~~~~~~~


- ``attribution`` ([mail]) default unset
- ``branches`` ([cloud] or the top of the file) default develop
- ``command`` ([panel]) default unset
- ``consensus_branches`` ([gates] or the top of the file) default develop, main
- ``deny_file`` ([mail]) default unset
- ``email`` ([identity]) default unset
- ``graphify`` (the top of the file) default graphify-out/graph.json
- ``harness`` ([panel]) default unset
- ``headless`` ([panel]) default unset
- ``identity`` ([mail]) default unset
- ``idle_drop`` ([mcp]) default 120s
- ``workflow.max_children`` ([workflow]) default 24
- ``panel.max_children`` ([panel]) default 8
- ``panel.max_descendants`` ([panel]) default 12
- ``workflow.max_descendants`` ([workflow]) default 48
- ``max_tokens`` ([panel] or [workflow]) default unset
- ``max_wall_secs`` ([panel] or [workflow]) default unset
- ``name`` ([identity]) default unset
- ``notmuch`` ([mail]) default unset
- ``notmuch_config`` ([mail]) default unset
- ``orphan_idle`` ([mcp]) default 6h
- ``owners`` (the top of the file) default unset
- ``policyd_config`` ([gates] or the top of the file) default unset
- ``project`` (the top of the file) default the git checkout directory name
- ``protected_branches`` ([gates] or the top of the file) default main, master, develop
- ``quorum`` ([gates] or the top of the file) default 2
- ``rewrite`` ([gates] or the top of the file) default true
- ``runner`` ([panel]) default unset
- ``skip`` ([git_hooks]) default /tmp, /var/tmp, /dev/shm
- ``template`` ([panel]) default unset


LJOS variables
~~~~~~~~~~~~~~


- ``LJOS_ASK_PACK`` default unset
- ``LJOS_ATOMS_DRIVER`` default unset
- ``LJOS_ATOM_NOW_MS`` default unset
- ``LJOS_BIN`` default unset
- ``LJOS_BIN_DIR`` default unset
- ``LJOS_CAPTURE`` default on
- ``LJOS_CARDS_DIR`` default unset
- ``LJOS_CITE`` default unset
- ``LJOS_CLAIM_NOW`` default unset, the clock
- ``LJOS_CLOUD_AGE_SECRET`` default unset
- ``LJOS_CONFIG`` default unset
- ``LJOS_DEBUG`` default unset
- ``LJOS_EXTRACT`` default unset
- ``LJOS_EXTRACT_DEBUG`` default unset
- ``LJOS_GIT_HOOK`` default unset
- ``LJOS_GIT_HOOKS`` default unset
- ``LJOS_GRAPHIFY`` default unset; then the graphify key, else graphify-out/graph.json
- ``LJOS_GUARD`` default on outside a temporary home
- ``LJOS_GUARD_FAIL_OPEN`` default unset
- ``LJOS_GUARD_TIMEOUT`` default 5 seconds
- ``LJOS_HANDOFF_DIR`` default unset; the brief rules print a placeholder
- ``LJOS_HARNESS_SESSION`` default unset
- ``LJOS_HOLD`` default unset
- ``LJOS_HOOKD`` default on
- ``LJOS_HOOK_ADMIT_MS`` default unset
- ``LJOS_HOOK_PACK_TIMEOUT_MS`` default 1500, at most 2000
- ``LJOS_HOOK_QUEUE`` default unset
- ``LJOS_HOOK_RELAY`` default unset
- ``LJOS_HOOK_SLOTS`` default unset
- ``LJOS_HOOK_TIMING`` default unset
- ``LJOS_HUD_BIN`` default unset; sibling of ljos, then PATH
- ``LJOS_HUD_LOG`` default unset; beside the summon socket
- ``LJOS_HUD_SOCKET`` default $XDG_RUNTIME_DIR/ljos/hud.sock
- ``LJOS_HUD_TRAY`` default on
- ``LJOS_INBOX_SYNC_WAIT`` default 30 seconds
- ``LJOS_INSTALL_SH`` default unset
- ``LJOS_INSTALL_URL`` default the published install script
- ``LJOS_IN_HOOK`` default unset
- ``LJOS_JUDGE`` default unset
- ``LJOS_LEASE_SECS`` default 900
- ``LJOS_MAX_PARALLEL`` default the machine's parallelism, clamped to 4
- ``LJOS_MCP_BIN`` default unset
- ``LJOS_MCP_CONFIG`` default unset
- ``LJOS_MCP_FORWARDED`` default unset
- ``LJOS_MCP_GENERATION`` default unset
- ``LJOS_MCP_IDLE`` default 30 minutes
- ``LJOS_MCP_IDLE_DROP`` default 120s
- ``LJOS_MCP_ORPHAN_IDLE`` default 6h
- ``LJOS_MCP_TIMEOUT`` default 30 seconds
- ``LJOS_MCP_TIMEOUT_MS`` default unset; LJOS_MCP_TIMEOUT applies
- ``LJOS_MCP_TOOLS`` default the core set
- ``LJOS_MEMBER_BIN`` default unset
- ``LJOS_MEMLOCK_TEST_CHILD`` default unset
- ``LJOS_MEMORY_WHY`` default unset
- ``LJOS_MEM_PORT`` default 8761
- ``LJOS_MESH_KEY_FILE`` default unset
- ``LJOS_MESH_LOGIN_SERVER`` default https://hs.grund.rgoswami.me
- ``LJOS_MESH_TUN`` default /dev/net/tun
- ``LJOS_MESH_WAIT`` default 30 seconds
- ``LJOS_OFF`` default unset
- ``LJOS_ORG_DRIVER`` default unset
- ``LJOS_PACKSETD_SIGINT_CHILD`` default unset
- ``LJOS_PACK_CONNECT_MS`` default 300, at most 1000
- ``LJOS_PACK_TIMEOUT_MS`` default 4000, at most 5000
- ``LJOS_PANDOC`` default pandoc on PATH
- ``LJOS_PANEL_BIN`` default unset
- ``LJOS_PANEL_CHILD`` default unset
- ``LJOS_PANEL_COMMAND`` default unset
- ``LJOS_PANEL_CONCURRENCY`` default the machine's parallelism, clamped to 4
- ``LJOS_PANEL_HARNESS`` default unset
- ``LJOS_PANEL_PERSONAS`` default unset
- ``LJOS_PANEL_RUNNER`` default unset
- ``LJOS_PANEL_TEMPLATE`` default unset
- ``LJOS_PANE_TOOL`` default the first tool that answers
- ``LJOS_POLICY_RECALL_WAIT`` default 3 seconds
- ``LJOS_PROCESS_NONCE`` default unset
- ``LJOS_PROC_TREE_PROBE`` default unset
- ``LJOS_PROGRAMS`` default unset
- ``LJOS_PROJECT`` default unset
- ``LJOS_RELEASE_DIR`` default unset
- ``LJOS_SEARCH_BUSY_WAIT`` default 2 seconds
- ``LJOS_SEARCH_WAIT`` default 3 seconds
- ``LJOS_SEARCH_WARMUP_WAIT`` default 5 seconds
- ``LJOS_SEAT`` default unset
- ``LJOS_SEAT_KEY`` default unset
- ``LJOS_SESSION_ID`` default unset
- ``LJOS_SHARE_FAULT`` default unset
- ``LJOS_SHARE_WAIT`` default 60 seconds
- ``LJOS_SHELL_SESSION`` default unset
- ``LJOS_SITTING_CLAIM_WAIT`` default 10 seconds
- ``LJOS_STALE_DAYS`` default 7
- ``LJOS_SYNC_HOST`` default unset
- ``LJOS_TEST_ARGV`` default unset
- ``LJOS_TEST_CANON`` default unset
- ``LJOS_TEST_CANON_TMP`` default unset
- ``LJOS_TEST_HARNESS`` default unset
- ``LJOS_TEST_HISTORY_ROOT`` default unset
- ``LJOS_TEST_HOSTNAME`` default unset
- ``LJOS_TEST_REQUIRE_TOOLS`` default unset
- ``LJOS_TEST_SESSION`` default unset
- ``LJOS_TRACKER_BATCH_SECS`` default 60 seconds
- ``LJOS_TRACKER_DIR`` default unset
- ``LJOS_TRACKER_GC_BYTES`` default 33554432 bytes
- ``LJOS_TRACKER_GIT`` default push
- ``LJOS_TRACKER_LOCK_WAIT`` default 20 seconds
- ``LJOS_TRACKER_PUSH_WAIT`` default 5 seconds
- ``LJOS_TRACKER_REMOTE`` default unset
- ``LJOS_TRACKER_SEARCH`` default unset
- ``LJOS_TRACKER_SPLIT_BYTES`` default 1048576 bytes
- ``LJOS_TRACKER_TOKEN`` default unset
- ``LJOS_TRACK_WORK`` default off
- ``LJOS_UNATTENDED`` default unset
- ``LJOS_UNSIGNED_BALLOTS`` default unset
- ``LJOS_USER_RULES_SYNC`` default on
- ``LJOS_USER_RULES_SYNC_MS`` default 4000 after a write, 1500 at session start, 8000 for --sync
- ``LJOS_USER_RULES_TRACKER`` default unset
- ``LJOS_WAKE_BIN`` default unset
- ``LJOS_WAKE_COMMIT`` default unset
- ``LJOS_WAKE_FROM`` default unset
- ``LJOS_WAKE_INTERRUPT`` default unset
- ``LJOS_WAKE_ISSUE`` default unset
- ``LJOS_WAKE_LAUNCHER_KEY`` default unset
- ``LJOS_WAKE_MSG`` default unset
- ``LJOS_WAKE_SEAT`` default unset
- ``LJOS_WORKFLOW_LOCK_WAIT`` default 30
- ``LJOS_WRITER_PIDFILE`` default unset
- ``LJOS_WRITER_STANDIN`` default unset
- ``LJOS_WRITE_SCOPE`` default unset


Packset settings ljos passes through
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~


- ``PACKSET_AUTH`` default unset
- ``PACKSET_HOME`` default $HOME/.local/share/packset
- ``PACKSET_PORT`` default 8761
- ``PACKSET_TIMEOUT_MS`` default unset; a hook uses 1500 and a command uses 4000
- ``PACKSET_TOKEN`` default unset
- ``PACKSET_TOKEN_FILE`` default unset
- ``PACKSET_URL`` default http://127.0.0.1:8761
- ``PACKSET_WORKSPACE`` default seat


Vissue settings ljos passes through
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~


- ``ISSUE_ROOT`` default unset
- ``VISSUE_AGENT`` default unset
- ``VISSUE_CONFIG`` default $XDG_CONFIG_HOME/vissue/config.toml
- ``VISSUE_NO_ROUTE`` default unset
- ``VISSUE_PREFIX`` default Software, when ljos creates the tracker
- ``VISSUE_ROOT`` default unset; then root in the vissue config, else $XDG_DATA_HOME/vissue/tracker

A temporary HOME or working directory
-------------------------------------

ljos treats a ``HOME`` under ``/tmp``, ``/var/tmp``, ``/private/tmp``, ``/dev/shm`` or an absolute per-session ``$TMPDIR`` at least two components below ``/`` and not equal to or an ancestor of the passwd home as a test sandbox.
There the hook's command check is off, and memory is off, unless ``PACKSET_URL`` or the selected writer port is nonblank.
The selected port is ``PACKSET_PORT`` when it is set, and ``LJOS_MEM_PORT`` otherwise.
With no writer address or selected port, five outputs print one line: the memory row, the guard row, ``ljos remember``, ``ljos search`` and ``ljos consensus``.
That line reads ``memory and the command guard are off because the home is under /tmp (HOME); set HOME to a directory outside /tmp``, naming whichever of those directories the home is under.
A failed first ``ljos remember`` prints no seat notice before that line.
Search and consensus exit 1 with that line.
That home does not start a pack writer: ``ljos onboard`` and ``ljos doctor --fix`` do not run ``packset ensure`` there, so a trial install does not leave a writer that nothing stops.
A working directory under one of those, with a ``HOME`` outside them, is not a sandbox: ``cd /tmp && rm -rf ~/proj`` is judged.
``LJOS_GUARD=on`` keeps the check on under a temporary ``HOME``, which is how a test suite exercises it; ``LJOS_OFF`` still switches it off.
When a writer address or a nonblank selected port is set the guard row keeps the temporary-directory reason.
``ljos doctor`` shows a ``guard`` row marked ``warn`` whenever the check is off, from ``LJOS_OFF`` or from a temporary ``HOME``.
With no writer address or selected port, ``install.sh`` prints that memory-and-guard line.
When ``PACKSET_URL`` is ``off``, the setup line says memory is off and the guard-only line follows.
When ``PACKSET_URL`` names a writer, the setup line names the memory writer and the guard-only line follows.

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
session start, the permission check, the prompt, both tool results, compaction, stop and session end
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
| ``Stop``               | speaks only when the turn ran no tool; blocks one turn that was asked for a decision and cast no ballot; with no issue held, blocks one turn that used tools and never touched the seat; with Jev on, audits the turn once from the runner's transcript; then uses one turn for unread interrupt mail (``decision: block``, or ``followup_message`` in the editor harness) and      |
|                        | receipts it                                                                                                                                                                                                                                                                                                                                                                         |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``SubagentStop``       | names the issue the subagent's conversation has claimed and asks for the ballot on a decision; with Jev on, audits the turn once                                                                                                                                                                                                                                                    |
+------------------------+-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+

No hook takes or finishes a claim, and a Write files no claim. A later
sitting hands an expired claim back. The reclaim step of ``ljos sitting``
and of ``ljos_sitting`` releases a claim-graph node quiet longer than
``LJOS_LEASE_SECS`` (900 when unset; 0 turns the step off) whose
conversation is gone, or that no conversation on this host names, and
keeps one whose runner still runs. That release does not depend on
``LJOS_TRACKER_GIT``. Only when ``LJOS_TRACKER_GIT`` is exactly ``push`` does
the step first hand back this machine's ``STARTED`` tracker claims older
than the lease whose conversation on this host is gone. Unset is not
``push``. A tracker claim that no conversation on this host names stays.
A claim the step keeps, or one not yet past the lease, stays until
``ljos finish``, ``ljos release`` or ``ljos complete`` (``ljos_finish``, ``ljos_release``,
``ljos_complete``) ends it, or another seat takes it over as stale.
On Linux, a child of the recorded client does not keep the MCP connection
alive after that client exits.

Sync files
==========

+-------------------------------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| File                                            | Contents                                                                                                                                                                                                                                                                                                                                                   |
+=================================================+============================================================================================================================================================================================================================================================================================================================================================+
| ``.ljos/sync.toml`` (in the tracker repository) | the repository's scope and its age recipients; a ``[projects]`` table sends one project's lessons to another scope                                                                                                                                                                                                                                         |
+-------------------------------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``.ljos/atoms/``                                | one sealed log per machine. ``<host>/snap-<seq>.jsonl.age`` is a snapshot sealed once, ``<host>/seg-<seq>-<hash>.jsonl.age`` is a segment sealed once, and ``<host>/plain.sha256`` is the plaintext hash. No sync reads what that hash says: a sync writes nothing when this home noted, in ``sync/logs/`` below the state directory, that the snapshots   |
|                                                 | and segments, with the bytes they have now, open to the atoms it would write. A sync never removes or seals again a snapshot or segment; when one does not open on this machine, or ``age`` cannot authenticate it, the next sync with atoms not yet sealed seals them whole into one new snapshot and prints a warning; that snapshot carries the atoms   |
|                                                 | the pack lists, the same atoms a log that opens gives a peer, and an atom the pack no longer lists is dropped as on any sync; when this machine's key is not a recipient it writes nothing and prints a warning that names ``ljos sync --key``. Only ``ljos sync --compact`` folds them into one new snapshot, and it exits non-zero and removes nothing   |
|                                                 | when the newest snapshot, or a segment after it, does not open on this machine; an older piece that does not open is covered by the newest snapshot and is removed with the rest; a sync with new atoms adds one segment of those atoms, however many segments there are. A sync keeps the plaintext of each piece it seals below the state directory, in  |
|                                                 | ``sync/opened/`` beside what an import keeps, so its next sync reads its own log without running ``age -d``. Those plaintexts are files of mode 0600 in a directory of mode 0700, as ``age.key`` is. Each kept plaintext and note is signed with a random key kept in ``sync/kept.key`` of mode 0600, and is used only when that signature and the sealed  |
|                                                 | bytes it names agree; otherwise the log is read again with ``age -d``, so removing ``sync/opened/`` always gives back the committed log. A sync commits only those files, so the temp file of a write that was cut off is never committed. A committed snapshot or segment that was changed or removed in the checkout is not committed again; only ``ljos |
|                                                 | sync --compact`` commits a removal. A new snapshot or segment takes the seq one past every piece in the checkout and in ``HEAD``, so its path is never one ``HEAD`` has. Before ``ljos sync --compact`` removes a piece, it reads its new snapshot again with ``age -d``, and removes nothing unless that snapshot is at a path ``HEAD`` does not have and |
|                                                 | opens to every atom the pack lists; when a snapshot or segment still differs from ``HEAD`` after the fold, it commits nothing and exits non-zero. A log from before this layout, ``<host>.jsonl.age`` and ``<host>.chunks/``, is copied once into ``snap-0`` and the segments, and the old path is removed                                                 |
+-------------------------------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
| ``.ljos/user-rules/``                           | one file of user rules per machine, sealed to the scope's recipients (``<host>.json.age``); a plaintext ``<host>.json`` is from before the seal                                                                                                                                                                                                            |
+-------------------------------------------------+------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
